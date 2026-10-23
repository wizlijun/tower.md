import type { CanvasNode, DelegationView, ProjectState, ProjectView, PromptScope } from './api'
import type { QuestionCard, RecordedAnswer } from './human'
import { compareAttention, FIRST_SCREEN, signalEntries, signalQuiet } from './attention'

export interface RuntimeTask {
  input: { id: string; title: string; module: string; touches: string[]; allowed_paths: string[]; depends_on: string[]; acceptance: string[] }
  state: 'proposed' | 'ready' | 'running' | 'paused' | 'verifying' | 'held' | 'merged'
  baseline: string
  needs_review: boolean
  active_run: string | null
}
export interface RuntimeRun {
  binding: { id: string; host: string; worktree: string; branch: string; self_reported?: { name: string; version: string } | null }
  task: string
  state: 'active' | 'released'
  cost_usd: number | null
}
export interface ActivityBadge { label: string; symbol: string; tone: 'pending' | 'verifying' | 'failed' | 'passed' }
export interface RuntimeAttention {
  id: string
  title: string
  kind: '禁止' | '检查未通过' | '提问' | '待批提案' | '告警' | '抽查' | '汇报过期' | '承诺弱化'
  priority: number
  at: number
  modules: string[]
  tasks: string[]
  action: string
  question?: QuestionCard
  commit?: string
  proposal?: string
  detail?: string
  /** The delegated object whose acceptance card or reports answer this item. */
  delegation?: string
  /** FR-128 class 1: the sender declared it time-sensitive. */
  urgent?: boolean
  /** The card's object when it is neither a task nor a module. */
  object?: string
  /** The merged signal a person may acknowledge or mute, and the message they see. */
  signal?: { from: string; kind: string; target: string; seen: string }
}
export interface RuntimePrompt {
  id: string
  scope: PromptScope
  text: string
  recipients: { run: string; task: string; module: string }[]
  delivered: string[]
}
export const taskStateLabels: Record<RuntimeTask['state'], string> = {
  proposed: '提议', ready: '就绪', running: '执行中', paused: '暂停', verifying: '已声称、未确认', held: '等待放行', merged: '已合并',
}

