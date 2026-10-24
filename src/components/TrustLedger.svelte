<script lang="ts">
  import type { TrackRecord } from '../lib/api'
  import type { DelegationControls } from '../lib/delegation'
  import { allowsLabel, delegateLabel, formatCount, formatMeasure, formatRatio, levelSourceLabels, localTime } from '../lib/delegation'
  import DelegationStatus from './DelegationStatus.svelte'
  let { controls }: { controls: DelegationControls } = $props()
  let view = $derived(controls.view)
  let locked = $derived(controls.disabled || controls.busy || Boolean(controls.pending))
  let levels = $derived(view.settings.delegation_levels)
  let kinds = $derived([...new Set([...view.track_record.map((row) => row.work_kind), ...view.objects.map((item) => item.work_kind)])].sort())
  let chosen = $state<Record<string, string>>({})
  let projectLevel = $state('')
  let renameFrom = $state('')
  let renameTo = $state('')
  let renameValid = $derived(Boolean(renameFrom && renameTo.trim() && renameTo.trim() !== renameFrom && renameTo.trim() !== '*'
    && new TextEncoder().encode(renameTo.trim()).length <= 256))

  const rowKey = (row: TrackRecord) => `${row.config}\u0000${row.work_kind}`
  function setLevel(config: string, kind: string, level: string) {
    if (!level || locked) return
    controls.onAction({ kind: 'level', data: { config, kind, level } })
  }
  function rename() {
    if (!renameValid || locked) return
    controls.onAction({ kind: 'rename', data: { from: renameFrom, to: renameTo.trim() } })
    renameTo = ''
  }
</script>

<p class="panel-description">每行是一个受托者配置在一个工作类别上的记录，没有总分。未接入的指标显示“未接入”或“—”，不当作 0。委托级别只由人设定，建议不会自动生效。验收时长只计 App 内。</p>
<DelegationStatus {controls} />

{#if !view.track_record.length}
  <p class="panel-description">还没有完成的委托记录。</p>
{/if}
{#each view.track_record as row (rowKey(row))}
  <section class="ledger-row" aria-label="{row.work_kind} · {row.config}">
    <h3>{row.work_kind} · {delegateLabel(row.delegate)}</h3>
    {#if row.config_changed}<p class="verification-status verification-pending">! 配置已变化，建议提高抽查比例</p>{/if}
    <dl class="source-list ledger-metrics">
      <dt>配置</dt><dd><code>{row.config}</code></dd>
      <dt>记录</dt><dd>{row.records}</dd>
      <dt>一次通过</dt><dd>{formatRatio(row.first_pass)}</dd>
      <dt>按时</dt><dd>{formatRatio(row.on_time)}</dd>
      <dt>达到完成标准</dt><dd>{formatRatio(row.met_done_when)}</dd>
      <dt>越界</dt><dd>{formatCount(row.out_of_bounds)}</dd>
      <dt>兜圈子</dt><dd>{formatCount(row.loops)}</dd>
      <dt>削弱标准</dt><dd>{formatCount(row.weakened)}</dd>
      <dt>交付时长</dt><dd>{formatMeasure(row.delegate_to_deliver_ms)}</dd>
      <dt>验收时长</dt><dd>{formatMeasure(row.review_ms)}</dd>
      <dt>退回</dt><dd>{formatMeasure(row.sent_back, String)}</dd>
      <dt>抽查比例</dt><dd>{Math.round(row.spot_check_rate * 100)}%</dd>
      <dt>委托级别</dt><dd>{row.level}（来源：{levelSourceLabels[row.level_source]}）</dd>
      <dt>可代做</dt><dd>{allowsLabel(row.allows)}</dd>
      {#if row.suggestion}<dt>建议</dt><dd>{row.suggestion.direction === 'up' ? '↑ 可升至' : '↓ 宜降至'} {row.suggestion.level}（需人设定）</dd>{/if}
    </dl>
    {#if levels.length}
      <div class="button-row">
        <select aria-label="委托级别 {row.work_kind} · {row.config}" disabled={locked} value={chosen[rowKey(row)] ?? row.suggestion?.level ?? row.level}
          onchange={(event) => { chosen[rowKey(row)] = event.currentTarget.value }}>
          {#each levels as level}<option value={level}>{level}</option>{/each}
        </select>
        <button type="button" disabled={locked} onclick={() => setLevel(row.config, row.work_kind, chosen[rowKey(row)] ?? row.suggestion?.level ?? row.level)}>设定级别</button>
      </div>
    {/if}
  </section>
{/each}

{#if levels.length}
  <section class="ledger-row" aria-label="项目级委托级别">
    <h3>项目级委托级别</h3>
    <p class="panel-description">没有更具体设定时，所有配置与类别都用它；未设定时用清单第一项。</p>
    <div class="button-row">
      <select aria-label="项目级委托级别" disabled={locked} bind:value={projectLevel}>
        <option value="">选择级别</option>
        {#each levels as level}<option value={level}>{level}</option>{/each}
      </select>
      <button type="button" disabled={locked || !projectLevel} onclick={() => setLevel('*', '*', projectLevel)}>设定级别</button>
    </div>
  </section>
{/if}

{#if kinds.length}
  <form class="draft-edit-form" aria-label="工作类别改名或合并" onsubmit={(event) => { event.preventDefault(); rename() }}>
    <fieldset class="draft-edit-fields" disabled={locked}>
      <h3>工作类别改名或合并</h3>
      <label>原类别<select bind:value={renameFrom}>
        <option value="">选择类别</option>
        {#each kinds as kind}<option value={kind}>{kind}</option>{/each}
      </select></label>
      <label>改为<input bind:value={renameTo} placeholder="新名字；填已有类别即合并" /></label>
      <div class="button-row"><button type="submit" disabled={!renameValid || locked}>改名</button></div>
    </fieldset>
  </form>
{/if}

{#if view.configs.length}
  <details class="sample-history">
    <summary>受托者配置历史</summary>
    {#each view.configs as history (history.name)}
      <p><strong>{history.name}</strong></p>
      <ol class="diagnostics">
        {#each history.configs as entry (entry.config)}
          <li>{delegateLabel(entry.delegate)} · 首次 {localTime(entry.first.at)} · {entry.delegations} 次委托</li>
        {/each}
      </ol>
    {/each}
  </details>
{/if}

{#if view.rules.length}
  <details class="sample-history">
    <summary>规则（{view.rules.length}）</summary>
    <ul class="diagnostics">
      {#each view.rules as rule (rule.id)}<li>{rule.work_kind}：{rule.text}{rule.source ? '（来自一次退回）' : ''}</li>{/each}
    </ul>
  </details>
{/if}
