---
type: Verification
title: S6 Agent 综合、持久草稿与 App 验收
---

# 范围与源码

起点 `5c192a2`，实现提交 `15e7e39d291d310ac5176d9f2fea0f55912305c3`。完成本阶段的 S6 综合提交与画布审阅：会话 Agent 读取固定提交和 S1–S5 报告，通过已保留的 JSON Schema 提交意图、模块、接口、验收、风险和问题。App 默认仍只有画布。

模块 ID、文件边界和验证命令冻结；名称、职责、置信度与理由可综合。通过/失败只取原 S5 实际结果，Agent 输入不能覆盖。提交者是自报的 harness/version，不构成可信身份或人工批准。未知 OKF 字段保留，原始对象文本原样保存。

守护进程用独立临时 index 和 Git plumbing 创建不可变的 `refs/towermd/drafts/<id>`，保存提交输入、扫描原件及可重建对象/画布；不修改用户 index、工作树或 main。相同输入和 producer 幂等；有 origin 时只推送该私有引用，失败保留本地原件供重试。读取列表不联网，跨机器获取草稿尚未接入 App。

这不是完整 M1 或正式发布：尚无 S7 编辑、S8 批准、B0、hub Evidence/快照恢复或任务闭环。未新增依赖，未修改用户全局 Git/harness 配置。

# 自动化与构建

- 工作区 Rust 214 项通过，覆盖 schema/对象绑定、禁止伪造批准、原 S5 证据、真实 Git 持久化、幂等、来源过期、缓存删除与重启、日志缺失，以及 bare origin 仅推送私有引用。
- 原生端 5 项通过，含项目会话绑定与草稿 ID 校验。
- 前端 42 项通过，覆盖草稿列表/详情、迟到响应、项目切换、证据缺失、同草稿重新读取时坐标和视口保持。
- 类型检查零错误、零警告；技能通过 `quick_validate.py`。
- 初次前端回归有一项失败：测试夹具中的扫描 revision 与视图 revision 不一致。修正夹具以符合真实序列化结果，保留严格来源校验；最终全部通过。初次与最终日志均保留。
- 生产 App 构建成功，保留既有 STATIC_VCRUNTIME 弃用提示。工具链串行运行，nice 10、Cargo jobs 2；无重复构建或测试进程。

# 实际 Agent 与实机验收

独立 QA 仓库位于产物目录的 `qa-project`，没有 origin。实际运行随包 CLI 完成扫描和验证；会话中的另一个 Agent 按项目 `towermd-baseline` 技能读取固定提交的 README、源码和 Makefile，生成提交输入，由随包 CLI 提交。producer 诚实记作自报 `codex/unknown`，没有验证模型版本，也不声称无人介入的模型调度。

| 标识 | 值 |
| --- | --- |
| QA 源提交 | `afc377633b9777aa263a7cfddbf86f691a4c988f` |
| 源 tree | `2c22f5a8fbeb2a3b02ecc6b86ffba126f949ba5e` |
| 扫描 ID | `01a15a11-72f9-7300-a8b7-ef9750c19230` |
| 草稿 ID | `d-b4ca073a88462eb525031f7121dff42d752d9f1162d3d84c92da94be33f41b38` |
| 私有草稿提交 | `159cafead2f8c61a7613c12003a6bbfbf23683ce` |

通过 Computer Use 操作本次生产 App 和原生文件夹选择框，实际确认：

1. 选择 QA 项目，空画布打开草稿列表；提交后重新读取，出现未批准草稿。
2. 同一画布显示 3 个 Module、1 个 Intent、2 个 Interface、3 个 Acceptance，共 9 个节点；保留 3 条共同修改关系，新增 2 条有明确标签的实际调用接口。
3. format 故意设置的断言失败、math 的真实测试通过、pipeline 无测试分别显示。综合文本没有将失败或未测试改成通过。
4. 按需综合说明显示来源提交、扫描 ID、自报 producer、4 条风险和 1 个待确认问题，以及真实验证结果；没有常驻侧栏或工具栏。
5. 相同输入重试得到同一草稿且返回内容一致。移走 S1–S5 派生草稿缓存并重启 daemon，仍能逐字恢复原始提交。App 首次访问旧端点报错，点击重新读取后恢复，无需重启 App。
6. 临时移走验证日志后，原文仍能恢复；实机所有模块明确显示“证据不可复核”，不显示可信通过。恢复日志并重新选择同一草稿后恢复真实状态，节点位置和视口保持。
7. QA main、源码树、工作树状态和 index 哈希在前后完全一致；index SHA-256 为 `d05f270baedba4de307bf85cd1fe7e7c9a7103e06712cfe550d8b6baac5bd53f`。没有 B0 或 tag。测试中移走的缓存与日志已全部恢复。

最终保留一个 App（PID 30618）和一个 daemon（PID 34307）；无构建或测试进程残留。最终采样 syspolicyd 为 0.6% CPU。当前窗口保留完整 QA 综合画布。

# 产物与使用

App：`/Users/bruce/git/tower.md/.release/s6-synthesis-20261021/Tower.md.app`

同目录保留 `manifest.json`、实际综合输入、原始/恢复/缺失日志/最终草稿响应、QA 前后状态与测试构建日志。manifest 绑定实现提交、完整包文件/前端哈希和实际 QA 标识。

| 文件 | SHA-256 |
| --- | --- |
| `Contents/MacOS/towermd-app` | `faf6da9622e1d3fca6df66104077572085dffb8a4ca10aefaf54c7aecdd4d6eb` |
| `Contents/MacOS/towermd` | `d8b05ce56cf4611c27f6932f6b477dce422abb933b2257580d1dffa0f3eb94d4` |

当前窗口可直接试用。重新打开时，选择同目录 `qa-project`，画布右键 →“查看综合草稿”→选择条目；点击卡片查看原文，点击左上角“综合草稿 · 未批准”查看风险和问题。滚轮缩放、拖动空白处浏览画布。

自己的项目先完成 S1–S5，再按 README 使用项目技能和 `baseline schema/template/submit/list/show`。提交输入是临时 JSON，Agent 不直接写用户文档。S6 草稿不会变成已批准结构；后续须补齐 S7 审阅编辑、唯一主干写入/恢复与 S8 人工批准。

仅验收 macOS Apple Silicon。没有发行者签名或公证，保留工具链 ad-hoc 签名；这是本机测试候选，没有正式发布 tag 或远端 release。

提交前已核对 worktree、分支和 stash：只有根 worktree/main，无 stash。用户原有未跟踪 `.towermd/`、`AGENTS.md`、`CLAUDE.md`、`docs/superpowers/`、`docs/tower/` 均未纳入提交。
