# 任务计划

## 2026-10-17 M0 整改（D0–D6 已批准）

执行依据：`docs/superpowers/plans/2026-10-17-m0-remediation.md`。用户于本会话明确答复“全部按推荐方案执行”：D0 四批提交现状（不含 S5）；D1 私有快照引用；D2 App 确认恢复端点；D3 临时分离工作树验证、继承环境且允许网络；D4 init 尽力预检；D5 按表同步规格；D6 增补 M0 有效指标。不再重复询问这些裁决。本轮不推送。

- [x] 阅读整改总表、经验和现状，记录人的 D0–D6 裁决
- [x] D0：按精确路径分四个连续基线检查点提交 M0-1/2/3/4（完整基线为第四个提交）
- [x] 批次 0 余项：R16 同步规格、独立复审、完整基线复验并提交（`b5a881f8d5f8e32a0d17816fc81f12ad2edd3637`）
- [ ] 批次 1：R1 离线读取与后台 fetch；R2 init 本地独立和显式目录
- [ ] 批次 2：R6 HubProtocol 与租约事件；R5 追加 journal、压缩与增量投影
- [ ] 批次 3：R3 私有快照落点；R7 有界重做与连续前缀；R4 App 确认恢复、trailer/发件箱；R14 事件触发快照
- [ ] 批次 4：R8 共用分离工作树验证与异步 scan/wait、逐模块证据；R9 层级草稿与读取
- [ ] 批次 5：R10 clone 重绑；R11 非法层级保护；R12 画布逐条数值处理；R13 状态与接口诊断；R15 文档和提交记录
- [ ] 全量测试、fmt、Clippy、规格与质量复审、真实 daemon 路径验收；逐批记录提交号

本轮此前的沙箱 S5 工作仅保留在 `/private/tmp/towermd-m0-s5`，不并入现状基线；D3 已将其归入后续安全加固。根 README/中英文 CHANGELOG 已恢复到既有 M0 已验收内容，后续按整改结果重新更新。D0 精确路径冻结于 `/private/tmp/towermd-m0-baseline-paths.json`，46 个文件逐一匹配本轮开始 320 项测试通过的工作区。

D0 提交记录（本轮不推送）：M0-1 `be0dff184df47710a8c4a5ac350a69177e6a79cb`（13 文件）；M0-2 `9f9eee60335e4baf485b37dbb0416577202f4801`（13 文件）；M0-3 `fdba1fb24894278a5deaa95830bf7dc19dab2d1b`（9 文件）；M0-4 `d68b961fb4c92989e6eb30eaf516802293f80061`（11 文件）。这四个连续 checkpoint 保存同一已验收工作区的文件分组，只有齐全后才形成完整可构建基线，不把中间 checkpoint 当作独立发布验收。

批次 0 验证：完整四批基线在独立目标目录 `/private/tmp/towermd-m0-remediation-target` 串行运行全部 workspace 测试 **320/320 通过、0 跳过**；fmt 与 Clippy（全部 target、`-D warnings`）通过。R16 的 10 项文本一致性检查已红→绿，独立规格与文档质量复审均 PASS。FR-92 已明确业务投影由 daemon/core 计算、hub 只保存不透明检查点，不重新解析 `data`。

补充诊断：此前 S5 隔离工作树整套测试一次在既有并发首启动测试挂起，发现 daemon 继承测试管道导致 EOF 未到；单项重跑通过，原整套中断不计绿。诊断留存 `/private/tmp/towermd-task2-workspace-hang.txt`，不得把分项结果冒称完整 workspace 通过。

## 2026-10-17 M0 整改（对照规格 v0.5 评审后）

整改总表：[`docs/superpowers/plans/2026-10-17-m0-remediation.md`](../docs/superpowers/plans/2026-10-17-m0-remediation.md)（未提交，草稿）。上一节的 S5 沙箱路线暂停，等 D3 裁决。

- [ ] 人裁决 D0–D6（D0 提交基线、D1 快照落点、D2 重建确认端点、D3 S5 机制、D4 init 推送检查、D5 追认规格改写、D6 覆盖率指标）
- [ ] 批次 0：提交现状基线；R16 规格同步
- [ ] 批次 1：R1 离线可用、R2 init
- [ ] 批次 2：R6 协议抽象与事件、R5 存储有界
- [ ] 批次 3：R3 快照落点、R7 重做、R4 重建可达、R14 快照时机
- [ ] 批次 4：R8 验证器与 S5、R9 冷启动输出
- [ ] 批次 5：R10–R13、R15

