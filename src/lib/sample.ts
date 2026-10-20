import type { CanvasNode, ProjectView } from './api'

export const SAMPLE_NODE_IDS = {
  input: 'sample-input', engine: 'sample-engine', output: 'sample-output',
  baseline: 'sample-baseline', task: 'sample-task', question: 'sample-question',
  verification: 'sample-verification', review: 'sample-review',
} as const

export const SAMPLE_QUESTION = '转换后的文本希望使用什么输出格式？'
export const SAMPLE_VERIFICATION_RESULTS = ['passed', 'failed', 'no_tests'] as const
export type SampleVerificationResult = typeof SAMPLE_VERIFICATION_RESULTS[number]

export type SampleAction =
  | { type: 'approve_baseline' }
  | { type: 'start_task' }
  | { type: 'answer'; text: string }
  | { type: 'verify'; result: SampleVerificationResult }
  | { type: 'approve_review' }
  | { type: 'reject_review' }

export interface SampleState {
  baseline: 'draft' | 'approved'
  task: 'blocked' | 'ready' | 'running' | 'waiting_answer' | 'awaiting_review' | 'done'
  question: 'blocked' | 'open' | 'answered'
  answer: string | null
  verification: 'blocked' | 'ready' | SampleVerificationResult
  review: 'blocked' | 'pending' | 'approved' | 'rejected'
  history: { seq: number; action: SampleAction; message: string }[]
  cost_usd: null
}

export const SAMPLE_LABELS = {
  baseline: { draft: '未批准', approved: '示例已批准' },
  task: { blocked: '等待基线审阅', ready: '待开始', running: '执行中', waiting_answer: '等待回答', awaiting_review: '等待放行', done: '示例已完成' },
  question: { blocked: '任务开始后出现', open: '待回答', answered: '已回答' },
  verification: { blocked: '等待任务与回答', ready: '待验证', passed: '模拟通过', failed: '模拟失败', no_tests: '模拟无测试' },
  review: { blocked: '等待验证通过', pending: '待放行', approved: '示例已放行', rejected: '已退回，需重新验证' },
} as const

export function createSample(): SampleState {
  return {
    baseline: 'draft', task: 'blocked', question: 'blocked', answer: null,
    verification: 'blocked', review: 'blocked', history: [], cost_usd: null,
  }
}

/** A local walkthrough only: no client, repository, command execution or evidence. */
export function transition(state: SampleState, action: SampleAction): SampleState {
  // Replaying the answer cannot ask again, change the answer, or add another record.
  if (action.type === 'answer' && state.question === 'answered' && action.text.trim() === state.answer) {
    return state
  }
  const next: SampleState = {
    ...state, history: state.history.map(entry => ({ ...entry, action: { ...entry.action } })),
  }
  let message: string
  switch (action.type) {
    case 'approve_baseline':
      if (state.baseline !== 'draft') throw new Error('示例基线已批准。')
      next.baseline = 'approved'
      next.task = 'ready'
      message = '批准示例基线，转换任务可以开始。'
      break
    case 'start_task':
      if (state.baseline !== 'approved' || state.task !== 'ready') throw new Error('请先审阅示例基线，再开始待执行任务。')
      next.task = 'waiting_answer'
      next.question = 'open'
      message = '示例任务开始，等待确认输出格式。'
      break
    case 'answer': {
      const answer = action.text.trim()
      if (!answer) throw new Error('请填写回答。')
      if (state.question !== 'open' || state.task !== 'waiting_answer') throw new Error('当前没有待答问题；已有回答会保留。')
      next.answer = answer
      next.question = 'answered'
      next.task = 'running'
      next.verification = 'ready'
      message = `保存示例回答：${answer}`
      break
    }
    case 'verify':
      if (state.baseline !== 'approved' || state.task !== 'running' || state.question !== 'answered') throw new Error('先开始任务并回答问题，才能模拟验证。')
      if (!SAMPLE_VERIFICATION_RESULTS.includes(action.result)) throw new Error('未知的示例验证结果。')
      next.verification = action.result
      next.review = action.result === 'passed' ? 'pending' : 'blocked'
      next.task = action.result === 'passed' ? 'awaiting_review' : 'running'
      message = `${SAMPLE_LABELS.verification[action.result]}；未运行真实命令。`
      break
    case 'approve_review':
      if (state.review !== 'pending' || state.verification !== 'passed' || state.task !== 'awaiting_review') throw new Error('只有本轮模拟验证通过后，才能示例放行。')
      next.review = 'approved'
      next.task = 'done'
      message = '人工放行示例任务；不认证真实任务完成。'
      break
    case 'reject_review':
      if (state.review !== 'pending' || state.task !== 'awaiting_review') throw new Error('当前没有待放行的示例结果。')
      next.review = 'rejected'
      next.task = 'running'
      next.verification = 'ready'
      message = '退回示例结果，保留已有回答，等待重新验证。'
      break
    default:
      throw new Error('未知的示例动作。')
  }
  next.history.push({ seq: next.history.length + 1, action: { ...action }, message })
  return next
}

