---
type: Guide
status: draft
---

# Tower.md 会话技能

这些技能面向用户已打开的 harness 会话。Tower.md 不启动模型，不修改全局 harness 配置；用户按自己的 harness 规则加载本目录各技能文件夹，也可在会话中明确要求 Agent 阅读相应 `SKILL.md`。先让该会话能从 `PATH` 调用当前构建的 `towermd`，不要混用旧版 CLI 与新技能。

| 技能 | 作用 | 当前入口 |
| --- | --- | --- |
| [towermd-init](towermd-init/SKILL.md) | 初始化与交接冷启动 | `init --docs-dir` |
| [towermd-baseline](towermd-baseline/SKILL.md) | S6 固定提交综合草稿 | `dev scan start/wait/show`、`dev draft template/submit/show`、`schema dev.draft.submit` |
| [towermd-claim](towermd-claim/SKILL.md) | 读委托卡、先澄清、领取、绑定运行、检查点收消息、回应汇报请求与主动汇报上报 | `status`、`task show/claim/release`、`dev clarify`、`reply`、`report`、`signal` |
| [towermd-ask](towermd-ask/SKILL.md) | 向 App 提问、等待或挂起 | `ask`、`wait` |
| [towermd-propose](towermd-propose/SKILL.md) | 整理慢层与越界提案 | `propose <input.json>`；批准和驳回在 App |
| [towermd-finish](towermd-finish/SKILL.md) | 交付前自检、G0/G2/G3/G5、逐条自述、验证证据、提交回执 | `dev check`、`dev finish --claims`、`dev trailers` |

首次流程是 init → 提交启用配置 → baseline 扫描与综合 → 人在 App 审阅并批准 B0 →（人选了先澄清时）clarify 后等人定稿 → 在任务工作树 claim → 开发/ask/reply/report/signal → 自检后 finish → trailers → Git 合并与推送 → status 核实 merged。技能不写用户文档对象、快照或 `.towermd/`，这些写入由 CLI 与守护进程处理；普通代码和 Git 提交仍由会话 Agent 完成。

每个检查点调用一次 `towermd status`。stdout 只有一个 JSON 信封（`v`、`ok`、`data` 或 `error`、`delivered`、`next`、`warnings`）。回答和插话放在 `delivered[]`，先逐条处理它们，再读 `data`；CLI 在 stdout 写完后才确认送达。退出码 5 表示暂时没有结果：如果有 `next`，按 `next` 继续等；没有 `next`（`deferred`）就结束回合，不要忙等，回答会出现在之后调用的 `delivered[]` 里。只有 `error.retry.safe` 为真（等待、租约被占用、守护进程或 hub 暂时不可用这类瞬时情况）才原样重试，写命令沿用 `error.retry.request_id` 和原输入；`safe` 为假时只回显请求 ID，原样重试不会成功，先按 `error`、`next` 处理原因。

范围以 [spec §18](../docs/specs/2026-10-16-spec-towermd-layered-agent-canvas-mvp.md#18-里程碑与验收) 为准：当前聚焦 M1。`propose`、真实差异审阅与人工批准后的基线出版属于 M1；一句话生长/seed、回答出版为决策、临时规则、G1/G4/G6 是 M2；无头运行、技能包锁定及自动组装是 M3。存在技能文件不代表相应功能已实现或已实机验收。
