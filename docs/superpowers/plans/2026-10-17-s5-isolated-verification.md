---
type: ImplementationPlan
title: 隔离验证器与冷启动 S5 实跑
status: draft
generated:
  by: codex/gpt-6
sources:
  - resource: /docs/specs/2026-10-16-spec-towermd-layered-agent-canvas-mvp.md
  - resource: /docs/superpowers/plans/2026-10-16-m0-4-canvas-coldstart.md
  - resource: /tasks/todo.md
---

# 目标与边界

补齐已确认的下一项：在固定权威提交上执行可解析的候选验证命令，生成真实结果与可恢复证据。准备扫描仍不执行程序；不启动 harness，不批准或修改模块、验证契约、画布和 B0，不将任何任务自动标为完成。

# 实施契约

1. 核心验证器放在 `towermd-core`。将固定提交中的普通 blob 按原始字节物化到临时执行目录，保留执行位；拒绝不安全路径、符号链接和 submodule，绝不读取调用者未提交文件。执行目录不含 `.git`，不创建共享 Git 工作树或执行 checkout hooks/filters。
2. 首版使用 macOS 系统 `sandbox-exec` 作为平台适配实现，无新依赖。自检验证只读写入被拒绝，隔离不可用或非支持平台返回不可执行，绝不无沙箱回退。持久文件写入仅允许独立临时输出目录；另允许无持久状态设备 `/dev/null`，以支持正常子进程重定向。禁止网络。环境白名单只传 PATH、必要的系统工具链定位及专用临时 HOME/TMPDIR、Cargo target/Go cache 等输出位置；不继承密钥、Git 注入变量或 harness 配置。工具缓存仅可读取，不自动安装依赖或联网。
3. 仅执行 daemon 自己从固定 tree 发现的结构化 argv；未知 argv、需要确认或 unsupported 的命令保留未执行。每个 `(cwd, argv)` 只执行一次并映射到所有相关模块。每命令超时 1–300 秒，执行批次共用最多 300 秒的预算（不含已有的有界扫描与物化步骤）；stdout/stderr 各保留至多 1 MiB 并继续排空，截断显式标记。运行与后台子进程使用独立进程组，超时、错误和正常主进程退出后均回收进程组；回收失败不能报告通过。
4. `baseline scan` 与原 HTTP prepare 契约不变。新增明确的 `baseline verify --timeout-seconds N` 和实例认证的执行端点；请求不接受任意命令、actor、批准字段、输出路径或关闭隔离参数。CLI 先输出真实 JSON 回执，仅所有验证计划通过时退出 0；失败、部分执行、无法执行或无可执行命令均退出非 0，无测试不产生通过证据。运行前严格校验显式 run、clone 绑定及 hub 权威，执行期间不持 hub 锁；完成后续租不得掩盖真实结果。
5. 日志由 daemon 的唯一产物写模块保存到主仓库 `<docs_dir>/artifacts/<verification-id>/`，独立运行目录使用 UUID，不覆盖文件；拒绝符号链接路径，确认该目录被 Git 忽略（未忽略则使用本 clone `info/exclude` 登记，不修改全局配置）。结果含实际 commit/tree、命令来源、cwd、状态、退出码、隔离版本、日志相对路径与 SHA-256、`local_only`。日志失败不记录成功证据。
6. 每次验证结果经 hub `evidence.recorded`、`process:towermd-verifier`、L5 事件落盘，费用为 null；快照投影登记 Evidence，重放保持不可变。当前日志采用规格允许的本地忽略目录回退，明确 `local_only`，不宣称 Arca 同步。证据元数据可重建，删除本地日志不能伪称完整恢复。

# 验收与审查

- [ ] 先写失败测试，再实现验证器：结果矩阵、固定输入、拒写、禁网、独立输出、秘密环境清理、超时和后台进程回收、输出上限
- [ ] daemon/CLI 测试：认证及未知字段拒绝、prepare 无执行、执行前 run/clone 校验、命令去重、日志不可变与落盘失败、主工作树/refs/index 不变
- [ ] 证据 journal 重放、快照重建等价，不改变任务完成状态
- [ ] 独立规格审查通过后进行独立质量审查，发现问题修复并复验
- [ ] 真实临时仓库 S5 验收、完整 workspace 测试、fmt、Clippy 和 build

已知范围：严格只读会拒绝要求原地生成文件或在源码旁安装依赖的测试；此类命令应明确失败，不放宽沙箱。App 批准、B0 发布、完整 M1 验证契约与 Arca 自动同步仍为后续项。本轮沿用既有未提交交付方式，禁止提交此前未经独立核对的改动。
