<script lang="ts">
 import {onMount,onDestroy} from 'svelte';
 import {recoveryClient,validRecoveryId,decodeRecoveryView,decodeRebuildReport,type RecoveryClient,type RecoveryView,type RebuildReport} from '../lib/recovery';
 import {failureMessage} from '../lib/draft';
 let {token,client=recoveryClient}:{token:string;client?:RecoveryClient}=$props();
 let entry=$state('');let active=$state<RecoveryView|null>(null);let busy=$state(false);let error=$state('');let result=$state<RebuildReport|null>(null);let now=$state(Date.now());let attempted=$state<string[]>([]);
 let archive=$state<{view:RecoveryView;result:RebuildReport|null;error:string}[]>([]);let generation=0;
 onDestroy(()=>{generation++;});onMount(()=>{const timer=setInterval(()=>now=Date.now(),1000);return()=>clearInterval(timer);});
 const expired=$derived(active!==null&&now>=active.request.expires_at);
 const canConfirm=$derived(active!==null&&active.can_confirm&&!attempted.includes(active.request.request_id)&&!expired&&!busy&&!result);
 function keepAudit(){if(active)archive=[...archive,{view:active,result,error}];active=null;result=null;error='';}
 function changed(value:string){entry=value;if(active&&value!==active.request.request_id)keepAudit();}
 async function load(){if(busy)return;const id=entry.trim();if(!validRecoveryId(id)){error='请粘贴 CLI 输出的 64 位小写十六进制 request_id。';return;}
  keepAudit();entry=id;const g=++generation;busy=true;
  try{const loaded=decodeRecoveryView(await client.load(token,id),id);if(g!==generation)return;active=loaded;now=Date.now();}
  catch(failure){if(g===generation)error=failureMessage(failure);}finally{if(g===generation)busy=false;}
 }
 async function confirm(){if(!active||!canConfirm)return;const id=active.request.request_id;const g=++generation;busy=true;error='';attempted=[...attempted,id];
  try{const report=decodeRebuildReport(await client.confirm(token,id));if(g!==generation)return;result=report;}
  catch(failure){if(g===generation)error=failureMessage(failure);}finally{if(g===generation)busy=false;}
 }
</script>
<section class="recovery-card" aria-label="本地 hub 恢复">
 <h2>恢复本地 hub</h2>
 <p>在所选项目中运行 <code>towermd hub rebuild</code>，粘贴输出的 request_id 后读取。命令只生成请求；载入和倒计时均不会执行恢复。</p>
 <label>已登记的恢复请求 ID<input value={entry} disabled={busy} oninput={(event)=>changed(event.currentTarget.value)} placeholder="64 位小写十六进制 request_id" autocomplete="off" spellcheck={false}/></label>
 <button type="button" class="secondary" disabled={busy} onclick={load}>读取原恢复请求</button>
 {#if busy}<p role="status">正在处理已登记的原恢复请求…</p>{/if}
 {#if error}<p class="draft-diagnostic" role="alert">{error}</p><p>不会自动重试确认。来源变化、过期或恢复边界已持久但安装失败时，须运行 CLI 取得新的请求 ID；旧请求保留审计。</p>{/if}
 {#if active}
  <section aria-label="原恢复请求详情">
   <p>确认身份：<strong>{active.human_actor}</strong></p>
   <dl class="review-binding">{#each Object.entries(active.request) as [key,value]}<dt>{key}</dt><dd><code>{value}</code></dd>{/each}</dl>
   <p>原有效期：{new Date(active.request.requested_at).toISOString()} 至 {new Date(active.request.expires_at).toISOString()}。刷新只读取原期限。</p>
   {#if expired}<p role="status">原恢复请求已过期；须运行 CLI 取得新请求和新 ID。</p>{:else}<p>距原期限约 {Math.max(0,Math.ceil((active.request.expires_at-now)/1000))} 秒。服务端会再次核对期限与来源。</p>{/if}
   {#if !active.can_confirm||attempted.includes(active.request.request_id)}<p>此 ID 已尝试确认，当前卡片只保留审计；不会以旧 ID 再安装。</p>{/if}
   {#if result}<section class="recovery-result" aria-label="恢复结果"><h3>恢复已完成</h3><p>恢复边界提交：<code>{result.commit}</code></p><p>seq：{result.seq} · epoch：{result.epoch}</p></section>{/if}
   <button type="button" disabled={!canConfirm} onclick={confirm}>确认恢复此请求</button>
  </section>
 {:else if !busy}<p>尚未读取已登记的请求，不能确认恢复。</p>{/if}
 {#if archive.length}<section aria-label="原恢复请求审计"><h3>已查看的原请求 · 只读</h3>{#each archive as old}<details><summary>{old.view.request.request_id} · 原期限 {old.view.request.expires_at}</summary><pre>{JSON.stringify(old.view.request,null,2)}</pre>{#if old.error}<p>{old.error}</p>{/if}{#if old.result}<pre>{JSON.stringify(old.result,null,2)}</pre>{/if}</details>{/each}</section>{/if}
</section>
