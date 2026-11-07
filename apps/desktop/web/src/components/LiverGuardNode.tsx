import { Handle, Position, useUpdateNodeInternals, type NodeProps } from '@xyflow/react';
import { memo, useEffect } from 'react';
import { NODE_SPECS, outputHandles, summarize, targetRefs } from '@/ir/catalog';
import { HEADER_IN_TOP } from '@/ir/nodeBox';
import type { LiverGuardNode as LiverGuardNodeType } from '@/ir/graph';
import { cn } from '@/lib/cn';
import { useEditor } from '@/store/editor';
import { TargetThumb } from './TargetThumb';

function LiverGuardNodeView({ id, data, selected }: NodeProps<LiverGuardNodeType>) {
  const spec = NODE_SPECS[data.kind];
  const runtime = useEditor((s) => s.runtime[id]);
  const updateInternals = useUpdateNodeInternals();
  const handles = outputHandles(data.kind, data.params);
  const handleKey = handles.map((h) => h.id).join('|');
  const refs = [...new Set(targetRefs(data.kind, data.params))].slice(0, 3);
  const Icon = spec.icon;

  // Handles of a switch change with its cases; React Flow must re-measure them.
  useEffect(() => updateInternals(id), [handleKey, id, updateInternals]);

  const state = runtime?.state;
  return (
    <div
      className={cn(
        'w-[220px] rounded-lg border bg-panel text-ink shadow-lg transition-shadow',
        selected ? 'border-accent ring-1 ring-accent' : 'border-line',
        state === 'running' && 'liverguard-running border-accent',
        state === 'ok' && 'border-emerald-500',
        state === 'failed' && 'border-red-500',
      )}
    >
      <div className="relative flex h-8 items-center gap-1.5 rounded-t-[7px] px-2.5 text-white">
        {/* Nodes without output rows (end) take their input here. */}
        {data.kind !== 'start' && handles.length === 0 && <Handle type="target" position={Position.Left} style={{ top: HEADER_IN_TOP }} className="liverguard-handle-in" />}
        <Icon size={14} />
        <span className="truncate font-medium">{data.label || spec.label}</span>
        {data.label && <span className="ml-auto text-[11px] opacity-70">{spec.label}</span>}
        {runtime?.score !== undefined && <span className="ml-auto font-mono text-[11px]">{runtime.score.toFixed(2)}</span>}
      </div>
      {(refs.length > 0 || summarize(data.kind, data.params)) && (
        <div className="flex h-11 items-center gap-1.5 px-2.5">
          {refs.map((name) => (
            <TargetThumb key={name} name={name} />
          ))}
          <span className="min-w-0 flex-1 truncate text-xs text-mute">{summarize(data.kind, data.params)}</span>
        </div>
      )}
      {handles.map((h, i) => (
        <div key={h.id} className="relative flex h-6 items-center justify-end border-t border-line/60 pr-4 pl-2.5 text-[11px]">
          {/* The input shares the first output's row, so a straight chain is a straight line. */}
          {i === 0 && data.kind !== 'start' && <Handle type="target" position={Position.Left} className="liverguard-handle-in" />}
          <span className={h.tone === 'fail' ? 'text-red-300' : 'text-emerald-300'}>{h.label}</span>
          <Handle type="source" position={Position.Right} id={h.id} className={h.tone === 'fail' ? 'liverguard-handle-fail' : 'liverguard-handle-ok'} />
        </div>
      ))}
    </div>
  );
}

export const LiverGuardNodeComponent = memo(LiverGuardNodeView);
