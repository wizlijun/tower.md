<script lang="ts">
  import { untrack } from 'svelte'
  import type { BaselineDraft, CanvasNode, ReviewAction, ReviewCommand, ReviewModule } from '../lib/api'
  let { draft, node, disabled, onSubmit }: {
    draft: BaselineDraft; node: CanvasNode; disabled: boolean; onSubmit: (action: ReviewAction) => void
  } = $props()
  let modules: ReviewModule[] = $derived(draft.review?.modules ?? draft.scan.modules.map((entry) => ({
    id: entry.id, title: draft.view.nodes.find((item) => (item.object_id || item.id) === entry.id)?.title ?? entry.title,
    files: entry.files, commands: draft.scan.commands.filter((command) => command.modules.includes(entry.id)).map(({ directory, command }) => ({ directory, command })),
    verification_pending: false, commands_confirmed: false,
  })))
  let module = $derived(modules.find((entry) => entry.id === (node.object_id || node.id)))
  let title = $state(untrack(() => node.title))
  let mergeId = $state('')
  let splitTitle = $state('')
  let splitFiles = $state<string[]>([])
  const splitId = `mod-${crypto.randomUUID()}`
  const interfaceId = `iface-${crypto.randomUUID()}`
  let interfaceTo = $state('')
  let interfaceTitle = $state('')
  let contract = $state('')
  let commands = $state<ReviewCommand[]>(untrack(() => module?.commands.map((command) => ({ ...command })) ?? []))

  function send(action: ReviewAction) { onSubmit(action) }
  function toggleFile(file: string, checked: boolean) {
    splitFiles = checked ? [...splitFiles, file] : splitFiles.filter((entry) => entry !== file)
  }
</script>

<p class="panel-description">更改保存为未批准草稿。拖动卡片顶部的手柄可保存位置。调整模块范围或命令后，需要重新验证。</p>
<fieldset class="draft-edit-fields" {disabled}>
  <form class="draft-edit-form" onsubmit={(event) => { event.preventDefault(); send({ kind: 'rename', node_id: node.id, title }) }}>
    <label>名称<input bind:value={title} required /></label>
    <button type="submit">保存名称</button>
  </form>
  {#if node.object_type?.toLowerCase() === 'interface'}
    <p class="panel-description">删除接口会同时移除其关系线，修改只进入草稿。</p>
    <button onclick={() => send({ kind: 'remove_interface', id: node.object_id || node.id })}>删除此接口</button>
  {/if}
  {#if module}
    <details>
      <summary>验证命令{module.commands_confirmed ? ' · 已确认' : ' · 待确认'}</summary>
      <p class="panel-description">确认只记录命令，不会执行。空列表明确表示当前没有测试命令。</p>
      <form class="draft-edit-form" onsubmit={(event) => { event.preventDefault(); send({ kind: 'commands', module_id: module!.id, commands }) }}>
        {#each commands as command, index}
          <div class="draft-command-row">
            <label>执行目录<input aria-label={`执行目录 ${index + 1}`} bind:value={command.directory} placeholder="." /></label>
            <label>命令<input aria-label={`验证命令 ${index + 1}`} bind:value={command.command} required /></label>
            <button type="button" onclick={() => { commands = commands.filter((_, position) => position !== index) }}>移除命令 {index + 1}</button>
          </div>
        {/each}
        <div class="button-row"><button type="button" onclick={() => { commands = [...commands, { directory: '.', command: '' }] }}>添加命令</button><button type="submit">确认验证命令</button></div>
      </form>
    </details>
    <details>
      <summary>合并模块</summary>
      <p class="panel-description">将另一个模块的文件、命令和引用合入当前模块；合并后须重新验证。</p>
      <form class="draft-edit-form" onsubmit={(event) => { event.preventDefault(); send({ kind: 'merge', module_ids: [module!.id, mergeId], target_id: module!.id, title }) }}>
        <label>合入的模块<select value={mergeId} onchange={(event) => { mergeId = event.currentTarget.value }} required>
          <option value="">选择模块</option>{#each modules.filter((entry) => entry.id !== module?.id) as entry}<option value={entry.id}>{entry.title}</option>{/each}
        </select></label>
        <button type="submit" disabled={!mergeId}>合并到当前模块</button>
      </form>
    </details>
    <details>
      <summary>拆分模块</summary>
      <p class="panel-description">从固定来源提交的文件中选取一部分，放入新模块。两个模块都须重新验证。</p>
      <form class="draft-edit-form" onsubmit={(event) => { event.preventDefault(); send({ kind: 'split', module_id: module!.id, new_id: splitId, title: splitTitle, files: splitFiles }) }}>
        <label>新模块名称<input bind:value={splitTitle} required /></label>
        <div class="draft-file-list">{#each module.files as file}<label class="draft-file-option"><input type="checkbox" checked={splitFiles.includes(file)} onchange={(event) => toggleFile(file, event.currentTarget.checked)} />{file}</label>{/each}</div>
        <button type="submit" disabled={!splitFiles.length || splitFiles.length === module.files.length}>拆分选中文件</button>
      </form>
    </details>
    <details>
      <summary>补充接口</summary>
      <form class="draft-edit-form" onsubmit={(event) => { event.preventDefault(); send({ kind: 'interface', id: interfaceId, from: module!.id, to: interfaceTo, title: interfaceTitle, contract }) }}>
        <label>目标模块<select value={interfaceTo} onchange={(event) => { interfaceTo = event.currentTarget.value }} required><option value="">选择模块</option>{#each modules.filter((entry) => entry.id !== module?.id) as entry}<option value={entry.id}>{entry.title}</option>{/each}</select></label>
        <label>接口名称<input bind:value={interfaceTitle} required /></label>
        <label>接口约定<textarea bind:value={contract} required rows="4"></textarea></label>
        <button type="submit" disabled={!interfaceTo}>保存接口</button>
      </form>
    </details>
  {/if}
</fieldset>

