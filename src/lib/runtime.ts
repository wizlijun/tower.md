import type { CanvasNode, DelegationView, ProjectState, ProjectView, PromptScope } from './api'
import type { QuestionCard, RecordedAnswer } from './human'
import { attentionItem, attentionList, attentionSettings, signalEntries, signalQuiet, TOWER } from './attention'
import { followUpsFrom, recap, rhythmAttention, rhythmFrom, type Recap } from './follow-ups'

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
  kind: '禁止' | '检查未通过' | '提问' | '待批提案' | '告警' | '抽查' | '失联' | '兜圈子' | '无人可答' | '承诺弱化' | '跟进' | '浮现' | '豁免到期' | '复查决策'
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
  /**
   * FR-138: a submitted proposal approved on the card in one step, checked
   * against these fields of the prepared publication. Absent when it lowers
   * the bar (FR-125): that one is reviewed in full.
   */
  approve?: { draft_id: string | null; project_id: string; source_revision: string }
  /** FR-138, FR-208: the spot-checked attempt confirmed on the card, recording its unproven criteria. */
  confirm?: { object: string; attempt: string; unproven: number[] }
  /** FR-134: the due follow-up a person completes on the card. */
  followUp?: string
  /** FR-186: the decision a person keeps or voids on the card. */
  decisionReview?: string
  /** FR-136: the resurfaced object a person settles with "看过了". */
  resurfaced?: string
  /** FR-136: "上次到哪了" for a resurfaced object. */
  recap?: Recap
  /** FR-130: when the item may interrupt, as the daemon decided. */
  timing?: 'now' | 'next_visit'
  /** FR-135: about quiet objects only; waits under "later". */
  quiet?: boolean
}
export interface RuntimePrompt {
  id: string
  scope: PromptScope
  text: string
  recipients: { run: string; task: string; module: string }[]
  delivered: string[]
  /** Runs whose delegate said it acted on the prompt (FR-68), with its note. */
  handled: Record<string, string | null>
}
/** FR-68's three states, in their only order. */
export type PromptState = '排队中' | '已送达' | '已处理'
export function promptState(prompt: RuntimePrompt, run: string): PromptState {
  return run in prompt.handled ? '已处理' : prompt.delivered.includes(run) ? '已送达' : '排队中'
}
export function promptSummary(prompt: RuntimePrompt): string {
  const counts = { 排队中: 0, 已送达: 0, 已处理: 0 }
  for (const recipient of prompt.recipients) counts[promptState(prompt, recipient.run)] += 1
  return (['已处理', '已送达', '排队中'] as const).filter((state) => counts[state]).map((state) => `${counts[state]} ${state}`).join(' · ')
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
  const handledReceipts = Object.values(record(projection.PromptHandled))
  const prompts: RuntimePrompt[] = Object.values(record(projection.Prompt)).flatMap((value) => {
    const prompt = record(value)
    if (typeof prompt.id !== 'string' || typeof prompt.text !== 'string' || !Array.isArray(prompt.recipients)) return []
    const delivered = deliveries.flatMap((value) => {
      const delivery = record(record(value).delivery), message = record(delivery.message)
      return message.kind === 'prompt' && message.id === prompt.id && typeof delivery.run === 'string' ? [delivery.run] : []
    })
    const handled: Record<string, string | null> = {}
    for (const value of handledReceipts) {
      const receipt = record(value), delivery = record(receipt.delivery), message = record(delivery.message)
      if (message.kind === 'prompt' && message.id === prompt.id && typeof delivery.run === 'string') {
        handled[delivery.run] = typeof receipt.note === 'string' ? receipt.note : null
      }
    }
    return [{ id: prompt.id, text: prompt.text, scope: prompt.scope as PromptScope,
      recipients: prompt.recipients, delivered: [...new Set(delivered)].sort(), handled }]
  }).sort((a, b) => a.id.localeCompare(b.id))
  const attention: RuntimeAttention[] = []
  for (const [id, value] of Object.entries(record(projection.Alert)).sort(([a], [b]) => a.localeCompare(b))) {
      const item = record(value)
      if (item.resolved === true || item.status === 'resolved' || item.state === 'resolved') continue
      const data = record(item.data)
      const detail = record(data.detail)
      const protectedChange = data.reason === 'unapproved_protected_change'
      // FR-03: a commit that changed only process configuration is a minor change, registered rather than versioned.
      const minor = protectedChange && detail.class === 'minor'
      const failed = data.reason === 'task_gate_failed'
      const incomplete = data.reason === 'inspection_incomplete'
      const affected = tasks.filter((task) => task.input.id === detail.task)
      attention.push({ id, kind: protectedChange || incomplete ? '禁止' : failed ? '检查未通过' : '告警',
        priority: protectedChange || incomplete ? 0 : failed ? 1 : 5,
        at: timestamp(item.at), modules: [...new Set(affected.flatMap((task) => [task.input.module, ...(task.input.touches ?? [])]))],
        tasks: affected.map((task) => task.input.id),
        title: minor ? '主干含未登记的流程配置变更（次要变更）' : protectedChange ? '主干含未经批准的人定内容变更' : failed ? '任务未通过完成检查' : incomplete ? '主干历史尚不能完整核验' : String(item.title ?? data.reason ?? data.question ?? id),
        action: minor ? '审阅该提交后转提案登记；流程配置不能认领或普通撤销。' : protectedChange ? '审阅该提交后选择认领、撤销或转提案。' : failed ? '查看验证证据，修正后重新运行完成验证。' : incomplete ? '补全 Git 历史或恢复合法的主干，再等待重新核验。' : '查看原始告警并核对项目状态。',
        commit: protectedChange && typeof detail.commit === 'string' ? detail.commit : undefined,
        detail: typeof detail.error === 'string' ? detail.error : undefined })
  }
  for (const question of questions.filter((question) => !answers[question.draft.id])) {
    attention.push({ id: question.draft.id, kind: '提问', priority: 2, at: timestamp(record(question).opened_at),
      modules: [question.binding.module], tasks: [question.binding.task], action: '回答问题；涉及人定内容的回答仍须经提案批准。',
      title: question.draft.text, question })
  }
  const lowered = new Set(signalEntries(projection.TowerApi).filter((entry) => entry.data.kind === WEAKENING && entry.last.actor === TOWER)
    .map((entry) => entry.target))
  for (const [id, value] of Object.entries(record(projection.Proposal))) {
    const entry = record(value)
    if (['approved', 'rejected', 'superseded'].includes(String(entry.status))) continue
    const proposal = record(entry.proposal), impact = record(proposal.impact)
    const sourceQuestion = questions.find((question) => answers[question.draft.id]?.proposal === id)
    const ready = entry.status === 'submitted' && !lowered.has(id) && typeof proposal.project_id === 'string'
      && typeof proposal.source_revision === 'string' && (proposal.draft_id === null || typeof proposal.draft_id === 'string')
    attention.push({ id, kind: '待批提案', priority: 3, at: timestamp(entry.at),
      approve: ready ? { draft_id: proposal.draft_id as string | null, project_id: proposal.project_id as string,
        source_revision: proposal.source_revision as string } : undefined,
      modules: strings(impact.modules).length ? strings(impact.modules) : sourceQuestion ? [sourceQuestion.binding.module] : [],
      tasks: strings(impact.tasks).length ? strings(impact.tasks) : sourceQuestion ? [sourceQuestion.binding.task] : [],
      action: entry.status === 'submitted' ? '审阅完整差异与影响后批准或驳回。' : '先补全正式提案及验证，再审阅批准。',
      title: String(proposal.reason ?? entry.text ?? id), proposal: id })
  }
  const delegation = state?.status === 'ready' ? state.delegation_view ?? null : null
  attention.push(...delegationAttention(delegation, tasks))
  attention.push(...signalAttention(projection.TowerApi, tasks, now))
  attention.push(...waiverAttention(projection.Waiver, tasks, now))
  attention.push(...decisionReviewAttention(projection.QuestionDecision, projection.DecisionReview, now, questions, answers))
  const followUps = followUpsFrom(projection)
  const rhythm = rhythmFrom(state?.status === 'ready' ? state.quiet : null)
  attention.push(...rhythmAttention({ rhythm, followUps, tasks, delegation, recap: (object) => recap(object, {
    delegation, towerApi: projection.TowerApi, questions, answers, proposals: projection.Proposal, followUps }) }))
  // FR-128, FR-135: the daemon orders the list, sets the first screen and puts
  // items about quiet objects under "later"; the App's own items above only
  // add what their cards act on. An older daemon sends no list: the items keep
  // their assembly order and none is put aside.
  const settings = attentionSettings(state?.status === 'ready' ? state.attention : null)
  const list = attentionList(state?.status === 'ready' ? state.attention_list : null)
  const own = new Map(attention.map((item) => [item.id, item]))
  const ordered = list ? list.items.map((entry) => ({ ...attentionItem(entry), ...own.get(entry.id), timing: entry.timing, quiet: entry.quiet })) : attention
  const shown = Math.min(list ? list.shown : settings.budget, ordered.length)
  const asleep = list ? ordered.filter((item) => item.quiet) : []
  const quieted = signalEntries(projection.TowerApi).filter((entry) => signalQuiet(entry, now)
    && (entry.data.level === 'active' || entry.data.level === 'time_sensitive')).length
  return { tasks, runs, questions, answers, prompts, allAttention: ordered, attention: ordered.slice(0, shown),
    later: ordered.slice(shown), shown, asleep: asleep.length, settings, quieted, delegation, backlog: delegation?.backlog ?? null,
    followUps, rhythm }
}

