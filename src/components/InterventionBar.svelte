<script lang="ts">
  import { untrack } from 'svelte';
  import { decodeInterventionView, decodePreparedIntervention, decodeInterventionOutcome, sameAction, deliveryCounts, type InterventionAction, type InterventionClient, type InterventionScope, type InterventionView, type PreparedIntervention, type ResumeRequest } from '../lib/interventions';
  let { token, client, modules = [], tasks = [], refreshKey = '', onrecorded } = $props<{ token: string; client: InterventionClient; modules?: { id: string; title: string }[]; tasks?: { id: string; title: string; module: string }[]; refreshKey?: string; onrecorded?: () => void }>();
  const selectedToken = $derived(token); const selectedClient = $derived(client);
  const selectedRefreshKey = $derived(refreshKey);
  let view = $state<InterventionView | null>(null);
  let pending = $state<PreparedIntervention[]>([]);
  let blockedOriginals = $state<string[]>([]);
  let knownQueue = $state(false); let busy = $state(false); let error = $state(''); let notice = $state('');
  let scopeKind = $state<'project' | 'module' | 'task'>('project'); let target = $state(''); let text = $state('');
  let pausePreview = $state(false); let resumePreview = $state<ResumeRequest | null>(null);
  let generation = 0;
  let lastRefreshKey: string | undefined; let refreshQueued = false; let refreshTimer: ReturnType<typeof setTimeout> | undefined;
  const moduleOptions = $derived(modules.length ? modules : [...new Set(view?.active_runs.map(run => run.module) ?? [])].map(id => ({ id, title: id })));
  const taskOptions = $derived(tasks.length ? tasks : [...new Set(view?.active_runs.map(run => run.task) ?? [])].map(id => ({ id, title: id, module: '' })));
  const ready = $derived(view !== null && knownQueue && pending.length === 0 && !busy);
  const scope = $derived<InterventionScope | null>(scopeKind === 'project' ? { kind: 'project' } : target ? scopeKind === 'module' ? { kind: 'module', module: target } : { kind: 'task', task: target } : null);
  const resumeCurrent = $derived(view?.paused === true && resumePreview !== null && view.resume_request?.request === resumePreview.request && view.pause_revision === resumePreview.pause_revision
    && JSON.stringify(view.resume_request.runs) === JSON.stringify(resumePreview.runs) && JSON.stringify(view.active_runs.map(run => run.run).sort()) === JSON.stringify(resumePreview.runs));
  const failureMessage = (failure: unknown) => failure instanceof Error ? failure.message : String(failure);
  const current = (serial: number, selected: string) => serial === generation && selected === selectedToken;
  $effect(() => {
    const selected = selectedToken; const source = selectedClient;
    generation++; view = null; pending = []; blockedOriginals = []; knownQueue = false; busy = false; error = ''; notice = ''; scopeKind = 'project'; target = ''; text = ''; pausePreview = false; resumePreview = null; lastRefreshKey = undefined; refreshQueued = false;
    untrack(() => void reload(selected, source));
    return () => { generation++; clearTimeout(refreshTimer); refreshTimer = undefined; };
  });
  $effect(() => { const key = selectedRefreshKey; untrack(() => { if (lastRefreshKey !== undefined && key !== lastRefreshKey) scheduleRefresh(); lastRefreshKey = key; }); });
  function scheduleRefresh() { clearTimeout(refreshTimer); refreshTimer = setTimeout(() => { refreshTimer = undefined; if (busy) refreshQueued = true; else void reload(); }, 200); }
  function finished(serial: number, selected: string) { if (current(serial, selected)) { busy = false; if (refreshQueued) { refreshQueued = false; scheduleRefresh(); } } }
  async function reload(selected = selectedToken, source = selectedClient) {
    const serial = ++generation; busy = true; error = ''; knownQueue = false;
    try {
      const [rawView, originals] = await Promise.all([source.load(selected), source.pending(selected)]);
      if (!current(serial, selected)) return;
      const loaded = decodeInterventionView(rawView); const saved: PreparedIntervention[] = originals.map(decodePreparedIntervention);
      if (saved.some(original => original.project_id !== loaded.project_id || original.human_actor !== loaded.human_actor)) throw new Error('原操作队列与所选项目或本机人工身份不一致。');
      view = loaded; pending = saved; knownQueue = true;
    } catch (failure) { if (current(serial, selected)) error = failureMessage(failure); }
    finally { finished(serial, selected); }
  }
  async function send(original: PreparedIntervention, serial: number, selected: string) {
    const result = decodeInterventionOutcome(await selectedClient.submit(selected, original.request_id), original);
    if (!current(serial, selected)) return;
    if (result.state === 'confirmed') {
      pending = pending.filter(item => item.request_id !== original.request_id);
      notice = original.action.action === 'interject' ? '插话请求已确认记录；送达情况以服务端消息记录为准。' : '原控制请求已确认记录；当前状态以重新读取为准。';
      if(original.action.action==='interject'&&text===original.action.text)text='';
      pausePreview = false; resumePreview = null; onrecorded?.();
      await reload(selected, selectedClient);
    } else {
      error = result.error?.message ?? '原请求尚未确认；保留原 UUID，重试不会创建另一份操作。';
      if (result.state === 'rejected' || result.state === 'conflict') { knownQueue = false; notice = '请重新读取原队列与实际状态；不会以新 UUID 自动重发。'; }
    }
  }
  async function submit(action: InterventionAction) {
    if (!ready || !view) return;
    const selected = selectedToken; const serial = ++generation; const originalView = view;
    busy = true; error = ''; notice = ''; let original: PreparedIntervention | null = null;
    try {
      original = decodePreparedIntervention(await selectedClient.prepare(selected, originalView.action_ticket, action));
      if (!current(serial, selected)) return;
      pending = [original];
      if (original.project_id !== originalView.project_id || original.human_actor !== originalView.human_actor || !sameAction(original.action, action)) {blockedOriginals=[...blockedOriginals,original.request_id];throw new Error('已保存原操作与刚确认的范围、内容或身份不同；不会提交。');}
      await send(original, serial, selected);
    } catch (failure) { if (current(serial, selected)) { error = failureMessage(failure); knownQueue = false; } }
    finally { finished(serial, selected); }
  }
  async function retry(original: PreparedIntervention) {
    if (busy || !view || original.project_id !== view.project_id || blockedOriginals.includes(original.request_id)) return;
    const selected = selectedToken; const serial = ++generation; busy = true; error = '';
    try { await send(original, serial, selected); }
    catch (failure) { if (current(serial, selected)) error = failureMessage(failure); }
    finally { finished(serial, selected); }
  }
  async function requestResume() {
    if (!ready || !view?.paused) return;
    const selected = selectedToken; const serial = ++generation; busy = true; error = ''; resumePreview = null;
    try {
      await selectedClient.requestResume(selected);
      const [rawView, originals] = await Promise.all([selectedClient.load(selected), selectedClient.pending(selected)]);
      if (!current(serial, selected)) return;
      const loaded = decodeInterventionView(rawView); const saved: PreparedIntervention[] = originals.map(decodePreparedIntervention);
      if (saved.some(original => original.project_id !== loaded.project_id || original.human_actor !== loaded.human_actor)) throw new Error('原操作队列与所选项目或本机人工身份不一致。');
      pending = saved; view = loaded; knownQueue = true;
      const request = loaded.resume_request;
      if (request) resumePreview = { request: request.request, pause_revision: request.pause_revision, runs: [...request.runs] };
      else notice = '当前没有可确认的恢复请求；未恢复任务领取。';
    } catch (failure) { if (current(serial, selected)) { error = failureMessage(failure); knownQueue = false; } }
    finally { finished(serial, selected); }
  }
