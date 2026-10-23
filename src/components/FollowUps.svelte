<script lang="ts">
  import type { FollowUpEntry, FollowUpWhen } from '../lib/api'
  import { followUpTextError, MAX_OPEN_FOLLOW_UPS, whenLabel } from '../lib/follow-ups'
  // FR-134: a person's own reminders with a condition. Only the App adds or
  // completes them; when one is due it joins the attention list as class 4.
  let { followUps, targets, disabled = false, busy = false, error = null, receipt = null, frozen = false, onAdd, onFinish, onRetry }: {
    followUps: FollowUpEntry[]
    /** What a condition can watch: the project first, then each object. */
    targets: { id: string; title: string }[]
    disabled?: boolean
    busy?: boolean
    error?: string | null
    receipt?: string | null
    /** A sent request whose result is unknown; it can only be retried as sent. */
    frozen?: boolean
    /** Resolves `true` once the follow-up is recorded; the form stays otherwise. */
    onAdd: (text: string, when: FollowUpWhen) => Promise<boolean> | boolean
    onFinish: (entry: FollowUpEntry) => void
    onRetry: () => void
  } = $props()
  let text = $state('')
  let on = $state<FollowUpWhen['on']>('date')
  let at = $state('')
  let target = $state('')
  let kind = $state('')
  let open = $derived(followUps.filter((entry) => !entry.done))
  let full = $derived(open.length >= MAX_OPEN_FOLLOW_UPS)
  let title = (id: string) => targets.find((entry) => entry.id === id)?.title ?? id
  let chosen = $derived(target || targets[0]?.id || '')
  let when = $derived.by((): FollowUpWhen | null => {
    if (on === 'date') {
      const parsed = Date.parse(at)
      return Number.isFinite(parsed) ? { on, at: new Date(parsed).toISOString() } : null
    }
    if (!chosen) return null
    if (on === 'signal') return kind.trim() ? { on, target: chosen, kind: kind.trim() } : { on, target: chosen }
    return { on, target: chosen }
  })
  let problem = $derived(followUpTextError(text) ?? (when ? null : on === 'date' ? '选一个到点时间。' : '选一个对象。'))

  async function submit(event: SubmitEvent) {
    event.preventDefault()
    if (problem || !when || busy || disabled || full || frozen) return
    if (await onAdd(text.trim(), when)) { text = ''; at = ''; kind = '' }
  }
</script>

<section class="follow-ups" aria-label="跟进项">
  <h3>跟进项（{open.length}/{MAX_OPEN_FOLLOW_UPS}）</h3>
  {#if open.length}
    <ul class="diagnostics">
      {#each open as entry (entry.id)}
        <li><span aria-hidden="true">⚑</span> {entry.text}<span class="panel-description"> · {whenLabel(entry.when, title)}</span>
          <button {disabled} onclick={() => onFinish(entry)}>完成</button></li>
      {/each}
    </ul>
  {:else}<p class="panel-description">没有未完成的跟进项。</p>{/if}
  <form class="follow-up-form" aria-label="添加跟进项" onsubmit={submit}>
    <input aria-label="跟进什么" placeholder="例如：回头看看周报有没有人读" bind:value={text} disabled={busy || frozen} maxlength="400" />
    <div class="button-row">
      <select aria-label="到点条件" bind:value={on} disabled={busy || frozen}>
        <option value="date">到某个时间</option>
        <option value="done">某个对象完成时</option>
        <option value="activity">某个对象有新动静时</option>
        <option value="signal">某个对象出现信号时</option>
      </select>
      {#if on === 'date'}
        <input type="datetime-local" aria-label="到点时间" bind:value={at} disabled={busy || frozen} />
      {:else}
        <select aria-label="对象" value={chosen} onchange={(event) => { target = (event.currentTarget as HTMLSelectElement).value }} disabled={busy || frozen}>
          {#each targets as entry (entry.id)}<option value={entry.id}>{entry.title}</option>{/each}
        </select>
        {#if on === 'signal'}<input aria-label="信号类别（可选）" placeholder="信号类别（可选）" bind:value={kind} disabled={busy || frozen} />{/if}
      {/if}
    </div>
    <div class="button-row">
      {#if frozen}
        <button type="button" class="primary-button" onclick={onRetry} disabled={busy}>重试原提交</button>
      {:else}
        <button type="submit" class="primary-button" disabled={disabled || busy || full || !!problem}>添加跟进</button>
      {/if}
    </div>
    {#if full}<p class="panel-description">最多 {MAX_OPEN_FOLLOW_UPS} 条未完成的跟进项；先完成一条再加。</p>
    {:else if text && problem}<p class="panel-description" role="alert">{problem}</p>{/if}
    {#if error}<p class="panel-description" role="alert">{error}</p>{/if}
    {#if receipt}<p class="panel-description" role="status">{receipt}</p>{/if}
  </form>
</section>