/** Within this long of its expiry a waiver joins the attention list (core `waivers::SOON_HOURS`). */
export const WAIVER_SOON_MS = 24 * 3600 * 1000

/**
 * FR-72: a waiver in its last day. It is never extended: once it lapses the
 * gate blocks again, so a person decides now whether the fix lands in time.
 */
export function waiverAttention(group: unknown, tasks: RuntimeTask[] = [], now = Date.now()): RuntimeAttention[] {
  return Object.values(record(group)).flatMap((value): RuntimeAttention[] => {
    const waiver = record(value), granted = record(waiver.granted)
    if (typeof waiver.id !== 'string' || typeof waiver.object !== 'string' || typeof waiver.gate !== 'string'
      || typeof waiver.until !== 'string') return []
    const until = Date.parse(waiver.until)
    if (!Number.isFinite(until) || until <= now || until - now > WAIVER_SOON_MS) return []
    const affected = tasks.filter((task) => task.input.id === waiver.object)
    return [{ id: `waiver:${waiver.id}`, kind: '豁免到期', priority: 3, at: timestamp(granted.at),
      modules: affected.map((task) => task.input.module), tasks: affected.map((task) => task.input.id),
      object: affected.length ? undefined : waiver.object, delegation: waiver.object,
      title: `门禁 ${waiver.gate} 的豁免将于 ${new Date(until).toLocaleString()} 到期`,
      detail: `豁免理由：${String(waiver.reason ?? '')}；到期后不续期，门禁恢复拦截`,
      action: '到期前修好让检查通过；仍需放行就等到期后另行豁免。' }]
  })
}

