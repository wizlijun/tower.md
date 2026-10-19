import type { InterventionView, PreparedIntervention, InterventionAction, InterventionClient } from './interventions';
export const requestId = '01900000-0000-7000-8000-000000000001';
export const viewFixture = (): InterventionView => ({ project_id: 'P', paused: false, pause_revision: null, active_runs: [{ run: 'R-1', task: 'T-1', module: 'M-1' }], resume_request: null, messages: [], action_ticket: '01900000-0000-7000-8000-000000000002', human_actor: 'human:owner' });
export const originalFixture = (action: InterventionAction): PreparedIntervention => ({ request_id: requestId, project_id: 'P', human_actor: 'human:owner', body_hash: 'a'.repeat(64), action });
export const confirmedFixture = (action?:InterventionAction) => ({ request_id: requestId, state: 'confirmed' as const, response: { receipt: { request_id: requestId, body_hash: 'a'.repeat(64), result_ref: { hold: `H-${requestId}`, opening_eid:'actual-opening-eid',pause_revision:action?.action==='pause_all'?'c'.repeat(64):action?.action==='confirm_resume'?action.pause_revision:null,runs:action?.action==='confirm_resume'?action.runs:[] } },historical_only:false }, error: null });

/** Read-only test adapter: an accidental human action fails the test path. */
export function readonlyControlsFixture(): InterventionClient {
 const unavailable = async (): Promise<never> => { throw new Error('Unexpected control mutation in review-only test'); };
 return { load: async () => viewFixture(), pending: async () => [], prepare: unavailable, submit: unavailable, requestResume: unavailable };
}
