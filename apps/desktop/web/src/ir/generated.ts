/* Generated from the Rust IR schema (crates/liverguard-ir). Do not edit; run `pnpm gen:types`. */

/**
 * This interface was referenced by `IrDoc`'s JSON-Schema
 * via the `definition` "Node".
 */
export type Node = {
  id: string;
  /**
   * Display name; only affects the editor and run logs.
   */
  label?: string | null;
  /**
   * Optional cap on how often the node may run (guards cycles).
   */
  maxVisits?: number | null;
  pos?: Pos;
} & Node1;
export type Node1 =
  | {
      data: EmptyData;
      type: 'start';
    }
  | {
      data: EmptyData;
      type: 'end';
    }
  | {
      data: TapData;
      type: 'tap';
    }
  | {
      data: LongPressData;
      type: 'longPress';
    }
  | {
      data: SwipeData;
      type: 'swipe';
    }
  | {
      data: KeyData;
      type: 'key';
    }
  | {
      data: TextData;
      type: 'text';
    }
  | {
      data: LogData;
      type: 'log';
    }
  | {
      data: SnapshotData;
      type: 'snapshot';
    }
  | {
      data: SleepData;
      type: 'sleep';
    }
  | {
      data: WaitForData;
      type: 'waitFor';
    }
  | {
      data: WaitGoneData;
      type: 'waitGone';
    }
  | {
      data: WaitStableData;
      type: 'waitStable';
    }
  | {
      data: IfData;
      type: 'if';
    }
  | {
      data: SwitchData;
      type: 'switch';
    }
  | {
      data: LoopData;
      type: 'loop';
    }
  | {
      data: RetryData;
      type: 'retry';
    }
  | {
      data: SetVarData;
      type: 'setVar';
    }
  | {
      data: CallData;
      type: 'call';
    };
/**
 * Branch condition. `visible` / `missing` look at the screen; the rest combine
 * them or read variables.
 *
 * This interface was referenced by `IrDoc`'s JSON-Schema
 * via the `definition` "Condition".
 */
export type Condition =
  | {
      kind: 'visible';
      targets: string[];
    }
  | {
      kind: 'missing';
      target: string;
    }
  | {
      kind: 'var';
      name: string;
      op?: 'eq' | 'ne' | 'lt' | 'le' | 'gt' | 'ge' | 'contains' | 'match';
      value: VarValue;
    }
  | {
      cond: Condition;
      kind: 'not';
    }
  | {
      conds: Condition[];
      kind: 'all';
    }
  | {
      conds: Condition[];
      kind: 'any';
    };
/**
 * A script variable value.
 *
 * This interface was referenced by `IrDoc`'s JSON-Schema
 * via the `definition` "VarValue".
 */
export type VarValue = boolean | number | string;
/**
 * This interface was referenced by `IrDoc`'s JSON-Schema
 * via the `definition` "Target".
 */
export type Target = TemplateTarget | PointTarget;
/**
 * This interface was referenced by `IrDoc`'s JSON-Schema
 * via the `definition` "Strategy".
 */
export type Strategy = 'tpl' | 'mstpl' | 'sift';
/**
 * This interface was referenced by `IrDoc`'s JSON-Schema
 * via the `definition` "CompareOp".
 */
export type CompareOp = 'eq' | 'ne' | 'lt' | 'le' | 'gt' | 'ge' | 'contains' | 'match';
/**
 * This interface was referenced by `IrDoc`'s JSON-Schema
 * via the `definition` "LoopMode".
 */
export type LoopMode = 'count' | 'while' | 'forever';
/**
 * This interface was referenced by `IrDoc`'s JSON-Schema
 * via the `definition` "Mask".
 */
export type Mask = 'none' | 'edges';
/**
 * This interface was referenced by `IrDoc`'s JSON-Schema
 * via the `definition` "Multi".
 */
export type Multi = 'nearest-last' | 'highest' | 'center' | 'nth';
/**
 * This interface was referenced by `IrDoc`'s JSON-Schema
 * via the `definition` "ScaleMode".
 */
export type ScaleMode = 'auto' | 'fixed';
/**
 * This interface was referenced by `IrDoc`'s JSON-Schema
 * via the `definition` "Verify".
 */
