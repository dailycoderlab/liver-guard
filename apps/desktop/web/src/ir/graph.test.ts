import { describe, expect, it } from 'vitest';
import { NODE_SPECS, outputHandles, targetRefs } from './catalog';
import { nodeBox } from './nodeBox';
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
  const flow = toFlowState(emptyDoc('t').flow);
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
    doc.flow.nodes.push({ id: 'n1', type: 'tap', pos: { x: 0, y: 0 }, data: { target: 'used' } });
    expect(unusedTargets(doc)).toEqual(['idle']);
  });
});

describe('wire format', () => {
  it('round-trips through React Flow state unchanged', () => {
    const doc = emptyDoc('t');
    doc.flow.nodes.push({ id: 'n1', type: 'tap', pos: { x: 260, y: 0 }, label: 'buy', maxVisits: 5, data: { target: 'a', durationMs: 60 } });
    doc.flow.edges.push({ id: 'e1', source: 'start', sourceHandle: 'next', target: 'n1' });
    expect(fromFlowState(toFlowState(doc.flow))).toEqual(doc.flow);
  });

  it('omits optional editor fields when unset', () => {
    const flow = fromFlowState(toFlowState(emptyDoc('t').flow));
    expect(Object.keys(flow.nodes[0]).sort()).toEqual(['data', 'id', 'pos', 'type']);
  });
});

describe('nodeBox', () => {
  it('adds up the card: border, header, optional body, one row per output', () => {
    const start = nodeBox('start', {});
    expect(start.height).toBe(1 + 32 + 24 + 1);
    expect(start.outs.map((o) => [o.id, o.y])).toEqual([['next', 1 + 32 + 12.5]]);

    const tap = nodeBox('tap', { target: 't' });
    expect(tap.height).toBe(1 + 32 + 44 + 48 + 1);
    expect(tap.outs.map((o) => o.y)).toEqual([1 + 32 + 44 + 12.5, 1 + 32 + 44 + 24 + 12.5]);
    expect(tap.inY).toBe(tap.outs[0].y);

    const end = nodeBox('end', {});
    expect(end.height).toBe(1 + 32 + 1);
    expect(end.outs).toEqual([]);
    expect(end.inY).toBe(17.5);
  });
});

