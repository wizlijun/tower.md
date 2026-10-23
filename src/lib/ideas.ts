import type { IdeaEntry } from './api'

/** The same limit the daemon checks (`ideas::TEXT_CHARS`). */
export const IDEA_TEXT_CHARS = 200
/** FR-183: promotions share the attention budget of the first screen. */
export const PROMOTION_BUDGET = 3

/** Ideas from the projection: open ones first, newest first within each half. */
export function ideasFrom(projection: unknown): IdeaEntry[] {
  const group = (projection as Record<string, unknown> | null | undefined)?.Idea
  if (!group || typeof group !== 'object') return []
  return Object.values(group as Record<string, IdeaEntry>)
    .filter((idea) => typeof idea?.id === 'string' && typeof idea.text === 'string' && Number.isSafeInteger(idea.added?.seq))
    .sort((a, b) => Number(!!a.promoted) - Number(!!b.promoted) || b.added.seq - a.added.seq)
}

/** A rejection the person can fix before sending; `null` when the sentence is fine. */
export function ideaTextError(text: string): string | null {
  const value = text.trim()
  if (!value) return '先写一句话。'
  // eslint-disable-next-line no-control-regex
  if (/[\u0000-\u001f\u007f-\u009f]/.test(value)) return '一句话里不能有换行或控制字符。'
  if ([...value].length > IDEA_TEXT_CHARS) return `一句话最多 ${IDEA_TEXT_CHARS} 个字。`
  return null
}

/** FR-183: past the budget, say how many promoted things still wait for someone. */
export function followUpHint(promotedThisVisit: number): string | null {
  const waiting = promotedThisVisit - PROMOTION_BUDGET
  return waiting > 0 ? `另有 ${waiting} 个已立项的事还没有人跟进` : null
}

export function promotionLabel(idea: IdeaEntry): string | null {
  if (!idea.promoted) return null
  if (idea.promoted.as === 'project') return '已立为项目目标'
  if (idea.promoted.as === 'task') return `已立为任务 ${idea.promoted.target ?? ''}`.trim()
  return `已立项（${idea.promoted.as}）`
}

export function signerLabel(actor: string): string {
  return actor.startsWith('human:') ? '你记下的' : actor === 'process:towermd-cli' ? '命令行记下的' : `${actor} 记下的`
}
