// Scatter of a human click's landing point, as a multiple of the standard spread.
// The bounds mirror `MIN_SPREAD` / `MAX_SPREAD` / `DEFAULT_SPREAD` in
// `crates/liverguard-ir/src/node.rs`; `spread.test.ts` pins the numbers.

export const DEFAULT_SPREAD = 1;
export const MIN_SPREAD = 0.3;
export const MAX_SPREAD = 2;

/** A spread the engine accepts: non-numbers become the default, the rest is kept in range. */
export function clampSpread(value: unknown): number {
  if (typeof value !== 'number' || !Number.isFinite(value)) return DEFAULT_SPREAD;
  return Math.min(MAX_SPREAD, Math.max(MIN_SPREAD, value));
}

/** Plain-words reading of a spread, shown next to the number. */
export function describeSpread(value: unknown): string {
  const spread = clampSpread(value);
  if (spread < 0.75) return '集中';
  if (spread <= 1.25) return spread === DEFAULT_SPREAD ? '标准（推荐）' : '接近标准';
  return '分散';
}
