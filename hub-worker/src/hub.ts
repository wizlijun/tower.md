// The project hub protocol, ported from crates/towermd-core/src/hub.rs.
//
// One Durable Object per project is the only sequencer. Every request runs
// synchronously inside one SQLite transaction, so requests are totally ordered
// and a failure leaves no partial write. Event `data` and snapshot checkpoints
// are stored and returned as the producer's JSON text, never re-encoded.
//
// Replies use the Rust serde shapes of `HubProtocol` results and `HubError`,
// so the daemon decodes them with the same types as the local hub.

export interface Fence { name: string; holder: string; epoch: number }
export interface Lease { name: string; holder: string; epoch: number; expires_at: number }
export interface SnapshotAck { commit: string; seq: number; epoch: number }

interface State {
  floor_seq: number;
  /** Highest grant epoch at `floor_seq`, for acknowledgements below the head. */
  floor_epoch: number;
  last_seq: number;
  epoch: number;
  leases: Record<string, Lease>;
  ack: SnapshotAck | null;
  pause_event: string | null;
}

type Delta =
  | { Grant: { leases: Lease[] } }
  | { Renew: { leases: Lease[] } }
  | { Release: { fences: Fence[] } }
  | { Expire: { fences: Fence[] } };

interface Input {
  eid: string; host: string; actor: string; type: string; layer: string; obj: string;
  guards: Fence[];
  /** Raw JSON text. */
  data: string;
}

/** A protocol failure carrying the serialized `HubError`. */
export class HubFailure extends Error {
  constructor(readonly json: string) { super(json); }
}
const fail = (variant: string, body: unknown): never => {
  throw new HubFailure(JSON.stringify({ [variant]: body }));
};
const invalid = (message: string): never => fail("Invalid", message);

const MAX_TIME = 253_402_300_799_999;
const HUB_ACTOR = "process:towermd-hub";

export class ProjectHubCore {
  constructor(private sql: SqlStorage, private transaction: <T>(run: () => T) => T) {
    sql.exec(`CREATE TABLE IF NOT EXISTS events (
      seq INTEGER PRIMARY KEY, eid TEXT NOT NULL UNIQUE,
      heartbeat INTEGER NOT NULL, grant_epoch INTEGER NOT NULL, json TEXT NOT NULL)`);
    sql.exec("CREATE TABLE IF NOT EXISTS receipts (eid TEXT PRIMARY KEY, seq INTEGER NOT NULL)");
    sql.exec("CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL)");
  }

  /** Handles one wire request; always returns one reply text. */
  handle(text: string, now: number): string {
    let request: Record<string, unknown>;
    try {
      request = JSON.parse(text);
      if (typeof request !== "object" || request === null || Array.isArray(request)) throw 0;
    } catch {
      return JSON.stringify({ id: null, err: { Invalid: "request must be a JSON object" } });
    }
    const id = JSON.stringify(request.id ?? null);
    try {
      const ok = this.transaction(() => this.dispatch(request, now));
      return `{"id":${id},"ok":${ok}}`;
    } catch (error) {
      const err = error instanceof HubFailure ? error.json : JSON.stringify({ Io: String(error) });
      return `{"id":${id},"err":${err}}`;
    }
  }

  private dispatch(r: Record<string, unknown>, now: number): string {
    switch (r.op) {
      case "append": return this.append(input(r.event, r.data_json), now);
      case "subscribe": return this.subscribe(count(r.after, "after"), now);
      case "lease_acquire": {
        const q = object(r.request);
        return this.acquire(text(q.host, "host"), strings(q.names), text(q.holder, "holder"), count(q.ttl_ms, "ttl_ms"), now);
      }
      case "lease_renew": {
        const q = object(r.request);
        return this.renew(text(q.host, "host"), fences(q.fences), count(q.ttl_ms, "ttl_ms"), now);
      }
      case "lease_release": {
        const q = object(r.request);
        return this.release(text(q.host, "host"), fences(q.fences), now);
      }
      case "snapshot_ack": return this.snapshotAck(ack(r.ack), json(r.checkpoint_json, "checkpoint_json"));
      case "rebuild": return invalid("the remote hub is not rebuilt from a host");
      default: return invalid(`unknown op ${String(r.op)}`);
    }
  }

  // --- state -------------------------------------------------------------

