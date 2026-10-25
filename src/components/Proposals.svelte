<script lang="ts">
  import { onMount, onDestroy } from 'svelte'
  import type { ProjectClient, ProposalEntry, PublicationPreview, PublicationApproval, ProposalRejection } from '../lib/api'
  import { humanRequestId } from '../lib/request-id'
  import { crossReviewLabel, type CrossReviewEntry } from '../lib/controller'
  let { client, token, visible, onBlocked, onClose, onComplete, onViewDraft, crossReviews = {} }: {
    client: ProjectClient; token: string; visible: boolean
    onBlocked: (value: boolean) => void; onClose: () => void; onComplete: () => void
    onViewDraft: (id: string) => void
    /** M3-H: a different harness's review, evidence for the person who decides. */
    crossReviews?: Record<string, CrossReviewEntry>
  } = $props()
  let entries = $state<ProposalEntry[]>([])
  let selected = $state<ProposalEntry | null>(null)
  let preview = $state<PublicationPreview | null>(null)
  let busy = $state(false)
  let error = $state<string | null>(null)
  let result = $state<string | null>(null)
  let reason = $state('')
  let rejecting = $state(false)
  type Pending = { kind: 'approve'; id: string; input: PublicationApproval } | { kind: 'reject'; id: string; input: ProposalRejection }
  let pending = $state<Pending | null>(null)
  let active = true
  onDestroy(() => { active = false })
  onMount(() => { void load() })

  async function load() {
    if (busy || pending) return
    busy = true; error = null
    try {
      if (!client.listProposals) throw new Error('当前客户端不支持提案读取。')
      const value = await client.listProposals(token)
      if (!active) return
      entries = value.proposals
      if (selected) selected = entries.find((entry) => entry.id === selected?.id) ?? null
    } catch (cause) { if (active) error = String(cause) }
    finally { if (active) busy = false }
  }
  function select(entry: ProposalEntry) {
    if (busy || pending) return
    selected = entry; preview = null; error = null; result = null; rejecting = false; reason = ''
  }
  function updateEntry(id: string, changes: Partial<ProposalEntry>) {
    entries = entries.map((entry) => entry.id === id ? { ...entry, ...changes } : entry)
    if (selected?.id === id) selected = entries.find((entry) => entry.id === id) ?? null
  }
  async function prepare() {
    if (!selected?.proposal || selected.status !== 'submitted' || pending || busy) return
    const entry = selected
    busy = true; error = null; onBlocked(true)
    try {
      if (!client.prepareProposal) throw new Error('当前客户端不支持提案出版。')
      const value = await client.prepareProposal(token, entry.id)
      if (!active) return
      if (value.draft_id !== entry.proposal!.draft_id || value.project_id !== entry.proposal!.project_id
        || value.source_revision !== entry.proposal!.source_revision || !value.request_id || !value.content_hash) {
        throw new Error('提案出版预览与原件不一致，请重新读取。')
      }
      preview = value; rejecting = false
    } catch (cause) { if (active) error = String(cause) }
    finally { if (active) { busy = false; onBlocked(false) } }
  }
  async function approve() {
    if (!selected || !preview || pending || busy) return
    pending = { kind: 'approve', id: selected.id, input: {
      request_id: preview.request_id, target: preview.target, prepared_at: preview.prepared_at, content_hash: preview.content_hash,
    } }
    await submit(false)
  }
  async function reject() {
    if (!selected?.proposal || !reason.trim() || busy || pending) return
    pending = { kind: 'reject', id: selected.id, input: {
      request_id: humanRequestId(), proposal_hash: selected.proposal.content_hash, reason,
    } }
    await submit(false)
  }
  async function submit(retry = true) {
    if (!pending || busy) return
    const request = pending
    busy = true; error = null; onBlocked(true)
    try {
      if (request.kind === 'approve') {
        if (!client.approveProposal) throw new Error('操作已拒绝：当前客户端不支持批准提案。')
        const receipt = await client.approveProposal(token, request.id, request.input)
        if (receipt.content_hash !== request.input.content_hash || receipt.target !== request.input.target
          || !receipt.commit || !receipt.tag || !receipt.snapshot_commit) throw new Error('出版回执不匹配，请重试原确认。')
        if (!active) return
        updateEntry(request.id, { status: 'approved' })
        result = `已出版 ${receipt.tag} · ${receipt.commit}`
      } else {
        if (!client.rejectProposal) throw new Error('操作已拒绝：当前客户端不支持驳回提案。')
        const receipt = await client.rejectProposal(token, request.id, request.input)
        if (receipt.id !== request.id || receipt.rejection?.eid !== request.input.request_id
          || receipt.proposal?.content_hash !== request.input.proposal_hash || receipt.status !== 'rejected') {
          throw new Error('驳回回执不匹配，请重试原确认。')
        }
        if (!active) return
        updateEntry(request.id, receipt)
        result = '已驳回提案并记录理由。'
      }
      pending = null; preview = null; onBlocked(false); onComplete()
    } catch (cause) {
      if (active) {
        error = String(cause)
        if (!retry && error.replace(/^Error:\s*/, '').startsWith('操作已拒绝：')) { pending = null; onBlocked(false) }
      }
    } finally { if (active) busy = false }
  }
</script>

