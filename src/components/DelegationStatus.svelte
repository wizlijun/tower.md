<script lang="ts">
  import type { DelegationControls } from '../lib/delegation'
  import { actionObject } from '../lib/delegation'
  let { controls }: { controls: DelegationControls } = $props()
  const actionLabels: Record<string, string> = {
    finalize: '定稿委托', used: '记下我用过', confirm: '确认验收', send_back: '退回', rule: '记为规则',
    rename: '工作类别改名', level: '设定委托级别', signal: '处理提醒', decision: '记为决策', ask: '提问', withdraw: '撤回请求',
  }
</script>

{#if controls.error}<p role="alert">{controls.error}</p>{/if}
{#if controls.pending}
  <div class="delegation-pending">
    <p class="panel-description">{actionLabels[controls.pending.kind]}{actionObject(controls.pending) ? ` · ${actionObject(controls.pending)}` : ''}：提交结果尚待确认。重试沿用原请求号和全部原输入。</p>
    <button class="primary-button" disabled={controls.busy} onclick={() => { if (!controls.busy) controls.onRetry() }}>{controls.busy ? '正在确认…' : '重试原提交'}</button>
  </div>
{:else if controls.result}
  <p class="verification-status verification-passed" role="status">✓ {controls.result}</p>
{/if}
