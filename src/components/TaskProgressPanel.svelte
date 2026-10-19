<script lang="ts">
 import {taskProgressClient,progressStateLabel,evidenceResultLabel,type TaskProgressClient,type TaskProgressView} from '../lib/task-progress';
 import {failureMessage} from '../lib/draft';
 let {token,client=taskProgressClient}=$props<{token:string;client?:TaskProgressClient}>();
 let view=$state<TaskProgressView|null>(null);
 let expanded=$state(false);
 let busy=$state(false);
 let error=$state('');
 let generation=0;
 const visibleTasks=$derived(view?(expanded?view.tasks:view.tasks.slice(0,3)):[]);
 $effect(()=>{token;generation++;view=null;expanded=false;error='';busy=false;});
 async function load(){
  const serial=++generation;const project=token;
  busy=true;error='';view=null;expanded=false;
  try{const result=await client.load(project);if(serial===generation&&project===token)view=result;}
  catch(failure){if(serial===generation&&project===token)error=failureMessage(failure);}
  finally{if(serial===generation&&project===token)busy=false;}
 }
</script>

<section class="task-progress-panel" aria-label="任务进度">
 <div class="task-progress-heading"><div><h3>任务进度</h3><p>读取真实任务、运行和证据记录。手动刷新，不续租或推进任务。</p></div><button type="button" disabled={busy} onclick={()=>void load()}>{view?'刷新任务进度':'读取任务进度'}</button></div>
 {#if busy}<p role="status">正在核对任务记录与来源…</p>{/if}
 {#if error}<p role="alert">{error}</p>{/if}
 {#if view}
  <p class="task-progress-source">{view.state==='recovery_required'?'◇ 快照历史 · 需恢复':view.state==='recorded'?'● 已核实的任务记录':'◇ 暂无任务记录'}{#if view.through_seq!==null} · 截至 seq {view.through_seq}{/if}<span>读取时间：{new Date(view.captured_at).toLocaleString()}</span></p>
  {#each view.diagnostics as diagnostic}<p class="status">{diagnostic}</p>{/each}
  {#if view.tasks.length===0}<p>尚无可展示的任务。读取不会创建任务或初始化项目。</p>{:else}
   <ul class="task-progress-list">
    {#each visibleTasks as task (task.id)}
     <li class="task-progress-item">
      <div class="task-progress-task"><strong>{typeof task.title==='string'?task.title:task.id}</strong><span class="task-progress-state">{progressStateLabel(task.state)}</span></div>
      <p><code>{task.id}</code>{#if typeof task.module==='string'} · 模块 {task.module}{/if}{#if view.state==='recovery_required'} · 历史记录{/if}</p>
      {#each view.runs.filter(run=>run.task===task.id) as run (run.id)}<p class="task-progress-run">运行 <code>{run.id}</code> · {progressStateLabel(run.state)}{#if typeof run.host==='string'} · {run.host}{/if}</p>{/each}
      {#each view.evidence.filter(evidence=>evidence.task===task.id) as evidence (evidence.id)}
       <p class="task-progress-evidence">证据 <code>{evidence.id}</code> · {evidenceResultLabel(evidence.result)}{#if typeof evidence.commit==='string'} · 提交 <code>{evidence.commit.slice(0,12)}</code>{/if}</p>
      {/each}
     </li>
    {/each}
   </ul>
   {#if view.tasks.length>3}<button type="button" class="secondary" onclick={()=>expanded=!expanded}>{expanded?'收起列表':`查看全部（${view.tasks.length}）`}</button>{/if}
  {/if}
 {/if}
</section>