{#if visible}
  <aside class="canvas-panel" aria-label="提案">
    <div class="panel-heading"><h2>提案</h2><button class="close-button" onclick={onClose} aria-label="关闭提案">×</button></div>
    <button onclick={load} disabled={busy || !!pending}>刷新提案</button>
    {#if error}<p role="alert">{error}</p>{/if}
    {#if busy}<p role="status">正在核对提案…</p>{/if}
    {#if result}<p role="status">{result}</p>{/if}
    {#if !entries.length && !busy}<p>当前没有提案。</p>{/if}
    <div class="draft-list">{#each entries as entry (entry.id)}
      <button onclick={() => select(entry)} disabled={busy || !!pending} aria-pressed={selected?.id === entry.id}>
        <strong>{entry.proposal?.reason ?? entry.text ?? entry.id}</strong>
        <span>{entry.id} · {({ draft: '待补全', submitted: '待审阅', approved: '已批准', rejected: '已驳回' })[entry.status]}</span>
      </button>
    {/each}</div>
    {#if selected?.proposal}
      {@const proposal = selected.proposal}
      <h3>理由与建议</h3><p>{proposal.reason}</p><p>{proposal.recommendation}</p>
      <h3>备选方案</h3><ul>{#each proposal.alternatives as alternative}<li>{alternative}</li>{/each}</ul>
      <dl class="source-list">
        <dt>当前版本</dt><dd>{proposal.baseline_tag}</dd>
        <dt>意图</dt><dd>{proposal.impact.intents.join('、') || '无'}</dd>
        <dt>模块</dt><dd>{proposal.impact.modules.join('、') || '无'}</dd>
        <dt>接口</dt><dd>{proposal.impact.interfaces.join('、') || '无'}</dd>
        <dt>任务</dt><dd>{proposal.impact.tasks.join('、') || '无'}</dd>
        <dt>路径</dt><dd>{proposal.impact.paths.join('、') || '无'}</dd>
      </dl>
      {#if proposal.impact_notes && Object.keys(proposal.impact_notes).length}
        <h3>影响说明</h3>
        <dl class="source-list">{#each Object.entries(proposal.impact_notes) as [object, note] (object)}<dt>{object}</dt><dd>{note}</dd>{/each}</dl>
      {/if}
      {#if crossReviews[selected.id]}
        {@const review = crossReviews[selected.id]}
        <section aria-label="交叉评审">
          <h3>交叉评审（仅供参考）</h3>
          <p>{crossReviewLabel(review, proposal.content_hash)}</p>
          <p>{review.summary}</p>
          {#if review.findings.length}<ul>{#each review.findings as finding}<li>{({ high: '高', medium: '中', low: '低' })[finding.severity]} · {finding.text}</li>{/each}</ul>{/if}
          {#if review.error}<p class="panel-description">未完成原因：{review.error}</p>{/if}
          <p class="panel-description">评审结论只是证据；批准与否仍由你决定。</p>
        </section>
      {/if}
      {#if selected.weakening?.length}
        <section aria-label="这是弱化">
          <h3>⇣ 这是弱化</h3>
          <ul>{#each selected.weakening as item}<li>{item.text}</li>{/each}</ul>
          <p class="panel-description">批准即接受这些放宽；不同意就驳回。</p>
        </section>
      {/if}
      <h3>实际变更</h3>
      {#if proposal.source_proposal_id}
        {@const source = entries.find((entry) => entry.id === proposal.source_proposal_id)}
        <details><summary>原始捕获 · {proposal.source_proposal_id}</summary>
          {#if source?.text}<pre class="document-content">{source.text}</pre>{/if}
          {#if source?.targets}<pre class="document-content">{JSON.stringify(source.targets, null, 2)}</pre>{/if}
          {#if source?.proposal}<p>{source.proposal.reason}</p><code>{source.proposal.source_commit}</code>{/if}
          {#if !source}<p>当前列表没有该原件；请重新读取提案后核对。</p>{/if}
        </details>
      {/if}
      {#each proposal.changes as change}
        <details><summary>{change.path}</summary>
          <h4>之前</h4><pre class="document-content">{change.before ?? (change.before_oid ? `二进制原件 ${change.before_oid}` : '不存在')}</pre>
          <h4>之后</h4><pre class="document-content">{change.after ?? (change.after_oid ? `二进制原件 ${change.after_oid}` : '删除')}</pre>
        </details>
      {/each}
      {#if proposal.draft_id}<button onclick={() => onViewDraft(proposal.draft_id!)} disabled={busy || !!pending}>在画布上审阅草稿</button>{/if}
      {#if selected.status === 'submitted' && !result}
        <div class="button-row">
          <button onclick={prepare} disabled={busy || !!pending}>准备批准</button>
          <button onclick={() => { rejecting = true; preview = null }} disabled={busy || !!pending}>驳回</button>
        </div>
        {#if preview}
          <p>将出版 {preview.target}，在同一提交中更新人定的内容、写入决策与版本清单。</p>
          <p>内容摘要 <code>{preview.content_hash}</code></p>
          <button class="primary-button" onclick={approve} disabled={busy || !!pending}>确认批准并出版 {preview.target}</button>
        {/if}
        {#if rejecting && !pending}
          <label>驳回理由<textarea bind:value={reason} disabled={busy}></textarea></label>
          <button onclick={reject} disabled={busy || !reason.trim()}>确认驳回提案</button>
        {/if}
      {:else if selected.status === 'draft'}<p>草稿尚未补全为经过审阅、验证的版本提案，当前不能批准。</p>{/if}
    {:else if selected}<p>{selected.text}</p><p>需要受托者补全真实草稿、差异与影响分析后再提交。</p>{/if}
    {#if pending}<p>确认结果未知，原请求已保留。</p><button onclick={() => submit()} disabled={busy}>重试原提案确认</button>{/if}
  </aside>
{/if}
