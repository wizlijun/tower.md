import type { PromptScope } from './api'
import { rfc3339Local } from './delegation'

/**
 * FR-70: quick commands typed at the prompt entry. Each one maps to an
 * action the App already has; none adds a new power. The daemon checks
 * every request again, so parsing here only turns text into that action.
 */
export type QuickContext = { kind: 'prompt'; scope: PromptScope } | { kind: 'answer' }
export type QuickCommand =
  | { kind: 'message'; text: string }
  | { kind: 'answer'; text: string; assumption: string }
  | { kind: 'pause' }
  | { kind: 'resume' }
  /** Spec 15.5 and the development kit's `/retry`: one task, not the whole project. */
  | { kind: 'pause_task' | 'resume_task' | 'retry'; task: string }
  | { kind: 'report'; target: string; text: string; due: string }
  | { kind: 'rule'; scope: PromptScope; text: string; until: string }
  | { kind: 'error'; message: string }

export const QUICK_COMMANDS: { usage: string; does: string; where: QuickContext['kind'] }[] = [
  { usage: '/pause', does: '在项目入口暂停整个项目（同"暂停"按钮）；在任务上只暂停这个任务', where: 'prompt' },
  { usage: '/resume', does: '恢复项目或这个任务的暂停', where: 'prompt' },
  { usage: '/retry', does: '在任务上输入：把这个任务重新排队派发', where: 'prompt' },
  { usage: '/report [内容]', does: '请受托者 24 小时内汇报进展', where: 'prompt' },
  { usage: '/rule <内容> --until <时间>', does: '设临时规则：30m、2h、3d、18:00 或完整时间，最长 30 天', where: 'prompt' },
  { usage: '/assume <假设>', does: '单独一行，随回答一起记下所依据的假设', where: 'answer' },
]

const RULE_CHARS = 200
const ASSUMPTION_CHARS = 200
const MAX_DAYS = 30
const REPORT_HOURS = 24
const DEFAULT_REPORT = '请汇报当前进展'

function lineOk(text: string, limit: number): boolean {
  return [...text].length <= limit && !/\p{Cc}/u.test(text)
}

/** `30m`, `2h`, `3d`, `18:00` (next occurrence) or a date and time; local time unless an offset is given. */
export function parseUntil(spec: string, now: Date): Date | null {
  const relative = /^(\d{1,4})([mhd])$/.exec(spec)
  if (relative) {
    const unit = { m: 60_000, h: 3_600_000, d: 86_400_000 }[relative[2] as 'm' | 'h' | 'd']
    return new Date(now.getTime() + Number(relative[1]) * unit)
  }
  const clock = /^(\d{1,2}):(\d{2})$/.exec(spec)
  if (clock) {
    const [hours, minutes] = [Number(clock[1]), Number(clock[2])]
    if (hours > 23 || minutes > 59) return null
    const at = new Date(now.getFullYear(), now.getMonth(), now.getDate(), hours, minutes)
    if (at <= now) at.setDate(at.getDate() + 1)
    return at
  }
  const local = /^(\d{4})-(\d{1,2})-(\d{1,2})[ T](\d{1,2}):(\d{2})$/.exec(spec)
  if (local) {
    const [year, month, day, hours, minutes] = local.slice(1).map(Number)
    const at = new Date(year, month - 1, day, hours, minutes)
    return at.getMonth() === month - 1 && at.getDate() === day && at.getHours() === hours ? at : null
  }
  if (/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}(:\d{2})?(\.\d+)?(Z|[+-]\d{2}:\d{2})$/.test(spec)) {
    const at = new Date(spec)
    return Number.isNaN(at.getTime()) ? null : at
  }
  return null
}

function rule(rest: string, scope: PromptScope, now: Date): QuickCommand {
  const match = /^(.*?)\s+--until\s+(\S+(?: \d{1,2}:\d{2})?)$/.exec(rest)
  if (!match) return { kind: 'error', message: '临时规则须带到期时间：/rule <内容> --until 2h' }
  const text = match[1].trim()
  if (!text || !lineOk(text, RULE_CHARS)) return { kind: 'error', message: `临时规则须是一句话：非空、不换行、不超过 ${RULE_CHARS} 字` }
  const until = parseUntil(match[2], now)
  if (!until) return { kind: 'error', message: `看不懂到期时间"${match[2]}"：可写 30m、2h、3d、18:00 或 2026-10-25 18:00` }
  // RFC 3339 here keeps whole minutes, so compare what is actually sent.
  const sent = rfc3339Local(until)
  const at = new Date(sent).getTime()
  if (at <= now.getTime()) return { kind: 'error', message: '临时规则的到期时间须晚于现在' }
  if (at > now.getTime() + MAX_DAYS * 86_400_000) return { kind: 'error', message: `临时规则最长 ${MAX_DAYS} 天` }
  return { kind: 'rule', scope, text, until: sent }
}

function answer(input: string): QuickCommand {
  const lines = input.split('\n')
  const marked = lines.filter((line) => /^\s*\/assume(\s|$)/.test(line))
  if (!marked.length) return { kind: 'message', text: input }
  if (marked.length > 1) return { kind: 'error', message: '一条回答只写一行 /assume' }
  const assumption = marked[0].trim().slice('/assume'.length).trim()
  if (!assumption || !lineOk(assumption, ASSUMPTION_CHARS)) return { kind: 'error', message: `假设须是不超过 ${ASSUMPTION_CHARS} 字的一行` }
  const text = lines.filter((line) => line !== marked[0]).join('\n').trim()
  if (!text) return { kind: 'error', message: '写了假设，还要写回答本身' }
  return { kind: 'answer', text, assumption }
}

/**
 * Plain text is a message (a prompt or an answer). A first word `/name`
 * is a command; `//` at the start sends the rest as plain text.
 */
export function parseQuickCommand(input: string, context: QuickContext, now = new Date()): QuickCommand {
  const text = input.trim()
  if (text.startsWith('//')) return { kind: 'message', text: text.slice(1) }
  if (context.kind === 'answer') return answer(input)
  const command = /^\/([a-z]+)(?:\s+([\s\S]*))?$/.exec(text)
  if (!command) return { kind: 'message', text: input }
  const [name, rest = ''] = [command[1], (command[2] ?? '').trim()]
  const scope = context.scope
  switch (name) {
    case 'pause':
    case 'resume':
      if (rest) return { kind: 'error', message: `/${name} 不带内容` }
      if (scope.kind === 'task') return { kind: name === 'pause' ? 'pause_task' : 'resume_task', task: scope.task }
      if (scope.kind !== 'global') return { kind: 'error', message: `暂停与恢复针对整个项目或一个任务；请从项目入口或任务上输入 /${name}` }
      return { kind: name }
    case 'retry':
      if (rest) return { kind: 'error', message: '/retry 不带内容' }
      if (scope.kind !== 'task') return { kind: 'error', message: '/retry 须指向一个任务；请从该任务插话' }
      return { kind: 'retry', task: scope.task }
    case 'report': {
      if (scope.kind === 'global') return { kind: 'error', message: '/report 须指向一个任务或模块；请从该节点插话' }
      const target = scope.kind === 'task' ? scope.task : scope.module
      const due = rfc3339Local(new Date(now.getTime() + REPORT_HOURS * 3_600_000))
      return { kind: 'report', target, text: rest || DEFAULT_REPORT, due }
    }
    case 'rule': return rule(rest, scope, now)
    case 'assume': return { kind: 'error', message: '/assume 只用于回答问题' }
    default: return { kind: 'error', message: `没有快捷命令 /${name}；要原样发送，以 // 开头` }
  }
}
