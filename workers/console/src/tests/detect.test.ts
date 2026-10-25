import { describe, expect, it } from "vitest";
import { detectCandidates, iou } from "../detect";
import type { PixelBox } from "../types";

const WIDTH = 1200;
const HEIGHT = 800;
const SCALE = 2; // device pixels per logical point

/** Deterministic pseudo random noise so the scene is stable across runs. */
function noise(seed: number): () => number {
  let state = seed;
  return () => {
    state = (state * 1664525 + 1013904223) % 4294967296;
    return state / 4294967296;
  };
}

function scene(options: { button?: PixelBox; flat?: boolean } = {}): ImageData {
  const data = new Uint8ClampedArray(WIDTH * HEIGHT * 4);
  const random = noise(42);

  // A textured background, because a flat one is not a realistic UI and hides
  // the gradient based detector's failure modes.
  for (let y = 0; y < HEIGHT; y++) {
    for (let x = 0; x < WIDTH; x++) {
      const index = (y * WIDTH + x) * 4;
      const base = options.flat ? 40 : 38 + ((x * 7 + y * 3) % 18);
      const jitter = options.flat ? 0 : Math.floor(random() * 7) - 3;
      data[index] = base + jitter;
      data[index + 1] = base + jitter;
      data[index + 2] = base + 6 + jitter;
      data[index + 3] = 255;
    }
  }

  // A button: bright fill, light border, and an inner "glyph" so it has real
  // texture rather than being a flat rectangle.
  const button = options.button;
  if (button) {
    for (let y = button.y; y < button.y + button.h; y++) {
      for (let x = button.x; x < button.x + button.w; x++) {
        if (x < 0 || y < 0 || x >= WIDTH || y >= HEIGHT) continue;
        const index = (y * WIDTH + x) * 4;
        const border =
          x < button.x + 6 || x >= button.x + button.w - 6 || y < button.y + 6 || y >= button.y + button.h - 6;
        data[index] = border ? 250 : 120;
        data[index + 1] = border ? 245 : 170;
        data[index + 2] = border ? 235 : 60;
        data[index + 3] = 255;
      }
    }
    const cx = button.x + button.w / 2;
    const cy = button.y + button.h / 2;
    for (let y = button.y + 10; y < button.y + button.h - 10; y++) {
      for (let x = button.x + 10; x < button.x + button.w - 10; x++) {
        const inside = Math.abs(x - cx) < 3 + (y - cy) ** 2 * 0.002 || Math.abs(y - cy) < 4;
        if (!inside) continue;
        const index = (y * WIDTH + x) * 4;
        data[index] = 255;
        data[index + 1] = 255;
        data[index + 2] = 255;
      }
    }
  }

  return { data, width: WIDTH, height: HEIGHT, colorSpace: "srgb" } as ImageData;
}

describe("detectCandidates", () => {
  it("finds the button under the click and returns a tight box", () => {
    const button: PixelBox = { x: 400, y: 300, w: 240, h: 120 };
    const image = scene({ button });
    const click = { x: button.x + button.w / 2, y: button.y + button.h / 2 };

    const candidates = detectCandidates(image.data as unknown as Uint8ClampedArray, WIDTH, HEIGHT, click, {
      scale: SCALE,
    });

    expect(candidates.length).toBeGreaterThan(0);
    const best = candidates.find((candidate) => candidate.kind === "contour") ?? candidates[0];
    expect(best.kind).toBe("contour");
    expect(iou(best, button)).toBeGreaterThan(0.55);
    expect(best.x).toBeLessThanOrEqual(click.x);
    expect(best.x + best.w).toBeGreaterThanOrEqual(click.x);
  });

  it("never returns a box larger than the configured maximum", () => {
    const button: PixelBox = { x: 200, y: 200, w: 600, h: 300 };
    const image = scene({ button });
    const candidates = detectCandidates(
      image.data as unknown as Uint8ClampedArray,
      WIDTH,
      HEIGHT,
      { x: 500, y: 350 },
      { scale: SCALE, maxSide: 220 },
    );

    for (const candidate of candidates) {
      expect(Math.max(candidate.w, candidate.h)).toBeLessThanOrEqual(220 * SCALE + 2);
    }
  });

  it("flags a flat area as a fallback instead of pretending to find a control", () => {
    const image = scene({ flat: true });
    const candidates = detectCandidates(image.data as unknown as Uint8ClampedArray, WIDTH, HEIGHT, { x: 600, y: 400 }, {
      scale: SCALE,
    });

    expect(candidates).toHaveLength(1);
    expect(candidates[0].kind).toBe("fallback");
  });

  it("puts the native AX box first when the platform provides one", () => {
    const button: PixelBox = { x: 400, y: 300, w: 240, h: 120 };
    const axBox: PixelBox = { x: 402, y: 302, w: 236, h: 116 };
    const image = scene({ button });

    const candidates = detectCandidates(
      image.data as unknown as Uint8ClampedArray,
      WIDTH,
      HEIGHT,
      { x: 520, y: 360 },
      { scale: SCALE, axBox },
    );

    expect(candidates[0].kind).toBe("ax");
    expect(iou(candidates[0], axBox)).toBeGreaterThan(0.9);
  });

  it("keeps the click point inside every candidate", () => {
    const button: PixelBox = { x: 700, y: 500, w: 220, h: 110 };
    const image = scene({ button });
    const click = { x: 810, y: 555 };

    const candidates = detectCandidates(image.data as unknown as Uint8ClampedArray, WIDTH, HEIGHT, click, {
      scale: SCALE,
    });

    for (const candidate of candidates) {
      expect(candidate.x).toBeLessThanOrEqual(click.x);
      expect(candidate.x + candidate.w).toBeGreaterThanOrEqual(click.x);
      expect(candidate.y).toBeLessThanOrEqual(click.y);
      expect(candidate.y + candidate.h).toBeGreaterThanOrEqual(click.y);
    }
  });

  it("finds a control in the corner of the surface", () => {
    const button: PixelBox = { x: 0, y: 680, w: 200, h: 116 };
    const image = scene({ button });
    const candidates = detectCandidates(image.data as unknown as Uint8ClampedArray, WIDTH, HEIGHT, { x: 100, y: 738 }, {
      scale: SCALE,
    });

    const best = candidates[0];
    expect(best.x).toBeGreaterThanOrEqual(0);
    expect(best.y + best.h).toBeLessThanOrEqual(HEIGHT);
  });

  it("stays inside the interaction budget for a search window", () => {
    const button: PixelBox = { x: 400, y: 300, w: 240, h: 120 };
    const image = scene({ button });

    const started = performance.now();
    detectCandidates(image.data as unknown as Uint8ClampedArray, WIDTH, HEIGHT, { x: 520, y: 360 }, { scale: SCALE });
    const elapsed = performance.now() - started;

    // 1200x800 is the size the device actually sends (600x400pt at 2x).
    expect(elapsed).toBeLessThan(600);
  });
});