## 2026-10-17 继续开发：隔离验证器与冷启动 S5

此执行方向已被上方 M0 整改及 D3 裁决替代。以下保留历史工作与验证记录，未同步、未提交、未发布。

承接已记录的下一项，先完成 S5 真实执行，不扩展到 M3 harness 调度或 App/B0。继续开发已获授权；沿用 `/private/tmp` detached 工作树、现有依赖、未提交交付约定。当前工作区已有交付内容不得混入新提交。

- [x] 回顾经验、现有规格和未完成项，核对执行与批准边界
- [x] 重跑当前基线：`cargo test --workspace --locked` 320 项通过；默认沙箱的本机端口限制已定位
- [x] 在独立工作树实现固定提交、只读源码、独立输出、禁网及超时回收的验证器；隔离不可用时拒绝执行（双审 PASS、12 项真实沙箱专项通过）
- [ ] 经 daemon 接入 S5 执行请求；扫描继续只读，结构化命令去重，待确认命令不执行
- [ ] 保存不可变日志及哈希，证据经唯一 hub 定序并可从快照重建；不改变 Task 完成状态或慢层
- [ ] 完成规格审查、质量审查、真实隔离执行和 workspace 验证
- [ ] 同步精确文件，更新 README、中英文未发布日志与回顾，明确未提交/未发布状态

验证计划：先重跑已有 320 项基线；再验证通过/失败/超时/缺工具、固定提交与脏工作树隔离、越界写和网络拒绝、输出截断、后台子进程回收、日志落盘失败及 hub 重放/快照恢复；最后运行 fmt、Clippy、build 和完整测试。当前受工具沙箱限制，daemon 本机端口与 macOS sandbox 探测须在批准的本机执行环境运行；已确认沙箱探测成功。

阶段记录（尚未提交/发布）：Task1 独立规格与质量审查均 PASS，新增真实 sandbox 测试 12/12、原 Git timeout 10/10、repo 单元 3/3 通过。审查修复 APFS 大小写/Unicode 目录别名、探针与合法源码名碰撞；真实依赖构建暴露 `/dev/null` 子进程重定向问题，已添加仅该无持久状态设备的例外并复审，隔离版本为 v2。退出码 71 加 `sandbox-exec:` stderr 前缀可能把真实 Failed 分类为 Unavailable，原始退出码/日志保留，不会误报通过；作为非阻塞限制记录。

真实源码 API 验收：由当前已验收 Tower.md 源码创建独立临时 clone `/private/tmp/towermd-s5-acceptance-oqsnf9i5`，仅添加初始化配置与定向 Makefile 验证入口形成夹具提交 `0bc9536919a8ccdee47f17de7205987d32b5c73c`，tree `7ed7a0f44be9c960e7d6b8d6d7820782b34f1d68`。只读沙箱离线完整编译依赖，`cargo test --offline --locked -p towermd-core --test canvas` 18/18 通过；日志在 `/private/tmp/towermd-s5-api-probe/`。此时仅验证核心 API，CLI 与 hub 证据接入仍在实施，不能当作最终 S5 产品验收或本仓库发布证据。

## 2026-10-16 M0 后续执行（superpowers）

用户已要求持续推进，按 M0-2 → M0-3 → M0-4 的依赖顺序执行；不重复询问已有授权。M0-2、M0-3 及 M0-4 已确认的准备范围均已验证并同步主工作区；隔离 detached worktree 保留，不创建分支、不提交。

- [x] 重新验证 M0-1：77 项测试通过；阅读后续规格与 superpowers 技能
- [x] 独立审查 M0-2 边界；对相关决策、领取分支和有效路径三处歧义提请确认
- [x] M0-2：持久化 hub、幂等定序、原子多模块租约与 epoch（已验证并同步）
- [x] M0-2：权威 main 对象目录、用户级守护进程、工作树绑定与任务命令（含 WebSocket 订阅、任务卡与 harness 自报）
- [x] M0-2：规格/质量审查、并发与重启验收、同步已验证改动
- [x] M0-3：快照与恢复请求实现、双重审查、主工作区 276 项测试和 fmt/Clippy/build 通过；实际 App 放行仍不可用
- [x] M0-4 已确认范围：画布编解码、只读对象视图、候选模块扫描与测试计划；规格/质量复审和主工作区验收通过
- [ ] 后续缺口：隔离运行器及 S5 实跑、App 放行与基线发布；整个 M0 尚未完成

