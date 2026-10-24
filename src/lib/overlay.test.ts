import { describe, expect, it } from 'vitest'
import type { CanvasNode, DraftComparison } from './api'
import { draftMarks, ghostNodes, layoutMoves, proposalMarks, relationEnds, sameMark, tweenAt } from './overlay'

function card(id: string, x: number, extra: Partial<CanvasNode> = {}): CanvasNode {
  return { id: `n-${id}`, object_id: id, kind: 'text', title: `标题 ${id}`, content: '', x, y: 0, width: 200, height: 100, ...extra }
}
function comparison(objects: DraftComparison['comparison']['objects']): DraftComparison['comparison'] {
  return { draft_id: 'draft-1', baseline: 'B-1', commit: 'abc', objects, unchanged: [], unassigned: [] }
}

describe('ghost layer (FR-46)', () => {
  it('keeps removed and moved or resized cards of the old version, never groups or unchanged ones', () => {
    const before = [card('a', 0), card('b', 100), card('c', 200), card('d', 300), card('g', 0, { kind: 'group' })]
    const after = [card('a', 0), card('b', 150), card('c', 200, { height: 140 }), card('g', 50, { kind: 'group' })]
    expect(ghostNodes(before, after).map(({ node, why }) => [node.object_id, why])).toEqual([['b', 'moved'], ['c', 'moved'], ['d', 'removed']])
    expect(ghostNodes(after, after)).toEqual([])
  })

  it('glides moved cards from the old place to the new, eased, and clamps outside 0–1', () => {
    const moves = layoutMoves([card('a', 0), card('b', 100), card('gone', 0)], [card('a', 0, { y: 0, height: 300 }), card('b', 300), card('new', 9)])
    expect([...moves.keys()]).toEqual(['n-b'])
    const move = moves.get('n-b')!
    expect(tweenAt(move, 0)).toEqual({ x: 100, y: 0 })
    expect(tweenAt(move, 0.5)).toEqual({ x: 200, y: 0 })
    expect(tweenAt(move, 1)).toEqual({ x: 300, y: 0 })
    expect(tweenAt(move, 2)).toEqual({ x: 300, y: 0 })
    expect(tweenAt(move, -1)).toEqual({ x: 100, y: 0 })
    expect(tweenAt(move, 0.25).x).toBeLessThan(150)
  })
})

describe('proposed changes and relation ends on a draft (FR-45, FR-36, FR-35)', () => {
  it('reads relation ends from the frontmatter only', () => {
    expect(relationEnds('---\ntype: Interface\nfrom: api\nto: "store"\n---\nfrom: body')).toEqual(['api', 'store'])
    expect(relationEnds('---\ntype: Acceptance\nmodule: api\n---\n')).toEqual(['api'])
    expect(relationEnds('from: api')).toEqual([])
  })

  it('dashes changed objects with what changed, leaves removed ones to the ghost layer, and links merges', () => {
    const after = [card('api', 0), card('store', 300), card('new', 600)]
    const before = [card('api', 0), card('store', 300), card('old', 900)]
    const marks = draftMarks(comparison([
      { id: 'new', object_type: 'Module', title: '新', kinds: ['added'] },
      { id: 'store', object_type: 'Module', title: '存储', kinds: ['content', 'renamed'] },
      { id: 'old', object_type: 'Module', title: '旧', kinds: ['removed', 'merged_into'], related: 'store' },
    ]), after, before)
    expect(marks.new).toEqual({ proposed: '+ 新增' })
    expect(marks.store).toEqual({ proposed: '✎ 改名 · ¶ 内容', relation: ['标题 old'] })
    expect(marks.old).toBeUndefined()
    expect(marks.api).toBeUndefined()
  })

  it('lights both modules of a split and both ends of a moved interface, titled by the other end', () => {
    const iface = card('if-1', 900, { object_type: 'Interface', content: '---\ntype: Interface\nfrom: api\nto: store\n---\n' })
    const after = [card('api', 0), card('store', 300), card('part', 600), iface]
    const before = [card('api', 0), card('store', 300), { ...iface, content: '---\ntype: Interface\nfrom: api\nto: cache\n---\n' }, card('cache', 1200)]
    const marks = draftMarks(comparison([
      { id: 'part', object_type: 'Module', title: '拆出', kinds: ['added', 'split_from'], related: 'api' },
      { id: 'if-1', object_type: 'Interface', title: '接口', kinds: ['endpoints'] },
    ]), after, before)
    expect(marks.part).toEqual({ proposed: '⇠ 拆分自 · + 新增', relation: ['标题 api'] })
    expect(marks.api?.relation).toEqual(['标题 part', '标题 store', '标题 cache'])
    expect(marks.store?.relation).toEqual(['标题 api', '标题 cache'])
    expect(marks['if-1']).toEqual({ proposed: '↔ 端点' })
    expect(marks.cache).toBeUndefined()
  })

  it('treats an acceptance with a new module as one end of the relation itself', () => {
    const after = [card('acc', 0, { object_type: 'Acceptance', content: '---\ntype: Acceptance\nmodule: api\n---\n' }), card('api', 300)]
    const marks = draftMarks(comparison([{ id: 'acc', object_type: 'Acceptance', title: '验收', kinds: ['endpoints'] }]), after, [])
    expect(marks.acc?.relation).toEqual(['标题 api'])
    expect(marks.api?.relation).toEqual(['标题 acc'])
  })
})

describe('proposals on the approved version (FR-45, FR-36)', () => {
  const nodes = [card('api', 0), card('store', 300), card('if-1', 600)]
  const entry = (status: string, kind: string, modules: string[], interfaces: string[] = []) =>
    ({ status, proposal: { kind, impact: { modules, interfaces } } })

  it('dashes what waiting proposals touch and draws nothing for decided ones', () => {
    const marks = proposalMarks({
      'P-1': entry('submitted', 'scope', ['api']),
      'P-2': entry('draft', 'interface', ['api', 'store'], ['if-1']),
      'P-3': entry('approved', 'merge', ['store', 'api']),
      'P-4': entry('rejected', 'module', ['if-1']),
      'P-5': entry('superseded', 'module', ['if-1']),
    }, nodes)
    expect(marks.api).toEqual({ proposed: '提案待审阅；提案待补全', relation: ['标题 store'] })
    expect(marks.store).toEqual({ proposed: '提案待补全', relation: ['标题 api'] })
    expect(marks['if-1']).toEqual({ proposed: '提案待补全' })
    expect(proposalMarks({ 'P-3': entry('approved', 'merge', ['store', 'api']) }, nodes)).toEqual({})
  })

  it('tolerates a missing or odd projection and ignores objects not on the canvas', () => {
    expect(proposalMarks(undefined, nodes)).toEqual({})
    expect(proposalMarks({ x: 1, y: { status: 'submitted', proposal: { impact: { modules: 'api' } } } }, nodes)).toEqual({})
    expect(proposalMarks({ 'P-1': entry('submitted', 'merge', ['api', 'elsewhere']) }, nodes)).toEqual({ api: { proposed: '提案待审阅', relation: ['elsewhere'] } })
  })

  it('compares marks by value', () => {
    expect(sameMark(undefined, {})).toBe(true)
    expect(sameMark({ proposed: 'a', relation: ['x'] }, { proposed: 'a', relation: ['x'] })).toBe(true)
    expect(sameMark({ proposed: 'a' }, { proposed: 'b' })).toBe(false)
    expect(sameMark({ relation: ['x'] }, { relation: ['x', 'y'] })).toBe(false)
  })
})