</script>

<section class="intervention-bar" aria-label="项目插话与运行控制">
  <div class="intervention-heading"><h3>给 Agent 插话</h3><button type="button" class="secondary" disabled={busy} onclick={() => void reload()}>重新读取控制状态</button></div>
  <form onsubmit={(event) => { event.preventDefault(); if (scope && text.trim()) void submit({ action: 'interject', scope, text }); }}>
    <div class="intervention-scope"><label>发送范围<select bind:value={scopeKind} disabled={!ready} onchange={() => target = ''}><option value="project">全局 · 当前活动运行</option><option value="module">模块</option><option value="task">任务</option></select></label>
      {#if scopeKind !== 'project'}<label>{scopeKind === 'module' ? '选择模块' : '选择任务'}<select bind:value={target} disabled={!ready}><option value="">请选择真实对象</option>{#each scopeKind === 'module' ? moduleOptions : taskOptions as option (option.id)}<option value={option.id}>{option.title} · {option.id}</option>{/each}</select></label>{/if}
    </div>
    <label>插话内容<textarea bind:value={text} disabled={!ready} rows="2" placeholder="补充约束或请求停写；不会直接改变基线或批准门禁。"></textarea></label>
    <button type="submit" disabled={!ready || !scope || !text.trim()}>明确发送此范围插话</button>
  </form>
  {#if error}<p role="alert">{error}</p>{/if}{#if notice}<p role="status">{notice}</p>{/if}
  {#if !knownQueue && !busy}<p>尚未核对原操作队列；重新读取后才能创建新操作。</p>{/if}
  {#if pending.length}<section aria-label="待确认的原控制操作"><h4>保留原 UUID 的操作</h4>{#each pending as original (original.request_id)}<div class="intervention-original"><code>{original.request_id}</code><pre>{JSON.stringify(original.action, null, 2)}</pre><button type="button" class="secondary" disabled={busy || !knownQueue || blockedOriginals.includes(original.request_id)} onclick={() => void retry(original)}>重试这个原操作</button></div>{/each}</section>{/if}
  {#if view}<section class="intervention-controls" aria-label="暂停与恢复任务领取"><p>{view.paused ? '⏸ 新任务领取已暂停' : '▶ 新任务可按门禁领取'} · {view.active_runs.length} 个登记的活动运行</p>
    {#if !view.paused}<button type="button" class="secondary" disabled={!ready} onclick={() => pausePreview = true}>暂停全部…</button>{#if pausePreview}<div class="intervention-confirm"><p>将暂停新的任务领取，并向当时活动运行排队发送停写提示。现有 harness 不会被此操作杀死。</p><button type="button" disabled={!ready} onclick={() => void submit({ action: 'pause_all' })}>确认暂停新的任务领取</button><button type="button" class="secondary" disabled={busy} onclick={() => pausePreview = false}>取消暂停</button></div>{/if}
    {:else}<button type="button" class="secondary" disabled={!ready} onclick={() => void requestResume()}>读取恢复确认范围</button><p>此步只建立并读取恢复请求；还需下方明确确认才恢复领取。</p>{/if}
    {#if resumePreview}<section class="intervention-confirm" aria-label="原恢复范围确认"><h4>核对这一次恢复范围</h4><p>原请求：<code>{resumePreview.request}</code></p><p>暂停版本：<code>{resumePreview.pause_revision}</code></p><p>完整运行范围（{resumePreview.runs.length}）：</p><ul>{#each resumePreview.runs as run (run)}<li><code>{run}</code></li>{:else}<li>0 个运行，不补造收件人。</li>{/each}</ul><p>确认只恢复任务领取，不启动或恢复 harness，也不放行其他门禁。</p>{#if !resumeCurrent}<p role="status">暂停版本或运行范围已变化；须重新读取范围，旧确认已禁用。</p>{/if}<button type="button" disabled={!ready || !resumeCurrent} onclick={() => resumePreview && void submit({ action: 'confirm_resume', request: resumePreview.request, pause_revision: resumePreview.pause_revision, runs: [...resumePreview.runs] })}>确认恢复以上精确范围</button><button type="button" class="secondary" disabled={busy} onclick={() => resumePreview = null}>取消恢复确认</button></section>{/if}
  </section><section aria-label="插话送达记录"><h4>消息送达记录</h4>{#each view.messages as message (message.id)}{@const counts = deliveryCounts(message)}<div class="intervention-message"><p>{message.text}</p><p><code>{message.id}</code> · 已送达 {counts.delivered} · 排队中 {counts.queued}{#if counts.inactive} · 历史未送达 {counts.inactive}{/if}</p>{#if message.recipients.length === 0}<p>0 个收件人，未送达任何运行。</p>{/if}<p>送达仅表示消息已完整输出给运行，不代表已处理。</p></div>{:else}<p>尚无消息记录。</p>{/each}</section>{/if}
</section>
