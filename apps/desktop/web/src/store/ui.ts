import { create } from 'zustand';
import type { FramePacket, ProbeRow, ScriptSummary, WindowInfo } from '@/api';
import { loadSizes, saveSizes, type PanelSizes } from '@/lib/layout';

export type LeftTab = 'scripts' | 'nodes' | 'targets';
export type RightTab = 'inspector' | 'live';
export type BottomTab = 'logs' | 'problems' | 'probe';

interface UiState extends PanelSizes {
  leftTab: LeftTab;
  rightTab: RightTab;
  bottomTab: BottomTab;
  bottomOpen: boolean;
  scripts: ScriptSummary[];
  windows: WindowInfo[];
  probeRows: ProbeRow[] | null;
  probing: boolean;
  /** An uploaded screenshot waiting for the 画面 panel to take it over. */
  pendingFrame: FramePacket | null;
  /** How many rounds the 循环 button runs; kept for the session so it can be re-run quickly. */
  repeatCount: number;
  /** The surface picker is highlighted for a moment after something needed a surface that was not usable. */
  surfaceAttention: boolean;
  busy: string | null;
  toast: { id: number; text: string; tone: 'info' | 'error' } | null;
  set: (patch: Partial<Omit<UiState, 'set' | 'notify' | 'setSize'>>) => void;
  /** Resize a panel; `persist` writes the layout (done once, when a drag ends). */
  setSize: (key: keyof PanelSizes, value: number, persist?: boolean) => void;
  notify: (text: string, tone?: 'info' | 'error') => void;
}

let toastId = 0;

export const useUi = create<UiState>()((set, get) => ({
  ...loadSizes(),
  leftTab: 'scripts',
  rightTab: 'inspector',
  bottomTab: 'logs',
  bottomOpen: true,
  scripts: [],
  windows: [],
  probeRows: null,
  probing: false,
  pendingFrame: null,
  repeatCount: 5,
  surfaceAttention: false,
  busy: null,
  toast: null,
  set: (patch) => set(patch),
  setSize: (key, value, persist = false) => {
    set({ [key]: value } as Pick<PanelSizes, typeof key>);
    if (persist) {
      const { leftWidth, rightWidth, bottomHeight } = get();
      saveSizes({ leftWidth, rightWidth, bottomHeight });
    }
  },
  notify: (text, tone = 'info') => {
    const id = (toastId += 1);
    set({ toast: { id, text, tone } });
    setTimeout(() => set((s) => (s.toast?.id === id ? { toast: null } : {})), tone === 'error' ? 6000 : 2500);
  },
}));
