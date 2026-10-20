import { cleanup, fireEvent, render, screen } from '@testing-library/svelte'
import { afterEach, describe, expect, it, vi } from 'vitest'
import TaskDetails from './TaskDetails.svelte'
import type { RunDetails } from '../lib/api'
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
    expect(screen.getByText('<script>显示为原文</script>')).toBeTruthy()
    expect(screen.getByRole('region', { name: '问答与插话原文 R-1' }).querySelector('script')).toBeNull()
    expect(screen.getByText(/当前未采集 harness 原始输出/)).toBeTruthy()
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
