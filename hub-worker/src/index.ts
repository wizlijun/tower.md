// Optional self-deployed Tower.md hub. The Worker routes
// `GET /projects/<project_id>` WebSocket upgrades to that project's Durable
// Object; the object answers each request message with exactly one reply.
// No authentication or encryption yet: deploy only for trusted networks.
import { DurableObject } from "cloudflare:workers";
import { ProjectHubCore } from "./hub";

export interface Env {
  HUB: DurableObjectNamespace<ProjectHub>;
}

export class ProjectHub extends DurableObject<Env> {
  private core: ProjectHubCore;

  constructor(ctx: DurableObjectState, env: Env) {
    super(ctx, env);
    this.core = new ProjectHubCore(ctx.storage.sql, (run) => ctx.storage.transactionSync(run));
  }

  /** One protocol request; exposed for tests that need a fixed clock. */
  handle(text: string, now: number = Date.now()): string {
    return this.core.handle(text, now);
  }

  async fetch(request: Request): Promise<Response> {
    if (request.headers.get("Upgrade") !== "websocket") {
      return new Response("expected a WebSocket upgrade", { status: 426 });
    }
    const pair = new WebSocketPair();
    this.ctx.acceptWebSocket(pair[1]);
    return new Response(null, { status: 101, webSocket: pair[0] });
  }

  async webSocketMessage(ws: WebSocket, message: string | ArrayBuffer) {
    const text = typeof message === "string" ? message : new TextDecoder().decode(message);
    ws.send(this.handle(text));
  }

  async webSocketClose(ws: WebSocket, code: number) {
    try { ws.close(code === 1005 ? 1000 : code, "closing"); } catch { /* already closed */ }
  }
}

export default {
  async fetch(request: Request, env: Env): Promise<Response> {
    const url = new URL(request.url);
    if (url.pathname === "/health") return new Response("ok");
    const match = /^\/projects\/([^/]+)$/.exec(url.pathname);
    if (!match) return new Response("not found", { status: 404 });
    let project: string;
    try { project = decodeURIComponent(match[1]); } catch { return new Response("bad project id", { status: 400 }); }
    if (project === "") return new Response("bad project id", { status: 400 });
    return env.HUB.get(env.HUB.idFromName(project)).fetch(request);
  },
} satisfies ExportedHandler<Env>;
