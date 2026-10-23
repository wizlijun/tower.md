import { describe, expect, it } from 'vitest'
import { completionLabels, gateStatusLabels, objectTypeLabel, setByLabels } from './naming'

describe('first-version names (FR-188, NFR-14)', () => {
  it('labels who set a card without exposing layers', () => {
    expect(setByLabels).toEqual({ human: '人定', delegate: '受托者推进' })
  })

  it('shows object types by their first-version names and keeps unknown types as written', () => {
    expect(objectTypeLabel('Baseline')).toBe('版本')
    expect(objectTypeLabel('Acceptance')).toBe('完成标准')
    expect(objectTypeLabel('Gate')).toBe('检查')
    expect(objectTypeLabel('Module')).toBe('模块')
    expect(objectTypeLabel('Custom')).toBe('Custom')
  })

  it('names every blocking gate state 检查未通过', () => {
    expect([gateStatusLabels.failed, gateStatusLabels.error, completionLabels.blocked]).toEqual(['检查未通过', '检查未通过', '检查未通过'])
  })
})
