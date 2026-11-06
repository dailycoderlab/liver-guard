import { describe, expect, it } from 'vitest';
import { clamp, clampPanel, DEFAULT_SIZES, LIMITS, loadSizes, saveSizes } from './layout';

function memory(initial?: string) {
  let value = initial ?? null;
  return {
    getItem: () => value,
    setItem: (_: string, v: string) => {
      value = v;
    },
  };
}

describe('clamp', () => {
  it('keeps values inside the range', () => {
    expect(clamp(5, 10, 20)).toBe(10);
    expect(clamp(25, 10, 20)).toBe(20);
    expect(clamp(15, 10, 20)).toBe(15);
  });

  it('prefers the minimum when the window leaves no room', () => {
    expect(clamp(300, 200, 150)).toBe(200);
  });
});

describe('clampPanel', () => {
  it('rounds and limits per panel', () => {
    expect(clampPanel('leftWidth', 10)).toBe(LIMITS.leftWidth.min);
    expect(clampPanel('rightWidth', 5000)).toBe(LIMITS.rightWidth.max);
    expect(clampPanel('bottomHeight', 200.6)).toBe(201);
  });
});

describe('saved layout', () => {
  it('round-trips', () => {
    const store = memory();
    saveSizes({ leftWidth: 300, rightWidth: 400, bottomHeight: 180 }, store);
    expect(loadSizes(store)).toEqual({ leftWidth: 300, rightWidth: 400, bottomHeight: 180 });
  });

  it('falls back to defaults for missing or corrupt data', () => {
    expect(loadSizes(memory())).toEqual(DEFAULT_SIZES);
    expect(loadSizes(memory('not json'))).toEqual(DEFAULT_SIZES);
    expect(loadSizes(undefined as never)).toEqual(DEFAULT_SIZES);
  });

  it('ignores bad fields and clamps stale ones', () => {
    const loaded = loadSizes(memory(JSON.stringify({ leftWidth: 'wide', rightWidth: 99999, bottomHeight: null })));
    expect(loaded.leftWidth).toBe(DEFAULT_SIZES.leftWidth);
    expect(loaded.rightWidth).toBe(LIMITS.rightWidth.max);
    expect(loaded.bottomHeight).toBe(DEFAULT_SIZES.bottomHeight);
  });
});