export type Verify = 'none' | 'rgb';

export interface IrDoc {
  entry?: string;
  flows: {
    [k: string]: Flow;
  };
  irVersion?: number;
  meta: Meta;
  targets?: {
    [k: string]: Target;
  };
  vars?: {
    [k: string]: VarValue;
  };
}
/**
 * This interface was referenced by `IrDoc`'s JSON-Schema
 * via the `definition` "Flow".
 */
export interface Flow {
  edges?: Edge[];
  nodes?: Node[];
}
/**
 * This interface was referenced by `IrDoc`'s JSON-Schema
 * via the `definition` "Edge".
 */
export interface Edge {
  id: string;
  source: string;
  sourceHandle: string;
  target: string;
  targetHandle?: string | null;
}
/**
 * Editor position; the engine ignores it.
 */
export interface Pos {
  x: number;
  y: number;
}
/**
 * This interface was referenced by `IrDoc`'s JSON-Schema
 * via the `definition` "EmptyData".
 */
export interface EmptyData {}
/**
 * Random delay and offset shared by the click-like nodes.
 *
 * This interface was referenced by `IrDoc`'s JSON-Schema
 * via the `definition` "TapData".
 */
export interface TapData {
  /**
   * Fixed wait before the input, milliseconds.
   */
  delayMs?: number;
  /**
   * `[low, high]` random wait added on top of `delay_ms`.
   *
   * @minItems 2
   * @maxItems 2
   */
  delayRangeMs?: [number, number] | null;
  durationMs?: number;
  /**
   * Maximum random offset as a fraction of the matched box (capped at 0.5).
   */
  offsetRatio?: number;
  target: string;
}
/**
 * Random delay and offset shared by the click-like nodes.
 *
 * This interface was referenced by `IrDoc`'s JSON-Schema
 * via the `definition` "LongPressData".
 */
export interface LongPressData {
  /**
   * Fixed wait before the input, milliseconds.
   */
  delayMs?: number;
  /**
   * `[low, high]` random wait added on top of `delay_ms`.
   *
   * @minItems 2
   * @maxItems 2
   */
  delayRangeMs?: [number, number] | null;
  durationMs?: number;
  /**
   * Maximum random offset as a fraction of the matched box (capped at 0.5).
   */
  offsetRatio?: number;
  target: string;
}
/**
 * This interface was referenced by `IrDoc`'s JSON-Schema
 * via the `definition` "SwipeData".
 */
export interface SwipeData {
  durationMs?: number;
  from: string;
  to?: string | null;
  /**
   * Relative drag `[dx, dy]` in fractions of the surface when `to` is absent.
   *
   * @minItems 2
   * @maxItems 2
   */
  vector?: [number, number] | null;
}
/**
 * This interface was referenced by `IrDoc`'s JSON-Schema
 * via the `definition` "KeyData".
 */
export interface KeyData {
  key: string;
}
/**
 * This interface was referenced by `IrDoc`'s JSON-Schema
 * via the `definition` "TextData".
 */
export interface TextData {
  text: string;
}
/**
 * This interface was referenced by `IrDoc`'s JSON-Schema
 * via the `definition` "LogData".
 */
export interface LogData {
  level?: string;
  msg?: string;
}
/**
 * This interface was referenced by `IrDoc`'s JSON-Schema
 * via the `definition` "SnapshotData".
 */
export interface SnapshotData {
  label?: string;
}
/**
 * This interface was referenced by `IrDoc`'s JSON-Schema
 * via the `definition` "SleepData".
 */
export interface SleepData {
  ms: number;
}
/**
 * This interface was referenced by `IrDoc`'s JSON-Schema
 * via the `definition` "WaitForData".
 */
export interface WaitForData {
  /**
   * Proceeds as soon as any of these shows up.
   */
  any: string[];
  intervalMs?: number;
  timeoutMs?: number;
}
/**
 * This interface was referenced by `IrDoc`'s JSON-Schema
 * via the `definition` "WaitGoneData".
 */
export interface WaitGoneData {
  intervalMs?: number;
  target: string;
  timeoutMs?: number;
}
/**
 * This interface was referenced by `IrDoc`'s JSON-Schema
 * via the `definition` "WaitStableData".
 */
