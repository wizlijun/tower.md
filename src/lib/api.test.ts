import { beforeEach, describe, expect, it, vi } from 'vitest'

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn(), Channel: class {} }))

import { invoke } from '@tauri-apps/api/core'
import { projectClient, type DelegationReceipt } from './api'

const receipt: DelegationReceipt = { request_id: 'r', event: 'r', seq: 1, status: 'accepted', obj: 'T-1' }

describe('delegation actions', () => {
  beforeEach(() => vi.mocked(invoke).mockReset().mockResolvedValue(receipt))

  it('each human action calls its own native command with the token and the typed body only', async () => {
    const request_id = '01a16296-8923-71e0-8932-c72fe6582f5b'
    const calls: [() => Promise<unknown>, string, unknown][] = [
      [() => projectClient.finalizeDelegation!('A', { request_id, object: 'T-1', data: { brief: { done_when: ['passes'] }, answers: ['JSON'] } }),
        'finalize_delegation', { request_id, object: 'T-1', data: { brief: { done_when: ['passes'] }, answers: ['JSON'] } }],
      [() => projectClient.recordAcceptanceUsed!('A', { request_id, object: 'T-1', data: { criteria: [0] } }),
        'record_acceptance_used', { request_id, object: 'T-1', data: { criteria: [0] } }],
      [() => projectClient.confirmAcceptance!('A', { request_id, object: 'T-1', data: { review_ms: 9, unproven: [] } }),
        'confirm_acceptance', { request_id, object: 'T-1', data: { review_ms: 9, unproven: [] } }],
      [() => projectClient.sendBackAcceptance!('A', { request_id, object: 'T-1', data: { reason: 'empty input', review_ms: 9 } }),
        'send_back_acceptance', { request_id, object: 'T-1', data: { reason: 'empty input', review_ms: 9 } }],
      [() => projectClient.recordRule!('A', { request_id, object: 'rule-1', data: { kind: 'docs', text: 'cite' } }),
        'record_rule', { request_id, object: 'rule-1', data: { kind: 'docs', text: 'cite' } }],
      [() => projectClient.renameWorkKind!('A', { request_id, data: { from: 'a', to: 'b' } }),
        'rename_work_kind', { request_id, data: { from: 'a', to: 'b' } }],
      [() => projectClient.setDelegationLevel!('A', { request_id, data: { config: '*', kind: 'docs', level: 'report_only' } }),
        'set_delegation_level', { request_id, data: { config: '*', kind: 'docs', level: 'report_only' } }],
      [() => projectClient.dismissSignal!('A', { request_id, data: { from: 'ci', kind: 'eng.crap', target: 'proj', mode: 'ack', seen: 'e1' } }),
        'dismiss_signal', { request_id, data: { from: 'ci', kind: 'eng.crap', target: 'proj', mode: 'ack', seen: 'e1' } }],
      [() => projectClient.addIdea!('A', { request_id, text: '做一个周报工具' }), 'add_idea', { request_id, text: '做一个周报工具' }],
      [() => projectClient.promoteIdea!('A', { request_id, idea: 'I-1', as: 'project' }), 'promote_idea', { request_id, idea: 'I-1', as: 'project' }],
    ]
    for (const [call, command, input] of calls) {
      await expect(call()).resolves.toEqual(receipt)
      expect(invoke).toHaveBeenLastCalledWith(command, { token: 'A', input })
    }
    expect(invoke).toHaveBeenCalledTimes(10)
  })
})
