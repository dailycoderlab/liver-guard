// Helpers behind `NumberInput`: it edits a text draft (so a field can be emptied
// while typing) and draws its own steppers (WebKit hides the native ones).

/** Characters a half-typed number may contain: `-`, `1.`, `.5`, `` ... */
export const isNumberDraft = (text: string) => /^-?\d*\.?\d*$/.test(text);

/** The number a draft stands for, or null while it is empty or incomplete (`-`, `.`). */
export function parseDraft(text: string): number | null {
  if (text.trim() === '') return null;
  const value = Number(text);
  return Number.isFinite(value) ? value : null;
}

export const clampTo = (value: number, min?: number, max?: number) => Math.min(max ?? Infinity, Math.max(min ?? -Infinity, value));

const decimalsOf = (n: number) => {
  const text = String(n);
  const dot = text.indexOf('.');
  return dot < 0 ? 0 : text.length - dot - 1;
};

/** One step up or down, without float noise (`0.1 + 0.2` stays `0.3`) and inside min/max. */
export function stepNumber(value: number, direction: 1 | -1, step = 1, min?: number, max?: number): number {
  const digits = Math.max(decimalsOf(step), decimalsOf(value));
  return clampTo(Number((value + direction * step).toFixed(digits)), min, max);
}
