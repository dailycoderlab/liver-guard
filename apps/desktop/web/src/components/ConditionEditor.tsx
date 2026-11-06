import { useShallow } from 'zustand/react/shallow';
import { Plus, Trash2 } from 'lucide-react';
import type { Condition } from '@/ir/model';
import { useEditor } from '@/store/editor';
import { TargetMulti, TargetSelect } from './TargetFields';
import { Button, Input, Select } from './ui';

type Kind = Condition['kind'];

const KIND_LABELS: Record<Kind, string> = {
  visible: '目标可见（任一）',
  missing: '目标不可见',
  var: '变量比较',
  not: '取反',
  all: '全部满足',
  any: '任一满足',
};

const OPS = ['eq', 'ne', 'lt', 'le', 'gt', 'ge', 'contains', 'match'] as const;
const OP_LABELS: Record<(typeof OPS)[number], string> = { eq: '等于', ne: '不等于', lt: '小于', le: '小于等于', gt: '大于', ge: '大于等于', contains: '包含', match: '正则匹配' };

export function defaultCondition(kind: Kind): Condition {
  switch (kind) {
    case 'visible':
      return { kind, targets: [] };
    case 'missing':
      return { kind, target: '' };
    case 'var':
      return { kind, name: '', op: 'eq', value: 0 };
    case 'not':
      return { kind, cond: defaultCondition('visible') };
    case 'all':
    case 'any':
      return { kind, conds: [defaultCondition('visible')] };
  }
}

/** "true" / "false" / numbers keep their type; everything else is a string. */
export function parseVarValue(text: string): boolean | number | string {
  if (text === 'true') return true;
  if (text === 'false') return false;
  const n = Number(text);
  return text.trim() !== '' && Number.isFinite(n) ? n : text;
}

export function ConditionEditor({ value, onChange, depth = 0 }: { value: Condition | null | undefined; onChange: (next: Condition) => void; depth?: number }) {
  const vars = useEditor(useShallow((s) => Object.keys(s.vars)));
  const cond = value ?? defaultCondition('visible');
  return (
    <div className={depth ? 'flex flex-col gap-2 rounded-md border border-line bg-canvas/50 p-2' : 'flex flex-col gap-2'}>
      <Select value={cond.kind} onChange={(e) => onChange(defaultCondition(e.target.value as Kind))}>
        {(Object.keys(KIND_LABELS) as Kind[]).map((k) => (
          <option key={k} value={k}>
            {KIND_LABELS[k]}
          </option>
        ))}
      </Select>
      {cond.kind === 'visible' && <TargetMulti value={cond.targets} onChange={(targets) => onChange({ ...cond, targets })} />}
      {cond.kind === 'missing' && <TargetSelect value={cond.target} onChange={(target) => onChange({ ...cond, target })} />}
      {cond.kind === 'var' && (
        <div className="grid grid-cols-[1fr_auto_1fr] gap-1.5">
          <Input list="sky-vars" placeholder="变量名" value={cond.name} onChange={(e) => onChange({ ...cond, name: e.target.value })} />
          <Select className="w-24" value={cond.op ?? 'eq'} onChange={(e) => onChange({ ...cond, op: e.target.value as (typeof OPS)[number] })}>
            {OPS.map((op) => (
              <option key={op} value={op}>
                {OP_LABELS[op]}
              </option>
            ))}
          </Select>
          <Input placeholder="值" value={String(cond.value)} onChange={(e) => onChange({ ...cond, value: parseVarValue(e.target.value) })} />
          <datalist id="sky-vars">
            {vars.map((v) => (
              <option key={v} value={v} />
            ))}
          </datalist>
        </div>
      )}
      {cond.kind === 'not' && <ConditionEditor depth={depth + 1} value={cond.cond} onChange={(inner) => onChange({ ...cond, cond: inner })} />}
      {(cond.kind === 'all' || cond.kind === 'any') && (
        <>
          {cond.conds.map((inner, i) => (
            <div key={i} className="flex items-start gap-1">
              <div className="min-w-0 flex-1">
                <ConditionEditor depth={depth + 1} value={inner} onChange={(next) => onChange({ ...cond, conds: cond.conds.map((c, j) => (j === i ? next : c)) })} />
              </div>
              <Button variant="ghost" small title="移除" onClick={() => onChange({ ...cond, conds: cond.conds.filter((_, j) => j !== i) })}>
                <Trash2 size={13} />
              </Button>
            </div>
          ))}
          <Button small onClick={() => onChange({ ...cond, conds: [...cond.conds, defaultCondition('visible')] })}>
            <Plus size={13} /> 添加条件
          </Button>
        </>
      )}
    </div>
  );
}
