import { DurableObject } from "cloudflare:workers";
import type { Env } from "./env";
import {
  decodeBinary,
  type Bounds,
  type Caps,
  type ConsoleToHub,
  type DeviceToHub,
  type HubToDevice,
  type HubToConsole,
  type ScoreAgg,
} from "./protocol";

type Attachment = { role: "device" | "console"; deviceId?: string };

type DeviceState = {
  deviceId: string;
  name: string;
  platform: string;
  caps: Caps;
  bounds: Bounds;
  lastSeen: number;
};

/**
 * One hub per user. Devices and consoles both open a WebSocket here, so a frame
 * travels device -> hub -> console without ever hitting D1 or R2, and an
 * injected click travels console -> hub -> device.
 *
 * The object hibernates when idle: only incoming messages wake it, so an
 * always-connected device costs nothing until it actually says something.
 */
export class RecordingHub extends DurableObject<Env> {
  private states = new Map<string, DeviceState>();

  constructor(state: DurableObjectState, env: Env) {
    super(state, env);
    // Rehydrate after eviction so a console that opens later still sees devices.
    state.blockConcurrencyWhile(async () => {
      const stored = await state.storage.list<DeviceState>({ prefix: "dev:" });
      for (const [key, value] of stored) this.states.set(key.slice(4), value);
    });
  }

  async fetch(request: Request): Promise<Response> {
    const url = new URL(request.url);

    if (url.pathname === "/ws") {
      if (request.headers.get("upgrade") !== "websocket") {
        return new Response("expected websocket", { status: 426 });
      }
      const role = (url.searchParams.get("role") ?? "console") as "device" | "console";
      const deviceId = url.searchParams.get("deviceId") ?? undefined;
      return this.acceptSocket(role, deviceId);
    }

    if (url.pathname === "/command") {
      const body = (await request.json()) as { deviceId: string; message: HubToDevice };
      const sent = this.sendToDevice(body.deviceId, body.message);
      return Response.json({ sent });
    }

    if (url.pathname === "/state") {
      return Response.json({
        devices: [...this.states.values()].map((state) => ({
          ...state,
          online: this.deviceSockets(state.deviceId).length > 0,
        })),
      });
    }

    if (url.pathname === "/console-count") {
      return Response.json({ count: this.consoleSockets().length });
    }

    return new Response("not found", { status: 404 });
  }

  private acceptSocket(role: "device" | "console", deviceId?: string): Response {
    const pair = new WebSocketPair();
    const client = pair[0];
    const server = pair[1];

    this.ctx.acceptWebSocket(server);
    server.serializeAttachment({ role, deviceId } satisfies Attachment);

    if (role === "console") {
      // Give the console the state it needs before any frame arrives.
      server.send(JSON.stringify({ t: "hub_ready" }));
      for (const state of this.states.values()) {
        server.send(JSON.stringify(deviceStateMessage(state, this.isOnline(state.deviceId))));
      }
    }

    return new Response(null, { status: 101, webSocket: client });
  }

  private isOnline(deviceId: string): boolean {
    return this.deviceSockets(deviceId).length > 0;
  }

  async webSocketMessage(ws: WebSocket, message: string | ArrayBuffer): Promise<void> {
    const attachment = (ws.deserializeAttachment() ?? { role: "console" }) as Attachment;

    if (message instanceof ArrayBuffer) {
      this.relayBinary(attachment, message);
      return;
    }

    let msg: DeviceToHub | ConsoleToHub;
    try {
      msg = JSON.parse(message);
    } catch {
      return;
    }

    if (attachment.role === "device") {
      await this.onDeviceMessage(attachment, msg as DeviceToHub);
    } else {
      await this.onConsoleMessage(ws, attachment, msg as ConsoleToHub);
    }
  }

  async webSocketClose(ws: WebSocket): Promise<void> {
    const attachment = (ws.deserializeAttachment() ?? {}) as Attachment;
    if (attachment.role === "console") return;
    const deviceId = this.deviceIdOf(ws);
    if (!deviceId) return;
    // Another socket for the same device may still be alive (reconnect races).
    if (this.deviceSockets(deviceId).length > 0) return;
    const state = this.states.get(deviceId);
    if (state) this.broadcast(deviceStateMessage(state, false));
    await this.env.DB.prepare("UPDATE devices SET status = 'offline', last_seen_at = ? WHERE id = ?")
      .bind(Date.now(), deviceId)
      .run()
      .catch(() => undefined);
  }

  async webSocketError(ws: WebSocket): Promise<void> {
    await this.webSocketClose(ws);
  }

  // ---------------------------------------------------------------- routing

  private async onDeviceMessage(attachment: Attachment, msg: DeviceToHub): Promise<void> {
    const socketDeviceId = this.deviceIdentifier(msg);
    switch (msg.t) {
      case "hello": {
        const deviceId = attachment.deviceId ?? msg.deviceId;
        if (!deviceId) return;
        const state: DeviceState = {
          deviceId,
          name: msg.name,
          platform: msg.platform,
          caps: msg.caps,
          bounds: msg.bounds,
          lastSeen: Date.now(),
        };
        this.states.set(deviceId, state);
        await this.ctx.storage.put(`dev:${deviceId}`, state);
        this.broadcast(deviceStateMessage(state, true));
        await this.env.DB.prepare(
          "UPDATE devices SET status = 'online', last_seen_at = ?, screen_w = ?, screen_h = ?, scale = ?, caps_json = ? WHERE id = ?",
        )
          .bind(Date.now(), Math.round(msg.bounds.w), Math.round(msg.bounds.h), msg.bounds.scale, JSON.stringify(msg.caps), deviceId)
          .run()
          .catch(() => undefined);
        return;
      }
      case "run_summary":
        this.broadcastMessage({ ...msg } as HubToConsole);
        await this.persistRunSummary(msg);
        return;
      default:
        void socketDeviceId;
        this.broadcastMessage(msg as unknown as HubToConsole);
    }
  }

