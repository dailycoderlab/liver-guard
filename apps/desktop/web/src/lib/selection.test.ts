import { describe, expect, it } from 'vitest';
import { HANDLES, normalize, resizeRect } from './selection';

const handle = (id: string) => HANDLES.find((h) => h.id === id)!;

describe('selection', () => {
  it('normalizes two corners in any order to whole pixels', () => {
    expect(normalize({ x: 10.4, y: 20.6 }, { x: 4, y: 30 })).toEqual({ x: 4, y: 21, w: 6, h: 9 });
  });

  it('moves only the edges a grip owns', () => {
    const rect = { x: 10, y: 10, w: 20, h: 20 };
    expect(resizeRect(rect, handle('e'), { x: 50, y: 99 })).toEqual({ x: 10, y: 10, w: 40, h: 20 });
    expect(resizeRect(rect, handle('nw'), { x: 0, y: 5 })).toEqual({ x: 0, y: 5, w: 30, h: 25 });
  });

  it('flips when a grip is dragged past the opposite edge', () => {
    expect(resizeRect({ x: 10, y: 10, w: 20, h: 20 }, handle('w'), { x: 40, y: 0 })).toEqual({ x: 30, y: 10, w: 10, h: 20 });
  });
});
