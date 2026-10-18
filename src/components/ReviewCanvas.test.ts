import {cleanup,fireEvent,render,screen} from '@testing-library/svelte';
import {afterEach,expect,test} from 'vitest';
import ReviewCanvas from './ReviewCanvas.svelte';
import {savedFixture} from '../lib/draft.fixture';
import type {CanvasComparison} from '../lib/review';
afterEach(cleanup);
test('comparison labels both interface sides including deletion and unknown module diagnostics',async()=>{
 const baseline=savedFixture();const draft=savedFixture();draft.canvas_view.nodes=draft.canvas_view.nodes.filter(entry=>entry.index!==0);
 const comparison:CanvasComparison={state:'available',main_revision:'f'.repeat(40),baseline:'towermd/B0',baseline_canvas:baseline,draft_canvas:draft,diagnostics:['保留服务端诊断']};
 render(ReviewCanvas,{comparison,interfaceModules:['A-module','missing-module']});
 expect(screen.getByText('◆ A-module：主干已高亮；草稿无可显示节点')).toBeTruthy();
 expect(screen.getByText('◆ missing-module：主干无可显示节点；草稿无可显示节点')).toBeTruthy();
 expect(screen.getByText(/找不到 Interface 影响模块：missing-module/)).toBeTruthy();
 expect(screen.getByText('f'.repeat(40))).toBeTruthy();
 await fireEvent.click(screen.getByRole('button',{name:'主干'}));expect(screen.getByRole('button',{name:'主干'}).getAttribute('aria-pressed')).toBe('true');
 await fireEvent.click(screen.getByRole('button',{name:'草稿'}));expect(screen.getByRole('button',{name:'草稿'}).getAttribute('aria-pressed')).toBe('true');
 expect(screen.getByText('◆ A-module：主干已高亮；草稿无可显示节点')).toBeTruthy();
});
test('unavailable and empty native canvases show reasons and raw diagnostics without fallback nodes',()=>{
 const empty={canvas_view:{nodes:[],edges:[],diagnostics:[{message:'非法条目原文保留'}]},object_views:[]};
 const panel=render(ReviewCanvas,{comparison:{state:'available',main_revision:'e'.repeat(40),baseline:null,baseline_canvas:null,draft_canvas:empty,diagnostics:[]},interfaceModules:[]});
 expect(screen.getByText('权威 main 尚无画布；主干视图为空，不以目录图替代。')).toBeTruthy();
 expect(screen.getByText(/此视图没有可显示节点/)).toBeTruthy();expect(screen.getByText(/非法条目原文保留/)).toBeTruthy();panel.unmount();
 render(ReviewCanvas,{comparison:{state:'unavailable',reason:'权威读取变化，请重新加载'},interfaceModules:[]});
 expect(screen.getByText('画布对照不可用：权威读取变化，请重新加载')).toBeTruthy();expect(screen.queryByRole('group',{name:'画布对照模式'})).toBeNull();
});
