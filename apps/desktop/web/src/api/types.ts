// The contract between the UI and the Rust side (apps/desktop/src-tauri/src/commands.rs).
// Commands are hand-typed; the IR itself is typed from the Rust JSON Schema.

import type { IrDoc, Problem, Target } from '@/ir/model';

export interface Rect {
  x: number;
  y: number;
  w: number;
  h: number;
}

export interface Bounds extends Rect {
  scale: number;
}

export interface ScriptSummary {
  id: string;
  name: string;
}

export interface ScriptRecord {
  id: string;
  name: string;
  ir: IrDoc;
}

export interface WindowInfo {
  /** Window id: CGWindowNumber on macOS, HWND on Windows. */
  number: number;
  owner: string;
  title: string;
  bounds: Rect;
  label: string;
}

/** What a script drives: the whole display or a single window. */
export type Surface = { kind: 'fullScreen' } | { kind: 'window'; number: number };

export interface PermissionInfo {
  platform: 'macos' | 'windows' | 'other';
  /** macOS only. */
  accessibility: boolean;
  /** macOS only. */
  screenRecording: boolean;
  /** Who to tick in System Settings. */
  owner: string;
  problems: string[];
  ok: boolean;
}

export interface FramePacket {
  seq: number;
  bounds: Bounds;
  /** JPEG for the live stream, PNG for a snapshot. */
  image: Blob;
}

export interface ProbeRow {
  target: string;
  found: boolean;
  ok: boolean;
  score: number | null;
  threshold: number | null;
  rect: Rect | null;
  point: [number, number] | null;
  strategy: string | null;
  elapsedMs: number;
}

export type NodeState = 'running' | 'ok' | 'failed';

export interface RunResult {
  state: 'done' | 'failed' | 'stopped';
  stepCount: number;
  errorCode: string | null;
  errorMsg: string | null;
  errorNode: string | null;
  startedAt: number;
  endedAt: number;
}

export type RunMessage =
  | { kind: 'log'; level: string; event: string; payload: unknown }
  | { kind: 'node'; nodeId: string; state: NodeState; score?: number; point?: [number, number]; message?: string }
  | { kind: 'done'; runId: string; result: RunResult };

export interface RunSummary extends Partial<RunResult> {
  id: string;
  script?: string;
}

export interface RunDetail {
  id: string;
  record: RunSummary & { summary?: unknown };
  logs: string;
}

export type NotifyChannel = 'dingtalk' | 'wecom' | 'feishu' | 'generic';

/** Failure alert settings; they live in the app's settings file, not in any script. */
export interface NotifyConfig {
  enabled: boolean;
  channel: NotifyChannel;
  url: string;
  /** Signing secret (钉钉加签 / 飞书签名校验); empty when the robot has none. */
  secret: string;
  /** The robot's keyword rule; prepended to the message when missing. */
  keyword: string;
  /** Also notify when a looping run finishes every round successfully. */
  onLoopDone: boolean;
}

export interface Backend {
  /** `true` when talking to the real Rust core, `false` for the in-browser stand-in. */
  readonly native: boolean;

  listScripts(): Promise<ScriptSummary[]>;
  getScript(id: string): Promise<ScriptRecord>;
  createScript(name: string): Promise<ScriptRecord>;
  saveScript(id: string, ir: IrDoc): Promise<void>;
  renameScript(id: string, name: string): Promise<void>;
  /** Copy the saved version of a script under a new name. */
  duplicateScript(id: string, name: string): Promise<ScriptRecord>;
  /** Save a script and its template images to a file; resolves where it went, or null if cancelled. */
  exportScript(id: string): Promise<string | null>;
  /** Pick an exported file and add it as a new script; null if cancelled. */
  importScript(): Promise<ScriptRecord | null>;
  deleteScript(id: string): Promise<void>;

  getNotifyConfig(): Promise<NotifyConfig>;
  saveNotifyConfig(config: NotifyConfig): Promise<void>;
  /** Send a test message with this (possibly unsaved) config; rejects with the reason. */
  sendTestNotification(config: NotifyConfig): Promise<void>;
  validate(ir: IrDoc): Promise<Problem[]>;

  listWindows(): Promise<WindowInfo[]>;
  permissions(): Promise<PermissionInfo>;
  requestPermissions(): Promise<PermissionInfo>;
  openPermissionSettings(kind: 'accessibility' | 'screen_recording'): Promise<void>;

  /** Streams frames until the returned function is called. */
  liveStart(surface: Surface, onFrame: (frame: FramePacket) => void, onError: (message: string) => void): Promise<() => Promise<void>>;
  snapshot(): Promise<FramePacket>;
  /** Pins a user-supplied screenshot in place of a capture, so it can be boxed like a snapshot. */
  loadImage(image: Blob): Promise<FramePacket>;
  detectCandidates(seq: number, x: number, y: number): Promise<Rect[]>;
  createTemplate(seq: number, rect: Rect): Promise<Target>;
  assetPng(sha: string): Promise<Blob>;

  /** `repeat` is how many times the whole graph is walked in this one run (at least 1). */
  runStart(ir: IrDoc, surface: Surface, repeat: number, onMessage: (message: RunMessage) => void): Promise<string>;
  runStop(): Promise<void>;
  probe(ir: IrDoc, surface: Surface): Promise<ProbeRow[]>;
  listRuns(): Promise<RunSummary[]>;
  getRun(id: string): Promise<RunDetail>;
}