  private state(): State {
    const row = this.sql.exec<{ value: string }>("SELECT value FROM meta WHERE key = 'state'").toArray()[0];
    return row ? JSON.parse(row.value) : {
      floor_seq: 0, floor_epoch: 0, last_seq: 0, epoch: 0, leases: {}, ack: null, pause_event: null,
    };
  }
  private save(state: State) {
    this.sql.exec("INSERT OR REPLACE INTO meta (key, value) VALUES ('state', ?)", JSON.stringify(state));
  }
  private checkpoint(): string {
    const row = this.sql.exec<{ value: string }>("SELECT value FROM meta WHERE key = 'checkpoint'").toArray()[0];
    return row ? row.value : "{}";
  }
  private head(state: State): string {
    const leases: Record<string, Lease> = {};
    for (const name of Object.keys(state.leases).sort()) leases[name] = state.leases[name];
    const head: Record<string, unknown> = {
      floor_seq: state.floor_seq, contiguous_seq: state.last_seq, epoch_high_water: state.epoch,
      ack: state.ack, leases,
    };
    if (state.pause_event !== null) head.pause_event = state.pause_event;
    return JSON.stringify(head);
  }
  private active(state: State, fence: Fence, now: number): boolean {
    const lease = state.leases[fence.name];
    return lease !== undefined && sameFence(lease, fence) && lease.expires_at > now;
  }
  private validateFences(state: State, list: Fence[], now: number) {
    validateNames(list.map((f) => f.name));
    list.forEach(validateFence);
    const expired = list.filter((f) => !this.active(state, f, now));
    if (expired.length > 0) fail("FenceExpired", { fences: expired });
  }

  // --- events ------------------------------------------------------------

  private build(state: State, item: Input, now: number, delta: Delta | null): { json: string; stale: boolean; epoch: number | null; heartbeat: boolean } {
    validateInput(item);
    let epoch: number | null = item.guards[0]?.epoch ?? null;
    const stale = item.guards.some((f) => !this.active(state, f, now));
    let heartbeat = false;
    if (delta) {
      const [variant, body] = Object.entries(delta)[0] as [string, { leases?: Lease[]; fences?: Fence[] }];
      epoch = (body.leases ?? body.fences)![0].epoch;
      heartbeat = variant === "Renew";
    }
    const seq = state.last_seq + 1;
    let json = `{"seq":${seq},"eid":${q(item.eid)},"host":${q(item.host)},"t":${q(timestamp(now))},`
      + `"actor":${q(item.actor)},"type":${q(item.type)},"layer":${q(item.layer)},"obj":${q(item.obj)},`
      + `"data":${item.data},"epoch":${JSON.stringify(epoch)},"stale":${stale},"cost_usd":null,`
      + `"guards":${JSON.stringify(item.guards.map(fenceOf))}`;
    if (delta) json += `,"lease_delta":${JSON.stringify(delta)}`;
    if (heartbeat) json += `,"heartbeat":true`;
    return { json: json + "}", stale, epoch, heartbeat };
  }

  private commit(state: State, eid: string, type: string, built: { json: string; stale: boolean; heartbeat: boolean }, delta: Delta | null): string {
    const seq = state.last_seq + 1;
    let grantEpoch = 0;
    if (!built.stale) {
      if (type === "project.paused") state.pause_event = eid;
      if (type === "project.resumed") state.pause_event = null;
    }
    if (delta && ("Grant" in delta || "Renew" in delta)) {
      const leases = "Grant" in delta ? delta.Grant.leases : delta.Renew.leases;
      for (const lease of leases) {
        state.leases[lease.name] = lease;
        state.epoch = Math.max(state.epoch, lease.epoch);
        if ("Grant" in delta) grantEpoch = Math.max(grantEpoch, lease.epoch);
      }
    } else if (delta) {
      const list = "Release" in delta ? delta.Release.fences : delta.Expire.fences;
      for (const fence of list) {
        const lease = state.leases[fence.name];
        if (lease && sameFence(lease, fence)) delete state.leases[fence.name];
      }
    }
    state.last_seq = seq;
    this.sql.exec("INSERT INTO events (seq, eid, heartbeat, grant_epoch, json) VALUES (?, ?, ?, ?, ?)",
      seq, eid, built.heartbeat ? 1 : 0, grantEpoch, built.json);
    this.save(state);
    return built.json;
  }

