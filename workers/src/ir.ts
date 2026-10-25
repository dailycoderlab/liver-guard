import { z } from "zod";

/**
 * IR (intermediate representation) schema, v1.
 *
 * Design rules that the schema enforces:
 *  - A script never stores screen coordinates for a tap. It stores a Target
 *    descriptor (a template crop plus where to search and what to click).
 *  - `point` targets are the single explicit escape hatch for steps that have no
 *    visual anchor at all (tap empty space to advance a dialog, and so on), and
 *    they must carry a `zone` so a wrong-layout device cannot be mis-clicked.
 */

export const NormRect = z.tuple([z.number(), z.number(), z.number(), z.number()]);
export const NormPoint = z.tuple([z.number(), z.number()]);

export const ScaleSpec = z.object({
  mode: z.enum(["auto", "fixed"]).default("auto"),
  min: z.number().min(0.2).max(3).default(0.7),
  max: z.number().min(0.2).max(3).default(1.5),
  steps: z.number().int().min(1).max(21).default(9),
});

export const TemplateTarget = z.object({
  type: z.literal("template"),
  /** content address of the crop, `sha256:<hex>` */
  asset: z.string().min(1),
  /** crop size in pixels at record time */
  assetBox: z.object({ w: z.number().positive(), h: z.number().positive() }),
  /** pixels per logical point on the recording device */
  recordScale: z.number().positive().default(1),
  /**
   * Logical width of the recording capture surface. Normalizing the template by
   * the recording width (and the live frame by the current width) is what lets a
   * control keep its relative position across differently sized screens without
   * any scale search.
   */
  recordWidth: z.number().min(0).default(0),
  /** normalized search region on the captured surface */
  roi: NormRect,
  /** where inside the matched box to click, 0..1 */
  anchor: NormPoint.default([0.5, 0.5]),
  strategy: z.array(z.enum(["tpl", "mstpl", "sift"])).default(["tpl", "mstpl"]),
  scale: ScaleSpec.default({}),
  threshold: z.number().min(0).max(1).default(0.82),
  warnThreshold: z.number().min(0).max(1).default(0.7),
  verify: z.enum(["none", "rgb"]).default("rgb"),
  mask: z.enum(["none", "edges"]).default("none"),
  multi: z.enum(["nearest-last", "highest", "center", "nth"]).default("nearest-last"),
  nth: z.number().int().min(1).max(50).optional(),
  /** false when detection had to fall back to a guess, surfaced in the console */
  auto: z.boolean().default(true),
});

export const PointTarget = z.object({
  type: z.literal("point"),
  norm: NormPoint,
  zone: NormRect,
});

export const Target = z.discriminatedUnion("type", [TemplateTarget, PointTarget]);

export type Step = { op: string } & Record<string, unknown>;

const TargetName = z.string().min(1);
const StepId = z.string().optional();

export const Condition: z.ZodType<any> = z.lazy(() =>
  z.union([
    z.object({ visible: z.union([TargetName, z.array(TargetName)]) }),
    z.object({ missing: TargetName }),
    z.object({
      var: z.string(),
      op: z.enum(["eq", "ne", "lt", "le", "gt", "ge", "contains", "match"]),
      value: z.union([z.string(), z.number(), z.boolean()]),
    }),
    z.object({ not: Condition }),
    z.object({ all: z.array(Condition) }),
    z.object({ any: z.array(Condition) }),
  ]),
);

export const Step: z.ZodType<Step> = z.lazy(() =>
  z.union([
    z.object({
      op: z.literal("tap"),
      id: StepId,
      target: TargetName,
      offset: NormPoint.optional(),
      note: z.string().optional(),
    }),
    z.object({
      op: z.literal("longPress"),
      id: StepId,
      target: TargetName,
      durationMs: z.number().int().min(50).max(10000).default(600),
    }),
    z.object({
      op: z.literal("swipe"),
      id: StepId,
      from: TargetName,
      to: TargetName.optional(),
      vector: NormPoint.optional(),
      durationMs: z.number().int().min(50).max(10000).default(300),
    }),
    z.object({ op: z.literal("sleep"), id: StepId, ms: z.number().int().min(0).max(600000) }),
    z.object({
      op: z.literal("waitFor"),
      id: StepId,
      any: z.array(TargetName).min(1),
      timeoutMs: z.number().int().min(100).max(600000).default(10000),
      intervalMs: z.number().int().min(50).max(5000).default(500),
    }),
    z.object({
      op: z.literal("waitGone"),
      id: StepId,
      target: TargetName,
      timeoutMs: z.number().int().min(100).max(600000).default(10000),
      intervalMs: z.number().int().min(50).max(5000).default(500),
    }),
    z.object({
      op: z.literal("waitStable"),
      id: StepId,
      ms: z.number().int().min(50).max(10000).default(400),
      timeoutMs: z.number().int().min(100).max(60000).default(5000),
    }),
    z.object({
      op: z.literal("if"),
      id: StepId,
      when: Condition,
      then: z.array(Step),
      else: z.array(Step).optional(),
    }),
    z.object({
      op: z.literal("loop"),
      id: StepId,
      count: z.number().int().min(1).max(100000).default(1),
      body: z.array(Step),
    }),
    z.object({ op: z.literal("call"), id: StepId, block: z.string().min(1) }),
    z.object({
      op: z.literal("retry"),
      id: StepId,
      times: z.number().int().min(1).max(20).default(3),
      backoffMs: z.number().int().min(0).max(60000).default(500),
      body: z.array(Step),
    }),
    z.object({
      op: z.literal("log"),
      id: StepId,
      level: z.enum(["debug", "info", "warn", "error"]).default("info"),
      msg: z.string(),
    }),
    z.object({ op: z.literal("key"), id: StepId, key: z.string().min(1) }),
    z.object({ op: z.literal("text"), id: StepId, text: z.string() }),
    z.object({ op: z.literal("snapshot"), id: StepId, label: z.string().default("snap") }),
  ]),
);

