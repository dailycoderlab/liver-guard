import { useRef, type KeyboardEvent, type PointerEvent } from 'react';
import { cn } from '@/lib/cn';
import { clamp, DEFAULT_SIZES, LIMITS, MIN_CENTER_HEIGHT, MIN_CENTER_WIDTH, type PanelSizes } from '@/lib/layout';
import { useUi } from '@/store/ui';

type Edge = 'right' | 'left' | 'top';

/**
 * A drag handle on one edge of a panel.
 *
 * `right` sits on the right edge of the left panel, `left` on the left edge of
 * the right panel, `top` on the top edge of the bottom panel. Drag to resize,
 * double-click to reset, arrow keys to nudge. The size is kept in the UI store
 * and written to localStorage when the drag ends.
 */
export function Splitter({ edge, size }: { edge: Edge; size: keyof PanelSizes }) {
  const value = useUi((s) => s[size]);
  const setSize = useUi((s) => s.setSize);
  const drag = useRef<{ start: number; initial: number; max: number } | null>(null);
  const horizontal = edge !== 'top';
  const { min } = LIMITS[size];

  /** Largest size that still leaves the centre column usable. */
  const maxFor = () => {
    const { leftWidth, rightWidth } = useUi.getState();
    const room =
      size === 'leftWidth'
        ? window.innerWidth - rightWidth - MIN_CENTER_WIDTH
        : size === 'rightWidth'
          ? window.innerWidth - leftWidth - MIN_CENTER_WIDTH
          : window.innerHeight - MIN_CENTER_HEIGHT - 40;
    return Math.min(LIMITS[size].max, room);
  };

  const apply = (next: number, persist = false) => setSize(size, Math.round(clamp(next, min, maxFor())), persist);

  const onPointerDown = (event: PointerEvent<HTMLDivElement>) => {
    if (event.button !== 0) return;
    event.preventDefault();
    event.currentTarget.setPointerCapture(event.pointerId);
    drag.current = { start: horizontal ? event.clientX : event.clientY, initial: value, max: maxFor() };
    document.body.style.userSelect = 'none';
    document.body.style.cursor = horizontal ? 'col-resize' : 'row-resize';
  };

  const onPointerMove = (event: PointerEvent<HTMLDivElement>) => {
    const d = drag.current;
    if (!d) return;
    const delta = (horizontal ? event.clientX : event.clientY) - d.start;
    // The left panel grows rightwards; the right and bottom panels grow
    // towards the start of the screen.
    apply(edge === 'right' ? d.initial + delta : d.initial - delta);
  };

  const end = (event: PointerEvent<HTMLDivElement>) => {
    if (!drag.current) return;
    drag.current = null;
    event.currentTarget.releasePointerCapture(event.pointerId);
    document.body.style.userSelect = '';
    document.body.style.cursor = '';
    setSize(size, useUi.getState()[size], true);
  };

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    const grow = edge === 'right' ? 'ArrowRight' : edge === 'left' ? 'ArrowLeft' : 'ArrowUp';
    const shrink = edge === 'right' ? 'ArrowLeft' : edge === 'left' ? 'ArrowRight' : 'ArrowDown';
    const step = event.shiftKey ? 40 : 10;
    if (event.key === grow) apply(value + step, true);
    else if (event.key === shrink) apply(value - step, true);
    else return;
    event.preventDefault();
  };

  return (
    <div
      role="separator"
      aria-orientation={horizontal ? 'vertical' : 'horizontal'}
      aria-valuenow={value}
      aria-valuemin={min}
      aria-valuemax={LIMITS[size].max}
      tabIndex={0}
      title="拖动调整大小，双击还原"
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={end}
      onPointerCancel={end}
      onDoubleClick={() => apply(DEFAULT_SIZES[size], true)}
      onKeyDown={onKeyDown}
      className={cn(
        'group absolute z-20 touch-none outline-none',
        horizontal ? 'inset-y-0 w-1.5 cursor-col-resize' : 'inset-x-0 h-1.5 cursor-row-resize',
        edge === 'right' && '-right-[3px]',
        edge === 'left' && '-left-[3px]',
        edge === 'top' && '-top-[3px]',
      )}
    >
      <div
        className={cn(
          'absolute bg-accent opacity-0 transition-opacity group-hover:opacity-70 group-focus-visible:opacity-70 group-active:opacity-100',
          horizontal ? 'inset-y-0 left-[2px] w-0.5' : 'inset-x-0 top-[2px] h-0.5',
        )}
      />
    </div>
  );
}
