export type DemoNode = {
  id: string;
  layer: string;
  title: string;
  description: string;
  status: string;
  tone: 'goal' | 'module' | 'running' | 'review' | 'done';
  symbol: string;
  x: number;
  y: number;
  facts: [string, string][];
  width?: number;
  height?: number;
  content?: string;
  kind?: string;
};

// 固定演示数据：不读取项目，不代表已批准基线或真实执行结果。
export const demoNodes: DemoNode[] = [
  { id: 'G-01', layer: 'L0 · 目标', title: '交付第一个可用版本', description: '让 Agent 推进，让人只在关键处放行。', status: '演示目标', tone: 'goal', symbol: '◎', x: 40, y: 250,
    facts: [['范围', '本地优先的工程驾驶舱'], ['验收', '看清结构、进展与需要关注的事']] },
  { id: 'M-01', layer: 'L1 · 模块', title: '画布与项目导航', description: '把目标、模块和任务放在同一张图上。', status: '演示基线', tone: 'module', symbol: '◇', x: 370, y: 130,
    facts: [['目录', 'src/ · src-tauri/'], ['边界', '结构变更需要人工批准']] },
  { id: 'M-02', layer: 'L1 · 模块', title: 'Agent 执行与证据', description: '独立推进任务，用可追溯证据说明进度。', status: '演示基线', tone: 'module', symbol: '◇', x: 370, y: 390,
    facts: [['目录', 'crates/towermd-core/'], ['原则', '进度只认证据，不认自述']] },
  { id: 'T-01', layer: 'L5 · 任务', title: '构建可交互画布', description: '展示节点关系，支持平移、缩放和详情。', status: '进行中', tone: 'running', symbol: '◷', x: 710, y: 40,
    facts: [['执行者', '演示 Agent A'], ['下一步', '连接真实项目投影']] },
  { id: 'T-02', layer: 'L5 · 任务', title: '确认首版交付范围', description: '这一项需要人来决定，Agent 等待放行。', status: '待人确认', tone: 'review', symbol: '△', x: 710, y: 250,
    facts: [['待决定', '首版是否只包含核心工作流'], ['边界', '此处仅演示，不提交真实批准']] },
  { id: 'T-03', layer: 'L5 · 任务', title: '验证本地快照恢复', description: '演示已附带测试证据的完成状态。', status: '证据齐备', tone: 'done', symbol: '✓', x: 710, y: 460,
    facts: [['证据', '示例测试记录，非真实执行结果'], ['费用', '未知（演示）']] },
];

export const demoEdges = [['G-01', 'M-01'], ['G-01', 'M-02'], ['M-01', 'T-01'], ['M-01', 'T-02'], ['M-02', 'T-03']];
