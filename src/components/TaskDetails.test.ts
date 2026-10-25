import { cleanup, fireEvent, render, screen } from '@testing-library/svelte'
import { afterEach, describe, expect, it, vi } from 'vitest'
import TaskDetails from './TaskDetails.svelte'
import type { RunDetails } from '../lib/api'
import type { DelegationControls } from '../lib/delegation'
import { delegationView } from '../test/delegation'
import type { RuntimeRun, RuntimeTask } from '../lib/runtime'

afterEach(cleanup)
const task: RuntimeTask = {
  input: { id: 'T-1', title: '实现任务', module: 'A-1', touches: [], allowed_paths: ['src/**'], depends_on: [], acceptance: ['测试通过'] },
  state: 'verifying', baseline: 'towermd/B0', needs_review: false, active_run: 'R-1',
}
const run: RuntimeRun = {
  binding: { id: 'R-1', host: 'local', worktree: '/repo/task', branch: 't/T-1' },
  task: 'T-1', state: 'active', cost_usd: null,
}
const details: RunDetails = {
  run_id: 'R-1', task_id: 'T-1', seq: 8, harness_output: null,
  messages: [
    { kind: 'question', id: 'H-1', text: '选择 JSON 还是 YAML？', at: '2026-10-21T11:00:00Z', actor: 'process:towermd-cli' },
    { kind: 'answer', id: 'answer-original', text: '选择 JSON\n保留所有字段', at: '2026-10-21T11:01:00Z', actor: 'human:local' },
    { kind: 'answer', id: 'relay-original', text: '人在会话中选择了 CSV', at: '2026-10-21T11:01:30Z', actor: 'process:towermd-cli' },
    { kind: 'prompt', id: 'P-1', text: '<script>显示为原文</script>', at: '2026-10-21T11:02:00Z', actor: 'human:local' },
  ],
  events: [{ eid: 'original-finish', seq: 8, at: '2026-10-21T12:00:00Z', actor: 'process:towermd-verifier', kind: 'task.finish.completed', object: 'E-1', stale: false, evidence_id: 'E-1' }],
  evidence: [{ id: 'E-1', completion_eid: 'original-finish', revision: 'a'.repeat(40), tree: 'b'.repeat(40), passed: false, report_sha256: 'c'.repeat(64),
    report: { actor: 'process:towermd-verifier', revision: 'a'.repeat(40), tree: 'b'.repeat(40), started_at: '2026-10-21T12:00:00Z', finished_at: '2026-10-21T12:00:01Z', cleanup_complete: true,
      modules: [{ module: 'A-1', status: 'failed', command_ids: ['cmd-1'], shared: false }],
      commands: [{ id: 'cmd-1', directory: '', argv: ['sh', '-c', 'test -f missing'], modules: ['A-1'], status: 'failed', exit_code: 1, duration_ms: 25, log: null, log_sha256: null, log_excerpt: 'real frozen failure', log_truncated: false, error: null }],
    },
  }],
}

