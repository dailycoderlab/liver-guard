import { RefreshCw } from 'lucide-react';
import { useEffect } from 'react';
import { refreshWindows } from '@/store/actions';
import { useEditor } from '@/store/editor';
import { useUi } from '@/store/ui';
import { parseSurface, surfaceValue } from '@/lib/surface';
import { Button, Select } from './ui';

/**
 * Which screen or window the script works on: the one place to choose it. Preview, probing and
 * running all use it, so it lives in the top bar next to 运行 and must be set before any of them.
 */
export function SurfacePicker() {
  const surface = useEditor((s) => s.surface);
  const setSurface = useEditor((s) => s.setSurface);
  const locked = useEditor((s) => s.runStatus !== 'idle');
  const windows = useUi((s) => s.windows);
  const attention = useUi((s) => s.surfaceAttention);

  useEffect(() => {
    void refreshWindows();
    // Coming back from the target app is when its window list is most likely to have changed.
    const onFocus = () => void refreshWindows();
    window.addEventListener('focus', onFocus);
    return () => window.removeEventListener('focus', onFocus);
  }, []);

  const value = surfaceValue(surface);
  const known = surface?.kind !== 'window' || windows.some((w) => w.number === surface.number);

  return (
    <div className="flex items-center gap-1" title="脚本在哪个窗口上截图、匹配和点击。预览、探测、运行都用它，运行前必须先选">
      <span className="whitespace-nowrap text-xs text-mute">窗口</span>
      <Select
        className={`h-8 w-44 ${attention ? 'border-red-400 ring-2 ring-red-400/40' : ''} ${value === '' ? 'text-mute' : ''}`}
        value={value}
        disabled={locked}
        onChange={(e) => setSurface(parseSurface(e.target.value))}
      >
        <option value="" disabled>
          请选择运行窗口…
        </option>
        <option value="full">整个屏幕</option>
        {windows.map((w) => (
          <option key={w.number} value={w.number}>
            {w.label}
          </option>
        ))}
        {!known && surface?.kind === 'window' && <option value={surface.number}>窗口 #{surface.number}（已不存在）</option>}
      </Select>
      <Button variant="ghost" disabled={locked} title="刷新窗口列表" onClick={() => void refreshWindows()}>
        <RefreshCw size={14} />
      </Button>
    </div>
  );
}