依赖：serde 1、serde_json 1、chrono 0.4、fs2 0.4、dirs 5、sha2 0.10、hex 0.4、tokio 1、reqwest 0.12 沿用 Note.md 版本线；axum 0.8 属于已获批准的守护进程 HTTP 选型。暂不引入 WebSocket 客户端库。hub 使用同步落盘的原子 journal，投影从事件重放，暂不引入 SQLite 派生索引。验证包含故障不更新内存、重启幂等、旧 epoch 无效、同 clone 多 worktree 和异 clone 同 ID 拒绝。

当前后续工作状态：**本轮确认范围已验收、未提交、未发布**。2026-10-16 人已确认：没有相关 Decision 时允许，显式引用须存在于权威 main；先创建目标工作树及 t/<task-id> 分支再领取，暂不迁移绑定；任务 allowed_paths 为实际范围，模块 paths 仅为上限。答复在本记录与实现计划中持久化，不重复询问。

M0-3 设计答复（2026-10-16）：人已确认 state.json 保存业务投影及 eid→seq 等最小恢复元数据，租约摘要仅追溯、灾难恢复全失效；恢复必须先将新 seq/epoch 边界写入权威 main，再开放服务，App 人工确认不得绕过。

M0-4 范围答复（2026-10-16）：人已确认本轮先交付画布编解码、候选模块扫描与测试计划；S5 在只读工作树实际执行测试，留待隔离运行器就绪。此范围不等于整个 M0 完成。M0-4 使用独立 `/tmp/towermd-m0-4` 工作树，不与 M0-3 复审混合。

M0-4 真实仓库验证：Note.md 固定源码提交 `f2bdc01b2bfd0bf9f9129e144e550360aab444b8` 的独立临时 clone，仅增加测试配置得到夹具提交 `715ce69cb579d06b98f47c6693cd6febdc603a0f`；tree `aeba1ff7e15fc841dc7a0b1d81053b304c51eebf`。最终主工作区构建的 CLI 两次扫描 JSON 字节完全相等：1952/1952 个已识别的已跟踪源码文件归入 25 个候选模块，S5 为 not_run。候选 Canvas 可解码，index 字节和工作树不变，无 hub 或 canvas 落盘，临时 daemon 已停止；Note.md 原仓库仍干净。

M0-2 历史验证进度：隔离工作树全量 158 项通过，随后补普通 frontmatter 与非对象路径两项红绿回归；fmt、Clippy、build 已通过。规格审查的六项缺口已补实现与回归：禁用 Git replacement refs、全树慢层对象路径保护、任务上下文展开、harness 自报、持续事件订阅、init/daemon 命令续租。最终结论见下段。

M0-2 规格与质量复审均 PASS；主工作区最终 163 项测试、fmt、Clippy（-D warnings）、build 全通过。质量修复包含 daemon 进程组隔离及显式无效 --run 拒绝停机。已精确同步主工作区，未提交、未发布；HEAD 未改变。随后在 `/tmp/towermd-m0-3` 开发快照与恢复基础，最终结论见下段。

M0-3 审查结论：2026-10-17 人已批准无远端且 main 在任一工作树检出时一律推迟快照，待用户自行切到其他分支再写；远端及未检出的本地 main 不受影响。已更新 FR-91 与计划并删除 commit --only 路径；符号 main 采用拒绝与 no-deref 防护，恢复网络期间释放 hub 锁并在安装前复查并发变化。独立质量复审关闭原三项 P1，专项 51/51 通过；隔离工作树及主工作区均完成 276 项完整测试与 fmt/Clippy/build。未提交、未发布。

M0-4 规格审查修复已完成红绿与复审：浅克隆不据残缺历史合并模块；父目录 paths 不跨入子模块；画布边只绑定 Interface；合法换行文件名按字面读取；只读续租不处理其他过期 Run。质量审查另修正 CI cwd 等价写法丢命令、未识别 Make/Cargo 误报无测试、复制 hub 后绕过持久 clone 绑定三项问题，原审查者均复验 PASS。S5 仍为 not_run，不生成通过证据。

### 本轮最终回顾（2026-10-17）

