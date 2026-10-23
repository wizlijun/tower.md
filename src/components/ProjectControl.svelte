<script module lang="ts">
  export type ProjectControlState = {
    paused: boolean
    pause_event: string | null
    resume_requests: { event: string; pause_event: string; actor: string; at: string }[]
  }
  export type ControlAction = { kind: 'pause' } | { kind: 'resume'; pause_event: string }
</script>

<script lang="ts">
  let { control, onPause, onResume, frozen = null, busy = false, error = null, onRetry }: {
    control: ProjectControlState
    onPause: () => void
    onResume: (pauseEvent: string) => void
    frozen?: ControlAction | null
    busy?: boolean
    error?: string | null
    onRetry: () => void
  } = $props()
  let requests = $derived(control.resume_requests.filter((request) => request.pause_event === control.pause_event))
</script>

<p class="panel-description">暂停会拒绝新的任务领取，并向提交时的活动运行发送检查点暂停指令。受托者在安全检查点响应；这不表示进程已停止，也不延长模块占用。</p>
{#if error}<p role="alert">{error}</p>{/if}
{#if frozen}
  <p class="verification-status verification-pending">! {frozen.kind === 'pause' ? '暂停请求待确认' : '恢复请求待确认'}</p>
  {#if frozen.kind === 'resume'}<dl class="source-list"><dt>原暂停</dt><dd>{frozen.pause_event}</dd></dl>{/if}
  <p class="panel-description">结果尚未确认。重试沿用原请求号和原操作，不会根据新状态替换请求。</p>
  <button class="primary-button" disabled={busy} onclick={() => { if (!busy) onRetry() }}>{busy ? '正在确认…' : '重试原操作'}</button>
{:else if control.paused}
  <p class="verification-status verification-pending">Ⅱ 已暂停新任务领取</p>
  {#if control.pause_event}<dl class="source-list"><dt>暂停记录</dt><dd>{control.pause_event}</dd></dl>{/if}
  {#if requests.length}
    <p>有 {requests.length} 条恢复请求，须由你确认后才能恢复。</p>
    <details><summary>查看恢复请求</summary><ul class="diagnostics">{#each requests as request}
      <li>{request.actor} · {request.at} · {request.event}</li>
    {/each}</ul></details>
  {/if}
  <p class="panel-description">恢复将解除这次暂停并向当前活动运行发送恢复指令。运行仍须持有有效的模块占用并通过检查。</p>
  <button class="primary-button" disabled={busy || !control.pause_event} onclick={() => { if (!busy && control.pause_event) onResume(control.pause_event) }}>确认恢复项目</button>
  {#if !control.pause_event}<p role="alert">缺少原暂停记录，暂不能恢复。请等待状态刷新。</p>{/if}
{:else}
  <p>项目允许领取新任务。</p>
  <button disabled={busy} onclick={() => { if (!busy) onPause() }}>暂停项目</button>
{/if}
