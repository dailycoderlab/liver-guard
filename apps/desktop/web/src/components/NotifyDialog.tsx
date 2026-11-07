import * as Dialog from '@radix-ui/react-dialog';
import { Eye, EyeOff, Send, X } from 'lucide-react';
import { useEffect, useState } from 'react';
import { backend } from '@/api';
import type { NotifyChannel, NotifyConfig } from '@/api/types';
import { guarded } from '@/store/actions';
import { useUi } from '@/store/ui';
import { Button, Field, Input, Select } from './ui';

const CHANNELS: { id: NotifyChannel; label: string; hint: string }[] = [
  { id: 'dingtalk', label: '钉钉群机器人', hint: '群设置 → 智能群助手 → 添加机器人 → 自定义。安全设置选「加签」或「自定义关键词」都可以，对应填在下面。' },
  { id: 'wecom', label: '企业微信群机器人', hint: '群聊 → 添加群机器人，复制 Webhook 地址。' },
  { id: 'feishu', label: '飞书群机器人', hint: '群设置 → 机器人 → 添加自定义机器人。安全设置选「签名校验」或「自定义关键词」都可以，对应填在下面。' },
  { id: 'generic', label: '通用 Webhook', hint: '向该地址 POST 一个 JSON：{ event, title, text, details }，details 里有脚本名、节点、原因、轮次等字段。' },
];

const EMPTY: NotifyConfig = { enabled: false, channel: 'dingtalk', url: '', secret: '', keyword: '', onLoopDone: true };

type TestState = { kind: 'idle' } | { kind: 'sending' } | { kind: 'ok' } | { kind: 'error'; text: string };

