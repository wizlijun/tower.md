---
type: Plan
title: M0 整改批次 0：冻结基线与同步规格
description: 按已批准 D0–D6 保存 M0-1 至 M0-4 四个现状 checkpoint，并先完成 R16 文本一致性验证，为后续 TDD 整改提供可审阅基线。
tags: [towermd, plan, M0, 整改, baseline, spec]
generated: { by: codex/gpt-6, at: 2026-10-16T16:32:24Z }
sources:
  - id: remediation
    resource: docs/superpowers/plans/2026-10-17-m0-remediation.md
    title: M0 整改总表 D0–D6、R16 与批次依赖
  - id: spec
    resource: docs/specs/2026-10-16-spec-towermd-layered-agent-canvas-mvp.md
    title: Tower.md 规格 v0.5
  - id: owner-1017-remediation
    resource: 2026-10-17 用户在对话中确认“全部按推荐方案执行”
    title: D0–D6 的执行授权
status: stable
---

# M0 整改批次 0：冻结基线与同步规格 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 保存四批已验证的现状 checkpoint，并将 R16 规格同步为已批准的 D1–D6 方案，通过文本红绿检查后才开始后续代码计划。

**Architecture:** D0 由主 Agent 在主工作区 `/Users/bruce/git/tower.md` 用冻结显式路径分四次提交；本批规格工作者只修改本计划及目标规格，不暂存、不提交。四批保存的是同一个已验收工作区的模块文件分组；共享 `lib.rs`、`main.rs`、`daemon.rs` 已含后续模块接入，不能假装存在四个独立历史实现。只有四批齐全才形成完整可构建基线，中间 checkpoint 不作独立发布验收。随后单独提交 R16，后续批次先写各自 TDD 计划。

**Tech Stack:** 现有 Rust workspace、Git、Python 3（仅临时文档断言）；不新增依赖。

---

## 文件与责任边界

| 文件 | 操作与责任 |
| --- | --- |
| `docs/specs/2026-10-16-spec-towermd-layered-agent-canvas-mvp.md` | 本工作者修改 R16 与必要伴随一致性表述；不改其他规格 |
| `docs/superpowers/plans/2026-10-17-m0-fix-0-baseline-spec.md` | 本工作者新建本计划；记录授权、冻结路径、检查与依赖 |
| `/private/tmp/towermd-m0-baseline-paths.json` | 主 Agent 已冻结的 46 条 D0 显式路径，按 13/13/9/11 分组；只读核对 |
| `/tmp/towermd-r16-check.py` | 临时文本回归检查，不进入仓库 |
| `tasks/todo.md` | 主 Agent 记录四批基线与 R16 的实际提交号；本工作者不写 |

用户已明确批准推荐执行方案，不再询问执行方式。S5 沙箱工作树 `/private/tmp/towermd-m0-s5` 保留为安全加固参考，不并入 D0。`AGENTS.md`、`CLAUDE.md`、`tasks/` 与本轮规格改写均不混进四批现状提交。本轮不推送。

### Task 1: D0 四个现状 checkpoint（主 Agent 执行）

**Files:** 下列冻结清单全部已在工作区；只提交清单路径，不修改源码，不将规格或 S5 新文件混入。

**M0-1 — 13 files**

- `.gitignore`
- `Cargo.toml`
- `Cargo.lock`
- `crates/towermd-core/Cargo.toml`
- `crates/towermd-core/src/lib.rs`
- `crates/towermd-core/src/config.rs`
- `crates/towermd-core/src/init.rs`
- `crates/towermd-core/src/okf.rs`
- `crates/towermd-core/src/repo.rs`
- `crates/towermd-core/src/types.rs`
- `crates/towermd-core/templates/agents.md`
- `crates/towermd-core/tests/init_push.rs`
- `docs/superpowers/plans/2026-10-16-m0-1-core-objects-init.md`

**M0-2 — 13 files**

- `crates/towermd/Cargo.toml`
- `crates/towermd/src/main.rs`
- `crates/towermd/src/daemon.rs`
- `crates/towermd/tests/init_cli.rs`
- `crates/towermd/tests/daemon_cli.rs`
- `crates/towermd-core/src/catalog.rs`
- `crates/towermd-core/src/hub.rs`
- `crates/towermd-core/src/tasks.rs`
- `crates/towermd-core/tests/catalog.rs`
- `crates/towermd-core/tests/hub.rs`
- `crates/towermd-core/tests/tasks.rs`
- `crates/towermd-core/tests/repo_timeout.rs`
- `docs/superpowers/plans/2026-10-16-m0-2-local-hub-daemon.md`

**M0-3 — 9 files**

