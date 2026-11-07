import { describe, expect, it } from 'vitest';
import { NODE_SPECS } from './catalog';
import { createNode, toFlowState } from './graph';
import { emptyDoc, type NodeType } from './model';
import { setVarNames } from './vars';

function nodes(...specs: Array<[NodeType, Record<string, unknown>?]>) {
  const flow = toFlowState(emptyDoc('t').flow);
  for (const [kind, params] of specs) {
    flow.nodes.push(createNode(flow, kind, { ...NODE_SPECS[kind].defaults(), ...params }, { x: 0, y: 0 }));
  }
  return flow.nodes;
}

describe('setVarNames', () => {
  it('lists the distinct, trimmed names from setVar nodes, sorted', () => {
    const list = nodes(['setVar', { name: 'level' }], ['setVar', { name: ' coins ' }], ['setVar', { name: 'level' }], ['tap'], ['setVar', { name: '' }]);
    expect(setVarNames(list)).toEqual(['coins', 'level']);
  });

  it('is empty without setVar nodes', () => {
    expect(setVarNames(nodes(['tap']))).toEqual([]);
  });
});