export interface WaitStableData {
  /**
   * How long the screen must stay unchanged.
   */
  ms?: number;
  timeoutMs?: number;
}
/**
 * This interface was referenced by `IrDoc`'s JSON-Schema
 * via the `definition` "IfData".
 */
export interface IfData {
  when: Condition;
}
/**
 * This interface was referenced by `IrDoc`'s JSON-Schema
 * via the `definition` "SwitchData".
 */
export interface SwitchData {
  /**
   * Targets checked in order; the first visible one takes its `case:<target>` handle.
   */
  cases: string[];
}
/**
 * This interface was referenced by `IrDoc`'s JSON-Schema
 * via the `definition` "LoopData".
 */
export interface LoopData {
  count?: number;
  /**
   * Safety cap for `while` / `forever`.
   */
  maxIterations?: number | null;
  mode?: 'count' | 'while' | 'forever';
  /**
   * Required for `while`.
   */
  when?: Condition | null;
}
/**
 * This interface was referenced by `IrDoc`'s JSON-Schema
 * via the `definition` "RetryData".
 */
export interface RetryData {
  backoffMs?: number;
  times?: number;
}
/**
 * This interface was referenced by `IrDoc`'s JSON-Schema
 * via the `definition` "SetVarData".
 */
export interface SetVarData {
  name: string;
  value: VarValue;
}
/**
 * This interface was referenced by `IrDoc`'s JSON-Schema
 * via the `definition` "CallData".
 */
export interface CallData {
  flow: string;
}
/**
 * This interface was referenced by `IrDoc`'s JSON-Schema
 * via the `definition` "Meta".
 */
export interface Meta {
  game?: string | null;
  maxDurationMs?: number;
  minEngine?: string | null;
  name: string;
}
export interface TemplateTarget {
  type: 'template';
  /**
   * @minItems 2
   * @maxItems 2
   */
  anchor?: [number, number];
  /**
   * SHA-256 of the PNG asset in the local asset directory.
   */
  asset: string;
  assetBox: AssetBox;
  auto?: boolean;
  mask?: 'none' | 'edges';
  multi?: 'nearest-last' | 'highest' | 'center' | 'nth';
  nth?: number | null;
  /**
   * Pixels per logical point of the recording surface.
   */
  recordScale?: number;
  /**
   * Logical width of the surface the crop was taken from. This makes one crop
   * work on a differently sized screen; 0 falls back to the runtime width.
   */
  recordWidth?: number;
  /**
   * Normalized search region `[x, y, w, h]`.
   *
   * @minItems 4
   * @maxItems 4
   */
  roi: [number, number, number, number];
  scale?: ScaleSpec;
  strategy?: Strategy[];
  threshold?: number;
  verify?: 'none' | 'rgb';
  warnThreshold?: number;
}
/**
 * This interface was referenced by `IrDoc`'s JSON-Schema
 * via the `definition` "AssetBox".
 */
export interface AssetBox {
  h: number;
  w: number;
}
/**
 * Scale search ladder. The defaults cover "fixed size UI on a bigger screen"
 * (0.7x) as well as proportional layouts that come out slightly off (1.5x).
 */
export interface ScaleSpec {
  max?: number;
  min?: number;
  mode?: 'auto' | 'fixed';
  steps?: number;
}
export interface PointTarget {
  type: 'point';
  /**
   * @minItems 2
   * @maxItems 2
   */
  norm: [number, number];
  /**
   * @minItems 4
   * @maxItems 4
   */
  zone: [number, number, number, number];
}
/**
 * This interface was referenced by `IrDoc`'s JSON-Schema
 * via the `definition` "Pos".
 */
export interface Pos1 {
  x: number;
  y: number;
}
/**
 * Scale search ladder. The defaults cover "fixed size UI on a bigger screen"
 * (0.7x) as well as proportional layouts that come out slightly off (1.5x).
 *
 * This interface was referenced by `IrDoc`'s JSON-Schema
 * via the `definition` "ScaleSpec".
 */
export interface ScaleSpec1 {
  max?: number;
  min?: number;
  mode?: 'auto' | 'fixed';
  steps?: number;
}
