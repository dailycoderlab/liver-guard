import { Crop, Crosshair, ImageUp, Pencil, Trash2 } from 'lucide-react';
import { useMemo, useState } from 'react';
import { targetRefs } from '@/ir/catalog';
import { cn } from '@/lib/cn';
import { uploadImage } from '@/store/actions';
import { useEditor } from '@/store/editor';
import { useUi } from '@/store/ui';
import { confirmDialog, promptDialog } from './Dialogs';
import { TargetCropDialog } from './TargetCropDialog';
import { TargetThumb } from './TargetThumb';
import { Button, Empty, Field, NumberInput, Select } from './ui';

export function TargetsPanel() {
  const targets = useEditor((s) => s.targets);
  const flow = useEditor((s) => s.flow);
  const scriptId = useEditor((s) => s.scriptId);
  const { renameTarget, deleteTarget, patchTarget } = useEditor.getState();
  const [open, setOpen] = useState<string | null>(null);
  const [cropping, setCropping] = useState<string | null>(null);

  const usage = useMemo(() => {
    const count: Record<string, number> = {};
    for (const node of flow.nodes) for (const ref of new Set(targetRefs(node.data.kind, node.data.params))) count[ref] = (count[ref] ?? 0) + 1;
    return count;
  }, [flow]);

  if (!scriptId) return <Empty>先打开一个脚本</Empty>;
  const names = Object.keys(targets);

  return (
    <div className="flex h-full flex-col">
      <div className="flex flex-col gap-1.5 p-3">
        <Button className="w-full" onClick={() => useUi.getState().set({ rightTab: 'live' })}>
          <Crosshair size={14} /> 从画面截取新目标
        </Button>
        <Button className="w-full" onClick={() => void uploadImage()}>
          <ImageUp size={14} /> 上传图片并框选
        </Button>
      </div>
      {names.length === 0 ? (
        <Empty>还没有目标。到「画面」里框选按钮或图标即可创建。</Empty>
      ) : (
        <ul className="flex-1 overflow-y-auto px-2 pb-2">
          {names.map((name) => {
            const target = targets[name];
            const used = usage[name] ?? 0;
            return (
              <li key={name} className="mb-1 rounded-md border border-line bg-raised/50">
                <div className="group flex cursor-pointer items-center gap-2 p-1.5" onClick={() => setOpen(open === name ? null : name)}>
                  <TargetThumb name={name} className="h-9 w-12" zoomable />
                  <div className="min-w-0 flex-1">
                    <div className="truncate">{name}</div>
                    <div className={cn('text-[11px]', used ? 'text-mute' : 'text-amber-400')}>{used ? `被 ${used} 个节点使用` : '未被使用'}</div>
                  </div>
                  {target.type === 'template' && (
                    <button
                      className="hidden text-mute hover:text-ink group-hover:block"
                      title="框选图片：只保留图片里的一部分，定位更精细"
                      onClick={(e) => {
                        e.stopPropagation();
                        setCropping(name);
                      }}
                    >
                      <Crop size={13} />
                    </button>
                  )}
                  <button
                    className="hidden text-mute hover:text-ink group-hover:block"
                    title="重命名"
                    onClick={async (e) => {
                      e.stopPropagation();
                      const next = await promptDialog({ title: '重命名目标', initial: name, confirmText: '重命名' });
                      if (next && renameTarget(name, next) === null && next !== name) useUi.getState().notify('名称已存在', 'error');
                    }}
                  >
                    <Pencil size={13} />
                  </button>
                  <button
                    className="hidden text-mute hover:text-red-400 group-hover:block"
                    title="删除"
                    onClick={async (e) => {
                      e.stopPropagation();
                      const message = used ? `「${name}」正被 ${used} 个节点使用，删除后这些节点会报错。` : undefined;
                      if (await confirmDialog({ title: `删除目标「${name}」`, message, confirmText: '删除', danger: true })) deleteTarget(name);
                    }}
                  >
                    <Trash2 size={13} />
                  </button>
                </div>
                {open === name && target.type === 'template' && (
                  <div className="grid grid-cols-2 gap-2 border-t border-line p-2">
                    <Field label="命中阈值" hint="低于此分不点击">
                      <NumberInput min={0} max={1} step={0.01} value={target.threshold ?? 0.85} onChange={(threshold) => patchTarget(name, { threshold })} />
                    </Field>
                    <Field label="告警阈值" hint="低于此分记警告">
                      <NumberInput min={0} max={1} step={0.01} value={target.warnThreshold ?? 0.7} onChange={(warnThreshold) => patchTarget(name, { warnThreshold })} />
                    </Field>
                    <Field label="颜色校验">
                      <Select value={target.verify ?? 'none'} onChange={(e) => patchTarget(name, { verify: e.target.value as 'none' | 'rgb' })}>
                        <option value="none">关闭</option>
                        <option value="rgb">RGB 校验</option>
                      </Select>
                    </Field>
                    <Field label="多个命中时">
                      <Select value={target.multi ?? 'nearest-last'} onChange={(e) => patchTarget(name, { multi: e.target.value as 'nearest-last' | 'highest' | 'center' | 'nth' })}>
                        <option value="nearest-last">离上次最近</option>
                        <option value="highest">得分最高</option>
                        <option value="center">最靠近中心</option>
                        <option value="nth">第 N 个</option>
                      </Select>
                    </Field>
                  </div>
                )}
                {open === name && target.type === 'point' && <div className="border-t border-line p-2 text-xs text-mute">固定坐标点，按屏幕比例定位，不做图像匹配。</div>}
              </li>
            );
          })}
        </ul>
      )}
      <TargetCropDialog name={cropping} onClose={() => setCropping(null)} />
    </div>
  );
}
