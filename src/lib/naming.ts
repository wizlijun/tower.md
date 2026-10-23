// First-version names (core spec §13, FR-188, NFR-14) for what the canvas
// shows. Object types and `set_by` stay interface names; only labels change.
export type SetBy = 'human' | 'delegate'

export const setByLabels: Record<SetBy, string> = { human: '人定', delegate: '受托者推进' }

const objectTypeLabels: Record<string, string> = {
  Intent: '目标', Module: '模块', Interface: '接口', Acceptance: '完成标准', Baseline: '版本', Decision: '决策',
  Proposal: '提案', Task: '任务', Run: '运行', Evidence: '证据', Hold: '待回应', Gate: '检查', SkillPack: '技能包', Rule: '规则',
}

export function objectTypeLabel(type: string): string {
  return objectTypeLabels[type] ?? type
}
