import { useShallow } from 'zustand/react/shallow';
import { Plus, X } from 'lucide-react';
import { cn } from '@/lib/cn';
import { useEditor } from '@/store/editor';
import { useUi } from '@/store/ui';
import { confirmDialog, promptDialog } from './Dialogs';

export function FlowTabs() {
  const flows = useEditor(useShallow((s) => Object.keys(s.flows)));
  const active = useEditor((s) => s.activeFlow);
  const entry = useEditor((s) => s.entry);
  const { setActiveFlow, addFlow, renameFlow, deleteFlow } = useEditor.getState();
  const notify = useUi((s) => s.notify);

  return (
    <div className="flex h-9 shrink-0 items-end gap-0.5 overflow-x-auto border-b border-line bg-panel px-2">
      {flows.map((name) => (
        <div
          key={name}
          onClick={() => setActiveFlow(name)}
          onDoubleClick={async () => {
            const next = await promptDialog({ title: '重命名流程', initial: name, confirmText: '重命名' });
            if (next && next !== name && renameFlow(name, next) === null) notify('流程名称已存在', 'error');
          }}
          className={cn('group flex h-8 cursor-pointer items-center gap-1.5 rounded-t-md border border-b-0 px-3', name === active ? 'border-line bg-canvas text-ink' : 'border-transparent text-mute hover:text-ink')}
        >
          <span>{name}</span>
          {name === entry && <span className="rounded bg-emerald-600/25 px-1 text-[10px] text-emerald-300">入口</span>}
          {name !== entry && (
            <button
              className="hidden text-mute hover:text-red-400 group-hover:block"
              title="删除流程"
              onClick={async (e) => {
                e.stopPropagation();
                if (await confirmDialog({ title: `删除流程「${name}」`, message: '调用它的节点会被清空。', confirmText: '删除', danger: true })) deleteFlow(name);
              }}
            >
              <X size={12} />
            </button>
          )}
        </div>
      ))}
      <button
        className="mb-1 ml-1 flex h-6 w-6 items-center justify-center rounded text-mute hover:bg-raised hover:text-ink"
        title="新建子流程"
        onClick={async () => {
          const name = await promptDialog({ title: '新建子流程', label: '流程名称', confirmText: '创建' });
          if (name && addFlow(name) === null) notify('流程名称已存在', 'error');
        }}
      >
        <Plus size={14} />
      </button>
    </div>
  );
}
