---
type: Verification
title: 固定提交扫描与候选画布验收
---

# 范围与源码

在用户保留的基本框架上继续，起点 `01874ad`，实现提交为 `4d2d30c91f31099250d926a4816909dbdcb09c78`。本次交付真实仓库 → S1–S4 确定性候选 → App 只读查看这一段，不代表完整 M0、M1 或全部 spec 完成。

核心读取固定权威提交的文件、清单与最近 64 个非合并提交；按目录构造模块候选，用共同修改历史修正边界。生成实际 OKF Module 文件和 JSON Canvas，并列出源码覆盖率、热点、历史关系、候选验证命令及补测建议。没有执行仓库命令，没有产生验证证据或 B0。

App 经打包的 CLI 启动或连接本用户唯一 daemon；CLI 和 App 共用项目服务。报告只存 `<git-common-dir>/towermd/drafts/<scan-id>/`，可删除后重建。未修改用户文档、工作树、暂存区、main 或用户全局配置。

# 验证

- 工作区 Rust 完整回归：188 项通过。
- 真机发现三节点同行连线遮挡及合并模块遗漏根验证命令后，修正初始布局和命令归属；最终冷启动专项 9 项、daemon 集成 4 项再次通过。
- 原生端 4 项测试通过，包括选择取消、旧 token、迟到选择与地址约束。后端锁排队结束后再次校验项目 token，过期排队请求不启动扫描。
- 前端 23 项测试通过，类型检查 0 error / 0 warning；覆盖扫描轮询、错误、取消选择、迟到结果、卸载、刷新回 main 和显式恢复旧候选。
- 最终 Vite 生产构建与 Tauri `.app` 打包成功。串行构建、nice 10、Cargo jobs 2；没有并发启动多套构建。仅有既存的 STATIC_VCRUNTIME 弃用提示。
- 缓存删除重建、daemon 重启读取、同 clone 工作树复用、独立 clone 冲突、实例/客户端检查、符号链接拒绝、固定提交和脏工作树隔离均有真实 Git 测试。

# 真机结果

用 Computer Use 操作实际 App 和原生文件夹选择框。验收仓库为当前 Tower.md 框架的独立本地 clone，移除 origin 后仅在该 QA clone 导入配置；未改原开发仓库的配置。

- QA 路径：`/Users/bruce/git/tower.md/.release/cold-start-20261021/qa-project`
- 固定提交：`d3244d7ca5e976bfb15297ec0c8524d8ab62eef0`
- 源码树：`9f7d9d9ba31f83cd3dfc3861ebd2b830304777ef`
- 最终 App 发起的扫描：`01a159dd-c803-72ef-9d36-074722da39c8`
- 结果：识别 40 个源码文件，分配 40 个，3 个模块、3 条历史共同修改关系。这里的 100% 是识别范围内的归属率，不是语义正确率或测试覆盖率。

实际完成：打开项目、空画布启动扫描、候选自动出现、节点原文与固定提交查看、扫描说明、刷新回 main、显式查看原候选、取消切换保留候选。主界面没有常驻侧栏或工具栏；候选有虚线卡片和“候选 · 尚未验证”文字。

中间候选还验证了服务退出 → 刷新报错 → 点击重试恢复，以及重启后 CLI 读取与原报告一致。最终包重新验证了真实扫描、布局、节点与关系显示，并从实际报告确认合并的 `src` / `src-tauri` 模块同时关联根 pnpm test/check 和原生 Cargo 命令。扫描前后 QA 的 main SHA、Git 状态和 index SHA-256 完全一致。

最终保留一个 App 进程和一个配套 daemon。旧候选全部退出，旧文件仅留作本机验收记录。采样时 syspolicyd CPU 为 0%，未改系统安全设置。

# 本机产物与使用

App：`/Users/bruce/git/tower.md/.release/cold-start-20261021/Tower.md.app`

同目录 `manifest.json` 记录实现提交、前端与二进制哈希、QA 提交和扫描 ID；`qa-report-final.json` 保存最终实际报告，构建和回归日志也保存在该目录。

| 文件 | SHA-256 |
| --- | --- |
| `Contents/MacOS/towermd-app` | `044e6d7164699e2a451aeb2dfca6d2b7197664644131bed5ccc4760b0564d196` |
| `Contents/MacOS/towermd` | `b8771a049b6eda1a16f2480b91abf5f65b3683401f38101ae522a586e3ecf052` |

只验收了 macOS Apple Silicon。无发行者签名或公证，保留工具链的 ad-hoc 签名。本次是本机候选，没有正式发布 tag 或远端 release。

当前窗口已经显示候选。重新打开时，选择上述 QA 仓库，点击“扫描仓库”；卡片可查看原文，右键可看扫描说明或返回当前 main 画布。自己的项目需要先将 `.towermd/config.yaml` 提交到权威 main；有 origin 时读取已 fetch 的 origin/main。

# 限制与后续主干

候选只是单层目录启发式，未解析 import，关系只表示共同修改；初始布局是确定性网格，未接 ELK。模块置信度为保守的固定初值，不能视为校准后的概率。候选不能编辑或批准。

仍需完成共享 S5 验证器、S6 Agent 综合/提交、唯一主干写入与快照恢复、App 的 S7/S8 审阅批准，随后才是任务领取、问答、验证与合并闭环。当前没有用按钮、假进度或扫描结果替代这些能力。

提交前检查：仅根 worktree 和 main，无其他本地分支、无 stash。用户原有未跟踪 `.towermd/`、`AGENTS.md`、`CLAUDE.md`、`docs/superpowers/`、`docs/tower/` 未改动或提交。
