import type { Rect } from '@/api';

export type Point = { x: number; y: number };

export const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));

/** The rectangle spanned by two corner points, in whole pixels. */
export function normalize(a: Point, b: Point): Rect {
  const x = Math.min(a.x, b.x);
  const y = Math.min(a.y, b.y);
  return { x: Math.round(x), y: Math.round(y), w: Math.round(Math.abs(a.x - b.x)), h: Math.round(Math.abs(a.y - b.y)) };
}

/** Eight grips around the selection; fx/fy say which edges a grip moves (0 = low edge, 1 = high edge, 0.5 = neither). */
export const HANDLES = [
  { id: 'nw', fx: 0, fy: 0, cursor: 'cursor-nwse-resize' },
  { id: 'n', fx: 0.5, fy: 0, cursor: 'cursor-ns-resize' },
  { id: 'ne', fx: 1, fy: 0, cursor: 'cursor-nesw-resize' },
  { id: 'e', fx: 1, fy: 0.5, cursor: 'cursor-ew-resize' },
  { id: 'se', fx: 1, fy: 1, cursor: 'cursor-nwse-resize' },
  { id: 's', fx: 0.5, fy: 1, cursor: 'cursor-ns-resize' },
  { id: 'sw', fx: 0, fy: 1, cursor: 'cursor-nesw-resize' },
  { id: 'w', fx: 0, fy: 0.5, cursor: 'cursor-ew-resize' },
] as const;
export type Handle = (typeof HANDLES)[number];

/** Drag `handle` of `orig` to `to`: only the edges that grip owns follow the pointer. */
export function resizeRect(orig: Rect, handle: Handle, to: Point): Rect {
  let x0 = orig.x;
  let x1 = orig.x + orig.w;
  let y0 = orig.y;
  let y1 = orig.y + orig.h;
  if (handle.fx === 0) x0 = to.x;
  if (handle.fx === 1) x1 = to.x;
  if (handle.fy === 0) y0 = to.y;
  if (handle.fy === 1) y1 = to.y;
  return normalize({ x: x0, y: y0 }, { x: x1, y: y1 });
}
