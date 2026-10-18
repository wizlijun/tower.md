<script lang="ts">
  import { onDestroy } from 'svelte';
  import { candidateClient, isDraft, type CandidateClient, type CandidateDraft, type ScanStatus } from '../lib/candidate';
  let { token, client = candidateClient, onloaded }: { token: string; client?: CandidateClient; onloaded: (draft: CandidateDraft) => void } = $props();
  let status = $state<ScanStatus | null>(null);
  let busy = $state(false);
  let error = $state('');
  let ready = $state(false);
  let request = 0;
  onDestroy(() => { request++; });
  const labels: Record<string, string> = { queued: '已排队', running: '正在扫描与验证', publishing: '正在记录结果', pending: '扫描仍在进行', completed: '扫描完成', failed: '扫描失败', uncertain: '记录结果尚不确定', interrupted: '扫描已中断', cancelled: '扫描已取消' };
  async function observe(serial: number, scanId: string) {
    const result = await client.wait(token, scanId);
    if (serial !== request) return;
    status = result;
    if (result.state === 'completed') {
      const draft = await client.draft(token, scanId);
      if (serial !== request) return;
      status = draft;
      if (isDraft(draft)) { ready = true; onloaded(draft); }
    }
  }
  async function start() {
    if (busy || status) return;
    const serial = ++request;
    busy = true; error = '';
    try {
      const result = await client.start(token);
      if (serial !== request) return;
      status = result;
      await observe(serial, result.scan_id);
    } catch (failure) { if (serial === request) error = message(failure); }
    finally { if (serial === request) busy = false; }
  }
  async function wait() {
    if (busy || !status) return;
    const serial = ++request;
    busy = true; error = '';
    try { await observe(serial, status.scan_id); }
    catch (failure) { if (serial === request) error = message(failure); }
    finally { if (serial === request) busy = false; }
  }
  function message(failure: unknown) {
    if (typeof failure === 'object' && failure && 'message' in failure) {
      const detail = 'detail' in failure && failure.detail ? `\n${String(failure.detail)}` : '';
      return String(failure.message) + detail;
    }
    return String(failure);
  }
</script>

<section class="candidate-control" aria-label="生成候选画布">
  <div><strong>{ready ? '候选画布 · 未批准' : '生成真实候选画布'}</strong><p>{ready ? '展示固定提交的候选模块与真实验证声明；编辑、保存和批准尚未接入。' : '仅点击后扫描固定提交并运行发现的验证命令，不启动模型，不批准基线。'}</p></div>
  {#if !status}<button type="button" disabled={busy} onclick={start}>{busy ? '正在启动…' : '生成候选画布'}</button>
  {:else}<div class="candidate-progress"><span role="status">{busy ? '正在等待真实扫描结果…' : labels[status.state] ?? status.state}</span><code>{status.scan_id}</code>{#if !ready}<button type="button" class="secondary" disabled={busy} onclick={wait}>继续等待</button>{/if}</div>{/if}
  {#if status?.diagnostic}<p class="candidate-diagnostic">{status.diagnostic}</p>{/if}
  {#if error}<p class="candidate-diagnostic" role="alert">{error}</p>{/if}
  {#if status && !ready && !busy}<p class="candidate-diagnostic">继续等待使用同一扫描 ID，不重新执行测试。切换项目只停止本窗口等待，后台扫描继续。</p>{/if}
</section>