  private async onConsoleMessage(ws: WebSocket, attachment: Attachment, msg: ConsoleToHub): Promise<void> {
    if (msg.t === "subscribe") {
      ws.serializeAttachment({ role: "console", deviceId: msg.deviceId } satisfies Attachment);
      const state = this.states.get(msg.deviceId);
      if (state) ws.send(JSON.stringify(deviceStateMessage(state, this.isOnline(msg.deviceId))));
      return;
    }
    if (msg.t === "ping") return;
    if ((msg as { deviceId?: string }).deviceId) return;

    const target = attachment.deviceId ?? this.lastKnownDevice();
    if (!target) {
      ws.send(JSON.stringify({ t: "error", code: "no_device", message: "no device selected" }));
      return;
    }
    this.sendToDevice(target, msg as unknown as HubToDevice);
  }

  private deviceIdentifier(msg: DeviceToHub): string | undefined {
    if (msg.t === "hello") return msg.deviceId;
    return undefined;
  }

  private lastKnownDevice(): string | undefined {
    return [...this.states.keys()].pop();
  }

  private relayBinary(attachment: Attachment, message: ArrayBuffer): void {
    const deviceId = attachment.deviceId ?? this.lastKnownDevice();
    if (!deviceId) return;
    let header;
    try {
      header = decodeBinary(message).header;
    } catch {
      return;
    }
    // Keep the frame's bounds authoritative: the console needs it to map clicks.
    if (header.t === "frame") {
      const state = this.states.get(deviceId);
      if (state) state.bounds = header.bounds;
    }
    for (const socket of this.consoleSockets()) {
      const sub = (socket.deserializeAttachment() ?? {}) as Attachment;
      if (sub.deviceId && sub.deviceId !== deviceId) continue;
      try {
        socket.send(message);
      } catch {
        /* socket is closing */
      }
    }
  }

  private deviceSockets(deviceId: string): WebSocket[] {
    return this.ctx.getWebSockets().filter((ws) => {
      const a = (ws.deserializeAttachment() ?? {}) as Attachment;
      return a.role === "device" && this.deviceIdOf(ws) === deviceId;
    });
  }

  private deviceIdOf(ws: WebSocket): string | undefined {
    const a = (ws.deserializeAttachment() ?? {}) as Attachment;
    if (a.role !== "device") return a.deviceId;
    return a.deviceId ?? this.lastKnownDevice();
  }

  private consoleSockets(): WebSocket[] {
    return this.ctx.getWebSockets().filter((ws) => {
      const a = (ws.deserializeAttachment() ?? {}) as Attachment;
      return a.role === "console";
    });
  }

  private sendToDevice(deviceId: string, message: HubToDevice): boolean {
    const sockets = this.deviceSockets(deviceId);
    if (sockets.length === 0) return false;
    const text = JSON.stringify(message);
    let sent = false;
    for (const socket of sockets) {
      try {
        socket.send(text);
        sent = true;
      } catch {
        /* socket is closing */
      }
    }
    return sent;
  }

  private broadcast(message: HubToConsole): void {
    this.broadcastMessage(message);
  }

  private broadcastMessage(message: HubToConsole): void {
    const text = JSON.stringify(message);
    for (const socket of this.consoleSockets()) {
      try {
        socket.send(text);
      } catch {
        /* socket is closing */
      }
    }
  }

  // ------------------------------------------------------------- durability

  private async persistRunSummary(msg: {
    runId: string;
    state: string;
    stepCount: number;
    errorCode?: string;
    errorMsg?: string;
    steps: ScoreAgg[];
  }): Promise<void> {
    const now = Date.now();
    const statements: D1PreparedStatement[] = [
      this.env.DB.prepare(
        "UPDATE runs SET state = ?, ended_at = ?, step_count = ?, error_code = ?, error_msg = ? WHERE id = ?",
      ).bind(msg.state, now, msg.stepCount, msg.errorCode ?? null, msg.errorMsg ?? null, msg.runId),
    ];
    for (const step of msg.steps) {
      statements.push(
        this.env.DB.prepare(
          `INSERT INTO scores (run_id, step_id, target_id, n, sum_score, min_score, max_ms, last_score, updated_at)
           VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
           ON CONFLICT(run_id, step_id, target_id) DO UPDATE SET
             n = excluded.n, sum_score = excluded.sum_score, min_score = excluded.min_score,
             max_ms = excluded.max_ms, last_score = excluded.last_score, updated_at = excluded.updated_at`,
        ).bind(msg.runId, step.stepId, step.targetId, step.n, step.avg * step.n, step.min, step.maxMs, step.last, now),
      );
    }
    await this.env.DB.batch(statements).catch(() => undefined);
  }
}

function deviceStateMessage(state: DeviceState, online: boolean): HubToConsole {
  return {
    t: "device_state",
    deviceId: state.deviceId,
    online,
    name: state.name,
    platform: state.platform,
    caps: state.caps,
    bounds: state.bounds,
  };
}

export type { DeviceState };
