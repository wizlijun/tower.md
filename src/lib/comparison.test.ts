import { describe, expect, it } from 'vitest'
import type { DraftComparison } from './api'
import { diffLine, diffSummary, proposalInput, proposalKind } from './comparison'

type Result = DraftComparison['comparison']
const result = (objects: Result['objects']): Result => ({ draft_id: 'd-1', baseline: 'towermd/B0', commit: 'c', objects, unchanged: [], unassigned: [] })

describe('draft comparison text', () => {
  it('names every change with a mark and a label, in a fixed order', () => {
    const line = diffLine({ id: 'A-1', object_type: 'Module', title: '引擎', kinds: ['files', 'renamed'], before_title: '内核', files_added: ['a.rs', 'b.rs'], files_removed: ['c.rs'] }, (id) => id)
    expect(line).toBe('引擎（A-1）· ✎ 改名（原名 内核） · ▤ 文件 +2 −1')
    expect(diffLine({ id: 'A-3', object_type: 'Module', title: '新', kinds: ['added', 'split_from'], related: 'A-1' }, () => '引擎'))
      .toBe('新（A-3）· ⇠ 拆分自 引擎 · + 新增')
  })

  it('counts objects per change kind and skips kinds that did not occur', () => {
    const summary = diffSummary(result([
      { id: 'A-1', object_type: 'Module', title: 'a', kinds: ['files'] },
      { id: 'A-2', object_type: 'Module', title: 'b', kinds: ['removed', 'merged_into'], related: 'A-1' },
      { id: 'AC-1', object_type: 'Acceptance', title: 'c', kinds: ['commands'] },
    ]))
    expect(summary.map((entry) => [entry.kind, entry.count])).toEqual([['merged_into', 1], ['removed', 1], ['files', 1], ['commands', 1]])
  })

  it('suggests a proposal kind from the changes but never submits it', () => {
    expect(proposalKind(result([{ id: 'A-2', object_type: 'Module', title: 'b', kinds: ['removed', 'merged_into'] }]))).toBe('merge')
    expect(proposalKind(result([{ id: 'A-3', object_type: 'Module', title: 'n', kinds: ['added', 'split_from'] }]))).toBe('split')
    expect(proposalKind(result([{ id: 'I-1', object_type: 'Interface', title: 'i', kinds: ['endpoints'] }]))).toBe('interface')
    expect(proposalKind(result([{ id: 'AC-1', object_type: 'Acceptance', title: 'c', kinds: ['commands'] }]))).toBe('acceptance')
    expect(proposalKind(result([{ id: 'A-1', object_type: 'Module', title: 'a', kinds: ['files'] }, { id: 'AC-1', object_type: 'Acceptance', title: 'c', kinds: ['commands'] }]))).toBe('module')
    expect(proposalKind(result([]))).toBe('other')
    expect(JSON.parse(proposalInput('d-1', result([])))).toMatchObject({ draft_id: 'd-1', kind: 'other' })
  })
})