- 主工作区最终 `cargo test --workspace --locked` **320 项通过**；`cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets --locked -- -D warnings`、`cargo build --workspace --locked` 全部退出 0。
- 新增专项：codec 18、只读视图 5、coldstart 10、adapter 10；task 25 项含新续租边界回归。独立 clone 绑定故障矩阵另有 18 项验证通过。
- 精确同步 48 个交付文件并核对冻结内容；更新 README 与中英文未发布日志。没有新增 M0-4 依赖，没有批准、测试执行或慢层写回入口。
- 仍未提交、未发布，无新增提交号；HEAD 为 `2a39a20e680a8995f1e9c2b260bcbbffff6c8148`，暂存区为空，原有 AGENTS.md/CLAUDE.md 未改。保留隔离工作树，不自动清理未提交内容。
- 后续先补隔离运行器与 S5 真实证据；App 确认、B0 生效和离线发件箱恢复依然待实现，不能把本轮准备能力标为整个 M0 完成。

## 2026-10-16 M0-1 执行（superpowers）

范围：执行已有 M0-1 计划，完成核心对象、配置和 `towermd init`；M0-2–M0-4 保持后续计划。

- [x] 阅读规格、设计原则、OKF 约束与 M0-1 计划；确认沿用 Note.md 依赖版本线
- [x] 用户同意安装 Rust stable 并继续实现 M0-1
- [x] 安装工具链，在隔离 worktree 中建立 Rust workspace（Rust/Cargo 1.99.1；骨架构建通过）
- [x] TDD：OKF 往返与类型/层级登记（OKF 16、types 9 项通过，规格与质量审查通过）
- [x] TDD：配置读取与未知字段保留（10 项测试通过，规格与质量审查通过）
- [x] TDD：init 核心及路径、已有文件保护（规格与质量审查通过，含并发与 linked worktree 回归）
- [x] TDD：CLI 非交互与真实终端交互（15 集成、3 单测通过；PTY 默认/重入/EOF 验证通过）
- [x] 规格审查、代码质量审查、测试与构建验证（主工作区 77 项测试、fmt、Clippy、构建全部通过）
- [x] 更新使用说明、中英文未发布日志和执行回顾

执行约定：使用 `/tmp/towermd-m0-1` 的 detached worktree 隔离开发，验收后将本次文件同步回当前工作区。只安装工具链，不改 shell 启动文件或全局 git/harness 配置。当前交付状态为**未提交**；不把已有未跟踪文件混入提交。

### 回顾

- 已实现：Rust workspace、OKF 文档、14 项 type 登记、层级判定、配置读取和 `towermd init`。依赖沿用 Note.md 的版本线，clap 加 derive；没有另引入依赖。
- 工具链：经用户同意安装 Rust/Cargo 1.99.1 与 rustfmt、Clippy；使用 `~/.cargo/bin`，未修改 shell 启动文件。新终端可执行 `source "$HOME/.cargo/env"` 使用 cargo。
- TDD：OKF、types、config、init/repo、CLI 均记录失败再通过；审查发现的不可序列化 YAML 值与 broken pipe panic 均补红绿回归后修复。
- 隔离 worktree 验证：77 项测试通过（核心 59、CLI 单测 3、CLI 集成 15）；fmt、Clippy `-D warnings`、workspace build 通过；并发 init 专项重复 30 次通过。
- 真实终端：首次回车取默认目录，再次执行无询问且 ID 不变；Ctrl-D 退出码 1，未生成配置与文档目录。
- 子代理完成各模块规格与质量审查及最终跨模块审查；执行前发现的路径越界、符号链接、文件占位、并发覆盖、Git 路径裁剪等问题已修正并测试。
- 已将本次文件同步回 `/Users/bruce/git/tower.md`，主工作区使用 `--locked` 完成全部 77 项测试、Clippy 和构建，格式检查通过。**未提交、未发布，无提交号**；HEAD 仍为 `2a39a20`，暂存区为空；原有 `AGENTS.md`、`CLAUDE.md` 未修改，任务记录仍保持原有未跟踪状态。
- 下一步：M0-2 本地 hub、守护进程与任务命令；M0-3 补快照和 FR-91 远端可推送性检查，M0-4 补画布与冷启动。当前不宣称整个 M0 完成。

## 2026-10-16 规格 v0.5：技能加 App、用户文档目录、唯一 hub 与快照

背景：用户说明 Tower.md = 技能包 + 独立 App；md 文档放用户指定的文档目录，`.towermd/` 只放隐藏配置；多 harness 多工作树、多机执行需要一个全局协调点（Cloudflare Worker）；活跃实时状态以唯一服务为主，快照存回 git 仓库。v0.1 选"技能优先，调度后置"。

