import { useShallow } from 'zustand/react/shallow';
import { Plus, X } from 'lucide-react';
import { useEditor } from '@/store/editor';
import { TargetThumb } from './TargetThumb';
import { Select } from './ui';

/** Choose one of the script's named targets. */
export function TargetSelect({ value, onChange, optional }: { value: string | null | undefined; onChange: (name: string) => void; optional?: boolean }) {
  const names = useEditor(useShallow((s) => Object.keys(s.targets)));
  const missing = value && !names.includes(value);
  return (
    <div className="flex items-center gap-2">
      {value ? <TargetThumb name={value} className="h-8 w-12" zoomable /> : null}
      <Select value={value ?? ''} onChange={(e) => onChange(e.target.value)} className={missing ? 'border-red-500' : undefined}>
        <option value="">{optional ? '（无）' : '请选择目标'}</option>
        {missing && <option value={value}>{value}（不存在）</option>}
        {names.map((n) => (
          <option key={n} value={n}>
            {n}
          </option>
        ))}
      </Select>
    </div>
  );
}

/** An ordered list of targets. */
export function TargetMulti({ value, onChange }: { value: string[]; onChange: (names: string[]) => void }) {
  const names = useEditor(useShallow((s) => Object.keys(s.targets)));
  return (
    <div className="flex flex-col gap-1.5">
      {value.map((name, i) => (
        <div key={`${name}-${i}`} className="flex items-center gap-2 rounded-md border border-line bg-canvas px-1.5 py-1">
          <TargetThumb name={name} zoomable />
          <span className="min-w-0 flex-1 truncate">{name}</span>
          <button className="text-mute hover:text-ink" title="移除" onClick={() => onChange(value.filter((_, j) => j !== i))}>
            <X size={14} />
          </button>
        </div>
      ))}
      <div className="flex items-center gap-1.5">
        <Plus size={14} className="text-mute" />
        <Select
          value=""
          onChange={(e) => {
            if (e.target.value) onChange([...value, e.target.value]);
          }}
        >
          <option value="">{names.length ? '添加目标…' : '先在「目标」里创建'}</option>
          {names
            .filter((n) => !value.includes(n))
            .map((n) => (
              <option key={n} value={n}>
                {n}
              </option>
            ))}
        </Select>
      </div>
    </div>
  );
}
