import { Channel, invoke } from '@tauri-apps/api/core';
import type { IrDoc, Problem, Target } from '@/ir/model';
import type {
  Backend,
  Bounds,
  FramePacket,
  NotifyConfig,
  PermissionInfo,
  ProbeRow,
  Rect,
  RunDetail,
  RunMessage,
  RunSummary,
  ScriptRecord,
  ScriptSummary,
  Surface,
  WindowInfo,
} from './types';

/** `seq u64 | x y w h scale f64 | image bytes`, all little endian. */
export const FRAME_HEADER_BYTES = 48;

export function parseFrame(buffer: ArrayBuffer, mime: string): FramePacket {
  if (buffer.byteLength <= FRAME_HEADER_BYTES) throw new Error('truncated frame');
  const view = new DataView(buffer);
  const bounds: Bounds = {
    x: view.getFloat64(8, true),
    y: view.getFloat64(16, true),
    w: view.getFloat64(24, true),
    h: view.getFloat64(32, true),
    scale: view.getFloat64(40, true),
  };
  return {
    seq: Number(view.getBigUint64(0, true)),
    bounds,
    image: new Blob([buffer.slice(FRAME_HEADER_BYTES)], { type: mime }),
  };
}

function toBytes(value: unknown): ArrayBuffer {
  if (value instanceof ArrayBuffer) return value;
  if (ArrayBuffer.isView(value)) return value.buffer.slice(value.byteOffset, value.byteOffset + value.byteLength) as ArrayBuffer;
  if (Array.isArray(value)) return new Uint8Array(value as number[]).buffer;
  throw new Error('unexpected binary payload');
}

type LiveMessage = { kind: 'error'; message: string };

export const tauriBackend: Backend = {
  native: true,

  listScripts: () => invoke<ScriptSummary[]>('list_scripts'),
  getScript: (id) => invoke<ScriptRecord>('get_script', { id }),
  createScript: (name) => invoke<ScriptRecord>('create_script', { name }),
  saveScript: (id, ir) => invoke<void>('save_script', { id, ir }),
  renameScript: (id, name) => invoke<void>('rename_script', { id, name }),
  duplicateScript: (id, name) => invoke<ScriptRecord>('duplicate_script', { id, name }),
  exportScript: (id) => invoke<string | null>('export_script', { id }),
  importScript: () => invoke<ScriptRecord | null>('import_script'),
  getNotifyConfig: () => invoke<NotifyConfig>('get_notify_config'),
  saveNotifyConfig: (config) => invoke<void>('save_notify_config', { config }),
  sendTestNotification: (config) => invoke<void>('send_test_notification', { config }),
  deleteScript: (id) => invoke<void>('delete_script', { id }),
  validate: (ir: IrDoc) => invoke<Problem[]>('validate_ir', { ir }),

  listWindows: () => invoke<WindowInfo[]>('list_windows'),
  permissions: () => invoke<PermissionInfo>('permissions'),
  requestPermissions: () => invoke<PermissionInfo>('request_permissions'),
  openPermissionSettings: (kind) => invoke<void>('open_permission_settings', { kind }),

  async liveStart(surface: Surface, onFrame, onError) {
    const channel = new Channel<ArrayBuffer | LiveMessage>();
    channel.onmessage = (message) => {
      if (message && typeof message === 'object' && 'kind' in message && message.kind === 'error') {
        onError(message.message);
        return;
      }
      try {
        onFrame(parseFrame(toBytes(message), 'image/jpeg'));
      } catch (error) {
        onError(String(error));
      }
    };
    await invoke('live_start', { surface, onFrame: channel });
    return () => invoke<void>('live_stop');
  },

  async snapshot() {
    return parseFrame(toBytes(await invoke<ArrayBuffer>('snapshot')), 'image/png');
  },
  async loadImage(image) {
    const bytes = new Uint8Array(await image.arrayBuffer());
    return parseFrame(toBytes(await invoke<ArrayBuffer>('load_image', bytes)), 'image/png');
  },
  detectCandidates: (seq, x, y) => invoke<Rect[]>('detect_candidates', { seq, x, y }),
  createTemplate: (seq, rect) => invoke<Target>('create_template', { seq, rect }),
  async assetPng(sha) {
    return new Blob([toBytes(await invoke<ArrayBuffer>('asset_png', { sha }))], { type: 'image/png' });
  },

  async runStart(ir, surface, repeat, onMessage) {
    const channel = new Channel<RunMessage>();
    channel.onmessage = onMessage;
    return invoke<string>('run_start', { ir, surface, repeat, onMessage: channel });
  },
  runStop: () => invoke<void>('run_stop'),
  probe: (ir, surface) => invoke<ProbeRow[]>('probe', { ir, surface }),
  listRuns: () => invoke<RunSummary[]>('list_runs'),
  getRun: (id) => invoke<RunDetail>('get_run', { id }),
};
