import { beforeEach, describe, expect, it } from 'vitest';
import type { Target } from '@/ir/model';
import { emptyDoc } from '@/ir/model';
import { useEditor } from './editor';

const template = (asset: string): Target =>
  ({ type: 'template', asset, assetBox: { w: 10, h: 10 }, roi: [0, 0, 1, 1] }) as Target;

const state = () => useEditor.getState();
const nodes = () => state().flow.nodes;
const edges = () => state().flow.edges;

beforeEach(() => {
  state().loadScript('s1', 'demo', emptyDoc('demo'));
});

describe('adding nodes', () => {
  it('wires a new node after the start node on a fresh script, then after the previous one', () => {
    const a = state().addNode('tap');
    const b = state().addNode('sleep');
    expect(edges().map((e) => [e.source, e.sourceHandle, e.target])).toEqual([
      ['start', 'next', a],
      [a, 'next', b],
    ]);
    expect(state().dirty).toBe(true);
  });

  it('places a dropped node without wiring it', () => {
    state().addNode('tap', { position: { x: 400, y: 200 }, after: null });
    expect(edges()).toHaveLength(0);
    expect(nodes().find((n) => n.data.kind === 'tap')?.position).toEqual({ x: 400, y: 200 });
  });
});

describe('history', () => {
  it('undoes and redoes structural changes', () => {
    state().addNode('tap');
    expect(nodes()).toHaveLength(2);
    state().undo();
    expect(nodes()).toHaveLength(1);
    expect(state().future).toHaveLength(1);
    state().redo();
    expect(nodes()).toHaveLength(2);
  });

  it('collapses typing in one field into one undo step', () => {
    const id = state().addNode('log');
    const before = state().past.length;
    for (const msg of ['h', 'he', 'hel', 'hell', 'hello']) state().updateNode(id, { params: { level: 'info', msg } });
    expect(state().past.length).toBe(before + 1);
    state().undo();
    expect(nodes().find((n) => n.id === id)?.data.params.msg).toBe('');
  });

  it('clears the redo stack on a new edit', () => {
    state().addNode('tap');
    state().undo();
    state().addNode('sleep');
    expect(state().future).toHaveLength(0);
  });

  it('starts clean after loading a script', () => {
    state().addNode('tap');
    state().loadScript('s2', 'other', emptyDoc('other'));
    expect(state().dirty).toBe(false);
    expect(state().past).toHaveLength(0);
  });
});

describe('targets', () => {
  it('names new targets uniquely', () => {
    expect(state().addTarget(template('a'), 'btn')).toBe('btn');
    expect(state().addTarget(template('b'), 'btn')).toBe('btn_2');
  });

  it('renaming a target updates nodes and switch handles that use it', () => {
    state().addTarget(template('a'), 'old');
    const tap = state().addNode('tap');
    state().updateNode(tap, { params: { target: 'old' } });
    const sw = state().addNode('switch');
    state().updateNode(sw, { params: { cases: ['old'] } });
    state().onConnect({ source: sw, sourceHandle: 'case:old', target: tap, targetHandle: null });

    expect(state().renameTarget('old', 'fresh')).toBe('fresh');
    expect(nodes().find((n) => n.id === tap)?.data.params.target).toBe('fresh');
    expect(nodes().find((n) => n.id === sw)?.data.params.cases).toEqual(['fresh']);
    expect(edges().some((e) => e.sourceHandle === 'case:fresh')).toBe(true);
    expect(edges().some((e) => e.sourceHandle === 'case:old')).toBe(false);
  });

  it('refuses a rename onto an existing name', () => {
    state().addTarget(template('a'), 'one');
    state().addTarget(template('b'), 'two');
    expect(state().renameTarget('one', 'two')).toBeNull();
  });
});

describe('serialisation', () => {
  it('round-trips through the wire format', () => {
    const tap = state().addNode('tap');
    state().updateNode(tap, { params: { target: 'x' }, label: 'go' });
    const ir = state().toIr();
    state().loadScript('s1', 'demo', ir);
    expect(state().toIr()).toEqual(ir);
  });
});

describe('run overlay', () => {
  it('records node states and the final result', () => {
    state().beginRun();
    state().applyRunMessage({ kind: 'node', nodeId: 'n1', state: 'running' });
    state().applyRunMessage({ kind: 'node', nodeId: 'n1', state: 'ok', score: 0.9 });
    expect(state().runtime['n1']).toEqual({ state: 'ok', score: 0.9 });
    state().applyRunMessage({ kind: 'done', runId: 'r', result: { state: 'done', stepCount: 1, errorCode: null, errorMsg: null, errorNode: null, startedAt: 0, endedAt: 1 } });
    expect(state().runStatus).toBe('idle');
    expect(state().lastResult?.state).toBe('done');
  });
});
