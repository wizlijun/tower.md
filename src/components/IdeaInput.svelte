<script lang="ts">
  import { ideaTextError } from '../lib/ideas'
  // FR-181/182: one sentence is enough to start; it becomes an idea card, not a task.
  let { onSubmit, frozen = null, busy = false, error = null, onRetry, label = '一句话写下想做的事' }: {
    /** Resolves `true` once the idea is recorded; the sentence stays otherwise. */
    onSubmit: (text: string) => Promise<boolean> | boolean
    /** A sent sentence whose result is unknown; it can only be retried as is. */
    frozen?: string | null
    busy?: boolean
    error?: string | null
    onRetry: () => void
    label?: string
  } = $props()
  let text = $state('')
  let problem = $derived(ideaTextError(text))

  async function submit(event: SubmitEvent) {
    event.preventDefault()
    if (problem || busy || frozen) return
    if (await onSubmit(text.trim())) text = ''
  }
</script>

<form class="idea-input" aria-label="记下一个想法" onsubmit={submit}>
  <label for="idea-text">{label}</label>
  <div class="idea-input-row">
    <input id="idea-text" type="text" placeholder="例如：做一个把会议录音转成待办的命令行工具"
      value={frozen ?? text} readonly={!!frozen} disabled={busy}
      oninput={(event) => { text = (event.currentTarget as HTMLInputElement).value }} />
    {#if frozen}
      <button type="button" class="primary-button" onclick={onRetry} disabled={busy}>重试原提交</button>
    {:else}
      <button type="submit" class="primary-button" disabled={busy || !!problem}>记下</button>
    {/if}
  </div>
  {#if text && problem && !frozen}<p class="idea-input-note" role="alert">{problem}</p>{/if}
  {#if error}<p class="idea-input-note" role="alert">{error}</p>{/if}
</form>
