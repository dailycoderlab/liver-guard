import * as Popover from '@radix-ui/react-popover';
import { Play, Repeat, Square } from 'lucide-react';
import { useState } from 'react';
import { runScript, stopScript } from '@/store/actions';
import { useEditor } from '@/store/editor';
import { useUi } from '@/store/ui';
import { Button, Field, NumberInput } from './ui';

/** 运行 runs the script once; 循环 asks how many rounds and runs them back to back in one run. */
export function RunControls() {
  const open = useEditor((s) => !!s.scriptId);
  const runStatus = useEditor((s) => s.runStatus);
  const repeatCount = useUi((s) => s.repeatCount);
  const [popover, setPopover] = useState(false);

  if (runStatus !== 'idle') {
    return (
      <Button variant="danger" disabled={runStatus === 'stopping'} onClick={stopScript}>
        <Square size={13} /> {runStatus === 'stopping' ? '正在停止…' : '停止'}
      </Button>
    );
  }

  const start = () => {
    setPopover(false);
    void runScript(repeatCount);
  };

  return (
    <div className="flex items-center gap-1.5">
      <Button variant="primary" disabled={!open} onClick={() => runScript(1)} title="运行一次，运行前自动保存">
        <Play size={13} /> 运行
      </Button>
      <Popover.Root open={popover} onOpenChange={setPopover}>
        <Popover.Trigger asChild>
          <Button disabled={!open} title="设定次数，循环运行">
            <Repeat size={13} /> 循环
          </Button>
        </Popover.Trigger>
        <Popover.Portal>
          <Popover.Content align="end" sideOffset={8} className="z-50 w-64 rounded-xl border border-line bg-panel p-3 shadow-2xl outline-none">
            <Field label="循环执行次数" hint="整张图从开始节点连续走这么多轮；每轮变量会清空，「结束」节点只结束当前一轮，某轮失败或手动停止则整个终止。运行没有总时长限制，要停请手动停止。">
              <NumberInput min={1} max={99999} value={repeatCount} onChange={(v) => useUi.getState().set({ repeatCount: Math.max(1, Math.round(v)) })} />
            </Field>
            <Button variant="primary" className="mt-3 w-full" onClick={start}>
              <Play size={13} /> 开始循环 {repeatCount} 次
            </Button>
          </Popover.Content>
        </Popover.Portal>
      </Popover.Root>
    </div>
  );
}
