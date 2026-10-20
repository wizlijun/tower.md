import type { CanvasNode, ProjectState, ProjectView, PromptScope } from './api'
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
export interface RuntimeAttention { id: string; title: string; kind: '告警' | '中断'; question?: QuestionCard; commit?: string; detail?: string }
export interface RuntimePrompt {
  id: string
  scope: PromptScope
  text: string
  recipients: { run: string; task: string; module: string }[]
  delivered: string[]
}
export const taskStateLabels: Record<RuntimeTask['state'], string> = {
  proposed: '提议', ready: '就绪', running: '执行中', paused: '暂停', verifying: '验证中', held: '等待放行', merged: '已合并',
}

function record(value: unknown): Record<string, unknown> {
  return value && typeof value === 'object' && !Array.isArray(value) ? value as Record<string, unknown> : {}
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
      attention.push({ id, kind: '告警', title: protectedChange ? 'main 含未经批准的受保护变更' : String(item.title ?? data.reason ?? data.question ?? id),
        commit: protectedChange && typeof detail.commit === 'string' ? detail.commit : undefined,
        detail: typeof detail.error === 'string' ? detail.error : undefined })
  }
  for (const question of questions.filter((question) => !answers[question.draft.id])) {
    attention.push({ id: question.draft.id, kind: '中断', title: question.draft.text, question })
  }
  return { tasks, runs, questions, answers, prompts, allAttention: attention, attention: attention.slice(0, 3) }
}
export function moduleActivity(tasks: RuntimeTask[]): Record<string, ActivityBadge> {
  const groups = new Map<string, RuntimeTask[]>()
  for (const task of tasks) groups.set(task.input.module, [...(groups.get(task.input.module) ?? []), task])
  return Object.fromEntries([...groups].map(([id, tasks]) => {
    const active = tasks.filter((task) => task.state === 'running' || task.state === 'verifying').length
    const blocked = tasks.some((task) => task.needs_review || task.state === 'held' || task.state === 'paused')
    const done = tasks.every((task) => task.state === 'merged')
    return [id, {
      label: `${tasks.length} 个任务${active ? ` · ${active} 运行` : ''}${blocked ? ' · 待关注' : ''}`,
      symbol: blocked ? '!' : active ? '▶' : done ? '✓' : '○',
      tone: blocked ? 'failed' : active ? 'verifying' : done ? 'passed' : 'pending',
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
