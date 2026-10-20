<script lang="ts">
  import { untrack } from 'svelte'
  import { SAMPLE_LABELS, SAMPLE_NODE_IDS, SAMPLE_QUESTION, type SampleAction, type SampleState } from '../lib/sample'
  import type { CanvasNode } from '../lib/api'
  let { state: sampleState, node, onAction }: { state: SampleState; node: CanvasNode; onAction: (action: SampleAction) => void } = $props()
  let answer = $state(untrack(() => sampleState.answer ?? ''))
</script>

<p class="panel-description">功能示例 · 仅使用内存中的示例数据。这里的确认、验证和放行都是模拟，不写入仓库，也不执行命令。</p>

{#if node.id === SAMPLE_NODE_IDS.baseline}
  <h3>基线组件</h3>
  <p class="panel-description">先确认示例中的三个模块及关系，任务才能领取。真实基线批准尚未接通。</p>
  <p class="verification-status">示例状态：{SAMPLE_LABELS.baseline[sampleState.baseline]}</p>
  <button disabled={sampleState.baseline !== 'draft'} onclick={() => onAction({ type: 'approve_baseline' })}>确认示例基线</button>
{:else if node.id === SAMPLE_NODE_IDS.task}
  <h3>任务组件</h3>
  <p class="panel-description">示例 Agent 领取任务后，会提出一个需要人回答的问题。此操作不会启动真实 Agent。</p>
  <p class="verification-status">示例状态：{SAMPLE_LABELS.task[sampleState.task]}</p>
  <button disabled={sampleState.task !== 'ready'} onclick={() => onAction({ type: 'start_task' })}>领取示例任务</button>
  {#if sampleState.task === 'blocked'}<p class="panel-description">先确认示例基线。</p>{/if}
  {#if sampleState.task === 'done'}<p class="panel-description">示例流程已完成；真实代码合并与发布尚未接通。</p>{/if}
{:else if node.id === SAMPLE_NODE_IDS.question}
  <h3>问答组件</h3>
  <p>{SAMPLE_QUESTION}</p>
  <p class="verification-status">示例状态：{SAMPLE_LABELS.question[sampleState.question]}</p>
  {#if sampleState.question === 'answered'}
    <p class="panel-description">已记录的示例回答：</p><pre class="document-content">{sampleState.answer}</pre>
    <p class="panel-description">退回后保留回答，避免重复询问。</p>
  {:else}
    <form class="draft-edit-form" onsubmit={(event) => { event.preventDefault(); onAction({ type: 'answer', text: answer }) }}>
      <label>示例回答<textarea bind:value={answer} required rows="4" disabled={sampleState.question !== 'open'}></textarea></label>
      <button type="submit" disabled={sampleState.question !== 'open' || !answer.trim()}>记录示例回答</button>
    </form>
    {#if sampleState.question === 'blocked'}<p class="panel-description">领取示例任务后会出现问题。</p>{/if}
  {/if}
{:else if node.id === SAMPLE_NODE_IDS.verification}
  <h3>验证组件</h3>
  <p class="panel-description">选择一个模拟结果以查看门禁行为。真实验证必须执行命令并绑定提交，不能由这里的选项替代。</p>
  <p class="verification-status">示例结果：{SAMPLE_LABELS.verification[sampleState.verification]}</p>
  <div class="button-row">
    <button disabled={sampleState.task !== 'running' || sampleState.question !== 'answered'} onclick={() => onAction({ type: 'verify', result: 'passed' })}>模拟通过</button>
    <button disabled={sampleState.task !== 'running' || sampleState.question !== 'answered'} onclick={() => onAction({ type: 'verify', result: 'failed' })}>模拟失败</button>
    <button disabled={sampleState.task !== 'running' || sampleState.question !== 'answered'} onclick={() => onAction({ type: 'verify', result: 'no_tests' })}>模拟无测试</button>
  </div>
  {#if sampleState.verification === 'failed' || sampleState.verification === 'no_tests'}<p class="panel-description">此结果不能放行。可重新选择模拟结果。</p>{/if}
{:else if node.id === SAMPLE_NODE_IDS.review}
  <h3>人工放行组件</h3>
  <p class="panel-description">只有示例验证通过才能放行；退回后需重新验证。此处不会批准真实任务，也不会合并代码。</p>
  <p class="verification-status">示例状态：{SAMPLE_LABELS.review[sampleState.review]}</p>
  <div class="button-row">
    <button disabled={sampleState.review !== 'pending'} onclick={() => onAction({ type: 'approve_review' })}>放行示例结果</button>
    <button disabled={sampleState.review !== 'pending'} onclick={() => onAction({ type: 'reject_review' })}>退回示例结果</button>
  </div>
{:else}
  <h3>模块组件</h3>
  <pre class="document-content">{node.content}</pre>
  <p class="panel-description">模块关系固定在这张画布。示例中的状态变化不会移动节点。</p>
{/if}

<details class="sample-history">
  <summary>示例操作记录（{sampleState.history.length}）</summary>
  {#if sampleState.history.length}<ol class="diagnostics">{#each sampleState.history as item}<li>{item.message}</li>{/each}</ol>
  {:else}<p class="panel-description">尚未操作。先打开基线卡片。</p>{/if}
</details>
