// The fixed geometry of a node card, shared by `LiverGuardNode.tsx` (which sets
// these sizes) and the auto layout (which needs to know where each handle sits so
// it can line edges up). Change one side and the other must follow; `graph.test.ts`
// pins the arithmetic and the card's classes use the same numbers.

import { outputHandles, summarize, targetRefs } from './catalog';

export const NODE_WIDTH = 220;
const BORDER = 1;
export const HEADER_HEIGHT = 32;
export const BODY_HEIGHT = 44;
export const ROW_HEIGHT = 24;
/** Input handle's offset inside the header, for nodes with no output rows. The extra half pixel keeps it on the same .5 grid as the rows' handles. */
export const HEADER_IN_TOP = HEADER_HEIGHT / 2 + 0.5;

/** Where a handle's centre is, measured from the node's top edge (outer border). */
export interface NodeBox {
  width: number;
  height: number;
  /** The input handle. It sits on the first output row so that a chain of nodes lines up. */
  inY: number;
  /** One entry per output handle, top to bottom. */
  outs: Array<{ id: string; y: number }>;
}

export function hasBody(kind: Parameters<typeof outputHandles>[0], params: Record<string, unknown>): boolean {
  return targetRefs(kind, params).length > 0 || summarize(kind, params) !== '';
}

export function nodeBox(kind: Parameters<typeof outputHandles>[0], params: Record<string, unknown>): NodeBox {
  const handles = outputHandles(kind, params);
  const rowsTop = BORDER + HEADER_HEIGHT + (hasBody(kind, params) ? BODY_HEIGHT : 0);
  // A row is 24px with a 1px top border inside it, and a handle is centred in the padding box.
  const outs = handles.map((h, i) => ({ id: h.id, y: rowsTop + i * ROW_HEIGHT + 1 + (ROW_HEIGHT - 1) / 2 }));
  return {
    width: NODE_WIDTH,
    height: rowsTop + handles.length * ROW_HEIGHT + BORDER,
    // Without rows (the end node) the input sits in the header.
    inY: outs.length > 0 ? outs[0].y : BORDER + HEADER_IN_TOP,
    outs,
  };
}
