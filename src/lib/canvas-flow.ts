import { MarkerType, type Node, type Edge } from '@xyflow/svelte';
import type { SavedDraft, CanvasEntry, DraftEdit, ObjectView } from './draft';
export type CanvasNodeData = Record<string, unknown> & { canonicalId: string; kind: string; title: string; text: string; binding?: string; editable: boolean; opaque: boolean; raw: unknown; object?: ObjectView };
export type CanvasEdgeData = Record<string, unknown> & { canonicalId: string; binding?: string; raw: unknown; editable: boolean };
export type FlowNode = Node<CanvasNodeData>;
export type FlowEdge = Edge<CanvasEdgeData>;
export function rawRecord(value: unknown): Record<string, unknown> | null { return value !== null && typeof value === 'object' && !Array.isArray(value) ? value as Record<string, unknown> : null; }
export function boundObject(raw: Record<string, unknown>): string | undefined { const binding = rawRecord(raw.towermd); return typeof binding?.obj === 'string' ? binding.obj : undefined; }
export function projectFlow(saved: SavedDraft, editable: boolean): { nodes: FlowNode[]; edges: FlowEdge[]; diagnostics: unknown[] } {
  editable = editable && saved.current_validity.state === 'current';
  const nodes: FlowNode[] = []; const edges: FlowEdge[] = []; const diagnostics = [...saved.canvas_view.diagnostics];
  const unique = (entries: CanvasEntry[]) => { const counts = new Map<string, number>(); for (const entry of entries) { const raw = rawRecord(entry.raw); if (typeof raw?.id === 'string') counts.set(raw.id, (counts.get(raw.id) ?? 0) + 1); } return counts; };
  const ids = unique(saved.canvas_view.nodes); const edgeIds = unique(saved.canvas_view.edges);
  for (const entry of saved.canvas_view.nodes) {
    const raw = rawRecord(entry.raw);
    if (!raw || typeof raw.id !== 'string' || !['x','y','width','height'].every(key => typeof raw[key] === 'number' && Number.isSafeInteger(raw[key]))) { diagnostics.push({ message: `节点 ${entry.index} 无可显示几何，原文保留` }); continue; }
    const duplicate = ids.get(raw.id) !== 1; const opaque = entry.opaque || duplicate;
    const binding = boundObject(raw); const object = binding ? saved.object_views.find(object => object.id === binding) : undefined;
    const kind = typeof raw.type === 'string' ? raw.type : 'unknown';
    const text = object?.text ?? (typeof raw.text === 'string' ? raw.text : typeof raw.url === 'string' ? raw.url : typeof raw.label === 'string' ? raw.label : typeof raw.file === 'string' ? raw.file : '');
    nodes.push({ id: duplicate ? `__opaque_node_${entry.index}` : raw.id, type: 'canvas', position: { x: raw.x as number, y: raw.y as number }, width: raw.width as number, height: raw.height as number, zIndex: kind === 'group' ? -1 : entry.index, draggable: editable && !opaque, connectable: editable && !opaque, deletable: false,
      data: { canonicalId: raw.id, kind, title: opaque ? `不透明条目 ${raw.id}` : object?.title ?? (typeof raw.label === 'string' ? raw.label : raw.id), text, binding, editable: editable && !opaque, opaque, raw: entry.raw, object } });
  }
  const rendered = new Set(nodes.filter(node => !node.data.opaque).map(node => node.id));
  for (const entry of saved.canvas_view.edges) {
    const raw = rawRecord(entry.raw);
    if (!raw || entry.opaque || typeof raw.id !== 'string' || edgeIds.get(raw.id) !== 1 || typeof raw.fromNode !== 'string' || typeof raw.toNode !== 'string' || !rendered.has(raw.fromNode) || !rendered.has(raw.toNode)) { diagnostics.push({ message: `关系 ${entry.index} 不可交互，原文保留` }); continue; }
    const binding = boundObject(raw);
    edges.push({ id: raw.id, type: 'canvas', source: raw.fromNode, target: raw.toNode, label: typeof raw.label === 'string' ? raw.label : undefined, deletable: false, sourceHandle: ['top','right','bottom','left'].includes(String(raw.fromSide)) ? `side:${raw.fromSide}` : 'side:right', targetHandle: ['top','right','bottom','left'].includes(String(raw.toSide)) ? `side:${raw.toSide}` : 'side:left', markerStart: raw.fromEnd === 'arrow' ? {type:MarkerType.ArrowClosed} : undefined, markerEnd: raw.toEnd === 'none' ? undefined : {type:MarkerType.ArrowClosed}, data: {canonicalId:raw.id,binding,raw:entry.raw,editable:editable && !binding} });
  }
  return { nodes,edges,diagnostics };
}
export function moveEdit(node: FlowNode, position: { x: number; y: number }): DraftEdit | null {
  if (!node.data.editable || node.data.opaque) return null;
  const original = rawRecord(node.data.raw);
  const x = Math.round(position.x), y = Math.round(position.y);
  if (!original || !Number.isSafeInteger(original.x) || !Number.isSafeInteger(original.y) || !Number.isSafeInteger(x) || !Number.isSafeInteger(y) || (x === original.x && y === original.y)) return null;
  return {action:'move',node:node.data.canonicalId,x,y};
}
