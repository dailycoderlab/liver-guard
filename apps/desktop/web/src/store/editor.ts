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
  type LiverGuardNode,
} from '@/ir/graph';
import { emptyDoc, type IrDoc, type IrMeta, type NodeType, type Problem, type Target } from '@/ir/model';
import { useUi } from './ui';

/** The part of the editor that is saved, and that undo/redo walks. */
export interface DocSlice {
  meta: IrMeta;
  targets: Record<string, Target>;
  flow: FlowState;
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
  dirty: boolean;
  /** Bumped after each 整理, so the canvas can bring the result into view. */
  layoutTick: number;
  past: DocSlice[];
  future: DocSlice[];

  problems: Problem[];
  /** Where the script runs; `null` until the user picks one (nothing runs, previews or probes before that). */
  surface: Surface | null;
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

  // history
  commit: (key?: string) => void;
  undo: () => void;
  redo: () => void;
  markSaved: () => void;

  // canvas
  onNodesChange: (changes: NodeChange<LiverGuardNode>[]) => void;
  onEdgesChange: (changes: EdgeChange[]) => void;
  onConnect: (connection: Connection) => void;
  beginDrag: () => void;
  addNode: (kind: NodeType, options?: { params?: Record<string, unknown>; position?: { x: number; y: number }; after?: string | null }) => string;
  updateNode: (id: string, patch: { params?: Record<string, unknown>; label?: string | null; maxVisits?: number | null }) => void;
  deleteNode: (id: string) => void;
  layout: () => Promise<void>;

  // targets
  addTarget: (target: Target, base: string) => string;
  patchTarget: (name: string, patch: Partial<Target>) => void;
  renameTarget: (from: string, to: string) => string | null;
  deleteTarget: (name: string) => void;

  // running
  setProblems: (problems: Problem[]) => void;
  setSurface: (surface: Surface | null) => void;
  beginRun: () => void;
  setRunStatus: (status: RunStatus) => void;
  applyRunMessage: (message: RunMessage) => void;
  clearRuntime: () => void;
  clearLogs: () => void;
}

function docSlice(state: DocSlice): DocSlice {
  return { meta: state.meta, targets: state.targets, flow: state.flow };
}

function sliceFromIr(ir: IrDoc): DocSlice {
  return {
    meta: ir.meta,
    targets: ir.targets ?? {},
    flow: toFlowState(ir.flow),
  };
}

export function sliceToIr(slice: DocSlice): IrDoc {
  return {
    irVersion: 3,
    meta: slice.meta,
    targets: slice.targets,
    flow: fromFlowState(slice.flow),
  };
}

let logSeq = 0;

