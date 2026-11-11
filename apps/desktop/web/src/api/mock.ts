// In-browser stand-in for the Rust core, used by `pnpm dev` without Tauri (and by
// UI screenshots). It keeps scripts in localStorage and fakes the screen with a
// small synthetic scene; matching and input do not exist here.

import { emptyDoc, type IrDoc, type Problem, type Target } from '@/ir/model';
import type {
  Backend,
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

const STORE_KEY = 'liverguard.mock.scripts';
const RUNS_KEY = 'liverguard.mock.runs';
const NOTIFY_KEY = 'liverguard.mock.notify';
const SCENE = { w: 960, h: 600 };

function load<T>(key: string, fallback: T): T {
  try {
    return JSON.parse(localStorage.getItem(key) ?? '') as T;
  } catch {
    return fallback;
  }
}

// Like the real store, skip records in an older IR shape (v2 had `flows`, no `flow`).
const scripts = (): ScriptRecord[] => load<ScriptRecord[]>(STORE_KEY, []).filter((s) => s.ir?.flow);
const saveScripts = (all: ScriptRecord[]) => localStorage.setItem(STORE_KEY, JSON.stringify(all));
const newId = () => crypto.randomUUID().replaceAll('-', '');
const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

// ------------------------------------------------------------------- scene

const BUTTONS = [
  { label: 'START', x: 330, y: 250, w: 300, h: 90, color: '#3c78c4' },
  { label: 'CLAIM', x: 120, y: 430, w: 220, h: 70, color: '#2f9e6d' },
  { label: 'OK', x: 640, y: 430, w: 180, h: 70, color: '#c4673c' },
];

function drawScene(tick: number): HTMLCanvasElement {
  const canvas = document.createElement('canvas');
  canvas.width = SCENE.w;
  canvas.height = SCENE.h;
  const ctx = canvas.getContext('2d')!;
  const gradient = ctx.createLinearGradient(0, 0, SCENE.w, SCENE.h);
  gradient.addColorStop(0, '#1d2433');
  gradient.addColorStop(1, '#2a1f3d');
  ctx.fillStyle = gradient;
  ctx.fillRect(0, 0, SCENE.w, SCENE.h);
  ctx.fillStyle = '#ffffff22';
  ctx.font = '600 28px system-ui';
  ctx.fillText(`Mock game  ·  frame ${tick}`, 28, 52);
  for (const b of BUTTONS) {
    ctx.fillStyle = b.color;
    ctx.beginPath();
    ctx.roundRect(b.x, b.y, b.w, b.h, 14);
    ctx.fill();
    ctx.strokeStyle = '#ffffffcc';
    ctx.lineWidth = 3;
    ctx.stroke();
    ctx.fillStyle = '#fff';
    ctx.font = '700 32px system-ui';
    ctx.textAlign = 'center';
    ctx.fillText(b.label, b.x + b.w / 2, b.y + b.h / 2 + 11);
    ctx.textAlign = 'start';
  }
  return canvas;
}

const toBlob = (canvas: HTMLCanvasElement, type: string) =>
  new Promise<Blob>((resolve, reject) => canvas.toBlob((b) => (b ? resolve(b) : reject(new Error('encode failed'))), type, 0.8));

let seq = 0;
let lastCanvas: HTMLCanvasElement | null = null;
const assets = new Map<string, Blob>();

async function frame(type: string): Promise<FramePacket> {
  seq += 1;
  lastCanvas = drawScene(seq);
  return { seq, bounds: { x: 0, y: 0, w: SCENE.w, h: SCENE.h, scale: 1 }, image: await toBlob(lastCanvas, type) };
}

// ------------------------------------------------------------------ running

let stopFlag = false;

/** The happy-path handle(s) the fake run follows. */
function nextHandle(kind: string): string[] {
  switch (kind) {
    case 'waitFor':
      return ['found'];
    case 'if':
      return ['true'];
    case 'loop':
    case 'retry':
      return ['body', 'done'];
    case 'switch':
      return ['default'];
    default:
      return ['next'];
  }
}

async function simulate(ir: IrDoc, repeat: number, emit: (m: RunMessage) => void): Promise<RunMessage & { kind: 'done' }> {
  const startedAt = Date.now();
  let steps = 0;
  emit({ kind: 'log', level: 'info', event: 'run_start', payload: { script: ir.meta.name, mock: true } });
  const rounds = Math.max(1, Math.round(repeat) || 1);
  for (let round = 1; round <= rounds && !stopFlag; round += 1) {
    if (rounds > 1) emit({ kind: 'log', level: 'info', event: 'round_start', payload: { round, total: rounds } });
    steps = await simulateRound(ir, emit, steps);
  }
  const state = stopFlag ? 'stopped' : 'done';
  emit({ kind: 'log', level: 'info', event: state === 'done' ? 'run_done' : 'run_stopped', payload: { steps, rounds } });
  return {
    kind: 'done',
    runId: newId(),
    result: { state, stepCount: steps, errorCode: null, errorMsg: null, errorNode: null, startedAt, endedAt: Date.now() },
  };
}

/** One walk from `start`; returns the running step total. */
async function simulateRound(ir: IrDoc, emit: (m: RunMessage) => void, before: number): Promise<number> {
  let steps = before;
  const flow = ir.flow;
  let node = flow.nodes.find((n) => n.type === 'start');
  const loops = new Map<string, number>();
  while (node && !stopFlag && steps - before < 200) {
    const current = node;
    if (current.type !== 'start') emit({ kind: 'node', nodeId: current.id, state: 'running' });
    await sleep(current.type === 'start' ? 10 : 350);
    if (current.type === 'log') emit({ kind: 'log', level: String(current.data.level ?? 'info'), event: 'script_log', payload: { stepId: current.id, msg: current.data.msg } });
    if (current.type === 'tap') emit({ kind: 'log', level: 'info', event: 'tap', payload: { stepId: current.id, target: current.data.target, score: 0.93 } });
    if (current.type !== 'start') emit({ kind: 'node', nodeId: current.id, state: 'ok', score: current.type === 'tap' ? 0.93 : undefined });
    steps += 1;
    if (current.type === 'end') break;

    let handles = nextHandle(current.type);
    if (current.type === 'loop') {
      const count = Number(current.data.count ?? 1);
      const done = loops.get(current.id) ?? 0;
      handles = done < count && current.data.mode !== 'while' ? ['body'] : ['done'];
      loops.set(current.id, done + 1);
    }
    const edge = flow.edges.find((e) => e.source === current.id && handles.includes(e.sourceHandle));
    node = edge ? flow.nodes.find((n) => n.id === edge.target) : undefined;
  }
  return steps;
}

// ------------------------------------------------------------ import/export

const BUNDLE_FORMAT = 'liverguard-script';

const toBase64 = async (blob: Blob) => {
  let text = '';
  for (const byte of new Uint8Array(await blob.arrayBuffer())) text += String.fromCharCode(byte);
  return btoa(text);
};

const fromBase64 = (text: string, type: string) => new Blob([Uint8Array.from(atob(text), (c) => c.charCodeAt(0))], { type });

function pickJsonFile(): Promise<File | null> {
  return new Promise((resolve) => {
    const input = document.createElement('input');
    input.type = 'file';
    input.accept = 'application/json,.json';
    input.onchange = () => resolve(input.files?.[0] ?? null);
    input.oncancel = () => resolve(null);
    input.click();
  });
}

// ------------------------------------------------------------------ backend

export const mockBackend: Backend = {
  native: false,

  async listScripts(): Promise<ScriptSummary[]> {
    return scripts().map(({ id, name }) => ({ id, name })).sort((a, b) => a.name.localeCompare(b.name));
  },
  async getScript(id) {
    const found = scripts().find((s) => s.id === id);
    if (!found) throw new Error('脚本不存在');
    return found;
  },
  async createScript(name) {
    const trimmed = name.trim();
    if (!trimmed) throw new Error('脚本名称不能为空');
    const record: ScriptRecord = { id: newId(), name: trimmed, ir: emptyDoc(trimmed) };
    saveScripts([...scripts(), record]);
    return record;
  },
  async saveScript(id, ir) {
    saveScripts(scripts().map((s) => (s.id === id ? { ...s, ir: { ...ir, meta: { ...ir.meta, name: s.name } } } : s)));
  },
  async renameScript(id, name) {
    saveScripts(scripts().map((s) => (s.id === id ? { ...s, name, ir: { ...s.ir, meta: { ...s.ir.meta, name } } } : s)));
  },
  async duplicateScript(id, name) {
    const trimmed = name.trim();
    if (!trimmed) throw new Error('脚本名称不能为空');
    const found = scripts().find((s) => s.id === id);
    if (!found) throw new Error('脚本不存在');
    const record: ScriptRecord = { id: newId(), name: trimmed, ir: { ...structuredClone(found.ir), meta: { ...found.ir.meta, name: trimmed } } };
    saveScripts([...scripts(), record]);
    return record;
  },
  async exportScript(id) {
    const found = scripts().find((s) => s.id === id);
    if (!found) throw new Error('脚本不存在');
    const bundleAssets: Record<string, string> = {};
    for (const target of Object.values(found.ir.targets)) {
      const blob = target.type === 'template' ? assets.get(target.asset) : undefined;
      if (target.type === 'template' && blob) bundleAssets[target.asset] = await toBase64(blob);
    }
    const bundle = { format: BUNDLE_FORMAT, version: 1, name: found.name, ir: found.ir, assets: bundleAssets };
    const url = URL.createObjectURL(new Blob([JSON.stringify(bundle, null, 2)], { type: 'application/json' }));
    const link = document.createElement('a');
    link.href = url;
    link.download = `${found.name}.liverguard.json`;
    link.click();
    URL.revokeObjectURL(url);
    return link.download;
  },
  async importScript() {
    const file = await pickJsonFile();
    if (!file) return null;
    let bundle: { format?: string; version?: number; name?: string; ir?: IrDoc; assets?: Record<string, string> };
    try {
      bundle = JSON.parse(await file.text());
    } catch {
      throw new Error('不是有效的脚本文件（无法解析为 JSON）');
    }
    if (bundle.format !== BUNDLE_FORMAT || !bundle.ir?.flow) throw new Error('不是 LiverGuard 导出的脚本文件');
    for (const [sha, text] of Object.entries(bundle.assets ?? {})) assets.set(sha, fromBase64(text, 'image/png'));
    const taken = new Set(scripts().map((s) => s.name));
    const base = bundle.name?.trim() || '导入的脚本';
    let name = base;
    for (let n = 2; taken.has(name); n += 1) name = n === 2 ? `${base}（导入）` : `${base}（导入 ${n - 1}）`;
    const record: ScriptRecord = { id: newId(), name, ir: { ...bundle.ir, meta: { ...bundle.ir.meta, name } } };
    saveScripts([...scripts(), record]);
    return record;
  },
  async getNotifyConfig() {
    return load<NotifyConfig>(NOTIFY_KEY, { channel: 'dingtalk', url: '', secret: '', keyword: '' });
  },
  async saveNotifyConfig(config) {
    if (config.url.trim() && !/^https?:\/\/\S+$/.test(config.url.trim())) throw new Error('Webhook 地址必须是 http:// 或 https:// 开头的链接');
    localStorage.setItem(NOTIFY_KEY, JSON.stringify(config));
  },
  async sendTestNotification(config) {
    if (!/^https?:\/\/\S+$/.test(config.url.trim())) throw new Error('请填写 Webhook 地址');
    await sleep(400);
    throw new Error('浏览器模拟后端不会真的发送；请在桌面应用里测试');
  },
  async deleteScript(id) {
    saveScripts(scripts().filter((s) => s.id !== id));
  },
  async validate(ir): Promise<Problem[]> {
    const problems: Problem[] = [];
    if (!ir.flow.nodes.some((n) => n.type === 'start')) problems.push({ severity: 'error', message: 'script has no start node', node: null });
    return problems;
  },

  async listWindows(): Promise<WindowInfo[]> {
    const make = (number: number, owner: string, title: string): WindowInfo => ({
      number,
      owner,
      title,
      bounds: { x: 0, y: 0, w: SCENE.w, h: SCENE.h },
      label: `#${number} ${owner} — ${title} [${SCENE.w}x${SCENE.h}]`,
    });
    return [make(101, 'Mock Game', 'Main'), make(102, 'Mock Launcher', 'Library')];
  },
  async permissions(): Promise<PermissionInfo> {
    return { platform: 'other', accessibility: true, screenRecording: true, owner: 'browser', problems: [], ok: true };
  },
  async requestPermissions() {
    return this.permissions();
  },
  async openPermissionSettings() {},

  async liveStart(_surface: Surface, onFrame) {
    let alive = true;
    (async () => {
      while (alive) {
        onFrame(await frame('image/jpeg'));
        await sleep(250);
      }
    })();
    return async () => {
      alive = false;
    };
  },
  snapshot: () => frame('image/png'),
  async loadImage(image) {
    const bitmap = await createImageBitmap(image);
    const canvas = document.createElement('canvas');
    canvas.width = bitmap.width;
    canvas.height = bitmap.height;
    canvas.getContext('2d')!.drawImage(bitmap, 0, 0);
    bitmap.close();
    seq += 1;
    lastCanvas = canvas;
    return { seq, bounds: { x: 0, y: 0, w: canvas.width, h: canvas.height, scale: 1 }, image: await toBlob(canvas, 'image/png') };
  },
  async detectCandidates(_seq, x, y): Promise<Rect[]> {
    const hit = BUTTONS.find((b) => x >= b.x && x <= b.x + b.w && y >= b.y && y <= b.y + b.h);
    const box = (cx: number, cy: number, w: number, h: number): Rect => ({ x: Math.round(cx - w / 2), y: Math.round(cy - h / 2), w, h });
    if (hit) return [{ x: hit.x, y: hit.y, w: hit.w, h: hit.h }, box(x, y, 64, 40)];
    return [box(x, y, 80, 80), box(x, y, 140, 100)];
  },
  async createTemplate(_seq, rect): Promise<Target> {
    const sceneW = lastCanvas?.width ?? SCENE.w;
    const sceneH = lastCanvas?.height ?? SCENE.h;
    const crop = document.createElement('canvas');
    crop.width = Math.max(1, rect.w);
    crop.height = Math.max(1, rect.h);
    if (lastCanvas) crop.getContext('2d')!.drawImage(lastCanvas, rect.x, rect.y, rect.w, rect.h, 0, 0, rect.w, rect.h);
    const sha = Array.from(crypto.getRandomValues(new Uint8Array(32)), (b) => b.toString(16).padStart(2, '0')).join('');
    assets.set(sha, await toBlob(crop, 'image/png'));
    return {
      type: 'template',
      asset: sha,
      assetBox: { w: rect.w, h: rect.h },
      recordScale: 1,
      recordWidth: sceneW,
      roi: [Math.max(0, rect.x / sceneW - 0.1), Math.max(0, rect.y / sceneH - 0.1), Math.min(1, rect.w / sceneW + 0.2), Math.min(1, rect.h / sceneH + 0.2)],
      anchor: [0.5, 0.5],
      strategy: ['tpl', 'mstpl'],
      scale: { mode: 'auto', min: 0.7, max: 1.5, steps: 9 },
      threshold: 0.85,
      warnThreshold: 0.7,
      verify: 'rgb',
      mask: 'none',
      multi: 'nearest-last',
      auto: true,
    } as Target;
  },
  async refineTemplate(target, rect): Promise<Target> {
    if (target.type !== 'template') throw new Error('只有模板目标可以框选图片');
    const source = assets.get(target.asset);
    if (!source) throw new Error('素材不存在');
    const bitmap = await createImageBitmap(source);
    const x = Math.min(Math.max(0, rect.x), bitmap.width - 1);
    const y = Math.min(Math.max(0, rect.y), bitmap.height - 1);
    const w = Math.min(rect.w, bitmap.width - x);
    const h = Math.min(rect.h, bitmap.height - y);
    if (w < 4 || h < 4) {
      bitmap.close();
      throw new Error('框选区域太小');
    }
    const crop = document.createElement('canvas');
    crop.width = w;
    crop.height = h;
    crop.getContext('2d')!.drawImage(bitmap, x, y, w, h, 0, 0, w, h);
    bitmap.close();
    const sha = Array.from(crypto.getRandomValues(new Uint8Array(32)), (b) => b.toString(16).padStart(2, '0')).join('');
    assets.set(sha, await toBlob(crop, 'image/png'));
    return { ...target, asset: sha, assetBox: { w, h } };
  },
  async assetPng(sha) {
    const blob = assets.get(sha);
    if (!blob) throw new Error('素材不存在');
    return blob;
  },

  async runStart(ir, _surface, repeat, onMessage) {
    stopFlag = false;
    const id = newId();
    simulate(ir, repeat, onMessage).then((done) => {
      onMessage(done);
      const runs = load<RunSummary[]>(RUNS_KEY, []);
      localStorage.setItem(RUNS_KEY, JSON.stringify([{ id: done.runId, script: ir.meta.name, ...done.result }, ...runs].slice(0, 30)));
    });
    return id;
  },
  async runStop() {
    stopFlag = true;
  },
  async probe(ir): Promise<ProbeRow[]> {
    return Object.keys(ir.targets).map((target, i) => ({
      target,
      found: true,
      ok: i % 3 !== 2,
      score: i % 3 !== 2 ? 0.95 - i * 0.01 : 0.55,
      threshold: 0.85,
      rect: { x: 100, y: 100, w: 80, h: 40 },
      point: [140, 120],
      strategy: 'tpl',
      elapsedMs: 8,
    }));
  },
  async listRuns() {
    return load<RunSummary[]>(RUNS_KEY, []);
  },
  async getRun(id): Promise<RunDetail> {
    const record = load<RunSummary[]>(RUNS_KEY, []).find((r) => r.id === id);
    if (!record) throw new Error('运行记录不存在');
    return { id, record, logs: '' };
  },

  // The browser has no release feed to ask.
  async checkUpdate() {
    return null;
  },
  async installUpdate() {
    throw new Error('浏览器里无法更新');
  },
};
