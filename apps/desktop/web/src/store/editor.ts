import { applyEdgeChanges, applyNodeChanges, type Connection, type EdgeChange, type NodeChange } from '@xyflow/react';
import { create } from 'zustand';
import { backend, type NodeState, type RunMessage, type RunResult, type Surface } from '@/api';
import { NODE_SPECS } from '@/ir/catalog';
import {
  appendAfter,
  autoLayout,
  connect,
  createNode,
  fromFlowState,
  placeNext,
  pruneEdges,
  removeNodes,
  renameTargetInParams,
  toFlowState,
  uniqueName,
  type FlowState,
  type SkyNode,
} from '@/ir/graph';
import { emptyDoc, MAIN_FLOW, type IrDoc, type IrMeta, type NodeType, type Problem, type Target, type VarValue } from '@/ir/model';

/** The part of the editor that is saved, and that undo/redo walks. */
export interface DocSlice {
  meta: IrMeta;
  vars: Record<string, VarValue>;
  targets: Record<string, Target>;
  entry: string;
  flows: Record<string, FlowState>;
}

export interface LogEntry {
  id: number;
  ts: number;
  level: string;
  event: string;
  payload: unknown;
}

export interface NodeRuntime {
  state: NodeState;
  score?: number;
}

export type RunStatus = 'idle' | 'running' | 'stopping';

const HISTORY_LIMIT = 100;
const COALESCE_MS = 800;
const LOG_LIMIT = 2000;

export interface EditorState extends DocSlice {
  scriptId: string | null;
  scriptName: string;
  activeFlow: string;
  dirty: boolean;
  past: DocSlice[];
  future: DocSlice[];

  problems: Problem[];
  surface: Surface;
  runStatus: RunStatus;
  runtime: Record<string, NodeRuntime>;
  logs: LogEntry[];
  lastResult: RunResult | null;
  lastCommitKey: string | null;
  lastCommitAt: number;

  // document
  loadScript: (id: string, name: string, ir: IrDoc) => void;
  close: () => void;
  toIr: () => IrDoc;
  setActiveFlow: (name: string) => void;

  // history
  commit: (key?: string) => void;
  undo: () => void;
  redo: () => void;
  markSaved: () => void;

  // canvas
  onNodesChange: (changes: NodeChange<SkyNode>[]) => void;
  onEdgesChange: (changes: EdgeChange[]) => void;
  onConnect: (connection: Connection) => void;
  beginDrag: () => void;
  addNode: (kind: NodeType, options?: { params?: Record<string, unknown>; position?: { x: number; y: number }; after?: string | null }) => string;
  updateNode: (id: string, patch: { params?: Record<string, unknown>; label?: string | null; maxVisits?: number | null }) => void;
  deleteNode: (id: string) => void;
  layout: () => void;

  // flows
  addFlow: (name: string) => string | null;
  deleteFlow: (name: string) => void;
  renameFlow: (from: string, to: string) => string | null;

  // targets, variables, meta
  addTarget: (target: Target, base: string) => string;
  patchTarget: (name: string, patch: Partial<Target>) => void;
  renameTarget: (from: string, to: string) => string | null;
  deleteTarget: (name: string) => void;
  setVar: (name: string, value: VarValue) => void;
  removeVar: (name: string) => void;
  setMaxDuration: (ms: number) => void;

  // running
  setProblems: (problems: Problem[]) => void;
  setSurface: (surface: Surface) => void;
  beginRun: () => void;
  setRunStatus: (status: RunStatus) => void;
  applyRunMessage: (message: RunMessage) => void;
  clearRuntime: () => void;
}

function docSlice(state: DocSlice): DocSlice {
  return { meta: state.meta, vars: state.vars, targets: state.targets, entry: state.entry, flows: state.flows };
}

function sliceFromIr(ir: IrDoc): DocSlice {
  return {
    meta: ir.meta,
    vars: ir.vars ?? {},
    targets: ir.targets ?? {},
    entry: ir.entry || MAIN_FLOW,
    flows: Object.fromEntries(Object.entries(ir.flows).map(([name, flow]) => [name, toFlowState(flow)])),
  };
}

