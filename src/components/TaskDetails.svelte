<script lang="ts">
  import type { RuntimeTask, RuntimeRun } from '../lib/runtime'
  import type { RunDetails } from '../lib/api'
  import VerificationDetails from './VerificationDetails.svelte'
  import { costLabel, taskGraph, taskStateLabels } from '../lib/runtime'
  import ProjectCanvas from './ProjectCanvas.svelte'
  let { tasks, runs, positions, scope, onPrompt, runDetails = {}, onRunDetails, detailsError, detailsLoading = null }: {
    tasks: RuntimeTask[]
    runs: RuntimeRun[]
    positions: Map<string, { x: number; y: number }>
    scope: string
    onPrompt?: (task: string, text?: string) => void
    runDetails?: Record<string, RunDetails>
    onRunDetails?: (id: string) => void
    detailsError?: string
    detailsLoading?: string | null
  } = $props()
  let selectedId = $state<string | null>(null)
  let showGraph = $state(false)
  let openedRun = $state<string | null>(null)
  const eventLabels: Record<string, string> = {
    'task.created': '任务创建', 'task.claimed': '领取任务', 'task.released': '释放任务',
    'task.finish.requested': '请求验证', 'task.finish.started': '验证开始',
    'task.finish.completed': '验证结果已记录', 'task.finish.interrupted': '验证中断',
    'task.g5.completed': '合并前检查已记录', 'task.merged': 'main 合并已认证',
    'hold.opened': '等待人工回答', 'hold.answered': '人工回答已记录',
    'hold.decision_reused': '引用已有决策', 'prompt.queued': '人工插话已记录',
    'project.paused': '项目暂停与检查点指令已记录', 'project.resumed': '人工解除项目暂停',
    'prompt.delivered': '消息已送达', 'lease.granted': '取得模块租约',
    'lease.expired': '模块租约到期', 'lease.released': '模块租约已释放',
    'recovery.completed': '恢复完成，旧运行权限已失效',
  }
  function loadRun(id: string) { openedRun = id; onRunDetails?.(id) }
  let selected = $derived(tasks.find((task) => task.input.id === selectedId))
  let graph = $derived(taskGraph(tasks, positions, scope))
  let taskRuns = $derived(selected ? runs.filter((run) => run.task === selected.input.id) : [])
</script>

