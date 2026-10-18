<script lang="ts">
  import type { DraftEdit, SavedDraft, SplitPart, VerifyCommand } from '../lib/draft';
  import VerifyCommandsEditor from './VerifyCommandsEditor.svelte';
  let { saved, busy = false, onedit }: { saved: SavedDraft; busy?: boolean; onedit: (edit: DraftEdit) => void } = $props();
  type Part = { key: string; id: string; title: string; verify: VerifyCommand[]; x: number; y: number };
  let action = $state('merge');
  let moduleId = $state(''); let mergeIds = $state<string[]>([]); let keep = $state(''); let title = $state('');
  let parts = $state<Part[]>([]); let paths = $state<Record<string, string>>({}); let acceptances = $state<Record<string, string>>({});
  let endpoints = $state<Record<string, [string, string]>>({}); let commands = $state<VerifyCommand[]>([]); let commandsReadable = $state(true);
  let interfaceId = $state(''); let interfacePath = $state(''); let interfaceText = $state(''); let from = $state(''); let to = $state('');
  let error = $state('');
  const modules = $derived(saved.object_views.filter(object => object.type === 'Module'));
  const interfaces = $derived(saved.object_views.filter(object => object.type === 'Interface'));
  const source = $derived(modules.find(object => object.id === moduleId));
  const sourcePaths = $derived(Array.isArray(source?.fields?.paths) ? source.fields.paths.filter((path): path is string => typeof path === 'string') : []);
  const sourceAcceptances = $derived(saved.object_views.filter(object => object.type === 'Acceptance' && object.fields?.module === moduleId));
  const sourceInterfaces = $derived(interfaces.filter(object => object.fields?.from === moduleId || object.fields?.to === moduleId));
  const copyCommands = (values: VerifyCommand[]) => values.map(command => ({cwd: command.cwd, argv: [...command.argv]}));
  function readableCommands(value: unknown): value is VerifyCommand[] {
    return Array.isArray(value) && value.every(command => command !== null && typeof command === 'object' && typeof command.cwd === 'string' && Array.isArray(command.argv) && command.argv.every((arg: unknown) => typeof arg === 'string'));
  }
  function addPart() { parts = [...parts, {key: crypto.randomUUID(), id: '', title: '', verify: [], x: parts.length * 400, y: 0}]; }
  function selectModule(value: string) {
    moduleId = value; error = ''; paths = {}; acceptances = {}; endpoints = {}; parts = []; addPart(); addPart();
    const fields = modules.find(object => object.id === value)?.fields;
    const original = fields && 'verify' in fields ? fields.verify : [];
    commandsReadable = readableCommands(original);
    commands = commandsReadable ? copyCommands(original as VerifyCommand[]) : [];
  }
  function selectInterface(value: string) {
    const object = interfaces.find(object => object.id === value);
    interfaceId = object?.id ?? ''; interfacePath = object?.path ?? ''; interfaceText = object?.text ?? '';
    from = typeof object?.fields?.from === 'string' ? object.fields.from : '';
    to = typeof object?.fields?.to === 'string' ? object.fields.to : '';
  }
  function assignEndpoint(id: string, index: number, value: string) {
    const pair: [string, string] = [...(endpoints[id] ?? ['', ''])]; pair[index] = value;
    endpoints = {...endpoints, [id]: pair};
  }
  function partId(key: string): string { return parts.find(part => part.key === key)?.id ?? ''; }
  function template() {
    interfacePath = `${saved.draft.bundle.docs_dir}/arch/${interfaceId}.md`;
    interfaceText = `---\ntype: Interface\nid: ${JSON.stringify(interfaceId)}\ntitle: ${JSON.stringify(interfaceId)}\n---\n\n`;
  }
  function submit(event: SubmitEvent) {
    event.preventDefault(); if (busy) return; error = '';
    if (action === 'merge') {
      if (mergeIds.length < 2 || !mergeIds.includes(keep)) { error = '请选择至少两个模块，并选择其中一个保留 ID。'; return; }
      onedit({action: 'merge', modules: [...mergeIds], keep, title});
    } else if (action === 'split') {
      if (parts.length < 2 || !sourcePaths.length || sourcePaths.some(path => !partId(paths[path])) || sourceAcceptances.some(object => !partId(acceptances[object.id]))) {
        error = '请将原模块的每条路径与每项验收明确分配到拆分部分。'; return;
      }
      const mapping: Record<string, [string, string]> = {};
      for (const object of sourceInterfaces) {
        const pair = [object.fields?.from, object.fields?.to].map((endpoint, index) => endpoint === moduleId ? partId(endpoints[object.id]?.[index]) : endpoint);
        if (pair.some(endpoint => typeof endpoint !== 'string' || !endpoint)) { error = `请明确分配接口 ${object.id} 的端点。`; return; }
        mapping[object.id] = pair as [string, string];
      }
      const result: SplitPart[] = parts.map(part => ({id: part.id, title: part.title, verify: copyCommands(part.verify), x: part.x, y: part.y,
        paths: sourcePaths.filter(path => paths[path] === part.key), acceptance_ids: sourceAcceptances.filter(object => acceptances[object.id] === part.key).map(object => object.id)}));
      onedit({action: 'split', module: moduleId, parts: result, interface_endpoints: mapping});
    } else if (action === 'confirm_verify') {
      if (!commandsReadable) { error = '现有验证字段无法读取，请保留原文并由 Agent 准备新的候选。'; return; }
      onedit({action: 'confirm_verify', module: moduleId, commands: copyCommands(commands)});
    }
    else if (action === 'put_interface') onedit({action: 'put_interface', object: {id: interfaceId, path: interfacePath, text: interfaceText}, from, to});
    else if (action === 'remove_interface') onedit({action: 'remove_interface', interface: interfaceId});
  }
