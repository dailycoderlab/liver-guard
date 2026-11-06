// What the editor knows about each node type: label, colour, default data and
// output handles. The handle rules mirror `NodeKind::output_handles` in Rust;
// `catalog.test.ts` pins them.

import type { LucideIcon } from 'lucide-react';
import {
  Camera,
  CircleStop,
  Clock,
  Eye,
  EyeOff,
  Hand,
  Keyboard,
  Layers,
  MessageSquare,
  MousePointerClick,
  MoveRight,
  Play,
  Repeat,
  RotateCcw,
  Split,
  Timer,
  Type,
  Variable,
  Waypoints,
} from 'lucide-react';
import type { NodeType } from './model';

export type HandleTone = 'ok' | 'fail';

export interface HandleSpec {
  id: string;
  label: string;
  tone: HandleTone;
}

export type Category = 'flow' | 'action' | 'wait' | 'logic';

export interface NodeSpec {
  type: NodeType;
  label: string;
  hint: string;
  category: Category;
  icon: LucideIcon;
  /** Tailwind colour classes for the node header. */
  accent: string;
  defaults: () => Record<string, unknown>;
}

export const CATEGORY_LABELS: Record<Category, string> = {
  flow: '流程',
  action: '动作',
  wait: '等待',
  logic: '逻辑',
};

export const NODE_SPECS: Record<NodeType, NodeSpec> = {
  start: { type: 'start', label: '开始', hint: '流程入口，每个流程恰好一个', category: 'flow', icon: Play, accent: 'bg-emerald-600', defaults: () => ({}) },
  end: { type: 'end', label: '结束', hint: '结束当前流程；在被调用的流程里只返回调用处', category: 'flow', icon: CircleStop, accent: 'bg-slate-600', defaults: () => ({}) },
  call: { type: 'call', label: '调用流程', hint: '运行另一个流程后继续', category: 'flow', icon: Layers, accent: 'bg-violet-600', defaults: () => ({ flow: '' }) },
  tap: { type: 'tap', label: '点击', hint: '找到目标后点击；找不到走「失败」', category: 'action', icon: MousePointerClick, accent: 'bg-sky-600', defaults: () => ({ target: '', durationMs: 60, delayMs: 0, offsetRatio: 0 }) },
  longPress: { type: 'longPress', label: '长按', hint: '找到目标后按住', category: 'action', icon: Hand, accent: 'bg-sky-600', defaults: () => ({ target: '', durationMs: 600, delayMs: 0, offsetRatio: 0 }) },
  swipe: { type: 'swipe', label: '滑动', hint: '从一个目标拖到另一个目标，或按比例拖动', category: 'action', icon: MoveRight, accent: 'bg-sky-600', defaults: () => ({ from: '', durationMs: 300, vector: [0, -0.3] }) },
  key: { type: 'key', label: '按键', hint: 'return / escape / space / f1…', category: 'action', icon: Keyboard, accent: 'bg-sky-600', defaults: () => ({ key: 'return' }) },
  text: { type: 'text', label: '输入文字', hint: '向当前焦点输入', category: 'action', icon: Type, accent: 'bg-sky-600', defaults: () => ({ text: '' }) },
  log: { type: 'log', label: '日志', hint: '写一条运行日志', category: 'action', icon: MessageSquare, accent: 'bg-sky-600', defaults: () => ({ level: 'info', msg: '' }) },
  snapshot: { type: 'snapshot', label: '截图记录', hint: '记录当前画面的指纹', category: 'action', icon: Camera, accent: 'bg-sky-600', defaults: () => ({ label: 'snap' }) },
  sleep: { type: 'sleep', label: '等待时间', hint: '固定等待若干毫秒', category: 'wait', icon: Clock, accent: 'bg-amber-600', defaults: () => ({ ms: 1000 }) },
  waitFor: { type: 'waitFor', label: '等待出现', hint: '任一目标出现即继续；超时走「超时」', category: 'wait', icon: Eye, accent: 'bg-amber-600', defaults: () => ({ any: [], timeoutMs: 10000, intervalMs: 500 }) },
  waitGone: { type: 'waitGone', label: '等待消失', hint: '目标消失后继续', category: 'wait', icon: EyeOff, accent: 'bg-amber-600', defaults: () => ({ target: '', timeoutMs: 10000, intervalMs: 500 }) },
  waitStable: { type: 'waitStable', label: '等待画面稳定', hint: '画面保持不变一段时间后继续', category: 'wait', icon: Timer, accent: 'bg-amber-600', defaults: () => ({ ms: 400, timeoutMs: 5000 }) },
  if: { type: 'if', label: '条件分支', hint: '按条件走「是」或「否」', category: 'logic', icon: Split, accent: 'bg-fuchsia-600', defaults: () => ({ when: { kind: 'visible', targets: [] } }) },
  switch: { type: 'switch', label: '多路分支', hint: '按顺序检查目标，第一个可见的走对应分支', category: 'logic', icon: Waypoints, accent: 'bg-fuchsia-600', defaults: () => ({ cases: [] }) },
  loop: { type: 'loop', label: '循环', hint: '重复执行循环体，结束后走「完成」', category: 'logic', icon: Repeat, accent: 'bg-fuchsia-600', defaults: () => ({ mode: 'count', count: 3 }) },
  retry: { type: 'retry', label: '重试', hint: '失败时重试循环体，用尽后走「失败」', category: 'logic', icon: RotateCcw, accent: 'bg-fuchsia-600', defaults: () => ({ times: 3, backoffMs: 500 }) },
  setVar: { type: 'setVar', label: '设置变量', hint: '给条件分支提供变量', category: 'logic', icon: Variable, accent: 'bg-fuchsia-600', defaults: () => ({ name: '', value: 0 }) },
};

