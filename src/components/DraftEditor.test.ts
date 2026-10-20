import { cleanup, fireEvent, render, screen } from '@testing-library/svelte'
import { afterEach, describe, expect, it, vi } from 'vitest'
import type { BaselineDraft, CanvasNode } from '../lib/api'
import DraftEditor from './DraftEditor.svelte'

afterEach(cleanup)

const node: CanvasNode = { id: 'node-a', object_id: 'module-a', object_type: 'Module', kind: 'file', title: '模块甲', content: '', x: 0, y: 0, width: 260, height: 180 }
function draft(): BaselineDraft {
  return {
    id: `d-${'a'.repeat(64)}`, producer: 'agent/1', self_reported: true, created_at: '2026-10-21T01:00:00Z',
    submission: { scan_id: 'scan', project_id: 'project', source_revision: 'a'.repeat(40), intents: [], modules: [], interfaces: [], acceptances: [], risks: [], questions: [] },
    scan: { id: 'scan', project_id: 'project', reference: 'main', revision: 'a'.repeat(40), tree: 'b'.repeat(40), view: { title: '', reference: 'main', revision: 'a'.repeat(40), docs_dir: 'docs/tower', diagnostics: [], nodes: [], edges: [], unplaced: [] }, modules: [], commands: [], unassigned: [], source_file_count: 3, assigned_file_count: 3, coverage: 1, diagnostics: [], hotspots: [], couplings: [] },
    view: { title: '', reference: 'main', revision: 'a'.repeat(40), docs_dir: 'docs/tower', diagnostics: [], nodes: [node], edges: [], unplaced: [] },
    review: { base_id: `d-${'b'.repeat(64)}`, actor: 'human:local', actions: [], pending_modules: [], confirmed_modules: [], modules: [
      { id: 'module-a', title: '模块甲', files: ['src/a.ts', 'src/b.ts'], commands: [], verification_pending: false, commands_confirmed: false },
      { id: 'module-b', title: '模块乙', files: ['other/c.ts'], commands: [], verification_pending: false, commands_confirmed: false },
    ] },
  }
}

describe('draft structural editing', () => {
  it('submits a strict source-file subset for split, and blocks empty or whole-module selection', async () => {
    const onSubmit = vi.fn()
    render(DraftEditor, { draft: draft(), node, disabled: false, onSubmit })
    await fireEvent.click(screen.getByText('拆分模块'))
    await fireEvent.input(screen.getByLabelText('新模块名称'), { target: { value: '新模块' } })
    const submit = screen.getByRole('button', { name: '拆分选中文件' }) as HTMLButtonElement
    expect(submit.disabled).toBe(true)
    await fireEvent.click(screen.getByLabelText('src/a.ts'))
    expect(submit.disabled).toBe(false)
    await fireEvent.click(submit)
    expect(onSubmit).toHaveBeenCalledWith({ kind: 'split', module_id: 'module-a', new_id: expect.stringMatching(/^mod-/), title: '新模块', files: ['src/a.ts'] })
    await fireEvent.click(screen.getByLabelText('src/b.ts'))
    expect(submit.disabled).toBe(true)
  })

  it('merges into the selected module and draws an explicit interface without inventing identities', async () => {
    const onSubmit = vi.fn()
    render(DraftEditor, { draft: draft(), node, disabled: false, onSubmit })
    await fireEvent.click(screen.getByText('合并模块'))
    await fireEvent.change(screen.getByLabelText('合入的模块'), { target: { value: 'module-b' } })
    await fireEvent.click(screen.getByRole('button', { name: '合并到当前模块' }))
    expect(onSubmit).toHaveBeenCalledWith({ kind: 'merge', module_ids: ['module-a', 'module-b'], target_id: 'module-a', title: '模块甲' })
    await fireEvent.click(screen.getByText('补充接口'))
    await fireEvent.change(screen.getByLabelText('目标模块'), { target: { value: 'module-b' } })
    await fireEvent.input(screen.getByLabelText('接口名称'), { target: { value: '读取结果' } })
    await fireEvent.input(screen.getByLabelText('接口约定'), { target: { value: '返回计算结果或明确错误。' } })
    await fireEvent.click(screen.getByRole('button', { name: '保存接口' }))
    expect(onSubmit).toHaveBeenLastCalledWith({ kind: 'interface', id: expect.stringMatching(/^iface-/), from: 'module-a', to: 'module-b', title: '读取结果', contract: '返回计算结果或明确错误。' })
  })
})
