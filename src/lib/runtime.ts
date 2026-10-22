import type { CanvasNode, DelegationView, ProjectState, ProjectView, PromptScope } from './api'
import type { QuestionCard, RecordedAnswer } from './human'

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
  kind: '禁止' | '门禁失败' | '提问' | '待批提案' | '告警' | '抽查' | '汇报过期'
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
export function runtimeProjection(state: ProjectState | null) {
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
      attention.push({ id, kind: protectedChange || incomplete ? '禁止' : failed ? '门禁失败' : '告警',
        priority: protectedChange || incomplete ? 0 : failed ? 1 : 5,
        at: timestamp(item.at), modules: [...new Set(affected.flatMap((task) => [task.input.module, ...(task.input.touches ?? [])]))],
        tasks: affected.map((task) => task.input.id),
        title: protectedChange ? 'main 含未经批准的受保护变更' : failed ? '任务未通过完成检查' : incomplete ? 'main 历史尚不能完整核验' : String(item.title ?? data.reason ?? data.question ?? id),
        action: protectedChange ? '审阅该提交后选择认领、撤销或转提案。' : failed ? '查看验证证据，修正代码后重新 finish。' : incomplete ? '补全 Git 历史或恢复合法 main，再等待重新核验。' : '查看原始告警并核对项目状态。',
        commit: protectedChange && typeof detail.commit === 'string' ? detail.commit : undefined,
        detail: typeof detail.error === 'string' ? detail.error : undefined })
  }
  for (const question of questions.filter((question) => !answers[question.draft.id])) {
    attention.push({ id: question.draft.id, kind: '提问', priority: 2, at: timestamp(record(question).opened_at),
      modules: [question.binding.module], tasks: [question.binding.task], action: '回答问题；涉及慢层的回答仍须经提案批准。',
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
  attention.sort((a, b) => a.priority - b.priority || a.at - b.at || b.tasks.length - a.tasks.length || a.id.localeCompare(b.id))
  return { tasks, runs, questions, answers, prompts, allAttention: attention, attention: attention.slice(0, 3),
    delegation, backlog: delegation?.backlog ?? null }
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
      title: task.input.title, content: `${taskStateLabels[task.state]}${task.needs_review ? ' · 需复核基线' : ''}`,
      ...positions.get(key)!, width: 250, height: 110,
    }
  })
  const ids = new Set(nodes.map((node) => node.id))
  const edges = tasks.flatMap((task) => (task.input.depends_on ?? []).filter((id) => ids.has(id)).map((id) => ({
    id: JSON.stringify([id, task.input.id]), from_node: id, to_node: task.input.id, label: '依赖',
  })))
  return { title: '任务依赖图', reference: 'hub', revision: '', docs_dir: null, nodes, edges, diagnostics: [], unplaced: [] }
}
