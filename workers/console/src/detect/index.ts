import type { Candidate, PixelBox } from "../types";
import {
  borderCoverage,
  boxBlur,
  connectedComponents,
  floodFillRegion,
  grayscaleVariance,
  histogram,
  morphClose,
  normalizeToByte,
  otsuThreshold,
  scharrMagnitude,
  threshold,
  toGray,
  type Component,
} from "./primitives";

export type DetectOptions = {
  /** device pixels per logical point in this image */
  scale: number;
  /** smallest sensible control, logical points */
  minSide?: number;
  /** largest sensible control, logical points */
  maxSide?: number;
  /** fraction of the box added on every side before cropping */
  padding?: number;
  strictness?: "loose" | "normal" | "tight";
  /** pixel box reported by the platform's native UI query, if any */
  axBox?: PixelBox | null;
};

const DEFAULTS = {
  minSide: 24,
  maxSide: 220,
  padding: 0.08,
};

/**
 * Find the control under the click point.
 *
 * Order matters and mirrors the plan: native UI element first (exact), then edge
 * based contours (works on self drawn game UI), then solid colour regions, and
 * finally a centred guess flagged `fallback` so the console can ask for a manual
 * box instead of silently storing a template that will never match.
 */
export function detectCandidates(
  rgba: Uint8ClampedArray,
  width: number,
  height: number,
  click: { x: number; y: number },
  options: DetectOptions,
): Candidate[] {
  const minSide = options.minSide ?? DEFAULTS.minSide;
  const maxSide = options.maxSide ?? DEFAULTS.maxSide;
  const padding = options.padding ?? DEFAULTS.padding;
  const strictness = options.strictness ?? "normal";
  const scale = options.scale || 1;

  const minSidePx = minSide * scale;
  const maxSidePx = maxSide * scale;

  const candidates: Candidate[] = [];

  if (options.axBox && options.axBox.w > 0 && options.axBox.h > 0) {
    candidates.push({ ...options.axBox, kind: "ax", score: 1 });
  }

  // Work on an integer downscale when the window is huge; boxes scale back up.
  const downsample = width * height > 2_500_000 ? 2 : 1;
  const w = Math.floor(width / downsample);
  const h = Math.floor(height / downsample);
  const src = downsample === 1 ? rgba : subsample(rgba, width, height, w, h, downsample);
  const clickX = click.x / downsample;
  const clickY = click.y / downsample;

  const gray = toGray(src, w, h);
  const blurred = boxBlur(gray, w, h);
  const mag = normalizeToByte(scharrMagnitude(blurred, w, h));

  const otsu = otsuThreshold(histogram(mag), mag.length);
  // Nudge the threshold with the strictness knob: loose finds more edges,
  // tight keeps only strong ones.
  const bias = strictness === "loose" ? -12 : strictness === "tight" ? 14 : 0;
  const binary = morphClose(threshold(mag, Math.max(4, Math.min(250, otsu + bias))), w, h);

  const minAreaPx = Math.max(12, (minSidePx / downsample) * (minSidePx / downsample) * 0.35);
  const components = connectedComponents(binary, w, h, minAreaPx);

  const fit = (component: Component, kind: Candidate["kind"]): Candidate | null => {
    const x0 = component.x0;
    const y0 = component.y0;
    const x1 = component.x1;
    const y1 = component.y1;
    const boxW = x1 - x0 + 1;
    const boxH = y1 - y0 + 1;

    if (clickX < x0 || clickX > x1 || clickY < y0 || clickY > y1) return null;
    if (boxW < minSidePx / downsample || boxH < minSidePx / downsample) return null;
    if (boxW > maxSidePx / downsample || boxH > maxSidePx / downsample) return null;

    const aspect = boxW / boxH;
    if (aspect < 0.25 || aspect > 4) return null;

    // Rectangularity is either "solid rectangle" or "rectangle outline": an
    // outline style button is mostly empty space inside its border, which a
    // plain area/bbox ratio would reject.
    const fill = component.area / (boxW * boxH);
    const border = borderCoverage(binary, w, h, { x0, y0, x1, y1 });
    const rectangularity = Math.max(fill, border);
    if (rectangularity < (strictness === "tight" ? 0.75 : strictness === "loose" ? 0.45 : 0.6)) return null;

    const variance = grayscaleVariance(gray, w, { x: x0, y: y0, w: boxW, h: boxH });
    if (variance < 24) return null;

    return {
      x: x0 * downsample,
      y: y0 * downsample,
      w: boxW * downsample,
      h: boxH * downsample,
      kind,
      score: Math.min(0.95, rectangularity),
    };
  };

  const contourHits = components
    .map((component) => fit(component, "contour"))
    .filter((candidate): candidate is Candidate => candidate !== null)
    .sort((a, b) => b.score - a.score)
    .slice(0, 2);
  candidates.push(...contourHits);

  if (contourHits.length === 0) {
    const tolerance = strictness === "loose" ? 28 : strictness === "tight" ? 10 : 18;
    const region = floodFillRegion(
      src,
      w,
      h,
      Math.round(clickX),
      Math.round(clickY),
      tolerance,
      Math.round((maxSidePx / downsample) ** 2),
    );
    if (region) {
      const color = fit(region, "color");
      if (color) candidates.push({ ...color, score: Math.min(0.7, color.score * 0.8) });
    }
  }

  const fallbackSide = Math.min(maxSidePx, Math.max(64 * scale, minSidePx * 2));
  candidates.push({
    x: click.x - fallbackSide / 2,
    y: click.y - fallbackSide / 2,
    w: fallbackSide,
    h: fallbackSide,
    kind: "fallback",
    score: 0.2,
  });

  return dedupe(candidates, width, height, maxSidePx, padding);
}

