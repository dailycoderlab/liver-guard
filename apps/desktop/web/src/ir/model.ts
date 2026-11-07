// Editor-side view of IR v3. The wire shape matches crates/liverguard-ir; `generated.ts`
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
  | 'setVar';

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
}

export interface IrDoc {
  irVersion: 3;
  meta: IrMeta;
  targets: Record<string, Target>;
  flow: IrFlow;
}

export interface Problem {
  severity: 'error' | 'warning';
  message: string;
  node: string | null;
}

export function emptyDoc(name: string): IrDoc {
  return {
    irVersion: 3,
    meta: { name },
    targets: {},
    flow: { nodes: [startNode('start', { x: 0, y: 0 })], edges: [] },
  };
}

export function startNode(id: string, pos: Pos): IrNode {
  return { id, type: 'start', pos, data: {} };
}
