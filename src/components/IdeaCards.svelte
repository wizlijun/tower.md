<script lang="ts">
  import type { IdeaEntry } from '../lib/api'
  import { promotionLabel, signerLabel } from '../lib/ideas'
  // FR-183: an idea waits here, outside the attention list, until a person
  // promotes it. Nothing on this list is claimable or delegated.
  let { ideas, onPromote, busy = false, hasGoal = false, hint = null, pending = null, onResume }: {
    ideas: IdeaEntry[]
    onPromote: (idea: IdeaEntry, as: 'project' | 'task') => void
    busy?: boolean
    /** The project already has a goal; a second one is refused. */
    hasGoal?: boolean
    hint?: string | null
    /** An unconfirmed promotion: what went wrong, retried only as sent. */
    pending?: { error: string | null; sending: boolean } | null
    onResume?: () => void
  } = $props()
</script>

{#if pending}
  <div class="idea-pending" role="alert">
    <p><span aria-hidden="true">!</span> {pending.sending ? '正在立项…' : `立项结果尚未确认${pending.error ? `：${pending.error}` : ''}`}</p>
    <button onclick={onResume} disabled={pending.sending}>继续确认立项</button>
  </div>
{/if}
{#if hint}<p class="idea-hint" role="status"><span aria-hidden="true">△</span> {hint}</p>{/if}
<ul class="idea-cards" aria-label="想法">
  {#each ideas as idea (idea.id)}
    <li class="idea-card" class:is-promoted={!!idea.promoted}>
      <span class="idea-card-state">
        {#if idea.promoted}<span aria-hidden="true">✓</span> {promotionLabel(idea)}{:else}<span aria-hidden="true">◇</span> 想法 · 未立项{/if}
      </span>
      <p>{idea.text}</p>
      <span class="idea-card-signer">{signerLabel(idea.added.actor)}</span>
      {#if !idea.promoted}
        <div class="button-row">
          <button onclick={() => onPromote(idea, 'project')} disabled={busy || hasGoal}
            title={hasGoal ? '项目已有目标' : undefined}>立项：作为项目目标</button>
          <button onclick={() => onPromote(idea, 'task')} disabled={busy}>立项：作为任务…</button>
        </div>
      {/if}
    </li>
  {/each}
</ul>
