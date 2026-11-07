// The sleep node stores its range in whole milliseconds (the engine draws a random
// millisecond inside it); the editor shows and accepts seconds.

export type SleepRange = { minMs: number; maxMs: number };

export const secondsToMs = (seconds: number) => Math.max(0, Math.round(seconds * 1000));

export const msToSeconds = (ms: number) => ms / 1000;

/**
 * Params to write while one bound is being typed. The other bound is left alone: a
 * half-typed value (`3` on the way to `30`) must not drag it along. `settleSleep` fixes
 * min > max once editing is over.
 */
export function sleepPatch(current: SleepRange, edit: Partial<{ minSeconds: number; maxSeconds: number }>): SleepRange {
  return {
    minMs: edit.minSeconds === undefined ? current.minMs : secondsToMs(edit.minSeconds),
    maxMs: edit.maxSeconds === undefined ? current.maxMs : secondsToMs(edit.maxSeconds),
  };
}

/** After editing `edited` is finished: if the range is reversed, the bound just edited wins and the other follows. */
export function settleSleep(current: SleepRange, edited: 'min' | 'max'): SleepRange {
  if (current.minMs <= current.maxMs) return current;
  return edited === 'min' ? { minMs: current.minMs, maxMs: current.minMs } : { minMs: current.maxMs, maxMs: current.maxMs };
}

/** `1 秒` for a fixed wait, `1–3 秒` for a range. */
export function formatSleepRange(minMs: number, maxMs: number): string {
  const s = (ms: number) => String(msToSeconds(ms));
  return minMs === maxMs ? `${s(minMs)} 秒` : `${s(minMs)}–${s(maxMs)} 秒`;
}
