---
type: Reference
title: 能力对等表（FR-190）
---

FR-190 要求守护进程的每个端点都能从命令行或 App 到达，画布渲染与布局拖动除外。本表逐条列出 `crates/towermd/src/daemon.rs` 中 `router` 注册的每个“方法 + 路径”，以及能到达它的入口。`crates/towermd/tests/capability_parity.rs` 守住本表：路由表与本表的行必须一一对应，没有空格，入口名必须真实存在。新增路由而不登记本表，测试即失败。

## 读法

- **App** 列是 Tauri invoke 名（`src-tauri/src/lib.rs` 的 `generate_handler!`）；**CLI** 列是 `towermd` 命令（空格分隔，选项不计入命令名）；**MCP** 列是 MCP 交互网关的工具名；**Tower API** 列是 Tower API 的动词。
- 反引号里的名字是直接入口，测试会核对它确实存在。
- `≈` 开头表示等价入口：入口不调用这条路由本身，但经另一条路由读到或做成同一件事，括号里写明怎么等价。
- `仅 App（理由）` 只出现在 CLI 列，表示这是人的动作。按 AGENTS.md，命令行不提供批准类命令，人的动作只从 App 发出，署名 `human:<id>`。按 2026-10-23 的裁定，这是规定的不对等，不算空格。
- `—（理由）` 表示这个入口按设计不到达这条路由。App 列里的“受托者的动作”与 CLI 列的“仅 App”对称：领取、提交完成、提问等是 Agent 的动作，人不替 Agent 做。
- 理由中不用反引号，以免被当成入口名核对。

## 对等表

