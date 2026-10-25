<script lang="ts">
  // Appendix A.2 "Save as Rule": a stereotype or color the template does not map only decorates
  // until a person writes it into the frame's rules as a state. Rules belong to the person, so
  // this goes through the same human-only rules change as the rules editor (L3, `human:local`).
  import type { FrameRules } from '../lib/api'

  let { tokens, states, rules, onSave }: {
    tokens: string[]; states: string[]; rules: FrameRules; onSave: (rules: FrameRules) => void
  } = $props()

  let chosen = $state<Record<string, string>>({})
  let shown = $derived(tokens.filter((token) => !rules.status_map?.[token]).slice(0, 8))

  function save(token: string) {
    const state = chosen[token]
    if (!state) return
    onSave({ ...rules, status_map: { ...(rules.status_map ?? {}), [token]: state } })
  }
</script>

{#if shown.length && states.length}
  <div class="frame-node-source-panel nodrag nopan" role="group" aria-label="未映射的状态写法">
    {#each shown as token (token)}
      <p>
        <code>{token}</code> 只当装饰。当作
        <select aria-label={`${token} 当作的状态`} value={chosen[token] ?? ''} onchange={(event) => (chosen[token] = event.currentTarget.value)}>
          <option value="">选择状态</option>
          {#each states as state (state)}<option value={state}>{state}</option>{/each}
        </select>
        <button type="button" class="subtle-button" disabled={!chosen[token]} onclick={() => save(token)}>写进容器规则</button>
      </p>
    {/each}
  </div>
{/if}
