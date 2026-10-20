<script lang="ts">
  import type { ScanReport, VerificationStatus } from '../lib/api'
  import { statusLabels, type CardStatus } from '../lib/verification'
  let { report, moduleId, running = false, evidenceError }: { report: ScanReport; moduleId?: string; running?: boolean; evidenceError?: string | null } = $props()
  let verification = $derived(report.verification)
  let moduleResult = $derived(verification?.modules.find((result) => result.module === moduleId))
  let cleanupFailed = $derived(!!verification && !verification.cleanup_complete && !running)
  function displayStatus(status: VerificationStatus): CardStatus { return cleanupFailed ? 'cleanup_failed' : status }
  let commands = $derived(verification?.commands.filter((command) => !moduleId || command.modules.includes(moduleId)) ?? [])
</script>

{#if evidenceError}
  <h3>验证证据不可复核</h3>
  <p class="verification-status verification-unverifiable">! 无法确认原验证结果，不显示通过状态。</p>
  <p class="panel-description">{evidenceError}</p>
{:else if verification}
  <h3>实际验证结果</h3>
  {#if cleanupFailed}<p class="verification-status verification-cleanup_failed">! 清理失败，不能接受通过结果。</p>{/if}
  {#if moduleResult}
    <p class={`verification-status verification-${displayStatus(moduleResult.status)}`}>
      <span aria-hidden="true">{statusLabels[displayStatus(moduleResult.status)].symbol}</span> {statusLabels[displayStatus(moduleResult.status)].label}
      {#if moduleResult.shared}<span> · 共享命令结果</span>{/if}
    </p>
    {#if moduleResult.status === 'no_tests'}
      <p class="panel-description">此模块未发现可执行的验证命令。候选任务建议：为 {report.modules.find((module) => module.id === moduleId)?.title || moduleId} 补充测试。</p>
    {/if}
  {/if}
  {#each commands as command}
    <section class="verification-command" aria-label={`验证命令 ${command.id}`}>
      <p class={`verification-status verification-${displayStatus(command.status)}`}><span aria-hidden="true">{statusLabels[displayStatus(command.status)].symbol}</span> {statusLabels[displayStatus(command.status)].label}</p>
      <dl class="source-list">
        <dt>目录</dt><dd>{command.directory || '.'}</dd>
        <dt>参数</dt><dd><code>{JSON.stringify(command.argv)}</code></dd>
        <dt>退出码</dt><dd>{command.exit_code ?? '未取得'}</dd>
        <dt>耗时</dt><dd>{command.duration_ms} ms</dd>
        <dt>覆盖模块</dt><dd>{command.modules.join('、')}{command.modules.length > 1 ? ' · 共享执行' : ''}</dd>
        {#if command.log}<dt>日志路径</dt><dd>{command.log}</dd>{/if}
        {#if command.log_sha256}<dt>日志 SHA</dt><dd><code>{command.log_sha256}</code></dd>{/if}
      </dl>
      {#if command.error}<p class="panel-description">{command.error}</p>{/if}
      {#if command.log_excerpt}<pre class="document-content">{command.log_excerpt}</pre>{/if}
      {#if command.log_truncated}<p class="panel-description">本机日志已按存储上限截断；上方仅显示开头摘要。</p>{/if}
    </section>
  {:else}<p class="panel-description">尚无命令执行结果；未运行不代表通过。</p>{/each}
  <dl class="source-list">
    <dt>执行者</dt><dd>{verification.actor}</dd>
    <dt>开始</dt><dd>{verification.started_at}</dd>
    <dt>结束</dt><dd>{verification.finished_at || '尚未结束'}</dd>
    <dt>清理</dt><dd>{verification.cleanup_complete ? '已完成' : running ? '尚未完成' : '失败'}</dd>
    <dt>提交</dt><dd>{verification.revision}</dd>
    <dt>源码树</dt><dd>{verification.tree}</dd>
  </dl>
{:else}
  <h3>候选验证命令 · 尚未验证（尚无执行结果）</h3>
  {#each report.commands.filter((command) => !moduleId || command.modules.includes(moduleId)) as command}
    <div class="scan-command"><span>{command.directory || '.'}</span><code>{command.command}</code></div>
  {:else}<p class="panel-description">没有发现验证命令；尚无验证结果。</p>{/each}
{/if}