describe('autoLayout', () => {
  type Wire = [string, string, string]; // source id, handle, target id

  async function laidOut(kinds: Record<string, NodeType>, wires: Wire[], params: Record<string, Record<string, unknown>> = {}) {
    const flow = toFlowState(emptyDoc('t').flow);
    for (const [id, kind] of Object.entries(kinds)) {
      if (kind === 'start') continue;
      const node = createNode(flow, kind, { ...NODE_SPECS[kind].defaults(), ...params[id] }, { x: 0, y: 0 });
      flow.nodes.push({ ...node, id });
    }
    let edges = flow.edges;
    for (const [source, sourceHandle, target] of wires) {
      edges = connect({ nodes: flow.nodes, edges }, { source: source === 'start' ? 'start' : source, sourceHandle, target, targetHandle: null });
    }
    const nodes = await autoLayout({ nodes: flow.nodes, edges });
    const byId = new Map(nodes.map((n) => [n.id, n]));
    const box = (id: string) => nodeBox(byId.get(id)!.data.kind, byId.get(id)!.data.params);
    return { nodes, edges, byId, box };
  }

  function overlaps(nodes: Awaited<ReturnType<typeof laidOut>>['nodes']) {
    const found: string[] = [];
    for (let i = 0; i < nodes.length; i++) {
      for (let j = i + 1; j < nodes.length; j++) {
        const [a, b] = [nodes[i], nodes[j]];
        const ba = nodeBox(a.data.kind, a.data.params);
        const bb = nodeBox(b.data.kind, b.data.params);
        const apart =
          a.position.x + ba.width <= b.position.x || b.position.x + bb.width <= a.position.x || a.position.y + ba.height <= b.position.y || b.position.y + bb.height <= a.position.y;
        if (!apart) found.push(`${a.id}/${b.id}`);
      }
    }
    return found;
  }

  const handleY = (l: Awaited<ReturnType<typeof laidOut>>, id: string, handle: string) => l.byId.get(id)!.position.y + l.box(id).outs.find((o) => o.id === handle)!.y;
  const inY = (l: Awaited<ReturnType<typeof laidOut>>, id: string) => l.byId.get(id)!.position.y + l.box(id).inY;

  it('lays a chain out left to right with every edge a straight line', async () => {
    const l = await laidOut({ start: 'start', a: 'tap', b: 'log', c: 'tap', done: 'end' }, [
      ['start', 'next', 'a'],
      ['a', 'next', 'b'],
      ['b', 'next', 'c'],
      ['c', 'next', 'done'],
    ], { a: { target: 't' }, c: { target: 't' } });
    const xs = ['start', 'a', 'b', 'c', 'done'].map((id) => l.byId.get(id)!.position.x);
    expect([...xs].sort((p, q) => p - q)).toEqual(xs);
    expect(new Set(xs).size).toBe(5);
    for (const [source, handle, target] of [['start', 'next', 'a'], ['a', 'next', 'b'], ['b', 'next', 'c'], ['c', 'next', 'done']]) {
      expect(handleY(l, source, handle), `${source} -> ${target}`).toBe(inY(l, target));
    }
  });

  it('puts the branches of a node in handle order without overlapping', async () => {
    const l = await laidOut(
      { start: 'start', test: 'if', yes: 'tap', no: 'tap', end1: 'end', end2: 'end' },
      [
        ['start', 'next', 'test'],
        ['test', 'true', 'yes'],
        ['test', 'false', 'no'],
        ['yes', 'next', 'end1'],
        ['no', 'next', 'end2'],
      ],
      { yes: { target: 'a' }, no: { target: 'b' } },
    );
    expect(overlaps(l.nodes)).toEqual([]);
    expect(l.byId.get('yes')!.position.y).toBeLessThan(l.byId.get('no')!.position.y);
    expect(l.byId.get('yes')!.position.x).toBeGreaterThan(l.byId.get('test')!.position.x);
  });

  it('keeps a loop, a merge, a switch and a loose node apart', async () => {
    const l = await laidOut(
      { start: 'start', find: 'waitFor', tap1: 'tap', tap2: 'tap', pick: 'switch', retry: 'retry', work: 'log', done: 'end', loose: 'log', alt: 'tap', fail: 'end' },
      [
        ['start', 'next', 'find'],
        ['find', 'found', 'tap1'],
        ['find', 'timeout', 'fail'],
        ['tap1', 'next', 'pick'],
        ['tap1', 'fail', 'retry'],
        ['pick', 'case:x', 'tap2'],
        ['pick', 'case:y', 'alt'],
        ['pick', 'default', 'work'],
        ['tap2', 'next', 'work'],
        ['alt', 'next', 'work'],
        ['alt', 'fail', 'find'], // back edge: a loop
        ['work', 'next', 'done'],
        ['retry', 'body', 'tap1'], // another cycle
        ['retry', 'done', 'work'],
        ['retry', 'failed', 'fail'],
      ],
      { find: { any: ['x'] }, tap1: { target: 'x' }, tap2: { target: 'y' }, alt: { target: 'z' }, pick: { cases: ['x', 'y'] } },
    );
    expect(overlaps(l.nodes)).toEqual([]);
    const start = l.byId.get('start')!;
    for (const node of l.nodes) if (node.id !== 'loose') expect(node.position.x).toBeGreaterThanOrEqual(start.position.x);
  });

  it('leaves edges it cannot draw alone and does not choke on an empty graph', async () => {
    const empty = toFlowState(emptyDoc('t').flow);
    expect(await autoLayout({ nodes: [], edges: [] })).toEqual([]);
    const lone = await autoLayout({ nodes: empty.nodes, edges: [{ id: 'e1', source: 'start', sourceHandle: 'gone', target: 'start' }] });
    expect(lone).toHaveLength(1);
  });
});
