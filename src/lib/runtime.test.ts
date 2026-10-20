import { describe, expect, it } from 'vitest'
import { costLabel, moduleActivity, runtimeProjection, taskGraph, type RuntimeTask } from './runtime'

function task(id: string, state: RuntimeTask['state'] = 'ready'): RuntimeTask {
  return { input: { id, title: id, module: 'A', touches: [], allowed_paths: ['src/**'], depends_on: [], acceptance: [] },
    state, baseline: 'towermd/B0', needs_review: false, active_run: null }
}

describe('real runtime canvas projection', () => {
  it('keeps existing task coordinates when state, ordering or dependency changes', () => {
    const positions = new Map<string, { x: number; y: number }>()
    const initial = taskGraph([task('T-2'), task('T-3')], positions, 'A')
    const changed = task('T-3', 'running')
    changed.input.depends_on = ['T-2']
    const updated = taskGraph([task('T-1'), changed, task('T-2', 'merged')], positions, 'A')
    for (const old of initial.nodes) {
      const node = updated.nodes.find((node) => node.id === old.id)!
      expect([node.x, node.y, node.width, node.height]).toEqual([old.x, old.y, old.width, old.height])
    }
    expect(updated.edges).toHaveLength(1)
    expect(updated.edges[0]).toMatchObject({ from_node: 'T-2', to_node: 'T-3' })
    expect(updated.nodes.find((node) => node.id === 'T-3')!.content).toBe('执行中')
    const reopened = taskGraph([task('T-2')], positions, 'A')
    expect(reopened.nodes[0].x).toBe(initial.nodes[0].x)
  })

  it('preserves null and missing costs as unknown and limits attention to three facts', () => {
    expect(costLabel(null)).toBe('未知（未报告）')
    expect(costLabel(undefined)).toBe('未知（未报告）')
    expect(costLabel(0)).toBe('$0.0000')
    const state = runtimeProjection({ status: 'ready', projection: {
      Task: { 'T-1': task('T-1', 'running') }, Run: {},
      Alert: Object.fromEntries([1, 2, 3, 4].map((n) => [`alert-${n}`, { data: { reason: `关注 ${n}` } }])),
    } })
    expect(state.attention).toHaveLength(3)
    expect(moduleActivity(state.tasks).A).toMatchObject({ label: '1 个任务 · 1 运行', symbol: '▶' })
    expect(runtimeProjection({ status: 'needs_recovery', projection: { Task: { 'T-1': task('T-1') } } }).tasks).toEqual([])
  })

  it('uses accepted answer and per-run delivery facts, not prompt submission as delivery', () => {
    const projection = {
      Hold: { H: { draft: { id: 'H', text: '如何实现？', options: [], recommended: null, context: [] },
        binding: { run: 'R-1', task: 'T-1', module: 'A' }, targets: [] } },
      Answer: { H: { input: { hold: 'H', text: '沿用现有实现', remember_as: { kind: 'temporary' } }, event: 'answer-id', proposal: null } },
      Prompt: { P: { id: 'P', scope: { kind: 'global' }, text: '先测试', recipients: [{ run: 'R-1', task: 'T-1', module: 'A' }, { run: 'R-2', task: 'T-2', module: 'A' }] } },
      Delivery: { D: { delivery: { run: 'R-1', message: { kind: 'prompt', id: 'P' } } } },
    }
    const result = runtimeProjection({ status: 'ready', projection })
    expect(result.attention).toEqual([])
    expect(result.questions).toHaveLength(1)
    expect(result.prompts[0].delivered).toEqual(['R-1'])
    expect(result.prompts[0].recipients).toHaveLength(2)
  })
})
