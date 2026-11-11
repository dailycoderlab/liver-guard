import * as Dialog from '@radix-ui/react-dialog';
import { useEffect, useState } from 'react';
import { create } from 'zustand';
import { Button, Input } from './ui';

interface DialogRequest {
  kind: 'confirm' | 'prompt';
  title: string;
  message?: string;
  label?: string;
  initial?: string;
  confirmText?: string;
  danger?: boolean;
  /** Confirm dialogs may offer a second, non-destructive choice (e.g. "不保存"). */
  altText?: string;
  resolve: (value: unknown) => void;
}

const useDialogs = create<{ current: DialogRequest | null }>(() => ({ current: null }));

function open<T>(request: Omit<DialogRequest, 'resolve'>): Promise<T> {
  return new Promise<T>((resolve) => {
    useDialogs.getState().current?.resolve(null);
    useDialogs.setState({ current: { ...request, resolve: resolve as (v: unknown) => void } });
  });
}

/** Resolves `true` (confirm), `'alt'` (second choice) or `false` (cancel). */
export const confirmDialog = (options: { title: string; message?: string; confirmText?: string; altText?: string; danger?: boolean }) =>
  open<boolean | 'alt'>({ kind: 'confirm', ...options }).then((v) => v ?? false);

/** Resolves the entered text, or `null` when cancelled. */
export const promptDialog = (options: { title: string; label?: string; initial?: string; confirmText?: string }) =>
  open<string | null>({ kind: 'prompt', ...options }).then((v) => v ?? null);

export function DialogHost() {
  const request = useDialogs((s) => s.current);
  const [text, setText] = useState('');
  useEffect(() => setText(request?.initial ?? ''), [request]);

  const close = (value: unknown) => {
    request?.resolve(value);
    useDialogs.setState({ current: null });
  };

  return (
    <Dialog.Root open={!!request} onOpenChange={(o) => !o && close(request?.kind === 'prompt' ? null : false)}>
      <Dialog.Portal>
        <Dialog.Overlay className="fixed inset-0 z-50 bg-black/55" />
        <Dialog.Content
          aria-describedby={undefined}
          className="fixed top-1/3 left-1/2 z-50 w-[380px] -translate-x-1/2 rounded-xl border border-line bg-panel p-4 shadow-2xl outline-none"
        >
          <Dialog.Title className="text-sm font-semibold">{request?.title}</Dialog.Title>
          {request?.message && <p className="mt-2 whitespace-pre-line text-mute">{request.message}</p>}
          {request?.kind === 'prompt' && (
            <div className="mt-3">
              {request.label && <div className="mb-1 text-xs text-mute">{request.label}</div>}
              <Input
                autoFocus
                value={text}
                onChange={(e) => setText(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === 'Enter' && text.trim()) close(text.trim());
                }}
              />
            </div>
          )}
          <div className="mt-4 flex justify-end gap-2">
            <Button onClick={() => close(request?.kind === 'prompt' ? null : false)}>取消</Button>
            {request?.altText && <Button onClick={() => close('alt')}>{request.altText}</Button>}
            <Button
              variant={request?.danger ? 'danger' : 'primary'}
              disabled={request?.kind === 'prompt' && !text.trim()}
              onClick={() => close(request?.kind === 'prompt' ? text.trim() : true)}
            >
              {request?.confirmText ?? '确定'}
            </Button>
          </div>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
