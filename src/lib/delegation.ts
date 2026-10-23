import type {
  Brief, Checkpoint, ConfirmInput, DelegationPhase, DelegationRequest, DelegationView, EvidenceSourceKind, FinalizeInput,
  LedgerRequest, LevelInput, Measure, Ratio, RenameInput, RuleInput, SelfReported, SendBackInput, TrackRecord, UsedInput,
} from './api'

/** `claimed_done` is never shown as complete (FR-33, FR-124). */
export const phaseLabels: Record<DelegationPhase, string> = {
  clarifying: '待澄清', proposed: '待定稿', ready: '已委托，待领取', running: '执行中',
  claimed_done: '已声称、未确认', done: '已完成',
}

/** FR-209: shape, color class and text label; never color alone. */
export const evidenceKinds: Record<EvidenceSourceKind, { symbol: string; label: string }> = {
  check: { symbol: '■', label: '检查' },
  claimed: { symbol: '○', label: '受托者自述' },
  used: { symbol: '▲', label: '人用过' },
}

export const levelSourceLabels: Record<TrackRecord['level_source'], string> = {
  exact: '本配置 × 本类别', config: '本配置', work_kind: '本类别', project: '项目级', default: '默认（清单第一项）',
}

export type DelegationIntent =
  | { kind: 'finalize'; object: string; data: FinalizeInput }
  | { kind: 'used'; object: string; data: UsedInput }
  | { kind: 'confirm'; object: string; data: ConfirmInput }
  | { kind: 'send_back'; object: string; data: SendBackInput }
  | { kind: 'rule'; object: string; data: RuleInput }
  | { kind: 'rename'; data: RenameInput }
  | { kind: 'level'; data: LevelInput }
export type DelegationAction =
  | { kind: 'finalize'; input: DelegationRequest<FinalizeInput> }
  | { kind: 'used'; input: DelegationRequest<UsedInput> }
  | { kind: 'confirm'; input: DelegationRequest<ConfirmInput> }
  | { kind: 'send_back'; input: DelegationRequest<SendBackInput> }
  | { kind: 'rule'; input: DelegationRequest<RuleInput> }
  | { kind: 'rename'; input: LedgerRequest<RenameInput> }
  | { kind: 'level'; input: LedgerRequest<LevelInput> }

/** After a send-back the App offers to keep the correction as a rule (FR-205). */
export interface RuleOffer { object: string; kind: string; source: string; text: string }

/** What the App hands to every delegation panel: one shared write channel. */
export interface DelegationControls {
  view: DelegationView
  disabled: boolean
  busy: boolean
  pending: DelegationAction | null
  error: string | null
  result: string | null
  ruleOffer: RuleOffer | null
  onAction: (intent: DelegationIntent) => void
  onRetry: () => void
  onDismissRule: () => void
}

export function actionObject(action: DelegationAction): string | null {
  return 'object' in action.input ? action.input.object : null
}

export function lines(value: string): string[] {
  return [...new Set(value.split('\n').map((line) => line.trim()).filter(Boolean))]
}

function pad(value: number): string {
  return String(value).padStart(2, '0')
}

