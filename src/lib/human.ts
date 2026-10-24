import type { PromptScope, TempRuleEntry } from './api'

export interface QuestionCard {
  draft: { id: string; text: string; options: string[]; recommended: number | null; context: string[] }
  binding: { run: string; task: string; module: string }
  targets: { id: string; layer: string }[]
}
export type AnswerDestination = 'temporary' | 'module_decision' | 'process_rule' | 'slow_proposal'
export interface RecordedAnswer {
  input: { hold: string; text: string; remember_as: { kind: AnswerDestination; id?: string } }
  event: string
  actor: string
  run: string
  answered_at: string
  proposal: string | null
  publication?: { proposal: string; source_proposal: string; publication_event: string; approval_event: string; baseline: string; commit: string } | null
}

/**
 * FR-69: active temporary rules as the prompt entry shows them. A global
 * entry shows every rule; a task or module entry shows the project-wide
 * rules and those set on exactly that scope. A rule stops applying at
 * `until` even before its expiry is recorded.
 */
export function activeTempRules(group: unknown, scope: PromptScope, now: number): TempRuleEntry[] {
  if (!group || typeof group !== 'object') return []
  const same = (rule: TempRuleEntry) => scope.kind === 'global' || rule.scope.kind === 'global'
    || JSON.stringify(rule.scope) === JSON.stringify(scope)
  return (Object.values(group) as TempRuleEntry[])
    .filter((rule) => rule && typeof rule.text === 'string' && !rule.expired && Date.parse(rule.until) > now && same(rule))
    .sort((a, b) => Date.parse(a.until) - Date.parse(b.until))
}