</script>

<form onsubmit={submit} aria-label="模块与接口编辑">
  <label>结构操作<select value={action} disabled={busy} onchange={event => {action = event.currentTarget.value; error = '';}}>
    <option value="merge">合并模块</option><option value="split">拆分模块</option><option value="put_interface">新增 / 编辑接口</option><option value="remove_interface">删除接口</option><option value="confirm_verify">确认验证命令</option>
  </select></label>
  {#if action === 'merge'}
    <fieldset><legend>参与合并的模块</legend>{#each modules as object}<label class="draft-check"><input type="checkbox" value={object.id} bind:group={mergeIds} disabled={busy}/>{object.title ?? object.id} · {object.id}</label>{/each}</fieldset>
    <label>保留模块 ID<select value={keep} onchange={event => keep = event.currentTarget.value} required disabled={busy}><option value="">选择保留的模块</option>{#each mergeIds as id}<option value={id}>{id}</option>{/each}</select></label>
    <label>合并后标题<input bind:value={title} required disabled={busy}/></label>
    <p class="status">模块间的内部接口须先明确删除或调整；无法安全合并时会显示具体原因。</p>
  {:else if action === 'split' || action === 'confirm_verify'}
    <label>源模块<select value={moduleId} required disabled={busy} onchange={event => selectModule(event.currentTarget.value)}><option value="">选择模块</option>{#each modules as object}<option value={object.id}>{object.title ?? object.id} · {object.id}</option>{/each}</select></label>
    {#if moduleId && action === 'confirm_verify'}
      {#if commandsReadable}<VerifyCommandsEditor {commands} {busy} onchange={value => commands = value}/>{:else}
        <p class="draft-diagnostic" role="alert">现有验证命令是展示文本或无法读取，不能将它默认为空列表保存，也不会自动拆分命令。请手动配置后再保存。</p>
        <details><summary>模块原文 · 保持不变</summary><pre>{source?.text}</pre></details>
        <button class="secondary" type="button" disabled={busy} onclick={() => {commands = []; commandsReadable = true;}}>手动配置替换验证命令</button>
      {/if}
    {/if}
    {#if moduleId && action === 'split'}
      {#each parts as part, index (part.key)}
        <fieldset><legend>拆分部分 {index + 1}</legend>
          <label>部分 {index + 1} ID<input bind:value={part.id} required disabled={busy}/></label>
          <label>部分 {index + 1} 标题<input bind:value={part.title} required disabled={busy}/></label>
          <div class="draft-fields"><label>部分 {index + 1} X<input type="number" step="1" bind:value={part.x} required disabled={busy}/></label><label>部分 {index + 1} Y<input type="number" step="1" bind:value={part.y} required disabled={busy}/></label></div>
          <VerifyCommandsEditor commands={part.verify} {busy} onchange={value => part.verify = value}/>
          {#if parts.length > 2}<button type="button" class="secondary" disabled={busy} onclick={() => parts = parts.filter(value => value.key !== part.key)}>移除部分 {index + 1}</button>{/if}
        </fieldset>
      {/each}
      <button type="button" class="secondary" disabled={busy} onclick={addPart}>添加拆分部分</button>
      <fieldset><legend>分配原模块路径</legend>{#each sourcePaths as path}<label>{path}<select value={paths[path] ?? ''} onchange={event => paths[path] = event.currentTarget.value} required disabled={busy}><option value="">选择拆分部分</option>{#each parts as part, index}<option value={part.key}>{part.id || `部分 ${index + 1}`}</option>{/each}</select></label>{/each}</fieldset>
      {#if sourceAcceptances.length}<fieldset><legend>分配原模块验收</legend>{#each sourceAcceptances as object}<label>验收 {object.id}<select value={acceptances[object.id] ?? ''} onchange={event => acceptances[object.id] = event.currentTarget.value} required disabled={busy}><option value="">选择拆分部分</option>{#each parts as part, index}<option value={part.key}>{part.id || `部分 ${index + 1}`}</option>{/each}</select></label>{/each}</fieldset>{/if}
      {#each sourceInterfaces as object}<fieldset><legend>接口 {object.id} 的端点</legend>{#each ['from', 'to'] as endpoint, index}{#if object.fields?.[endpoint] === moduleId}<label>接口 {object.id} {endpoint === 'from' ? '起点' : '终点'}<select value={endpoints[object.id]?.[index] ?? ''} required disabled={busy} onchange={event => assignEndpoint(object.id, index, event.currentTarget.value)}><option value="">选择拆分部分</option>{#each parts as part, partIndex}<option value={part.key}>{part.id || `部分 ${partIndex + 1}`}</option>{/each}</select></label>{:else}<p>{endpoint === 'from' ? '起点' : '终点'}：{String(object.fields?.[endpoint])}（保留）</p>{/if}{/each}</fieldset>{/each}
      <p class="status">路径、验收和接口端点需逐项分配；未分配的普通关系须先删除或调整。已有对象原文及未知字段由共享草稿内核保留。</p>
    {/if}
  {:else if action === 'put_interface'}
    <label>已有接口<select value="" disabled={busy} onchange={event => selectInterface(event.currentTarget.value)}><option value="">选择接口或填写新接口</option>{#each interfaces as object}<option value={object.id}>{object.id}</option>{/each}</select></label>
    <label>接口 ID<input bind:value={interfaceId} required disabled={busy}/></label>
    <button type="button" class="secondary" disabled={busy || !interfaceId || saved.object_views.some(object => object.id === interfaceId)} onclick={template}>填入新接口模板</button>
    <label>接口 Markdown 路径<input bind:value={interfacePath} required disabled={busy}/></label>
    <label>接口原文<textarea bind:value={interfaceText} required disabled={busy}></textarea></label>
    <label>接口起点<select value={from} onchange={event => from = event.currentTarget.value} required disabled={busy}><option value="">选择模块</option>{#each modules as object}<option value={object.id}>{object.id}</option>{/each}</select></label>
    <label>接口终点<select value={to} onchange={event => to = event.currentTarget.value} required disabled={busy}><option value="">选择模块</option>{#each modules as object}<option value={object.id}>{object.id}</option>{/each}</select></label>
  {:else if action === 'remove_interface'}
    <label>删除的接口<select value={interfaceId} onchange={event => interfaceId = event.currentTarget.value} required disabled={busy}><option value="">选择接口</option>{#each interfaces as object}<option value={object.id}>{object.id}</option>{/each}</select></label>
    <p class="status">保存后仅从草稿删除此接口及绑定关系，当前基线不变。</p>
  {/if}
  {#if error}<p class="draft-diagnostic" role="alert">{error}</p>{/if}
  <button type="submit" disabled={busy || (action === 'confirm_verify' && (!moduleId || !commandsReadable))}>保存结构编辑到草稿</button>
</form>