export const IR = z.object({
  irVersion: z.literal(1),
  meta: z.object({
    name: z.string().min(1),
    game: z.string().optional(),
    minEngine: z.string().optional(),
    maxDurationMs: z.number().int().min(1000).max(86400000).default(1800000),
  }),
  requires: z.array(z.string()).default(["screenshot", "templateMatch", "gesture"]),
  entry: z.string().default("main"),
  vars: z.record(z.union([z.string(), z.number(), z.boolean()])).default({}),
  blocks: z.record(z.array(Step)),
  targets: z.record(Target),
});

export type IR = z.infer<typeof IR>;
export type Target = z.infer<typeof Target>;
export type TemplateTarget = z.infer<typeof TemplateTarget>;
export type PointTarget = z.infer<typeof PointTarget>;

/** Walk every step in every block, including nested bodies. */
export function walkSteps(ir: IR, visit: (step: Step) => void): void {
  for (const block of Object.values(ir.blocks)) {
    walkList(block as Step[], visit);
  }
}

function walkList(steps: Step[], visit: (step: Step) => void): void {
  for (const step of steps) {
    visit(step);
    for (const key of ["body", "then", "else"]) {
      const nested = step[key];
      if (Array.isArray(nested)) walkList(nested as Step[], visit);
    }
  }
}

export function collectTargetRefs(ir: IR): Set<string> {
  const used = new Set<string>();
  const addTarget = (name: unknown) => {
    if (typeof name === "string") used.add(name);
  };
  const walkCondition = (cond: any) => {
    if (!cond || typeof cond !== "object") return;
    if (cond.visible) {
      Array.isArray(cond.visible) ? cond.visible.forEach(addTarget) : addTarget(cond.visible);
    }
    if (cond.missing) addTarget(cond.missing);
    for (const key of ["not"]) if (cond[key]) walkCondition(cond[key]);
    for (const key of ["all", "any"]) if (Array.isArray(cond[key])) cond[key].forEach(walkCondition);
  };

  walkSteps(ir, (step) => {
    addTarget(step.target);
    addTarget(step.from);
    addTarget(step.to);
    if (Array.isArray(step.any)) step.any.forEach(addTarget);
    if (step.when) walkCondition(step.when);
  });
  return used;
}

/** Structural checks the zod schema cannot express. */
export function validateIR(ir: IR): string[] {
  const errors: string[] = [];
  if (!ir.blocks[ir.entry]) errors.push(`entry block "${ir.entry}" does not exist`);

  for (const [name, target] of Object.entries(ir.targets)) {
    if (target.type === "template" && !target.asset) {
      errors.push(`target "${name}" has no asset`);
    }
  }

  for (const ref of collectTargetRefs(ir)) {
    if (!ir.targets[ref]) errors.push(`target "${ref}" is referenced but not defined`);
  }

  const blockNames = new Set(Object.keys(ir.blocks));
  walkSteps(ir, (step) => {
    if (step.op === "call" && typeof step.block === "string" && !blockNames.has(step.block)) {
      errors.push(`call references missing block "${step.block}"`);
    }
    if (step.op === "swipe" && !step.to && !step.vector) {
      errors.push("swipe needs either `to` or `vector`");
    }
  });

  return errors;
}

export function collectAssets(ir: IR): string[] {
  const out = new Set<string>();
  for (const target of Object.values(ir.targets)) {
    if (target.type === "template") out.add(target.asset);
  }
  return [...out];
}
