import { useShallow } from 'zustand/react/shallow';
import { Trash2 } from 'lucide-react';
import { NODE_SPECS } from '@/ir/catalog';
import type { Condition } from '@/ir/model';
import { selectFlow, useEditor } from '@/store/editor';
import { ConditionEditor, parseVarValue } from './ConditionEditor';
import { TargetMulti, TargetSelect } from './TargetFields';
import { Button, Empty, Field, Input, NumberInput, Section, Select, TextArea } from './ui';

type Params = Record<string, unknown>;

const num = (v: unknown, fallback: number) => (typeof v === 'number' ? v : fallback);
const str = (v: unknown) => (typeof v === 'string' ? v : '');

export function Inspector() {
  const node = useEditor((s) => {
    const selected = selectFlow(s).nodes.filter((n) => n.selected);
    return selected.length === 1 ? selected[0] : null;
  });
  const count = useEditor((s) => selectFlow(s).nodes.filter((n) => n.selected).length);
  const flows = useEditor(useShallow((s) => Object.keys(s.flows)));
  const vars = useEditor(useShallow((s) => Object.keys(s.vars)));
  const activeFlow = useEditor((s) => s.activeFlow);
  const updateNode = useEditor((s) => s.updateNode);
  const deleteNode = useEditor((s) => s.deleteNode);
  const locked = useEditor((s) => s.runStatus !== 'idle');

  if (!node) return <Empty>{count > 1 ? `已选中 ${count} 个节点` : '选中一个节点以编辑它的属性'}</Empty>;

  const { kind, params } = node.data;
  const spec = NODE_SPECS[kind];
  const set = (patch: Params) => updateNode(node.id, { params: { ...params, ...patch } });

  return (
    <fieldset disabled={locked} className="flex h-full min-w-0 flex-col overflow-y-auto border-0 p-0 disabled:opacity-60">
      <Section
        title={spec.label}
        action={
          kind !== 'start' && (
            <Button small variant="ghost" onClick={() => deleteNode(node.id)} title="删除节点">
              <Trash2 size={13} />
            </Button>
          )
        }
      >
        <p className="text-xs text-mute">{spec.hint}</p>
        <Field label="备注名">
          <Input value={node.data.label ?? ''} placeholder={spec.label} onChange={(e) => updateNode(node.id, { label: e.target.value })} />
        </Field>
        {kind !== 'start' && (
          <Field label="最多经过次数" hint="留空表示不限；超出时报错，防止死循环">
            <Input
              type="number"
              min={1}
              placeholder="不限"
              value={node.data.maxVisits ?? ''}
              onChange={(e) => updateNode(node.id, { maxVisits: e.target.value === '' ? null : Math.max(1, Math.round(Number(e.target.value))) })}
            />
          </Field>
        )}
      </Section>

      {kind !== 'start' && kind !== 'end' && (
        <Section title="参数">
          {(kind === 'tap' || kind === 'longPress') && (
            <>
              <Field label="目标">
                <TargetSelect value={str(params.target)} onChange={(target) => set({ target })} />
              </Field>
              <div className="grid grid-cols-2 gap-2">
                <Field label={kind === 'tap' ? '按下时长 ms' : '长按时长 ms'}>
                  <NumberInput min={0} value={num(params.durationMs, 60)} onChange={(durationMs) => set({ durationMs })} />
                </Field>
                <Field label="点击前延迟 ms">
                  <NumberInput min={0} value={num(params.delayMs, 0)} onChange={(delayMs) => set({ delayMs })} />
                </Field>
              </div>
              <Field label="点位随机偏移" hint="0 = 精确点击锚点；0.2 = 在目标尺寸 20% 内随机">
                <NumberInput min={0} max={1} step={0.05} value={num(params.offsetRatio, 0)} onChange={(offsetRatio) => set({ offsetRatio })} />
              </Field>
            </>
          )}

          {kind === 'swipe' && (
            <>
              <Field label="起点目标">
                <TargetSelect value={str(params.from)} onChange={(from) => set({ from })} />
              </Field>
              <Field label="终点目标" hint="不选则按下面的比例向量拖动">
                <TargetSelect optional value={str(params.to)} onChange={(to) => set({ to: to || null })} />
              </Field>
              {!params.to && (
                <div className="grid grid-cols-2 gap-2">
                  <Field label="水平向量（屏幕宽比例）">
                    <NumberInput step={0.05} value={(params.vector as [number, number] | undefined)?.[0] ?? 0} onChange={(x) => set({ vector: [x, (params.vector as [number, number] | undefined)?.[1] ?? 0] })} />
                  </Field>
                  <Field label="垂直向量（屏幕高比例）">
                    <NumberInput step={0.05} value={(params.vector as [number, number] | undefined)?.[1] ?? -0.3} onChange={(y) => set({ vector: [(params.vector as [number, number] | undefined)?.[0] ?? 0, y] })} />
                  </Field>
                </div>
              )}
              <Field label="滑动时长 ms">
                <NumberInput min={0} value={num(params.durationMs, 300)} onChange={(durationMs) => set({ durationMs })} />
              </Field>
            </>
          )}

          {kind === 'key' && (
            <Field label="按键" hint="return / escape / space / tab / f1…f12 / 单个字符">
              <Input value={str(params.key)} onChange={(e) => set({ key: e.target.value })} />
            </Field>
          )}
          {kind === 'text' && (
            <Field label="文字">
              <TextArea value={str(params.text)} onChange={(e) => set({ text: e.target.value })} />
            </Field>
          )}
          {kind === 'log' && (
            <>
              <Field label="级别">
                <Select value={str(params.level) || 'info'} onChange={(e) => set({ level: e.target.value })}>
                  {['debug', 'info', 'warn', 'error'].map((l) => (
                    <option key={l}>{l}</option>
                  ))}
                </Select>
              </Field>
              <Field label="内容">
                <TextArea value={str(params.msg)} onChange={(e) => set({ msg: e.target.value })} />
              </Field>
            </>
          )}
          {kind === 'snapshot' && (
            <Field label="标签">
              <Input value={str(params.label)} onChange={(e) => set({ label: e.target.value })} />
            </Field>
          )}
          {kind === 'sleep' && (
            <Field label="等待 ms">
              <NumberInput min={0} value={num(params.ms, 1000)} onChange={(ms) => set({ ms })} />
            </Field>
          )}

          {kind === 'waitFor' && (
            <>
              <Field label="任一目标出现即继续">
                <TargetMulti value={(params.any as string[]) ?? []} onChange={(any) => set({ any })} />
              </Field>
              <TimingFields params={params} set={set} />
            </>
          )}
          {kind === 'waitGone' && (
            <>
              <Field label="目标">
                <TargetSelect value={str(params.target)} onChange={(target) => set({ target })} />
              </Field>
              <TimingFields params={params} set={set} />
            </>
          )}
          {kind === 'waitStable' && (
            <div className="grid grid-cols-2 gap-2">
              <Field label="需保持不变 ms">
                <NumberInput min={0} value={num(params.ms, 400)} onChange={(ms) => set({ ms })} />
              </Field>
              <Field label="超时 ms">
                <NumberInput min={0} value={num(params.timeoutMs, 5000)} onChange={(timeoutMs) => set({ timeoutMs })} />
              </Field>
            </div>
          )}

          {kind === 'if' && (
            <Field label="条件">
              <ConditionEditor value={params.when as Condition} onChange={(when) => set({ when })} />
            </Field>
          )}
          {kind === 'switch' && (
            <Field label="按顺序检查，第一个可见的目标走对应分支" hint="都不可见走「其他」">
              <TargetMulti value={(params.cases as string[]) ?? []} onChange={(cases) => set({ cases })} />
            </Field>
          )}
          {kind === 'loop' && (
            <>
              <Field label="模式">
                <Select value={str(params.mode) || 'count'} onChange={(e) => set({ mode: e.target.value })}>
                  <option value="count">固定次数</option>
                  <option value="while">当条件成立</option>
                  <option value="forever">无限循环</option>
                </Select>
              </Field>
              {(params.mode ?? 'count') === 'count' && (
                <Field label="次数">
                  <NumberInput min={1} value={num(params.count, 3)} onChange={(count) => set({ count: Math.max(1, Math.round(count)) })} />
                </Field>
              )}
              {params.mode === 'while' && (
                <Field label="继续循环的条件">
                  <ConditionEditor value={params.when as Condition} onChange={(when) => set({ when })} />
                </Field>
              )}
              {params.mode !== 'count' && (
                <Field label="最多迭代次数" hint="留空使用引擎默认上限">
                  <Input type="number" min={1} value={(params.maxIterations as number | null | undefined) ?? ''} onChange={(e) => set({ maxIterations: e.target.value === '' ? null : Math.max(1, Math.round(Number(e.target.value))) })} />
                </Field>
              )}
            </>
          )}
          {kind === 'retry' && (
            <div className="grid grid-cols-2 gap-2">
              <Field label="最多次数">
                <NumberInput min={1} value={num(params.times, 3)} onChange={(times) => set({ times: Math.max(1, Math.round(times)) })} />
              </Field>
              <Field label="间隔 ms">
                <NumberInput min={0} value={num(params.backoffMs, 500)} onChange={(backoffMs) => set({ backoffMs })} />
              </Field>
            </div>
          )}
          {kind === 'setVar' && (
            <>
              <Field label="变量名">
                <Input list="liverguard-set-vars" value={str(params.name)} onChange={(e) => set({ name: e.target.value })} />
                <datalist id="liverguard-set-vars">
                  {vars.map((v) => (
                    <option key={v} value={v} />
                  ))}
                </datalist>
              </Field>
              <Field label="值" hint="true / false / 数字 / 文字">
                <Input value={String(params.value ?? '')} onChange={(e) => set({ value: parseVarValue(e.target.value) })} />
              </Field>
            </>
          )}
          {kind === 'call' && (
            <Field label="要调用的流程" hint="被调用流程里的「结束」只返回这里">
              <Select value={str(params.flow)} onChange={(e) => set({ flow: e.target.value })}>
                <option value="">请选择流程</option>
                {flows
                  .filter((f) => f !== activeFlow)
                  .map((f) => (
                    <option key={f}>{f}</option>
                  ))}
              </Select>
            </Field>
          )}
        </Section>
      )}
    </fieldset>
  );
}

function TimingFields({ params, set }: { params: Params; set: (patch: Params) => void }) {
  return (
    <div className="grid grid-cols-2 gap-2">
      <Field label="超时 ms">
        <NumberInput min={0} value={num(params.timeoutMs, 10000)} onChange={(timeoutMs) => set({ timeoutMs })} />
      </Field>
      <Field label="检测间隔 ms">
        <NumberInput min={50} value={num(params.intervalMs, 500)} onChange={(intervalMs) => set({ intervalMs })} />
      </Field>
    </div>
  );
}