/** Local wall time as RFC 3339 with the local offset at that instant. */
export function rfc3339Local(date: Date): string {
  const offset = -date.getTimezoneOffset()
  const sign = offset >= 0 ? '+' : '-'
  const abs = Math.abs(offset)
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}T${pad(date.getHours())}:${pad(date.getMinutes())}:00`
    + `${sign}${pad(Math.floor(abs / 60))}:${pad(abs % 60)}`
}

const DUE = /^(.*?)\s*@\s*(\d{4})-(\d{1,2})-(\d{1,2})[ T](\d{1,2}):(\d{2})$/

/** One checkpoint per line; `text @ 2026-10-25 18:00` (local time) sets a due time. */
export function parseCheckpoints(value: string): { checkpoints: Checkpoint[]; invalid: string[] } {
  const checkpoints: Checkpoint[] = [], invalid: string[] = []
  for (const line of value.split('\n').map((entry) => entry.trim()).filter(Boolean)) {
    const match = DUE.exec(line)
    if (!match) { checkpoints.push({ text: line }); continue }
    const [, text, year, month, day, hour, minute] = match
    const parts = [year, month, day, hour, minute].map(Number)
    const date = new Date(parts[0], parts[1] - 1, parts[2], parts[3], parts[4])
    const exact = date.getFullYear() === parts[0] && date.getMonth() === parts[1] - 1 && date.getDate() === parts[2]
      && date.getHours() === parts[3] && date.getMinutes() === parts[4]
    if (!text.trim() || !exact) invalid.push(line)
    else checkpoints.push({ text: text.trim(), due: rfc3339Local(date) })
  }
  return { checkpoints, invalid }
}

/** The editable line for a checkpoint, in local time. */
export function checkpointLine(checkpoint: Checkpoint): string {
  const date = checkpoint.due ? new Date(checkpoint.due) : null
  if (!date || Number.isNaN(date.getTime())) return checkpoint.text
  return `${checkpoint.text} @ ${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())} ${pad(date.getHours())}:${pad(date.getMinutes())}`
}

export function localTime(value: string): string {
  const date = new Date(value)
  if (Number.isNaN(date.getTime())) return value
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())} ${pad(date.getHours())}:${pad(date.getMinutes())}`
}

export interface BriefFields { why: string; done_when: string; limits: string; checkpoints: string; self_check: string }

/** The card in its fixed order; empty optional fields are omitted, not sent blank. */
export function buildBrief(fields: BriefFields): { brief: Brief; invalid: string[] } {
  const { checkpoints, invalid } = parseCheckpoints(fields.checkpoints)
  const brief = {
    ...(fields.why.trim() ? { why: fields.why.trim() } : {}),
    done_when: lines(fields.done_when),
    ...(fields.limits.trim() ? { limits: fields.limits.trim() } : {}),
    ...(checkpoints.length ? { checkpoints } : {}),
    ...(fields.self_check.trim() ? { self_check: fields.self_check.trim() } : {}),
  }
  return { brief, invalid }
}

export function delegateLabel(delegate: SelfReported | null | undefined): string {
  if (!delegate) return '未自报'
  return [`${delegate.name}/${delegate.version}`, delegate.vendor, delegate.model, delegate.skills && `技能 ${delegate.skills}`]
    .filter(Boolean).join(' · ') + '（自报）'
}

export function formatDuration(ms: number | null | undefined): string {
  if (typeof ms !== 'number' || !Number.isFinite(ms)) return '—'
  const seconds = Math.round(ms / 1000)
  if (seconds < 60) return `${seconds} 秒`
  const minutes = Math.floor(seconds / 60)
  if (minutes < 60) return `${minutes} 分`
  const hours = Math.floor(minutes / 60)
  if (hours < 24) return `${hours} 小时 ${minutes % 60} 分`
  return `${Math.floor(hours / 24)} 天 ${hours % 24} 小时`
}

export function formatRatio(ratio: Ratio): string {
  return ratio.rate === null ? '—' : `${Math.round(ratio.rate * 100)}%（${ratio.count}/${ratio.total}）`
}

export function formatMeasure(measure: Measure, format: (value: number) => string = formatDuration): string {
  return measure.median === null ? '—' : `中位 ${format(measure.median)} · 合计 ${format(measure.total)}（${measure.count} 次）`
}

/** Undetected measures are null and stay "未接入", never 0. */
export function formatCount(value: number | null, unknown = '未接入'): string {
  return value === null ? unknown : String(value)
}

/**
 * Review time counts only while the card is visible in the App (FR-202):
 * hidden windows pause it. Nothing outside the App is tracked.
 */
export function reviewTimer(options: { now?: () => number; visible?: () => boolean; target?: EventTarget } = {}) {
  const now = options.now ?? (() => performance.now())
  const visible = options.visible ?? (() => document.visibilityState !== 'hidden')
  const target = options.target ?? document
  let total = 0
  let since: number | null = visible() ? now() : null
  const update = () => {
    if (visible() && since === null) since = now()
    else if (!visible() && since !== null) { total += now() - since; since = null }
  }
  target.addEventListener('visibilitychange', update)
  return {
    elapsed: () => Math.max(0, Math.round(total + (since === null ? 0 : now() - since))),
    dispose: () => target.removeEventListener('visibilitychange', update),
  }
}
