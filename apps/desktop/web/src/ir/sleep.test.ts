import { describe, expect, it } from 'vitest';
import { formatSleepRange, msToSeconds, secondsToMs, settleSleep, sleepPatch } from './sleep';

describe('sleep range', () => {
  it('converts seconds to whole milliseconds without float noise', () => {
    expect(secondsToMs(1.234)).toBe(1234);
    expect(secondsToMs(0.001)).toBe(1);
    expect(secondsToMs(0.0004)).toBe(0);
    expect(secondsToMs(-2)).toBe(0);
    expect(msToSeconds(1500)).toBe(1.5);
  });

  it('changes only the bound being typed', () => {
    const base = { minMs: 52000, maxMs: 11212000 };
    // Typing 30 into max passes through 3, which is below min: min must not move.
    expect(sleepPatch(base, { maxSeconds: 3 })).toEqual({ minMs: 52000, maxMs: 3000 });
    expect(sleepPatch(base, { maxSeconds: 30 })).toEqual({ minMs: 52000, maxMs: 30000 });
    expect(sleepPatch(base, { minSeconds: 2 })).toEqual({ minMs: 2000, maxMs: 11212000 });
  });

  it('settles a reversed range in favour of the bound just edited', () => {
    const reversed = { minMs: 5000, maxMs: 3000 };
    expect(settleSleep(reversed, 'min')).toEqual({ minMs: 5000, maxMs: 5000 });
    expect(settleSleep(reversed, 'max')).toEqual({ minMs: 3000, maxMs: 3000 });
    const fine = { minMs: 1000, maxMs: 3000 };
    expect(settleSleep(fine, 'min')).toBe(fine);
  });

  it('describes a fixed wait and a range', () => {
    expect(formatSleepRange(1000, 1000)).toBe('1 秒');
    expect(formatSleepRange(500, 2500)).toBe('0.5–2.5 秒');
  });
});
