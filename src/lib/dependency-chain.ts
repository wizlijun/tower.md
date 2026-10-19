import type { ProgressTask } from './task-progress';
/** Longest unresolved dependency chain by task count, never a duration or schedule forecast. */
export function dependencyChain(tasks:ProgressTask[],allTasks:ProgressTask[]=tasks):{ids:string[];diagnostics:string[]}{
 const unresolved=new Map(tasks.filter(task=>task.state!=='merged').map(task=>[task.id,task]));
 const known=new Map(allTasks.map(task=>[task.id,task]));const diagnostics=new Set<string>();const memo=new Map<string,string[]>();const visiting=new Set<string>();let cyclic=false;
 function chain(id:string):string[]{
  const cached=memo.get(id);if(cached)return cached;
  if(visiting.has(id)){cyclic=true;diagnostics.add('依赖存在循环，不能认证最长未完成依赖链。');return [];}
  visiting.add(id);const task=unresolved.get(id)!;let before:string[]=[];
  if(Array.isArray(task.depends_on))for(const dependency of task.depends_on){if(typeof dependency!=='string')continue;const actual=known.get(dependency);
   if(!actual){diagnostics.add(`任务 ${id} 的依赖 ${dependency} 不在已读取记录中。`);continue;}
   if(!unresolved.has(dependency)){if(actual.state!=='merged')diagnostics.add(`任务 ${id} 依赖模块外未完成任务 ${dependency}，此处仅显示模块内链。`);continue;}
   const candidate=chain(dependency);if(candidate.length>before.length||(candidate.length===before.length&&candidate.join('\0')<before.join('\0')))before=candidate;
  }
  visiting.delete(id);const result=[...before,id];memo.set(id,result);return result;
 }
 let longest:string[]=[];for(const id of [...unresolved.keys()].sort()){const candidate=chain(id);if(candidate.length>longest.length)longest=candidate;}
 return {ids:cyclic?[]:longest,diagnostics:[...diagnostics]};
}
