/**
 * Minimal image primitives for UI element detection, in plain TypeScript.
 *
 * Deliberately no OpenCV/WASM: gradient, threshold and connected components are
 * all a UI detector needs, and a 500 line module keeps the console fast to load
 * and easy to debug. Everything here works on plain typed arrays so it can be
 * unit tested in Node without a canvas.
 */

export function toGray(rgba: Uint8ClampedArray, w: number, h: number): Uint8Array {
  const out = new Uint8Array(w * h);
  for (let i = 0, p = 0; i < out.length; i++, p += 4) {
    // Rec. 601 luma, integer weights to stay in integer math.
    out[i] = (rgba[p] * 77 + rgba[p + 1] * 150 + rgba[p + 2] * 29) >> 8;
  }
  return out;
}

export function boxBlur(gray: Uint8Array, w: number, h: number): Uint8Array {
  const out = new Uint8Array(w * h);
  for (let y = 0; y < h; y++) {
    for (let x = 0; x < w; x++) {
      let sum = 0;
      let n = 0;
      for (let dy = -1; dy <= 1; dy++) {
        const yy = y + dy;
        if (yy < 0 || yy >= h) continue;
        for (let dx = -1; dx <= 1; dx++) {
          const xx = x + dx;
          if (xx < 0 || xx >= w) continue;
          sum += gray[yy * w + xx];
          n++;
        }
      }
      out[y * w + x] = (sum / n) | 0;
    }
  }
  return out;
}

/** Scharr gradient magnitude, which reacts better than Sobel to thin UI edges. */
export function scharrMagnitude(gray: Uint8Array, w: number, h: number): Float32Array {
  const mag = new Float32Array(w * h);
  for (let y = 1; y < h - 1; y++) {
    for (let x = 1; x < w - 1; x++) {
      const i = y * w + x;
      const a = gray[i - w - 1];
      const b = gray[i - w];
      const c = gray[i - w + 1];
      const d = gray[i - 1];
      const f = gray[i + 1];
      const g = gray[i + w - 1];
      const hh = gray[i + w];
      const k = gray[i + w + 1];
      const gx = 3 * (c - a) + 10 * (f - d) + 3 * (k - g);
      const gy = 3 * (g - a) + 10 * (hh - b) + 3 * (k - c);
      mag[i] = Math.abs(gx) + Math.abs(gy);
    }
  }
  return mag;
}

export function normalizeToByte(values: Float32Array): Uint8Array {
  let max = 0;
  for (let i = 0; i < values.length; i++) if (values[i] > max) max = values[i];
  const out = new Uint8Array(values.length);
  if (max <= 0) return out;
  const k = 255 / max;
  for (let i = 0; i < values.length; i++) out[i] = (values[i] * k) | 0;
  return out;
}

export function histogram(bytes: Uint8Array): Uint32Array {
  const hist = new Uint32Array(256);
  for (let i = 0; i < bytes.length; i++) hist[bytes[i]]++;
  return hist;
}

/** Otsu's method: pick the threshold that maximises between-class variance. */
export function otsuThreshold(hist: Uint32Array, total: number): number {
  let sum = 0;
  for (let t = 0; t < 256; t++) sum += t * hist[t];

  let sumB = 0;
  let wB = 0;
  let best = 0;
  let bestVar = -1;

  for (let t = 0; t < 256; t++) {
    wB += hist[t];
    if (wB === 0) continue;
    const wF = total - wB;
    if (wF === 0) break;
    sumB += t * hist[t];
    const mB = sumB / wB;
    const mF = (sum - sumB) / wF;
    const between = wB * wF * (mB - mF) * (mB - mF);
    if (between > bestVar) {
      bestVar = between;
      best = t;
    }
  }
  return best;
}

export function threshold(bytes: Uint8Array, value: number): Uint8Array {
  const out = new Uint8Array(bytes.length);
  for (let i = 0; i < bytes.length; i++) out[i] = bytes[i] > value ? 1 : 0;
  return out;
}

function dilate(bin: Uint8Array, w: number, h: number): Uint8Array {
  const out = new Uint8Array(bin.length);
  for (let y = 0; y < h; y++) {
    for (let x = 0; x < w; x++) {
      const i = y * w + x;
      if (bin[i]) {
        out[i] = 1;
        continue;
      }
      let hit = 0;
      for (let dy = -1; dy <= 1 && !hit; dy++) {
        const yy = y + dy;
        if (yy < 0 || yy >= h) continue;
        for (let dx = -1; dx <= 1; dx++) {
          const xx = x + dx;
          if (xx < 0 || xx >= w) continue;
          if (bin[yy * w + xx]) {
            hit = 1;
            break;
          }
        }
      }
      out[i] = hit;
    }
  }
  return out;
}

function erode(bin: Uint8Array, w: number, h: number): Uint8Array {
  const out = new Uint8Array(bin.length);
  for (let y = 0; y < h; y++) {
    for (let x = 0; x < w; x++) {
      const i = y * w + x;
      if (!bin[i]) continue;
      let all = 1;
      for (let dy = -1; dy <= 1 && all; dy++) {
        const yy = y + dy;
        if (yy < 0 || yy >= h) {
          all = 0;
          break;
        }
        for (let dx = -1; dx <= 1; dx++) {
          const xx = x + dx;
          if (xx < 0 || xx >= w) {
            all = 0;
            break;
          }
          if (!bin[yy * w + xx]) {
            all = 0;
            break;
          }
        }
      }
      out[i] = all;
    }
  }
  return out;
}

/** Morphological close: bridges the small gaps that separate an icon from its border. */
export function morphClose(bin: Uint8Array, w: number, h: number): Uint8Array {
  return erode(dilate(bin, w, h), w, h);
}

