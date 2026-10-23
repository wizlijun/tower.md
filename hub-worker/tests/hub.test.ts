import { describe, it, expect } from "vitest";
import { env, runInDurableObject, SELF } from "cloudflare:test";
import { uuidv7 } from "../src/hub";

const T0 = Date.UTC(2026, 9, 1);
const HOST = "host-a";
let projects = 0;

/** A fresh project per test: storage is shared across tests (see vitest.config.ts). */
function project() {
  const stub = env.HUB.get(env.HUB.idFromName(`p-${Date.now()}-${projects++}`));
  let id = 0;
  const call = async (op: string, body: Record<string, unknown>, now = T0) => {
    const reply = await runInDurableObject(stub, (hub) => hub.handle(JSON.stringify({ id: ++id, op, ...body }), now));
    return JSON.parse(reply) as { id: number; ok?: any; err?: any };
  };
  return { call, raw: (text: string, now = T0) => runInDurableObject(stub, (hub) => hub.handle(text, now)) };
}

const event = (type: string, extra: Record<string, unknown> = {}) => ({
  event: { eid: uuidv7(T0), host: HOST, actor: "process:towermd-cli", type, layer: "L4", obj: "task:1", guards: [], ...extra },
  data_json: "{}",
});
const acquire = (names: string[], holder = "agent-a", ttl_ms = 1000) => ({ request: { host: HOST, names, holder, ttl_ms } });
const COMMIT = "a".repeat(40);

describe("sequencing", () => {
  it("assigns contiguous sequence numbers and keeps the producer's data text", async () => {
    const { call } = project();
    const first = await call("append", { ...event("task.created"), data_json: '{"n":1.50,"z":"\\u00e9"}' });
    const second = await call("append", event("task.updated"));
    expect(first.id).toBe(1);
    expect(first.ok.Event.seq).toBe(1);
    expect(second.ok.Event.seq).toBe(2);
    expect(first.ok.Event.cost_usd).toBeNull();
    expect(first.ok.Event.t).toBe("2026-10-01T00:00:00.000Z");
    const batch = await call("subscribe", { after: 0 });
    expect(batch.ok.events.map((e: any) => e.seq)).toEqual([1, 2]);
    expect(batch.ok.head).toMatchObject({ floor_seq: 0, contiguous_seq: 2, epoch_high_water: 0, ack: null, leases: {} });
  });

  it("returns the raw data text byte for byte", async () => {
    const { raw } = project();
    const reply = await raw(JSON.stringify({ id: 1, op: "append", ...event("task.created"), data_json: '{"n":1.50}' }));
    expect(reply).toContain('"data":{"n":1.50}');
  });

  it("is idempotent by eid: the same event while retained, its sequence after compaction", async () => {
    const { call } = project();
    const item = event("task.created");
    const first = await call("append", item);
    const again = await call("append", item);
    expect(again.ok).toEqual(first.ok);
    const granted = await call("lease_acquire", acquire(["main"]));
    expect(granted.ok.leases[0].epoch).toBe(1);
    expect(await call("snapshot_ack", { ack: { commit: COMMIT, seq: 2, epoch: 1 }, checkpoint_json: "{}" })).toEqual({ id: 4, ok: null });
    expect((await call("append", item)).ok).toEqual({ Duplicate: { seq: 1 } });
    expect((await call("subscribe", { after: 2 })).ok.events).toEqual([]);
  });

  it("rejects malformed and reserved events", async () => {
    const { call } = project();
    expect((await call("append", event("task.created", { eid: "not-a-uuid" }))).err.Invalid).toBe("eid must be UUIDv7");
    const v4 = "550e8400-e29b-41d4-a716-446655440000";
    expect((await call("append", event("task.created", { eid: v4 }))).err.Invalid).toBe("eid must be UUIDv7");
    expect((await call("append", event("task.created", { eid: `git:${"0".repeat(64)}` }))).ok.Event.seq).toBe(1);
    expect((await call("append", event("lease.granted"))).err.Invalid).toBe("reserved lease event type");
    expect((await call("append", event("task.created", { actor: "someone" }))).err.Invalid).toMatch(/^actor must be/);
    expect((await call("append", event("task.created", { layer: "L9" }))).err.Invalid).toBe("unknown event layer");
    expect((await call("append", event("task created"))).err.Invalid).toBe("type must be a nonempty token");
    expect((await call("subscribe", { after: 5 })).err.Invalid).toBe("subscription cursor exceeds contiguous sequence");
    expect((await call("nope", {})).err.Invalid).toBe("unknown op nope");
  });
});

