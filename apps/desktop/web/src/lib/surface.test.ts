import { describe, expect, it } from 'vitest';
import type { WindowInfo } from '@/api';
import { checkSurface, parseSurface, surfaceLabel, surfaceValue } from './surface';

const win = (number: number, label: string): WindowInfo => ({ number, owner: 'o', title: 't', bounds: { x: 0, y: 0, w: 1, h: 1 }, label });
const windows = [win(101, '#101 Game'), win(102, '#102 Launcher')];

describe('surface selection', () => {
  it('maps the select value both ways and treats empty as nothing chosen', () => {
    expect(surfaceValue(null)).toBe('');
    expect(surfaceValue({ kind: 'fullScreen' })).toBe('full');
    expect(surfaceValue({ kind: 'window', number: 101 })).toBe('101');
    expect(parseSurface('')).toBeNull();
    expect(parseSurface('full')).toEqual({ kind: 'fullScreen' });
    expect(parseSurface('102')).toEqual({ kind: 'window', number: 102 });
    expect(parseSurface('abc')).toBeNull();
  });

  it('refuses to start without a choice, or on a window that has gone away', () => {
    expect(checkSurface(null, windows)).toBe('none');
    expect(checkSurface({ kind: 'fullScreen' }, [])).toBe('ok');
    expect(checkSurface({ kind: 'window', number: 101 }, windows)).toBe('ok');
    expect(checkSurface({ kind: 'window', number: 999 }, windows)).toBe('gone');
  });

  it('names the surface for display', () => {
    expect(surfaceLabel(null, windows)).toBe('未选择');
    expect(surfaceLabel({ kind: 'fullScreen' }, windows)).toBe('整个屏幕');
    expect(surfaceLabel({ kind: 'window', number: 102 }, windows)).toBe('#102 Launcher');
    expect(surfaceLabel({ kind: 'window', number: 5 }, windows)).toContain('已不存在');
  });
});
