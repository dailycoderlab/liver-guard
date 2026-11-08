import { Bell, BellRing, LayoutGrid, Redo2, Save, ScanSearch, ShieldCheck, Undo2 } from 'lucide-react';
import { useEffect, useState } from 'react';
import { backend } from '@/api';
import { probeNow, save, validateNow } from '@/store/actions';
import { useEditor } from '@/store/editor';
import { useUi } from '@/store/ui';
import { isNotifyOn, NotifyDialog } from './NotifyDialog';
import { RunControls } from './RunControls';
import { SurfacePicker } from './SurfacePicker';
import { Button } from './ui';

export function TopBar() {
  const name = useEditor((s) => s.scriptName);
  const scriptId = useEditor((s) => s.scriptId);
  const dirty = useEditor((s) => s.dirty);
  const canUndo = useEditor((s) => s.past.length > 0);
  const canRedo = useEditor((s) => s.future.length > 0);
  const runStatus = useEditor((s) => s.runStatus);
  const { undo, redo, layout } = useEditor.getState();
  const probing = useUi((s) => s.probing);
  const [notifySettings, setNotifySettings] = useState(false);
  const [notifyOn, setNotifyOn] = useState(false);
  useEffect(() => {
    void backend.getNotifyConfig().then((c) => setNotifyOn(isNotifyOn(c)), () => undefined);
  }, []);
  const open = !!scriptId;
  const running = runStatus !== 'idle';

  return (
    <header className="flex h-11 shrink-0 items-center gap-1.5 border-b border-line bg-panel px-3" data-tauri-drag-region>
      <div className="mr-3 flex min-w-0 shrink items-baseline gap-2" data-tauri-drag-region>
        <span className="text-[15px] font-semibold tracking-tight">LiverGuard</span>
        {open && (
          <span className="max-w-64 truncate text-mute">
            {name}
            {dirty && <span className="ml-1 text-amber-300" title="有未保存的修改">●</span>}
          </span>
        )}
      </div>

      <Button variant="ghost" disabled={!canUndo || running} onClick={undo} title="撤销 (⌘Z)">
        <Undo2 size={15} />
      </Button>
      <Button variant="ghost" disabled={!canRedo || running} onClick={redo} title="重做 (⇧⌘Z)">
        <Redo2 size={15} />
      </Button>
      <Button variant="ghost" disabled={!open || running} onClick={layout} title="整理布局">
        <LayoutGrid size={15} /> 整理
      </Button>

      <Button variant="ghost" onClick={() => setNotifySettings(true)} title={notifyOn ? '运行通知：已开启' : '运行通知：未开启。脚本出错、循环执行完成时发送 Webhook（钉钉、企业微信、飞书等）'}>
        {notifyOn ? <BellRing size={15} className="text-emerald-400" /> : <Bell size={15} />}
      </Button>

      <div className="mx-1 h-5 w-px bg-line" />
      <SurfacePicker />
      <Button variant="ghost" disabled={!open} onClick={validateNow} title="校验脚本">
        <ShieldCheck size={15} /> 校验
      </Button>
      <Button variant="ghost" disabled={!open || running || probing} onClick={probeNow} title="在当前画面里检查每个目标能否找到">
        <ScanSearch size={15} /> 探测
      </Button>

      <div className="ml-auto flex items-center gap-1.5">
        <Button disabled={!open || !dirty} onClick={save} title="保存 (⌘S)">
          <Save size={14} /> 保存
        </Button>
        <div className="mx-1 h-5 w-px bg-line" />
        <RunControls />
      </div>
      <NotifyDialog open={notifySettings} onOpenChange={setNotifySettings} onSaved={(c) => setNotifyOn(isNotifyOn(c))} />
    </header>
  );
}