{#if !tasks.length}
  <p class="panel-description">当前模块尚无任务。</p>
{:else}
  <button onclick={() => { showGraph = !showGraph }}>{showGraph ? '收起任务依赖图' : '任务依赖图'}</button>
  {#if showGraph}
    <div class="task-graph" aria-label="任务依赖图">
      <ProjectCanvas view={graph} onOpen={(node) => { selectedId = node.id }} />
    </div>
  {/if}
  <div class="draft-list" aria-label="真实任务列表">
    {#each tasks as task (task.input.id)}
      <button onclick={() => { selectedId = task.input.id }} aria-pressed={selectedId === task.input.id}>
        <strong>{task.input.title}</strong>
        <span>{task.input.id} · {taskStateLabels[task.state]}{task.needs_review ? ' · 需复核基线' : ''}</span>
      </button>
    {/each}
  </div>
  {#if selected}
    <h3>{selected.input.title}</h3>
    {#if onPrompt}
      <button onclick={() => onPrompt?.(selected!.input.id)}>向任务插话</button>
      <button onclick={() => onPrompt?.(selected!.input.id, '请在下一个安全检查点暂停当前任务并汇报进度。保留当前工作；继续执行前先等待明确的恢复指令，并检查租约仍有效。')}>请求暂停任务</button>
      <button onclick={() => onPrompt?.(selected!.input.id, '请在下一个回合结束后汇报当前任务：已完成的变更、实际验证证据、阻塞问题与下一步。自述完成不能替代 finish 和 main 合并认证。')}>要求汇报</button>
    {/if}
    <dl class="source-list">
      <dt>状态</dt><dd>{taskStateLabels[selected.state]}</dd>
      <dt>基线</dt><dd>{selected.baseline}</dd>
      <dt>模块</dt><dd>{selected.input.module}</dd>
      <dt>授权路径</dt><dd>{(selected.input.allowed_paths ?? []).join('、') || '未声明'}</dd>
      <dt>依赖</dt><dd>{(selected.input.depends_on ?? []).join('、') || '无'}</dd>
      <dt>验收</dt><dd>{(selected.input.acceptance ?? []).join('；') || '未声明'}</dd>
    </dl>
    {#if selected.needs_review}<p class="verification-status verification-pending">! 基线已变化，需要复核任务。请先释放旧运行，按新批准基线创建新任务并声明依赖，再重新领取和验证；旧记录保留。</p>{/if}
    {#if !taskRuns.length}<p class="panel-description">暂无运行记录；费用未知（未报告）。</p>{/if}
    {#each taskRuns as run (run.binding.id)}
      <h3>运行 {run.binding.id}</h3>
      <dl class="source-list">
        <dt>状态</dt><dd>{run.state === 'active' ? '进行中' : '已释放'}</dd>
        <dt>主机</dt><dd>{run.binding.host}</dd>
        <dt>工作树</dt><dd>{run.binding.worktree}</dd>
        <dt>分支</dt><dd>{run.binding.branch}</dd>
        <dt>费用</dt><dd>{costLabel(run.cost_usd)}</dd>
        {#if run.binding.self_reported}<dt>执行器</dt><dd>{run.binding.self_reported.name}/{run.binding.self_reported.version}（自报）</dd>{/if}
      </dl>
      {#if onRunDetails}<button onclick={() => loadRun(run.binding.id)}>查看时间线与验证证据 · {run.binding.id}</button>{/if}
      {#if openedRun === run.binding.id}
        {#if detailsLoading === run.binding.id}<p class="panel-description" role="status">正在读取已接受的运行原件…</p>{/if}
        {#if detailsError}<p class="panel-description" role="alert">{detailsError}</p>{/if}
        {@const details = runDetails[run.binding.id]}
        {#if details && details.run_id === run.binding.id && details.task_id === selected.input.id}
          <section aria-label={`运行时间线 ${run.binding.id}`}>
            <h3>运行时间线</h3>
            <ol>
              {#each details.events as event (event.eid)}
                <li>
                  <strong>{eventLabels[event.kind] ?? event.kind}</strong>
                  {#if event.stale}<span> · 已过期，不参与进度</span>{/if}
                  <p class="panel-description">#{event.seq} · {event.at} · {event.actor}</p>
                  <p class="panel-description">{event.object} · 原件 {event.eid}</p>
                </li>
              {:else}<li>尚无可读取的运行事件。</li>{/each}
            </ol>
            <p class="panel-description">当前未采集 harness 原始输出；下方内容仅来自已冻结的验证报告。</p>
          </section>
          <section aria-label={`问答与插话原文 ${run.binding.id}`}>
            <h3>问答与插话原文</h3>
            <p class="panel-description">以下是已接受的原文记录；慢层变更仍须批准，记录本身不表示已生效或已送达。</p>
            {#each details.messages as message (`${message.kind}-${message.id}`)}
              <h4>{message.kind === 'question' ? '问题' : message.kind === 'answer' ? '回答' : '插话'} · {message.id}</h4>
              <p class="panel-description">{message.at} · {message.actor}</p>
              <pre class="document-content">{message.text}</pre>
            {:else}<p class="panel-description">当前运行尚无问答或插话原文。</p>{/each}
          </section>
          {#each details.evidence as evidence (evidence.completion_eid)}
            <section aria-label={`验证证据 ${evidence.id}`}>
              <h3>验证证据 {evidence.id}</h3>
              <p class={`verification-status verification-${evidence.passed ? 'passed' : 'failed'}`}>{evidence.passed ? '✓ 验证通过' : '! 验证未通过'} · 不等同于任务已合并</p>
              <dl class="source-list">
                <dt>完成原件</dt><dd>{evidence.completion_eid}</dd>
                <dt>报告 SHA</dt><dd><code>{evidence.report_sha256}</code></dd>
              </dl>
              <VerificationDetails report={{ verification: evidence.report, modules: [], commands: [] }} />
            </section>
          {:else}<p class="panel-description">尚无已接受的完整验证报告；未运行不代表通过。</p>{/each}
        {/if}
      {/if}
    {/each}
  {/if}
{/if}
