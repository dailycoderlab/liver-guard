import { Background, BackgroundVariant, Controls, MiniMap, ReactFlow, ReactFlowProvider, useReactFlow, type NodeTypes } from '@xyflow/react';
import '@xyflow/react/dist/style.css';
import { useCallback, useEffect, useRef } from 'react';
import { NODE_SPECS } from '@/ir/catalog';
import type { NodeType } from '@/ir/model';
import { selectFlow, useEditor } from '@/store/editor';
import { SkyNodeComponent } from './SkyNode';

const nodeTypes: NodeTypes = { sky: SkyNodeComponent };
export const DRAG_MIME = 'application/x-sky-node';

function CanvasInner() {
  const nodes = useEditor((s) => selectFlow(s).nodes);
  const edges = useEditor((s) => selectFlow(s).edges);
  const activeFlow = useEditor((s) => s.activeFlow);
  // The graph is read-only while a run is in progress: node ids in the run overlay must keep meaning.
  const locked = useEditor((s) => s.runStatus !== 'idle');
  const { onNodesChange, onEdgesChange, onConnect, beginDrag, addNode } = useEditor.getState();
  const flow = useReactFlow();
  const fitted = useRef<string | null>(null);

  // Fit the view the first time each flow of each script is shown.
  const scriptId = useEditor((s) => s.scriptId);
  useEffect(() => {
    const key = `${scriptId}/${activeFlow}`;
    if (fitted.current === key) return;
    fitted.current = key;
    requestAnimationFrame(() => flow.fitView({ padding: 0.3, maxZoom: 1, duration: 0 }));
  }, [scriptId, activeFlow, flow]);

  const onDrop = useCallback(
    (event: React.DragEvent) => {
      const kind = event.dataTransfer.getData(DRAG_MIME) as NodeType;
      if (!kind || !NODE_SPECS[kind]) return;
      event.preventDefault();
      const position = flow.screenToFlowPosition({ x: event.clientX - 110, y: event.clientY - 20 });
      addNode(kind, { position, after: null });
    },
    [addNode, flow],
  );

  return (
    <ReactFlow
      nodes={nodes}
      edges={edges}
      nodeTypes={nodeTypes}
      onNodesChange={onNodesChange}
      onEdgesChange={onEdgesChange}
      onConnect={onConnect}
      onNodeDragStart={beginDrag}
      onDrop={onDrop}
      onDragOver={(e) => {
        if (e.dataTransfer.types.includes(DRAG_MIME)) e.preventDefault();
      }}
      nodesDraggable={!locked}
      nodesConnectable={!locked}
      deleteKeyCode={locked ? null : ['Backspace', 'Delete']}
      selectionKeyCode="Shift"
      multiSelectionKeyCode={['Meta', 'Control']}
      minZoom={0.25}
      maxZoom={1.75}
      snapToGrid
      snapGrid={[10, 10]}
      defaultEdgeOptions={{ type: 'smoothstep', style: { strokeWidth: 1.75 } }}
      connectionRadius={28}
      proOptions={{ hideAttribution: true }}
    >
      <Background variant={BackgroundVariant.Dots} gap={22} size={1.2} color="#2a3342" />
      <Controls showInteractive={false} />
      <MiniMap pannable zoomable nodeColor="#2f3b52" maskColor="rgb(14 17 23 / 0.55)" className="!bg-panel !border !border-line" />
    </ReactFlow>
  );
}

export function Canvas() {
  return (
    <ReactFlowProvider>
      <CanvasInner />
    </ReactFlowProvider>
  );
}
