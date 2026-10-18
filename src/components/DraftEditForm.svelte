<script lang="ts">
  import { boundObject, rawRecord } from '../lib/canvas-flow';
  import type { SavedDraft, DraftEdit, AnnotationNode } from '../lib/draft';
  let { saved, busy = false, selected = '', edge = false, connection, onedit }: { saved: SavedDraft; busy?: boolean; selected?: string; edge?: boolean; connection?: {from:string;to:string}|null; onedit:(edit:DraftEdit)=>void } = $props();
  let action = $state('put_annotation'); let kind = $state<'text'|'link'|'group'>('text');
  let id = $state(''); let text = $state(''); let x = $state(0); let y = $state(0); let width = $state(320); let height = $state(180);
  let from = $state(''); let to = $state(''); let title = $state('');
  const nodes = $derived(saved.canvas_view.nodes.filter(entry=>!entry.opaque).flatMap(entry=>{const raw=rawRecord(entry.raw);return raw&&typeof raw.id==='string'?[raw]:[]}));
  const selectedRaw = $derived(rawRecord((edge?saved.canvas_view.edges:saved.canvas_view.nodes).find(entry=>rawRecord(entry.raw)?.id===selected)?.raw));
  const binding = $derived(selectedRaw ? boundObject(selectedRaw) : undefined);
  const canAnnotation = $derived(!edge && !!selectedRaw && !binding && ['text','link','group'].includes(String(selectedRaw.type)) && !saved.canvas_view.nodes.find(entry=>rawRecord(entry.raw)?.id===selected)?.opaque);
  const canRelation = $derived(edge && !!selectedRaw && !binding && !saved.canvas_view.edges.find(entry=>rawRecord(entry.raw)?.id===selected)?.opaque);
  $effect(()=>{if(connection){action='put_relation';from=connection.from;to=connection.to;id='';text='';}});
  function editSelected(){
    if(!selectedRaw)return;
    id=selected; x=Number(selectedRaw.x);y=Number(selectedRaw.y);width=Number(selectedRaw.width);height=Number(selectedRaw.height);
    if(canAnnotation){action='put_annotation';kind=selectedRaw.type as typeof kind;text=String(selectedRaw.text??selectedRaw.url??selectedRaw.label??'');}
    else if(canRelation){action='put_relation';from=String(selectedRaw.fromNode);to=String(selectedRaw.toNode);text=String(selectedRaw.label??'');}
    else if(binding){action='rename';id=binding;title=saved.object_views.find(object=>object.id===binding)?.title??'';}
  }
  function submit(event:SubmitEvent){event.preventDefault();if(busy)return;
    const nodeId=id||`note-${crypto.randomUUID()}`;
    if(action==='put_annotation'){
      const content:AnnotationNode['content']=kind==='text'?{type:'text',text}:kind==='link'?{type:'link',url:text}:{type:'group',label:text};
      onedit({action:'put_annotation',node:{id:nodeId,x:Math.round(x),y:Math.round(y),width:Math.round(width),height:Math.round(height),content}});
    }else if(action==='put_relation')onedit({action:'put_relation',relation:{id:id||`edge-${crypto.randomUUID()}`,from,to,label:text}});
    else if(action==='rename')onedit({action:'rename',object:id,title});
    else if(action==='move')onedit({action:'move',node:id,x:Math.round(x),y:Math.round(y)});
    else if(action==='add_docref')onedit({action:'add_docref',node:nodeId,path:text,x:Math.round(x),y:Math.round(y)});
  }
</script>
<section class="draft-editor" aria-label="有限草稿编辑">
  <h3>编辑草稿 · 不改变当前基线</h3>
  {#if selected}<p>已选：<code>{selected}</code>{binding?` · 对象 ${binding}`:''}</p><button class="secondary" type="button" disabled={busy||(!canAnnotation&&!canRelation&&!binding)} onclick={editSelected}>编辑选中条目</button>{/if}
  <form onsubmit={submit}>
    <label>操作<select value={action} onchange={event=>action=event.currentTarget.value} disabled={busy}><option value="put_annotation">文本 / 链接 / 分组</option><option value="put_relation">普通关系</option><option value="rename">对象改名</option><option value="move">移动节点</option><option value="add_docref">引用已有 Markdown</option></select></label>
    <label>{action==='rename'?'对象 ID':action==='put_relation'?'关系 ID':'节点 ID'}<input bind:value={id} disabled={busy} placeholder={action==='put_annotation'||action==='put_relation'?'留空生成新 ID':''} required={action==='rename'||action==='move'}/></label>
    {#if action==='put_annotation'}<label>卡片类型<select value={kind} onchange={event=>kind=event.currentTarget.value as typeof kind} disabled={busy}><option value="text">文本</option><option value="link">链接</option><option value="group">分组</option></select></label>{/if}
    {#if action==='put_relation'}<label>起点<select value={from} onchange={event=>from=event.currentTarget.value} required disabled={busy}><option value="">选择节点</option>{#each nodes as node}<option value={String(node.id)}>{String(node.id)}</option>{/each}</select></label><label>终点<select value={to} onchange={event=>to=event.currentTarget.value} required disabled={busy}><option value="">选择节点</option>{#each nodes as node}<option value={String(node.id)}>{String(node.id)}</option>{/each}</select></label>{/if}
    {#if action==='rename'}<label>新标题<input bind:value={title} required disabled={busy}/></label>
    {:else if action!=='move'}<label>{action==='add_docref'?'固定来源中的 Markdown 路径':action==='put_relation'?'关系标签':kind==='link'?'链接 URL':kind==='group'?'分组名称':'卡片文本'}<textarea bind:value={text} disabled={busy} required={action==='add_docref'||(action==='put_annotation'&&kind==='link')}></textarea></label>{/if}
    {#if action!=='rename'&&action!=='put_relation'}<div class="draft-fields"><label>X<input type="number" step="1" bind:value={x} required disabled={busy}/></label><label>Y<input type="number" step="1" bind:value={y} required disabled={busy}/></label></div>{/if}
    {#if action==='put_annotation'}<div class="draft-fields"><label>宽<input type="number" min="1" step="1" bind:value={width} required disabled={busy}/></label><label>高<input type="number" min="1" step="1" bind:value={height} required disabled={busy}/></label></div>{/if}
    <button disabled={busy} type="submit">保存此编辑到草稿</button>
  </form>
  {#if canAnnotation}<button class="secondary" type="button" disabled={busy} onclick={()=>onedit({action:'remove_annotation',node:selected})}>删除此注释</button>{/if}
  {#if canRelation}<button class="secondary" type="button" disabled={busy} onclick={()=>onedit({action:'remove_relation',edge:selected})}>删除此普通关系</button>{/if}
  <p class="status">文件节点、真实接口和不透明条目不会通过注释操作修改或删除。验证命令不会在编辑时执行。</p>
</section>
