import type { FlowNode, FlowEdge } from './canvas-flow';
import type { ProjectOverview } from './project';
import type { TaskProgressClient, TaskProgressView, ProgressTask, ProgressProposal } from './task-progress';
import { progressStateLabel } from './task-progress';

export type RawEvent = Record<string, unknown> & { seq: number; eid: string; obj: string; type: string; data: unknown; t?: string };
export type AttentionItem = { id: string; kind: string; symbol: string; label: string; title: string; tasks: string[]; opened: number; priority: number; raw: Record<string, unknown>; proposal?:ProgressProposal };
const object = (value: unknown): Record<string, unknown> => value !== null && typeof value === 'object' && !Array.isArray(value) ? value as Record<string, unknown> : {};
const string = (value: unknown): string => typeof value === 'string' ? value : '';
export const holdsOf = (view: TaskProgressView | null): Record<string, unknown>[] => view && Array.isArray(view.holds) ? view.holds : [];
export const eventsOf = (view: TaskProgressView | null): RawEvent[] => view && Array.isArray(view.events) ? view.events as RawEvent[] : [];
const holdKinds: Record<string, { priority: number; symbol: string; label: string }> = {
  forbidden: { priority: 1, symbol: '⛔', label: '禁止' },
  gatefailed: { priority: 4, symbol: '🚧', label: '门禁失败' },
  review: { priority: 5, symbol: '👁', label: '评审' },
  ask: { priority: 6, symbol: '❓', label: '提问' },
};
const taskDiagnostic = (hold: Record<string, unknown>) => hold.scope === 'TaskBranch' && ((hold.kind === 'forbidden' && hold.gate === 'G2') || (hold.kind === 'gatefailed' && hold.gate === 'G5'));
/** Keep current and historical automatic diagnostics readable in the same Z2. */
export const taskDiagnostics = (view: TaskProgressView | null, task: string): Record<string, unknown>[] => holdsOf(view).filter(hold => taskDiagnostic(hold) && hold.task === task);
export function unresolvedItems(view: TaskProgressView | null): AttentionItem[] {
  return holdsOf(view).flatMap(hold => {
    const body = object(hold.body); const kind = taskDiagnostic(hold) ? string(hold.kind) : string(body.kind) || string(hold.kind); const definition = holdKinds[kind];
    if (!definition || hold.reply != null || body.verdict != null || hold.resolved_by != null || hold.resolution != null || hold.source_state != null) return [];
    if ((kind === 'forbidden' && !taskDiagnostic(hold)) || (taskDiagnostic(hold) && view?.state !== 'recorded')) return [];
    const recipients = Array.isArray(hold.recipients) ? hold.recipients.map(object) : [];
    const binding = object(body.binding);
    const tasks = [...new Set([string(hold.task), string(binding.task), ...recipients.map(recipient => string(recipient.task))].filter(Boolean))];
    if (kind === 'gatefailed' && tasks.length && tasks.every(id => ['merged', 'ended'].includes(view?.tasks.find(task => task.id === id)?.state ?? ''))) return [];
    return [{ id: string(hold.id), kind, ...definition, title: string(body.question) || (kind === 'review' ? '未决任务评审记录' : kind === 'forbidden' ? `${string(hold.gate)} 禁止：${string(hold.path) || '查看实际越界检查原件'}` : `${string(hold.gate)} 门禁失败记录`), tasks, opened: typeof hold.opened_ms === 'number' ? hold.opened_ms : Number.MAX_SAFE_INTEGER, raw: hold }];
  }).sort((a, b) => a.priority - b.priority || a.opened - b.opened || b.tasks.length - a.tasks.length || a.id.localeCompare(b.id));
}
export function attentionItems(view: TaskProgressView | null): AttentionItem[] {
  const holds = unresolvedItems(view).filter(hold => hold.kind !== 'ask' || hold.raw.attention === true);
  const blocked = new Map<string, ProgressTask[]>();
  for (const task of view?.tasks ?? []) if (task.state === 'blocked' && typeof task.module === 'string') blocked.set(task.module, [...(blocked.get(task.module) ?? []), task]);
  const modules: AttentionItem[] = [...blocked].filter(([, tasks]) => !holds.some(hold => hold.tasks.some(id => tasks.some(task => task.id === id)))).map(([module, tasks]) => ({ id: `module:${module}`, kind: 'module', symbol: '△', label: '模块受阻', title: `模块 ${module} 有 ${tasks.length} 个受阻任务`, tasks: tasks.map(task => task.id), opened: Number.MAX_SAFE_INTEGER, priority: 100, raw: { tasks } }));
  const proposals:AttentionItem[]=(view?.state==='recorded'?view.proposals??[]:[]).filter(proposal=>proposal.state==='review_pending').map(proposal=>({id:proposal.binding.object_id,kind:'proposal',symbol:'◇',label:'待批提案',title:proposal.reason,tasks:proposal.impact.tasks,opened:proposal.created_at_ms,priority:20,raw:proposal as unknown as Record<string,unknown>,proposal}));
  // A real Task flag is a derived review prompt, not a synthetic Review Hold.
  // Preserve all formal interruption/Proposal priorities; this comes before
  // generic blocked modules. No wait timestamp is inferred from a bounded tail.
  const rechecks: AttentionItem[] = (view?.state === 'recorded' ? view.tasks : []).filter(task => task.needs_recheck === true).map(task => ({ id: `recheck:${task.id}`, kind: 'recheck', symbol: '↻', label: '需复核', title: `${task.title ?? task.id} 需复核`, tasks: [task.id], opened: Number.MAX_SAFE_INTEGER, priority: 30, raw: task }));
  return [...holds, ...proposals, ...rechecks, ...modules].sort((a, b) => a.priority - b.priority || (a.kind==='proposal'&&b.kind==='proposal' ? b.tasks.length-a.tasks.length || a.opened-b.opened : a.opened - b.opened || b.tasks.length - a.tasks.length) || a.id.localeCompare(b.id));
}
export function moduleTasks(view: TaskProgressView | null, id: string): ProgressTask[] { return view?.tasks.filter(task => task.module === id) ?? []; }
export function moduleSummary(view: TaskProgressView | null, id: string): string {
  if (!view) return '◌ 正在读取真实任务与中断';
  const tasks = moduleTasks(view, id); const holds = unresolvedItems(view).filter(hold => hold.tasks.some(task => tasks.some(item => item.id === task)));
  const running = tasks.filter(task => ['running', 'verifying'].includes(task.state)).length;
  const blocked = tasks.filter(task => ['blocked', 'held'].includes(task.state)).length;
  const rechecks = view.state === 'recorded' ? tasks.filter(task => task.needs_recheck === true).length : 0;
  return `${holds.length ? `${holds[0].symbol} ${holds[0].label} · ${holds.length} 项中断` : rechecks ? '↻ 需复核' : blocked ? '△ 受阻' : running ? '▶ 推进中' : '◇ 无当前中断'}${rechecks ? ` · ${rechecks} 个任务需复核` : ''}\n${tasks.filter(task => task.state === 'merged').length}/${tasks.length} 已合并 · ${running} 运行/验证 · ${blocked} 等待`;
}
export function overviewFlow(view: ProjectOverview, progress: TaskProgressView | null): { nodes: FlowNode[]; edges: FlowEdge[] } {
  const coherent = progress?.revision === (view.authorityRevision ?? view.revision) ? progress : null;
  const nodes: FlowNode[] = view.nodes.map(node => {
    const binding = node.obj ?? ''; const isModule = node.objectType === 'Module' || !!coherent?.tasks.some(task => task.module === binding);
    const tasks = isModule ? moduleTasks(coherent, binding) : [];
    const blocked = tasks.some(task => ['blocked', 'held'].includes(task.state)) || unresolvedItems(coherent).some(hold => ['forbidden', 'gatefailed'].includes(hold.kind) && hold.tasks.some(id => tasks.some(task => task.id === id)));
    const recheck = coherent?.state === 'recorded' && tasks.some(task => task.needs_recheck === true);
    return { id: node.id, type: 'canvas', position: { x: node.x, y: node.y }, width: node.width, height: node.height, class: blocked ? 'live-node-blocked' : recheck ? 'live-node-recheck' : undefined, draggable: false, connectable: false, deletable: false,
      data: { canonicalId: node.id, kind: node.objectType ?? node.kind, title: node.title, text: binding && isModule && (!progress || coherent) ? moduleSummary(coherent, binding) : node.text, binding: binding || undefined, editable: false, opaque: ['opaque', 'dangling', 'mismatch', 'unsafe'].includes(node.status), raw: node } };
  });
  const edges: FlowEdge[] = view.edges.map((edge, index) => ({ id: `overview-edge-${index}`, type: 'canvas', source: edge.from, target: edge.to, sourceHandle: 'side:right', targetHandle: 'side:left', deletable: false }));
  return { nodes, edges };
}
export function taskFlow(tasks: ProgressTask[], positions: Map<string, { x: number; y: number }>, progress: TaskProgressView | null = null): { nodes: FlowNode[]; edges: FlowEdge[] } {
  const ids = new Set(tasks.map(task => task.id));
  const nodes: FlowNode[] = tasks.map(task => {
    if (!positions.has(task.id)) { const index = positions.size; positions.set(task.id, { x: (index % 3) * 330, y: Math.floor(index / 3) * 180 }); }
    const holds = unresolvedItems(progress).filter(hold => hold.tasks.includes(task.id));
    return { id: task.id, type: 'canvas', position: positions.get(task.id)!, width: 290, height: 140, class: ['blocked', 'held'].includes(task.state) ? 'live-node-blocked' : task.state === 'merged' ? 'live-node-merged' : undefined, draggable: false, connectable: false, deletable: false,
      data: { canonicalId: task.id, kind: 'Task', title: task.title ?? task.id, text: `${progressStateLabel(task.state)}${task.needs_recheck === true ? '\n↻ 需复核' : ''}${holds.length ? `\n${holds[0].symbol} ${holds[0].label} · ${holds.length} 项中断` : ''}`, binding: task.id, editable: false, opaque: false, raw: task } };
  });
  const edges: FlowEdge[] = tasks.flatMap(task => Array.isArray(task.depends_on) ? task.depends_on.filter((id): id is string => typeof id === 'string' && ids.has(id)).map(id => ({ id: `dependency:${id}:${task.id}`, type: 'canvas', source: id, target: task.id, sourceHandle: 'side:right', targetHandle: 'side:left', label: '依赖', deletable: false })) : []);
  return { nodes, edges };
}
export function taskEvents(view: TaskProgressView, id: string): RawEvent[] {
  const runs = new Set(view.runs.filter(run => run.task === id).map(run => run.id));
  const evidence = new Set(view.evidence.filter(item => item.task === id || runs.has(string(item.run))).map(item => item.id));
  const holds = new Set(holdsOf(view).filter(hold => {
    const body = object(hold.body); const binding = object(body.binding);
    return hold.task === id || binding.task === id || (Array.isArray(hold.recipients) && hold.recipients.some(recipient => object(recipient).task === id));
  }).map(hold => string(hold.id)));
  return eventsOf(view).filter(event => { const data = object(event.data); const recipients = object(data.hold).recipients; return event.obj === id || runs.has(event.obj) || evidence.has(event.obj) || holds.has(event.obj) || data.task === id || object(data.task).id === id || runs.has(string(data.run)) || runs.has(string(object(data.run).id)) || (Array.isArray(recipients) && recipients.some(recipient => { const target = object(recipient); return target.task === id || runs.has(string(target.run)); })); }).sort((a, b) => a.seq - b.seq);
}

