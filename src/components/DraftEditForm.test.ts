import {cleanup,fireEvent,render,screen} from '@testing-library/svelte';import {afterEach,expect,test,vi} from 'vitest';import DraftEditForm from './DraftEditForm.svelte';import {savedFixture} from '../lib/draft.fixture';
afterEach(cleanup);
async function choose(element:HTMLElement,value:string){const select=element as HTMLSelectElement;select.selectedIndex=Array.from(select.options).findIndex(option=>option.value===value||(option as HTMLOptionElement&{__value?:unknown}).__value===value);await fireEvent.change(select);}

test('free text is a typed annotation and preserves the complete original document',async()=>{
 const saved=savedFixture();const before=JSON.stringify(saved);const onedit=vi.fn();render(DraftEditForm,{saved,onedit});
 await fireEvent.input(screen.getByRole('textbox',{name:'节点 ID'}),{target:{value:'new-note'}});await fireEvent.input(screen.getByRole('textbox',{name:'卡片文本'}),{target:{value:'<script>仅原文</script>'}});await fireEvent.click(screen.getByRole('button',{name:'保存此编辑到草稿'}));
 expect(onedit).toHaveBeenCalledWith({action:'put_annotation',node:{id:'new-note',x:0,y:0,width:320,height:180,content:{type:'text',text:'<script>仅原文</script>'}}});expect(JSON.stringify(saved)).toBe(before);
});
test('ordinary relations keep real node IDs and file bindings never gain annotation deletion',async()=>{
 const saved=savedFixture();const onedit=vi.fn();const view=render(DraftEditForm,{saved,onedit,selected:'A-module'});
 expect(screen.queryByRole('button',{name:'删除此注释'})).toBeNull();await fireEvent.click(screen.getByRole('button',{name:'编辑选中条目'}));expect((screen.getByRole('combobox',{name:'操作'}) as HTMLSelectElement).value).toBe('rename');
 await choose(screen.getByRole('combobox',{name:'操作'}),'put_relation');await fireEvent.input(screen.getByRole('textbox',{name:'关系 ID'}),{target:{value:'note-to-module'}});await choose(screen.getByRole('combobox',{name:'起点'}),'note');await choose(screen.getByRole('combobox',{name:'终点'}),'A-module');await fireEvent.input(screen.getByRole('textbox',{name:'关系标签'}),{target:{value:'反向关系'}});await fireEvent.click(screen.getByRole('button',{name:'保存此编辑到草稿'}));expect(onedit).toHaveBeenLastCalledWith({action:'put_relation',relation:{id:'note-to-module',from:'note',to:'A-module',label:'反向关系'}});
 view.unmount();saved.canvas_view.edges[0].raw={id:'relation',fromNode:'A-module',toNode:'note',towermd:{obj:'IF-real'}};render(DraftEditForm,{saved,onedit,selected:'relation',edge:true});expect(screen.queryByRole('button',{name:'删除此普通关系'})).toBeNull();
});
test('opaque annotation-shaped raw is retained but cannot be edited or removed',()=>{
 const saved=savedFixture();saved.canvas_view.nodes[1].opaque=true;render(DraftEditForm,{saved,onedit:vi.fn(),selected:'note'});expect(screen.queryByRole('button',{name:'删除此注释'})).toBeNull();expect((screen.getByRole('button',{name:'编辑选中条目'}) as HTMLButtonElement).disabled).toBe(true);
});