function record(value: unknown): Record<string, unknown> {
  return value && typeof value === 'object' && !Array.isArray(value) ? value as Record<string, unknown> : {}
}
function strings(value: unknown): string[] {
  return Array.isArray(value) ? value.filter((item): item is string => typeof item === 'string') : []
}
function timestamp(value: unknown): number {
  if (typeof value === 'number' && Number.isFinite(value)) return value
  const parsed = typeof value === 'string' ? Date.parse(value) : NaN
  return Number.isFinite(parsed) ? parsed : 0
}
export function runtimeProjection(state: ProjectState | null, now = Date.now()) {
  const projection = state?.status === 'ready' ? record(state.projection) : {}
  const tasks = Object.values(record(projection.Task)).filter((value): value is RuntimeTask => {
    const task = record(value), input = record(task.input)
    return typeof input.id === 'string' && typeof input.module === 'string' && typeof input.title === 'string'
      && typeof task.state === 'string' && task.state in taskStateLabels
  }).sort((a, b) => a.input.id.localeCompare(b.input.id))
  const runs = Object.values(record(projection.Run)).filter((value): value is RuntimeRun => {
    const run = record(value), binding = record(run.binding)
    return typeof binding.id === 'string' && typeof run.task === 'string'
      && (run.state === 'active' || run.state === 'released')
  }).sort((a, b) => a.binding.id.localeCompare(b.binding.id))
  const questions = Object.values(record(projection.Hold)).filter((value): value is QuestionCard => {
    const question = record(value), draft = record(question.draft), binding = record(question.binding)
    return typeof draft.id === 'string' && typeof draft.text === 'string' && typeof binding.module === 'string'
      && Array.isArray(draft.options) && Array.isArray(draft.context) && Array.isArray(question.targets)
  }).sort((a, b) => a.draft.id.localeCompare(b.draft.id))
  const answers = Object.fromEntries(Object.entries(record(projection.Answer)).filter(([, value]) => {
    const answer = record(value), input = record(answer.input)
    return typeof input.hold === 'string' && typeof input.text === 'string' && typeof answer.event === 'string'
  })) as Record<string, RecordedAnswer>
  const deliveries = Object.values(record(projection.Delivery))
  const prompts: RuntimePrompt[] = Object.values(record(projection.Prompt)).flatMap((value) => {
    const prompt = record(value)
    if (typeof prompt.id !== 'string' || typeof prompt.text !== 'string' || !Array.isArray(prompt.recipients)) return []
    const delivered = deliveries.flatMap((value) => {
      const delivery = record(record(value).delivery), message = record(delivery.message)
      return message.kind === 'prompt' && message.id === prompt.id && typeof delivery.run === 'string' ? [delivery.run] : []
    })
    return [{ id: prompt.id, text: prompt.text, scope: prompt.scope as PromptScope,
      recipients: prompt.recipients, delivered: [...new Set(delivered)].sort() }]
  }).sort((a, b) => a.id.localeCompare(b.id))
  const attention: RuntimeAttention[] = []
  for (const [id, value] of Object.entries(record(projection.Alert)).sort(([a], [b]) => a.localeCompare(b))) {
      const item = record(value)
      if (item.resolved === true || item.status === 'resolved' || item.state === 'resolved') continue
      const data = record(item.data)
      const detail = record(data.detail)
      const protectedChange = data.reason === 'unapproved_protected_change'
      const failed = data.reason === 'task_gate_failed'
      const incomplete = data.reason === 'inspection_incomplete'
      const affected = tasks.filter((task) => task.input.id === detail.task)
      attention.push({ id, kind: protectedChange || incomplete ? '禁止' : failed ? '检查未通过' : '告警',
        priority: protectedChange || incomplete ? 0 : failed ? 1 : 5,
        at: timestamp(item.at), modules: [...new Set(affected.flatMap((task) => [task.input.module, ...(task.input.touches ?? [])]))],
        tasks: affected.map((task) => task.input.id),
        title: protectedChange ? '主干含未经批准的人定内容变更' : failed ? '任务未通过完成检查' : incomplete ? '主干历史尚不能完整核验' : String(item.title ?? data.reason ?? data.question ?? id),
        action: protectedChange ? '审阅该提交后选择认领、撤销或转提案。' : failed ? '查看验证证据，修正后重新运行完成验证。' : incomplete ? '补全 Git 历史或恢复合法的主干，再等待重新核验。' : '查看原始告警并核对项目状态。',
        commit: protectedChange && typeof detail.commit === 'string' ? detail.commit : undefined,
        detail: typeof detail.error === 'string' ? detail.error : undefined })
  }
  for (const question of questions.filter((question) => !answers[question.draft.id])) {
    attention.push({ id: question.draft.id, kind: '提问', priority: 2, at: timestamp(record(question).opened_at),
      modules: [question.binding.module], tasks: [question.binding.task], action: '回答问题；涉及人定内容的回答仍须经提案批准。',
      title: question.draft.text, question })
  }
  for (const [id, value] of Object.entries(record(projection.Proposal))) {
    const entry = record(value)
    if (['approved', 'rejected', 'superseded'].includes(String(entry.status))) continue
    const proposal = record(entry.proposal), impact = record(proposal.impact)
    const sourceQuestion = questions.find((question) => answers[question.draft.id]?.proposal === id)
    attention.push({ id, kind: '待批提案', priority: 3, at: timestamp(entry.at),
      modules: strings(impact.modules).length ? strings(impact.modules) : sourceQuestion ? [sourceQuestion.binding.module] : [],
      tasks: strings(impact.tasks).length ? strings(impact.tasks) : sourceQuestion ? [sourceQuestion.binding.task] : [],
      action: entry.status === 'submitted' ? '审阅完整差异与影响后批准或驳回。' : '先补全正式提案及验证，再审阅批准。',
      title: String(proposal.reason ?? entry.text ?? id), proposal: id })
  }
  const delegation = state?.status === 'ready' ? state.delegation_view ?? null : null
  attention.push(...delegationAttention(delegation, tasks))
  attention.push(...signalAttention(projection.TowerApi, tasks, now))
  attention.sort(compareAttention)
  const quieted = signalEntries(projection.TowerApi).filter((entry) => signalQuiet(entry, now)
    && (entry.data.level === 'active' || entry.data.level === 'time_sensitive')).length
  return { tasks, runs, questions, answers, prompts, allAttention: attention, attention: attention.slice(0, FIRST_SCREEN),
    later: attention.slice(FIRST_SCREEN), quieted, delegation, backlog: delegation?.backlog ?? null }
}

