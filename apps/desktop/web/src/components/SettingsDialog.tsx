import * as Dialog from '@radix-ui/react-dialog';
import { Plus, Trash2, X } from 'lucide-react';
import { useState } from 'react';
import { useEditor } from '@/store/editor';
import { parseVarValue } from './ConditionEditor';
import { Button, Field, Input, NumberInput } from './ui';

export function SettingsDialog({ open, onOpenChange }: { open: boolean; onOpenChange: (open: boolean) => void }) {
  const meta = useEditor((s) => s.meta);
  const vars = useEditor((s) => s.vars);
  const { setMaxDuration, setVar, removeVar } = useEditor.getState();
  const [draft, setDraft] = useState('');

  const add = () => {
    const name = draft.trim();
    if (name && !(name in vars)) setVar(name, 0);
    setDraft('');
  };

  return (
    <Dialog.Root open={open} onOpenChange={onOpenChange}>
      <Dialog.Portal>
        <Dialog.Overlay className="fixed inset-0 z-50 bg-black/55" />
        <Dialog.Content aria-describedby={undefined} className="fixed top-[18%] left-1/2 z-50 w-[460px] -translate-x-1/2 rounded-xl border border-line bg-panel p-4 shadow-2xl outline-none">
          <div className="mb-3 flex items-center justify-between">
            <Dialog.Title className="text-sm font-semibold">脚本设置</Dialog.Title>
            <Dialog.Close className="text-mute hover:text-ink">
              <X size={16} />
            </Dialog.Close>
          </div>
          <Field label="最长运行时间（分钟）" hint="超过后引擎自动停止，防止脚本失控">
            <NumberInput min={1} value={Math.round(meta.maxDurationMs / 60000)} onChange={(m) => setMaxDuration(m * 60000)} />
          </Field>
          <div className="mt-4 mb-1.5 text-xs text-mute">变量（初始值）</div>
          <div className="flex flex-col gap-1.5">
            {Object.entries(vars).map(([name, value]) => (
              <div key={name} className="grid grid-cols-[1fr_1fr_auto] items-center gap-1.5">
                <span className="truncate font-mono">{name}</span>
                <Input value={String(value)} onChange={(e) => setVar(name, parseVarValue(e.target.value))} />
                <Button variant="ghost" small onClick={() => removeVar(name)}>
                  <Trash2 size={13} />
                </Button>
              </div>
            ))}
            <div className="flex gap-1.5">
              <Input placeholder="新变量名" value={draft} onChange={(e) => setDraft(e.target.value)} onKeyDown={(e) => e.key === 'Enter' && add()} />
              <Button onClick={add} disabled={!draft.trim() || draft.trim() in vars}>
                <Plus size={14} /> 添加
              </Button>
            </div>
          </div>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
