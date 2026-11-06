import { describe, expect, it } from 'vitest';
import { NODE_SPECS, outputHandles, targetRefs } from './catalog';
import {
  appendAfter,
  autoLayout,
  connect,
  createNode,
  fromFlowState,
  nextId,
  placeNext,
  pruneEdges,
  removeNodes,
  renameTargetInParams,
  toFlowState,
  uniqueName,
  unusedTargets,
  type FlowState,
} from './graph';
import { emptyDoc, type IrDoc, type NodeType } from './model';

function flowWith(...kinds: NodeType[]): FlowState {
  const flow = toFlowState(emptyDoc('t').flows.main);
  for (const kind of kinds) {
    flow.nodes.push(createNode(flow, kind, NODE_SPECS[kind].defaults(), { x: 0, y: 0 }));
  }
  return flow;
}

describe('catalog handles mirror the Rust engine', () => {
  const cases: Array<[NodeType, string[]]> = [
    ['start', ['next']],
    ['end', []],
    ['tap', ['next', 'fail']],
    ['longPress', ['next', 'fail']],
    ['swipe', ['next', 'fail']],
    ['key', ['next']],
    ['waitFor', ['found', 'timeout']],
    ['waitGone', ['next', 'timeout']],
    ['waitStable', ['next', 'timeout']],
    ['if', ['true', 'false']],
    ['loop', ['body', 'done']],
    ['retry', ['body', 'done', 'failed']],
    ['call', ['next']],
  ];
  it.each(cases)('%s', (type, expected) => {
    expect(outputHandles(type).map((h) => h.id)).toEqual(expected);
  });

  it('switch has one handle per case plus default', () => {
    expect(outputHandles('switch', { cases: ['a', 'b'] }).map((h) => h.id)).toEqual(['case:a', 'case:b', 'default']);
  });

  it('every node type has a spec whose defaults are plain JSON', () => {
    for (const spec of Object.values(NODE_SPECS)) {
      expect(JSON.parse(JSON.stringify(spec.defaults()))).toEqual(spec.defaults());
    }
  });
});

describe('ids and names', () => {
  it('picks the smallest unused id', () => {
    expect(nextId(['n1', 'n3'], 'n')).toBe('n2');
    expect(nextId([], 'e')).toBe('e1');
  });

  it('suffixes duplicate names', () => {
    expect(uniqueName(['btn'], 'btn')).toBe('btn_2');
    expect(uniqueName(['btn', 'btn_2'], 'btn')).toBe('btn_3');
    expect(uniqueName([], 'btn')).toBe('btn');
  });
});

describe('connect', () => {
  it('wires a handle and replaces the old edge of the same handle', () => {
    let flow = flowWith('log', 'log');
    flow = { ...flow, edges: connect(flow, { source: 'start', sourceHandle: 'next', target: 'n1', targetHandle: null }) };
    expect(flow.edges).toHaveLength(1);
    flow = { ...flow, edges: connect(flow, { source: 'start', sourceHandle: 'next', target: 'n2', targetHandle: null }) };
    expect(flow.edges).toHaveLength(1);
    expect(flow.edges[0].target).toBe('n2');
  });

  it('keeps edges of different handles', () => {
    let flow = flowWith('tap', 'log', 'log');
    flow = { ...flow, edges: connect(flow, { source: 'n1', sourceHandle: 'next', target: 'n2', targetHandle: null }) };
    flow = { ...flow, edges: connect(flow, { source: 'n1', sourceHandle: 'fail', target: 'n3', targetHandle: null }) };
    expect(flow.edges.map((e) => e.sourceHandle).sort()).toEqual(['fail', 'next']);
  });

  it('refuses start as a target, unknown handles and unknown nodes', () => {
    const flow = flowWith('log');
    expect(connect(flow, { source: 'n1', sourceHandle: 'next', target: 'start', targetHandle: null })).toBe(flow.edges);
    expect(connect(flow, { source: 'n1', sourceHandle: 'nope', target: 'n1', targetHandle: null })).toBe(flow.edges);
    expect(connect(flow, { source: 'ghost', sourceHandle: 'next', target: 'n1', targetHandle: null })).toBe(flow.edges);
  });

  it('allows cycles', () => {
    let flow = flowWith('log');
    flow = { ...flow, edges: connect(flow, { source: 'n1', sourceHandle: 'next', target: 'n1', targetHandle: null }) };
    expect(flow.edges).toHaveLength(1);
  });
});

