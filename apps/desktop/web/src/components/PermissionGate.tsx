import { ShieldAlert } from 'lucide-react';
import { useCallback, useEffect, useState } from 'react';
import { backend, type PermissionInfo } from '@/api';
import { Button } from './ui';

/** Blocks the app with an explanation while a required OS permission is missing. */
export function PermissionGate({ children }: { children: React.ReactNode }) {
  const [info, setInfo] = useState<PermissionInfo | null>(null);

  const refresh = useCallback(async () => {
    try {
      setInfo(await backend.permissions());
    } catch {
      setInfo(null);
    }
  }, []);

  useEffect(() => {
    void refresh();
    const timer = setInterval(refresh, 2000);
    return () => clearInterval(timer);
  }, [refresh]);

  if (!info || info.ok) return <>{children}</>;

  return (
    <div className="flex h-full items-center justify-center p-8">
      <div className="w-[460px] rounded-xl border border-line bg-panel p-6">
        <div className="mb-3 flex items-center gap-2 text-base font-semibold text-amber-300">
          <ShieldAlert size={18} /> 需要系统权限
        </div>
        <p className="text-mute">Sky 需要下列权限才能截图和模拟点击。授权对象是 <b className="text-ink">{info.owner}</b>，授权后会自动继续。</p>
        <ul className="my-4 list-disc pl-5 text-ink">
          {info.problems.map((p) => (
            <li key={p}>{p}</li>
          ))}
        </ul>
        <div className="flex flex-wrap gap-2">
          {!info.screenRecording && <Button onClick={() => backend.openPermissionSettings('screen_recording')}>打开「屏幕录制」设置</Button>}
          {!info.accessibility && <Button onClick={() => backend.openPermissionSettings('accessibility')}>打开「辅助功能」设置</Button>}
          <Button variant="primary" onClick={() => backend.requestPermissions().then(setInfo)}>
            请求授权
          </Button>
        </div>
      </div>
    </div>
  );
}
