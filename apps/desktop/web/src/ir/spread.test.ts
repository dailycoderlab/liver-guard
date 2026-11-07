import { describe, expect, it } from 'vitest';
import { NODE_SPECS } from './catalog';
import { clampSpread, DEFAULT_SPREAD, describeSpread, MAX_SPREAD, MIN_SPREAD } from './spread';

describe('spread', () => {
  it('matches the engine bounds', () => {
    expect([MIN_SPREAD, DEFAULT_SPREAD, MAX_SPREAD]).toEqual([0.3, 1, 2]);
  });

  it('keeps values in range and falls back to the default for non-numbers', () => {
    expect(clampSpread(1.4)).toBe(1.4);
    expect(clampSpread(0)).toBe(MIN_SPREAD);
    expect(clampSpread(9)).toBe(MAX_SPREAD);
    expect(clampSpread(undefined)).toBe(DEFAULT_SPREAD);
    expect(clampSpread(Number.NaN)).toBe(DEFAULT_SPREAD);
  });

  it('describes the level in words', () => {
    expect(describeSpread(0.5)).toBe('集中');
    expect(describeSpread(1)).toBe('标准（推荐）');
    expect(describeSpread(1.1)).toBe('接近标准');
    expect(describeSpread(1.8)).toBe('分散');
  });

  it('new click and swipe nodes start at the recommended spread', () => {
    for (const type of ['tap', 'longPress', 'swipe'] as const) {
      expect(NODE_SPECS[type].defaults().spread).toBe(DEFAULT_SPREAD);
    }
  });
});
