// Editor-side view of IR v2. The wire shape matches crates/sky-ir; `generated.ts`
// (from the Rust JSON Schema) is the source for `Target` and `Condition`.

import type { Condition, Target } from './generated';

export type { Condition, Target };

export type NodeType =
  | 'start'
  | 'end'
  | 'tap'
  | 'longPress'
  | 'swipe'
  | 'key'
  | 'text'
  | 'log'
  | 'snapshot'
  | 'sleep'
  | 'waitFor'
  | 'waitGone'
  | 'waitStable'
  | 'if'
  | 'switch'
  | 'loop'
  | 'retry'
  | 'setVar'
  | 'call';

export type VarValue = boolean | number | string;

export interface Pos {
  x: number;
  y: number;
}

/** A node as stored in a script file. */
export interface IrNode {
  id: string;
  type: NodeType;
  pos: Pos;
  label?: string;
  maxVisits?: number;
  data: Record<string, unknown>;
}

export interface IrEdge {
  id: string;
  source: string;
  sourceHandle: string;
  target: string;
  targetHandle?: string;
}

export interface IrFlow {
  nodes: IrNode[];
  edges: IrEdge[];
}

export interface IrMeta {
  name: string;
  game?: string;
  minEngine?: string;
  maxDurationMs: number;
}

export interface IrDoc {
  irVersion: 2;
  meta: IrMeta;
  vars: Record<string, VarValue>;
  targets: Record<string, Target>;
  entry: string;
  flows: Record<string, IrFlow>;
}

export interface Problem {
  severity: 'error' | 'warning';
  message: string;
  flow: string | null;
  node: string | null;
}

export const DEFAULT_MAX_DURATION_MS = 1_800_000;
export const MAIN_FLOW = 'main';

export function emptyDoc(name: string): IrDoc {
  return {
    irVersion: 2,
    meta: { name, maxDurationMs: DEFAULT_MAX_DURATION_MS },
    vars: {},
    targets: {},
    entry: MAIN_FLOW,
    flows: { [MAIN_FLOW]: { nodes: [startNode('start', { x: 0, y: 0 })], edges: [] } },
  };
}

export function startNode(id: string, pos: Pos): IrNode {
  return { id, type: 'start', pos, data: {} };
}
