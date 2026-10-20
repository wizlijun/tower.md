import type { BaselineDraft, ScanReport, VerificationStatus } from './api'

export type CardStatus = VerificationStatus | 'unverified' | 'verifying' | 'cleanup_failed' | 'unverifiable' | 'pending'
export const statusLabels: Record<CardStatus, { symbol: string; label: string }> = {
  pending: { symbol: '◷', label: '待重新验证' },
  unverifiable: { symbol: '!', label: '证据不可复核' },
  cleanup_failed: { symbol: '!', label: '清理失败' },
  unverified: { symbol: '○', label: '未验证' },
  verifying: { symbol: '◷', label: '验证中' },
  passed: { symbol: '✓', label: '通过' },
  failed: { symbol: '✕', label: '失败' },
  timed_out: { symbol: '⌛', label: '超时' },
  cancelled: { symbol: '⊘', label: '已取消' },
  no_tests: { symbol: '△', label: '无测试' },
  not_run: { symbol: '—', label: '未运行' },
}

export function moduleStatuses(report: ScanReport | null, running: boolean): Record<string, CardStatus> {
  if (!report) return {}
  return Object.fromEntries(report.modules.map((module) => [
    module.id,
    report.verification && !report.verification.cleanup_complete && !running ? 'cleanup_failed' :
    report.verification?.modules.find((result) => result.module === module.id)?.status
      ?? (running ? 'verifying' : 'unverified'),
  ]))
}

// Verification changes evidence, not the structure or coordinates under review.
export function updateCandidate(previous: ScanReport | null, incoming: ScanReport): ScanReport {
  if (!previous || previous.id !== incoming.id) return incoming
  const nextNodes = new Map(incoming.view.nodes.map((node) => [node.id, node]))
  return {
    ...incoming,
    view: {
      ...previous.view,
      nodes: previous.view.nodes.map((node) => ({
        ...(nextNodes.get(node.id) ?? node),
        x: node.x, y: node.y, width: node.width, height: node.height,
      })),
    },
  }
}

// Keep the original scan immutable while displaying results for reviewed modules.
export function baselineReport(draft: BaselineDraft): ScanReport {
  const verification = draft.review?.verification
  if (!verification) return draft.scan
  return {
    ...draft.scan,
    id: verification.run_id,
    view: draft.view,
    verification: verification.report,
    modules: draft.review!.modules.map((module) => ({
      id: module.id, title: module.title, paths: module.files, files: module.files,
      confidence: 1, rationale: '人工审阅后的模块',
      command_ids: verification.report.modules.find((result) => result.module === module.id)?.command_ids ?? [],
      suggested_task: module.commands.length ? null : '补充模块验证测试',
    })),
  }
}

export function baselineEvidenceError(draft: BaselineDraft): string | null | undefined {
  return draft.review?.verification ? draft.review_evidence_error : draft.evidence_error
}
