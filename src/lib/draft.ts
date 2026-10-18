import { invoke } from '@tauri-apps/api/core';
export interface DraftObject { id: string; path: string; text: string }
export interface VerifyCommand { cwd: string; argv: string[] }
export interface SplitPart { id: string; title: string; paths: string[]; verify: VerifyCommand[]; acceptance_ids: string[]; x: number; y: number }
export interface AnnotationNode { id: string; x: number; y: number; width: number; height: number; content: { type: 'text'; text: string } | { type: 'link'; url: string } | { type: 'group'; label: string } }
export type DraftEdit =
  | { action: 'put_annotation'; node: AnnotationNode } | { action: 'remove_annotation'; node: string }
  | { action: 'put_relation'; relation: { id: string; from: string; to: string; label: string } } | { action: 'remove_relation'; edge: string }
  | { action: 'move'; node: string; x: number; y: number } | { action: 'rename'; object: string; title: string }
  | { action: 'merge'; modules: string[]; keep: string; title: string }
  | { action: 'split'; module: string; parts: SplitPart[]; interface_endpoints: Record<string, [string, string]> }
  | { action: 'put_interface'; object: DraftObject; from: string; to: string } | { action: 'remove_interface'; interface: string }
  | { action: 'confirm_verify'; module: string; commands: VerifyCommand[] }
  | { action: 'add_docref'; node: string; path: string; x: number; y: number };
export interface CanvasEntry { index: number; opaque: boolean; raw: unknown }
export interface ObjectView extends DraftObject { type: string | null; title: string | null; fields?: Record<string,unknown> }
export interface SavedDraft {
  state: 'saved'; current_validity: { state: 'current' | 'historical_only'; reason?: string }; project_id: string; id: string; revision: string; request_id: string; human_actor: string;
  draft: { bundle: { id: string; project_id: string; source_revision: string; target_baseline: string; docs_dir: string; canvas_text: string; objects: Record<string, DraftObject>; questions: string[]; risks: string[]; deletions: string[] }; content_hash: string; [key: string]: unknown };
  canvas_view: { nodes: CanvasEntry[]; edges: CanvasEntry[]; diagnostics: unknown[] };
  object_views: ObjectView[];
  [key: string]: unknown;
}
export interface PreparedEdit { request_id: string; draft_id: string; expected_revision: string; human_actor: string }
export interface SaveReceipt {
  receipt: { request_id: string; body_hash: string; result_ref: { draft_id: string; revision: string; reference: string; content_hash: string; delivery: { state: 'local_saved' | 'remote_saved' | 'local_saved_remote_pending'; reason?: string } } };
  current_validity: { state: 'current' | 'historical_only'; reason?: string };
}
export interface EditOutcome { state: 'confirmed' | 'uncertain' | 'conflict' | 'rejected'; request_id: string; response: SaveReceipt | null; error: { code: string; message: string; detail: string | null } | null }
export interface DraftClient {
  load(token: string, id: string): Promise<SavedDraft>;
  prepare(token: string, id: string, expectedRevision: string, edit: DraftEdit): Promise<PreparedEdit>;
  submit(token: string, requestId: string): Promise<EditOutcome>;
  pending(token: string, id: string): Promise<PreparedEdit[]>;
}
const record = (value: unknown): value is Record<string, unknown> => value !== null && typeof value === 'object' && !Array.isArray(value);
export function decodeSaved(value: unknown, id: string): SavedDraft {
  if (!record(value) || value.state !== 'saved' || value.id !== id || typeof value.project_id !== 'string' || !value.project_id || typeof value.revision !== 'string' || !value.revision || typeof value.request_id !== 'string' || !value.request_id || typeof value.human_actor !== 'string' || !value.human_actor.startsWith('human:')) throw new Error('saved draft：缺少真实保存身份、revision 或本机人工身份');
  if (!record(value.current_validity) || !['current','historical_only'].includes(String(value.current_validity.state))) throw new Error('saved draft：缺少当前有效性声明');
  const frozen = value.draft; const canvas = value.canvas_view;
  if (!record(frozen) || !record(frozen.bundle) || frozen.bundle.id !== id || frozen.bundle.project_id !== value.project_id || typeof frozen.bundle.canvas_text !== 'string' || !record(frozen.bundle.objects) || typeof frozen.content_hash !== 'string' || !record(canvas) || !Array.isArray(canvas.nodes) || !Array.isArray(canvas.edges) || !Array.isArray(canvas.diagnostics) || !Array.isArray(value.object_views)) throw new Error('saved draft：缺少原文或 native core 读取投影');
  for (const key of ['source_revision', 'target_baseline', 'docs_dir']) if (typeof frozen.bundle[key] !== 'string' || !frozen.bundle[key]) throw new Error(`saved draft.bundle.${key} 缺失`);
  for (const key of ['questions', 'risks', 'deletions']) if (!Array.isArray(frozen.bundle[key])) throw new Error(`saved draft.bundle.${key} 缺失`);
  for (const entry of [...canvas.nodes, ...canvas.edges]) if (!record(entry) || !Number.isSafeInteger(entry.index) || typeof entry.opaque !== 'boolean' || !('raw' in entry)) throw new Error('saved draft.canvas_view：缺少条目身份与 opaque 声明');
  return value as unknown as SavedDraft;
}
export const draftClient: DraftClient = {
  load: async (token, id) => decodeSaved(await invoke('load_draft', { token, id }), id),
  prepare: (token, id, expectedRevision, edit) => invoke('prepare_draft_edit', { token, id, expectedRevision, edit }),
  submit: (token, requestId) => invoke('submit_draft_edit', { token, requestId }),
  pending: (token, id) => invoke('pending_draft_edits', { token, id }),
};
export function failureMessage(failure: unknown): string {
  return record(failure) && 'message' in failure ? String(failure.message) + (failure.detail ? `\n${String(failure.detail)}` : '') : String(failure);
}
