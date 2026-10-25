import type { Bounds, NormRect, PixelBox } from "./types";

export type LogicalBox = { x: number; y: number; w: number; h: number };

/** Window pixel box -> global logical points (Quartz space). */
export function pixelToLogical(box: PixelBox, window: Bounds, scale: number): LogicalBox {
  return {
    x: window.x + box.x / scale,
    y: window.y + box.y / scale,
    w: box.w / scale,
    h: box.h / scale,
  };
}

export function logicalToPixel(box: LogicalBox, window: Bounds, scale: number): PixelBox {
  return {
    x: (box.x - window.x) * scale,
    y: (box.y - window.y) * scale,
    w: box.w * scale,
    h: box.h * scale,
  };
}

/**
 * Where inside the matched box to click. Kept as a continuous ratio rather than
 * a centre point so "click the left third of a long button" survives a change of
 * screen size.
 */
export function computeAnchor(clickLogical: { x: number; y: number }, box: LogicalBox): [number, number] {
  const ax = box.w > 0 ? (clickLogical.x - box.x) / box.w : 0.5;
  const ay = box.h > 0 ? (clickLogical.y - box.y) / box.h : 0.5;
  return [clamp(ax, 0.1, 0.9), clamp(ay, 0.1, 0.9)];
}

/**
 * Search region for a target: the 3x5 grid cell the control sits in, grown by one
 * cell on every side, then unioned with the control's own box so a control on
 * the very edge of the surface is still inside its own search region.
 */
export function inferRoi(box: LogicalBox, bounds: Bounds, cols = 3, rows = 5): NormRect {
  const cx = clamp((box.x + box.w / 2 - bounds.x) / bounds.w, 0, 1);
  const cy = clamp((box.y + box.h / 2 - bounds.y) / bounds.h, 0, 1);

  const col = Math.min(cols - 1, Math.floor(cx * cols));
  const row = Math.min(rows - 1, Math.floor(cy * rows));

  const c0 = Math.max(0, col - 1) / cols;
  const c1 = Math.min(cols, col + 2) / cols;
  const r0 = Math.max(0, row - 1) / rows;
  const r1 = Math.min(rows, row + 2) / rows;

  const boxX0 = clamp((box.x - bounds.x) / bounds.w, 0, 1);
  const boxY0 = clamp((box.y - bounds.y) / bounds.h, 0, 1);
  const boxX1 = clamp((box.x + box.w - bounds.x) / bounds.w, 0, 1);
  const boxY1 = clamp((box.y + box.h - bounds.y) / bounds.h, 0, 1);

  const margin = 0.02;
  const x = clamp(Math.min(c0, boxX0) - margin, 0, 1);
  const y = clamp(Math.min(r0, boxY0) - margin, 0, 1);
  const x1 = clamp(Math.max(c1, boxX1) + margin, 0, 1);
  const y1 = clamp(Math.max(r1, boxY1) + margin, 0, 1);

  return [round(x, 4), round(y, 4), round(Math.max(0.02, x1 - x), 4), round(Math.max(0.02, y1 - y), 4)];
}

/** Click point in normalized frame coordinates -> logical points. */
export function normalizedToLogical(n: [number, number], bounds: Bounds): { x: number; y: number } {
  return { x: bounds.x + n[0] * bounds.w, y: bounds.y + n[1] * bounds.h };
}

export async function cropToPng(source: ImageBitmap, box: PixelBox): Promise<Blob> {
  const x = Math.max(0, Math.round(box.x));
  const y = Math.max(0, Math.round(box.y));
  const w = Math.max(1, Math.min(Math.round(box.w), source.width - x));
  const h = Math.max(1, Math.min(Math.round(box.h), source.height - y));

  const canvas = document.createElement("canvas");
  canvas.width = w;
  canvas.height = h;
  const ctx = canvas.getContext("2d");
  if (!ctx) throw new Error("canvas 2d context unavailable");
  ctx.drawImage(source, x, y, w, h, 0, 0, w, h);

  return new Promise((resolve, reject) => {
    canvas.toBlob((blob) => (blob ? resolve(blob) : reject(new Error("toBlob failed"))), "image/png");
  });
}

export async function sha256Hex(blob: Blob): Promise<string> {
  const digest = await crypto.subtle.digest("SHA-256", await blob.arrayBuffer());
  return [...new Uint8Array(digest)].map((b) => b.toString(16).padStart(2, "0")).join("");
}

export function blobToDataUrl(blob: Blob): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(String(reader.result));
    reader.onerror = () => reject(reader.error);
    reader.readAsDataURL(blob);
  });
}

export function clamp(value: number, min: number, max: number): number {
  return Math.max(min, Math.min(max, value));
}

export function round(value: number, digits = 3): number {
  const k = 10 ** digits;
  return Math.round(value * k) / k;
}