describe("leases", () => {
  it("grants a batch with rising epochs and refuses live owners", async () => {
    const { call } = project();
    const granted = await call("lease_acquire", acquire(["main", "task:1"]));
    expect(granted.ok.leases.map((l: any) => [l.name, l.epoch, l.expires_at])).toEqual([["main", 1, T0 + 1000], ["task:1", 2, T0 + 1000]]);
    expect(granted.ok.event).toMatchObject({
      seq: 1, actor: "process:towermd-hub", type: "lease.granted", layer: "L5", obj: "main", epoch: 1, data: null,
      lease_delta: { Grant: { leases: granted.ok.leases } },
    });
    expect(granted.ok.event.heartbeat).toBeUndefined();
    const conflict = await call("lease_acquire", acquire(["task:1"], "agent-b"));
    expect(conflict.err.LeaseConflict.owners).toEqual([granted.ok.leases[1]]);
    const head = (await call("subscribe", { after: 1 })).ok.head;
    expect(head.epoch_high_water).toBe(2);
    expect(Object.keys(head.leases)).toEqual(["main", "task:1"]);
  });

  it("renews as a heartbeat, releases, and fences out stale holders", async () => {
    const { call } = project();
    const [lease] = (await call("lease_acquire", acquire(["main"]))).ok.leases;
    const fence = { name: "main", holder: "agent-a", epoch: lease.epoch };
    const renewed = await call("lease_renew", { request: { host: HOST, fences: [fence], ttl_ms: 5000 } }, T0 + 500);
    expect(renewed.ok.leases[0].expires_at).toBe(T0 + 5500);
    expect(renewed.ok.event).toMatchObject({ type: "lease.renewed", heartbeat: true, lease_delta: { Renew: { leases: renewed.ok.leases } } });
    const wrong = { ...fence, epoch: 9 };
    expect((await call("lease_renew", { request: { host: HOST, fences: [wrong], ttl_ms: 1 } })).err).toEqual({ FenceExpired: { fences: [wrong] } });
    const released = await call("lease_release", { request: { host: HOST, fences: [fence] } }, T0 + 600);
    expect(released.ok.event.lease_delta).toEqual({ Release: { fences: [fence] } });
    expect(released.ok.leases).toEqual([]);
    const next = await call("lease_acquire", acquire(["main"], "agent-b"), T0 + 700);
    expect(next.ok.leases[0].epoch).toBe(2);
  });

  it("expires overdue leases when a subscriber reads, and marks events guarded by them stale", async () => {
    const { call } = project();
    const [lease] = (await call("lease_acquire", acquire(["task:1"]))).ok.leases;
    const fence = { name: "task:1", holder: "agent-a", epoch: lease.epoch };
    const fresh = await call("append", event("task.updated", { guards: [fence] }), T0 + 10);
    expect(fresh.ok.Event).toMatchObject({ epoch: 1, stale: false, guards: [fence] });
    const batch = await call("subscribe", { after: 2 }, T0 + 1000);
    expect(batch.ok.events).toHaveLength(1);
    expect(batch.ok.events[0]).toMatchObject({ type: "lease.expired", host: "local", lease_delta: { Expire: { fences: [fence] } } });
    expect(batch.ok.head.leases).toEqual({});
    const late = await call("append", event("task.updated", { guards: [fence] }), T0 + 1001);
    expect(late.ok.Event.stale).toBe(true);
  });

  it("validates lease requests", async () => {
    const { call } = project();
    expect((await call("lease_acquire", acquire([]))).err.Invalid).toBe("lease batch must not be empty");
    expect((await call("lease_acquire", acquire(["a", "a"]))).err.Invalid).toBe("duplicate lease name");
    expect((await call("lease_acquire", acquire(["a"], "agent-a", 0))).err.Invalid).toBe("lease TTL must be positive");
    expect((await call("lease_acquire", { request: { host: HOST, names: "a" } })).err.Invalid).toBe("malformed names");
  });
});

describe("pause", () => {
  it("refuses new claims and work leases while paused, but not the main lease", async () => {
    const { call } = project();
    const pause = event("project.paused", { layer: "L5", obj: "project" });
    expect((await call("append", pause)).ok.Event.seq).toBe(1);
    const pauseEid = pause.event.eid;
    expect((await call("append", event("project.paused", { layer: "L5", obj: "project" }))).err.Invalid)
      .toBe("project already paused or pause actor invalid");
    expect((await call("append", event("task.claimed"))).err.Invalid).toBe("项目已暂停，新任务领取被拒绝");
    expect((await call("lease_acquire", acquire(["task:1"]))).err.Invalid).toBe("项目已暂停，新任务领取和工作租约申请被拒绝");
    expect((await call("lease_acquire", acquire(["main"]))).ok.leases[0].name).toBe("main");
    expect((await call("subscribe", { after: 0 })).ok.head.pause_event).toBe(pauseEid);
    expect((await call("append", event("project.resumed", { layer: "L5", obj: pauseEid }))).err.Invalid)
      .toBe("resume requires human confirmation of the current pause");
    const resumed = await call("append", event("project.resumed", { layer: "L5", obj: pauseEid, actor: "human:local" }));
    expect(resumed.ok.Event.seq).toBe(3);
    expect((await call("subscribe", { after: 0 })).ok.head.pause_event).toBeUndefined();
  });
});

