import { Camera, Crosshair, Pause, Play, RefreshCw, Scissors } from 'lucide-react';
import { useCallback, useEffect, useRef, useState } from 'react';
import { backend, type FramePacket, type Rect, type Surface } from '@/api';
import { guarded } from '@/store/actions';
import { useEditor } from '@/store/editor';
import { useUi } from '@/store/ui';
import { Button, Empty, Input, Select } from './ui';

type Mode = 'idle' | 'live' | 'frozen';

const surfaceValue = (s: Surface) => (s.kind === 'fullScreen' ? 'full' : String(s.number));

const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));

function normalize(a: { x: number; y: number }, b: { x: number; y: number }): Rect {
  const x = Math.min(a.x, b.x);
  const y = Math.min(a.y, b.y);
  return { x: Math.round(x), y: Math.round(y), w: Math.round(Math.abs(a.x - b.x)), h: Math.round(Math.abs(a.y - b.y)) };
}

/** Put a freshly made target on the selected node when that slot is empty. */
function assignToSelected(name: string) {
  const state = useEditor.getState();
  const node = state.flows[state.activeFlow]?.nodes.find((n) => n.selected);
  if (!node) return;
  const { kind, params } = node.data;
  if ((kind === 'tap' || kind === 'longPress' || kind === 'waitGone') && !params.target) state.updateNode(node.id, { params: { ...params, target: name } });
  else if (kind === 'swipe' && !params.from) state.updateNode(node.id, { params: { ...params, from: name } });
  else if (kind === 'waitFor') state.updateNode(node.id, { params: { ...params, any: [...((params.any as string[]) ?? []), name] } });
}

