import { useEffect } from 'react';
import { backend } from '@/api';
import { Canvas } from '@/components/Canvas';
import { DialogHost } from '@/components/Dialogs';
import { FlowTabs } from '@/components/FlowTabs';
import { Inspector } from '@/components/Inspector';
import { LivePanel } from '@/components/LivePanel';
import { LogPanel } from '@/components/LogPanel';
import { Palette } from '@/components/Palette';
import { Splitter } from '@/components/Splitter';
import { PermissionGate } from '@/components/PermissionGate';
import { ScriptsPanel } from '@/components/ScriptsPanel';
import { TargetsPanel } from '@/components/TargetsPanel';
import { TopBar } from '@/components/TopBar';
import { Empty } from '@/components/ui';
import { cn } from '@/lib/cn';
import { MIN_CENTER_HEIGHT, MIN_CENTER_WIDTH } from '@/lib/layout';
import { resolveUnsaved, runScript, save } from '@/store/actions';
import { useEditor } from '@/store/editor';
import { useUi, type LeftTab, type RightTab } from '@/store/ui';

const LEFT_TABS: [LeftTab, string][] = [
  ['scripts', '脚本'],
  ['nodes', '节点'],
  ['targets', '目标'],
];
const RIGHT_TABS: [RightTab, string][] = [
  ['inspector', '属性'],
  ['live', '画面'],
];

function Tabs<T extends string>({ tabs, value, onChange }: { tabs: [T, string][]; value: T; onChange: (v: T) => void }) {
  return (
    <div className="flex h-9 shrink-0 border-b border-line">
      {tabs.map(([id, label]) => (
        <button key={id} onClick={() => onChange(id)} className={cn('flex-1 border-b-2 text-xs', value === id ? 'border-accent text-ink' : 'border-transparent text-mute hover:text-ink')}>
          {label}
        </button>
      ))}
    </div>
  );
}

function useShortcuts() {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const mod = e.metaKey || e.ctrlKey;
      if (!mod) return;
      const editing = (e.target as HTMLElement | null)?.closest('input, textarea, select');
      const key = e.key.toLowerCase();
      if (key === 's') {
        e.preventDefault();
        void save();
      } else if (key === 'z' && !editing) {
        e.preventDefault();
        const state = useEditor.getState();
        if (state.runStatus !== 'idle') return;
        if (e.shiftKey) state.redo();
        else state.undo();
      } else if (key === 'y' && !editing) {
        e.preventDefault();
        useEditor.getState().redo();
      } else if (key === 'enter') {
        e.preventDefault();
        void runScript();
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, []);
}

/** Ask before closing the window with unsaved edits. */
function useCloseGuard() {
  useEffect(() => {
    if (!backend.native) {
      const onUnload = (e: BeforeUnloadEvent) => {
        if (useEditor.getState().dirty) e.preventDefault();
      };
      window.addEventListener('beforeunload', onUnload);
      return () => window.removeEventListener('beforeunload', onUnload);
    }
    let unlisten: (() => void) | undefined;
    void import('@tauri-apps/api/window').then(async ({ getCurrentWindow }) => {
      const win = getCurrentWindow();
      unlisten = await win.onCloseRequested(async (event) => {
        if (!useEditor.getState().dirty) return;
        event.preventDefault();
        if (await resolveUnsaved()) await win.destroy();
      });
    });
    return () => unlisten?.();
  }, []);
}

export function App() {
  const leftTab = useUi((s) => s.leftTab);
  const rightTab = useUi((s) => s.rightTab);
  const bottomOpen = useUi((s) => s.bottomOpen);
  const leftWidth = useUi((s) => s.leftWidth);
  const rightWidth = useUi((s) => s.rightWidth);
  const bottomHeight = useUi((s) => s.bottomHeight);
  const toast = useUi((s) => s.toast);
  const set = useUi((s) => s.set);
  const scriptId = useEditor((s) => s.scriptId);
  useShortcuts();
  useCloseGuard();

  return (
    <PermissionGate>
      <div className="flex h-full flex-col">
        <TopBar />
        <div className="flex min-h-0 flex-1">
          <aside style={{ width: leftWidth, maxWidth: `calc(100vw - ${rightWidth + MIN_CENTER_WIDTH}px)` }} className="relative flex shrink-0 flex-col border-r border-line bg-panel">
            <Splitter edge="right" size="leftWidth" />
            <Tabs tabs={LEFT_TABS} value={leftTab} onChange={(v) => set({ leftTab: v })} />
            <div className="min-h-0 flex-1">{leftTab === 'scripts' ? <ScriptsPanel /> : leftTab === 'nodes' ? scriptId ? <Palette /> : <Empty>先打开一个脚本</Empty> : <TargetsPanel />}</div>
          </aside>

          <main className="flex min-w-0 flex-1 flex-col">
            {scriptId ? (
              <>
                <FlowTabs />
                <div className="min-h-0 flex-1">
                  <Canvas />
                </div>
              </>
            ) : (
              <Empty>
                <div>
                  <div className="mb-1 text-base text-ink">从左侧选择或新建一个脚本</div>
                  <div>脚本是一张流程图：拖入节点、连线，用「是 / 否」「出现 / 超时」控制走向。</div>
                </div>
              </Empty>
            )}
            {bottomOpen ? (
              <div style={{ height: bottomHeight, maxHeight: `calc(100vh - ${MIN_CENTER_HEIGHT + 40}px)` }} className="relative shrink-0 border-t border-line bg-panel">
                <Splitter edge="top" size="bottomHeight" />
                <LogPanel />
              </div>
            ) : (
              <button className="h-7 shrink-0 border-t border-line bg-panel text-xs text-mute hover:text-ink" onClick={() => set({ bottomOpen: true })}>
                展开日志面板
              </button>
            )}
          </main>

          <aside style={{ width: rightWidth, maxWidth: `calc(100vw - ${leftWidth + MIN_CENTER_WIDTH}px)` }} className="relative flex shrink-0 flex-col border-l border-line bg-panel">
            <Splitter edge="left" size="rightWidth" />
            <Tabs tabs={RIGHT_TABS} value={rightTab} onChange={(v) => set({ rightTab: v })} />
            <div className="min-h-0 flex-1">
              <div className={cn('h-full', rightTab !== 'inspector' && 'hidden')}>{scriptId ? <Inspector /> : <Empty>先打开一个脚本</Empty>}</div>
              <div className={cn('h-full', rightTab !== 'live' && 'hidden')}>
                <LivePanel active={rightTab === 'live'} />
              </div>
            </div>
          </aside>
        </div>
      </div>
      {toast && (
        <div className={cn('fixed bottom-6 left-1/2 z-[60] -translate-x-1/2 rounded-lg border px-4 py-2 shadow-xl', toast.tone === 'error' ? 'border-red-500/60 bg-red-950 text-red-100' : 'border-line bg-raised text-ink')}>{toast.text}</div>
      )}
      <DialogHost />
    </PermissionGate>
  );
}
