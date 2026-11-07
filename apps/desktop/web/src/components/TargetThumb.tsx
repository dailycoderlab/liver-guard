import { Crosshair, ImageOff } from 'lucide-react';
import { useAssetUrl } from '@/hooks/useAsset';
import { cn } from '@/lib/cn';
import { useEditor } from '@/store/editor';
import { previewImage } from './ImagePreview';

/**
 * Thumbnail of a named target: the template crop, or a crosshair for a fixed point.
 * `zoomable` makes a click open the crop full-size; leave it off where a click already means
 * something else (the thumbnails on canvas nodes select and drag the node).
 */
export function TargetThumb({ name, className, zoomable }: { name: string; className?: string; zoomable?: boolean }) {
  const target = useEditor((s) => s.targets[name]);
  const sha = target?.type === 'template' ? target.asset : undefined;
  const url = useAssetUrl(sha);
  const box = cn('flex shrink-0 items-center justify-center overflow-hidden rounded border border-line bg-canvas', className ?? 'h-7 w-10');
  if (!target) return <span className={cn(box, 'text-red-400')} title={`目标 "${name}" 不存在`}><ImageOff size={14} /></span>;
  if (target.type === 'point') return <span className={cn(box, 'text-mute')} title={name}><Crosshair size={14} /></span>;
  if (!url) return <span className={box} />;
  return (
    <img
      src={url}
      alt={name}
      title={zoomable ? `${name}（点击放大）` : name}
      className={cn(box, 'object-contain', zoomable && 'cursor-zoom-in hover:border-accent')}
      draggable={false}
      onClick={
        zoomable
          ? (e) => {
              e.stopPropagation();
              previewImage(url, name);
            }
          : undefined
      }
    />
  );
}
