<script lang="ts">
  import type { MainResolutionOperation, MainResolutionPreview, MainResolutionReceipt, ProjectClient } from '../lib/api'
  let { client, token, commit, visible, onBlocked, onClose, onComplete }: {
    client: ProjectClient; token: string; commit: string; visible: boolean
    onBlocked: (blocked: boolean) => void
    onClose: () => void
    onComplete: () => void
  } = $props()
  let preview = $state<MainResolutionPreview | null>(null)
  let receipt = $state<MainResolutionReceipt | null>(null)
  let busy = $state(false)
  let attempted = $state(false)
  let error = $state<string | null>(null)
  let capturing = $state(false)
  let captured = $state<string | null>(null)

  async function prepare(operation: MainResolutionOperation) {
    if (busy || attempted) return
    busy = true
    error = null
    preview = null
    onBlocked(true)
    try {
      if (!client.prepareMainResolution) throw new Error('当前客户端不支持处理 main 变更。')
      const value = await client.prepareMainResolution(token, operation, commit)
      if (value.version !== 1 || value.target_commit !== commit || value.operation !== operation
        || !value.request_id || !value.main_revision || !value.content_hash || !Array.isArray(value.changes)) {
        throw new Error('预览与所选提交不一致，请重新读取。')
      }
      preview = value
    } catch (cause) { error = String(cause) }
    finally { busy = false; onBlocked(false) }
  }

  async function confirm() {
    if (!preview || busy) return
    const input = preview
    const retry = attempted
    attempted = true
    busy = true
    error = null
    onBlocked(true)
    try {
      if (!client.confirmMainResolution) throw new Error('操作已拒绝：当前客户端不支持确认处理。')
      const value = await client.confirmMainResolution(token, input)
      if (value.request_id !== input.request_id || value.operation !== input.operation
        || value.target_commit !== input.target_commit || !value.event
        || !Number.isSafeInteger(value.seq) || value.seq <= 0 || !value.resolution_commit || !value.snapshot_commit) {
        throw new Error('处理回执与原确认不一致，请重试原请求。')
      }
      receipt = value
      attempted = false
      onBlocked(false)
      onComplete()
    } catch (cause) {
      error = String(cause)
      if (!retry && error.replace(/^Error:\s*/, '').startsWith('操作已拒绝：')) {
        attempted = false
        onBlocked(false)
      }
    } finally { busy = false }
  }

  async function capture() {
    if (busy || (attempted && !capturing)) return
    const retry = attempted
    busy = true; attempted = true; capturing = true; preview = null; error = null; onBlocked(true)
    try {
      if (!client.captureMainProposal) throw new Error('操作已拒绝：当前客户端不支持转为提案。')
      const value = await client.captureMainProposal(token, commit)
      if (value.status !== 'draft' || value.proposal?.source_commit !== commit || !value.id) {
        throw new Error('提案捕获回执与原提交不一致，请重试原请求。')
      }
      captured = value.id
      attempted = false; capturing = false; onBlocked(false); onComplete()
    } catch (cause) {
      error = String(cause)
      if (!retry && error.replace(/^Error:\s*/, '').startsWith('操作已拒绝：')) {
        attempted = false; capturing = false; onBlocked(false)
      }
    } finally { busy = false }
  }
</script>

{#if visible}
  <aside class="canvas-panel" aria-label="处理主干变更">
    <div class="panel-heading"><h2>处理主干变更</h2><button class="close-button" onclick={onClose} aria-label="关闭主干变更">×</button></div>
    <p>提交 <code>{commit}</code></p>
    {#if captured}
      <p role="status">已保存提案 {captured}。</p>
      <p>原禁止中断仍保留。请从“提案”继续补全草稿、审阅并批准；捕获不表示原提交已获批准。</p>
    {:else if receipt}
      <p role="status">已{receipt.operation === 'claim' ? '认领' : '撤销'}此提交。</p>
      <p>实际提交 <code>{receipt.resolution_commit}</code></p>
    {:else}
      <p class="panel-description">认领表示你确认这是你批准的慢层变更。撤销会生成新的反向提交，不改写已有历史。系统配置、快照和基线清单不能通过认领批准。</p>
      <div class="button-row">
        <button onclick={() => prepare('claim')} disabled={busy || attempted}>认领此提交</button>
        <button onclick={() => prepare('revert')} disabled={busy || attempted}>生成撤销提交</button>
        <button onclick={capture} disabled={busy || attempted}>转为提案</button>
      </div>
      {#if preview}
        <h3>{preview.operation === 'claim' ? '确认认领内容' : '确认撤销内容'}</h3>
        <p>来源 main：<code>{preview.main_revision}</code></p>
        <ul>{#each preview.changes as change}<li>{change.status} · {change.path}</li>{/each}</ul>
        <details open><summary>实际内容差异</summary><pre class="document-content">{preview.diff}</pre></details>
        <p>内容摘要：<code>{preview.content_hash}</code></p>
        <button class="primary-button" onclick={confirm} disabled={busy}>
          {attempted ? '重试原确认' : preview.operation === 'claim' ? '确认认领该提交' : '确认生成撤销提交'}
        </button>
      {/if}
      {#if busy}<p role="status">正在核对实际提交…</p>{/if}
      {#if error}<p role="alert">{error}</p>{/if}
      {#if attempted}<p>结果尚未确认，原请求已保留。请重试原确认后再切换项目。</p>{/if}
      {#if capturing}<button onclick={capture} disabled={busy}>重试原提案捕获</button>{/if}
    {/if}
  </aside>
{/if}
