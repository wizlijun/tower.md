import { expect, test } from 'vitest';
import { decodeInterventionView, decodeInterventionOutcome, deliveryCounts, sameAction } from './interventions';
import { viewFixture, originalFixture, confirmedFixture } from './interventions.fixture';
test('恢复必须携带实际暂停版本与唯一排序范围，不接受错版本视图或重复运行', () => {
  const view = { ...viewFixture(), paused: true, pause_revision: 'pause-1', resume_request: { request: 'resume-1', pause_revision: 'pause-1', runs: ['R-1'] } };
  expect(decodeInterventionView(view)).toBe(view);
  expect(() => decodeInterventionView({ ...view, resume_request: { ...view.resume_request, pause_revision: 'pause-other' } })).toThrow();
  expect(() => decodeInterventionView({ ...view, resume_request: { ...view.resume_request, runs: ['R-1', 'R-1'] } })).toThrow();
  expect(() => decodeInterventionView({ ...view, resume_request: { ...view.resume_request, runs: ['R-2', 'R-1'] } })).toThrow();
});
test('确认回执必须逐值绑定原 UUID 和正文hash，未确定不能携带成功回执', () => {
  const original = originalFixture({ action: 'interject', scope: { kind: 'project' }, text: '保持现有边界' });
  expect(decodeInterventionOutcome(confirmedFixture(), original).state).toBe('confirmed');
  expect(() => decodeInterventionOutcome({ ...confirmedFixture(), request_id: 'other' }, original)).toThrow();
  expect(() => decodeInterventionOutcome({ ...confirmedFixture(), response: { receipt: { ...confirmedFixture().response.receipt, body_hash: 'b'.repeat(64) } } }, original)).toThrow();
  expect(() => decodeInterventionOutcome({ ...confirmedFixture(), state: 'uncertain' }, original)).toThrow();
});
test('提示范围和恢复完整运行范围不被替换，送达不推出已处理', () => {
  expect(sameAction({ action: 'interject', scope: { kind: 'module', module: 'A' }, text: '限制' }, { action: 'interject', scope: { kind: 'module', module: 'B' }, text: '限制' })).toBe(false);
  expect(sameAction({ action: 'confirm_resume', request: 'Q', pause_revision: 'P', runs: ['R-1'] }, { action: 'confirm_resume', request: 'Q', pause_revision: 'P', runs: ['R-1', 'R-2'] })).toBe(false);
  expect(deliveryCounts({ id: 'H', kind: 'prompt', text: '提示', scope: { kind: 'project' }, recipients: [{ run: 'R-1', task: 'T', delivered: true, actionable: false }, { run: 'R-2', task: 'T2', delivered: false, actionable: true }, { run: 'R-3', task: 'T3', delivered: false, actionable: false }] })).toEqual({ delivered: 1, queued: 1, inactive: 1 });
  expect(deliveryCounts({ id: 'H', kind: 'prompt', text: '提示', scope: { kind: 'project' }, recipients: [] })).toEqual({ delivered: 0, queued: 0, inactive: 0 });
});