/** Expand by padding, clamp into the image, drop near duplicates. */
function dedupe(candidates: Candidate[], width: number, height: number, maxSidePx: number, padding: number): Candidate[] {
  const out: Candidate[] = [];
  for (const candidate of candidates) {
    // A native element frame is already exact: padding it would only make the
    // template include neighbouring UI.
    const isExact = candidate.kind === "ax";
    const grow = isExact ? 0 : Math.max(candidate.w, candidate.h) * padding;
    let box: PixelBox = {
      x: candidate.x - grow,
      y: candidate.y - grow,
      w: candidate.w + grow * 2,
      h: candidate.h + grow * 2,
    };

    if (!isExact && (box.w > maxSidePx || box.h > maxSidePx)) {
      const shrink = maxSidePx / Math.max(box.w, box.h);
      const cx = box.x + box.w / 2;
      const cy = box.y + box.h / 2;
      box = { x: cx - (box.w * shrink) / 2, y: cy - (box.h * shrink) / 2, w: box.w * shrink, h: box.h * shrink };
    }

    box.x = Math.max(0, Math.min(width - 8, box.x));
    box.y = Math.max(0, Math.min(height - 8, box.y));
    box.w = Math.min(box.w, width - box.x);
    box.h = Math.min(box.h, height - box.y);

    const rounded: PixelBox = {
      x: Math.round(box.x),
      y: Math.round(box.y),
      w: Math.round(box.w),
      h: Math.round(box.h),
    };

    const duplicate = out.some((existing) => iou(existing, rounded) > 0.75);
    if (!duplicate) out.push({ ...candidate, ...rounded });
  }
  return out;
}

export function iou(a: PixelBox, b: PixelBox): number {
  const x0 = Math.max(a.x, b.x);
  const y0 = Math.max(a.y, b.y);
  const x1 = Math.min(a.x + a.w, b.x + b.w);
  const y1 = Math.min(a.y + a.h, b.y + b.h);
  if (x1 <= x0 || y1 <= y0) return 0;
  const inter = (x1 - x0) * (y1 - y0);
  return inter / (a.w * a.h + b.w * b.h - inter);
}

function subsample(
  rgba: Uint8ClampedArray,
  width: number,
  height: number,
  w: number,
  h: number,
  factor: number,
): Uint8ClampedArray {
  const out = new Uint8ClampedArray(w * h * 4);
  for (let y = 0; y < h; y++) {
    for (let x = 0; x < w; x++) {
      const src = ((y * factor) * width + x * factor) * 4;
      const dst = (y * w + x) * 4;
      out[dst] = rgba[src];
      out[dst + 1] = rgba[src + 1];
      out[dst + 2] = rgba[src + 2];
      out[dst + 3] = 255;
    }
  }
  return out;
}