/** The UTC calendar day of a time (core `decision_review::day`). */
function utcDay(at: number): string {
  return new Date(at).toISOString().slice(0, 10)
}

/**
 * FR-186: from its review day on (UTC), a decision comes back with the
 * question, what was decided and why. The latest keep sets the next day
 * (none ends reviews); a voided or superseded decision never comes back.
 */
export function decisionReviewAttention(decisions: unknown, reviews: unknown, now = Date.now(),
  questions: QuestionCard[] = [], answers: Record<string, unknown> = {}): RuntimeAttention[] {
  const all = Object.entries(record(decisions)).map(([id, value]) => [id, record(value)] as const)
  const superseded = new Set(all.flatMap(([, decision]) => strings(decision.supersedes)))
  return all.flatMap(([id, decision]): RuntimeAttention[] => {
    if (superseded.has(id) || typeof decision.question !== 'string') return []
    const review = record(record(reviews)[id])
    const date = review.voided ? undefined : review.kept ? review.review_on : decision.review_on
    if (typeof date !== 'string' || !/^\d{4}-\d{2}-\d{2}$/.test(date) || utcDay(now) < date) return []
    const reason = typeof decision.rationale === 'string' ? `，理由：${decision.rationale}` : ''
    // The options come from the question this decision answered, when it is still in the projection.
    const eid = record(decision.source).eid
    const hold = typeof eid === 'string' ? Object.entries(answers).find(([, answer]) => record(answer).event === eid)?.[0] : undefined
    const offered = questions.find((question) => question.draft.id === hold)?.draft.options ?? []
    const options = offered.length ? `当时的选项：${offered.join('、')}；` : ''
    const module = typeof decision.module === 'string' && decision.module ? [decision.module] : []
    return [{ id: `decision-review:${id}`, kind: '复查决策', priority: 6, at: Date.parse(`${date}T00:00:00Z`),
      modules: module, tasks: [], object: `决策 ${id}`, decisionReview: id, title: decision.question,
      detail: `${date} 到期复查；${options}当时定为“${String(decision.decision ?? '')}”${reason}`, action: '看当时的决定和理由再定' }]
  })
}

