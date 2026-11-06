import { AlertTriangle, CheckCircle2, XCircle } from 'lucide-react';
import { useEffect, useRef } from 'react';
import { cn } from '@/lib/cn';
import { useEditor } from '@/store/editor';
import { useUi, type BottomTab } from '@/store/ui';
import { Empty } from './ui';

const LEVEL_COLOR: Record<string, string> = { debug: 'text-mute', info: 'text-ink', warn: 'text-amber-300', warning: 'text-amber-300', error: 'text-red-400' };

function payloadText(payload: unknown): string {
  if (payload === null || payload === undefined) return '';
  if (typeof payload === 'string') return payload;
  return Object.entries(payload as Record<string, unknown>)
    .map(([k, v]) => `${k}=${typeof v === 'object' ? JSON.stringify(v) : String(v)}`)
    .join('  ');
}

const TIME = new Intl.DateTimeFormat('zh-CN', { hour12: false, hour: '2-digit', minute: '2-digit', second: '2-digit' });

function Logs() {
  const logs = useEditor((s) => s.logs);
  const result = useEditor((s) => s.lastResult);
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const el = ref.current;
    if (el && el.scrollHeight - el.scrollTop - el.clientHeight < 80) el.scrollTop = el.scrollHeight;
  }, [logs.length]);
  if (logs.length === 0 && !result) return <Empty>运行后日志显示在这里</Empty>;
  return (
    <div ref={ref} className="h-full overflow-y-auto px-3 py-2 font-mono text-xs leading-5 select-text">
      {logs.map((l) => (
        <div key={l.id} className={cn('flex gap-3', LEVEL_COLOR[l.level] ?? 'text-ink')}>
          <span className="shrink-0 text-mute">{TIME.format(l.ts)}</span>
          <span className="shrink-0 font-medium">{l.event}</span>
          <span className="min-w-0 break-all text-mute">{payloadText(l.payload)}</span>
        </div>
      ))}
      {result && (
        <div className={cn('mt-1 flex items-center gap-1.5 font-sans text-sm', result.state === 'done' ? 'text-emerald-400' : result.state === 'stopped' ? 'text-amber-300' : 'text-red-400')}>
          {result.state === 'done' ? <CheckCircle2 size={14} /> : <XCircle size={14} />}
          {result.state === 'done' ? '运行完成' : result.state === 'stopped' ? '已停止' : `运行失败：${result.errorMsg ?? result.errorCode ?? '未知错误'}`}
          <span className="text-mute">· {result.stepCount} 步 · {((result.endedAt - result.startedAt) / 1000).toFixed(1)}s</span>
        </div>
      )}
    </div>
  );
}

function Problems() {
  const problems = useEditor((s) => s.problems);
  const setActiveFlow = useEditor((s) => s.setActiveFlow);
  if (problems.length === 0) return <Empty>没有问题</Empty>;
  return (
    <ul className="h-full overflow-y-auto p-2">
      {problems.map((p, i) => (
        <li
          key={i}
          className="flex cursor-pointer items-start gap-2 rounded px-2 py-1 hover:bg-raised"
          onClick={() => {
            if (p.flow) setActiveFlow(p.flow);
            if (p.node) {
              const state = useEditor.getState();
              const flow = state.flows[state.activeFlow];
              useEditor.setState({ flows: { ...state.flows, [state.activeFlow]: { ...flow, nodes: flow.nodes.map((n) => ({ ...n, selected: n.id === p.node })) } } });
              useUi.getState().set({ rightTab: 'inspector' });
            }
          }}
        >
          {p.severity === 'error' ? <XCircle size={14} className="mt-0.5 shrink-0 text-red-400" /> : <AlertTriangle size={14} className="mt-0.5 shrink-0 text-amber-300" />}
          <span className="flex-1">{p.message}</span>
          {p.flow && (
            <span className="shrink-0 font-mono text-[11px] text-mute">
              {p.flow}
              {p.node ? `/${p.node}` : ''}
            </span>
          )}
        </li>
      ))}
    </ul>
  );
}

function Probe() {
  const rows = useUi((s) => s.probeRows);
  const probing = useUi((s) => s.probing);
  if (probing) return <Empty>正在探测当前画面…</Empty>;
  if (!rows) return <Empty>点击顶部「探测」，检查每个目标在当前画面里能否找到</Empty>;
  if (rows.length === 0) return <Empty>脚本里没有目标</Empty>;
  return (
    <table className="w-full text-xs">
      <thead className="sticky top-0 bg-panel text-mute">
        <tr className="text-left">
          <th className="px-3 py-1.5 font-medium">目标</th>
          <th className="px-2 font-medium">结果</th>
          <th className="px-2 font-medium">得分 / 阈值</th>
          <th className="px-2 font-medium">位置</th>
          <th className="px-2 font-medium">策略</th>
          <th className="px-2 font-medium">耗时</th>
        </tr>
      </thead>
      <tbody>
        {rows.map((r) => (
          <tr key={r.target} className="border-t border-line/60">
            <td className="px-3 py-1.5">{r.target}</td>
            <td className={cn('px-2', r.ok ? 'text-emerald-400' : r.found ? 'text-amber-300' : 'text-red-400')}>{r.ok ? '可点击' : r.found ? '低于阈值' : '未找到'}</td>
            <td className="px-2 font-mono">{r.score === null ? '—' : `${r.score.toFixed(3)} / ${r.threshold?.toFixed(2) ?? '—'}`}</td>
            <td className="px-2 font-mono">{r.point ? `${Math.round(r.point[0])}, ${Math.round(r.point[1])}` : '—'}</td>
            <td className="px-2">{r.strategy ?? '—'}</td>
            <td className="px-2 font-mono">{r.elapsedMs} ms</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

export function LogPanel() {
  const tab = useUi((s) => s.bottomTab);
  const problems = useEditor((s) => s.problems.length);
  const set = useUi((s) => s.set);
  const tabs: [BottomTab, string][] = [
    ['logs', '日志'],
    ['problems', problems ? `问题 ${problems}` : '问题'],
    ['probe', '探测'],
  ];
  return (
    <div className="flex h-full flex-col">
      <div className="flex h-8 shrink-0 items-center gap-1 border-b border-line px-2">
        {tabs.map(([id, label]) => (
          <button key={id} onClick={() => set({ bottomTab: id })} className={cn('rounded px-2.5 py-1 text-xs', tab === id ? 'bg-raised text-ink' : 'text-mute hover:text-ink')}>
            {label}
          </button>
        ))}
        <button className="ml-auto text-xs text-mute hover:text-ink" onClick={() => set({ bottomOpen: false })}>
          收起
        </button>
      </div>
      <div className="min-h-0 flex-1">{tab === 'logs' ? <Logs /> : tab === 'problems' ? <Problems /> : <Probe />}</div>
    </div>
  );
}
