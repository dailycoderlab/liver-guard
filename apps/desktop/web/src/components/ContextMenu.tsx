import { useEffect, useRef } from 'react';
import { cn } from '@/lib/cn';

export interface MenuState {
  x: number;
  y: number;
}

export interface MenuItem {
  label: string;
  onSelect: () => void;
  danger?: boolean;
  disabled?: boolean;
}

const MENU_WIDTH = 120;
const ITEM_HEIGHT = 28;

/**
 * A small right-click menu. The page-level native menu is disabled in
 * production builds, so anything that needs one draws its own.
 */
export function ContextMenu({ at, items, onClose }: { at: MenuState; items: MenuItem[]; onClose: () => void }) {
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const close = (event: Event) => {
      if (event instanceof MouseEvent && ref.current?.contains(event.target as Node)) return;
      onClose();
    };
    const onKey = (event: KeyboardEvent) => event.key === 'Escape' && onClose();
    window.addEventListener('mousedown', close);
    window.addEventListener('contextmenu', close, true);
    window.addEventListener('blur', onClose);
    window.addEventListener('resize', onClose);
    window.addEventListener('keydown', onKey);
    return () => {
      window.removeEventListener('mousedown', close);
      window.removeEventListener('contextmenu', close, true);
      window.removeEventListener('blur', onClose);
      window.removeEventListener('resize', onClose);
      window.removeEventListener('keydown', onKey);
    };
  }, [onClose]);

  // Keep the menu inside the window when opened near an edge.
  const left = Math.min(at.x, window.innerWidth - MENU_WIDTH - 8);
  const top = Math.min(at.y, window.innerHeight - items.length * ITEM_HEIGHT - 12);

  return (
    <div
      ref={ref}
      role="menu"
      style={{ left, top, width: MENU_WIDTH }}
      className="fixed z-50 rounded-lg border border-line bg-panel p-1 shadow-2xl"
      onContextMenu={(e) => e.preventDefault()}
    >
      {items.map((item) => (
        <button
          key={item.label}
          role="menuitem"
          disabled={item.disabled}
          className={cn(
            'flex h-7 w-full items-center rounded px-2 text-left text-xs hover:bg-raised disabled:pointer-events-none disabled:opacity-40',
            item.danger ? 'text-red-400' : 'text-ink',
          )}
          onClick={() => {
            onClose();
            item.onSelect();
          }}
        >
          {item.label}
        </button>
      ))}
    </div>
  );
}
