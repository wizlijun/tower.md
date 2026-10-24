import { cleanup, fireEvent, render, screen } from '@testing-library/svelte'
import { afterEach, describe, expect, it, vi } from 'vitest'
import FrameTray from './FrameTray.svelte'
import type { FrameTemplate } from '../lib/api'
import { selectValue } from '../test/select'

afterEach(cleanup)

const kanban: FrameTemplate = {
  id: 'kanban', origin: 'starter', path: 'starter:kanban', version: '1.0.0', title: '看板',
  layout: { x: { category: { field: 'state', values: ['待办', '完成'] } }, y: 'order', structure: [] },
  states: ['待办', '完成'], done_states: ['完成'], card_fields: {}, limits: { title_max: 30, body_max: 100 }, default_set_by: 'delegate', problems: [],
}
const broken: FrameTemplate = { id: 'broken', origin: 'installed', path: '.towermd/extensions/broken/towermd-plugin.yaml', version: '1.0.0', problems: ['按状态分栏的模板须声明 states'] }

function props(overrides: Record<string, unknown> = {}) {
  return { templates: [kanban, broken], onCreate: vi.fn(async () => true), onRetry: vi.fn(), onAbandon: vi.fn(), onReload: vi.fn(), ...overrides }
}

describe('the template tray', () => {
  it('places a template only after the person names it and clicks', async () => {
    const input = props()
    render(FrameTray, input)
    expect(screen.getByText(/横向按状态分栏/)).toBeTruthy()
    await fireEvent.click(screen.getByRole('button', { name: /看板/ }))
    expect(input.onCreate).not.toHaveBeenCalled()
    const title = screen.getByLabelText('名称') as HTMLInputElement
    expect(title.value).toBe('看板')
    expect((screen.getByLabelText('由谁推进') as HTMLSelectElement).value).toBe('delegate')
    await fireEvent.input(title, { target: { value: '读论文' } })
    await fireEvent.input(screen.getByLabelText(/规则/), { target: { value: '每读完一篇论文，加一张卡片' } })
    await fireEvent.click(screen.getByRole('button', { name: '放到画布中央' }))
    expect(input.onCreate).toHaveBeenCalledWith({ template: kanban, title: '读论文', rules: '每读完一篇论文，加一张卡片', setBy: 'delegate' })
  })

  it('lets the person bind the cards to core objects when the template has done states', async () => {
    const input = props()
    render(FrameTray, input)
    await fireEvent.click(screen.getByRole('button', { name: /看板/ }))
    await selectValue(screen.getByLabelText('卡片来源'), 'mixed')
    await fireEvent.click(screen.getByRole('button', { name: '放到画布中央' }))
    expect(input.onCreate).toHaveBeenCalledWith({ template: kanban, title: '看板', rules: '', setBy: 'delegate', mode: 'mixed' })
  })

  it('asks a text-source template for a repository file, and offers it for no picked nodes (FR-163)', async () => {
    const plantuml: FrameTemplate = { ...kanban, id: 'plantuml', title: 'PlantUML', layout: { x: 'none', y: 'none', structure: ['nesting', 'relations'] },
      states: ['To Do', 'Done'], done_states: ['Done'], file_types: ['.puml', '.plantuml'] }
    const input = props({ templates: [plantuml] })
    render(FrameTray, input)
    await fireEvent.click(screen.getByRole('button', { name: /PlantUML/ }))
    expect(screen.queryByLabelText('卡片来源')).toBeNull()
    const place = screen.getByRole('button', { name: '放到画布中央' }) as HTMLButtonElement
    expect(place.disabled).toBe(true)
    await fireEvent.input(screen.getByLabelText(/仓库里的文件/), { target: { value: 'docs/plan.md' } })
    expect(screen.getByText(/文件须是/)).toBeTruthy()
    await fireEvent.input(screen.getByLabelText(/仓库里的文件/), { target: { value: 'docs/plan.puml' } })
    await fireEvent.click(screen.getByLabelText(/新建一份空图/))
    await fireEvent.click(place)
    expect(input.onCreate).toHaveBeenCalledWith({ template: plantuml, title: 'PlantUML', rules: '', setBy: 'delegate', source: { path: 'docs/plan.puml', create: true } })
    cleanup()
    render(FrameTray, props({ templates: [plantuml], converting: 2 }))
    expect((screen.getByRole('button', { name: /PlantUML/ }) as HTMLButtonElement).disabled).toBe(true)
  })

  it('shows why a broken template cannot be used and offers no way to place it', async () => {
    render(FrameTray, props())
    const button = screen.getByRole('button', { name: /broken/ }) as HTMLButtonElement
    expect(button.disabled).toBe(true)
    expect(screen.getByText('不能使用：按状态分栏的模板须声明 states')).toBeTruthy()
  })

  it('names a dropped template and blocks an empty name', async () => {
    const input = props({ dropped: 'kanban' })
    render(FrameTray, input)
    await fireEvent.input(screen.getByLabelText('名称'), { target: { value: ' ' } })
    expect(screen.getByText('给容器起个名字。')).toBeTruthy()
    expect((screen.getByRole('button', { name: '放到拖放处' }) as HTMLButtonElement).disabled).toBe(true)
  })

  it('retries an unconfirmed write as it was sent, or lets the person abandon it', async () => {
    const input = props({ pending: { what: '移动容器「读论文」', error: '本机服务请求失败', sending: false } })
    render(FrameTray, input)
    expect(screen.getByText('尚未确认：移动容器「读论文」')).toBeTruthy()
    await fireEvent.click(screen.getByRole('button', { name: '重试原请求' }))
    await fireEvent.click(screen.getByRole('button', { name: '放弃' }))
    expect(input.onRetry).toHaveBeenCalledTimes(1)
    expect(input.onAbandon).toHaveBeenCalledTimes(1)
  })
})
