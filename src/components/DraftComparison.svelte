<script lang="ts">
  import type { DraftComparison } from '../lib/api'
  import { diffLine, diffSummary, proposalInput } from '../lib/comparison'
  // J4: a read-only comparison. Nothing here moves a node or submits anything.
  let { comparison, loading, error, titleOf, onRetry, onClose, onProposals }: {
    comparison: DraftComparison | null; loading: boolean; error: string | null
    titleOf: (id: string) => string
    onRetry: () => void; onClose: () => void; onProposals: () => void
  } = $props()
</script>

<aside class="canvas-panel" aria-label="草稿与基线对比">
  <div class="panel-heading"><h2>{comparison ? `与 ${comparison.comparison.baseline} 对比` : '与基线对比'}</h2><button class="close-button" aria-label="关闭详情" onclick={onClose}>×</button></div>
  {#if loading}<p role="status">正在对比草稿与已批准基线…</p>{/if}
  {#if error}<p role="alert">{error}</p><button onclick={onRetry} disabled={loading}>重新对比</button>{/if}
  {#if comparison}
    {@const result = comparison.comparison}
    {#if comparison.outdated}<p role="alert">⚠ 当前已批准基线是 {comparison.current_baseline}，本草稿从 {result.baseline} 开始；请重新从基线进入草稿。</p>{/if}
    {#if comparison.stale}<p role="alert">⚠ 草稿来源提交已不是当前权威提交，只供查看。</p>{/if}
    <p class="panel-description">对比只读，不改变画布上任何节点的位置；未批准，也不会自动提交或出版。</p>
    {#if result.objects.length}
      <ul class="diagnostics" aria-label="变化类型">
        {#each diffSummary(result) as entry}<li><span aria-hidden="true">{entry.mark}</span> {entry.label} · {entry.count} 个对象</li>{/each}
      </ul>
      <h3>变化的对象</h3>
      <ul class="diagnostics">
        {#each result.objects as object}
          <li>{diffLine(object, titleOf)}
            {#if object.files_added?.length}<br />新增文件：{object.files_added.join('、')}{/if}
            {#if object.files_removed?.length}<br />移出文件：{object.files_removed.join('、')}{/if}
          </li>
        {/each}
      </ul>
    {:else}
      <p class="panel-description">与 {result.baseline} 相比没有结构或内容变化。</p>
    {/if}
    {#if result.unchanged.length}<p class="panel-description">未变化：{result.unchanged.length} 个对象。</p>{/if}
    {#if result.unassigned.length}
      <h3>未划分区</h3>
      <ul class="diagnostics">{#each result.unassigned as file}<li>{file}</li>{/each}</ul>
    {/if}
    <h3>提案将带的影响</h3>
    <dl class="source-list">
      <dt>模块</dt><dd>{comparison.impact.modules.join('、') || '无'}</dd>
      <dt>意图</dt><dd>{comparison.impact.intents.join('、') || '无'}</dd>
      <dt>接口</dt><dd>{comparison.impact.interfaces.join('、') || '无'}</dd>
      <dt>任务</dt><dd>{comparison.impact.tasks.join('、') || '无'}</dd>
      <dt>文件</dt><dd>{comparison.changed_paths.length} 个对象文件</dd>
    </dl>
    <h3>出版</h3>
    <p class="panel-description">后续基线须经提案：由受托者在终端提交，再由你在“提案”中审阅并批准。App 不代为提交提案。</p>
    <pre class="document-content">towermd propose proposal.json

{proposalInput(result.draft_id, result)}</pre>
    <button onclick={onProposals}>打开提案</button>
  {/if}
</aside>