| 方法 | 路径 | App | CLI | MCP | Tower API | 说明 |
| --- | --- | --- | --- | --- | --- | --- |
| GET | `/health` | ≈ `choose_project`（经 sidecar 的 daemon ensure 探活） | `daemon status`、`daemon ensure`、`doctor` | —（不在网关） | —（非接入动词） | 守护进程身份与 API 版本 |
| POST | `/shutdown` | —（关闭画布不停止执行，App 不停守护进程） | `daemon stop` | —（不在网关） | —（非接入动词） | |
| POST | `/projects/open` | `choose_project`、`open_known_project` | ≈ `up`（任一项目命令先打开当前项目） | —（不在网关） | —（非接入动词） | |
| POST | `/projects/init` | `init_project` | ≈ `init`（命令行在本地初始化） | —（不在网关） | —（非接入动词） | |
| GET | `/app/locate` | `follow_locate` | —（App 的导航通道；命令行经 open 向它推送） | —（不在网关） | —（非接入动词） | 只在内存，不写事件 |
| GET | `/overview` | `overview` | `overview` | —（不在网关） | —（非接入动词） | FR-133 |
| GET | `/projects/{handle}/view` | `load_project` | `show`、`decision ls`、`decision show` | —（不在网关） | —（非接入动词） | |
| GET | `/projects/{handle}/state` | `project_state` | `up`、`needs`、`since`、`ls`、`task ls`、`rule ls` | —（不在网关） | —（非接入动词） | |
| POST | `/projects/{handle}/main-resolutions/prepare` | `prepare_main_resolution` | 仅 App（人的动作：处置 main 上未经授权的提交） | —（不在网关） | —（非接入动词） | |
| POST | `/projects/{handle}/main-resolutions/confirm` | `confirm_main_resolution` | 仅 App（人的动作：确认处置预览） | —（不在网关） | —（非接入动词） | |
| GET | `/projects/{handle}/runs/{id}/events` | `run_details` | `dev events` | —（不在网关） | —（非接入动词） | |
| GET | `/projects/{handle}/tasks` | ≈ `project_state`（状态体含任务） | `task show` | —（不在网关） | —（非接入动词） | |
| POST | `/projects/{handle}/tasks` | ≈ `create_human_task`（人建的任务走 tasks/human） | `task add` | —（不在网关） | —（非接入动词） | |
| POST | `/projects/{handle}/tasks/{id}/claim` | —（受托者的动作） | `task claim` | —（不在网关） | —（非接入动词） | |
| POST | `/projects/{handle}/tasks/{id}/edit` | —（受托者的动作：只改命令行建且未领取的任务） | `task edit` | —（不在网关） | —（非接入动词） | |
| GET | `/projects/{handle}/history` | ≈ `subscribe_project`（事件流推送同样的事件） | `history`、`watch` | —（不在网关） | —（非接入动词） | |
| POST | `/projects/{handle}/task-release` | ≈ `reassign_task`（人改派走 tasks/{id}/release） | `task release` | —（不在网关） | —（非接入动词） | |
| POST | `/projects/{handle}/task-status` | ≈ `project_state`（状态体含运行） | `status` | —（不在网关） | —（非接入动词） | |
| POST | `/projects/{handle}/main-proposals` | `capture_main_proposal` | 仅 App（人的动作：把 main 上的人工提交收为提案） | —（不在网关） | —（非接入动词） | |
| GET | `/projects/{handle}/proposals` | `list_proposals` | ≈ `ls`（ls proposals 读状态体） | —（不在网关） | —（非接入动词） | |
| POST | `/projects/{handle}/proposals` | —（受托者的动作） | `propose` | —（不在网关） | —（非接入动词） | |
| GET | `/projects/{handle}/proposals/{id}` | ≈ `list_proposals`（列表含全文） | ≈ `show`（show 按 ID 找提案） | —（不在网关） | —（非接入动词） | |
| POST | `/projects/{handle}/proposals/{id}/publication/prepare` | `prepare_proposal` | 仅 App（人的动作：批准提案前的预览） | —（不在网关） | —（非接入动词） | |
| POST | `/projects/{handle}/proposals/{id}/publication/approve` | `approve_proposal` | 仅 App（人的动作：批准提案，慢层只由人改） | —（不在网关） | —（非接入动词） | |
| POST | `/projects/{handle}/proposals/{id}/reject` | `reject_proposal` | 仅 App（人的动作：驳回提案） | —（不在网关） | —（非接入动词） | |
| POST | `/projects/{handle}/task-check` | —（受托者的动作） | `dev check` | —（不在网关） | —（非接入动词） | |
| POST | `/projects/{handle}/task-finish` | —（受托者的动作） | `dev finish` | —（不在网关） | —（非接入动词） | |
| GET | `/projects/{handle}/task-finish/{id}` | ≈ `run_details`（运行详情含完成证据） | `dev finish`、`wait` | —（不在网关） | —（非接入动词） | |
| POST | `/projects/{handle}/task-trailers` | —（受托者的动作） | `dev trailers` | —（不在网关） | —（非接入动词） | |
| GET | `/projects/{handle}/dispatch-queue` | ≈ `project_state`（状态体含派发队列） | `dev queue` | —（不在网关） | —（非接入动词） | |
| POST | `/projects/{handle}/dispatch-queue` | `retry_task` | `dev run` | —（不在网关） | —（非接入动词） | |
| POST | `/projects/{handle}/dispatch-queue/{task}/remove` | ≈ `pause_task`（暂停任务即停止派发） | `dev dequeue` | —（不在网关） | —（非接入动词） | |
| GET | `/projects/{handle}/adapters` | —（遗留：App 尚无适配器清单入口） | `dev adapters` | —（不在网关） | —（非接入动词） | |
| POST | `/projects/{handle}/ask` | —（受托者的动作） | `ask` | `towermd_ask` | —（非接入动词） | |
| POST | `/projects/{handle}/wait` | —（受托者的动作） | `wait` | `towermd_ask` | —（非接入动词） | |
| POST | `/projects/{handle}/answers` | `answer_question` | 仅 App（人的动作：回答问题） | —（不在网关） | —（非接入动词） | |
| POST | `/projects/{handle}/answers/relay` | ≈ `answer_question`（人直接回答） | `answer` | —（不在网关） | —（非接入动词） | 只做转述，不记为决策、规则或批准 |
| POST | `/projects/{handle}/prompts` | `send_prompt` | 仅 App（人的动作：插话） | —（不在网关） | —（非接入动词） | |
| POST | `/projects/{handle}/pause` | `control_project` | `pause` | —（不在网关） | —（非接入动词） | 暂停任何一方都可以 |
| POST | `/projects/{handle}/resume-request` | ≈ `control_project`（人直接恢复） | `resume` | —（不在网关） | —（非接入动词） | 命令行只能请求恢复 |
| POST | `/projects/{handle}/resume` | `control_project` | 仅 App（人的动作：恢复项目） | —（不在网关） | —（非接入动词） | |
| POST | `/projects/{handle}/tasks/human` | `create_human_task` | 仅 App（人的动作：以 human 署名建任务） | —（不在网关） | —（非接入动词） | |
| GET | `/projects/{handle}/tasks/options` | `task_options` | 仅 App（只服务 App 的建任务表单） | —（不在网关） | —（非接入动词） | |
| GET | `/projects/{handle}/tasks/{id}` | ≈ `project_state`（状态体含任务；任务下钻视图是遗留） | ≈ `task show`（读同一任务与运行） | —（不在网关） | —（非接入动词） | 规格 15.5 |
| POST | `/projects/{handle}/tasks/{id}/release` | `reassign_task` | 仅 App（人的动作：改派） | —（不在网关） | —（非接入动词） | |
| POST | `/projects/{handle}/tasks/{id}/pause` | `pause_task` | 仅 App（人的动作：暂停单个任务） | —（不在网关） | —（非接入动词） | |
| POST | `/projects/{handle}/tasks/{id}/resume` | `resume_task` | 仅 App（人的动作：恢复单个任务） | —（不在网关） | —（非接入动词） | |
| POST | `/projects/{handle}/tasks/{id}/rollback` | `rollback_task` | 仅 App（人的动作：回滚） | —（不在网关） | —（非接入动词） | |
| POST | `/projects/{handle}/locate` | —（App 经 follow_locate 接收定位） | `open` | —（不在网关） | —（非接入动词） | |
| POST | `/projects/{handle}/tasks/{id}/report` | ≈ `ask_about`（经 requests 要求汇报） | 仅 App（人的动作：要求受托者汇报） | —（不在网关） | —（非接入动词） | |
| GET | `/projects/{handle}/modules/{id}` | —（遗留：App 尚无模块下钻视图） | `dev module` | —（不在网关） | —（非接入动词） | 规格 15.5 |
| GET | `/projects/{handle}/holds` | ≈ `project_state`（注意力清单含等人的事） | ≈ `ls`（ls holds） | —（不在网关） | —（非接入动词） | |
| POST | `/projects/{handle}/seed` | —（遗留：App 尚无演示种子入口） | `dev seed` | —（不在网关） | —（非接入动词） | |
| GET | `/projects/{handle}/events` | `subscribe_project` | ≈ `watch`（轮询 history） | —（不在网关） | —（非接入动词） | WebSocket |
| POST | `/projects/{handle}/messages` | —（受托者的动作） | ≈ `status`（每条命令先投递消息） | ≈ `towermd_ask`、`towermd_request_review`、`towermd_note`（工具调用顺带投递） | —（非接入动词） | |
| POST | `/projects/{handle}/messages/ack` | —（受托者的动作） | ≈ `status`（投递后确认） | ≈ `towermd_ask`、`towermd_request_review`、`towermd_note`（投递后确认） | —（非接入动词） | |
| POST | `/projects/{handle}/messages/handled` | —（受托者的动作） | `handled` | —（不在网关） | —（非接入动词） | |
| POST | `/projects/{handle}/delegations/{id}/clarify` | —（受托者的动作） | `dev clarify` | —（不在网关） | —（非接入动词） | |
| POST | `/projects/{handle}/delegations/{id}/finalize` | `finalize_delegation` | 仅 App（人的动作：定稿委托） | —（不在网关） | —（非接入动词） | |
| POST | `/projects/{handle}/delegations/{id}/used` | `record_acceptance_used` | 仅 App（人的动作：记录验收已用） | —（不在网关） | —（非接入动词） | |
| POST | `/projects/{handle}/delegations/{id}/confirm` | `confirm_acceptance` | 仅 App（人的动作：确认验收） | —（不在网关） | —（非接入动词） | |
| POST | `/projects/{handle}/delegations/{id}/send-back` | `send_back_acceptance` | 仅 App（人的动作：退回验收） | —（不在网关） | —（非接入动词） | |
| POST | `/projects/{handle}/delegations/{id}/waive` | `waive_gate` | 仅 App（人的动作：豁免门禁，带到期时间） | —（不在网关） | —（非接入动词） | |
| POST | `/projects/{handle}/delegations/{id}/interrupt` | `resolve_interrupt` | 仅 App（人的动作：处置中断） | —（不在网关） | —（非接入动词） | |
| POST | `/projects/{handle}/reviews` | —（受托者的动作） | `ask --review` | `towermd_request_review` | —（非接入动词） | |
| POST | `/projects/{handle}/reviews/wait` | —（受托者的动作） | `wait` | `towermd_request_review` | —（非接入动词） | |
| POST | `/projects/{handle}/reviews/{id}/resolve` | `resolve_review` | 仅 App（人的动作：评审裁决，不超时自动通过） | —（不在网关） | —（非接入动词） | |
| POST | `/projects/{handle}/permissions` | —（只供网关） | —（只供网关，FR-60） | `towermd_permission` | —（非接入动词） | |
| POST | `/projects/{handle}/permissions/wait` | —（只供网关） | —（只供网关，FR-60） | `towermd_permission` | —（非接入动词） | |
| POST | `/projects/{handle}/permissions/{id}/resolve` | `resolve_permission` | 仅 App（人的动作：授权，超时默认拒绝） | —（不在网关） | —（非接入动词） | |
| POST | `/projects/{handle}/reports/{id}/reply` | —（受托者的动作） | `reply` | —（不在网关） | `reply` | |
| POST | `/tower` | —（App 经自己的路由记录人的动作） | ≈ `report`（同一信封走项目内路由） | —（不在网关） | `report`、`signal`、`request_report`、`card`、`summary`、`reply` | FR-117，拒绝 App 客户端 |
| POST | `/projects/{handle}/tower` | —（App 经自己的路由记录人的动作） | `report`、`signal`、`summary`、`frame add`、`frame update`、`frame move`、`frame link`、`frame archive`、`frame propose`、`frame edit` | `towermd_note` | ≈ `report`、`signal`、`summary`、`card`（项目内同一信封） | |
| POST | `/projects/{handle}/rules` | `record_rule` | 仅 App（人的动作：定规则） | —（不在网关） | —（非接入动词） | |
| POST | `/projects/{handle}/work-kinds/rename` | `rename_work_kind` | 仅 App（人的动作：改工作类型名） | —（不在网关） | —（非接入动词） | |
| POST | `/projects/{handle}/delegation-levels` | `set_delegation_level` | 仅 App（人的动作：设委托级别） | —（不在网关） | —（非接入动词） | |
| POST | `/projects/{handle}/signals/dismiss` | `dismiss_signal` | 仅 App（人的动作：消除信号） | —（不在网关） | —（非接入动词） | |
| POST | `/projects/{handle}/requests` | `ask_about` | 仅 App（人的动作：向受托者提要求） | —（不在网关） | —（非接入动词） | |
| POST | `/projects/{handle}/requests/{id}/withdraw` | `withdraw_request` | 仅 App（人的动作：撤回要求） | —（不在网关） | —（非接入动词） | |
| POST | `/projects/{handle}/ideas` | `add_idea` | `idea add` | —（不在网关） | —（非接入动词） | |
| POST | `/projects/{handle}/ideas/{id}/promote` | `promote_idea` | 仅 App（人的动作：把想法升为任务） | —（不在网关） | —（非接入动词） | |
| POST | `/projects/{handle}/follow-ups` | `add_follow_up` | 仅 App（人的动作：记待跟进） | —（不在网关） | —（非接入动词） | |
| POST | `/projects/{handle}/temp-rules` | `add_temp_rule` | 仅 App（人的动作：定临时规则） | —（不在网关） | —（非接入动词） | |
| POST | `/projects/{handle}/follow-ups/{id}/done` | `finish_follow_up` | 仅 App（人的动作：了结待跟进） | —（不在网关） | —（非接入动词） | |
| POST | `/projects/{handle}/resurfaced/{id}/seen` | `see_resurfaced` | 仅 App（人的动作：确认已看到） | —（不在网关） | —（非接入动词） | |
| POST | `/projects/{handle}/controller-plans` | `request_plan` | 仅 App（人的动作：交给总控出计划） | —（不在网关） | —（非接入动词） | 12.5、FR-67 |
| POST | `/projects/{handle}/controller-plans/{id}/resolve` | `resolve_plan` | 仅 App（人的动作：确认计划或接管） | —（不在网关） | —（非接入动词） | 不自动放行 |
| POST | `/projects/{handle}/decisions` | `record_decision` | 仅 App（人的动作：记决策） | —（不在网关） | —（非接入动词） | |
| POST | `/projects/{handle}/decisions/{id}/review` | `review_decision` | 仅 App（人的动作：复审决策） | —（不在网关） | —（非接入动词） | |
| GET | `/projects/{handle}/templates` | `list_templates` | ≈ `template ls`（命令行在本地读模板） | —（不在网关） | —（非接入动词） | |
| POST | `/projects/{handle}/frames` | `create_frame` | 仅 App（人的动作：建画框） | —（不在网关） | —（非接入动词） | |
| POST | `/projects/{handle}/frames/{id}/place` | `place_frame` | 仅 App（布局拖动，FR-190 豁免） | —（不在网关） | —（非接入动词） | |
| POST | `/projects/{handle}/frames/{id}/cards` | `write_card` | 仅 App（人的动作：人写卡片；Agent 经 frame add 等写） | —（不在网关） | —（非接入动词） | |
| POST | `/projects/{handle}/frames/{id}/rules` | `change_frame_rules` | 仅 App（人的动作：改画框规则；Agent 经 frame propose 提议） | —（不在网关） | —（非接入动词） | |
| POST | `/projects/{handle}/frames/{id}/resolve` | `resolve_frame_item` | 仅 App（人的动作：处置待定项） | —（不在网关） | —（非接入动词） | |
| POST | `/projects/{handle}/frames/{id}/pins` | `pin_cards` | 仅 App（布局拖动，FR-190 豁免） | —（不在网关） | —（非接入动词） | |
| POST | `/projects/{handle}/frames/{id}/materialize/prepare` | `prepare_materialize` | 仅 App（人的动作：落盘画布前预览） | —（不在网关） | —（非接入动词） | |
| POST | `/projects/{handle}/frames/{id}/materialize/confirm` | `confirm_materialize` | 仅 App（人的动作：确认落盘） | —（不在网关） | —（非接入动词） | |
| GET | `/projects/{handle}/frames/{id}/source` | `frame_source` | `frame source` | —（不在网关） | —（非接入动词） | |
| GET | `/projects/{handle}/frames/{id}/source/history` | `frame_source_history` | `frame source --history` | —（不在网关） | —（非接入动词） | |
| POST | `/projects/{handle}/frames/{id}/source/relink` | `relink_frame_source` | 仅 App（人的动作：重连源文件） | —（不在网关） | —（非接入动词） | |
| GET | `/projects/{handle}/frames/{id}/export` | `export_frame` | `frame export` | —（不在网关） | —（非接入动词） | |
| GET | `/projects/{handle}/frames/{id}/views/{name}` | `frame_view` | `frame view` | —（不在网关） | —（非接入动词） | |
| GET | `/projects/{handle}/frames/{id}/history` | `frame_history` | `frame history` | —（不在网关） | —（非接入动词） | |
| GET | `/projects/{handle}/frames/{id}/history/{seq}` | `frame_history` | `frame history --at` | —（不在网关） | —（非接入动词） | |
| POST | `/projects/{handle}/starters/{id}/adopt` | `choose_starter` | 仅 App（人的动作：采用起步包） | —（不在网关） | —（非接入动词） | |
| POST | `/projects/{handle}/starters/{id}/dismiss` | `choose_starter` | 仅 App（人的动作：不用起步包） | —（不在网关） | —（非接入动词） | |
| POST | `/projects/{handle}/baselines/{id}/publication/prepare` | `prepare_publication` | 仅 App（人的动作：出版基线前预览） | —（不在网关） | —（非接入动词） | |
| POST | `/projects/{handle}/baselines/{id}/publication/approve` | `approve_publication` | 仅 App（人的动作：批准出版基线） | —（不在网关） | —（非接入动词） | |
| POST | `/projects/{handle}/snapshot` | —（守护进程自动快照，App 不需要手动触发） | `snapshot` | —（不在网关） | —（非接入动词） | |
| POST | `/projects/{handle}/hub/rebuild` | `request_recovery` | `hub rebuild` | —（不在网关） | —（非接入动词） | 命令行只能请求 |
| POST | `/projects/{handle}/hub/rebuild/{id}/confirm` | `confirm_recovery` | 仅 App（人的动作：确认恢复 hub） | —（不在网关） | —（非接入动词） | |
| POST | `/projects/{handle}/scans` | `start_scan` | `dev scan start` | —（不在网关） | —（非接入动词） | |
| GET | `/projects/{handle}/scans/{id}` | `poll_scan` | `dev scan show`、`dev scan wait` | —（不在网关） | —（非接入动词） | |
| POST | `/projects/{handle}/scans/{id}/cancel` | `cancel_scan` | `dev scan cancel` | —（不在网关） | —（非接入动词） | |
| GET | `/drafts/{id}` | ≈ `poll_scan`（旧路径，同一扫描结果） | ≈ `dev scan show`（同一扫描结果） | —（不在网关） | —（非接入动词） | |
| GET | `/projects/{handle}/baselines` | `list_baselines` | `dev draft ls` | —（不在网关） | —（非接入动词） | |
| POST | `/projects/{handle}/baselines` | ≈ `draft_from_scan`（App 从扫描直接成稿） | `dev draft submit` | —（不在网关） | —（非接入动词） | |
| GET | `/projects/{handle}/baselines/{id}` | `load_baseline` | `dev draft show` | —（不在网关） | —（非接入动词） | |
| POST | `/projects/{handle}/baselines/{id}/review` | `review_baseline` | 仅 App（人的动作：审基线草稿） | —（不在网关） | —（非接入动词） | |
| POST | `/projects/{handle}/baselines/{id}/verify` | `verify_baseline` | 仅 App（人的动作：发起核验） | —（不在网关） | —（非接入动词） | |
| GET | `/projects/{handle}/baseline-verifications/{id}` | `poll_baseline_verification` | 仅 App（只服务人发起的核验） | —（不在网关） | —（非接入动词） | |
| POST | `/projects/{handle}/baseline-verifications/{id}/cancel` | `cancel_baseline_verification` | 仅 App（人的动作：取消核验） | —（不在网关） | —（非接入动词） | |
| GET | `/projects/{handle}/scans/{id}/template` | ≈ `draft_from_scan`（App 从扫描直接成稿） | `dev draft template` | —（不在网关） | —（非接入动词） | |
| POST | `/projects/{handle}/scans/{id}/draft` | `draft_from_scan` | ≈ `dev draft template`、`dev draft submit`（先取模板再提交） | —（不在网关） | —（非接入动词） | |
| GET | `/projects/{handle}/baselines/{id}/compare` | `compare_baseline` | `dev draft compare` | —（不在网关） | —（非接入动词） | |

## 遗留

- App 没有任务下钻视图与模块下钻视图（规格 15.5），也没有适配器清单和演示种子入口。这几条路由目前只能经命令行或状态体等价读到，留给 App 画布线补。
- 快照只由守护进程自动做；App 没有手动触发入口，按设计不补。
