# Tower.md

Tower.md 管执行 issue，Note.md 管 AI 知识，两者的关系类比 Jira 与 Confluence。

官网：[towermd.net](https://towermd.net)

Tower.md 是一个本地优先的多 Agent 工程驾驶舱：主界面是无限画布，慢层（目标、架构基线、验收标准）由人批准，快层（任务、运行、提交）由 Agent 通过无头 harness 执行。文本状态存在 git，大文件交给 [Arca](https://github.com/wizlijun/arca)。

## 设计原则

与 Note.md 的五个信念一一对应，详见 [设计原则](docs/product-principles.md)。

1. **Agent 的动作无限，你的注意力有限——放行才是残余。** 默认沉默，首屏只给 3 件事，同一个问题不问第二遍。
2. **仓库高于服务。** 执行状态是代码仓里的纯文本；守护进程、索引、画布都可删除、可重建。
3. **Agent 推进，你放行。** 结构只在人批准处改变；Agent 说完成不算，有证据才算。
4. **你的回答属于仓库，不属于会话。** 人的回答、授权、豁免落为决策、规则或证据，换 Agent、换机器都还在。
5. **一个仓库，多个 harness——你是塔台。** harness 可替换，仓库中立，Tower.md 自己也能被移走。

与 Note.md 之间还有三条接口原则：主张与承诺分开出版；两道人工闸门，每个方向一道；同一套契约，学一次两边用。

## 文档

- [设计原则](docs/product-principles.md)
- [MVP 规格 v0.3](docs/specs/2026-10-16-spec-towermd-layered-agent-canvas-mvp.md)

规格中引用的研究与会议原文位于同级目录 `~/git/sotvault`，链接按相对路径 `../../../sotvault/` 指向那里。