export const NODE_ORDER: NodeType[] = Object.keys(NODE_SPECS) as NodeType[];

const ok = (id: string, label: string): HandleSpec => ({ id, label, tone: 'ok' });
const fail = (id: string, label: string): HandleSpec => ({ id, label, tone: 'fail' });

/** Output handles of a node, in display order. */
export function outputHandles(type: NodeType, data: Record<string, unknown> = {}): HandleSpec[] {
  switch (type) {
    case 'end':
      return [];
    case 'tap':
    case 'longPress':
    case 'swipe':
      return [ok('next', '下一步'), fail('fail', '失败')];
    case 'waitFor':
      return [ok('found', '出现'), fail('timeout', '超时')];
    case 'waitGone':
    case 'waitStable':
      return [ok('next', '下一步'), fail('timeout', '超时')];
    case 'if':
      return [ok('true', '是'), fail('false', '否')];
    case 'loop':
      return [ok('body', '循环体'), ok('done', '完成')];
    case 'retry':
      return [ok('body', '重试体'), ok('done', '完成'), fail('failed', '失败')];
    case 'switch': {
      const cases = Array.isArray(data.cases) ? (data.cases as string[]) : [];
      return [...cases.map((name) => ok(`case:${name}`, name || '(未选目标)')), ok('default', '其他')];
    }
    default:
      return [ok('next', '下一步')];
  }
}

/** Targets a node reads, used to show thumbnails and to find unused targets. */
export function targetRefs(type: NodeType, data: Record<string, unknown>): string[] {
  const str = (v: unknown) => (typeof v === 'string' && v ? [v] : []);
  const list = (v: unknown) => (Array.isArray(v) ? (v as string[]).filter(Boolean) : []);
  switch (type) {
    case 'tap':
    case 'longPress':
    case 'waitGone':
      return str(data.target);
    case 'swipe':
      return [...str(data.from), ...str(data.to)];
    case 'waitFor':
      return list(data.any);
    case 'switch':
      return list(data.cases);
    case 'if':
      return conditionTargets(data.when as ConditionLike | undefined);
    case 'loop':
      return conditionTargets(data.when as ConditionLike | undefined);
    default:
      return [];
  }
}

type ConditionLike = { kind: string; targets?: string[]; target?: string; cond?: ConditionLike; conds?: ConditionLike[] };

export function conditionTargets(cond: ConditionLike | undefined): string[] {
  if (!cond) return [];
  switch (cond.kind) {
    case 'visible':
      return cond.targets ?? [];
    case 'missing':
      return cond.target ? [cond.target] : [];
    case 'not':
      return conditionTargets(cond.cond);
    case 'all':
    case 'any':
      return (cond.conds ?? []).flatMap(conditionTargets);
    default:
      return [];
  }
}

/** One-line summary shown inside a node. */
export function summarize(type: NodeType, data: Record<string, unknown>): string {
  switch (type) {
    case 'tap':
    case 'longPress':
      return String(data.target || '未选择目标');
    case 'swipe':
      return `${data.from || '未选择起点'} → ${data.to || '按比例'}`;
    case 'key':
      return String(data.key ?? '');
    case 'text':
      return String(data.text ?? '');
    case 'log':
      return String(data.msg ?? '');
    case 'sleep':
      return `${data.ms ?? 0} ms`;
    case 'waitFor':
      return ((data.any as string[]) ?? []).join(' | ') || '未选择目标';
    case 'waitGone':
      return String(data.target || '未选择目标');
    case 'waitStable':
      return `稳定 ${data.ms ?? 0} ms`;
    case 'if':
      return describeCondition(data.when as ConditionLike | undefined);
    case 'switch':
      return `${((data.cases as string[]) ?? []).length} 个分支`;
    case 'loop': {
      const mode = data.mode as string;
      if (mode === 'forever') return '无限循环';
      if (mode === 'while') return `当 ${describeCondition(data.when as ConditionLike | undefined)}`;
      return `${data.count ?? 1} 次`;
    }
    case 'retry':
      return `最多 ${data.times ?? 3} 次`;
    case 'setVar':
      return `${data.name || '?'} = ${String(data.value)}`;
    case 'call':
      return String(data.flow || '未选择流程');
    case 'snapshot':
      return String(data.label ?? '');
    default:
      return '';
  }
}

export function describeCondition(cond: ConditionLike | undefined): string {
  if (!cond) return '(未设置)';
  switch (cond.kind) {
    case 'visible':
      return `${(cond.targets ?? []).join(' | ') || '?'} 可见`;
    case 'missing':
      return `${cond.target || '?'} 不可见`;
    case 'var': {
      const c = cond as unknown as { name: string; op?: string; value: unknown };
      return `${c.name || '?'} ${c.op ?? 'eq'} ${String(c.value)}`;
    }
    case 'not':
      return `非(${describeCondition(cond.cond)})`;
    case 'all':
      return `全部(${(cond.conds ?? []).length})`;
    case 'any':
      return `任一(${(cond.conds ?? []).length})`;
    default:
      return cond.kind;
  }
}
