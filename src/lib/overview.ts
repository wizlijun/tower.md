import type { DemoNode } from './demo';
import type { ProjectOverview } from './project';

const statuses: Record<string, string> = { draft: '目录预览', bound: '对象引用', reference: '文档引用', annotation: '注释', dangling: '引用缺失', mismatch: '绑定不匹配', unsafe: '路径不可读', opaque: '未知节点' };
export function overviewNodes(view: ProjectOverview): DemoNode[] {
  return view.nodes.map(node => ({
    ...node,
    layer: view.source === 'repository' ? (node.kind === 'root' ? 'Git 项目' : 'Git 目录') : '仓库画布',
    description: view.source === 'repository' ? '固定提交中的目录结构预览，不代表已批准的模块划分。' : '来自权威 main 的画布，只读展示。',
    status: statuses[node.status] ?? node.status,
    tone: node.kind === 'root' ? 'goal' : ['unsafe', 'dangling', 'mismatch', 'opaque'].includes(node.status) ? 'review' : 'module',
    symbol: ['unsafe', 'dangling', 'mismatch', 'opaque'].includes(node.status) ? '△' : '◇',
    content: node.text,
    facts: [['来源提交', view.revision], ['类型', node.kind]],
  }));
}
