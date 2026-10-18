import { invoke } from '@tauri-apps/api/core';
import type { EditOutcome } from './draft';

export interface CurrentBaseline { baseline: string; main_revision: string }
export interface PreparedCreation {
  request_id: string; draft_id: string; baseline: string; main_revision: string; human_actor: string;
}
export interface CurrentDraftClient {
  load(token: string): Promise<CurrentBaseline | null>;
  pending(token: string): Promise<PreparedCreation[]>;
  prepare(token: string, baseline: string, mainRevision: string): Promise<PreparedCreation>;
  submit(token: string, requestId: string): Promise<EditOutcome>;
}
export const currentDraftClient: CurrentDraftClient = {
  load: token => invoke('load_current_baseline', { token }),
  pending: token => invoke('pending_draft_creations', { token }),
  prepare: (token, baseline, mainRevision) => invoke('prepare_create_baseline_draft', { token, baseline, mainRevision }),
  submit: (token, requestId) => invoke('submit_create_baseline_draft', { token, requestId }),
};
