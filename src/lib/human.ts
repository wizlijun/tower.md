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
}