/**
 * Unresolved spot checks (FR-204) and open questions nobody can answer yet
 * (FR-127) are attention candidates. Silence (FR-137) and stuck work
 * (FR-126) arrive as Tower.md's own signals, see `signalAttention`.
 */
export function delegationAttention(view: DelegationView | null | undefined, tasks: RuntimeTask[] = []): RuntimeAttention[] {
  if (!view) return []
  const title = (id: string) => view.objects.find((object) => object.id === id)?.title ?? id
  const modules = (id: string) => tasks.filter((task) => task.input.id === id).map((task) => task.input.module)
  const items: RuntimeAttention[] = []
  for (const check of view.spot_checks) {
    if (check.resolved) continue
    const card = view.objects.find((object) => object.id === check.object)?.card
    items.push({ id: `spot-check:${check.object}:${check.attempt}`, kind: '抽查', priority: 4, at: timestamp(check.done.at),
      modules: modules(check.object), tasks: [check.object], delegation: check.object,
      confirm: card?.can_judge && card.attempt === check.attempt ? { object: check.object, attempt: check.attempt, unproven: [...card.unproven] } : undefined,
      title: `抽查：${title(check.object)}`, action: '打开验收卡逐条复核证据，再确认或退回。' })
  }
  for (const request of view.requests ?? []) {
    if (!request.open || request.recipients.length) continue
    const target = request.data.target
    const known = view.objects.some((object) => object.id === target)
    items.push({ id: `unanswerable:${request.id}`, kind: '无人可答', priority: 3, at: timestamp(request.event.at),
      modules: modules(target), tasks: known ? [target] : [], delegation: known ? target : undefined,
      object: known || modules(target).length ? undefined : target,
      title: `无人可答：${title(target)} · ${request.data.text}`,
      action: '还没有受托者声明能回答这个节点；它会排队到有人声明为止。可以撤回，或在节点上改问别人。' })
  }
  return items
}
/** FR-125: the weakening signal, which only Tower.md itself raises. */
export const WEAKENING = 'core.weakening'
/** FR-137 and FR-126: Tower.md's own judgements that a request went unanswered or work is going in circles. */
export const SILENT = 'core.silent'
export const STUCK = 'core.stuck'
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
    if ((data.kind === SILENT || data.kind === STUCK) && entry.last.actor === TOWER) {
      const silent = data.kind === SILENT
      return [{ ...common, kind: silent ? '失联' as const : '兜圈子' as const, priority: 3,
        object: affected.length ? undefined : entry.target,
        detail: typeof data.detail === 'string' ? data.detail : undefined,
        action: silent ? '请求过了期限仍无人回答；查看后决定插话、换人或撤回请求。'
          : '同一件事反复打转；查看任务后决定插话、改写委托卡或暂停。' }]
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