describe("snapshot acknowledgements", () => {
  it("compacts, then asks lagging subscribers for the snapshot", async () => {
    const { call } = project();
    await call("lease_acquire", acquire(["main"]));
    await call("append", event("task.created"));
    await call("append", event("task.updated"));
    const checkpoint = '{"tasks":{"1":{"n":1.50}}}';
    expect((await call("snapshot_ack", { ack: { commit: COMMIT, seq: 2, epoch: 1 }, checkpoint_json: checkpoint })).ok).toBeNull();
    const required = await call("subscribe", { after: 1 });
    expect(required.err.SnapshotRequired.head).toMatchObject({ floor_seq: 2, contiguous_seq: 3, ack: { commit: COMMIT, seq: 2, epoch: 1 } });
    expect(required.err.SnapshotRequired.checkpoint).toEqual({ tasks: { 1: { n: 1.5 } } });
    expect((await call("subscribe", { after: 2 })).ok.events.map((e: any) => e.seq)).toEqual([3]);
    // Re-acknowledging the same point is idempotent; a different checkpoint is not.
    expect((await call("snapshot_ack", { ack: { commit: COMMIT, seq: 2, epoch: 1 }, checkpoint_json: '{"tasks":{"1":{"n":1.5}}}' })).ok).toBeNull();
    expect((await call("snapshot_ack", { ack: { commit: COMMIT, seq: 2, epoch: 1 }, checkpoint_json: "{}" })).err.Invalid)
      .toBe("same sequence requires the same epoch and checkpoint");
    expect((await call("snapshot_ack", { ack: { commit: COMMIT, seq: 1, epoch: 1 }, checkpoint_json: "{}" })).err.Invalid)
      .toBe("snapshot acknowledgement precedes checkpoint floor");
  });

  it("bounds the acknowledged epoch by the grants up to its sequence", async () => {
    const { call } = project();
    await call("lease_acquire", acquire(["main"]));
    await call("append", event("task.created"));
    await call("lease_acquire", acquire(["task:1"]));
    const bounds = "snapshot acknowledgement exceeds recorded sequence or epoch bounds";
    expect((await call("snapshot_ack", { ack: { commit: COMMIT, seq: 2, epoch: 2 }, checkpoint_json: "{}" })).err.Invalid).toBe(bounds);
    expect((await call("snapshot_ack", { ack: { commit: COMMIT, seq: 9, epoch: 1 }, checkpoint_json: "{}" })).err.Invalid).toBe(bounds);
    expect((await call("snapshot_ack", { ack: { commit: "xyz", seq: 1, epoch: 1 }, checkpoint_json: "{}" })).err.Invalid).toBe(bounds);
    expect((await call("snapshot_ack", { ack: { commit: COMMIT, seq: 2, epoch: 1 }, checkpoint_json: "{" })).err.Invalid)
      .toBe("checkpoint_json is not JSON");
    expect((await call("snapshot_ack", { ack: { commit: "b".repeat(64), seq: 3, epoch: 2 }, checkpoint_json: "{}" })).ok).toBeNull();
  });
});

describe("worker", () => {
  async function open(path: string) {
    const response = await SELF.fetch(`https://hub.example${path}`, { headers: { upgrade: "websocket", connection: "upgrade" } });
    expect(response.status).toBe(101);
    const ws = response.webSocket!;
    ws.accept();
    const replies: string[] = [];
    ws.addEventListener("message", (e) => { replies.push(typeof e.data === "string" ? e.data : ""); });
    const send = async (text: string) => {
      const before = replies.length;
      ws.send(text);
      for (let i = 0; i < 100 && replies.length === before; i++) await new Promise((r) => setTimeout(r, 10));
      return JSON.parse(replies[replies.length - 1]);
    };
    return { ws, send };
  }

  it("serves one sequencer per project over WebSocket", async () => {
    const name = `ws-${Date.now()}`;
    const a = await open(`/projects/${encodeURIComponent(name + "/a")}`);
    const b = await open(`/projects/${encodeURIComponent(name + "/a")}`);
    const other = await open(`/projects/${name}-b`);
    const append = (id: number) => JSON.stringify({ id, op: "append", ...event("task.created", { eid: uuidv7(Date.now()) }) });
    expect((await a.send(append(7))).ok.Event.seq).toBe(1);
    const second = await b.send(append(8));
    expect(second).toMatchObject({ id: 8, ok: { Event: { seq: 2 } } });
    expect((await other.send(append(9))).ok.Event.seq).toBe(1);
    expect(await a.send("not json")).toEqual({ id: null, err: { Invalid: "request must be a JSON object" } });
    a.ws.close(); b.ws.close(); other.ws.close();
  });

  it("answers health checks and refuses plain requests", async () => {
    expect(await (await SELF.fetch("https://hub.example/health")).text()).toBe("ok");
    expect((await SELF.fetch("https://hub.example/projects/x")).status).toBe(426);
    expect((await SELF.fetch("https://hub.example/elsewhere")).status).toBe(404);
  });
});