export type Component = {
  x0: number;
  y0: number;
  x1: number;
  y1: number;
  /** number of foreground pixels */
  area: number;
};

/** 8-connected components with an explicit stack (no recursion, no union-find). */
export function connectedComponents(bin: Uint8Array, w: number, h: number, minArea: number): Component[] {
  const visited = new Uint8Array(bin.length);
  const stack = new Int32Array(bin.length);
  const out: Component[] = [];

  for (let start = 0; start < bin.length; start++) {
    if (!bin[start] || visited[start]) continue;

    let sp = 0;
    stack[sp++] = start;
    visited[start] = 1;

    let x0 = start % w;
    let x1 = x0;
    let y0 = (start / w) | 0;
    let y1 = y0;
    let area = 0;

    while (sp > 0) {
      const i = stack[--sp];
      const x = i % w;
      const y = (i / w) | 0;
      area++;
      if (x < x0) x0 = x;
      if (x > x1) x1 = x;
      if (y < y0) y0 = y;
      if (y > y1) y1 = y;

      for (let dy = -1; dy <= 1; dy++) {
        const yy = y + dy;
        if (yy < 0 || yy >= h) continue;
        for (let dx = -1; dx <= 1; dx++) {
          if (dx === 0 && dy === 0) continue;
          const xx = x + dx;
          if (xx < 0 || xx >= w) continue;
          const j = yy * w + xx;
          if (bin[j] && !visited[j]) {
            visited[j] = 1;
            stack[sp++] = j;
          }
        }
      }
    }

    if (area >= minArea) out.push({ x0, y0, x1, y1, area });
  }
  return out;
}

/**
 * Flood fill from a seed pixel with a colour tolerance, capped by maxPixels.
 * Used for solid-colour buttons that the edge detector shreds into pieces.
 */
export function floodFillRegion(
  rgba: Uint8ClampedArray,
  w: number,
  h: number,
  sx: number,
  sy: number,
  tolerance: number,
  maxPixels: number,
): Component | null {
  if (sx < 0 || sy < 0 || sx >= w || sy >= h) return null;
  const seed = (sy * w + sx) * 4;
  const sr = rgba[seed];
  const sg = rgba[seed + 1];
  const sb = rgba[seed + 2];

  const visited = new Uint8Array(w * h);
  const stack = new Int32Array(Math.min(w * h, maxPixels + 1024));
  let sp = 0;
  const start = sy * w + sx;
  stack[sp++] = start;
  visited[start] = 1;

  let x0 = sx;
  let x1 = sx;
  let y0 = sy;
  let y1 = sy;
  let area = 0;

  const near = (i: number) => {
    const p = i * 4;
    return (
      Math.abs(rgba[p] - sr) <= tolerance &&
      Math.abs(rgba[p + 1] - sg) <= tolerance &&
      Math.abs(rgba[p + 2] - sb) <= tolerance
    );
  };

  while (sp > 0) {
    const i = stack[--sp];
    const x = i % w;
    const y = (i / w) | 0;
    area++;
    if (area > maxPixels) return null;
    if (x < x0) x0 = x;
    if (x > x1) x1 = x;
    if (y < y0) y0 = y;
    if (y > y1) y1 = y;

    for (let dy = -1; dy <= 1; dy++) {
      const yy = y + dy;
      if (yy < 0 || yy >= h) continue;
      for (let dx = -1; dx <= 1; dx++) {
        const xx = x + dx;
        if (xx < 0 || xx >= w) continue;
        const j = yy * w + xx;
        if (visited[j]) continue;
        if (!near(j)) continue;
        visited[j] = 1;
        if (sp >= stack.length) return null;
        stack[sp++] = j;
      }
    }
  }

  return { x0, y0, x1, y1, area };
}

export function grayscaleVariance(gray: Uint8Array, w: number, box: { x: number; y: number; w: number; h: number }): number {
  const x0 = Math.max(0, box.x | 0);
  const y0 = Math.max(0, box.y | 0);
  const x1 = Math.min(w, (box.x + box.w) | 0);
  const y1 = Math.min(gray.length / w, (box.y + box.h) | 0);
  let n = 0;
  let sum = 0;
  let sumSq = 0;
  for (let y = y0; y < y1; y++) {
    for (let x = x0; x < x1; x++) {
      const v = gray[y * w + x];
      sum += v;
      sumSq += v * v;
      n++;
    }
  }
  if (n === 0) return 0;
  const mean = sum / n;
  return sumSq / n - mean * mean;
}

/**
 * Fraction of a box's perimeter that is foreground.
 *
 * A button drawn as an outline has a low fill ratio (the ring is mostly empty
 * space) but a high border coverage, which is exactly how a person recognises
 * it. Without this, outline style controls get rejected as "not rectangular".
 */
export function borderCoverage(
  bin: Uint8Array,
  w: number,
  h: number,
  box: { x0: number; y0: number; x1: number; y1: number },
  thickness = 2,
): number {
  let total = 0;
  let hit = 0;
  const note = (x: number, y: number) => {
    if (x < 0 || y < 0 || x >= w || y >= h) return;
    total++;
    if (bin[y * w + x]) hit++;
  };

  for (let t = 0; t < thickness; t++) {
    const yTop = box.y0 + t;
    const yBottom = box.y1 - t;
    for (let x = box.x0; x <= box.x1; x++) {
      note(x, yTop);
      note(x, yBottom);
    }
    const xLeft = box.x0 + t;
    const xRight = box.x1 - t;
    for (let y = box.y0; y <= box.y1; y++) {
      note(xLeft, y);
      note(xRight, y);
    }
  }

  return total === 0 ? 0 : hit / total;
}