export function sampleView(state: SampleState): ProjectView {
  const card = (id: string, title: string, content: string, x: number, y: number, object_type: string): CanvasNode => ({
    id, kind: 'text', title, content: `示例 · ${content}`, x, y, width: 300, height: 200,
    object_id: id, object_type,
  })
  const ids = SAMPLE_NODE_IDS
  return {
    title: 'Tower.md · 功能示例', reference: 'sample', revision: 'sample-memory', docs_dir: null,
    diagnostics: ['纯内存功能示例。验证结果为模拟，不执行命令、不生成证据、不批准真实基线，也不写入仓库。'],
    nodes: [
      card(ids.input, '输入模块', '接收待转换文本。接口：把文本交给转换模块。', 0, 0, 'Module'),
      card(ids.engine, '转换模块', '把文本转换为大写。关联任务：补全输出格式。', 360, 0, 'Module'),
      card(ids.output, '输出模块', '按确认的格式返回转换结果。', 720, 0, 'Module'),
      card(ids.baseline, '基线审阅', `${SAMPLE_LABELS.baseline[state.baseline]}\n审阅这 3 个模块与接口后，批准示例基线。`, 0, 300, 'Baseline'),
      card(ids.task, '转换任务', `${SAMPLE_LABELS.task[state.task]}\n让转换结果按约定格式输出。费用：未知。`, 360, 300, 'Task'),
      card(ids.question, '待答问题', `${SAMPLE_LABELS.question[state.question]}\n${SAMPLE_QUESTION}${state.answer === null ? '' : `\n已保存回答：${state.answer}`}`, 720, 300, 'Hold'),
      card(ids.verification, '验证结果', `${SAMPLE_LABELS.verification[state.verification]}\n选择模拟结果体验通过、失败、无测试的路径。`, 360, 600, 'Evidence'),
      card(ids.review, '人工放行', `${SAMPLE_LABELS.review[state.review]}\n通过后可以放行，或退回要求重新验证。`, 720, 600, 'Hold'),
    ],
    edges: [
      { id: 'sample-input-engine', from_node: ids.input, to_node: ids.engine, label: '传入文本' },
      { id: 'sample-engine-output', from_node: ids.engine, to_node: ids.output, label: '输出结果' },
      { id: 'sample-baseline-task', from_node: ids.baseline, to_node: ids.task, label: '批准后开始' },
      { id: 'sample-module-task', from_node: ids.engine, to_node: ids.task, label: '关联任务', from_side: 'bottom', to_side: 'top' },
      { id: 'sample-task-question', from_node: ids.task, to_node: ids.question, label: '待确认' },
      { id: 'sample-task-verification', from_node: ids.task, to_node: ids.verification, label: '验证', from_side: 'bottom', to_side: 'top' },
      { id: 'sample-verification-review', from_node: ids.verification, to_node: ids.review, label: '通过后放行' },
    ],
    unplaced: [],
  }
}
