---
type: Guide
status: draft
---

# Tower.md 会话技能

这些技能面向用户已打开的 harness 会话。Tower.md 不启动模型，不修改全局 harness 配置；用户按自己的 harness 规则加载本目录各技能文件夹，也可在会话中明确要求 Agent 阅读相应 `SKILL.md`。先让该会话能从 `PATH` 调用当前构建的 `towermd`，不要混用旧版 CLI 与新技能。

| 技能 | 作用 | 当前入口 |
| --- | --- | --- |
| [towermd-init](towermd-init/SKILL.md) | 初始化与交接冷启动 | `init --docs-dir` |
| [towermd-baseline](towermd-baseline/SKILL.md) | S6 固定提交综合草稿 | `baseline scan/schema/template/submit/show` |
| [towermd-claim](towermd-claim/SKILL.md) | 领取、绑定运行与检查点收消息 | `status`、`task show/claim/release` |
| [towermd-ask](towermd-ask/SKILL.md) | 向 App 提问、等待或挂起 | `ask`、`ask --wait`（亦支持 `wait`） |
| [towermd-propose](towermd-propose/SKILL.md) | 整理慢层与越界提案 | `propose <input.json>`；批准和驳回在 App |
| [towermd-finish](towermd-finish/SKILL.md) | G0/G2/G3/G5、验证证据、提交回执 | `check`、`finish`、`trailers` |

首次流程是 init → 提交启用配置 → baseline 扫描与综合 → 人在 App 审阅并批准 B0 → 在任务工作树 claim → 开发/ask → finish → trailers → Git 合并与推送 → status 核实 merged。技能不写用户文档对象、快照或 `.towermd/`，这些写入由 CLI 与守护进程处理；普通代码和 Git 提交仍由会话 Agent 完成。

每个检查点调用一次 `towermd status`。回答和插话会先于业务命令结果输出，CLI 在 stdout 成功后才确认送达。消费者应逐条处理消息，不能假设整个 stdout 永远只有一个 JSON 对象。结果未知的写操作保留请求 ID 和原输入重试。

范围以 [spec §18](../docs/specs/2026-10-16-spec-towermd-layered-agent-canvas-mvp.md#18-里程碑与验收) 为准：当前聚焦 M1。`propose`、真实差异审阅与人工批准后的基线出版属于 M1；一句话生长/seed、回答出版为决策、临时规则、G1/G4/G6 是 M2；无头运行、技能包锁定及自动组装是 M3。存在技能文件不代表相应功能已实现或已实机验收。
