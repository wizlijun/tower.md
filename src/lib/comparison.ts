import type { DiffKind, DraftComparison, ObjectDiff } from './api'

/** Shape and label for each change kind, so the cue never rests on color alone. */
export const DIFF_LABELS: Record<DiffKind, { mark: string; label: string }> = {
  merged_into: { mark: '⇢', label: '合并入' },
  split_from: { mark: '⇠', label: '拆分自' },
  added: { mark: '+', label: '新增' },
  removed: { mark: '−', label: '删除' },
  renamed: { mark: '✎', label: '改名' },
  files: { mark: '▤', label: '文件' },
  endpoints: { mark: '↔', label: '端点' },
  commands: { mark: '▶', label: '验证命令' },
  moved: { mark: '⤧', label: '移动' },
  content: { mark: '¶', label: '内容' },
}
const ORDER = Object.keys(DIFF_LABELS) as DiffKind[]

/** Change kinds with how many objects carry each, in a fixed reading order. */
export function diffSummary(comparison: DraftComparison['comparison']): { kind: DiffKind; mark: string; label: string; count: number }[] {
  return ORDER.map((kind) => ({ kind, ...DIFF_LABELS[kind], count: comparison.objects.filter((object) => object.kinds.includes(kind)).length }))
    .filter((entry) => entry.count > 0)
}

/** One line per changed object: what it is and every way it changed. */
export function diffLine(object: ObjectDiff, titleOf: (id: string) => string): string {
  const parts = ORDER.filter((kind) => object.kinds.includes(kind)).map((kind) => {
    const { mark, label } = DIFF_LABELS[kind]
    if (kind === 'renamed' && object.before_title) return `${mark} ${label}（原名 ${object.before_title}）`
    if ((kind === 'merged_into' || kind === 'split_from') && object.related) return `${mark} ${label} ${titleOf(object.related)}`
    if (kind === 'files') {
      const added = object.files_added?.length ?? 0
      const removed = object.files_removed?.length ?? 0
      return `${mark} ${label} +${added} −${removed}`
    }
    return `${mark} ${label}`
  })
  return `${object.title}（${object.id}）· ${parts.join(' · ')}`
}

/** The proposal kind the changes suggest; the session may choose another. */
export function proposalKind(comparison: DraftComparison['comparison']): 'merge' | 'split' | 'interface' | 'acceptance' | 'module' | 'other' {
  const has = (kind: DiffKind, type?: string) => comparison.objects.some((object) => object.kinds.includes(kind) && (!type || object.object_type === type))
  if (has('merged_into')) return 'merge'
  if (has('split_from')) return 'split'
  if (comparison.objects.length && comparison.objects.every((object) => object.object_type === 'Interface')) return 'interface'
  if (comparison.objects.length && comparison.objects.every((object) => object.object_type === 'Acceptance')) return 'acceptance'
  if (has('added', 'Module') || has('removed', 'Module') || has('files', 'Module')) return 'module'
  return 'other'
}

/** The proposal input a session submits with `towermd propose`; the App never submits it. */
export function proposalInput(draftId: string, comparison: DraftComparison['comparison']): string {
  return JSON.stringify({ draft_id: draftId, kind: proposalKind(comparison), reason: '…', alternatives: ['…'], recommendation: '…' }, null, 2)
}