- `crates/towermd-core/src/recovery.rs`
- `crates/towermd-core/src/snapshot.rs`
- `crates/towermd-core/src/snapshot_git.rs`
- `crates/towermd-core/src/snapshot_service.rs`
- `crates/towermd-core/tests/hub_checkpoint.rs`
- `crates/towermd-core/tests/snapshot.rs`
- `crates/towermd-core/tests/snapshot_git.rs`
- `crates/towermd-core/tests/snapshot_service.rs`
- `docs/superpowers/plans/2026-10-16-m0-3-snapshot-recovery.md`

**M0-4 — 11 files**

- `crates/towermd-core/src/canvas.rs`
- `crates/towermd-core/src/canvas_view.rs`
- `crates/towermd-core/src/coldstart.rs`
- `crates/towermd-core/tests/canvas.rs`
- `crates/towermd-core/tests/canvas_view.rs`
- `crates/towermd-core/tests/coldstart.rs`
- `crates/towermd/tests/coldstart_cli.rs`
- `docs/superpowers/plans/2026-10-16-m0-4-canvas-coldstart.md`
- `README.md`
- `CHANGELOG.md`
- `CHANGELOG.zh-CN.md`

- [x] **Step 1: 核验冻结路径覆盖与排除项**

```bash
python3 - <<'PYTHON'
import json
from pathlib import Path
p = json.loads(Path('/private/tmp/towermd-m0-baseline-paths.json').read_text())
assert list(p) == ['M0-1', 'M0-2', 'M0-3', 'M0-4']
assert [len(v) for v in p.values()] == [13, 13, 9, 11]
flat = [x for group in p.values() for x in group]
assert len(flat) == len(set(flat)) == 46
assert all(Path(x).is_file() for x in flat)
assert not any(x.startswith(('tasks/', 'docs/specs/')) or x in ('AGENTS.md', 'CLAUDE.md') or '/s5' in x or '/verifier' in x for x in flat)
print('PASS D0 frozen paths: 13/13/9/11; 46 unique files; no S5/spec/tasks')
PYTHON
```

Expected: 一行 PASS；不改变文件或 index。主 Agent 另以开始时 SHA 清单确认本轮未修改源码。文档中 S5 已完成说法须与开始时验收基线一致，不能为未同步的 S5 声称交付。

- [x] **Step 2: 核对开始时验证证据**

基线已有 320 项通过的测试证据；主 Agent 核对冻结文件与该工作区字节一致。完整四批提交后运行：

```bash
cargo test --workspace --locked
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
```

Expected: 全部通过。中间 checkpoint 不运行或声称独立可构建验收，不用重写共享文件伪造历史。

- [x] **Step 3: 按冻结数组逐批精确暂存、检查与提交**

由主 Agent 每次只选择 `M0-1`、`M0-2`、`M0-3`、`M0-4` 中下一批；以下脚本完整展示逐批命令，但执行时每批结束先审阅输出与提交号：

```python
import json
import subprocess
from pathlib import Path
batches = json.loads(Path('/private/tmp/towermd-m0-baseline-paths.json').read_text())
for name, paths in batches.items():
    subprocess.run(['git', 'add', '--', *paths], check=True)
    staged = subprocess.check_output(['git', 'diff', '--cached', '--name-only', '-z']).decode().split('\0')
    assert set(filter(None, staged)) == set(paths), name
    subprocess.run(['git', 'diff', '--cached', '--check'], check=True)
    subprocess.run(['git', 'commit', '-m', f'baseline: preserve {name} current implementation', '-m', 'Known deviations: docs/superpowers/plans/2026-10-17-m0-remediation.md. Full baseline requires all four checkpoints; S5 excluded.'], check=True)
    print(name, subprocess.check_output(['git', 'rev-parse', 'HEAD']).decode().strip())
```

Expected: 连续四个 checkpoint，每次提交文件集与对应冻结数组完全一致；不推送。

- [x] **Step 4: 记录实际提交证据**

主 Agent 将四个 SHA、完整基线的最后一个 SHA、测试结果写入 `tasks/todo.md`，明确中间提交是文件分组 checkpoint。本工作者不写该文件。

### Task 2: R16 文本 TDD：先证明旧规格不能表达批准结论

**Files:**
- Test (temporary): `/tmp/towermd-r16-check.py`
- Read: `docs/superpowers/plans/2026-10-17-m0-remediation.md`
- Modify: `docs/specs/2026-10-16-spec-towermd-layered-agent-canvas-mvp.md`

- [x] **Step 1: 写临时文本一致性断言**

把下列完整脚本保存到 `/tmp/towermd-r16-check.py`；这是本批文档的红绿证据，不作为产品运行测试：

