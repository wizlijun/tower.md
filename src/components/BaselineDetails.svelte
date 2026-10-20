<script lang="ts">
  import type { BaselineDraft, PublicationPreview } from '../lib/api'
  import VerificationDetails from './VerificationDetails.svelte'
  import { baselineReport, baselineEvidenceError } from '../lib/verification'
  let { draft, publication = null }: { draft: BaselineDraft; publication?: PublicationPreview | null } = $props()
</script>

{#if publication}
  <dl class="source-list" aria-label="批准预览">
    <dt>目标基线</dt><dd>{publication.target}</dd>
    <dt>内容摘要</dt><dd><code>{publication.content_hash}</code></dd>
    <dt>来源提交</dt><dd><code>{publication.source_revision}</code></dd>
  </dl>
{/if}
<p class="panel-description">综合草稿 · 未批准。{draft.review ? '原始综合来自提交者自报，当前草稿已含下方署名的人工修订' : '结构与解释来自提交者自报'}；读取草稿不会执行命令或修改当前基线。真实验证来自固定提交的命令执行记录。</p>
{#if draft.stale}<p class="verification-status verification-unverifiable">! 来源提交已过期：当前 main 已改变。此草稿只供查看，不能批准。</p>{/if}
<dl class="source-list">
  {#if draft.review}
    <dt>综合来源（自报）</dt><dd>{draft.producer}</dd>
  {:else}
    <dt>提交者（自报）</dt><dd>{draft.producer}</dd>
    <dt>提交时间</dt><dd>{draft.created_at}</dd>
  {/if}
  <dt>来源提交</dt><dd><code>{draft.submission.source_revision}</code></dd>
  <dt>关联扫描</dt><dd>{draft.submission.scan_id}</dd>
  <dt>草稿</dt><dd>{draft.id}</dd>
</dl>
{#if draft.review}
  <h3>人工审阅</h3>
  <dl class="source-list"><dt>审阅者</dt><dd>{draft.review.actor}</dd><dt>{draft.review.verification ? '修订保存时间' : '本次审阅时间'}</dt><dd>{draft.created_at}</dd><dt>修改次数</dt><dd>{draft.review.actions.length}</dd><dt>原草稿</dt><dd>{draft.review.base_id}</dd></dl>
  {#each draft.review.modules as module}
    <p class="panel-description">{module.title} · {module.commands_confirmed ? '命令已确认' : '命令待确认'}{module.verification_pending ? ' · 待重新验证' : ''}</p>
  {/each}
{/if}
<h3>待回答的问题</h3>
{#if draft.submission.questions.length}
  <ul class="diagnostics">{#each draft.submission.questions as question}<li>{question}</li>{/each}</ul>
{:else}<p class="panel-description">提交者没有列出问题。</p>{/if}
<h3>风险</h3>
{#if draft.submission.risks.length}
  <ul class="diagnostics">{#each draft.submission.risks as risk}<li>{risk}</li>{/each}</ul>
{:else}<p class="panel-description">提交者没有列出风险，不代表没有风险。</p>{/if}
{#if draft.review?.verification}<dl class="source-list"><dt>当前验证输入</dt><dd><code>{draft.review.verification.input_sha256}</code></dd></dl>{/if}
<VerificationDetails historical={!draft.review?.verification && !!draft.review?.pending_modules.length} report={baselineReport(draft)} evidenceError={baselineEvidenceError(draft)} />
