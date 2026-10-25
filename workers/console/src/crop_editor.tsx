import { useEffect, useRef, useState } from "react";
import type { Bounds, Candidate, PixelBox } from "./types";

type Props = {
  contextUrl: string | null;
  imageSize: { w: number; h: number } | null;
  windowRect: Bounds | null;
  clickPx: [number, number] | null;
  candidates: Candidate[];
  selected: number;
  box: PixelBox | null;
  strictness: "loose" | "normal" | "tight";
  busy: boolean;
  onSelect: (index: number) => void;
  onBoxChange: (box: PixelBox) => void;
  onStrictness: (value: "loose" | "normal" | "tight") => void;
  onConfirm: () => void;
  onCancel: () => void;
};

type DragMode = "move" | "n" | "s" | "e" | "w" | "ne" | "nw" | "se" | "sw";

const HANDLES: DragMode[] = ["nw", "n", "ne", "e", "se", "s", "sw", "w"];

const HANDLE_STYLE: Record<string, React.CSSProperties> = {
  nw: { left: 0, top: 0, cursor: "nwse-resize" },
  n: { left: "50%", top: 0, cursor: "ns-resize" },
  ne: { left: "100%", top: 0, cursor: "nesw-resize" },
  e: { left: "100%", top: "50%", cursor: "ew-resize" },
  se: { left: "100%", top: "100%", cursor: "nwse-resize" },
  s: { left: "50%", top: "100%", cursor: "ns-resize" },
  sw: { left: 0, top: "100%", cursor: "nesw-resize" },
  w: { left: 0, top: "50%", cursor: "ew-resize" },
};

/**
 * Crop editor for one click.
 *
 * Works on the search window PNG the device sent, so every adjustment is local:
 * nothing round-trips to the device unless the operator drags outside the window.
 */
export function CropEditor(props: Props) {
  const { contextUrl, imageSize, clickPx, candidates, selected, box, busy } = props;
  const surfaceRef = useRef<HTMLDivElement>(null);
  const [drag, setDrag] = useState<{ mode: DragMode; startX: number; startY: number; origin: PixelBox } | null>(null);

  useEffect(() => {
    if (!drag) return;
    const onMove = (event: PointerEvent) => {
      const rect = surfaceRef.current?.getBoundingClientRect();
      if (!rect || !imageSize) return;
      const kx = imageSize.w / rect.width;
      const ky = imageSize.h / rect.height;
      const dx = (event.clientX - drag.startX) * kx;
      const dy = (event.clientY - drag.startY) * ky;
      props.onBoxChange(applyDrag(drag.origin, drag.mode, dx, dy, imageSize));
    };
    const onUp = () => setDrag(null);
    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", onUp);
    return () => {
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerup", onUp);
    };
  }, [drag, imageSize, props]);

  if (!contextUrl || !imageSize) {
    return (
      <div className="crop-editor empty">
        <p>在实时画面上点一下要操作的控件，这里会显示它的局部放大图和自动识别结果。</p>
      </div>
    );
  }

  const ratio = `${imageSize.w} / ${imageSize.h}`;
  const toPercent = (value: number, total: number) => `${(value / total) * 100}%`;

  return (
    <div className="crop-editor">
      <div className="crop-toolbar">
        <strong>裁图</strong>
        <label className="dim">
          严格度
          <select value={props.strictness} onChange={(event) => props.onStrictness(event.target.value as any)}>
            <option value="loose">松</option>
            <option value="normal">标准</option>
            <option value="tight">紧</option>
          </select>
        </label>
        <span className="dim">
          {candidates.length} 个候选
          {props.windowRect ? ` · 窗口 ${Math.round(props.windowRect.w)}×${Math.round(props.windowRect.h)} pt` : ""}
        </span>
      </div>

      <div className="crop-surface" ref={surfaceRef} style={{ aspectRatio: ratio }}>
        <img src={contextUrl} alt="search window" draggable={false} />

        {box && (
          <div
            className="crop-box"
            style={{
              left: toPercent(box.x, imageSize.w),
              top: toPercent(box.y, imageSize.h),
              width: toPercent(box.w, imageSize.w),
              height: toPercent(box.h, imageSize.h),
            }}
            onPointerDown={(event) => {
              event.stopPropagation();
              setDrag({ mode: "move", startX: event.clientX, startY: event.clientY, origin: box });
            }}
          >
            {HANDLES.map((handle) => (
              <span
                key={handle}
                className="handle"
                style={HANDLE_STYLE[handle]}
                onPointerDown={(event) => {
                  event.stopPropagation();
                  setDrag({ mode: handle, startX: event.clientX, startY: event.clientY, origin: box });
                }}
              />
            ))}
          </div>
        )}

        {clickPx && (
          <span
            className="click-dot"
            style={{ left: toPercent(clickPx[0], imageSize.w), top: toPercent(clickPx[1], imageSize.h) }}
          />
        )}
      </div>

      <div className="candidate-row">
        {candidates.map((candidate, index) => (
          <button
            key={index}
            className={`chip${index === selected ? " active" : ""}`}
            onClick={() => props.onSelect(index)}
            title={`${candidate.kind} · ${candidate.w}×${candidate.h}px · score ${candidate.score.toFixed(2)}`}
          >
            {label(candidate.kind)} {Math.round(candidate.w)}×{Math.round(candidate.h)}
          </button>
        ))}
        {candidates.some((candidate) => candidate.kind === "fallback") && (
          <span className="warn">自动识别没找到控件轮廓，请手动拖框选中按钮</span>
        )}
      </div>

      <div className="crop-actions">
        <button className="primary" disabled={busy || !box} onClick={props.onConfirm}>
          确认这一步
        </button>
        <button disabled={busy} onClick={props.onCancel}>
          取消
        </button>
      </div>
    </div>
  );
}

function label(kind: Candidate["kind"]): string {
  switch (kind) {
    case "ax":
      return "系统控件";
    case "contour":
      return "轮廓";
    case "color":
      return "色块";
    default:
      return "默认框";
  }
}

function applyDrag(origin: PixelBox, mode: DragMode, dx: number, dy: number, image: { w: number; h: number }): PixelBox {
  const minSide = 12;
  let { x, y, w, h } = origin;

  if (mode === "move") {
    x += dx;
    y += dy;
  } else {
    if (mode.includes("w")) {
      const next = Math.min(origin.x + dx, origin.x + origin.w - minSide);
      w = origin.w + (origin.x - next);
      x = next;
    }
    if (mode.includes("e")) {
      w = Math.max(minSide, origin.w + dx);
    }
    if (mode.includes("n")) {
      const next = Math.min(origin.y + dy, origin.y + origin.h - minSide);
      h = origin.h + (origin.y - next);
      y = next;
    }
    if (mode.includes("s")) {
      h = Math.max(minSide, origin.h + dy);
    }
  }

  return {
    x: Math.max(0, Math.min(image.w - minSide, Math.round(x))),
    y: Math.max(0, Math.min(image.h - minSide, Math.round(y))),
    w: Math.max(minSide, Math.min(image.w - Math.max(0, x), Math.round(w))),
    h: Math.max(minSide, Math.min(image.h - Math.max(0, y), Math.round(h))),
  };
}
