import { invoke } from '@tauri-apps/api/core';
/** Fixed native bridge; this private integration is not yet applied to the shipped App. */
export type InterventionScope = { kind: 'project' } | { kind: 'module'; module: string } | { kind: 'task'; task: string };
export type InterventionAction = { action: 'interject'; scope: InterventionScope; text: string } | { action: 'pause_all' } | { action: 'confirm_resume'; request: string; pause_revision: string; runs: string[] };
export interface ResumeRequest { request: string; pause_revision: string; runs: string[] }
export interface InterventionMessage { id: string; kind: string; text: string; scope: InterventionScope | null; recipients: { run: string; task: string; delivered: boolean; actionable: boolean }[] }
export interface InterventionView {
  project_id: string; paused: boolean; pause_revision: string | null;
  active_runs: { run: string; task: string; module: string }[];
  resume_request: ResumeRequest | null; messages: InterventionMessage[];
  /** App-native wrappers only; daemon cannot accept a human actor supplied by the page. */
  action_ticket: string; human_actor: string;
}
export interface PreparedIntervention { request_id: string; project_id: string; human_actor: string; body_hash: string; action: InterventionAction }
export interface InterventionOutcome {
  request_id: string; state: 'confirmed' | 'uncertain' | 'conflict' | 'rejected';
  response: { receipt: { request_id: string; body_hash: string; result_ref: { hold:string; opening_eid:string; pause_revision:string|null; runs:string[] } }; historical_only:boolean } | null;
  error: { code: string; message: string; detail?: string | null } | null;
}
export interface InterventionClient {
  load(token: string): Promise<InterventionView>;
  prepare(token: string, actionTicket: string, action: InterventionAction): Promise<PreparedIntervention>;
  pending(token: string): Promise<PreparedIntervention[]>;
  submit(token: string, requestId: string): Promise<InterventionOutcome>;
  /** Fixed bundled-CLI `resume --all` request creation. It never confirms a resume. */
  requestResume(token: string): Promise<void>;
}
const object = (value: unknown): value is Record<string, unknown> => value !== null && typeof value === 'object' && !Array.isArray(value);
const id = (value: unknown): value is string => typeof value === 'string' && value.length > 0 && !/[\s\x00-\x1f\x7f]/u.test(value);
const uuid = (value: unknown): value is string => typeof value === 'string' && /^[a-f0-9]{8}-[a-f0-9]{4}-7[a-f0-9]{3}-[89ab][a-f0-9]{3}-[a-f0-9]{12}$/.test(value);
const hash = (value: unknown): value is string => typeof value === 'string' && /^[a-f0-9]{64}$/.test(value);
const human = (value: unknown): value is string => typeof value === 'string' && /^human:[^:/\s\x00-\x1f\x7f]+$/u.test(value);
function scope(value: unknown): value is InterventionScope {
  return object(value) && (value.kind === 'project' || (value.kind === 'module' && id(value.module)) || (value.kind === 'task' && id(value.task)));
}
function sortedRuns(value: unknown): value is string[] {
  return Array.isArray(value) && value.every(id) && new Set(value).size === value.length && value.every((run, index) => index === 0 || value[index - 1] < run);
}
function resume(value: unknown): value is ResumeRequest { return object(value) && id(value.request) && id(value.pause_revision) && sortedRuns(value.runs); }
export function actionValid(value: unknown): value is InterventionAction {
  if (!object(value)) return false;
  return (value.action === 'interject' && scope(value.scope) && typeof value.text === 'string' && value.text.trim().length > 0)
    || value.action === 'pause_all' || (value.action === 'confirm_resume' && resume(value));
}
export function decodeInterventionView(value: unknown): InterventionView {
  if (!object(value) || !id(value.project_id) || !human(value.human_actor) || !uuid(value.action_ticket) || typeof value.paused !== 'boolean'
    || !(value.pause_revision === null || id(value.pause_revision)) || !Array.isArray(value.active_runs)
    || !value.active_runs.every(run => object(run) && id(run.run) && id(run.task) && id(run.module))
    || !(value.resume_request === null || resume(value.resume_request)) || !Array.isArray(value.messages)
    || !value.messages.every(message => object(message) && id(message.id) && id(message.kind) && typeof message.text === 'string' && (message.scope === null || scope(message.scope)) && Array.isArray(message.recipients)
      && message.recipients.every(recipient => object(recipient) && id(recipient.run) && id(recipient.task) && typeof recipient.delivered === 'boolean' && typeof recipient.actionable === 'boolean')))
    throw new Error('项目控制视图缺少实际项目、暂停范围或消息送达状态。');
  if (new Set(value.active_runs.map(run => run.run)).size !== value.active_runs.length || (value.paused && value.pause_revision === null)
    || (value.resume_request !== null && (!value.paused || value.resume_request.pause_revision !== value.pause_revision))) throw new Error('暂停与恢复原请求范围不一致；请重新读取。');
  return value as unknown as InterventionView;
}
export function decodePreparedIntervention(value: unknown): PreparedIntervention {
  if (!object(value) || !uuid(value.request_id) || !id(value.project_id) || !human(value.human_actor) || !hash(value.body_hash) || !actionValid(value.action)) throw new Error('原操作缺少 UUID、正文绑定或本机人工身份。');
  return value as unknown as PreparedIntervention;
}
export function sameAction(first: InterventionAction, second: InterventionAction): boolean {
  if (first.action !== second.action) return false;
  if (first.action === 'pause_all') return true;
  if (first.action === 'interject' && second.action === 'interject') return first.text === second.text && first.scope.kind === second.scope.kind
    && (first.scope.kind !== 'module' || (second.scope.kind === 'module' && first.scope.module === second.scope.module))
    && (first.scope.kind !== 'task' || (second.scope.kind === 'task' && first.scope.task === second.scope.task));
  return first.action === 'confirm_resume' && second.action === 'confirm_resume' && first.request === second.request && first.pause_revision === second.pause_revision && JSON.stringify(first.runs) === JSON.stringify(second.runs);
}
export function decodeInterventionOutcome(value: unknown, original: PreparedIntervention): InterventionOutcome {
  if (!object(value) || value.request_id !== original.request_id || !['confirmed', 'uncertain', 'conflict', 'rejected'].includes(String(value.state)) || !(value.error === null || (object(value.error) && typeof value.error.code === 'string' && typeof value.error.message === 'string'))) throw new Error('操作结果缺少原请求身份；请保留原 UUID。');
  if (value.state === 'confirmed') {
    if (!object(value.response) || !object(value.response.receipt) || value.response.receipt.request_id !== original.request_id || value.response.receipt.body_hash !== original.body_hash || !('result_ref' in value.response.receipt)) throw new Error('回执与原请求正文不同；不能推断操作成功。');
    const result=value.response.receipt.result_ref;
    if(typeof value.response.historical_only!=='boolean'||!object(result)||result.hold!==`H-${original.request_id}`||!id(result.opening_eid)||!(result.pause_revision===null||hash(result.pause_revision))||!sortedRuns(result.runs))throw new Error('控制回执缺少真实原件或完整排序范围。');
    const action=original.action;
    if((action.action==='interject'&&result.pause_revision!==null)||(action.action==='pause_all'&&result.pause_revision===null)||(action.action==='confirm_resume'&&(result.pause_revision!==action.pause_revision||JSON.stringify(result.runs)!==JSON.stringify(action.runs))))throw new Error('控制回执与原暂停版本或完整恢复范围不同。');
  } else if (value.response !== null) throw new Error('未确认的操作不能当成有效回执。');
  return value as unknown as InterventionOutcome;
}
export function deliveryCounts(message: InterventionMessage): { queued: number; delivered: number; inactive: number } {
  return { queued: message.recipients.filter(recipient => !recipient.delivered && recipient.actionable).length, delivered: message.recipients.filter(recipient => recipient.delivered).length, inactive: message.recipients.filter(recipient => !recipient.delivered && !recipient.actionable).length };
}
export const interventionClient: InterventionClient = {
  load: async token => decodeInterventionView(await invoke('load_interventions', { token })),
  prepare: async (token, actionTicket, action) => decodePreparedIntervention(await invoke('prepare_intervention', { token, actionTicket, action })),
  pending: async token => {
    const value: unknown = await invoke('pending_interventions', { token });
    if (!Array.isArray(value)) throw new Error('原控制操作队列不可读；不会创建新 UUID。');
    const originals = value.map(decodePreparedIntervention);
    if (new Set(originals.map(original => original.request_id)).size !== originals.length) throw new Error('原控制操作队列含重复 UUID。');
    return originals;
  },
  submit: (token, requestId) => invoke('submit_intervention', { token, requestId }),
  requestResume: token => invoke('request_resume', { token }),
};