```python
from pathlib import Path
s = Path('docs/specs/2026-10-16-spec-towermd-layered-agent-canvas-mvp.md').read_text()
checks = {
    'D1 私有快照 CAS': all(x in s for x in ['refs/towermd/snapshot', '比较并交换', 'main` 与私有引用']),
    'D2 App 确认': 'POST /hub/rebuild/:request_id/confirm' in s and 'M0 以 App 客户端身份' in s,
    'D3 共用临时验证器': all(x in s for x in ['$(git-common-dir)/towermd/verify/', '继承用户的环境变量', '回收整个进程组', '不触碰用户工作树的临时分离工作树']),
    'D4 尽力预检': all(x in s for x in ['尽力检查', '网络失败、认证失败', '快照写入时会再次检查']),
    'D5 有界恢复元数据': all(x in s for x in ['不保存 receipts', '非心跳事件的 `eid → seq` 从归档', '省略心跳的 `seq` 区间']),
    'D6 冷启动检查': 'M0 保留覆盖率指标' in s and '每层模块数应为 3–9' in s,
    'R6 协议抽象': all(x in s for x in ['HubProtocol', 'lease_acquire', 'lease_renew', 'lease_release', 'snapshot_ack', '业务投影由守护进程中的 `towermd-core` 计算', '调用方将其作为不透明检查点提供', 'hub 只持久化该数据及定序元数据']),
    'R7 连续前缀': 'contiguous_seq()' in s and '乱序到达的测试替身' in s,
    'R8 扫描等待': 'baseline scan --wait <scan-id>' in s and '每次最多等 90 秒' in s and '不增加独立的 `baseline verify`' in s,
    '确认与修订溯源': '[^owner-1017-remediation]' in s and '## 修订记录' in s,
}
for name, ok in checks.items():
    print(('PASS ' if ok else 'FAIL ') + name)
assert all(checks.values()), 'R16 文本一致性检查未通过'