  private system(state: State, host: string, type: string, delta: Delta, now: number): string {
    const body = Object.values(delta)[0] as { leases?: Lease[]; fences?: Fence[] };
    const item: Input = {
      eid: uuidv7(now), host, actor: HUB_ACTOR, type, layer: "L5",
      obj: (body.leases ?? body.fences)![0].name, guards: [], data: "null",
    };
    const built = this.build(state, item, now, delta);
    const event = this.commit(state, item.eid, type, built, delta);
    return `{"event":${event},"leases":${JSON.stringify(body.leases ?? [])}}`;
  }

  private expire(state: State, now: number) {
    timestamp(now);
    const due = Object.values(state.leases)
      .filter((lease) => lease.expires_at <= now)
      .sort((a, b) => a.expires_at - b.expires_at || (a.name < b.name ? -1 : a.name > b.name ? 1 : 0) || a.epoch - b.epoch);
    if (due.length > 0) this.system(state, "local", "lease.expired", { Expire: { fences: due.map(fenceOf) } }, now);
  }

  private eventsAfter(after: number): string[] {
    return this.sql.exec<{ json: string }>("SELECT json FROM events WHERE seq > ? ORDER BY seq", after)
      .toArray().map((row) => row.json);
  }

  // --- operations ----------------------------------------------------------

  private append(item: Input, now: number): string {
    const state = this.state();
    const retained = this.sql.exec<{ json: string }>("SELECT json FROM events WHERE eid = ?", item.eid).toArray()[0];
    if (retained) return `{"Event":${retained.json}}`;
    const receipt = this.sql.exec<{ seq: number }>("SELECT seq FROM receipts WHERE eid = ?", item.eid).toArray()[0];
    if (receipt) return JSON.stringify({ Duplicate: { seq: receipt.seq } });
    validateInput(item);
    if (item.type.startsWith("lease.")) invalid("reserved lease event type");
    if (state.pause_event !== null && item.type === "task.claimed") invalid("项目已暂停，新任务领取被拒绝");
    const built = this.build(state, item, now, null);
    validateControl(state.pause_event, item, built.stale);
    return `{"Event":${this.commit(state, item.eid, item.type, built, null)}}`;
  }

  private subscribe(after: number, now: number): string {
    const state = this.state();
    if (after < state.floor_seq) {
      return fail("SnapshotRequired", { head: JSON.parse(this.head(state)), checkpoint: JSON.parse(this.checkpoint()) });
    }
    if (after > state.last_seq) invalid("subscription cursor exceeds contiguous sequence");
    this.expire(state, now);
    return `{"events":[${this.eventsAfter(after).join(",")}],"head":${this.head(state)}}`;
  }

  private acquire(host: string, names: string[], holder: string, ttl: number, now: number): string {
    const state = this.state();
    validateToken("host", host);
    validateNames(names);
    if (state.pause_event !== null && names.some(isWorkLease)) invalid("项目已暂停，新任务领取和工作租约申请被拒绝");
    validateToken("lease holder", holder);
    const expiry = expiration(now, ttl);
    const owners = names.map((n) => state.leases[n]).filter((l) => l !== undefined && l.expires_at > now);
    if (owners.length > 0) fail("LeaseConflict", { owners });
    let epoch = state.epoch;
    const leases = names.map((name) => ({ name, holder, epoch: ++epoch, expires_at: expiry }));
    return this.system(state, host, "lease.granted", { Grant: { leases } }, now);
  }

  private renew(host: string, list: Fence[], ttl: number, now: number): string {
    const state = this.state();
    validateToken("host", host);
    this.validateFences(state, list, now);
    const expiry = expiration(now, ttl);
    const leases = list.map((f) => ({ name: f.name, holder: f.holder, epoch: f.epoch, expires_at: expiry }));
    return this.system(state, host, "lease.renewed", { Renew: { leases } }, now);
  }

  private release(host: string, list: Fence[], now: number): string {
    const state = this.state();
    validateToken("host", host);
    this.validateFences(state, list, now);
    return this.system(state, host, "lease.released", { Release: { fences: list.map(fenceOf) } }, now);
  }