export function sliceToIr(slice: DocSlice): IrDoc {
  return {
    irVersion: 2,
    meta: slice.meta,
    vars: slice.vars,
    targets: slice.targets,
    entry: slice.entry,
    flows: Object.fromEntries(Object.entries(slice.flows).map(([name, flow]) => [name, fromFlowState(flow)])),
  };
}

const runtimeKey = (flow: string, nodeId: string) => `${flow}/${nodeId}`;
export { runtimeKey };

let logSeq = 0;

export const useEditor = create<EditorState>()((set, get) => {
  const emptySlice = sliceFromIr(emptyDoc(''));

  /** Replace the active flow. */
  const setFlow = (flow: FlowState, extra: Partial<EditorState> = {}) => {
    const { activeFlow, flows } = get();
    set({ flows: { ...flows, [activeFlow]: flow }, dirty: true, ...extra });
  };

  return {
    ...emptySlice,
    scriptId: null,
    scriptName: '',
    activeFlow: MAIN_FLOW,
    dirty: false,
    past: [],
    future: [],
    problems: [],
    surface: { kind: 'fullScreen' },
    runStatus: 'idle',
    runtime: {},
    logs: [],
    lastResult: null,
    lastCommitKey: null,
    lastCommitAt: 0,

    // ----------------------------------------------------------- document
    loadScript: (id, name, ir) =>
      set({
        ...sliceFromIr(ir),
        scriptId: id,
        scriptName: name,
        activeFlow: ir.entry || MAIN_FLOW,
        dirty: false,
        past: [],
        future: [],
        problems: [],
        runtime: {},
        logs: [],
        lastResult: null,
        lastCommitKey: null,
      }),

    close: () =>
      set({
        ...emptySlice,
        scriptId: null,
        scriptName: '',
        activeFlow: MAIN_FLOW,
        dirty: false,
        past: [],
        future: [],
        problems: [],
        runtime: {},
      }),

    toIr: () => sliceToIr(get()),

    setActiveFlow: (name) => {
      if (get().flows[name]) set({ activeFlow: name });
    },

    // ------------------------------------------------------------ history
    /**
     * Remember the current document before a change. Edits that share a `key`
     * within a short window (typing in one field) collapse into one step.
     */
    commit: (key) => {
      const state = get();
      const now = Date.now();
      if (key && key === state.lastCommitKey && now - state.lastCommitAt < COALESCE_MS) {
        set({ lastCommitAt: now });
        return;
      }
      set({
        past: [...state.past, docSlice(state)].slice(-HISTORY_LIMIT),
        future: [],
        lastCommitKey: key ?? null,
        lastCommitAt: now,
      });
    },

    undo: () => {
      const state = get();
      const previous = state.past[state.past.length - 1];
      if (!previous) return;
      const flows = previous.flows;
      set({
        ...previous,
        past: state.past.slice(0, -1),
        future: [docSlice(state), ...state.future].slice(0, HISTORY_LIMIT),
        activeFlow: flows[state.activeFlow] ? state.activeFlow : previous.entry,
        dirty: true,
        lastCommitKey: null,
      });
    },

    redo: () => {
      const state = get();
      const next = state.future[0];
      if (!next) return;
      set({
        ...next,
        past: [...state.past, docSlice(state)].slice(-HISTORY_LIMIT),
        future: state.future.slice(1),
        activeFlow: next.flows[state.activeFlow] ? state.activeFlow : next.entry,
        dirty: true,
        lastCommitKey: null,
      });
    },

    markSaved: () => set({ dirty: false }),

    // ------------------------------------------------------------- canvas
    onNodesChange: (changes) => {
      const flow = get().flows[get().activeFlow];
      if (!flow) return;
      const removing = changes.some((c) => c.type === 'remove');
      if (removing) get().commit();
      const nodes = applyNodeChanges(changes, flow.nodes);
      // Removing a node also removes its edges (React Flow reports them separately,
      // but pruning here keeps the state consistent within one update).
      const next: FlowState = removing ? { nodes, edges: pruneEdges({ nodes, edges: flow.edges }) } : { nodes, edges: flow.edges };
      const edits = changes.some((c) => c.type === 'remove' || c.type === 'add' || c.type === 'replace' || (c.type === 'position' && c.dragging !== undefined));
      if (edits) setFlow(next);
      else set({ flows: { ...get().flows, [get().activeFlow]: next } });
    },

    onEdgesChange: (changes) => {
      const flow = get().flows[get().activeFlow];
      if (!flow) return;
      const removing = changes.some((c) => c.type === 'remove');
      if (removing) get().commit();
      const edges = applyEdgeChanges(changes, flow.edges);
      if (removing || changes.some((c) => c.type === 'add' || c.type === 'replace')) setFlow({ nodes: flow.nodes, edges });
      else set({ flows: { ...get().flows, [get().activeFlow]: { nodes: flow.nodes, edges } } });
    },

    onConnect: (connection) => {
      const flow = get().flows[get().activeFlow];
      if (!flow) return;
      const edges = connect(flow, connection);
      if (edges === flow.edges) return;
      get().commit();
      setFlow({ nodes: flow.nodes, edges });
    },

    /** One undo step per drag, not one per mouse move. */
    beginDrag: () => get().commit(),

    addNode: (kind, options = {}) => {
      const flow = get().flows[get().activeFlow];
      // New nodes follow the selected one; on a fresh flow they follow the start node.
      const selected = flow.nodes.find((n) => n.selected)?.id ?? (flow.nodes.length === 1 ? flow.nodes[0].id : undefined);
      const after = options.after === undefined ? selected : (options.after ?? undefined);
      const position = options.position ?? placeNext(flow, after);
      const node = createNode(flow, kind, options.params ?? NODE_SPECS[kind].defaults(), position);
      get().commit();
      const edges = appendAfter(flow, after, node);
      const deselected = flow.nodes.map((n) => (n.selected ? { ...n, selected: false } : n));
      setFlow({ nodes: [...deselected, { ...node, selected: true }], edges });
      return node.id;
    },

    updateNode: (id, patch) => {
      const flow = get().flows[get().activeFlow];
      const node = flow?.nodes.find((n) => n.id === id);
      if (!node) return;
      get().commit(`node:${id}`);
      const data = { ...node.data };
      if (patch.params) data.params = patch.params;
      if (patch.label !== undefined) data.label = patch.label || undefined;
      if (patch.maxVisits !== undefined) data.maxVisits = patch.maxVisits ?? undefined;
      const nodes = flow.nodes.map((n) => (n.id === id ? { ...n, data } : n));
      // Editing a switch can remove a handle; the edge that hung on it goes with it.
      setFlow({ nodes, edges: pruneEdges({ nodes, edges: flow.edges }) });
    },

    deleteNode: (id) => {
      const flow = get().flows[get().activeFlow];
      if (!flow || flow.nodes.find((n) => n.id === id)?.data.kind === 'start') return;
      get().commit();
      setFlow(removeNodes(flow, new Set([id])));
    },

    layout: () => {
      const flow = get().flows[get().activeFlow];
      if (!flow) return;
      get().commit();
      setFlow({ nodes: autoLayout(flow), edges: flow.edges });
    },

    // -------------------------------------------------------------- flows
    addFlow: (name) => {
      const clean = name.trim();
      if (!clean || get().flows[clean]) return null;
      get().commit();
      const fresh = sliceFromIr(emptyDoc('')).flows[MAIN_FLOW];
      set({ flows: { ...get().flows, [clean]: fresh }, activeFlow: clean, dirty: true });
      return clean;
    },

    deleteFlow: (name) => {
      const state = get();
      if (name === state.entry || !state.flows[name]) return;
      state.commit();
      const flows = { ...state.flows };
      delete flows[name];
      // Calls into a deleted flow are cleared rather than left dangling.
      for (const [key, flow] of Object.entries(flows)) {
        flows[key] = {
          ...flow,
          nodes: flow.nodes.map((n) =>
            n.data.kind === 'call' && n.data.params.flow === name ? { ...n, data: { ...n.data, params: { ...n.data.params, flow: '' } } } : n,
          ),
        };
      }
      set({ flows, activeFlow: state.activeFlow === name ? state.entry : state.activeFlow, dirty: true });
    },

    renameFlow: (from, to) => {
      const state = get();
      const clean = to.trim();
      if (!clean || clean === from || state.flows[clean] || !state.flows[from]) return null;
      state.commit();
      const flows: Record<string, FlowState> = {};
      for (const [key, flow] of Object.entries(state.flows)) {
        flows[key === from ? clean : key] = {
          ...flow,
          nodes: flow.nodes.map((n) =>
            n.data.kind === 'call' && n.data.params.flow === from ? { ...n, data: { ...n.data, params: { ...n.data.params, flow: clean } } } : n,
          ),
        };
      }
      set({
        flows,
        entry: state.entry === from ? clean : state.entry,
        activeFlow: state.activeFlow === from ? clean : state.activeFlow,
        dirty: true,
      });
      return clean;
    },

    // ---------------------------------------------- targets, vars, meta
    addTarget: (target, base) => {
      const state = get();
      const name = uniqueName(Object.keys(state.targets), base || 'target');
      state.commit();
      set({ targets: { ...state.targets, [name]: target }, dirty: true });
      return name;
    },

    patchTarget: (name, patch) => {
      const state = get();
      const current = state.targets[name];
      if (!current) return;
      state.commit(`target:${name}`);
      set({ targets: { ...state.targets, [name]: { ...current, ...patch } as Target }, dirty: true });
    },

    renameTarget: (from, to) => {
      const state = get();
      const clean = to.trim();
      if (!clean || clean === from || state.targets[clean] || !state.targets[from]) return null;
      state.commit();
      const targets: Record<string, Target> = {};
      for (const [key, value] of Object.entries(state.targets)) targets[key === from ? clean : key] = value;
      const flows: Record<string, FlowState> = {};
      for (const [key, flow] of Object.entries(state.flows)) {
        flows[key] = {
          ...flow,
          nodes: flow.nodes.map((n) => ({
            ...n,
            data: {
              ...n.data,
              params: renameTargetInParams(n.data.params, from, clean) as Record<string, unknown>,
            },
          })),
          // Switch handles are named after their target.
          edges: flow.edges.map((e) => (e.sourceHandle === `case:${from}` ? { ...e, sourceHandle: `case:${clean}` } : e)),
        };
      }
      set({ targets, flows, dirty: true });
      return clean;
    },

    deleteTarget: (name) => {
      const state = get();
      if (!state.targets[name]) return;
      state.commit();
      const targets = { ...state.targets };
      delete targets[name];
      set({ targets, dirty: true });
    },

    setVar: (name, value) => {
      const state = get();
      state.commit(`var:${name}`);
      set({ vars: { ...state.vars, [name]: value }, dirty: true });
    },

    removeVar: (name) => {
      const state = get();
      state.commit();
      const vars = { ...state.vars };
      delete vars[name];
      set({ vars, dirty: true });
    },

    setMaxDuration: (ms) => {
      const state = get();
      state.commit('maxDuration');
      set({ meta: { ...state.meta, maxDurationMs: Math.max(1000, Math.round(ms)) }, dirty: true });
    },

    // ------------------------------------------------------------ running
    setProblems: (problems) => set({ problems }),
    setSurface: (surface) => set({ surface }),
    setRunStatus: (status) => set({ runStatus: status }),

    beginRun: () => set({ runStatus: 'running', runtime: {}, logs: [], lastResult: null }),

    applyRunMessage: (message) => {
      if (message.kind === 'node') {
        const key = runtimeKey(message.flow, message.nodeId);
        set({ runtime: { ...get().runtime, [key]: { state: message.state, score: message.score } } });
      } else if (message.kind === 'log') {
        const entry: LogEntry = { id: (logSeq += 1), ts: Date.now(), level: message.level, event: message.event, payload: message.payload };
        set({ logs: [...get().logs, entry].slice(-LOG_LIMIT) });
      } else {
        set({ runStatus: 'idle', lastResult: message.result });
      }
    },

    clearRuntime: () => set({ runtime: {}, lastResult: null }),
  };
});

/** Convenience: the active flow, never undefined once a script is open. */
const NO_FLOW: FlowState = { nodes: [], edges: [] };

export function selectFlow(state: EditorState): FlowState {
  return state.flows[state.activeFlow] ?? NO_FLOW;
}

export async function saveCurrent(): Promise<void> {
  const state = useEditor.getState();
  if (!state.scriptId) return;
  await backend.saveScript(state.scriptId, state.toIr());
  state.markSaved();
}
