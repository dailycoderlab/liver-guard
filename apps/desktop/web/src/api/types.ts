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
  | { kind: 'node'; flow: string; nodeId: string; state: NodeState; score?: number; point?: [number, number]; message?: string }
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

export interface Backend {
  /** `true` when talking to the real Rust core, `false` for the in-browser stand-in. */
  readonly native: boolean;

  listScripts(): Promise<ScriptSummary[]>;
  getScript(id: string): Promise<ScriptRecord>;
  createScript(name: string): Promise<ScriptRecord>;
  saveScript(id: string, ir: IrDoc): Promise<void>;
  renameScript(id: string, name: string): Promise<void>;
  deleteScript(id: string): Promise<void>;
  validate(ir: IrDoc): Promise<Problem[]>;

  listWindows(): Promise<WindowInfo[]>;
  permissions(): Promise<PermissionInfo>;
  requestPermissions(): Promise<PermissionInfo>;
  openPermissionSettings(kind: 'accessibility' | 'screen_recording'): Promise<void>;

  /** Streams frames until the returned function is called. */
  liveStart(surface: Surface, onFrame: (frame: FramePacket) => void, onError: (message: string) => void): Promise<() => Promise<void>>;
  snapshot(): Promise<FramePacket>;
  detectCandidates(seq: number, x: number, y: number): Promise<Rect[]>;
  createTemplate(seq: number, rect: Rect): Promise<Target>;
  assetPng(sha: string): Promise<Blob>;

  runStart(ir: IrDoc, surface: Surface, onMessage: (message: RunMessage) => void): Promise<string>;
  runStop(): Promise<void>;
  probe(ir: IrDoc, surface: Surface): Promise<ProbeRow[]>;
  listRuns(): Promise<RunSummary[]>;
  getRun(id: string): Promise<RunDetail>;
}