  private snapshotAck(ack: SnapshotAck, checkpoint: string): string {
    const state = this.state();
    validateAck(ack, state.last_seq, state.epoch);
    if (ack.seq < state.floor_seq) invalid("snapshot acknowledgement precedes checkpoint floor");
    const previous = state.ack;
    if (previous) {
      if (ack.seq < previous.seq) invalid("snapshot acknowledgement cannot move sequence backwards");
      if (ack.seq === previous.seq && state.floor_seq === ack.seq) {
        if (ack.epoch !== previous.epoch || canonical(JSON.parse(checkpoint)) !== canonical(JSON.parse(this.checkpoint()))) {
          invalid("same sequence requires the same epoch and checkpoint");
        }
        if (ack.commit === previous.commit) return "null";
      }
    }
    const grants = this.sql.exec<{ epoch: number | null }>(
      "SELECT MAX(grant_epoch) AS epoch FROM events WHERE seq <= ?", ack.seq).toArray()[0];
    const epochAtAck = Math.max(state.floor_epoch, grants?.epoch ?? 0);
    validateAck(ack, ack.seq, epochAtAck);
    // Compact: keep non-heartbeat receipts so a late resend stays idempotent.
    this.sql.exec("INSERT OR REPLACE INTO receipts (eid, seq) SELECT eid, seq FROM events WHERE seq <= ? AND heartbeat = 0", ack.seq);
    this.sql.exec("DELETE FROM events WHERE seq <= ?", ack.seq);
    this.sql.exec("INSERT OR REPLACE INTO meta (key, value) VALUES ('checkpoint', ?)", checkpoint);
    state.floor_seq = ack.seq;
    state.floor_epoch = epochAtAck;
    state.ack = { commit: ack.commit, seq: ack.seq, epoch: ack.epoch };
    this.save(state);
    return "null";
  }
}

// --- validation (mirrors hub.rs) -------------------------------------------

function validateInput(item: Input) {
  validateEid(item.eid);
  validateToken("host", item.host);
  validateActor(item.actor);
  validateToken("type", item.type);
  validateToken("object", item.obj);
  if (!/^L[0-5]$/.test(item.layer.trim())) invalid("unknown event layer");
  if (item.guards.length > 0) {
    validateNames(item.guards.map((f) => f.name));
    item.guards.forEach(validateFence);
  }
}

function validateControl(current: string | null, item: Input, stale: boolean) {
  if (!["project.paused", "project.resume.requested", "project.resumed"].includes(item.type)) return;
  if (item.layer !== "L5" || item.guards.length > 0 || stale) invalid("invalid project control envelope");
  if (item.type === "project.paused") {
    if (current !== null || !["human:local", "process:towermd-cli"].includes(item.actor)) {
      invalid("project already paused or pause actor invalid");
    }
  } else if (item.type === "project.resume.requested") {
    if (item.actor !== "process:towermd-cli" || current !== item.obj) invalid("resume request must name the current pause");
  } else if (item.actor !== "human:local" || current !== item.obj) {
    invalid("resume requires human confirmation of the current pause");
  }
}

export function validateEid(eid: string) {
  if (eid.startsWith("git:")) {
    if (!/^[0-9a-fA-F]{64}$/.test(eid.slice(4))) invalid("invalid git eid token");
    return;
  }
  // Same accepted spellings as Rust's `Uuid::parse_str`.
  let body = eid;
  if (/^urn:uuid:/i.test(body)) body = body.slice(9);
  else if (body.startsWith("{") && body.endsWith("}")) body = body.slice(1, -1);
  let hex = "";
  if (/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i.test(body)) hex = body.replace(/-/g, "");
  else if (/^[0-9a-f]{32}$/i.test(body)) hex = body;
  if (hex === "" || hex[12] !== "7" || !"89abAB".includes(hex[16])) invalid("eid must be UUIDv7");
}

function validateToken(label: string, value: string) {
  if (value === "" || /[\p{White_Space}\p{Cc}]/u.test(value)) invalid(`${label} must be a nonempty token`);
}

function validateActor(actor: string) {
  validateToken("actor", actor);
  const part = (s: string) => s !== "" && !s.includes(":") && !s.includes("/");
  let valid = false;
  if (actor.startsWith("human:")) valid = part(actor.slice(6));
  else if (actor.startsWith("process:")) valid = part(actor.slice(8));
  else {
    const at = actor.indexOf("/");
    valid = at >= 0 && part(actor.slice(0, at)) && part(actor.slice(at + 1));
  }
  if (!valid) invalid("actor must be human:<id>, process:<id>, or <producer>/<version>");
}

