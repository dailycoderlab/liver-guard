import type { Surface, WindowInfo } from '@/api';

/** The value of the surface `<select>`; the empty string is "nothing chosen yet". */
export const surfaceValue = (surface: Surface | null): string => (!surface ? '' : surface.kind === 'fullScreen' ? 'full' : String(surface.number));

export function parseSurface(value: string): Surface | null {
  if (value === 'full') return { kind: 'fullScreen' };
  const number = Number(value);
  return value !== '' && Number.isFinite(number) ? { kind: 'window', number } : null;
}

export type SurfaceCheck = 'ok' | 'none' | 'gone';

/**
 * Whether a run / probe / preview can start on `surface`. A window id is only good while that
 * window exists: closing and reopening the target gives it a new id.
 */
export function checkSurface(surface: Surface | null, windows: WindowInfo[]): SurfaceCheck {
  if (!surface) return 'none';
  if (surface.kind === 'fullScreen') return 'ok';
  return windows.some((w) => w.number === surface.number) ? 'ok' : 'gone';
}

/** A short name for the surface, for places that only have room for text. */
export function surfaceLabel(surface: Surface | null, windows: WindowInfo[]): string {
  if (!surface) return '未选择';
  if (surface.kind === 'fullScreen') return '整个屏幕';
  return windows.find((w) => w.number === surface.number)?.label ?? `窗口 #${surface.number}（已不存在）`;
}
