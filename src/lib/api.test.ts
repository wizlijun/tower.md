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
      [() => projectClient.waiveGate!('A', { request_id, object: 'T-1', data: { gate: 'crap', reason: '上游修复前', until: '2026-10-31T00:00:00Z' } }),
        'waive_gate', { request_id, object: 'T-1', data: { gate: 'crap', reason: '上游修复前', until: '2026-10-31T00:00:00Z' } }],
      [() => projectClient.renameWorkKind!('A', { request_id, data: { from: 'a', to: 'b' } }),
        'rename_work_kind', { request_id, data: { from: 'a', to: 'b' } }],
      [() => projectClient.setDelegationLevel!('A', { request_id, data: { config: '*', kind: 'docs', level: 'report_only' } }),
        'set_delegation_level', { request_id, data: { config: '*', kind: 'docs', level: 'report_only' } }],
      [() => projectClient.dismissSignal!('A', { request_id, data: { from: 'ci', kind: 'eng.crap', target: 'proj', mode: 'ack', seen: 'e1' } }),
        'dismiss_signal', { request_id, data: { from: 'ci', kind: 'eng.crap', target: 'proj', mode: 'ack', seen: 'e1' } }],
      [() => projectClient.askAbout!('A', { request_id, data: { target: 'T-1', ask: 'question', text: '为什么？', due: '2026-10-24T00:00:00Z' } }),
        'ask_about', { request_id, data: { target: 'T-1', ask: 'question', text: '为什么？', due: '2026-10-24T00:00:00Z' } }],
      [() => projectClient.withdrawRequest!('A', { request_id, object: 'q-1', data: {} }), 'withdraw_request', { request_id, object: 'q-1', data: {} }],
      [() => projectClient.addIdea!('A', { request_id, text: '做一个周报工具' }), 'add_idea', { request_id, text: '做一个周报工具' }],
      [() => projectClient.promoteIdea!('A', { request_id, idea: 'I-1', as: 'project' }), 'promote_idea', { request_id, idea: 'I-1', as: 'project' }],
      [() => projectClient.addFollowUp!('A', { request_id, text: '回头看看', when: { on: 'done', target: 'T-1' } }), 'add_follow_up', { request_id, text: '回头看看', when: { on: 'done', target: 'T-1' } }],
      [() => projectClient.finishFollowUp!('A', { request_id, follow_up: 'F-1' }), 'finish_follow_up', { request_id, follow_up: 'F-1' }],
      [() => projectClient.seeResurfaced!('A', { request_id, object: 'T-1' }), 'see_resurfaced', { request_id, object: 'T-1' }],
      [() => projectClient.addTempRule!('A', { request_id, text: '先别合并', scope: { kind: 'task', task: 'T-1' }, until: '2026-10-25T09:00:00+08:00' }),
        'add_temp_rule', { request_id, text: '先别合并', scope: { kind: 'task', task: 'T-1' }, until: '2026-10-25T09:00:00+08:00' }],
      [() => projectClient.createFrame!('A', { request_id, id: 'C-1', title: '读论文', template: 'kanban', place: { x: 0, y: 0, width: 880, height: 420 } }),
        'create_frame', { request_id, id: 'C-1', title: '读论文', template: 'kanban', place: { x: 0, y: 0, width: 880, height: 420 } }],
      [() => projectClient.placeFrame!('A', { request_id, frame: 'C-1', place: { x: 40, y: 0, width: 880, height: 420 } }),
        'place_frame', { request_id, frame: 'C-1', place: { x: 40, y: 0, width: 880, height: 420 } }],
      [() => projectClient.writeCard!('A', { request_id, frame: 'C-1', op: { op: 'add', title: '读论文' } }), 'write_card', { request_id, frame: 'C-1', op: { op: 'add', title: '读论文' } }],
      [() => projectClient.resolveFrameItem!('A', { request_id, frame: 'C-1', pending: 7, accept: true }), 'resolve_frame_item', { request_id, frame: 'C-1', pending: 7, accept: true }],
      [() => projectClient.pinCards!('A', { request_id, frame: 'C-1', pins: { 'C-1.2': { x: 24, y: 80 }, 'T-1': null } }), 'pin_cards', { request_id, frame: 'C-1', pins: { 'C-1.2': { x: 24, y: 80 }, 'T-1': null } }],
    ]
    for (const [call, command, input] of calls) {
      await expect(call()).resolves.toEqual(receipt)
      expect(invoke).toHaveBeenLastCalledWith(command, { token: 'A', input })
    }
    expect(invoke).toHaveBeenCalledTimes(22)
  })

  it('asks the native side for a system notification with the title and body only', async () => {
    vi.mocked(invoke).mockResolvedValue(undefined)
    await projectClient.notify!('立即 · 告警', '10-23 09:00 · 项目：构建坏了')
    expect(invoke).toHaveBeenLastCalledWith('notify_attention', { title: '立即 · 告警', body: '10-23 09:00 · 项目：构建坏了' })
  })
})
