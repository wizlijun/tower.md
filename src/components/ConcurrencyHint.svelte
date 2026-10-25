<script lang="ts">
  // FR-31: parallel delegates keep interrupting the person, so a lower
  // `runner.concurrency` is worth considering. A reminder only: Tower.md
  // never edits config.yaml, and nothing here changes the limit.
  import type { ConcurrencyHint } from '../lib/api'
  let { hint }: { hint: ConcurrencyHint | null | undefined } = $props()
</script>

{#if hint?.suggest && hint.reason}
  <p class="panel-description" role="note" aria-label="并发提示">
    <span aria-hidden="true">⇣</span> 并发提示：{hint.reason}
  </p>
{/if}
