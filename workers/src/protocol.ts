/**
 * Wire protocol between device, console and the hub.
 *
 * Control messages are JSON text frames. Images never travel as base64: they go
 * in a binary envelope `[u32 headerLength][JSON header][payload bytes]`, so the
 * Durable Object can relay a 1 MB search window without touching the pixels.
 */

export type Role = "device" | "console";

/** Capture surface geometry, all values in logical points (Quartz space). */
export type Bounds = {
  x: number;
  y: number;
  w: number;
  h: number;
  /** pixels per logical point */
  scale: number;
  orientation?: "portrait" | "landscape";
};

export type Caps = {
  /** highest frame rate the surface can sustain */
  maxFps: number;
  injectKinds: Array<"tap" | "longPress" | "swipe" | "key" | "text">;
  /** native UI element lookup available on this platform */
  uiQuery: "ax" | "uia" | "node" | "none";
  /** largest search window the device is willing to crop, in logical points */
  maxWindow: [number, number];
  /** true when the platform may black out protected windows */
  secureScreen: boolean;
  surface: "window" | "screen" | "mirror";
};

export type DeviceHello = {
  t: "hello";
  role: "device";
  deviceId?: string;
  name: string;
  platform: string;
  caps: Caps;
  bounds: Bounds;
};

export type ConsoleHello = { t: "hello"; role: "console"; deviceId?: string };

export type InjectKind = "tap" | "longPress" | "swipe" | "key" | "text";

export type InjectMsg = {
  t: "inject";
  reqId: string;
  kind: InjectKind;
  /** normalized [0,1] coordinates relative to the captured frame content */
  at: [number, number];
  /** swipe end, normalized */
  to?: [number, number];
  frameSeq: number;
  durationMs?: number;
  /** when true the device captures a pre-action frame and crops a search window */
  record: boolean;
  key?: string;
  text?: string;
};

export type RecropMsg = {
  t: "recrop";
  reqId: string;
  /** requested window around the original click, logical points */
  window: [number, number];
};

export type StreamMsg = { t: "stream"; on: boolean; fps: number; quality: number };

export type RunMsg = {
  t: "run";
  runId: string;
  scriptId: string;
  version: number;
  params?: Record<string, unknown>;
};

export type StopMsg = { t: "stop"; runId: string };

export type DeviceToHub =
  | DeviceHello
  | { t: "inject_ack"; reqId: string; bounds: Bounds; windowRect: Bounds; axBox?: Bounds }
  | { t: "stale_frame"; reqId: string; bounds: Bounds }
  | { t: "run_state"; runId: string; state: string; step?: number; stepId?: string; errorCode?: string; errorMsg?: string }
  | { t: "score"; items: ScoreItem[] }
  | { t: "log"; items: LogItem[] }
  | { t: "shot"; runId: string; key: string }
  | RunSummary
  | { t: "error"; reqId?: string; code: string; message: string };

export type ScoreItem = {
  runId: string;
  stepId: string;
  targetId: string;
  score: number;
  strategy: string;
  ms: number;
  reused: boolean;
};

/** Per-step aggregate the client reports once a run ends. */
export type ScoreAgg = {
  stepId: string;
  targetId: string;
  n: number;
  avg: number;
  min: number;
  maxMs: number;
  last: number;
};

export type RunSummary = {
  t: "run_summary";
  runId: string;
  state: "done" | "failed" | "stopped";
  stepCount: number;
  errorCode?: string;
  errorMsg?: string;
  steps: ScoreAgg[];
};

export type LogItem = {
  runId: string;
  ts: number;
  level: "debug" | "info" | "warn" | "error";
  event: string;
  payload?: Record<string, unknown>;
};

export type HubToDevice =
  | StreamMsg
  | InjectMsg
  | RecropMsg
  | RunMsg
  | StopMsg
  | { t: "ping" };

export type ConsoleToHub =
  | ConsoleHello
  | { t: "subscribe"; deviceId: string }
  | StreamMsg
  | InjectMsg
  | RecropMsg
  | RunMsg
  | StopMsg
  | { t: "record_start"; scriptId?: string }
  | { t: "record_stop" }
  | { t: "ping" };

export type HubToConsole =
  | { t: "hub_ready" }
  | { t: "device_state"; deviceId: string; online: boolean; caps?: Caps; bounds?: Bounds; name?: string; platform?: string }
  | { t: "inject_ack"; reqId: string; bounds: Bounds; windowRect: Bounds; axBox?: Bounds }
  | { t: "stale_frame"; reqId: string; bounds: Bounds }
  | { t: "run_state"; runId: string; state: string; step?: number; stepId?: string; errorCode?: string; errorMsg?: string }
  | { t: "score"; items: ScoreItem[] }
  | { t: "log"; items: LogItem[] }
  | { t: "shot"; runId: string; key: string }
  | RunSummary
  | { t: "error"; reqId?: string; code: string; message: string };

/** Image headers carried in the binary envelope. */
export type FrameHeader = {
  t: "frame";
  seq: number;
  bounds: Bounds;
  ts: number;
  /** mime of the payload */
  mime: "image/jpeg";
};

export type ContextHeader = {
  t: "context" | "recrop_context";
  reqId: string;
  windowRect: Bounds;
  axBox?: Bounds;
  clickPx?: [number, number];
  mime: "image/png";
};

export type BinaryHeader = FrameHeader | ContextHeader;

const HEADER_LEN_BYTES = 4;

export function encodeBinary(header: BinaryHeader, payload: Uint8Array): Uint8Array {
  const json = new TextEncoder().encode(JSON.stringify(header));
  const out = new Uint8Array(HEADER_LEN_BYTES + json.byteLength + payload.byteLength);
  new DataView(out.buffer).setUint32(0, json.byteLength, false);
  out.set(json, HEADER_LEN_BYTES);
  out.set(payload, HEADER_LEN_BYTES + json.byteLength);
  return out;
}

export function decodeBinary(buf: ArrayBuffer): { header: BinaryHeader; payload: Uint8Array } {
  const view = new DataView(buf);
  const headerLen = view.getUint32(0, false);
  const bytes = new Uint8Array(buf);
  const header = JSON.parse(new TextDecoder().decode(bytes.subarray(HEADER_LEN_BYTES, HEADER_LEN_BYTES + headerLen)));
  return { header, payload: bytes.subarray(HEADER_LEN_BYTES + headerLen) };
}

export function isBinaryMessage(data: unknown): data is ArrayBuffer {
  return data instanceof ArrayBuffer;
}
