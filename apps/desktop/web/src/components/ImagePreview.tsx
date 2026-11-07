import * as Dialog from '@radix-ui/react-dialog';
import { X } from 'lucide-react';
import { useState } from 'react';
import { create } from 'zustand';

const usePreview = create<{ current: { url: string; title: string } | null }>(() => ({ current: null }));

/** Show an image full-size in a modal. */
export const previewImage = (url: string, title: string) => usePreview.setState({ current: { url, title } });

export function ImagePreviewHost() {
  const current = usePreview((s) => s.current);
  const [size, setSize] = useState<string | null>(null);

  return (
    <Dialog.Root
      open={!!current}
      onOpenChange={(open) => {
        if (!open) {
          usePreview.setState({ current: null });
          setSize(null);
        }
      }}
    >
      <Dialog.Portal>
        <Dialog.Overlay className="fixed inset-0 z-50 bg-black/70" />
        <Dialog.Content
          aria-describedby={undefined}
          className="fixed top-1/2 left-1/2 z-50 flex max-h-[90vh] max-w-[90vw] -translate-x-1/2 -translate-y-1/2 flex-col gap-2 rounded-xl border border-line bg-panel p-3 shadow-2xl outline-none"
        >
          <div className="flex items-center justify-between gap-6">
            <Dialog.Title className="min-w-0 truncate text-sm font-semibold">
              {current?.title}
              {size && <span className="ml-2 font-normal text-mute">{size}</span>}
            </Dialog.Title>
            <Dialog.Close className="text-mute hover:text-ink" title="关闭 (Esc)">
              <X size={16} />
            </Dialog.Close>
          </div>
          {current && (
            // Template crops are often tiny: the minimum width enlarges them, the maxima keep big ones on screen.
            <img
              src={current.url}
              alt={current.title}
              draggable={false}
              onLoad={(e) => setSize(`${e.currentTarget.naturalWidth} × ${e.currentTarget.naturalHeight} px`)}
              className="max-h-[75vh] max-w-[85vw] min-w-72 rounded border border-line bg-canvas object-contain"
            />
          )}
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
