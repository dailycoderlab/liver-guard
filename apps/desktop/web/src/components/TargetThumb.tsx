import { Crosshair, ImageOff } from 'lucide-react';
import { useAssetUrl } from '@/hooks/useAsset';
import { cn } from '@/lib/cn';
import { useEditor } from '@/store/editor';

/** Thumbnail of a named target: the template crop, or a crosshair for a fixed point. */
export function TargetThumb({ name, className }: { name: string; className?: string }) {
  const target = useEditor((s) => s.targets[name]);
  const sha = target?.type === 'template' ? target.asset : undefined;
  const url = useAssetUrl(sha);
  const box = cn('flex shrink-0 items-center justify-center overflow-hidden rounded border border-line bg-canvas', className ?? 'h-7 w-10');
  if (!target) return <span className={cn(box, 'text-red-400')} title={`目标 "${name}" 不存在`}><ImageOff size={14} /></span>;
  if (target.type === 'point') return <span className={cn(box, 'text-mute')} title={name}><Crosshair size={14} /></span>;
  return url ? <img src={url} alt={name} title={name} className={cn(box, 'object-contain')} draggable={false} /> : <span className={box} />;
}
