import { create } from 'zustand';
import type { ProbeRow, ScriptSummary, WindowInfo } from '@/api';

export type LeftTab = 'scripts' | 'nodes' | 'targets';
export type RightTab = 'inspector' | 'live';
export type BottomTab = 'logs' | 'problems' | 'probe';

interface UiState {
  leftTab: LeftTab;
  rightTab: RightTab;
  bottomTab: BottomTab;
  bottomOpen: boolean;
  scripts: ScriptSummary[];
  windows: WindowInfo[];
  probeRows: ProbeRow[] | null;
  probing: boolean;
  busy: string | null;
  toast: { id: number; text: string; tone: 'info' | 'error' } | null;
  set: (patch: Partial<Omit<UiState, 'set' | 'notify'>>) => void;
  notify: (text: string, tone?: 'info' | 'error') => void;
}

let toastId = 0;

export const useUi = create<UiState>()((set) => ({
  leftTab: 'scripts',
  rightTab: 'inspector',
  bottomTab: 'logs',
  bottomOpen: true,
  scripts: [],
  windows: [],
  probeRows: null,
  probing: false,
  busy: null,
  toast: null,
  set: (patch) => set(patch),
  notify: (text, tone = 'info') => {
    const id = (toastId += 1);
    set({ toast: { id, text, tone } });
    setTimeout(() => set((s) => (s.toast?.id === id ? { toast: null } : {})), tone === 'error' ? 6000 : 2500);
  },
}));
