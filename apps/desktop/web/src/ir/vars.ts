import type { LiverGuardNode } from './graph';

/** Variable names written by the script's `setVar` nodes, sorted: what the editors suggest. */
export function setVarNames(nodes: LiverGuardNode[]): string[] {
  const names = new Set<string>();
  for (const node of nodes) {
    if (node.data.kind !== 'setVar') continue;
    const name = node.data.params.name;
    if (typeof name === 'string' && name.trim()) names.add(name.trim());
  }
  return [...names].sort();
}
