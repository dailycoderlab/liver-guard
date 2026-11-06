// Operations that touch the backend. Components call these; the stores stay pure.

import { backend } from '@/api';
import { confirmDialog, promptDialog } from '@/components/Dialogs';
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

export async function deleteScript(id: string, name: string) {
  if (!(await confirmDialog({ title: '删除脚本', message: `确定删除「${name}」？此操作不可撤销。`, confirmText: '删除', danger: true }))) return;
  if (await guarded(async () => (await backend.deleteScript(id), true))) {
    if (useEditor.getState().scriptId === id) useEditor.getState().close();
    await refreshScripts();
  }
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

export async function runScript() {
  const state = useEditor.getState();
  if (!state.scriptId || state.runStatus !== 'idle') return;
  if (!(await validateNow())) return;
  if (state.dirty && !(await guarded(async () => (await saveCurrent(), true)))) return;
  const ui = useUi.getState();
  ui.set({ bottomTab: 'logs', bottomOpen: true });
  state.beginRun();
  const started = await guarded(() => backend.runStart(useEditor.getState().toIr(), useEditor.getState().surface, (m) => useEditor.getState().applyRunMessage(m)));
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
  useUi.getState().set({ probing: true, bottomTab: 'probe', bottomOpen: true });
  const rows = await guarded(() => backend.probe(state.toIr(), state.surface));
  useUi.getState().set({ probing: false, ...(rows ? { probeRows: rows } : {}) });
}
