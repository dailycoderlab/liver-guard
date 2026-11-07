// Pure graph helpers: IR <-> React Flow conversion and the editing rules.
// Nothing here touches React, so every rule is unit tested.

import type { Connection, Edge, Node } from '@xyflow/react';
import { outputHandles, targetRefs } from './catalog';
import { nodeBox } from './nodeBox';
import type { IrDoc, IrEdge, IrFlow, IrNode, NodeType } from './model';

export interface LiverGuardNodeData extends Record<string, unknown> {
  kind: NodeType;
  params: Record<string, unknown>;
  label?: string;
  maxVisits?: number;
}

export type LiverGuardNode = Node<LiverGuardNodeData, 'liverguard'>;
export type LiverGuardEdge = Edge;

export interface FlowState {
  nodes: LiverGuardNode[];
  edges: LiverGuardEdge[];
}

export function toFlowNode(node: IrNode): LiverGuardNode {
  return {
    id: node.id,
    type: 'liverguard',
    position: { x: node.pos.x, y: node.pos.y },
    data: { kind: node.type, params: { ...node.data }, label: node.label, maxVisits: node.maxVisits },
    // The start node is the flow's anchor: it can move but never be deleted.
    deletable: node.type !== 'start',
  };
}

export function toFlowEdge(edge: IrEdge): LiverGuardEdge {
  return {
    id: edge.id,
    source: edge.source,
    sourceHandle: edge.sourceHandle,
    target: edge.target,
    targetHandle: edge.targetHandle,
  };
}

export function toFlowState(flow: IrFlow): FlowState {
  return { nodes: flow.nodes.map(toFlowNode), edges: flow.edges.map(toFlowEdge) };
}

export function fromFlowNode(node: LiverGuardNode): IrNode {
  const out: IrNode = {
    id: node.id,
    type: node.data.kind,
    pos: { x: Math.round(node.position.x), y: Math.round(node.position.y) },
    data: node.data.params,
  };
  if (node.data.label) out.label = node.data.label;
  if (node.data.maxVisits !== undefined) out.maxVisits = node.data.maxVisits;
  return out;
}

export function fromFlowEdge(edge: LiverGuardEdge): IrEdge {
  const out: IrEdge = { id: edge.id, source: edge.source, sourceHandle: edge.sourceHandle ?? 'next', target: edge.target };
  if (edge.targetHandle) out.targetHandle = edge.targetHandle;
  return out;
}

export function fromFlowState(state: FlowState): IrFlow {
  return { nodes: state.nodes.map(fromFlowNode), edges: state.edges.map(fromFlowEdge) };
}

/** Smallest unused `<prefix><n>`. */
export function nextId(taken: Iterable<string>, prefix: string): string {
  const used = new Set(taken);
  let n = 1;
  while (used.has(`${prefix}${n}`)) n += 1;
  return `${prefix}${n}`;
}

/** `base`, or `base_2`, `base_3`… when taken. */
export function uniqueName(taken: Iterable<string>, base: string): string {
  const used = new Set(taken);
  if (!used.has(base)) return base;
  let n = 2;
  while (used.has(`${base}_${n}`)) n += 1;
  return `${base}_${n}`;
}

export function createNode(flow: FlowState, kind: NodeType, params: Record<string, unknown>, position: { x: number; y: number }): LiverGuardNode {
  return {
    id: nextId(
      flow.nodes.map((n) => n.id),
      'n',
    ),
    type: 'liverguard',
    position,
    data: { kind, params },
    deletable: kind !== 'start',
  };
}

/**
 * Wire `connection`. Each output handle carries at most one edge, so wiring a
 * used handle replaces its old edge. Returns the same array when the
 * connection is not allowed.
 */
export function connect(flow: FlowState, connection: Connection): LiverGuardEdge[] {
  const { source, target } = connection;
  const sourceHandle = connection.sourceHandle ?? 'next';
  if (!source || !target) return flow.edges;
  const from = flow.nodes.find((n) => n.id === source);
  const to = flow.nodes.find((n) => n.id === target);
  if (!from || !to || to.data.kind === 'start') return flow.edges;
  if (!outputHandles(from.data.kind, from.data.params).some((h) => h.id === sourceHandle)) return flow.edges;

  const kept = flow.edges.filter((e) => !(e.source === source && e.sourceHandle === sourceHandle));
  const id = nextId(
    flow.edges.map((e) => e.id),
    'e',
  );
  return [...kept, { id, source, sourceHandle, target }];
}

/** Drop edges whose source handle no longer exists (e.g. a removed switch case). */
export function pruneEdges(flow: FlowState): LiverGuardEdge[] {
  const ids = new Set(flow.nodes.map((n) => n.id));
  const handles = new Map(flow.nodes.map((n) => [n.id, new Set(outputHandles(n.data.kind, n.data.params).map((h) => h.id))]));
  return flow.edges.filter((e) => ids.has(e.source) && ids.has(e.target) && handles.get(e.source)?.has(e.sourceHandle ?? 'next'));
}

export function removeNodes(flow: FlowState, ids: Set<string>): FlowState {
  const doomed = new Set([...ids].filter((id) => flow.nodes.find((n) => n.id === id)?.data.kind !== 'start'));
  const nodes = flow.nodes.filter((n) => !doomed.has(n.id));
  return { nodes, edges: flow.edges.filter((e) => !doomed.has(e.source) && !doomed.has(e.target)) };
}