export const useEditor = create<EditorState>()((set, get) => {
  const emptySlice = sliceFromIr(emptyDoc(''));

  /** Replace the graph. */
  const setFlow = (flow: FlowState, extra: Partial<EditorState> = {}) => {
    set({ flow, dirty: true, ...extra });
  };

  return {
    ...emptySlice,
    scriptId: null,
    scriptName: '',
    dirty: false,
    layoutTick: 0,
    past: [],
    future: [],
    problems: [],
    surface: null,
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
        dirty: false,
        past: [],
        future: [],
        problems: [],
        runtime: {},
      }),

    toIr: () => sliceToIr(get()),

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
      set({
        ...previous,
        past: state.past.slice(0, -1),
        future: [docSlice(state), ...state.future].slice(0, HISTORY_LIMIT),
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
        dirty: true,
        lastCommitKey: null,
      });
    },

    markSaved: () => set({ dirty: false }),

    // ------------------------------------------------------------- canvas
    onNodesChange: (changes) => {
      const flow = get().flow;
      const removing = changes.some((c) => c.type === 'remove');
      if (removing) get().commit();
      const nodes = applyNodeChanges(changes, flow.nodes);
      // Removing a node also removes its edges (React Flow reports them separately,
      // but pruning here keeps the state consistent within one update).
      const next: FlowState = removing ? { nodes, edges: pruneEdges({ nodes, edges: flow.edges }) } : { nodes, edges: flow.edges };
      const edits = changes.some((c) => c.type === 'remove' || c.type === 'add' || c.type === 'replace' || (c.type === 'position' && c.dragging !== undefined));
      if (edits) setFlow(next);
      else set({ flow: next });
    },

    onEdgesChange: (changes) => {
      const flow = get().flow;
      const removing = changes.some((c) => c.type === 'remove');
      if (removing) get().commit();
      const edges = applyEdgeChanges(changes, flow.edges);
      if (removing || changes.some((c) => c.type === 'add' || c.type === 'replace')) setFlow({ nodes: flow.nodes, edges });
      else set({ flow: { nodes: flow.nodes, edges } });
    },

    onConnect: (connection) => {
      const flow = get().flow;
      const edges = connect(flow, connection);
      if (edges === flow.edges) return;
      get().commit();
      setFlow({ nodes: flow.nodes, edges });
    },

    /** One undo step per drag, not one per mouse move. */
    beginDrag: () => get().commit(),

    addNode: (kind, options = {}) => {
      const flow = get().flow;
      // New nodes follow the selected one; on a fresh script they follow the start node.
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
      const flow = get().flow;
      const node = flow.nodes.find((n) => n.id === id);
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
      const flow = get().flow;
      if (flow.nodes.find((n) => n.id === id)?.data.kind === 'start') return;
      get().commit();
      setFlow(removeNodes(flow, new Set([id])));
    },

    layout: async () => {
      let laid;
      try {
        laid = await autoLayout(get().flow);
      } catch (error) {
        useUi.getState().notify(`整理失败：${error instanceof Error ? error.message : String(error)}`, 'error');
        return;
      }
      // The graph may have been edited while ELK worked: apply positions to what is there now.
      const positions = new Map(laid.map((n) => [n.id, n.position]));
      const flow = get().flow;
      get().commit();
      setFlow({ nodes: flow.nodes.map((n) => ({ ...n, position: positions.get(n.id) ?? n.position })), edges: flow.edges });
      set((s) => ({ layoutTick: s.layoutTick + 1 }));
    },

    // ---------------------------------------------- targets
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
      const flow: FlowState = {
        nodes: state.flow.nodes.map((n) => ({
          ...n,
          data: {
            ...n.data,
            params: renameTargetInParams(n.data.params, from, clean) as Record<string, unknown>,
          },
        })),
        // Switch handles are named after their target.
        edges: state.flow.edges.map((e) => (e.sourceHandle === `case:${from}` ? { ...e, sourceHandle: `case:${clean}` } : e)),
      };
      set({ targets, flow, dirty: true });
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

    // ------------------------------------------------------------ running
    setProblems: (problems) => set({ problems }),
    setSurface: (surface) => set({ surface }),
    setRunStatus: (status) => set({ runStatus: status }),

    beginRun: () => set({ runStatus: 'running', runtime: {}, logs: [], lastResult: null }),

    applyRunMessage: (message) => {
      if (message.kind === 'node') {
        set({ runtime: { ...get().runtime, [message.nodeId]: { state: message.state, score: message.score } } });
      } else if (message.kind === 'log') {
        const entry: LogEntry = { id: (logSeq += 1), ts: Date.now(), level: message.level, event: message.event, payload: message.payload };
        set({ logs: [...get().logs, entry].slice(-LOG_LIMIT) });
      } else {
        set({ runStatus: 'idle', lastResult: message.result });
      }
    },

    clearRuntime: () => set({ runtime: {}, lastResult: null }),
    clearLogs: () => set({ logs: [], lastResult: null }),
  };
});

export async function saveCurrent(): Promise<void> {
  const state = useEditor.getState();
  if (!state.scriptId) return;
  await backend.saveScript(state.scriptId, state.toIr());
  state.markSaved();
}
