import { ArrowUpCircle, Loader2 } from 'lucide-react';
import { useEffect } from 'react';
import { backend } from '@/api';
import { useEditor } from '@/store/editor';
import { useUi } from '@/store/ui';
import { updatableVersion, useUpdate } from '@/store/update';
import { confirmDialog } from './Dialogs';
import { Button } from './ui';

const MAX_NOTES = 400;

/** Top bar entry for in-app updates. Checks once at startup; a dot appears when a newer version is waiting. */
export function UpdateButton() {
  const state = useUpdate((s) => s.state);
  const { check, install } = useUpdate.getState();

  useEffect(() => {
    // Silent: a failed startup check (offline, no release yet) must not bother anyone.
    if (backend.native) void useUpdate.getState().check();
  }, []);

  if (!backend.native) return null;

  const found = updatableVersion(state);
  const busy = state.phase === 'checking' || state.phase === 'installing';

  const onClick = async () => {
    const { notify } = useUi.getState();
    let target = found;
    if (!target) {
      await check();
      const next = useUpdate.getState().state;
      target = updatableVersion(next);
      if (!target) {
        if (next.phase === 'error') notify(`检查更新失败：${next.message}`, 'error');
        else notify('已经是最新版本');
        return;
      }
    }
    if (useEditor.getState().runStatus !== 'idle') {
      notify(`发现新版本 ${target.version}。脚本正在运行，请先停止再更新`);
      return;
    }
    const notes = target.notes && (target.notes.length > MAX_NOTES ? `${target.notes.slice(0, MAX_NOTES)}…` : target.notes);
    const ok = await confirmDialog({
      title: `更新到 ${target.version}`,
      message: [
        `当前版本 ${target.currentVersion}。下载并安装后，LiverGuard 会自动重启，未保存的修改会丢失。`,
        notes ? `\n${notes}` : '',
      ].join('\n'),
      confirmText: '更新并重启',
    });
    if (ok !== true) return;
    await install();
    const after = useUpdate.getState().state;
    if (after.phase === 'error') notify(`更新失败：${after.message}`, 'error');
  };

  const title =
    state.phase === 'installing'
      ? `正在安装 ${state.info.version}…`
      : state.phase === 'checking'
        ? '正在检查更新…'
        : found
          ? `发现新版本 ${found.version}，点击更新`
          : '检查更新';

  return (
    <Button variant="ghost" disabled={busy} onClick={onClick} title={title} className="relative">
      {busy ? <Loader2 size={15} className="animate-spin" /> : <ArrowUpCircle size={15} className={found ? 'text-emerald-400' : undefined} />}
      {found && !busy && <span className="absolute top-1 right-1 size-1.5 rounded-full bg-emerald-400" />}
    </Button>
  );
}
