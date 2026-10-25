export type Bounds = {
  x: number;
  y: number;
  w: number;
  h: number;
  scale: number;
  orientation?: "portrait" | "landscape";
};

export type Caps = {
  maxFps: number;
  injectKinds: string[];
  uiQuery: "ax" | "uia" | "node" | "none";
  maxWindow: [number, number];
  secureScreen: boolean;
  surface: "window" | "screen" | "mirror";
};

export type DeviceInfo = {
  id: string;
  name: string;
  platform: string;
  online: boolean;
  caps?: Caps | null;
  bounds?: Bounds | null;
};

/** Box in window pixels, origin at the window image's top-left. */
export type PixelBox = { x: number; y: number; w: number; h: number };

export type CandidateKind = "ax" | "contour" | "color" | "fallback";

export type Candidate = PixelBox & {
  kind: CandidateKind;
  score: number;
};

export type NormRect = [number, number, number, number];

export type TemplateTarget = {
  type: "template";
  asset: string;
  assetBox: { w: number; h: number };
  recordScale: number;
  recordWidth: number;
  roi: NormRect;
  anchor: [number, number];
  strategy: Array<"tpl" | "mstpl" | "sift">;
  scale: { mode: "auto" | "fixed"; min: number; max: number; steps: number };
  threshold: number;
  warnThreshold: number;
  verify: "none" | "rgb";
  mask: "none" | "edges";
  multi: "nearest-last" | "highest" | "center" | "nth";
  auto: boolean;
};

export type PointTarget = { type: "point"; norm: [number, number]; zone: NormRect };

export type Target = TemplateTarget | PointTarget;

export type Step = { op: string; id?: string; [key: string]: unknown };

export type DraftIR = {
  irVersion: 1;
  meta: { name: string; game?: string; maxDurationMs: number };
  requires: string[];
  entry: string;
  vars: Record<string, string | number | boolean>;
  blocks: Record<string, Step[]>;
  targets: Record<string, Target>;
};

export function emptyDraft(name: string): DraftIR {
  return {
    irVersion: 1,
    meta: { name, maxDurationMs: 1800000 },
    requires: ["screenshot", "templateMatch", "gesture"],
    entry: "main",
    vars: {},
    blocks: { main: [] },
    targets: {},
  };
}

/** A step plus everything the editor needs to render and re-crop it. */
export type StoredStep = {
  step: Step;
  targetKey?: string;
  target?: TemplateTarget;
  /** data URL used for the step list thumbnail */
  thumb?: string;
  box?: { pixel: PixelBox; logical: { x: number; y: number; w: number; h: number } };
  confidence?: CandidateKind;
};

export function isTargetStep(item: StoredStep): item is StoredStep & { targetKey: string; target: TemplateTarget } {
  return !!item.targetKey && !!item.target;
}