- [x] 0 摘要、0.2 依赖分层、1.2 非目标：改为技能包 + CLI + App；hub 为唯一活跃状态源
- [x] 5 系统架构：三个面、hub 的两种实现、快照、各端协作；FR-04/05/06 改写
- [x] 6.1 目录：`.towermd/` 只放配置；`<docs_dir>/` 放慢层 md、`tower.canvas`、快照
- [x] 6.2 模块去掉 `map`，坐标进 `tower.canvas`；新增 6.5 tower.canvas 数据模型
- [x] 6.3 事件：hub 定序 `seq` + 客户端 `eid`；租约与快照事件
- [x] 7 分支：技能优先下的分支约定、租约 epoch、路径守卫改为检查加事后检测
- [x] 10 执行：新增 10.0 技能优先模式与 Tower.md 技能包；无头双环标为 M3
- [x] 12 中断：CLI 通道（实时等待/deferred）；插话随下一次 CLI 调用送达
- [x] 16 接口：CLI、hub 协议
- [x] 17 NFR、18 里程碑重切、20 风险与待定问题、21 追溯
- [x] 自检（占位、一致性、范围、歧义）→ 独立检查 → 提交
- [x] 交人审阅；Q10 原则与不变量改写待人确认（2026-10-16 人答复"都按你的改"，已改 AGENTS.md 与 product-principles.md）

需人确认：`product-principles.md` 信念二与 `AGENTS.md` 不变量的改写建议（规格里注明，不直接改）。

### 回顾

- 自检：FR-01–FR-112 无重号缺号；脚注引用与定义一一对应；README 改为 v0.5 描述。
- 独立检查（子代理通读全文）提出 25 条，核对属实后全部处理。要点：
  - 证据改为绑定 tree，不绑定 SHA；写 trailer、压缩检查点不再让"已合并"无法成立（FR-107、FR-12）。
  - 快照与慢层提交一律用底层命令在 `origin/main` 上生成，不移动本地分支、不碰工作树、不用用户 git 身份；git 远端的比较并交换加 `(epoch, seq)` 检查实现 fencing（FR-91）；主干租约的取得与交接（FR-110）。
  - `seq` 连续无空洞、快照只覆盖连续前缀、截断后保留 `eid` 索引；重建时 `seq`/`epoch` 跳号、租约作废、从 `main` trailer 补事件（FR-87、FR-92、FR-93）。
  - 失效 `epoch` 的事件统一为"记录但标过期、不参与投影"。
  - 威胁模型写明 v0.1 防粗心不防恶意；`human:` 事件只能来自 App 签名（FR-111）；签名绑定内容哈希，公钥不读仓库。
  - 租约 TTL 与续租、`ask`/`finish` 分段等待以避开 harness 命令超时（FR-112、FR-59、FR-107）。
  - 里程碑：B0 批准与提案提前到 M1；一句话生长改由 `towermd-seed` 技能执行；import 解析与社区发现推迟到 M2。
  - 依赖总控的状态转换与交互逐条写出 v0.1 降级行为。
- 未处理：把 M3 细节移到附录（篇幅调整，留到写 M0 计划时再看）。

## 2026-10-16 M0 实现计划（superpowers writing-plans）

M0 拆成四份计划，放在 `docs/superpowers/plans/`，逐份写、逐份交人确认执行方式。

- [x] M0-1 核心对象与 init：`2026-10-16-m0-1-core-objects-init.md`
- [x] M0-2 本地 hub、守护进程单实例、多终端运行判定、`status` 与 `task add/claim/show/release`（主工作区 163 项验收通过，未提交）
- [x] M0-3 快照写入、主干租约、`hub rebuild` 请求（主工作区已验证，未提交；App 执行入口不在本轮）
- [ ] M0-4 `tower.canvas` 编解码、对象目录加载、冷启动 S1–S5

需人确认：

- 写计划时本机没有 Rust 工具链；2026-10-16 用户已同意安装并继续实现，执行状态见本文件顶部。
- clap 沿用 Note.md 的 4.x，但加 `derive` 特性（Note.md 用 builder 写法）。理由：M0 起子命令较多，derive 写法更短、更不易错。这不算新增依赖。
- `AGENTS.md`、`CLAUDE.md`、`tasks/` 目前未跟踪，是否纳入版本控制由人决定。
