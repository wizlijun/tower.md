import {expect,test} from 'vitest';
import {dependencyChain} from './dependency-chain';
import type {ProgressTask} from './task-progress';
const task=(id:string,depends_on:string[]=[],state='ready'):ProgressTask=>({type:'Task',id,state,depends_on});
test('只按真实未合并任务与依赖计算最长链，验证或自述done不冒充合并',()=>{const tasks=[task('A',[],'merged'),task('B',['A'],'verifying'),task('C',['B']),task('D')];expect(dependencyChain(tasks).ids).toEqual(['B','C']);expect(dependencyChain([task('D',[],'ended')]).ids).toEqual(['D']);});
test('循环不给假关键链，缺失或模块外依赖明确诊断',()=>{const cycle=dependencyChain([task('A',['B']),task('B',['A'])]);expect(cycle.ids).toEqual([]);expect(cycle.diagnostics[0]).toContain('循环');const local=[task('A',['missing','EXTERNAL'])];const result=dependencyChain(local,[...local,task('EXTERNAL')]);expect(result.diagnostics).toHaveLength(2);expect(result.ids).toEqual(['A']);});
