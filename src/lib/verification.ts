import type { ScanReport, VerificationStatus } from './api'

export type CardStatus = VerificationStatus | 'unverified' | 'verifying' | 'cleanup_failed' | 'unverifiable'
export const statusLabels: Record<CardStatus, { symbol: string; label: string }> = {
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
