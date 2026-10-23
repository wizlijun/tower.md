import { describe, expect, it } from 'vitest'
import { followUpHint, ideaTextError, ideasFrom, promotionLabel, signerLabel } from './ideas'

const mark = (seq: number, actor = 'human:local') => ({ at: 't', eid: `e${seq}`, seq, actor })

describe('ideas', () => {
  it('reads only well-formed ideas, open ones first and newest first', () => {
    const projection = { Idea: {
      a: { id: 'a', text: '旧的', added: mark(1) },
      b: { id: 'b', text: '已立项', added: mark(3), promoted: { as: 'project', event: mark(4) } },
      c: { id: 'c', text: '新的', added: mark(2) },
      broken: { id: 'broken', added: mark(5) },
    }, Task: { a: {} } }
    expect(ideasFrom(projection).map((idea) => idea.id)).toEqual(['c', 'a', 'b'])
    expect(ideasFrom({})).toEqual([])
    expect(ideasFrom(null)).toEqual([])
  })

  it('checks a sentence the way the daemon does', () => {
    expect(ideaTextError('  ')).toBe('先写一句话。')
    expect(ideaTextError('一行\n两行')).toMatch(/换行/)
    expect(ideaTextError('长'.repeat(200))).toBeNull()
    expect(ideaTextError('长'.repeat(201))).toMatch(/200/)
    expect(ideaTextError('做一个周报工具')).toBeNull()
  })

  it('hints only past the attention budget of three', () => {
    expect(followUpHint(3)).toBeNull()
    expect(followUpHint(5)).toBe('另有 2 个已立项的事还没有人跟进')
  })

  it('labels promotion and signer in words, not only by style', () => {
    expect(promotionLabel({ id: 'a', text: 'x', added: mark(1) })).toBeNull()
    expect(promotionLabel({ id: 'a', text: 'x', added: mark(1), promoted: { as: 'task', target: 'T-1', event: mark(2) } })).toBe('已立为任务 T-1')
    expect(signerLabel('process:towermd-cli')).toBe('命令行记下的')
    expect(signerLabel('claude-code/2.1')).toBe('claude-code/2.1 记下的')
  })
})
