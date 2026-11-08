import * as Dialog from '@radix-ui/react-dialog';
import { Scissors, X } from 'lucide-react';
import { useEffect, useRef, useState } from 'react';
import { backend, type Rect } from '@/api';
import { useAssetUrl } from '@/hooks/useAsset';
import { clamp, HANDLES, type Handle, normalize, type Point, resizeRect } from '@/lib/selection';
import { guarded } from '@/store/actions';
import { useEditor } from '@/store/editor';
import { useUi } from '@/store/ui';
import { Button } from './ui';

/** Longest sides the image is drawn at; small crops are enlarged to fit, big ones shrunk. */
const MAX_W = 560;
const MAX_H = 380;
/** Smallest selection the core accepts, in template pixels. */
const MIN_SIDE = 4;

/**
 * Box a smaller area inside an existing template target's image and make that the target's
 * image. The target keeps its name, settings and search region, so nodes using it need no
 * change; the old image stays on disk, so undo brings it back.
 */
export function TargetCropDialog({ name, onClose }: { name: string | null; onClose: () => void }) {
  return (
    <Dialog.Root open={name !== null} onOpenChange={(open) => !open && onClose()}>
      <Dialog.Portal>
        <Dialog.Overlay className="fixed inset-0 z-50 bg-black/70" />
        <Dialog.Content aria-describedby={undefined} className="fixed top-1/2 left-1/2 z-50 flex max-h-[92vh] max-w-[92vw] -translate-x-1/2 -translate-y-1/2 flex-col gap-3 rounded-xl border border-line bg-panel p-4 shadow-2xl outline-none">
          {name !== null && <CropBody name={name} onClose={onClose} />}
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}

function CropBody({ name, onClose }: { name: string; onClose: () => void }) {
  const target = useEditor((s) => s.targets[name]);
  const url = useAssetUrl(target?.type === 'template' ? target.asset : undefined);
  const [natural, setNatural] = useState<{ w: number; h: number } | null>(null);
  const [rect, setRect] = useState<Rect | null>(null);
  const [saving, setSaving] = useState(false);
  const stageRef = useRef<HTMLDivElement>(null);
  const drag = useRef<Point | null>(null);
  const resize = useRef<{ handle: Handle; orig: Rect } | null>(null);

  // The target can disappear under an open dialog (undo, delete); there is nothing left to crop.
  useEffect(() => {
    if (!target || target.type !== 'template') onClose();
  }, [target, onClose]);

  const scale = natural ? Math.min(MAX_W / natural.w, MAX_H / natural.h) : 1;
  const toImage = (e: React.PointerEvent): Point => {
    const box = (stageRef.current ?? e.currentTarget).getBoundingClientRect();
    return { x: clamp(((e.clientX - box.left) / box.width) * (natural?.w ?? 1), 0, natural?.w ?? 1), y: clamp(((e.clientY - box.top) / box.height) * (natural?.h ?? 1), 0, natural?.h ?? 1) };
  };

  const onStageDown = (e: React.PointerEvent) => {
    if (!natural) return;
    e.currentTarget.setPointerCapture(e.pointerId);
    drag.current = toImage(e);
    setRect(null);
  };
  const onStageMove = (e: React.PointerEvent) => {
    const start = drag.current;
    if (start) setRect(normalize(start, toImage(e)));
  };
  const onStageUp = () => {
    drag.current = null;
  };

  const onHandleDown = (e: React.PointerEvent, handle: Handle) => {
    if (!rect) return;
    e.stopPropagation();
    e.currentTarget.setPointerCapture(e.pointerId);
    resize.current = { handle, orig: rect };
  };
  const onHandleMove = (e: React.PointerEvent) => {
    const r = resize.current;
    if (r) setRect(resizeRect(r.orig, r.handle, toImage(e)));
  };
  const onHandleUp = () => {
    resize.current = null;
  };

  const usable = !!rect && rect.w >= MIN_SIDE && rect.h >= MIN_SIDE;
  const apply = async () => {
    if (!target || !rect || !usable) return;
    setSaving(true);
    const refined = await guarded(() => backend.refineTemplate(target, rect));
    setSaving(false);
    if (!refined || refined.type !== 'template') return;
    useEditor.getState().patchTarget(name, { asset: refined.asset, assetBox: refined.assetBox });
    useUi.getState().notify(`已更新目标「${name}」的图片（${refined.assetBox.w}×${refined.assetBox.h} px，可撤销）`);
    onClose();
  };

  const shown = rect && natural ? { left: `${(rect.x / natural.w) * 100}%`, top: `${(rect.y / natural.h) * 100}%`, width: `${(rect.w / natural.w) * 100}%`, height: `${(rect.h / natural.h) * 100}%` } : null;

  return (
    <>
      <div className="flex items-center justify-between gap-6">
        <Dialog.Title className="min-w-0 truncate text-sm font-semibold">
          框选目标图片「{name}」
          {natural && <span className="ml-2 font-normal text-mute">{natural.w} × {natural.h} px</span>}
        </Dialog.Title>
        <Dialog.Close className="text-mute hover:text-ink" title="关闭 (Esc)">
          <X size={16} />
        </Dialog.Close>
      </div>

      <div className="flex min-h-0 justify-center overflow-auto">
        {url && (
          <div
            ref={stageRef}
            className="relative shrink-0 cursor-crosshair touch-none select-none overflow-hidden rounded border border-line bg-canvas"
            style={natural ? { width: natural.w * scale, height: natural.h * scale } : undefined}
            onPointerDown={onStageDown}
            onPointerMove={onStageMove}
            onPointerUp={onStageUp}
            onPointerCancel={onStageUp}
          >
            <img
              src={url}
              alt={name}
              draggable={false}
              onLoad={(e) => setNatural({ w: e.currentTarget.naturalWidth, h: e.currentTarget.naturalHeight })}
              className="block h-full w-full"
              style={{ imageRendering: scale > 1 ? 'pixelated' : 'auto' }}
            />
            {shown && (
              <div className="pointer-events-none absolute border-2 border-red-500 bg-red-500/15" style={shown}>
                {HANDLES.map((h) => (
                  <div
                    key={h.id}
                    className={`pointer-events-auto absolute size-3 -translate-x-1/2 -translate-y-1/2 rounded-sm border border-white bg-red-500 ${h.cursor}`}
                    style={{ left: `${h.fx * 100}%`, top: `${h.fy * 100}%`, touchAction: 'none' }}
                    onPointerDown={(e) => onHandleDown(e, h)}
                    onPointerMove={onHandleMove}
                    onPointerUp={onHandleUp}
                    onPointerCancel={onHandleUp}
                  />
                ))}
              </div>
            )}
          </div>
        )}
      </div>

      <p className="max-w-[560px] text-[11px] leading-5 text-mute">
        拖动框出要保留的区域，拖动小方块可以微调。确定后这个目标的图片换成框内的部分，名称、阈值、搜索范围都不变，用到它的节点会直接用新图片匹配；之后可以撤销（⌘Z）。
      </p>

      <div className="flex items-center justify-between gap-2">
        <span className={rect && !usable ? 'text-amber-400' : 'text-mute'}>{rect ? (usable ? `选区 ${rect.w} × ${rect.h} px` : `选区太小（至少 ${MIN_SIDE} × ${MIN_SIDE} px）`) : '还没有选区'}</span>
        <div className="flex gap-2">
          <Button onClick={onClose}>取消</Button>
          <Button variant="primary" disabled={!usable || saving} onClick={apply}>
            <Scissors size={13} /> {saving ? '处理中…' : '用选区替换图片'}
          </Button>
        </div>
      </div>
    </>
  );
}
