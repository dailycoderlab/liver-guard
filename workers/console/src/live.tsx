import { useEffect, useRef, useState } from "react";
import type { Bounds } from "./types";

export type ClickMark = { id: number; nx: number; ny: number; ts: number };

type Props = {
  frameUrl: string | null;
  bounds: Bounds | null;
  marks: ClickMark[];
  busy: boolean;
  onClickSurface: (normalized: [number, number]) => void;
};

/**
 * Live view of the captured surface.
 *
 * The container is sized to the surface's exact aspect ratio, so there is no
 * letterbox to subtract: a click at container fraction (u, v) is already the
 * normalized coordinate the protocol wants.
 */
export function LiveView({ frameUrl, bounds, marks, busy, onClickSurface }: Props) {
  const ref = useRef<HTMLDivElement>(null);
  const [hover, setHover] = useState<{ u: number; v: number } | null>(null);

  useEffect(() => setHover(null), [bounds?.w, bounds?.h]);

  const ratio = bounds && bounds.h > 0 ? `${bounds.w} / ${bounds.h}` : "16 / 10";

  const handle = (event: React.MouseEvent<HTMLDivElement>) => {
    if (busy) return;
    const rect = ref.current?.getBoundingClientRect();
    if (!rect || rect.width === 0 || rect.height === 0) return;
    const u = (event.clientX - rect.left) / rect.width;
    const v = (event.clientY - rect.top) / rect.height;
    if (u < 0 || u > 1 || v < 0 || v > 1) return;
    onClickSurface([round4(u), round4(v)]);
  };

  return (
    <div className="live">
      <div
        ref={ref}
        className={`live-surface${busy ? " busy" : ""}`}
        style={{ aspectRatio: ratio }}
        onClick={handle}
        onMouseMove={(event) => {
          const rect = ref.current?.getBoundingClientRect();
          if (!rect) return;
          setHover({
            u: (event.clientX - rect.left) / rect.width,
            v: (event.clientY - rect.top) / rect.height,
          });
        }}
        onMouseLeave={() => setHover(null)}
      >
        {frameUrl ? <img src={frameUrl} alt="device frame" draggable={false} /> : <div className="placeholder">等待画面…</div>}

        {marks.map((mark) => (
          <span key={mark.id} className="mark" style={{ left: `${mark.nx * 100}%`, top: `${mark.ny * 100}%` }} />
        ))}

        {hover && (
          <span className="crosshair" style={{ left: `${hover.u * 100}%`, top: `${hover.v * 100}%` }} />
        )}
      </div>

      <div className="live-meta">
        {bounds ? (
          <>
            <span>
              捕获区域 {Math.round(bounds.w)}×{Math.round(bounds.h)} pt
            </span>
            <span>scale {bounds.scale.toFixed(2)}x</span>
            <span>
              原点 {Math.round(bounds.x)}, {Math.round(bounds.y)}
            </span>
            {hover && (
              <span className="dim">
                光标 {hover.u.toFixed(3)}, {hover.v.toFixed(3)}
              </span>
            )}
          </>
        ) : (
          <span className="dim">未连接设备</span>
        )}
      </div>
    </div>
  );
}

function round4(value: number): number {
  return Math.round(value * 10000) / 10000;
}