describe('run details read only evidence', () => {
  it('requests pause and report through explicit prompts without changing task facts', async () => {
    const prompt = vi.fn()
    render(TaskDetails, { tasks: [task], runs: [run], positions: new Map(), scope: 'A-1', onPrompt: prompt })
    await fireEvent.click(screen.getByRole('button', { name: /实现任务/ }))
    await fireEvent.click(screen.getByRole('button', { name: '请求暂停任务' }))
    expect(prompt).toHaveBeenLastCalledWith('T-1', expect.stringContaining('下一个安全检查点'))
    await fireEvent.click(screen.getByRole('button', { name: '要求汇报' }))
    expect(prompt).toHaveBeenLastCalledWith('T-1', expect.stringContaining('实际验证证据'))
    expect(task.state).toBe('verifying')
    expect(run.state).toBe('active')
  })

  it('offers reassign only for the active run and sends the run the person saw', async () => {
    const delegation: DelegationControls = { view: delegationView(), disabled: false, busy: false, pending: null, error: null, result: null, ruleOffer: null,
      onAction: vi.fn(), onRetry: vi.fn(), onDismissRule: vi.fn() }
    const old: RuntimeRun = { ...run, binding: { ...run.binding, id: 'R-0' }, state: 'released' }
    render(TaskDetails, { tasks: [task], runs: [old, run], positions: new Map(), scope: 'A-1', delegation })
    await fireEvent.click(screen.getByRole('button', { name: /实现任务/ }))
    expect(screen.queryByRole('button', { name: /结束运行 R-0/ })).toBeNull()
    await fireEvent.click(screen.getByRole('button', { name: '重派 · 结束运行 R-1，任务回到就绪' }))
    expect(delegation.onAction).toHaveBeenCalledWith({ kind: 'reassign', object: 'T-1', data: { run: 'R-1' } })
    expect(task.state).toBe('verifying')
  })

  it('pauses, resumes, retries and rolls back one task only through the person\'s actions', async () => {
    const delegation: DelegationControls = { view: delegationView(), disabled: false, busy: false, pending: null, error: null, result: null, ruleOffer: null,
      onAction: vi.fn(), onRetry: vi.fn(), onDismissRule: vi.fn() }
    const ready: RuntimeTask = { ...task, input: { ...task.input, id: 'T-2', title: '待派发任务' }, state: 'ready', active_run: null }
    const paused: RuntimeTask = { ...task, input: { ...task.input, id: 'T-3', title: '已暂停任务' }, state: 'paused', active_run: null }
    const merged: RuntimeTask = { ...task, input: { ...task.input, id: 'T-4', title: '已合入任务' }, state: 'merged', active_run: null }
    const merges = { 'T-4': [{ commit: 'a'.repeat(40), rollback: 'none' as const }, { commit: 'b'.repeat(40), rollback: 'done' as const }] }
    render(TaskDetails, { tasks: [ready, paused, merged], runs: [], positions: new Map(), scope: 'A-1', delegation, merges, onPrompt: vi.fn() })
    await fireEvent.click(screen.getByRole('button', { name: /待派发任务/ }))
    expect(screen.queryByRole('button', { name: '请求暂停任务' })).toBeNull()
    await fireEvent.click(screen.getByRole('button', { name: /^暂停任务/ }))
    await fireEvent.click(screen.getByRole('button', { name: /^重试/ }))
    await fireEvent.click(screen.getByRole('button', { name: /已暂停任务/ }))
    expect(screen.queryByRole('button', { name: /^重试/ })).toBeNull()
    await fireEvent.click(screen.getByRole('button', { name: '继续任务' }))
    await fireEvent.click(screen.getByRole('button', { name: /已合入任务/ }))
    expect(screen.queryByRole('button', { name: /^暂停任务/ })).toBeNull()
    expect(screen.getByText(/合入 bbbbbbbbbb 已回滚/)).toBeTruthy()
    await fireEvent.click(screen.getByRole('button', { name: /回滚这次合入 .* aaaaaaaaaa/ }))
    expect(vi.mocked(delegation.onAction).mock.calls.map(([intent]) => intent)).toEqual([
      { kind: 'pause', object: 'T-2', data: {} }, { kind: 'retry', object: 'T-2', data: {} },
      { kind: 'resume', object: 'T-3', data: {} }, { kind: 'rollback', object: 'T-4', data: { commit: 'a'.repeat(40) } },
    ])
  })

  it('loads on demand and displays original timeline, frozen failure and unknown harness output', async () => {
    const load = vi.fn()
    render(TaskDetails, { tasks: [task], runs: [run], positions: new Map(), scope: 'A-1', onRunDetails: load, runDetails: { 'R-1': details } })
    expect(load).not.toHaveBeenCalled()
    expect(screen.queryByText('real frozen failure')).toBeNull()
    await fireEvent.click(screen.getByRole('button', { name: /实现任务/ }))
    await fireEvent.click(screen.getByRole('button', { name: '查看时间线与验证证据 · R-1' }))
    expect(load).toHaveBeenCalledWith('R-1')
    expect(screen.getByRole('region', { name: '运行时间线 R-1' })).toBeTruthy()
    expect(screen.getByText('real frozen failure')).toBeTruthy()
    expect(screen.getByText('选择 JSON 还是 YAML？')).toBeTruthy()
    expect(screen.getByText(/选择 JSON.*保留所有字段/)).toBeTruthy()
    expect(screen.getByRole('heading', { name: '回答 · answer-original' })).toBeTruthy()
    expect(screen.getByRole('heading', { name: '转述回答（仅本次） · relay-original' })).toBeTruthy()
    expect(screen.getByText('<script>显示为原文</script>')).toBeTruthy()
    expect(screen.getByRole('region', { name: '问答与插话原文 R-1' }).querySelector('script')).toBeNull()
    expect(screen.getByText(/当前未采集受托者的原始输出/)).toBeTruthy()
    expect(screen.getByText(/验证未通过.*不等同于任务已合并/)).toBeTruthy()
    expect(screen.getByText('未知（未报告）')).toBeTruthy()
  })

  it('does not show cached evidence belonging to a different task', async () => {
    render(TaskDetails, { tasks: [task], runs: [run], positions: new Map(), scope: 'A-1', onRunDetails: vi.fn(), runDetails: { 'R-1': { ...details, task_id: 'T-other' } }, detailsError: '无法读取原件' })
    await fireEvent.click(screen.getByRole('button', { name: /实现任务/ }))
    await fireEvent.click(screen.getByRole('button', { name: '查看时间线与验证证据 · R-1' }))
    expect(screen.getByRole('alert').textContent).toContain('无法读取原件')
    expect(screen.queryByText('real frozen failure')).toBeNull()
  })
})
