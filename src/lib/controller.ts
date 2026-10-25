// M3-D controller answers, M3-H cross reviews and WP-X raw output archives,
// as the App shows them. All three are a process's records: the controller
// answers within a level a person set, a cross review is evidence for the
// person who decides, and an archive says where the raw output is. Nothing
// here writes.
import type { RecordedAnswer } from './human'

export const CONTROLLER = 'process:towermd-controller'

export type Verdict = 'agree' | 'concerns' | 'disagree' | 'unfinished'
export interface CrossReviewEntry {
  proposal: string
  proposal_hash: string
  agent: string | null
  harness: string
  model: string | null
  verdict: Verdict
  summary: string
  findings: { severity: 'high' | 'medium' | 'low'; text: string }[]
  error: string | null
  cost_usd: number | null
}
export interface ControllerOverride {
  hold: string; run: string; task: string
  replaced: string; replaced_at: string; replaced_text: string
  answer: string
  /** `run.finished` stamps of the run after the controller answered. */
  affected: string[]
}
export interface ArchiveSummary { attempts: number; localOnly: number }

function record(value: unknown): Record<string, unknown> {
  return value && typeof value === 'object' && !Array.isArray(value) ? value as Record<string, unknown> : {}
}

/** FR-65: who answered and on what basis; null for a person's own answer. */
export function controllerNote(answer: RecordedAnswer): string | null {
  if (answer.actor !== CONTROLLER) return null
  const basis = answer.basis?.length ? `依据：${answer.basis.join('；')}` : '未给出依据'
  return `总控代答（项目进程，不是人）· ${basis}${answer.notify ? ' · 答后通知' : ''}`
}

/** FR-66: a person replaced the controller's answer; earlier work may rest on the old one. */
export function overrideNote(answer: RecordedAnswer, override: ControllerOverride | undefined): string | null {
  if (!answer.overrides) return null
  const after = override?.affected.length
    ? `此后 ${override.task} 已结束 ${override.affected.length} 次尝试，可能按原回答做了，需要时重新派发`
    : '此后尚无结束的尝试'
  return `已改答：原总控回答"${answer.overrides.text}"（${answer.overrides.answered_at}）· ${after}`
}

export function overridesFrom(projection: unknown): Record<string, ControllerOverride> {
  return Object.fromEntries(Object.entries(record(record(projection).ControllerOverride)).filter(([, value]) => {
    const entry = record(value)
    return typeof entry.task === 'string' && typeof entry.replaced_text === 'string' && Array.isArray(entry.affected)
  })) as Record<string, ControllerOverride>
}

const verdicts: Record<Verdict, string> = { agree: '同意', concerns: '有顾虑', disagree: '不同意', unfinished: '未完成（不等于同意）' }

export function crossReviewsFrom(projection: unknown): Record<string, CrossReviewEntry> {
  return Object.fromEntries(Object.entries(record(record(projection).CrossReview)).filter(([, value]) => {
    const entry = record(value)
    return typeof entry.proposal === 'string' && typeof entry.proposal_hash === 'string' && typeof entry.verdict === 'string'
      && entry.verdict in verdicts && Array.isArray(entry.findings)
  })) as Record<string, CrossReviewEntry>
}

/** One line for the proposal panel; a review of other content is said to be stale. */
export function crossReviewLabel(review: CrossReviewEntry, contentHash: string | undefined): string {
  const who = review.agent ?? review.harness
  const stale = contentHash && contentHash !== review.proposal_hash ? ' · 评审的是旧内容' : ''
  const cost = review.cost_usd === null ? '费用未知' : `$${review.cost_usd.toFixed(2)}`
  return `${verdicts[review.verdict]} · ${who} · ${cost}${stale}`
}

/** FR-78: attempts whose raw output is on this machine only. */
export function archiveSummary(projection: unknown): ArchiveSummary {
  const archives = Object.values(record(record(projection).Artifact)).map(record).filter((entry) => typeof entry.store === 'string')
  return { attempts: archives.length, localOnly: archives.filter((entry) => entry.store === 'local').length }
}

export function archiveLabel(summary: ArchiveSummary): string | null {
  if (!summary.attempts) return null
  return summary.localOnly
    ? `原始输出 · ${summary.localOnly}/${summary.attempts} 次尝试仅本机（未进 Arca）`
    : `原始输出 · ${summary.attempts} 次尝试已入 Arca`
}