export type LiveStatus = 'loading' | 'live' | 'disconnected' | 'history' | 'unavailable';
/** One reader and subscription per mounted project. Late reads/subscriptions cannot publish after close. */
export function observeProgress(token: string, client: TaskProgressClient, onview: (view: TaskProgressView) => void, onstatus: (status: LiveStatus, message: string) => void): { close: () => void; refresh: () => void } {
  let closed = false, reading = false, dirty = false, forceReconnect = false, attempts = 0, subscription = 0;
  let current: TaskProgressView | null = null, unsubscribe: (() => void) | undefined;
  let refreshTimer: ReturnType<typeof setTimeout> | undefined, reconnectTimer: ReturnType<typeof setTimeout> | undefined;
  const status = (state: LiveStatus, message = '') => { if (!closed) onstatus(state, message); };
  const schedule = () => { if (closed) return; dirty = true; if (!refreshTimer) refreshTimer = setTimeout(() => { refreshTimer = undefined; void read(); }, 100); };
  function retry(failure: unknown) {
    if (closed) return;
    status('disconnected', String(failure));
    if (attempts < 6 && !reconnectTimer) reconnectTimer = setTimeout(() => { reconnectTimer = undefined; void read(true); }, Math.min(16000, 1000 * 2 ** attempts++));
  }
  async function connect() {
    if (closed || !current || current.state !== 'recorded' || !client.subscribe) return;
    const serial = ++subscription; let failed = false;
    unsubscribe?.(); unsubscribe = undefined;
    try {
      const stop = await client.subscribe(token, current.through_seq ?? 0, () => { if (!closed && serial === subscription && !failed) { attempts = 0; schedule(); } }, failure => {
        if (closed || serial !== subscription || failed) return;
        failed = true; unsubscribe?.(); unsubscribe = undefined; retry(failure);
      }, { project_id: current.project_id, revision: current.revision, daemon_instance: current.daemon_instance });
      if (closed || serial !== subscription || failed) stop(); else { unsubscribe = stop; status('live'); }
    } catch (failure) {
      if (!closed && serial === subscription && !failed) retry(failure);
    }
  }
  async function read(reconnect = false) {
    if (closed) return;
    if (reading) { dirty = true; forceReconnect ||= reconnect; return; }
    reading = true; dirty = false;
    const requestedReconnect = reconnect || forceReconnect; forceReconnect = false;
    try {
      const result = await client.load(token);
      if (closed) return;
      const changedIdentity = current !== null && (current.project_id !== result.project_id || current.revision !== result.revision || current.daemon_instance !== result.daemon_instance);
      current = result; onview(result);
      if (result.state !== 'recorded') { subscription++; unsubscribe?.(); unsubscribe = undefined; clearTimeout(reconnectTimer); reconnectTimer = undefined; status(result.state === 'recovery_required' ? 'history' : 'unavailable'); }
      else if (!client.subscribe) status('disconnected', '此客户端未提供实时订阅，当前为读取时记录。');
      else if (!unsubscribe || requestedReconnect || changedIdentity) await connect();
      else status('live');
    } catch (failure) { retry(failure); }
    finally { reading = false; if (dirty) schedule(); }
  }
  status('loading'); void read();
  return { close() { closed = true; subscription++; clearTimeout(refreshTimer); clearTimeout(reconnectTimer); unsubscribe?.(); }, refresh() { attempts = 0; clearTimeout(reconnectTimer); reconnectTimer = undefined; void read(true); } };
}