/**
 * Add `node` after `after`, on its first free output handle (preferring the
 * success path). Returns the new edge list.
 */
export function appendAfter(flow: FlowState, after: string | undefined, node: LiverGuardNode): LiverGuardEdge[] {
  if (!after) return flow.edges;
  const from = flow.nodes.find((n) => n.id === after);
  if (!from) return flow.edges;
  const handles = outputHandles(from.data.kind, from.data.params);
  const taken = new Set(flow.edges.filter((e) => e.source === after).map((e) => e.sourceHandle));
  const free = handles.find((h) => h.tone === 'ok' && !taken.has(h.id));
  if (!free) return flow.edges;
  return connect({ nodes: [...flow.nodes, node], edges: flow.edges }, { source: after, sourceHandle: free.id, target: node.id, targetHandle: null });
}

/** Where a new node goes: to the right of `after`, otherwise below everything. */
export function placeNext(flow: FlowState, after?: string): { x: number; y: number } {
  const anchor = flow.nodes.find((n) => n.id === after);
  if (anchor) {
    const taken = flow.edges.filter((e) => e.source === anchor.id).length;
    return { x: anchor.position.x + 260, y: anchor.position.y + taken * 110 };
  }
  const bottom = flow.nodes.reduce((max, n) => Math.max(max, n.position.y), -120);
  return { x: 0, y: bottom + 140 };
}

export function renameTargetInParams(value: unknown, from: string, to: string): unknown {
  if (value === from) return to;
  if (Array.isArray(value)) return value.map((v) => renameTargetInParams(v, from, to));
  if (value && typeof value === 'object') {
    return Object.fromEntries(Object.entries(value as Record<string, unknown>).map(([k, v]) => [k, renameTargetInParams(v, from, to)]));
  }
  return value;
}

/** Targets no node references. */
export function unusedTargets(doc: Pick<IrDoc, 'targets' | 'flow'>): string[] {
  const used = new Set<string>();
  for (const node of doc.flow.nodes) targetRefs(node.type, node.data).forEach((t) => used.add(t));
  return Object.keys(doc.targets).filter((name) => !used.has(name));
}

/**
 * Left-to-right layered layout, for the "整理" button only.
 *
 * ELK's layered algorithm with fixed ports: every output handle is a port at its
 * real height on the card, so branches leave in handle order and a node with one
 * successor is placed so the edge is a straight line. Cycles (loops, retries) are
 * broken depth-first from the start node. Returns the nodes with new positions.
 */
export async function autoLayout(flow: FlowState): Promise<LiverGuardNode[]> {
  const { default: ELK } = await import('elkjs/lib/elk.bundled.js');
  const boxes = new Map(flow.nodes.map((n) => [n.id, nodeBox(n.data.kind, n.data.params)]));
  const inPort = (id: string) => `${id}#in`;
  const outPort = (id: string, handle: string) => `${id}#out#${handle}`;

  const graph = {
    id: 'root',
    layoutOptions: {
      'elk.algorithm': 'layered',
      'elk.direction': 'RIGHT',
      'elk.edgeRouting': 'ORTHOGONAL',
      'elk.spacing.nodeNode': '36',
      'elk.layered.spacing.nodeNodeBetweenLayers': '90',
      'elk.layered.spacing.edgeNodeBetweenLayers': '30',
      'elk.layered.cycleBreaking.strategy': 'DEPTH_FIRST',
      'elk.layered.nodePlacement.strategy': 'BRANDES_KOEPF',
      'elk.layered.nodePlacement.favorStraightEdges': 'true',
      'elk.layered.considerModelOrder.strategy': 'NODES_AND_EDGES',
      'elk.separateConnectedComponents': 'true',
    },
    children: flow.nodes.map((node) => {
      const box = boxes.get(node.id)!;
      return {
        id: node.id,
        width: box.width,
        height: box.height,
        layoutOptions: { 'elk.portConstraints': 'FIXED_POS' },
        ports: [
          ...(node.data.kind === 'start' ? [] : [{ id: inPort(node.id), x: 0, y: box.inY, width: 0, height: 0, layoutOptions: { 'elk.port.side': 'WEST' } }]),
          ...box.outs.map((out) => ({ id: outPort(node.id, out.id), x: box.width, y: out.y, width: 0, height: 0, layoutOptions: { 'elk.port.side': 'EAST' } })),
        ],
      };
    }),
    // Edges into a start node, or from a handle the node no longer has, cannot be drawn: leave them out.
    edges: flow.edges
      .filter((e) => {
        const to = flow.nodes.find((n) => n.id === e.target);
        const from = boxes.get(e.source);
        return to && to.data.kind !== 'start' && from?.outs.some((o) => o.id === (e.sourceHandle ?? 'next'));
      })
      .map((e) => ({ id: e.id, sources: [outPort(e.source, e.sourceHandle ?? 'next')], targets: [inPort(e.target)] })),
  };

  const laid = await new ELK().layout(graph);
  const at = new Map((laid.children ?? []).map((c) => [c.id, c]));
  return flow.nodes.map((node) => {
    const placed = at.get(node.id);
    return placed?.x === undefined || placed.y === undefined ? node : { ...node, position: { x: Math.round(placed.x), y: Math.round(placed.y) } };
  });
}