describe('removeNodes and pruneEdges', () => {
  it('removes a node with its edges but never start', () => {
    let flow = flowWith('log', 'log');
    flow = { ...flow, edges: connect(flow, { source: 'start', sourceHandle: 'next', target: 'n1', targetHandle: null }) };
    flow = { ...flow, edges: connect(flow, { source: 'n1', sourceHandle: 'next', target: 'n2', targetHandle: null }) };
    const after = removeNodes(flow, new Set(['n1', 'start']));
    expect(after.nodes.map((n) => n.id)).toEqual(['start', 'n2']);
    expect(after.edges).toHaveLength(0);
  });

  it('prunes edges of a switch case that was removed', () => {
    let flow = flowWith('switch', 'log');
    flow.nodes[1].data.params = { cases: ['a'] };
    flow = { ...flow, edges: connect(flow, { source: 'n1', sourceHandle: 'case:a', target: 'n2', targetHandle: null }) };
    expect(flow.edges).toHaveLength(1);
    flow.nodes[1].data.params = { cases: [] };
    expect(pruneEdges(flow)).toHaveLength(0);
  });
});

describe('appendAfter', () => {
  it('uses the first free success handle', () => {
    let flow = flowWith('tap');
    const node = createNode(flow, 'log', {}, { x: 0, y: 0 });
    const edges = appendAfter(flow, 'n1', node);
    expect(edges).toHaveLength(1);
    expect(edges[0]).toMatchObject({ source: 'n1', sourceHandle: 'next', target: node.id });
    flow = { nodes: [...flow.nodes, node], edges };
    const other = createNode(flow, 'log', {}, { x: 0, y: 0 });
    // `next` is taken and `fail` is not a success path, so nothing is wired.
    expect(appendAfter(flow, 'n1', other)).toBe(flow.edges);
  });

  it('does nothing without an anchor', () => {
    const flow = flowWith();
    expect(appendAfter(flow, undefined, createNode(flow, 'log', {}, { x: 0, y: 0 }))).toBe(flow.edges);
  });
});

describe('placeNext', () => {
  it('goes right of the anchor and below existing branches', () => {
    let flow = flowWith('log', 'log');
    flow = { ...flow, edges: connect(flow, { source: 'start', sourceHandle: 'next', target: 'n1', targetHandle: null }) };
    expect(placeNext(flow, 'start')).toEqual({ x: 260, y: 110 });
    expect(placeNext(flow, 'n2')).toEqual({ x: 260, y: 0 });
  });

  it('without an anchor goes below everything', () => {
    const flow = flowWith('log');
    flow.nodes[1].position = { x: 0, y: 300 };
    expect(placeNext(flow)).toEqual({ x: 0, y: 440 });
  });
});

describe('targets', () => {
  it('renames a target everywhere in params, including nested conditions', () => {
    const params = { when: { kind: 'all', conds: [{ kind: 'visible', targets: ['a', 'b'] }, { kind: 'missing', target: 'a' }] } };
    expect(renameTargetInParams(params, 'a', 'z')).toEqual({
      when: { kind: 'all', conds: [{ kind: 'visible', targets: ['z', 'b'] }, { kind: 'missing', target: 'z' }] },
    });
  });

  it('collects references through conditions', () => {
    expect(targetRefs('if', { when: { kind: 'not', cond: { kind: 'visible', targets: ['a'] } } })).toEqual(['a']);
    expect(targetRefs('swipe', { from: 'a', to: 'b' })).toEqual(['a', 'b']);
    expect(targetRefs('log', {})).toEqual([]);
  });

  it('finds unused targets', () => {
    const doc: IrDoc = emptyDoc('t');
    doc.targets = { used: { type: 'point', norm: [0, 0], zone: [0, 0, 1, 1] }, idle: { type: 'point', norm: [0, 0], zone: [0, 0, 1, 1] } };
    doc.flows.main.nodes.push({ id: 'n1', type: 'tap', pos: { x: 0, y: 0 }, data: { target: 'used' } });
    expect(unusedTargets(doc)).toEqual(['idle']);
  });
});

describe('wire format', () => {
  it('round-trips through React Flow state unchanged', () => {
    const doc = emptyDoc('t');
    doc.flows.main.nodes.push({ id: 'n1', type: 'tap', pos: { x: 260, y: 0 }, label: 'buy', maxVisits: 5, data: { target: 'a', durationMs: 60 } });
    doc.flows.main.edges.push({ id: 'e1', source: 'start', sourceHandle: 'next', target: 'n1' });
    expect(fromFlowState(toFlowState(doc.flows.main))).toEqual(doc.flows.main);
  });

  it('omits optional editor fields when unset', () => {
    const flow = fromFlowState(toFlowState(emptyDoc('t').flows.main));
    expect(Object.keys(flow.nodes[0]).sort()).toEqual(['data', 'id', 'pos', 'type']);
  });
});

describe('autoLayout', () => {
  it('lays a chain out left to right', () => {
    let flow = flowWith('log', 'log');
    flow = { ...flow, edges: connect(flow, { source: 'start', sourceHandle: 'next', target: 'n1', targetHandle: null }) };
    flow = { ...flow, edges: connect(flow, { source: 'n1', sourceHandle: 'next', target: 'n2', targetHandle: null }) };
    const xs = autoLayout(flow).map((n) => n.position.x);
    expect(xs[0]).toBeLessThan(xs[1]);
    expect(xs[1]).toBeLessThan(xs[2]);
  });
});