/**
 * Unresolved spot checks (FR-204) and `active` overdue reports (FR-137) are
 * attention candidates; `passive` overdue reports stay in the task detail.
 */
export function delegationAttention(view: DelegationView | null | undefined, tasks: RuntimeTask[] = []): RuntimeAttention[] {
  if (!view) return []
  const title = (id: string) => view.objects.find((object) => object.id === id)?.title ?? id
  const modules = (id: string) => tasks.filter((task) => task.input.id === id).map((task) => task.input.module)
  const items: RuntimeAttention[] = []
  for (const check of view.spot_checks) {
    if (check.resolved) continue
    items.push({ id: `spot-check:${check.object}:${check.attempt}`, kind: '抽查', priority: 4, at: timestamp(check.done.at),
      modules: modules(check.object), tasks: [check.object], delegation: check.object,
      title: `抽查：${title(check.object)}`, action: '打开验收卡逐条复核证据，再确认或退回。' })
  }
  for (const request of view.report_requests) {
    if (!request.open || request.reply || request.overdue?.data.level !== 'active') continue
    items.push({ id: `report-overdue:${request.id}`, kind: '汇报过期', priority: 2, at: timestamp(request.overdue.event.at),
      modules: modules(request.object), tasks: [request.object], delegation: request.object,
      title: `连续未汇报：${title(request.object)} · ${request.text}`,
      action: '受托者已连续错过检查点汇报；查看任务后决定插话、要求汇报或暂停。' })
  }
  return items
}
/** FR-125: the weakening signal, which only Tower.md itself raises. */
export const WEAKENING = 'core.weakening'
const TOWER = 'process:towermd'
/**
 * Tower API signals (FR-118) at `active` or `time_sensitive` are attention
 * candidates; `log` and `passive` only reach the timeline and the digest.
 * Each merged entry (FR-119) is one item. It leaves the list when the sender
 * sends the same kind at a lower level, or when a person acknowledges or
 * mutes it in the App. The sender is self-reported, except for Tower.md's
 * own `core.weakening`, which becomes a 承诺弱化 card.
 */
