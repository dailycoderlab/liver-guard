// Operations that touch the backend. Components call these; the stores stay pure.

import { backend, type Surface } from '@/api';
import { confirmDialog, promptDialog } from '@/components/Dialogs';
import { checkSurface } from '@/lib/surface';
import { saveCurrent, useEditor } from './editor';
import { useUi } from './ui';

const messageOf = (error: unknown) => (error instanceof Error ? error.message : typeof error === 'string' ? error : JSON.stringify(error));

/** Run `task`, surfacing a failure as a toast instead of an unhandled rejection. */
export async function guarded<T>(task: () => Promise<T>): Promise<T | undefined> {
  try {
    return await task();
  } catch (error) {
    useUi.getState().notify(messageOf(error), 'error');
    return undefined;
  }
}

/** Reload the list of windows the surface picker offers. */
export async function refreshWindows() {
  const windows = await guarded(() => backend.listWindows());
  if (windows) useUi.getState().set({ windows });
  return windows;
}

let attentionTimer: ReturnType<typeof setTimeout> | undefined;

/** Draw the eye to the surface picker in the top bar. */
function flashSurfacePicker() {
  clearTimeout(attentionTimer);
  useUi.getState().set({ surfaceAttention: true });
  attentionTimer = setTimeout(() => useUi.getState().set({ surfaceAttention: false }), 2000);
}

/**
 * The surface to run, preview or probe on, or `null` after telling the user what is wrong: nothing
 * chosen yet, or a chosen window that has since closed (a reopened window gets a new id).
 */
export async function requireSurface(): Promise<Surface | null> {
  const { surface, setSurface } = useEditor.getState();
  const notify = useUi.getState().notify;
  if (!surface) {
    notify('请先在顶部选择运行画面（整个屏幕或某个窗口）', 'error');
    flashSurfacePicker();
    return null;
  }
  if (surface.kind === 'window') {
    const windows = await refreshWindows();
    if (windows && checkSurface(surface, windows) === 'gone') {
      setSurface(null);
      notify('所选窗口已经不存在（窗口关闭或重开后编号会变），请在顶部重新选择', 'error');
      flashSurfacePicker();
      return null;
    }
  }
  return surface;
}

export async function refreshScripts() {
  const scripts = await guarded(() => backend.listScripts());
  if (scripts) useUi.getState().set({ scripts });
}

/** Ask what to do with unsaved edits. Returns false when the user cancels. */
export async function resolveUnsaved(): Promise<boolean> {
  const { dirty, scriptName } = useEditor.getState();
  if (!dirty) return true;
  const answer = await confirmDialog({ title: '有未保存的修改', message: `「${scriptName}」有尚未保存的修改。`, confirmText: '保存', altText: '不保存' });
  if (answer === false) return false;
  if (answer === true) return (await guarded(async () => (await saveCurrent(), true))) ?? false;
  return true;
}

export async function save() {
  const state = useEditor.getState();
  if (!state.scriptId) return;
  if (await guarded(async () => (await saveCurrent(), true))) useUi.getState().notify('已保存');
}

export async function openScript(id: string) {
  if (id === useEditor.getState().scriptId) return;
  if (!(await resolveUnsaved())) return;
  const record = await guarded(() => backend.getScript(id));
  if (record) {
    useEditor.getState().loadScript(record.id, record.name, record.ir);
    useUi.getState().set({ probeRows: null });
  }
}

export async function createScript() {
  const name = await promptDialog({ title: '新建脚本', label: '脚本名称', confirmText: '创建' });
  if (!name || !(await resolveUnsaved())) return;
  const record = await guarded(() => backend.createScript(name));
  if (!record) return;
  useEditor.getState().loadScript(record.id, record.name, record.ir);
  await refreshScripts();
}

export async function renameScript(id: string, current: string) {
  const name = await promptDialog({ title: '重命名脚本', initial: current, confirmText: '重命名' });
  if (!name || name === current) return;
  if (await guarded(async () => (await backend.renameScript(id, name), true))) {
    const state = useEditor.getState();
    if (state.scriptId === id) useEditor.setState({ scriptName: name, meta: { ...state.meta, name } });
    await refreshScripts();
  }
}

