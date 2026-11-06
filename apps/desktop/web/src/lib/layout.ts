/** Sizes of the three resizable panels, in CSS pixels. */
export interface PanelSizes {
  leftWidth: number;
  rightWidth: number;
  bottomHeight: number;
}

export const DEFAULT_SIZES: PanelSizes = { leftWidth: 248, rightWidth: 340, bottomHeight: 220 };

/** Bounds per panel. The upper bounds are also capped by the window, see `Splitter`. */
export const LIMITS = {
  leftWidth: { min: 200, max: 520 },
  rightWidth: { min: 280, max: 700 },
  bottomHeight: { min: 96, max: 720 },
} as const;

/** The middle column never gets narrower or shorter than this. */
export const MIN_CENTER_WIDTH = 360;
export const MIN_CENTER_HEIGHT = 200;

const STORAGE_KEY = 'sky.layout.v1';

export function clamp(value: number, min: number, max: number): number {
  return Math.min(Math.max(value, min), Math.max(min, max));
}

type Key = keyof PanelSizes;

/** Keep a size inside its own limits. */
export function clampPanel(key: Key, value: number): number {
  return clamp(Math.round(value), LIMITS[key].min, LIMITS[key].max);
}

/** Read saved sizes; anything missing, non-numeric or out of range falls back. */
export function loadSizes(storage: Pick<Storage, 'getItem'> | undefined = safeStorage()): PanelSizes {
  const sizes = { ...DEFAULT_SIZES };
  try {
    const raw = storage?.getItem(STORAGE_KEY);
    if (!raw) return sizes;
    const parsed = JSON.parse(raw) as Partial<Record<Key, unknown>>;
    for (const key of Object.keys(sizes) as Key[]) {
      const value = parsed[key];
      if (typeof value === 'number' && Number.isFinite(value)) sizes[key] = clampPanel(key, value);
    }
  } catch {
    // Corrupt data just means defaults.
  }
  return sizes;
}

export function saveSizes(sizes: PanelSizes, storage: Pick<Storage, 'setItem'> | undefined = safeStorage()) {
  try {
    storage?.setItem(STORAGE_KEY, JSON.stringify(sizes));
  } catch {
    // Storage can be full or blocked; losing the layout is harmless.
  }
}

function safeStorage(): Storage | undefined {
  try {
    return typeof localStorage === 'undefined' ? undefined : localStorage;
  } catch {
    return undefined;
  }
}
