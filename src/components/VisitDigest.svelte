<script lang="ts">
  import type { ChangeSummary } from '../lib/attention'
  let { heading, lines, details, changes = [], onOpen, onClose }: {
    heading: string
    lines: [string, string, string]
    details: string[]
    /** FR-139: change summaries in the window, shown as sent. */
    changes?: ChangeSummary[]
    /** Opens the full attention list. */
    onOpen: () => void
    onClose: () => void
  } = $props()
</script>

<section class="visit-digest" aria-label="自上次查看以来">
  <div class="visit-digest-heading">
    <strong>{heading}</strong>
    <button class="close-button" aria-label="关闭摘要" onclick={onClose}>×</button>
  </div>
  <ol>{#each lines as line}<li>{line}</li>{/each}</ol>
  {#if changes.length}
    <section class="change-summary" aria-label="变化摘要">
      {#each changes as change (change.from + change.target)}
        <p>变化 · {change.target}（{change.from} 提供）</p>
        <ul>{#each change.lines as line}<li>{line}</li>{/each}</ul>
      {/each}
    </section>
  {/if}
  {#if details.length}
    <details><summary>展开明细（{details.length}）</summary>
      <ul>{#each details as line}<li>{line}</li>{/each}</ul>
    </details>
  {/if}
  <button class="subtle-button" onclick={onOpen}>查看关注清单</button>
</section>