export function LivePanel({ active }: { active: boolean }) {
  const surface = useEditor((s) => s.surface);
  const setSurface = useEditor((s) => s.setSurface);
  const scriptId = useEditor((s) => s.scriptId);
  const windows = useUi((s) => s.windows);
  const notify = useUi((s) => s.notify);

  const [mode, setMode] = useState<Mode>('idle');
  const [frame, setFrame] = useState<{ seq: number; w: number; h: number } | null>(null);
  const [rect, setRect] = useState<Rect | null>(null);
  const [candidates, setCandidates] = useState<Rect[]>([]);
  const [candidateIndex, setCandidateIndex] = useState(0);
  const [name, setName] = useState('target');
  const [error, setError] = useState<string | null>(null);

  const canvasRef = useRef<HTMLCanvasElement>(null);
  const stopRef = useRef<null | (() => Promise<void>)>(null);
  const decoding = useRef(false);
  const drag = useRef<{ start: { x: number; y: number }; moved: boolean } | null>(null);
  const lastClick = useRef<{ x: number; y: number } | null>(null);

  const refreshWindows = useCallback(async () => {
    const list = await guarded(() => backend.listWindows());
    if (list) useUi.getState().set({ windows: list });
  }, []);

  const draw = useCallback(async (packet: FramePacket) => {
    const canvas = canvasRef.current;
    if (!canvas || decoding.current) return;
    decoding.current = true;
    try {
      const bitmap = await createImageBitmap(packet.image);
      if (canvas.width !== bitmap.width || canvas.height !== bitmap.height) {
        canvas.width = bitmap.width;
        canvas.height = bitmap.height;
      }
      canvas.getContext('2d')?.drawImage(bitmap, 0, 0);
      // Read the size first: `close()` zeroes it, and the state updater runs later.
      const { width, height } = bitmap;
      bitmap.close();
      setFrame((prev) => (prev && prev.w === width && prev.h === height ? { ...prev, seq: packet.seq } : { seq: packet.seq, w: width, h: height }));
    } finally {
      decoding.current = false;
    }
  }, []);

  const stopLive = useCallback(async () => {
    const stop = stopRef.current;
    stopRef.current = null;
    if (stop) await guarded(stop);
  }, []);

  const startLive = useCallback(async () => {
    setError(null);
    setRect(null);
    setCandidates([]);
    const stop = await guarded(() => backend.liveStart(useEditor.getState().surface, (p) => void draw(p), (m) => setError(m)));
    if (stop) {
      stopRef.current = stop;
      setMode('live');
    }
  }, [draw]);

  const freeze = useCallback(async () => {
    const packet = await guarded(() => backend.snapshot());
    await stopLive();
    if (!packet) return;
    decoding.current = false;
    await draw(packet);
    setMode('frozen');
  }, [draw, stopLive]);

  // The preview only runs while this tab is showing.
  useEffect(() => {
    if (!active && mode === 'live') {
      void stopLive().then(() => setMode('idle'));
    }
  }, [active, mode, stopLive]);
  useEffect(() => () => void stopLive(), [stopLive]);
  useEffect(() => {
    if (active && windows.length === 0) void refreshWindows();
  }, [active, windows.length, refreshWindows]);

  const toImage = (e: React.PointerEvent) => {
    const box = e.currentTarget.getBoundingClientRect();
    return { x: clamp(((e.clientX - box.left) / box.width) * (frame?.w ?? 1), 0, frame?.w ?? 1), y: clamp(((e.clientY - box.top) / box.height) * (frame?.h ?? 1), 0, frame?.h ?? 1) };
  };

  const onPointerDown = (e: React.PointerEvent) => {
    if (mode !== 'frozen') return;
    e.currentTarget.setPointerCapture(e.pointerId);
    drag.current = { start: toImage(e), moved: false };
  };
  const onPointerMove = (e: React.PointerEvent) => {
    const d = drag.current;
    if (!d || !frame) return;
    const here = toImage(e);
    const box = e.currentTarget.getBoundingClientRect();
    const pixels = Math.hypot(((here.x - d.start.x) / frame.w) * box.width, ((here.y - d.start.y) / frame.h) * box.height);
    if (pixels > 4) d.moved = true;
    if (d.moved) setRect(normalize(d.start, here));
  };
  const onPointerUp = async (e: React.PointerEvent) => {
    const d = drag.current;
    drag.current = null;
    if (!d || !frame) return;
    if (d.moved) {
      setCandidates([]);
      return;
    }
    // A click asks the core for boxes around that point; clicking again in the same place cycles them.
    const at = toImage(e);
    const prev = lastClick.current;
    lastClick.current = at;
    if (prev && candidates.length > 1 && Math.hypot(prev.x - at.x, prev.y - at.y) < 6) {
      const next = (candidateIndex + 1) % candidates.length;
      setCandidateIndex(next);
      setRect(candidates[next]);
      return;
    }
    const found = await guarded(() => backend.detectCandidates(frame.seq, Math.round(at.x), Math.round(at.y)));
    if (found?.length) {
      setCandidates(found);
      setCandidateIndex(0);
      setRect(found[0]);
    }
  };

  const createTemplate = async () => {
    if (!frame || !rect || rect.w < 4 || rect.h < 4) return notify('框选区域太小', 'error');
    const target = await guarded(() => backend.createTemplate(frame.seq, rect));
    if (!target) return;
    const finalName = useEditor.getState().addTarget(target, name.trim() || 'target');
    assignToSelected(finalName);
    notify(`已创建目标「${finalName}」`);
    setRect(null);
    setCandidates([]);
  };

  const createPoint = () => {
    if (!frame || !rect) return notify('先点击或框选一个位置', 'error');
    const cx = (rect.x + rect.w / 2) / frame.w;
    const cy = (rect.y + rect.h / 2) / frame.h;
    const half = 0.03;
    const zone: [number, number, number, number] = [clamp(cx - half, 0, 1 - 2 * half), clamp(cy - half, 0, 1 - 2 * half), 2 * half, 2 * half];
    const finalName = useEditor.getState().addTarget({ type: 'point', norm: [cx, cy], zone }, name.trim() || 'point');
    assignToSelected(finalName);
    notify(`已创建坐标点「${finalName}」`);
    setRect(null);
  };

  if (!scriptId) return <Empty>先打开一个脚本</Empty>;

  const shown = rect && frame ? { left: `${(rect.x / frame.w) * 100}%`, top: `${(rect.y / frame.h) * 100}%`, width: `${(rect.w / frame.w) * 100}%`, height: `${(rect.h / frame.h) * 100}%` } : null;

  return (
    <div className="flex h-full flex-col">
      <div className="flex flex-col gap-2 border-b border-line p-3">
        <div className="flex items-center gap-1.5">
          <Select
            value={surfaceValue(surface)}
            disabled={mode !== 'idle'}
            onChange={(e) => setSurface(e.target.value === 'full' ? { kind: 'fullScreen' } : { kind: 'window', number: Number(e.target.value) })}
          >
            <option value="full">整个屏幕</option>
            {windows.map((w) => (
              <option key={w.number} value={w.number}>
                {w.label}
              </option>
            ))}
          </Select>
          <Button variant="ghost" title="刷新窗口列表" onClick={refreshWindows}>
            <RefreshCw size={14} />
          </Button>
        </div>
        <div className="flex gap-1.5">
          {mode === 'idle' && (
            <Button variant="primary" className="flex-1" onClick={startLive}>
              <Play size={14} /> 开始预览
            </Button>
          )}
          {mode === 'live' && (
            <>
              <Button variant="primary" className="flex-1" onClick={freeze}>
                <Camera size={14} /> 截取画面
              </Button>
              <Button onClick={() => void stopLive().then(() => setMode('idle'))}>
                <Pause size={14} /> 停止
              </Button>
            </>
          )}
          {mode === 'frozen' && (
            <Button className="flex-1" onClick={startLive}>
              <Play size={14} /> 继续预览
            </Button>
          )}
        </div>
        {error && <div className="text-xs text-red-400">{error}</div>}
      </div>

      <div className="min-h-0 flex-1 overflow-auto p-3">
        <div
          className={`relative mx-auto w-full overflow-hidden rounded-md border border-line bg-black ${mode === 'frozen' ? 'cursor-crosshair' : ''}`}
          style={{ aspectRatio: frame ? `${frame.w} / ${frame.h}` : '16 / 10' }}
          onPointerDown={onPointerDown}
          onPointerMove={onPointerMove}
          onPointerUp={onPointerUp}
        >
          <canvas ref={canvasRef} className="block h-full w-full" />
          {shown && <div className="pointer-events-none absolute border-2 border-accent bg-accent/15" style={shown} />}
          {mode === 'idle' && !frame && <div className="absolute inset-0 flex items-center justify-center text-mute">点击「开始预览」</div>}
        </div>
        {mode === 'frozen' && <p className="mt-2 text-xs text-mute">拖动框选目标；单击让引擎自动圈出，再次单击在候选框之间切换。</p>}
      </div>

      {mode === 'frozen' && (
        <div className="flex flex-col gap-2 border-t border-line p-3">
          <Input value={name} placeholder="目标名称" onChange={(e) => setName(e.target.value)} />
          <div className="flex gap-1.5">
            <Button variant="primary" className="flex-1" disabled={!rect} onClick={createTemplate}>
              <Scissors size={14} /> 创建模板目标
            </Button>
            <Button disabled={!rect} onClick={createPoint} title="不做图像匹配，按屏幕比例点击该位置">
              <Crosshair size={14} /> 坐标点
            </Button>
          </div>
        </div>
      )}
    </div>
  );
}
