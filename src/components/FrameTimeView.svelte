<script lang="ts">
  // FR-159: planned against actual on a time axis, or how a map's cards were
  // put forward and changed. Derived from the events on every read; it writes
  // and moves nothing.
  import type { FrameDeviation, FrameEvolution, FrameEvolutionChange, FramePlanned } from '../lib/api'
  import type { FrameReader } from '../lib/frames'

  type Props = { frame: string; view: 'plan_deviation' | 'argument_evolution'; reader: FrameReader; onClose: () => void }
  let { frame, view, reader, onClose }: Props = $props()

  let deviation = $state<FrameDeviation | null>(null)
  let evolution = $state<FrameEvolution | null>(null)
  let error = $state<string | null>(null)
  const title = $derived(view === 'plan_deviation' ? '计划与实际' : '演变')

  $effect(() => {
    const [id, name] = [frame, view]
    let current = true
    deviation = null
    evolution = null
    error = null
    const read = name === 'plan_deviation'
      ? reader.deviation?.(id).then((read) => { if (current) deviation = read.deviation })
      : reader.evolution?.(id).then((read) => { if (current) evolution = read.evolution })
    void read?.catch((cause) => { if (current) error = `读取${name === 'plan_deviation' ? '计划偏差' : '演变'}失败：${String(cause)}` })
    return () => { current = false }
  })

  const days = (count: number) => `${Math.abs(count)} 天`
  /** Shape and words, never colour alone. */
  function outcome(card: FramePlanned): string {
    if (card.done !== null) {
      const by = card.deviation_days ?? 0
      return by > 0 ? `✓ 晚 ${days(by)}` : by < 0 ? `✓ 早 ${days(by)}` : '✓ 按期'
    }
    if (card.overdue_days !== null && card.overdue_days > 0) return `▲ 逾期 ${days(card.overdue_days)}`
    return card.plan === null ? '○ 未定日期' : '○ 未到期'
  }
  const changeLabels: Record<FrameEvolutionChange, string> = {
    proposed: '提出', renamed: '改写', state: '改状态', moved: '挪动', linked: '连线', unlinked: '断开', archived: '归档', restored: '恢复',
  }
  function detail(step: FrameEvolution['steps'][number]): string {
    if (step.change === 'proposed' || step.change === 'state') return step.state ?? ''
    if (step.change === 'linked' || step.change === 'unlinked') return step.other ? `↔ ${step.other}` : ''
    if (step.change === 'moved') return step.other ? `→ ${step.other}` : '→ 顶层'
    return ''
  }
</script>

<div class="frame-node-flow frame-time-view nodrag nopan" role="group" aria-label={title}>
  {#if deviation}
    <p class="frame-flow-day" role="status">按字段 {deviation.field} · 截至 {deviation.today}</p>
    {#if deviation.cards.length}
      <div class="frame-flow-table">
        <table>
          <thead><tr><th>卡片</th><th>最初</th><th>现在</th><th>完成</th><th>结果</th></tr></thead>
          <tbody>
            {#each deviation.cards as card (card.card)}
              <tr title={(card.replans ?? []).map((replan) => `${replan.date}：${replan.from ?? '无'} → ${replan.to ?? '无'}`).join('\n') || undefined}>
                <td>{card.title}</td><td>{card.planned ?? '—'}</td>
                <td>{card.plan ?? '—'}{card.slip_days ? `（改期 ${card.slip_days > 0 ? '+' : '−'}${days(card.slip_days)}）` : ''}</td>
                <td>{card.done ?? '—'}</td><td>{outcome(card)}</td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    {:else}<span>还没有卡片。</span>{/if}
  {:else if evolution}
    {#if evolution.omitted}<p class="frame-flow-day" role="status">只列最近的 {evolution.steps.length} 步，更早的 {evolution.omitted} 步略去。</p>{/if}
    {#if evolution.steps.length}
      <div class="frame-flow-table">
        <table>
          <thead><tr><th>日期</th><th>卡片</th><th>变化</th><th></th><th>谁</th></tr></thead>
          <tbody>
            {#each [...evolution.steps].reverse() as step, index (index)}
              <tr><td>{step.date}</td><td title={step.card}>{step.title}</td><td>{changeLabels[step.change]}</td><td>{detail(step)}</td><td>{step.by}</td></tr>
            {/each}
          </tbody>
        </table>
      </div>
    {:else}<span>还没有变化。</span>{/if}
  {:else if !error}<span>正在计算…</span>{/if}
  {#if error}<span class="frame-node-history-error" role="alert">{error}</span>{/if}
  <button type="button" class="subtle-button" onclick={onClose}>关闭</button>
</div>
