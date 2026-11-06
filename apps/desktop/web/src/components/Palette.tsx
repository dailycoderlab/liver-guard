import { CATEGORY_LABELS, NODE_ORDER, NODE_SPECS, type Category } from '@/ir/catalog';
import { useEditor } from '@/store/editor';
import { DRAG_MIME } from './Canvas';

const CATEGORIES: Category[] = ['flow', 'action', 'wait', 'logic'];

/** Drag onto the canvas, or click to add after the selected node. */
export function Palette() {
  const addNode = useEditor((s) => s.addNode);
  const locked = useEditor((s) => s.runStatus !== 'idle');
  return (
    <fieldset disabled={locked} className="flex min-w-0 flex-col gap-4 overflow-y-auto border-0 p-3 disabled:opacity-50">
      {CATEGORIES.map((category) => (
        <div key={category}>
          <div className="mb-1.5 text-xs font-medium text-mute">{CATEGORY_LABELS[category]}</div>
          <div className="grid grid-cols-2 gap-1.5">
            {NODE_ORDER.filter((t) => t !== 'start' && NODE_SPECS[t].category === category).map((type) => {
              const spec = NODE_SPECS[type];
              const Icon = spec.icon;
              return (
                <button
                  key={type}
                  title={spec.hint}
                  draggable
                  onDragStart={(e) => {
                    e.dataTransfer.setData(DRAG_MIME, type);
                    e.dataTransfer.effectAllowed = 'move';
                  }}
                  onClick={() => addNode(type)}
                  className="flex cursor-grab items-center gap-1.5 rounded-md border border-line bg-raised px-2 py-1.5 text-left hover:border-accent active:cursor-grabbing"
                >
                  <span className={`flex h-5 w-5 items-center justify-center rounded text-white ${spec.accent}`}>
                    <Icon size={12} />
                  </span>
                  <span className="truncate text-xs">{spec.label}</span>
                </button>
              );
            })}
          </div>
        </div>
      ))}
    </fieldset>
  );
}
