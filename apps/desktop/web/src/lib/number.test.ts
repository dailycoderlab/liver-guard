import { describe, expect, it } from 'vitest';
import { clampTo, isNumberDraft, parseDraft, stepNumber } from './number';

describe('number draft', () => {
  it('accepts half-typed numbers and rejects letters', () => {
    for (const ok of ['', '-', '1', '1.', '.5', '-0.25']) expect(isNumberDraft(ok), ok).toBe(true);
    for (const bad of ['a', '1e3', '1.2.3', '--1', ' 1']) expect(isNumberDraft(bad), bad).toBe(false);
  });

  it('parses only complete numbers', () => {
    expect(parseDraft('')).toBeNull();
    expect(parseDraft('-')).toBeNull();
    expect(parseDraft('.')).toBeNull();
    expect(parseDraft('1.')).toBe(1);
    expect(parseDraft('.5')).toBe(0.5);
    expect(parseDraft('-2.5')).toBe(-2.5);
  });
});

describe('stepping', () => {
  it('has no float noise', () => {
    expect(stepNumber(0.1, 1, 0.2)).toBe(0.3);
    expect(stepNumber(0.85, 1, 0.01, 0, 1)).toBe(0.86);
    expect(stepNumber(1.234, 1, 0.1)).toBe(1.334);
  });

  it('stays inside min and max', () => {
    expect(stepNumber(0, -1, 1, 0)).toBe(0);
    expect(stepNumber(1, 1, 0.05, 0, 1)).toBe(1);
    expect(clampTo(5, 0, 1)).toBe(1);
    expect(clampTo(-5, 0)).toBe(0);
  });
});