export function signalAttention(group: unknown, tasks: RuntimeTask[] = [], now = Date.now()): RuntimeAttention[] {
  return signalEntries(group).flatMap((entry): RuntimeAttention[] => {
    const { data, from } = entry
    if ((data.level !== 'active' && data.level !== 'time_sensitive') || signalQuiet(entry, now)) return []
    const affected = tasks.filter((task) => task.input.id === entry.target)
    const count = entry.count > 1 ? ` ×${entry.count}` : ''
    const measured = data.value === undefined ? '' : ` · ${String(data.value)}${data.threshold === undefined ? '' : ` / ${String(data.threshold)}`}`
    const common = { id: `signal:${from.id}:${data.kind}:${entry.target}`, urgent: data.level === 'time_sensitive',
      at: timestamp(entry.last.at), modules: affected.map((task) => task.input.module), tasks: affected.map((task) => task.input.id),
      title: `${data.title}${count}`, signal: { from: from.id, kind: data.kind, target: entry.target, seen: entry.last.eid } }
    // FR-125: Tower.md's own judgement, not a sender's claim; only it may
    // sign the `core.` namespace.
    if (data.kind === WEAKENING && entry.last.actor === TOWER) {
      return [{ ...common, kind: '承诺弱化' as const, priority: 1,
        object: affected.length ? undefined : entry.target.startsWith('P-') ? `提案 ${entry.target}` : entry.target,
        detail: typeof data.detail === 'string' ? data.detail : undefined,
        action: '打开对应提案、委托卡或门禁配置核对改动；不同意就驳回或要求恢复。' }]
    }
    return [{ ...common, kind: '告警' as const, priority: data.level === 'time_sensitive' ? 2 : 5,
      object: affected.length ? undefined : entry.target,
      detail: `${from.id} 自报 · ${data.kind}${measured}${typeof data.detail === 'string' ? ` · ${data.detail}` : ''}`,
      action: '这是来源自报的提醒，不是证据；核对后再决定是否插话或暂停。' }]
  })
}
export function moduleActivity(tasks: RuntimeTask[], attention: RuntimeAttention[] = [], moduleIds: string[] = []): Record<string, ActivityBadge> {
  const groups = new Map<string, RuntimeTask[]>(moduleIds.map((id) => [id, []]))
  for (const task of tasks) for (const id of new Set([task.input.module, ...(task.input.touches ?? [])])) {
    groups.set(id, [...(groups.get(id) ?? []), task])
  }
  for (const item of attention) for (const id of item.modules) if (!groups.has(id)) groups.set(id, [])
  return Object.fromEntries([...groups].map(([id, tasks]) => {
    const concerns = attention.filter((item) => !item.modules.length || item.modules.includes(id))
    const severe = concerns.some((item) => item.priority <= 1 || item.kind === '待批提案')
    const active = tasks.filter((task) => task.state === 'running' || task.state === 'verifying').length
    const blocked = tasks.some((task) => task.needs_review || task.state === 'held' || task.state === 'paused')
    const done = tasks.length > 0 && tasks.every((task) => task.state === 'merged')
    return [id, {
      label: `${tasks.length} 个任务${active ? ` · ${active} 运行` : ''}${concerns.length ? ` · ${concerns[0].kind} ${concerns.length}` : blocked ? ' · 待关注' : ''}`,
      symbol: severe ? '⛔' : concerns.length ? '?' : blocked ? '!' : active ? '▶' : done ? '✓' : '○',
      tone: severe ? 'failed' : concerns.length || blocked ? 'pending' : active ? 'verifying' : done ? 'passed' : 'pending',
    }]
  }))
}
export function costLabel(value: number | null | undefined): string {
  return typeof value === 'number' && Number.isFinite(value) ? `$${value.toFixed(4)}` : '未知（未报告）'
}

/** Runtime-only positions are allocated once per task; events never relayout. */
export function taskGraph(
  tasks: RuntimeTask[], positions: Map<string, { x: number; y: number }>, scope: string,
): ProjectView {
  const nodes: CanvasNode[] = tasks.map((task) => {
    const key = `${scope}\0${task.input.id}`
    if (!positions.has(key)) {
      const index = [...positions.keys()].filter((entry) => entry.startsWith(`${scope}\0`)).length
      positions.set(key, { x: (index % 2) * 300, y: Math.floor(index / 2) * 160 })
    }
    return {
      id: task.input.id, object_id: task.input.id, object_type: 'Task', kind: 'text',
      title: task.input.title, content: `${taskStateLabels[task.state]}${task.needs_review ? ' · 需按新版本复核' : ''}`,
      ...positions.get(key)!, width: 250, height: 110,
    }
  })
  const ids = new Set(nodes.map((node) => node.id))
  const edges = tasks.flatMap((task) => (task.input.depends_on ?? []).filter((id) => ids.has(id)).map((id) => ({
    id: JSON.stringify([id, task.input.id]), from_node: id, to_node: task.input.id, label: '依赖',
  })))
  return { title: '任务依赖图', reference: 'hub', revision: '', docs_dir: null, nodes, edges, diagnostics: [], unplaced: [] }
}