/** Copy a script. The copy is made from the saved file, so open edits are saved (or knowingly left out) first. */
export async function duplicateScript(id: string, current: string) {
  const name = await promptDialog({ title: '复制脚本', label: '新脚本名称', initial: `${current} 副本`, confirmText: '复制' });
  if (!name) return;
  if (id === useEditor.getState().scriptId && !(await resolveUnsaved())) return;
  const record = await guarded(() => backend.duplicateScript(id, name));
  if (!record) return;
  await refreshScripts();
  useUi.getState().notify(`已复制为「${record.name}」`);
}

/** Save a script (and its template images) to a file. The file holds the saved version. */
export async function exportScript(id: string) {
  if (id === useEditor.getState().scriptId && !(await resolveUnsaved())) return;
  const where = await guarded(() => backend.exportScript(id));
  if (where) useUi.getState().notify(`已导出到 ${where}`);
}

/** Add a script from an exported file as a new script; it never replaces an existing one. */
export async function importScript() {
  const record = await guarded(() => backend.importScript());
  if (!record) return;
  await refreshScripts();
  useUi.getState().notify(`已导入「${record.name}」`);
}

export async function deleteScript(id: string, name: string) {
  if (!(await confirmDialog({ title: '删除脚本', message: `确定删除「${name}」？此操作不可撤销。`, confirmText: '删除', danger: true }))) return;
  if (await guarded(async () => (await backend.deleteScript(id), true))) {
    if (useEditor.getState().scriptId === id) useEditor.getState().close();
    await refreshScripts();
  }
}

/** Let the user choose an image file; resolves null when they cancel. */
function pickImageFile(): Promise<File | null> {
  return new Promise((resolve) => {
    const input = document.createElement('input');
    input.type = 'file';
    input.accept = 'image/png,image/jpeg,image/webp,image/bmp';
    input.onchange = () => resolve(input.files?.[0] ?? null);
    input.oncancel = () => resolve(null);
    input.click();
  });
}

/** Upload a screenshot and hand it to the 画面 panel, where it is boxed like a captured frame. */
export async function uploadImage() {
  if (!useEditor.getState().scriptId) return;
  const file = await pickImageFile();
  if (!file) return;
  const packet = await guarded(() => backend.loadImage(file));
  if (packet) useUi.getState().set({ pendingFrame: packet, rightTab: 'live' });
}

/** Validate the open script; true when it has no errors. */
export async function validateNow(): Promise<boolean> {
  const state = useEditor.getState();
  const problems = await guarded(() => backend.validate(state.toIr()));
  if (!problems) return false;
  state.setProblems(problems);
  const errors = problems.filter((p) => p.severity === 'error').length;
  if (problems.length) useUi.getState().set({ bottomTab: 'problems', bottomOpen: true });
  else useUi.getState().notify('校验通过');
  return errors === 0;
}

/** Run the open script; `repeat` walks the whole graph that many times in one run. */
export async function runScript(repeat = 1) {
  const state = useEditor.getState();
  if (!state.scriptId || state.runStatus !== 'idle') return;
  const surface = await requireSurface();
  if (!surface) return;
  if (!(await validateNow())) return;
  if (state.dirty && !(await guarded(async () => (await saveCurrent(), true)))) return;
  const ui = useUi.getState();
  ui.set({ bottomTab: 'logs', bottomOpen: true });
  state.beginRun();
  const started = await guarded(() => backend.runStart(useEditor.getState().toIr(), surface, Math.max(1, Math.round(repeat) || 1), (m) => useEditor.getState().applyRunMessage(m)));
  if (!started) useEditor.getState().setRunStatus('idle');
}

export async function stopScript() {
  const state = useEditor.getState();
  if (state.runStatus !== 'running') return;
  state.setRunStatus('stopping');
  await guarded(() => backend.runStop());
}

export async function probeNow() {
  const state = useEditor.getState();
  if (!state.scriptId || useUi.getState().probing) return;
  const surface = await requireSurface();
  if (!surface) return;
  useUi.getState().set({ probing: true, bottomTab: 'probe', bottomOpen: true });
  const rows = await guarded(() => backend.probe(state.toIr(), surface));
  useUi.getState().set({ probing: false, ...(rows ? { probeRows: rows } : {}) });
}