/** App-wide failure alert settings. Not part of any script, and never exported with one. */
export function NotifyDialog({ open, onOpenChange, onSaved }: { open: boolean; onOpenChange: (open: boolean) => void; onSaved?: (config: NotifyConfig) => void }) {
  const [config, setConfig] = useState<NotifyConfig>(EMPTY);
  const [loaded, setLoaded] = useState(false);
  const [reveal, setReveal] = useState(false);
  const [test, setTest] = useState<TestState>({ kind: 'idle' });
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!open) return;
    setLoaded(false);
    setReveal(false);
    setTest({ kind: 'idle' });
    setError(null);
    void guarded(() => backend.getNotifyConfig()).then((value) => {
      setConfig(value ?? EMPTY);
      setLoaded(true);
    });
  }, [open]);

  const channel = CHANNELS.find((c) => c.id === config.channel) ?? CHANNELS[0];
  const supportsSecret = config.channel === 'dingtalk' || config.channel === 'feishu';
  const supportsKeyword = config.channel !== 'wecom';
  const patch = (next: Partial<NotifyConfig>) => {
    setConfig((c) => ({ ...c, ...next }));
    setTest({ kind: 'idle' });
    setError(null);
  };

  const sendTest = async () => {
    setTest({ kind: 'sending' });
    try {
      await backend.sendTestNotification(config);
      setTest({ kind: 'ok' });
    } catch (e) {
      setTest({ kind: 'error', text: e instanceof Error ? e.message : String(e) });
    }
  };

  const save = async () => {
    setSaving(true);
    setError(null);
    try {
      await backend.saveNotifyConfig(config);
      onSaved?.(config);
      useUi.getState().notify(config.enabled ? '运行通知已开启' : '设置已保存（运行通知未开启）');
      onOpenChange(false);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setSaving(false);
    }
  };

  return (
    <Dialog.Root open={open} onOpenChange={onOpenChange}>
      <Dialog.Portal>
        <Dialog.Overlay className="fixed inset-0 z-50 bg-black/55" />
        <Dialog.Content aria-describedby={undefined} className="fixed top-[12%] left-1/2 z-50 w-[480px] -translate-x-1/2 rounded-xl border border-line bg-panel p-4 shadow-2xl outline-none">
          <div className="mb-3 flex items-center justify-between">
            <Dialog.Title className="text-sm font-semibold">运行通知</Dialog.Title>
            <Dialog.Close className="text-mute hover:text-ink">
              <X size={16} />
            </Dialog.Close>
          </div>

          <p className="mb-3 text-[11px] leading-5 text-mute">
            脚本因为没有被处理的失败而中止时（找不到目标、截图卡住等），向群机器人发一条消息；循环执行也会随之停止。脚本里接了「失败 / 超时」出口的失败视为已处理，不会通知；手动停止也不会。循环执行（多于一轮）全部成功结束时，也可以发一条完成通知。
            这是整个应用的设置，对所有脚本生效，不会随脚本导出。
          </p>

          <label className="mb-3 flex cursor-pointer items-center gap-2">
            <input type="checkbox" className="size-4 accent-[var(--color-accent)]" checked={config.enabled} onChange={(e) => patch({ enabled: e.target.checked })} />
            <span>脚本失败时发送通知</span>
          </label>
          <label className="mb-3 -mt-1 ml-6 flex cursor-pointer items-center gap-2">
            <input type="checkbox" className="size-4 accent-[var(--color-accent)]" checked={config.onLoopDone} disabled={!config.enabled} onChange={(e) => patch({ onLoopDone: e.target.checked })} />
            <span className={config.enabled ? '' : 'text-mute'}>循环执行全部完成时也通知</span>
          </label>

          <div className={loaded ? 'flex flex-col gap-3' : 'pointer-events-none flex flex-col gap-3 opacity-50'}>
            <Field label="接收方" hint={channel.hint}>
              <Select value={config.channel} onChange={(e) => patch({ channel: e.target.value as NotifyChannel })}>
                {CHANNELS.map((c) => (
                  <option key={c.id} value={c.id}>
                    {c.label}
                  </option>
                ))}
              </Select>
            </Field>
            <Field label="Webhook 地址" hint="含访问令牌，等同密码：只保存在本机的设置文件里，不会写进日志或导出文件。">
              <div className="relative">
                <Input type={reveal ? 'text' : 'password'} autoComplete="off" spellCheck={false} className="pr-8" placeholder="https://…" value={config.url} onChange={(e) => patch({ url: e.target.value })} />
                <button type="button" className="absolute inset-y-0 right-2 text-mute hover:text-ink" title={reveal ? '隐藏' : '显示'} onClick={() => setReveal((r) => !r)}>
                  {reveal ? <EyeOff size={14} /> : <Eye size={14} />}
                </button>
              </div>
            </Field>
            {supportsSecret && (
              <Field label={config.channel === 'dingtalk' ? '加签密钥（可选，SEC 开头）' : '签名密钥（可选）'}>
                <Input type={reveal ? 'text' : 'password'} autoComplete="off" spellCheck={false} value={config.secret} onChange={(e) => patch({ secret: e.target.value })} />
              </Field>
            )}
            {supportsKeyword && (
              <Field label="自定义关键词（可选）" hint="机器人设置了关键词时填它；消息里没有这个词时会自动加在开头。">
                <Input value={config.keyword} onChange={(e) => patch({ keyword: e.target.value })} />
              </Field>
            )}
          </div>

          {test.kind === 'ok' && <p className="mt-3 text-emerald-400">测试消息已发出，请到群里确认收到。</p>}
          {test.kind === 'error' && <p className="mt-3 break-all text-red-400">发送失败：{test.text}</p>}
          {error && <p className="mt-3 break-all text-red-400">{error}</p>}

          <div className="mt-4 flex items-center justify-between gap-2">
            <Button onClick={sendTest} disabled={!loaded || !config.url.trim() || test.kind === 'sending'} title="用当前填写的内容发一条测试消息（不需要先保存）">
              <Send size={13} /> {test.kind === 'sending' ? '发送中…' : '发送测试通知'}
            </Button>
            <div className="flex gap-2">
              <Button onClick={() => onOpenChange(false)}>取消</Button>
              <Button variant="primary" onClick={save} disabled={!loaded || saving}>
                保存
              </Button>
            </div>
          </div>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