```

- [x] **Step 2: 在修改前运行，确认红灯**

```bash
python3 /tmp/towermd-r16-check.py
```

Expected before edit: 10 行 FAIL 和 `AssertionError: R16 文本一致性检查未通过`；已实际观察到，说明原文缺少已批准的契约。

- [x] **Step 3: 按批准结论做最小规格同步**

精确修改以下位置；不改需求编号，不新增产品功能或依赖：

| 位置 | 必须落入规格的最终契约 |
| --- | --- |
| sources、修订记录与脚注 | 用户 2026-10-17 确认 D0–D6“全部按推荐方案执行”；署名 `codex/gpt-6`，不伪造 human 动作 |
| FR-83、FR-110 | 慢层仍只认权威 main；仅快照在 main/private 中取最大 `(epoch, seq)`，上任补 ack 同源 |
| FR-87、FR-89、FR-92、5.3 澄清 | hub 检查点保留非心跳 eid 索引；业务投影由 daemon 的 towermd-core 计算，调用方提供不透明检查点，hub 只持久化该数据与定序元数据、不解释业务 data；state 不存 receipts/eid 索引/逐条心跳，归档推导 eid；省略心跳 seq 区间与恢复跳号保留；ack 之后可压缩 |
| FR-91 | 无远端且 main 已检出时以 CAS 更新 `refs/towermd/snapshot`；不移动 branch/worktree/index；main 可写或有远端时写回 main；fencing 与最多三次非快进重做；init 尽力检查，网络/auth/无 main 警告，明确策略拒绝硬错 |
| FR-93、5.3 澄清 | CLI 只请求；App 确认后重建；trailer 回放、未知 trailer 告警、发件箱去重及丢弃续租；先持久化新边界到 main/private 再开放写服务；空 hub 的 status 可只读 |
| FR-16、FR-106、G0 与 7.1 流程图 | 追认 allowed_paths 授权、module paths 上限；先 worktree/branch 后 claim；无相关 Decision 允许，显式引用须在权威 main；流程图已有正确顺序，保持 |
| 第 8 节 S3/S5、8.2、FR-107 | 每层 3–9，无法满足须诊断；M0 保留覆盖率加命令/无测试候选任务；S5/finish 共用 git-common-dir 下临时 detached worktree，继承 env、允许网络、超时回收整个进程组；模块独立证据、root 命令共享且只跑一次 |
| 第 8 节草稿、16.2 | 可重建草稿保存到 `$(git-common-dir)/towermd/drafts/<scan-id>/`，不写 docs_dir/main；GET drafts 读取；S8 才出版 |
| 16.1 | scan 异步返回 ID，`baseline scan --wait <scan-id>` 每次最多等 90 秒；不增加 baseline verify |
| 16.2 | `POST /hub/rebuild/:request_id/confirm` 仅接受 App 客户端；v0.1 仅按客户端类型区分；请求持久化到运行时目录，不续租 |
| 6.3 与 16.3 | 事件表本来已正确 L5/epoch，保持；`HubProtocol` 七操作与不透明 data；claim 先 lease 后带 epoch append，失败释放；过期/放弃发 task.released；`contiguous_seq()` 查询与乱序替身契约验收 |
| NFR-11 | 私有快照也是标准 Git 引用，保持可移除语义 |
| 18 M0/M1 | M0 真守护进程路径 App 客户端确认重建、私有快照与旧 epoch、连续前缀替身、草稿可读/模块证据/层数/无测试任务；真人点击留 M1 |

- [x] **Step 4: 再运行验证绿灯**

```bash
python3 /tmp/towermd-r16-check.py
git diff --check -- docs/specs/2026-10-16-spec-towermd-layered-agent-canvas-mvp.md
```

Expected: 10 行 PASS，diff 检查无输出；已实际观察到。产品行为测试留给各代码批次，不把文本检查当作 M0 功能验收。

### Task 3: 规格自审与独立 R16 提交（主 Agent 已执行）

**Files:**
- Review/Commit: `docs/specs/2026-10-16-spec-towermd-layered-agent-canvas-mvp.md`
- Review/Commit: `docs/superpowers/plans/2026-10-17-m0-fix-0-baseline-spec.md`

- [x] **Step 1: 检查差异与矛盾句**

```bash
git diff 25092ce -- docs/specs/2026-10-16-spec-towermd-layered-agent-canvas-mvp.md
rg -n 'receipts|eid → seq|refs/towermd/snapshot|只读工作树|一律推迟|baseline verify|App 客户端|contiguous_seq' docs/specs/2026-10-16-spec-towermd-layered-agent-canvas-mvp.md
git diff --check -- docs/specs/2026-10-16-spec-towermd-layered-agent-canvas-mvp.md docs/superpowers/plans/2026-10-17-m0-fix-0-baseline-spec.md
```

Expected: 无“一律推迟”或 S5“只读工作树”的残余；receipts 只出现在“不保存”约束；baseline verify 只出现在“不增加”约束。检查私有快照读/写/ack/恢复/交接统一、慢层保护没有新增绕过、没有 CLI 批准命令。

- [x] **Step 2: 确认计划无占位与跨批实现**

```bash
rg -n 'T[B]D|T[O]DO|implement [l]ater|fill in [d]etails|Add appropriate error [h]andling|Similar to [T]ask' docs/superpowers/plans/2026-10-17-m0-fix-0-baseline-spec.md
```

Expected: 没有命中（退出码 1）。文件图与步骤均有精确路径；只同步本批文档。

- [x] **Step 3: 单独暂存本批文档与已批准的整改总表、审阅后提交**

```bash
git add -- docs/specs/2026-10-16-spec-towermd-layered-agent-canvas-mvp.md docs/superpowers/plans/2026-10-17-m0-fix-0-baseline-spec.md docs/superpowers/plans/2026-10-17-m0-remediation.md
git diff --cached --name-only
git diff --cached --check
git commit -m 'docs: align M0 remediation spec with approved decisions'
git rev-parse HEAD
```

Expected: 仅规格、本批计划与包含用户裁决记录的整改总表这三个文档；D0 四 checkpoint 完成后才执行。实际 R16 提交为 `b5a881f8d5f8e32a0d17816fc81f12ad2edd3637`，未推送；任务记录保存四个基线 SHA 与验证结果。

## 后续批次依赖与交接

| 批次 | 整改项 | 开工条件与验证责任 |
| --- | --- | --- |
| 1 | R1/R2 | 批次 0 完成；另写离线读取/续租、后台 fetch、init 的失败用例与 TDD 计划 |
| 2 | R6/R5 | 批次 0 完成；先协议契约失败用例，再存储有界与增量投影；R5 依赖 R6 |
| 3 | R3/R7/R4/R14 | 批次 2 完成；私有 ref/CAS 与 branch/worktree/index 不变、非快进重做、App 重建端到端、事件触发快照 |
| 4 | R8/R9 | 批次 0 完成，证据写入依赖 R6；统一验证器、超时整个进程组、模块证据、层数、可重建草稿 |
| 5 | R10–R13/R15 | 可穿插；各项独立复现测试，最后准确同步 README/双语 CHANGELOG、任务提交证据 |

各代码批次必须先写自己的精确 TDD 计划，再执行失败复现、最小实现、绿灯与规格/质量审查；完成后运行 workspace test/fmt/clippy，并只提交自身已验证的显式路径。本文件不代写后续批次计划，也不将规格同步当作全部整改完成。

[^owner-1017-remediation]: 2026-10-17 用户对整改总表 D0–D6 确认“全部按推荐方案执行”；规格修订脚注沿用同名来源。