function validateFence(f: Fence) {
  validateToken("lease name", f.name);
  validateToken("lease holder", f.holder);
  if (f.epoch === 0) invalid("fence epoch must be positive");
}

function validateNames(names: string[]) {
  const seen = new Set<string>();
  for (const name of names) {
    validateToken("lease name", name);
    if (seen.has(name)) invalid("duplicate lease name");
    seen.add(name);
  }
  if (seen.size === 0) invalid("lease batch must not be empty");
}

function validateAck(ack: SnapshotAck, last: number, epoch: number) {
  if (![40, 64].includes(ack.commit.length) || !/^[0-9a-fA-F]+$/.test(ack.commit)
    || ack.seq > last || ack.epoch === 0 || ack.epoch > epoch) {
    invalid("snapshot acknowledgement exceeds recorded sequence or epoch bounds");
  }
}

/** Work leases are named `<kind>:<id>`; `main` and `maintenance` are not. */
function isWorkLease(name: string): boolean {
  const at = name.indexOf(":");
  return at > 0 && at < name.length - 1;
}

function timestamp(now: number): string {
  if (now > MAX_TIME) invalid("time is outside RFC3339 range");
  return new Date(now).toISOString();
}

function expiration(now: number, ttl: number): number {
  if (ttl === 0) invalid("lease TTL must be positive");
  const end = now + ttl;
  timestamp(end);
  return end;
}

// --- request decoding: what serde would reject, this rejects -----------------

function object(value: unknown): Record<string, unknown> {
  if (typeof value !== "object" || value === null || Array.isArray(value)) return invalid("malformed request");
  return value as Record<string, unknown>;
}
function text(value: unknown, label: string): string {
  return typeof value === "string" ? value : invalid(`malformed ${label}`);
}
function count(value: unknown, label: string): number {
  return Number.isSafeInteger(value) && (value as number) >= 0 ? value as number : invalid(`malformed ${label}`);
}
function strings(value: unknown): string[] {
  if (!Array.isArray(value)) return invalid("malformed names");
  return value.map((item) => text(item, "name"));
}
function fences(value: unknown): Fence[] {
  if (!Array.isArray(value)) return invalid("malformed fences");
  return value.map((item) => {
    const f = object(item);
    return { name: text(f.name, "fence"), holder: text(f.holder, "fence"), epoch: count(f.epoch, "fence") };
  });
}
function ack(value: unknown): SnapshotAck {
  const a = object(value);
  return { commit: text(a.commit, "ack"), seq: count(a.seq, "ack"), epoch: count(a.epoch, "ack") };
}
function json(value: unknown, label: string): string {
  const raw = text(value, label);
  try { JSON.parse(raw); } catch { invalid(`${label} is not JSON`); }
  return raw;
}
function input(event: unknown, data: unknown): Input {
  const e = object(event);
  return {
    eid: text(e.eid, "eid"), host: text(e.host, "host"), actor: text(e.actor, "actor"),
    type: text(e.type, "type"), layer: text(e.layer, "layer"), obj: text(e.obj, "obj"),
    guards: fences(e.guards), data: json(data, "data_json"),
  };
}

// --- helpers ----------------------------------------------------------------

const q = (value: string) => JSON.stringify(value);
const fenceOf = (f: Fence): Fence => ({ name: f.name, holder: f.holder, epoch: f.epoch });
const sameFence = (lease: Fence, fence: Fence) =>
  lease.name === fence.name && lease.holder === fence.holder && lease.epoch === fence.epoch;

function canonical(value: unknown): string {
  if (Array.isArray(value)) return `[${value.map(canonical).join(",")}]`;
  if (typeof value === "object" && value !== null) {
    const entries = Object.keys(value).sort().map((k) => `${q(k)}:${canonical((value as Record<string, unknown>)[k])}`);
    return `{${entries.join(",")}}`;
  }
  return JSON.stringify(value);
}

export function uuidv7(now: number): string {
  const bytes = new Uint8Array(16);
  crypto.getRandomValues(bytes);
  let time = now;
  for (let i = 5; i >= 0; i--) { bytes[i] = time % 256; time = Math.floor(time / 256); }
  bytes[6] = (bytes[6] & 0x0f) | 0x70;
  bytes[8] = (bytes[8] & 0x3f) | 0x80;
  const hex = Array.from(bytes, (b) => b.toString(16).padStart(2, "0")).join("");
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`;
}
