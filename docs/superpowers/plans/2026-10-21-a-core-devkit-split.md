---
type: ImplementationPlan
title: A 部分：核心与开发套件拆成两个 crate 的实现计划
status: draft
generated:
  by: claude-code/opus-5.5
sources:
  - resource: /docs/superpowers/specs/2026-10-21-core-devkit-split-and-cli-c0-design.md
  - resource: /docs/specs/2026-10-21-spec-towermd-core.md
  - resource: /docs/specs/2026-10-21-spec-towermd-development-kit.md
  - resource: /docs/specs/2026-10-21-spec-towermd-cli.md
  - resource: /AGENTS.md
  - resource: /tasks/todo.md
---

# 核心与开发套件拆分（A 部分）Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把软件开发专属的业务（基线、任务、运行、完成证据、提案、冷启动与核验）从 `towermd-core` 搬进新的 `towermd-devkit` crate，经编译期 `Kit` trait 挂回核心。磁盘格式和事件名、字段一律不变，v0.1.1 的日志和快照重放结果逐字节一致。

**Architecture:** 先在核心里开出接缝（Kit trait、`initial_with`/`apply_with` reducer、`Assembly` 证明、`PathRule` 与 `SelfReported` 归位），让核心的非开发代码不再引用开发模块。然后用一次 `git mv` 把开发模块整体搬到 `crates/towermd-devkit`，同时把二进制的服务分进 `cli/` 和 `dev/`。最后清理核心里残留的开发词汇（工作租约、`scope`、init 拆分），并用 `boundary.rs` 护栏锁住。重放等价由 T1 先录下的四份真实 CLI 流程固定样本守护。

**Tech Stack:** Rust stable，edition 2021；Cargo workspace；serde/serde_json/serde_yaml；`cargo test`；perl 5 做机械改写。

---

## 命令约定

- 所有命令都在仓库根目录 `/Users/bruce/git/tower.md` 下执行。
- 每个 shell 会话先执行 `export PATH="$HOME/.cargo/bin:$PATH"`，否则找不到 `cargo`。
- `BASE=03f15f4`：本计划起点提交，HEAD 为 `fix(core): release outbox lock on drop despite inherited descriptors`。T5 用 `git show $BASE:...` 取原文件。
- 辅助脚本和临时文件一律放在 `target/` 下（已被 git 忽略），不进仓库。
- 工作区原有的 `README.md` 修改和未跟踪的 `docs/usage.md` 不属于本计划。提交时只 `git add` 计划列出的路径，**不要用 `git add -A` 或 `git commit -a`**。
- 每个任务只提交一次，提交信息以一个空行加 `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>` 结尾，写法统一为 `git commit -m "<标题>" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"`。
- 测试数的统计方法（与基线相同）：
  ```bash
  cargo test --workspace -- --list 2>/dev/null | grep -c ': test$'
  cargo test --manifest-path src-tauri/Cargo.toml -- --list 2>/dev/null | grep -c ': test$'
  ```
- 基线（控制方已在 03f15f4 上记录）：workspace 381 passed、0 failed、0 ignored；src-tauri 19 passed、1 ignored。
- 每个任务的新增测试数是固定的，见各任务的“预期”一行。全部任务完成后，workspace 应为 381 + 19 = **400**，src-tauri 不变。

### 共用辅助脚本（T5 首次使用前创建，T6、T7 复用）

`target/split-use.pl`：把 `use FROM::{...}` 块里属于指定模块的条目拆进新的 `use TO::{...}`，并把其余 `FROM::<模块>` 路径改写成 `TO::<模块>`。

```perl
#!/usr/bin/perl
# Usage: perl split-use.pl FROM TO mod1,mod2,... FILE...
# 1. In every `use FROM::{...};` block, top-level items whose first segment is
#    one of the listed modules move into a new `use TO::{...};` block.
# 2. Every remaining path `FROM::<listed module>` becomes `TO::<listed module>`.
use strict;
use warnings;

my ($from, $to, $mods, @files) = @ARGV;
die "usage: split-use.pl FROM TO mods FILE...\n" unless @files;
my %move = map { $_ => 1 } split /,/, $mods;
my $alt = join '|', map { quotemeta } sort keys %move;
my $nested = qr/(?:[^{}]|\{(?:[^{}]|\{[^{}]*\})*\})*/;

for my $file (@files) {
    open my $in, '<', $file or die "$file: $!";
    my $src = do { local $/; <$in> };
    close $in;
    my $orig = $src;
    $src =~ s{^([ \t]*)use \Q$from\E::\{($nested)\};}{split_use($1, $2)}mge;
    $src =~ s{\b\Q$from\E::($alt)\b}{$to\::$1}g;
    next if $src eq $orig;
    open my $out, '>', $file or die "$file: $!";
    print $out $src;
    close $out;
    print "rewrote $file\n";
}

sub split_use {
    my ($indent, $body) = @_;
    my @items;
    my $cur = q{};
    my $depth = 0;
    for my $ch (split //, $body) {
        if ($ch eq ',' && $depth == 0) { push @items, $cur; $cur = ''; next; }
        $depth++ if $ch eq '{';
        $depth-- if $ch eq '}';
        $cur .= $ch;
    }
    push @items, $cur;
    my (@keep, @moved);
    for my $item (grep { /\S/ } map { s/^\s+|\s+$//gr =~ s/\s+/ /gr } @items) {
        my ($head) = $item =~ /^(\w+)/;
        if ($move{$head}) { push @moved, $item } else { push @keep, $item }
    }
    my @out;
    push @out, "${indent}use $from\::{" . join(', ', @keep) . "};" if @keep;
    push @out, "${indent}use $to\::{" . join(', ', @moved) . "};" if @moved;
    return join "\n", @out;
}
```

`target/add-arg.pl`：给匹配的调用追加最后一个实参，括号可跨行嵌套。

```perl
#!/usr/bin/perl
# Usage: perl add-arg.pl 'CALLEE_REGEX' 'ARG' FILE...
# Appends ARG as the last argument of every call whose callee path matches
# CALLEE_REGEX (balanced parentheses, may span lines). Prints counts.
use strict;
use warnings;
my ($callee, $arg, @files) = @ARGV;
die "usage: add-arg.pl CALLEE_REGEX ARG FILE...\n" unless @files;
for my $file (@files) {
    open my $in, '<', $file or die "$file: $!";
    my $src = do { local $/; <$in> };
    close $in;
    my $count = $src =~ s{($callee)(\((?:[^()]++|(?2))*\))}{$1 . substr($2, 0, -1) . ", $arg)"}ge;
    open my $out, '>', $file or die "$file: $!";
    print $out $src;
    close $out;
    print "$file: $count\n";
}
```

`CALLEE_REGEX` 里不能带捕获组，只能用 `(?:...)`，因为脚本用 `(?2)` 递归第二组。

---

## 与设计的偏差

设计文档是 `docs/superpowers/specs/2026-10-21-core-devkit-split-and-cli-c0-design.md` 第 2 节。下面每条偏差都给出理由，执行前请控制方确认。

| # | 设计 | 本计划 | 理由 |
| --- | --- | --- | --- |
| D1 | A1 先搬 `baseline`、`baseline_publication`、`draft_store`、`review`、`review_verification`，A2 再解耦 | 先开接缝（T2–T6），再在 T7 一次性 `git mv` 全部开发模块；二进制分目录（设计 A1 的另一半）放在 T8 | 实测这五个模块并非“没有其他模块依赖”：核心的 `project_state` 用 `baseline_publication::from_approval`，`main_inspection` 用它做提交证明，`proposal_model` 用 `baseline`，`question_projection::resolve_publication` 用 `proposal_model::Proposal`。先搬会形成 core → devkit 的反向依赖，无法编译。 |
| D2 | `ProjectState` 加 `extensions: BTreeMap<kit_id, Value>` 分区 | 投影保持扁平的 `serde_json::Value`，Kit 只登记自己的顶层组名（`state_groups`） | 现有投影就是扁平 `Value`，没有 `ProjectState` 结构体。分区会改变快照里的投影形状，违反“v0.1.1 重放结果一致”。 |
| D3 | `Kit: Send + Sync + 'static`，有 `id`、`object_types`、`reduce -> Reduced`、`init_project(ctx)` | `Kit: Sync`，每个方法都有默认实现：`state_groups`、`reduce -> Result<bool,String>`、`runs`、`prove_protected_commit`、`init`；每个方法只在第一个用到它的任务里加入；`Reducer` 仍是 `fn` 指针，通过 `Assembly { kits, reduce }` 组装 | YAGNI。`ProjectRuntime` 只接受 `fn` 指针 reducer（`project_runtime.rs:22`），改成 trait 对象会波及 runtime 和所有调用方。`id`、`object_types` 现在没有调用者。 |
| D4 | `task_model` 拆成核心 `task` 和 devkit `task_workspace` | `task_model` 整体搬进 devkit；核心 Task 另行立项 | `Task`、`Run` 序列化成一个对象写入投影和事件（`task.created`/`claimed` 的 `data`），拆字段会改变线上格式。核心目前没有任何非开发的 Task 使用者。 |
| D5 | `path_guard` 留核心（一致） | 另外把 `task_model` 里的 `PathRule`、`path_is_allowed`、`path_is_within` 移进 `path_guard`，`task_model` 用 `pub use` 转出 | `path_guard` 原本反向依赖 `task_model` 的这两个函数，不移就无法让 `task_model` 离开核心。 |
| D6 | `proposal_model` 留核心 | 搬进 devkit | 它校验基线标签、模块影响面和任务复核（`proposal_model.rs:199` 读 `task.input.module`），属于开发业务。核心仍保留投影组 `"Proposal"`，因为核心的 `question_projection` 也读写它。 |
| D7 | `types.rs` 拆成核心登记加 Kit 登记 | 不动 `types.rs` | 注册表是静态表，拆开需要运行时组装，并且会影响 `catalog` 的读取。本轮不改，另行立项。 |
| D8 | hub 改成通用 `lease_key: String` | 不加新字段；把“`module:` 前缀”泛化为“任何非空 `<kind>:<id>` 名称都是工作租约”（`hub::is_work_lease`） | 加字段会改变 hub 协议和日志格式。现有租约名 `main`、`maintenance` 不带冒号，所以泛化后它们的行为不变。 |
| D9 | `question_model` 改成 `scope`，旧数据读时把 `module` 映射为 `scope` | Rust 字段改名为 `scope`，用 `#[serde(rename = "module")]` 保持线上键名为 `module`；`PromptScope::Module` 改名为 `Scoped`，`kind` 仍是 `"module"`；不写旧数据映射；另新增 `question_model::SelfReported`，`task_model::HarnessSource` 成为它的别名 | 线上键名不变，旧数据天然能读，也就不需要映射。`question_model` 原本引用 `task_model::HarnessSource`，不先归位就无法搬走 `task_model`。 |
| D10 | `decisions/` 归 devkit；核心只负责 config、docs_dir、canvas 和通用模板段落 | `decisions/` 与 `PROTECTED_SUBDIRS` 留在核心；devkit 贡献 `intents`、`arch`、`specs`、`baselines` 四个目录、模板后半段和技能提示 | `decisions/` 由核心的 `question_model` 和 `catalog` 读写（决策记录是核心问题流程的产物）；`PROTECTED_SUBDIRS` 是核心 `path_guard` 和 `catalog` 的保护边界，移走就会出现核心不保护的慢层目录。init 当前也不创建 `tower.canvas`，本轮不新增。 |
| D11 | `boundary.rs`：禁用词零命中，“注释也算” | 一行白名单：`main_writer.rs` 里的 `&["worktree", "list", "--porcelain", "-z"],`；另外把两条注释改写（`durable_outbox.rs:64`、`main_writer.rs:844`） | 这一行是 git 子命令名。核心单写者必须检查其他 checkout 是否占用 `main`，换掉会削弱慢层保护。 |
| D12 | 二进制 `cli/` 里含 `project_service`，`dev/` 里含 scan 相关服务 | `project_service.rs` 留在 `src/` 作为 `#[path]` 汇总模块；不存在 scan 服务文件，扫描逻辑在 `project_service.rs` 和 `worker.rs` 里，本轮不动 | 移动 `project_service.rs` 会让它下面的全部 `#[path]` 换基准目录，收益为零。 |
| D13 | 用“现有 replay 测试加一个固定样本”验证重放 | 固定样本取自四个真实 CLI 测试流程结束时的 hub 日志（finish、app_pause、proposal、controls），共四份 | 单个样本覆盖不到 `task.merged`、`project.paused`、`proposal.submitted` 等关键事件。录制钩子默认不生效，只在设置 `TOWERMD_RECORD_REPLAY_GOLDEN` 时写文件。 |
| D14 | `tests/` 跟着模块走 | 另外把依赖开发装配的三个核心集成测试（`main_inspection.rs`、`main_resolution.rs`、`project_controls.rs`）移到 devkit；`tests/main_writer.rs` 里调用 `baseline_publication` 的 486–610 行拆成 `towermd-devkit/tests/baseline_remote_publication.rs` | 核心不能 dev-depend devkit（`boundary.rs` 会检查），这些测试又必须用真实的 DevKit 装配才有意义。测试总数不变。 |

## 代码事实与设计假设不符之处

1. A1 的五个模块不能先搬，原因见 D1。
2. 投影是扁平的 `serde_json::Value`，不存在 `ProjectState` 结构体（`project_state.rs:7-10`）。
3. `Reducer` 是 `fn(&mut Value,&Event)->Result<(),String>` 指针（`project_runtime.rs:22`），不是 trait 对象。
4. `types.rs` 是静态注册表，没有组装入口。
5. `Task` 和 `Run` 整体序列化进事件和投影，拆成核心 Task 会改格式。
6. 禁用词 `module:` 会命中字段声明，例如 `pub module: String,`；`question_model` 里有 8 处。
7. `main_writer.rs:533` 合法地调用 `git worktree list`。
8. `PROTECTED_SUBDIRS`、`decisions/` 的使用者在核心。
9. `AGENTS.md` **已被 git 跟踪**（`git ls-files --error-unmatch AGENTS.md` 成功），与控制方“未跟踪”的说法不符，T13 照常提交。
10. `proposal_model` 只处理 `proposal.submitted`、`proposal.captured`、`proposal.rejected`，内容与基线和模块绑定，是开发业务。
11. 被搬走的内联测试用字面量构造 `Event`，其中 `lease_delta`、`heartbeat`、`extra` 是 `pub(crate)` 字段，跨 crate 无法编译，必须改成 `serde_json::from_value`。
12. 字段改名后线上键名不变，所以设计设想的“旧数据映射”不需要。
13. 不存在 scan 服务文件。
14. `project_service.rs` 是 `#[path]` 汇总模块，移动它没有收益。
15. src-tauri 有自己的 `PromptScope`（`src-tauri/src/backend.rs:132`），只按 JSON 读，不受 T10 影响。
16. 核心 `tests/main_writer.rs` 的 486–610 行依赖 `baseline_publication`。
17. 设计说 `proposal_model_tests` 引用 `baseline_publication`，实际不引用；它引用的是 `project_state` 和 `Event` 字面量。
18. 核心被搬走的代码用到的 `pub(crate)` 项只有 `repo::git_timeout_env_output` 和 `repo::git_timeout_env_input_output`（T7 改为 `pub`），以及 `Event` 的三个 `pub(crate)` 字段（见第 11 条）。

## 文件地图

**新建**

| 路径 | 职责 |
| --- | --- |
| `crates/towermd-core/src/kit.rs` | `Kit` trait、`KitRun`、`InitContribution`、`NO_PROOF`、`prove_by_kits`、`Assembly` |
| `crates/towermd-core/tests/kit.rs` | Kit 接缝的行为测试（组名、委派、收件人、证明、装配） |
| `crates/towermd-core/tests/boundary.rs` | 核心禁用词护栏、核心清单不依赖 devkit |
| `crates/towermd/tests/support/golden.rs` | 固定样本录制钩子（默认不生效） |
| `crates/towermd-core/tests/replay_golden.rs` → T7 移到 `crates/towermd-devkit/tests/replay_golden.rs` | 重放等价测试 |
| `crates/towermd-core/tests/fixtures/replay-v0.1.1/{finish,app_pause,proposal,controls}/{events.jsonl,projection.json,initial.json}` → T7 移到 devkit | 固定样本 |
| `crates/towermd-core/src/dev_state.rs` → T7 移到 `crates/towermd-devkit/src/dev_state.rs` | DevKit：开发 reducer、运行列表、基线证明、init 贡献、`KITS`、`ASSEMBLY` |
| `crates/towermd-devkit/Cargo.toml`、`src/lib.rs` | 新 crate |
| `crates/towermd-devkit/templates/agents-dev.md` | `.towermd/AGENTS.md` 模板的开发段落（第 21 行起） |
| `crates/towermd-devkit/tests/init.rs`、`tests/fixtures/agents-v0.1.1.md` | init 输出与 v0.1.1 逐字节一致 |
| `crates/towermd-devkit/tests/baseline_remote_publication.rs` | 从核心 `tests/main_writer.rs` 拆出的基线出版测试 |

**移动（T7，`git mv`）**：核心 `src/` 下的 `task_model`、`task_finish`、`task_finish_tests`、`task_alerts`、`task_alerts_tests`、`verifier`、`cold_start`、`cold_start_verification`、`baseline`、`baseline_publication`、`draft_store`、`review`、`review_verification`、`proposal_model`、`proposal_model_tests`、`dev_state`（均为 `.rs`），以及 `schema/`；核心 `tests/` 下的 `baseline`、`cold_start`、`cold_start_verification`、`review`、`verifier`、`main_inspection`、`main_resolution`、`project_controls`、`replay_golden`（均为 `.rs`），以及 `fixtures/`。

**移动（T8）**：`crates/towermd/src/*_service.rs` 分进 `src/cli/` 和 `src/dev/`。

**修改**：根 `Cargo.toml`、`Cargo.lock`、`crates/towermd/Cargo.toml`、`src-tauri/Cargo.toml`、`src-tauri/Cargo.lock`、核心 `lib.rs`、`project_state.rs`、`project_controls.rs`、`question_projection.rs`、`question_model.rs`、`path_guard.rs`、`main_inspection.rs`、`snapshot_publication.rs`、`hub.rs`、`project_runtime.rs`、`init.rs`、`repo.rs`、`durable_outbox.rs`、`main_writer.rs`、`templates/agents.md`、二进制各服务与 `main.rs`、src-tauri 的 `backend.rs`/`lib.rs`（仅 `use` 路径）、`AGENTS.md`、`tasks/todo.md`。

---

### Task 0：确认基线

**Files:** 无改动，无提交。

- [ ] **Step 1：确认起点**

```bash
export PATH="$HOME/.cargo/bin:$PATH"
git rev-parse --short HEAD
git status --short
```

预期：`03f15f4`（或控制方在其后的提交）；状态里只有 ` M README.md` 和 `?? docs/usage.md`。

- [ ] **Step 2：引用基线测试数**

不重复统计。控制方已用“命令约定”里的方法记录：workspace 381、src-tauri 19 passed + 1 ignored。另外确认当前核心不依赖 devkit：

```bash
cargo tree -p towermd-core | grep -c towermd-devkit
```

预期输出 `0`（grep 无命中，退出码 1，属正常）。

---

### Task 1：录制 v0.1.1 重放固定样本

在动任何 reducer 之前，用当前代码录下四个真实流程的 hub 日志和投影。

**Files:**
- Create: `crates/towermd/tests/support/golden.rs`
- Create: `crates/towermd-core/tests/replay_golden.rs`
- Create: `crates/towermd-core/tests/fixtures/replay-v0.1.1/**`（录制产物）
- Modify: `crates/towermd/tests/task_cli.rs`（第 374 行前）、`crates/towermd/tests/question_cli.rs`（第 387 行前）、`crates/towermd/tests/proposal_cli.rs`（第 149 行后）、`crates/towermd/tests/project_control_cli.rs`（第 50 行前）

- [ ] **Step 1：写失败测试 `crates/towermd-core/tests/replay_golden.rs`**

```rust
//! v0.1.1 replay lock: journals recorded from real CLI flows must replay to
//! the recorded projection, and every event line must round-trip unchanged.
use std::{collections::BTreeSet, fs, path::PathBuf};

use serde_json::Value;
use towermd_core::{hub::Event, project_runtime, project_state};

const CASES: [&str; 4] = ["finish", "app_pause", "proposal", "controls"];
const REQUIRED_KINDS: [&str; 12] = [
    "baseline.approved",
    "baseline.created",
    "task.created",
    "task.claimed",
    "task.finish.requested",
    "task.finish.started",
    "task.finish.completed",
    "task.g5.completed",
    "task.merged",
    "hold.opened",
    "project.paused",
    "proposal.submitted",
];

fn case_dir(case: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/replay-v0.1.1").join(case)
}

fn lines(case: &str) -> Vec<String> {
    fs::read_to_string(case_dir(case).join("events.jsonl"))
        .unwrap_or_else(|error| panic!("{case}: {error}"))
        .lines()
        .map(str::to_owned)
        .collect()
}

fn events(case: &str) -> Vec<Event> {
    lines(case).iter().map(|line| serde_json::from_str(line).unwrap()).collect()
}

fn recorded(case: &str, name: &str) -> Value {
    serde_json::from_str(&fs::read_to_string(case_dir(case).join(name)).unwrap()).unwrap()
}

#[test]
fn v0_1_1_journals_replay_to_the_recorded_projection() {
    for case in CASES {
        let projection =
            project_runtime::replay(&project_state::initial(), &events(case), project_state::apply)
                .unwrap_or_else(|error| panic!("{case}: {error}"));
        assert_eq!(projection, recorded(case, "projection.json"), "{case}");
    }
}

#[test]
fn v0_1_1_event_lines_round_trip_byte_for_byte() {
    for case in CASES {
        for line in lines(case) {
            let event: Event = serde_json::from_str(&line).unwrap();
            assert_eq!(serde_json::to_string(&event).unwrap(), line, "{case}");
        }
    }
}

#[test]
fn initial_projection_is_unchanged() {
    for case in CASES {
        assert_eq!(project_state::initial(), recorded(case, "initial.json"), "{case}");
    }
}

#[test]
fn golden_covers_the_required_event_kinds() {
    let kinds: BTreeSet<String> =
        CASES.iter().flat_map(|case| events(case)).map(|event| event.kind).collect();
    for kind in REQUIRED_KINDS {
        assert!(kinds.contains(kind), "missing {kind}");
    }
}
```

- [ ] **Step 2：运行，确认失败**

```bash
cargo test -p towermd-core --test replay_golden
```

预期：4 个测试 FAIL，报 `finish: No such file or directory`。

- [ ] **Step 3：写录制钩子 `crates/towermd/tests/support/golden.rs`**

```rust
//! Records v0.1.1 replay fixtures from a finished real CLI flow. Inert unless
//! TOWERMD_RECORD_REPLAY_GOLDEN is set, so normal test runs never write files.
use std::{
    fs,
    path::{Path, PathBuf},
};

pub fn record(root: &Path, case: &str) {
    if std::env::var_os("TOWERMD_RECORD_REPLAY_GOLDEN").is_none() {
        return;
    }
    let journal = fs::read_to_string(root.join(".git/towermd/hub/journal.jsonl")).unwrap();
    // Only newline-terminated lines are durable journal records.
    let complete = &journal[..journal.rfind('\n').map_or(0, |end| end + 1)];
    let mut records = complete.lines();
    let header: serde_json::Value = serde_json::from_str(records.next().unwrap()).unwrap();
    assert_eq!(header["version"], 2, "journal header");
    let lines: Vec<&str> = records.collect();
    let events: Vec<towermd_core::hub::Event> =
        lines.iter().map(|line| serde_json::from_str(line).unwrap()).collect();
    assert_eq!(events.first().map(|event| event.seq), Some(1), "golden needs an uncompacted journal");
    let initial = towermd_core::project_state::initial();
    let projection =
        towermd_core::project_runtime::replay(&initial, &events, towermd_core::project_state::apply)
            .unwrap();
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../towermd-core/tests/fixtures/replay-v0.1.1")
        .join(case);
    fs::create_dir_all(&dir).unwrap();
    let body: String = lines.iter().map(|line| format!("{line}\n")).collect();
    fs::write(dir.join("events.jsonl"), body).unwrap();
    fs::write(dir.join("projection.json"), serde_json::to_string_pretty(&projection).unwrap() + "\n").unwrap();
    fs::write(dir.join("initial.json"), serde_json::to_string_pretty(&initial).unwrap() + "\n").unwrap();
}
```

- [ ] **Step 4：在四个测试里接入钩子**

模块声明：`proposal_cli.rs` 和 `project_control_cli.rs` 放在 `#![cfg(unix)]` 那一行之后；`task_cli.rs` 和 `question_cli.rs` 放在文件第一行。

```rust
#[path = "support/golden.rs"]
mod golden;
```

调用点（`root` 在四个测试里都已是 `&Path`）：

| 文件 | 插入位置 | 插入代码 |
| --- | --- | --- |
| `crates/towermd/tests/task_cli.rs` | `finish_persists_real_report_survives_restart_and_only_main_admission_merges_task` 的结尾 `}`（原第 374 行）之前 | `    golden::record(root, "finish");` |
| `crates/towermd/tests/question_cli.rs` | `app_tasks_and_project_pause_use_real_baseline_and_frozen_checkpoint_messages` 的结尾 `}`（原第 387 行）之前 | `    golden::record(root, "app_pause");` |
| `crates/towermd/tests/proposal_cli.rs` | 原第 149 行之后，即删除 `.git/towermd/hub` 的那一行（原第 150 行）之前 | `    golden::record(root,"proposal");` |
| `crates/towermd/tests/project_control_cli.rs` | 测试结尾 `}`（原第 50 行）之前 | `    golden::record(root,"controls");` |

先插调用点再插模块声明，行号才不会错位；插完用 `grep -n "golden::record" crates/towermd/tests/*.rs` 核对，应有 4 处。

- [ ] **Step 5：录制**

```bash
export TOWERMD_RECORD_REPLAY_GOLDEN=1
cargo test -p towermd --test task_cli finish_persists_real_report_survives_restart_and_only_main_admission_merges_task
cargo test -p towermd --test question_cli app_tasks_and_project_pause_use_real_baseline_and_frozen_checkpoint_messages
cargo test -p towermd --test proposal_cli slow_answer_is_completed_through_a_real_proposal_and_only_publication_resolves_the_hold
cargo test -p towermd --test project_control_cli cli_pause_and_resume_request_keep_process_identity_and_only_app_can_restore_admission
unset TOWERMD_RECORD_REPLAY_GOLDEN
find crates/towermd-core/tests/fixtures -type f | sort
```

预期：4 个测试 PASS，生成 12 个文件。如果某个流程报 `golden needs an uncompacted journal`，说明日志在该点已压缩，把调用点往前移到压缩发生之前，**并在提交说明里写明新位置**。

- [ ] **Step 6：运行重放测试，确认通过**

```bash
cargo test -p towermd-core --test replay_golden
cargo test -p towermd --test task_cli --test question_cli --test proposal_cli --test project_control_cli
```

预期：replay_golden 4 passed，其余不变。如果 `golden_covers_the_required_event_kinds` 报缺失，就在那个流程里把钩子挪到事件出现之后，或在报告中说明缺失的种类，**不要删除**这些必需种类。样本里会带有临时目录路径（例如运行绑定的工作树），事件和投影是自洽的，可以保留。

- [ ] **Step 7：全量回归并提交**

```bash
cargo test --workspace 2>&1 | grep -E '^test result' | awk '{s+=$4; f+=$6} END {print s" passed, "f" failed"}'
git add crates/towermd/tests/support/golden.rs crates/towermd/tests/task_cli.rs crates/towermd/tests/question_cli.rs crates/towermd/tests/proposal_cli.rs crates/towermd/tests/project_control_cli.rs crates/towermd-core/tests/replay_golden.rs crates/towermd-core/tests/fixtures
git commit -m "test: record v0.1.1 replay golden fixtures from real CLI flows" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

预期：385 passed，0 failed（新增 4 个）。

---

### Task 2：Kit 接缝与 `initial_with`

**Files:**
- Create: `crates/towermd-core/src/kit.rs`
- Create: `crates/towermd-core/src/dev_state.rs`（T5 会整体重写）
- Create: `crates/towermd-core/tests/kit.rs`
- Modify: `crates/towermd-core/src/lib.rs`、`crates/towermd-core/src/project_state.rs:1-10`

- [ ] **Step 1：写失败测试 `crates/towermd-core/tests/kit.rs`**

```rust
use serde_json::json;
use towermd_core::{kit::Kit, project_state};

struct Widget;
impl Kit for Widget {
    fn state_groups(&self) -> &'static [&'static str] {
        &["Widget"]
    }
}

#[test]
fn core_initial_holds_only_core_groups() {
    assert_eq!(
        project_state::core_initial(),
        json!({"Hold":{}, "Proposal":{}, "Prompt":{}, "Alert":{}})
    );
}

#[test]
fn kit_groups_join_the_initial_projection() {
    let state = project_state::initial_with(&[&Widget]);
    assert_eq!(state["Widget"], json!({}));
    assert_eq!(state.as_object().unwrap().len(), 5);
}
```

- [ ] **Step 2：运行，确认失败**

```bash
cargo test -p towermd-core --test kit
```

预期：编译失败，`could not find kit in towermd_core`。

- [ ] **Step 3：实现**

`crates/towermd-core/src/kit.rs`：

```rust
//! Compile-time extension seam. A kit adds projection groups and business
//! reducers on top of the core; the core never names a kit's concepts.

pub trait Kit: Sync {
    /// Top-level projection groups this kit owns, inserted before core groups.
    fn state_groups(&self) -> &'static [&'static str] {
        &[]
    }
}
```

`crates/towermd-core/src/dev_state.rs`：

```rust
//! Development kit projection groups (moved to towermd-devkit later).
use serde_json::Value;

use crate::{kit::Kit, project_state};

pub struct DevKit;
pub const GROUPS: &[&str] = &["Baseline", "BaselineApproval", "Task", "Run", "Evidence"];
pub const KITS: &[&dyn Kit] = &[&DevKit];

impl Kit for DevKit {
    fn state_groups(&self) -> &'static [&'static str] {
        GROUPS
    }
}

pub fn initial() -> Value {
    project_state::initial_with(KITS)
}
```

`crates/towermd-core/src/lib.rs`：在 `pub mod project_controls;` 之后加两行：

```rust
pub mod kit;
pub mod dev_state;
```

`crates/towermd-core/src/project_state.rs`：把第 3–10 行

```rust
use serde_json::{json, Value};

use crate::{baseline_publication, hub::Event};

pub fn initial() -> Value {
    json!({"Baseline":{}, "BaselineApproval":{}, "Task":{}, "Run":{},
        "Evidence":{}, "Hold":{}, "Proposal":{}, "Prompt":{}, "Alert":{}})
}
```

替换为

```rust
use serde_json::{json, Map, Value};

use crate::{baseline_publication, hub::Event, kit::Kit};

/// Groups every assembly has, after the kit groups.
pub const CORE_GROUPS: [&str; 4] = ["Hold", "Proposal", "Prompt", "Alert"];

pub fn initial_with(kits: &[&dyn Kit]) -> Value {
    let mut groups = Map::new();
    for name in kits.iter().flat_map(|kit| kit.state_groups().iter()).chain(CORE_GROUPS.iter()) {
        groups.insert((*name).to_owned(), json!({}));
    }
    Value::Object(groups)
}

pub fn core_initial() -> Value {
    initial_with(&[])
}

pub fn initial() -> Value {
    crate::dev_state::initial()
}
```

- [ ] **Step 4：运行，确认通过**

```bash
cargo test -p towermd-core --test kit --test replay_golden
```

预期：kit 2 passed；replay_golden 4 passed，其中 `initial_projection_is_unchanged` 证明组的顺序和内容都没变。

- [ ] **Step 5：全量回归并提交**

```bash
cargo test --workspace 2>&1 | grep -E '^test result' | awk '{s+=$4; f+=$6} END {print s" passed, "f" failed"}'
git add crates/towermd-core/src/kit.rs crates/towermd-core/src/dev_state.rs crates/towermd-core/src/lib.rs crates/towermd-core/src/project_state.rs crates/towermd-core/tests/kit.rs
git commit -m "refactor(core): add Kit seam for projection groups" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

预期：387 passed，0 failed。

---

### Task 3：`PathRule` 移进 `path_guard`

**Files:**
- Modify: `crates/towermd-core/src/task_model.rs:240-300`（移出）、`crates/towermd-core/src/path_guard.rs`（移入并去掉对 `task_model` 的依赖）
- Test: `crates/towermd-core/tests/path_guard.rs`（追加）

- [ ] **Step 1：写失败测试**

追加到 `crates/towermd-core/tests/path_guard.rs` 末尾：

```rust
#[test]
fn path_rules_live_in_path_guard() {
    use towermd_core::path_guard::{path_is_allowed, path_is_within};
    assert!(path_is_within("src/a/**", "src/**").unwrap());
    assert!(!path_is_within("src/**", "src/a/**").unwrap());
    assert!(path_is_allowed("src/a/b.rs", &["src/a/".into()]).unwrap());
    assert!(!path_is_allowed("src/b.rs", &["src/a/**".into()]).unwrap());
    assert!(path_is_allowed("src/", &["src/**".into()]).is_err());
    assert!(path_is_within("../x", "src/**").is_err());
}
```

- [ ] **Step 2：运行，确认失败**

```bash
cargo test -p towermd-core --test path_guard path_rules_live_in_path_guard
```

预期：编译失败，`no function path_is_allowed in path_guard`（或 `unresolved import`）。

- [ ] **Step 3：机械移动（行号已在 03f15f4 上模拟核对，`fn unique_ids` 保留在第 242 行）**

```bash
sed -n '240,300p' crates/towermd-core/src/task_model.rs | sed -e 's/^struct PathRule/pub struct PathRule/' -e 's/^    fn parse/    pub fn parse/' -e 's/^    fn covers/    pub fn covers/' > target/pathrule.rs
perl -i -ne 'print unless 240..301' crates/towermd-core/src/task_model.rs
perl -0pi -e 's/use crate::hub::\{Fence, Lease\};\n/use crate::hub::{Fence, Lease};\nuse crate::path_guard::PathRule;\npub use crate::path_guard::{path_is_allowed, path_is_within};\n/' crates/towermd-core/src/task_model.rs
{ echo; cat target/pathrule.rs; } >> crates/towermd-core/src/path_guard.rs
perl -pi -e 's/repo, task_model, types,/repo, types,/; s/\btask_model::(path_is_allowed|path_is_within)\b/$1/g' crates/towermd-core/src/path_guard.rs
grep -n "task_model" crates/towermd-core/src/path_guard.rs
```

预期：最后的 grep 无输出。`target/pathrule.rs` 以 `#[derive(Clone, Debug, PartialEq, Eq)]` 开头，以 `path_is_allowed` 的结尾 `}` 结束。`proposal_model`、`task_finish` 仍可通过 `crate::task_model::path_is_allowed` 这个再导出使用，不必改。

- [ ] **Step 4：运行，确认通过**

```bash
cargo test -p towermd-core --test path_guard
cargo check --workspace
```

预期：path_guard 全部 PASS。check 可能提示 `task_model.rs` 里 `Component`、`Path` 未使用，按提示只删除这两个名字，不动其他行。

- [ ] **Step 5：全量回归并提交**

```bash
cargo test --workspace 2>&1 | grep -E '^test result' | awk '{s+=$4; f+=$6} END {print s" passed, "f" failed"}'
git add crates/towermd-core/src/task_model.rs crates/towermd-core/src/path_guard.rs crates/towermd-core/tests/path_guard.rs
git commit -m "refactor(core): move PathRule into path_guard" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

预期：388 passed，0 failed。

---

### Task 4：`SelfReported` 归入 `question_model`

**Files:**
- Modify: `crates/towermd-core/src/question_model.rs`（`RunContext` 之前新增结构体；原第 368 行字段类型；原第 454–455 行错误文案）
- Modify: `crates/towermd-core/src/task_model.rs`（删除 `HarnessSource` 结构体，原第 143–148 行）
- Test: `crates/towermd-core/tests/question_model.rs`（改第 101 行，追加 1 个测试）

- [ ] **Step 1：写失败测试**

把 `crates/towermd-core/tests/question_model.rs` 第 101 行的 `towermd_core::task_model::HarnessSource{` 改成 `SelfReported{`（`question_model::*` 已经 glob 导入），然后在文件末尾追加：

```rust
#[test]
fn relayed_answer_self_report_needs_a_named_source() {
    let mut state=QuestionProjection::default();let mut events=Events::new();
    let opened=events.event("hold.opened","process:towermd-cli","H-1",
        QuestionEvent::Opened{question:question("H-1","R-1","A-a","格式？")});
    state.apply(&opened).unwrap();
    let relayed=events.event("hold.answered","process:towermd-cli","H-1",QuestionEvent::AnswerRelayed{
        hold:"H-1".into(),text:"JSON".into(),relayed_from:"human".into(),
        self_reported:Some(SelfReported{name:"".into(),version:"1".into()}),
    });
    let error=state.apply(&relayed).unwrap_err();
    assert!(error.contains("自述来源名称"),"{error}");
}
```

- [ ] **Step 2：运行，确认失败**

```bash
cargo test -p towermd-core --test question_model
```

预期：编译失败，`cannot find struct SelfReported`。

- [ ] **Step 3：实现（已在 03f15f4 上模拟核对）**

```bash
perl -0pi -e 's/(#\[derive\(Clone, Debug, PartialEq, Eq, Serialize, Deserialize\)\]\n#\[serde\(deny_unknown_fields\)\]\npub struct RunContext \{)/#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]\n#[serde(deny_unknown_fields)]\npub struct SelfReported {\n    pub name: String,\n    pub version: String,\n}\n\n$1/; s/self_reported:Option<crate::task_model::HarnessSource> \}/self_reported:Option<SelfReported> }/; s/"harness 名称"/"自述来源名称"/; s/"harness 版本"/"自述来源版本"/' crates/towermd-core/src/question_model.rs
perl -0pi -e 's/#\[derive\(Clone, Debug, PartialEq, Eq, Serialize, Deserialize\)\]\n#\[serde\(deny_unknown_fields\)\]\npub struct HarnessSource \{\n    pub name: String,\n    pub version: String,\n\}\n/pub use crate::question_model::SelfReported as HarnessSource;\n/' crates/towermd-core/src/task_model.rs
grep -n "SelfReported\|HarnessSource\|task_model" crates/towermd-core/src/question_model.rs crates/towermd-core/src/task_model.rs
```

预期输出（行号可能差 ±1）：`question_model.rs:32:pub struct SelfReported {`、`question_model.rs:375: self_reported:Option<SelfReported> },`、`task_model.rs:145:pub use crate::question_model::SelfReported as HarnessSource;`，以及 `task_model.rs` 里原有的 `HarnessSource` 使用处。`question_model.rs` 中不再出现 `task_model`。二进制里的 `HarnessSource`（`main.rs:333/369`、`daemon.rs:733`、`task_service`、`question_service:24`）通过别名继续编译，线上 JSON 不变。

- [ ] **Step 4：运行，确认通过**

```bash
cargo test -p towermd-core --test question_model
cargo check --workspace
```

预期：question_model 全部 PASS，check 无错误。

- [ ] **Step 5：全量回归并提交**

```bash
cargo test --workspace 2>&1 | grep -E '^test result' | awk '{s+=$4; f+=$6} END {print s" passed, "f" failed"}'
git add crates/towermd-core/src/question_model.rs crates/towermd-core/src/task_model.rs crates/towermd-core/tests/question_model.rs
git commit -m "refactor(core): name self-reported answer source in question model" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

预期：389 passed，0 failed。

---

### Task 5：把 reducer 拆成核心与 DevKit

本任务是重放风险最高的一步。核心 `project_state` 只保留与开发无关的分支，其余事件交给 Kit。`project_controls` 的收件运行改为从 Kit 读取。新文件都由脚本从 `$BASE` 的原文按行号拼出，保证业务代码逐字不变。

**Files:**
- Rewrite: `crates/towermd-core/src/project_state.rs`、`crates/towermd-core/src/dev_state.rs`
- Modify: `crates/towermd-core/src/kit.rs`、`crates/towermd-core/src/project_controls.rs`、`crates/towermd-core/src/question_projection.rs:70-89`（删除）、`crates/towermd-core/src/snapshot_publication.rs`（临时改用 `dev_state`，T6 再改）、`crates/towermd-core/src/proposal_model_tests.rs`、`crates/towermd-core/src/task_alerts_tests.rs`
- Modify（改名 `project_state` → `dev_state`）：`crates/towermd/src/{business,task,question,proposal,finish,human_task,run_details}_service.rs`、`crates/towermd/tests/review_cli.rs`、`crates/towermd/tests/support/golden.rs`、`crates/towermd-core/tests/{main_inspection,main_resolution,project_controls,replay_golden}.rs`
- Modify（改用纯核心）：`crates/towermd-core/tests/project_runtime.rs:101-134`、`crates/towermd-core/tests/question_projection.rs:46,56`
- Modify（加 Kit 实参）：`crates/towermd/src/project_control_service.rs:60,95`
- Test: `crates/towermd-core/tests/kit.rs`（追加 3 个测试）

- [ ] **Step 1：写失败测试**

在 `crates/towermd-core/tests/kit.rs` 顶部把 `use` 改成：

```rust
use serde_json::{json, Value};
use towermd_core::{
    hub::Event,
    kit::{Kit, KitRun},
    project_controls, project_state,
    question_model::RunContext,
};
```

然后在文件末尾追加（`RunContext{...}` 必须写在同一行，T10 的正则依赖这一点）：

```rust
struct Counter;
impl Kit for Counter {
    fn state_groups(&self) -> &'static [&'static str] {
        &["Counter"]
    }
    fn reduce(&self, state: &mut Value, event: &Event) -> Result<bool, String> {
        if event.kind != "counter.bumped" {
            return Ok(false);
        }
        state["Counter"] = json!(state["Counter"].as_u64().unwrap_or(0) + 1);
        Ok(true)
    }
    fn runs(&self, _: &Value) -> Result<Vec<KitRun>, String> {
        Ok(vec![KitRun { active: true, context: RunContext{run:"R-one".into(),task:"T-one".into(),module:"A-one".into()} }])
    }
}

fn bumped() -> Event {
    serde_json::from_value(json!({"seq":1,"eid":"e-1","host":"test","t":"2026-10-21T12:00:00Z",
        "actor":"process:test","type":"counter.bumped","layer":"L4","obj":"C-1","data":{},
        "epoch":null,"stale":false,"cost_usd":null,"guards":[]}))
    .unwrap()
}

#[test]
fn unknown_business_events_reach_the_kits() {
    let mut state = project_state::initial_with(&[&Counter]);
    project_state::apply_with(&[&Counter], &mut state, &bumped()).unwrap();
    assert_eq!(state["Counter"], json!(1));
}

#[test]
fn core_alone_rejects_kit_events() {
    let mut state = project_state::core_initial();
    assert_eq!(
        project_state::core_apply(&mut state, &bumped()).unwrap_err(),
        "尚未支持的业务事件：counter.bumped"
    );
}

#[test]
fn pause_recipients_come_from_kit_runs() {
    let state = project_state::initial_with(&[&Counter]);
    let recipients = |kits: &[&dyn Kit]| match project_controls::prepare_pause(&state, "E-pause", kits).unwrap() {
        project_controls::ControlEvent::Paused { prompt, .. } => {
            prompt.recipients.into_iter().map(|recipient| recipient.run).collect::<Vec<_>>()
        }
        other => panic!("{other:?}"),
    };
    assert_eq!(recipients(&[&Counter]), ["R-one"]);
    assert!(recipients(&[]).is_empty());
}
```

- [ ] **Step 2：运行，确认失败**

```bash
cargo test -p towermd-core --test kit
```

预期：编译失败，`no KitRun in kit`、`no function apply_with`。

- [ ] **Step 3：扩展 `kit.rs`**

在 `crates/towermd-core/src/kit.rs` 中，把 trait 及其上方内容替换为：

```rust
//! Compile-time extension seam. A kit adds projection groups and business
//! reducers on top of the core; the core never names a kit's concepts.
use serde_json::Value;

use crate::{hub::Event, question_model::RunContext};

/// A run a kit tracks, as seen by core prompt delivery.
pub struct KitRun {
    pub context: RunContext,
    pub active: bool,
}

pub trait Kit: Sync {
    /// Top-level projection groups this kit owns, inserted before core groups.
    fn state_groups(&self) -> &'static [&'static str] {
        &[]
    }
    /// Applies an event the core does not own. `Ok(false)` means not handled.
    fn reduce(&self, _state: &mut Value, _event: &Event) -> Result<bool, String> {
        Ok(false)
    }
    /// Runs that can receive core control prompts.
    fn runs(&self, _state: &Value) -> Result<Vec<KitRun>, String> {
        Ok(Vec::new())
    }
}
```

- [ ] **Step 4：重写核心 `project_state.rs`**

把下面的脚本保存为 `target/t5-project-state.sh`，用 `bash target/t5-project-state.sh` 执行：

```bash
#!/bin/bash
set -euo pipefail
O=target/project_state.orig.rs
git show 03f15f4:crates/towermd-core/src/project_state.rs > "$O"
OUT=crates/towermd-core/src/project_state.rs
{
cat <<'EOF'
//! Core projection reducer. The core owns admission, controls, alerts,
//! snapshots, recovery and lease bookkeeping; every other business event is
//! offered to the assembled kits in order. Self-reported Agent text cannot
//! transition an object to completion.
use serde_json::{json, Map, Value};

use crate::{hub::Event, kit::Kit};

/// Groups every assembly has, after the kit groups.
pub const CORE_GROUPS: [&str; 4] = ["Hold", "Proposal", "Prompt", "Alert"];

pub fn initial_with(kits: &[&dyn Kit]) -> Value {
    let mut groups = Map::new();
    for name in kits.iter().flat_map(|kit| kit.state_groups().iter()).chain(CORE_GROUPS.iter()) {
        groups.insert((*name).to_owned(), json!({}));
    }
    Value::Object(groups)
}

pub fn core_initial() -> Value {
    initial_with(&[])
}

pub fn core_apply(state: &mut Value, event: &Event) -> Result<(), String> {
    apply_with(&[], state, event)
}

pub fn apply_with(kits: &[&dyn Kit], state: &mut Value, event: &Event) -> Result<(), String> {
    if event.stale {
        return Ok(());
    }
    if crate::resolution_projection::apply(state, event)? { return Ok(()); }
    if crate::project_controls::apply(state, event, kits)? { return Ok(()); }
    match event.kind.as_str() {
EOF
sed -n '20,28p' "$O"
sed -n '116,158p' "$O"
sed -n '163,189p' "$O"
cat <<'EOF'
            for kit in kits {
                kit.reduce(state, event)?;
            }
        }
        "lease.expired" | "lease.released" => {
            if event.actor != "process:towermd-hub" {
                return Err("租约终止只能来自hub系统事件".into());
            }
            if !matches!(
                &event.lease_delta,
                Some(crate::hub::SystemDelta::Expire { .. } | crate::hub::SystemDelta::Release { .. })
            ) {
                return Err("租约终止事件缺少完整系统原件".into());
            }
            for kit in kits {
                kit.reduce(state, event)?;
            }
        }
EOF
sed -n '290,292p' "$O"
cat <<'EOF'
        _ => {
            for kit in kits {
                if kit.reduce(state, event)? {
                    return Ok(());
                }
            }
            return Err(format!("尚未支持的业务事件：{}", event.kind));
        }
    }
    Ok(())
}

EOF
sed -n '411,428p' "$O" | sed -e 's/^fn /pub fn /'
} > "$OUT"
echo "wrote $OUT"
```

拼接后的结构：

| 位置 | 内容 |
| --- | --- |
| 原 20–28 行 | `hub.rebuild.confirmed` |
| 原 116–158 行 | `alert.raised`、`alert.cleared`、`main.inspected` |
| 原 163–189 行 | hold/prompt、`snapshot.written`、`recovery.completed` 的校验部分 |
| 脚本补写 | `recovery.completed` 的收尾：把释放交给 Kit |
| 脚本补写 | 租约终止：核心只校验信封，释放交给 Kit |
| 原 290–292 行 | 其余 `lease.*` 只做记账 |
| 脚本补写 | 兜底分支：按顺序交给 Kit |
| 原 411–428 行 | `insert_unique`、`valid_oid`，改为 `pub` |

- [ ] **Step 5：重写 `dev_state.rs`，并删除 `question_projection::resolve_publication`**

把下面的脚本保存为 `target/t5-dev-state.sh`，用 `bash target/t5-dev-state.sh` 执行：

```bash
#!/bin/bash
set -euo pipefail
O=target/project_state.orig.rs
Q=target/question_projection.orig.rs
git show 03f15f4:crates/towermd-core/src/project_state.rs > "$O"
git show 03f15f4:crates/towermd-core/src/question_projection.rs > "$Q"
OUT=crates/towermd-core/src/dev_state.rs
{
cat <<'EOF'
//! Development kit projection: baselines, tasks, runs and finish evidence.
//! Event names and payloads are the v0.1.1 ones; only their owner moved.
use serde_json::{json, Value};

use crate::{
    baseline_publication,
    hub::Event,
    kit::{Kit, KitRun},
    project_state::{self, insert_unique, valid_oid},
    question_model::RunContext,
    task_model::{ReleaseReason, Run, RunState, TaskEvent},
};

pub struct DevKit;
pub const GROUPS: &[&str] = &["Baseline", "BaselineApproval", "Task", "Run", "Evidence"];
pub const KITS: &[&dyn Kit] = &[&DevKit];

impl Kit for DevKit {
    fn state_groups(&self) -> &'static [&'static str] {
        GROUPS
    }
    fn reduce(&self, state: &mut Value, event: &Event) -> Result<bool, String> {
        reduce(state, event)
    }
    fn runs(&self, state: &Value) -> Result<Vec<KitRun>, String> {
        Ok(tasks(state)?
            .runs
            .into_values()
            .map(|run| KitRun {
                active: run.state == RunState::Active,
                context: RunContext{run: run.binding.id, task: run.task, module: run.module},
            })
            .collect())
    }
}

pub fn initial() -> Value {
    project_state::initial_with(KITS)
}

pub fn apply(state: &mut Value, event: &Event) -> Result<(), String> {
    project_state::apply_with(KITS, state, event)
}

fn reduce(state: &mut Value, event: &Event) -> Result<bool, String> {
    if crate::proposal_model::apply(state, event)? {
        return Ok(true);
    }
    match event.kind.as_str() {
EOF
sed -n '29,115p' "$O" | sed -e 's/crate::question_projection::resolve_publication(/resolve_publication(/'
cat <<'EOF'
        "recovery.completed" => {
            // The real recovery.completed receipt revokes all old authority;
            // this is not a fabricated historical lease.expired event.
            release_runs(state, |_| true, ReleaseReason::Reassigned)?;
        }
        "lease.expired" | "lease.released" => {
            let fences = event.released_fences().to_vec();
            let reason = if event.kind == "lease.expired" {
                ReleaseReason::Expired
            } else {
                ReleaseReason::Voluntary
            };
            release_runs(state, |run| run.fences.iter().any(|fence| fences.contains(fence)), reason)?;
        }
EOF
sed -n '207,256p' "$O"
sed -n '159,162p' "$O"
cat <<'EOF'
        _ => return Ok(false),
    }
    Ok(true)
}

fn release_runs(
    state: &mut Value,
    select: impl Fn(&Run) -> bool,
    reason: ReleaseReason,
) -> Result<(), String> {
    let mut projection = tasks(state)?;
    let released: Vec<_> = projection
        .runs
        .values()
        .filter(|run| run.state == RunState::Active && select(run))
        .map(|run| run.binding.id.clone())
        .collect();
    for run_id in released {
        projection.apply(&TaskEvent::Released { run_id, reason })?;
    }
    write_tasks(state, &projection)
}

EOF
sed -n '298,409p' "$O"
echo
sed -n '430,445p' "$O"
echo
echo '/// Called only after the kit has checked the actual baseline.created'
echo '/// receipt against its accepted approval. Answer text alone cannot call this'
echo '/// transition; the new baseline and original answer remain separately linked.'
sed -n '73,89p' "$Q" | sed -e 's/^pub(crate) fn resolve_publication/fn resolve_publication/' -e 's/let mut projection=read(state)?;/let mut projection=crate::question_projection::read(state)?;/'
} > "$OUT"
perl -i -ne 'print unless 70..89' crates/towermd-core/src/question_projection.rs
echo "wrote $OUT"
```

拼接后的结构：

| 位置 | 内容 |
| --- | --- |
| 原 29–115 行 | `baseline.approved`、`baseline.created`；`resolve_publication` 改为本地函数 |
| 脚本补写 | `recovery.completed`、租约终止的释放（行为与原 190–205、257–289 行相同；`released_fences()` 与原 `SystemDelta::{Expire,Release}` 取到的是同一组 fence） |
| 原 207–256 行 | `task.created`、`task.claimed`、`task.released` |
| 原 159–162 行 | `task.finish.*`、`task.g5.completed`、`task.merged` → `apply_finish` |
| 原 298–409 行 | `apply_finish` |
| 原 430–445 行 | `tasks`、`write_tasks` |
| `question_projection.rs` 原 73–89 行 | `resolve_publication` |

核对：

```bash
grep -n "resolve_publication\|proposal_model" crates/towermd-core/src/question_projection.rs
grep -nE "task_model|task_finish|task_alerts|baseline_publication|proposal_model" crates/towermd-core/src/project_state.rs
```

预期：两条 grep 都无输出。

- [ ] **Step 6：`project_controls` 改从 Kit 取运行**

在 `crates/towermd-core/src/project_controls.rs` 中：

第 5–6 行

```rust
use crate::{hub::Event, project_state, question_model::{self, Prompt, PromptScope, RunContext},
    question_projection, task_model::RunState};
```

替换为

```rust
use crate::{hub::Event, kit::{Kit, KitRun}, question_model::{self, Prompt, PromptScope, RunContext},
    question_projection};
```

第 50–53 行 `active` 替换为

```rust
fn kit_runs(state:&Value,kits:&[&dyn Kit])->Result<Vec<KitRun>,String> {
    let mut runs=Vec::new();
    for kit in kits {runs.extend(kit.runs(state)?);}
    Ok(runs)
}

pub fn active(state:&Value,kits:&[&dyn Kit])->Result<Vec<RunContext>,String> {
    Ok(kit_runs(state,kits)?.into_iter().filter(|run|run.active).map(|run|run.context).collect())
}
```

第 65–68 行与第 73–76 行替换为

```rust
pub fn prepare_pause(state:&Value,eid:&str,kits:&[&dyn Kit])->Result<ControlEvent,String> {
    if read(state)?.paused {return Err("项目已暂停；请确认现有暂停后再恢复".into());}
    Ok(ControlEvent::Paused{scope:Scope::All,prompt:prompt(eid,pause_text(eid),&active(state,kits)?)?})
}
```

```rust
pub fn prepare_resume(state:&Value,eid:&str,pause_event:String,kits:&[&dyn Kit])->Result<ControlEvent,String> {
    require_pause(&read(state)?,&pause_event)?;
    Ok(ControlEvent::Resumed{scope:Scope::All,prompt:prompt(eid,resume_text(&pause_event),&active(state,kits)?)?,pause_event})
}
```

第 96 行签名改为 `pub fn apply(state:&mut Value,event:&Event,kits:&[&dyn Kit])->Result<bool,String> {`。第 109 行改为 `validate_prompt(state,kits,event,&prompt,pause_text(&event.eid))?;`，第 124 行改为 `validate_prompt(state,kits,event,&prompt,resume_text(&pause_event))?;`。

第 148–158 行 `validate_prompt` 替换为

```rust
fn validate_prompt(state:&Value,kits:&[&dyn Kit],event:&Event,actual:&Prompt,text:String)->Result<(),String> {
    if *actual!=prompt(&event.eid,text,&actual.recipients)? {return Err("项目控制指令或冻结收件人无效".into());}
    let runs=kit_runs(state,kits)?;
    for recipient in &actual.recipients {
        let run=runs.iter().find(|run|run.context.run==recipient.run).ok_or("控制指令收件运行不存在")?;
        if run.context!=*recipient {
            return Err("控制指令收件运行绑定不符".into());
        }
    }
    Ok(())
}
```

`prepare_resume_request` 不变。语义差异的说明见风险 R7：原代码按 `Run` 映射的键查找，现在按 `binding.id` 查找；投影里两者一致，由 `task_model` 校验保证。

- [ ] **Step 7：更新调用方**

先按“命令约定”创建 `target/split-use.pl` 和 `target/add-arg.pl`，再执行：

```bash
perl -pi -e 's/\bproject_state\b/dev_state/g' \
  crates/towermd/src/business_service.rs crates/towermd/src/task_service.rs crates/towermd/src/question_service.rs \
  crates/towermd/src/proposal_service.rs crates/towermd/src/finish_service.rs crates/towermd/src/human_task_service.rs \
  crates/towermd/src/run_details_service.rs crates/towermd/tests/review_cli.rs crates/towermd/tests/support/golden.rs \
  crates/towermd-core/tests/main_inspection.rs crates/towermd-core/tests/main_resolution.rs \
  crates/towermd-core/tests/project_controls.rs crates/towermd-core/tests/replay_golden.rs \
  crates/towermd-core/src/proposal_model_tests.rs crates/towermd-core/src/task_alerts_tests.rs \
  crates/towermd-core/src/snapshot_publication.rs
perl -pi -e 'if (101..134) { s/project_state::initial\(\)/project_state::core_initial()/g; s/project_state::apply\b/project_state::core_apply/g }' crates/towermd-core/tests/project_runtime.rs
perl -pi -e 's/towermd_core::project_state::apply\b/towermd_core::project_state::core_apply/g' crates/towermd-core/tests/question_projection.rs
perl target/add-arg.pl 'project_controls::(?:prepare_pause|prepare_resume|apply|active)' 'towermd_core::dev_state::KITS' crates/towermd/src/project_control_service.rs crates/towermd-core/tests/project_controls.rs
perl target/add-arg.pl 'project_controls::(?:prepare_pause|prepare_resume|apply|active)' '&[]' crates/towermd-core/tests/project_runtime.rs
grep -rnE "project_controls::(prepare_pause|prepare_resume|apply|active)\(" --include='*.rs' crates src-tauri/src | grep -v "KITS\|&\[\]\|kits" || true
```

预期：add-arg 输出 `project_control_service.rs: 2`、`project_runtime.rs: 2`，`tests/project_controls.rs` 为两位数；最后的 grep 无输出。`replay_golden.rs` 改用 `dev_state::{initial, apply}`，样本仍是 T1 录下的，这正是本任务的等价检查。

- [ ] **Step 8：运行，确认通过**

```bash
cargo check --workspace 2>&1 | grep -E "^(warning|error)" | sort | uniq -c
cargo test -p towermd-core --test kit --test replay_golden --test project_controls --test project_runtime --test question_projection
```

预期：check 无 error。如果 warning 只是 `dev_state.rs` 或 `project_state.rs` 里的未使用导入（如 `json`、`baseline_publication`、`valid_oid`），只删除报警告的那个名字。kit 5 passed，replay_golden 4 passed，其余全部 PASS。replay_golden 若失败，**停下**，对比 `projection.json` 与实际投影的差异并报告，不要改样本。

- [ ] **Step 9：全量回归并提交**

```bash
cargo test --workspace 2>&1 | grep -E '^test result' | awk '{s+=$4; f+=$6} END {print s" passed, "f" failed"}'
cargo test --manifest-path src-tauri/Cargo.toml 2>&1 | grep -E '^test result'
git add crates/towermd-core/src crates/towermd-core/tests crates/towermd/src crates/towermd/tests
git status --short
git commit -m "refactor(core): split project_state reducer into core and dev kit" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

预期：392 passed，0 failed；src-tauri 19 passed、1 ignored。提交前用 `git status --short` 确认暂存区里没有 `README.md` 和 `docs/usage.md`。

---

### Task 6：受保护提交证明改走 `Assembly`

`main_inspection` 不再直接证明基线出版，改为依次询问 Kit；`snapshot_publication::prove` 用传入的装配重放。

**Files:**
- Modify: `crates/towermd-core/src/kit.rs`、`crates/towermd-core/src/dev_state.rs`、`crates/towermd-core/src/main_inspection.rs`（第 5 行导入、第 29–37 行签名、第 74–77 行、删除第 131–152 行）、`crates/towermd-core/src/snapshot_publication.rs:5,15-25`
- Modify（加 `&towermd_core::dev_state::ASSEMBLY`）：`crates/towermd/src/finish_service.rs:113`、`crates/towermd/src/main_resolution_service.rs:39,95`、`crates/towermd-core/tests/main_resolution.rs:71,115,120`、`crates/towermd-core/tests/main_inspection.rs:48,81,94,159,168,172,192`
- Test: `crates/towermd-core/tests/kit.rs`（追加 2 个测试）

- [ ] **Step 1：写失败测试**

`crates/towermd-core/tests/kit.rs` 顶部加 `use std::path::Path;`，并把 `kit::{Kit, KitRun}` 改成 `kit::{self, Kit, KitRun}`。然后在文件末尾追加：

```rust
struct Prover;
impl Kit for Prover {
    fn prove_protected_commit(
        &self,
        _root: &Path,
        commit: &str,
        _project_id: &str,
        _receipt: &dyn Fn(&str) -> Option<Event>,
    ) -> Result<(), String> {
        if commit == "ok" { Ok(()) } else { Err("not mine".into()) }
    }
}

const COUNTER_KITS: &[&dyn Kit] = &[&Counter];

#[test]
fn protected_commits_need_a_kit_proof() {
    let none = |_: &str| -> Option<Event> { None };
    let root = Path::new(".");
    assert_eq!(kit::prove_by_kits(&[], root, "ok", "p", &none).unwrap_err(), kit::NO_PROOF);
    assert!(kit::prove_by_kits(&[&Counter, &Prover], root, "ok", "p", &none).is_ok());
    assert_eq!(kit::prove_by_kits(&[&Prover], root, "bad", "p", &none).unwrap_err(), "not mine");
}

#[test]
fn assembly_initial_uses_its_kits() {
    let assembly = kit::Assembly { kits: COUNTER_KITS, reduce: project_state::core_apply };
    assert_eq!(assembly.initial()["Counter"], json!({}));
}
```

- [ ] **Step 2：运行，确认失败**

```bash
cargo test -p towermd-core --test kit
```

预期：编译失败，`no function prove_by_kits`、`no struct Assembly`。

- [ ] **Step 3：扩展 `kit.rs`**

把 `kit.rs` 的 `use` 两行替换为

```rust
use std::path::Path;

use serde_json::Value;

use crate::{hub::Event, project_runtime::Reducer, question_model::RunContext};

/// Error when no kit can prove a protected commit (v0.1.1 wording).
pub const NO_PROOF: &str = "受保护提交缺少可证明的出版事件";
```

在 trait 的 `runs` 方法之后加：

```rust
    /// Proves that a protected main commit was published by this kit.
    fn prove_protected_commit(
        &self,
        _root: &Path,
        _commit: &str,
        _project_id: &str,
        _receipt: &dyn Fn(&str) -> Option<Event>,
    ) -> Result<(), String> {
        Err(NO_PROOF.into())
    }
```

在文件末尾加：

```rust
/// First successful kit proof wins; otherwise the last kit's error.
pub fn prove_by_kits(
    kits: &[&dyn Kit],
    root: &Path,
    commit: &str,
    project_id: &str,
    receipt: &dyn Fn(&str) -> Option<Event>,
) -> Result<(), String> {
    let mut last = Err(NO_PROOF.to_owned());
    for kit in kits {
        last = kit.prove_protected_commit(root, commit, project_id, receipt);
        if last.is_ok() {
            return last;
        }
    }
    last
}

/// A runtime assembly: the kits and the reducer built from exactly those kits.
pub struct Assembly {
    pub kits: &'static [&'static dyn Kit],
    pub reduce: Reducer,
}

impl Assembly {
    pub fn initial(&self) -> Value {
        crate::project_state::initial_with(self.kits)
    }
}
```

- [ ] **Step 4：DevKit 接管基线证明**

在 `crates/towermd-core/src/dev_state.rs` 中：

- `use` 里加 `std::path::Path` 和 `kit::Assembly`；`kit::{Kit, KitRun}` 改为 `kit::{Assembly, Kit, KitRun}`，并在文件顶部加 `use std::path::Path;`。
- `pub const KITS` 之后加：

```rust
pub const ASSEMBLY: Assembly = Assembly { kits: KITS, reduce: apply };
```

在 `impl Kit for DevKit` 的 `runs` 之后加：

```rust
    fn prove_protected_commit(
        &self,
        root: &Path,
        commit: &str,
        project_id: &str,
        receipt: &dyn Fn(&str) -> Option<Event>,
    ) -> Result<(), String> {
        prove_baseline(root, commit, project_id, receipt)
    }
```

在文件末尾加（函数体与 `main_inspection.rs` 原第 135–151 行相同，只改了路径前缀，`NO_PROOF` 与原文案逐字相同）：

```rust
fn prove_baseline(
    root: &Path,
    commit: &str,
    project_id: &str,
    receipt: &dyn Fn(&str) -> Option<Event>,
) -> Result<(), String> {
    let event = receipt(&crate::events::derived_eid(commit, "baseline.created")?)
        .ok_or(crate::kit::NO_PROOF)?;
    let approval_eid = event.data["approval_eid"].as_str().ok_or("基线出版事件缺少批准 ID")?;
    let approval = receipt(approval_eid).ok_or("基线出版缺少 canonical 批准原件")?;
    let publication = baseline_publication::from_approval(&approval)?;
    if event.stale || event.kind != "baseline.created" || event.layer != "L1"
        || event.eid != crate::events::derived_eid(commit, "baseline.created")?
        || !matches!(event.actor.as_str(), "process:towermd" | "process:towermd-recovery")
        || event.obj != publication.target || publication.project_id != project_id
        || event.data != json!({"commit":commit, "approval_eid":approval_eid,
            "content_hash":publication.content_hash})
        || event.guards.len() != 1 || event.guards[0].name != "main"
        || event.epoch != Some(event.guards[0].epoch)
    {
        return Err("基线出版事件与项目、批准或主干租约不一致".into());
    }
    publication.prove_commit(root, commit, &approval)
}
```

- [ ] **Step 5：`main_inspection` 与 `snapshot_publication` 改用装配**

`crates/towermd-core/src/main_inspection.rs`：

- 第 5–6 行导入改为 `use crate::{events, hub::Event, hub_protocol::NewEvent, kit::{self, Assembly}, main_resolution,` 与 `    path_guard::{self, Violation}, snapshot_publication};`。
- `inspect` 签名在 `receipt: impl Fn(&str) -> Option<Event>,` 之后加一行 `    assembly: &Assembly,`。
- 第 74 行 `let proof = prove_baseline(root, &changed.commit, project_id, &receipt).or_else(|baseline_error| {` 改为 `let proof = kit::prove_by_kits(assembly.kits, root, &changed.commit, project_id, &receipt).or_else(|baseline_error| {`。
- 第 77 行 `snapshot_publication::prove(root, &changed.commit, docs_dir, project_id, &event, &receipt)` 改为 `snapshot_publication::prove(root, &changed.commit, docs_dir, project_id, &event, &receipt, assembly)`。
- 删除 `fn prove_baseline` 整个函数（原第 131–152 行，含其后空行）。

`crates/towermd-core/src/snapshot_publication.rs`：

- 第 5 行改为 `use crate::{events, hub::Event, kit::Assembly, project_runtime, repo, snapshot::Snapshot};`。
- `prove` 签名在 `receipt: &impl Fn(&str) -> Option<Event>,` 之后加 `    assembly: &Assembly,`。
- 第 24 行改为 `        project_runtime::replay(&assembly.initial(), events, assembly.reduce)`。

调用方：

```bash
perl target/add-arg.pl 'main_inspection::inspect|snapshot_publication::prove' '&towermd_core::dev_state::ASSEMBLY' \
  crates/towermd/src/finish_service.rs crates/towermd/src/main_resolution_service.rs \
  crates/towermd-core/tests/main_resolution.rs crates/towermd-core/tests/main_inspection.rs
grep -n "dev_state\|project_state" crates/towermd-core/src/snapshot_publication.rs crates/towermd-core/src/main_inspection.rs
```

预期：add-arg 依次输出 1、2、3、7；最后的 grep 无输出。

- [ ] **Step 6：运行，确认通过**

```bash
cargo check --workspace
cargo test -p towermd-core --test kit --test main_inspection --test main_resolution --test replay_golden
```

预期：check 无 error；kit 7 passed，其余全部 PASS。

- [ ] **Step 7：全量回归并提交**

```bash
cargo test --workspace 2>&1 | grep -E '^test result' | awk '{s+=$4; f+=$6} END {print s" passed, "f" failed"}'
git add crates/towermd-core/src crates/towermd-core/tests crates/towermd/src
git commit -m "refactor(core): prove protected commits through kit assembly" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

预期：394 passed，0 failed。核对 `grep -nE "baseline_publication|task_model|proposal_model|task_finish" crates/towermd-core/src/{project_state,project_controls,question_projection,question_model,main_inspection,snapshot_publication,path_guard}.rs`，只应在 `question_model` 之外无输出；`path_guard` 也应无输出。

---

### Task 7：新建 `towermd-devkit` 并整体搬家（设计 A1）

本任务只动文件位置和 `use` 路径，不改行为。测试总数应与 T6 结束时相等（394），src-tauri 不变。

**Files:**
- Create: `crates/towermd-devkit/Cargo.toml`、`crates/towermd-devkit/src/lib.rs`
- Move: 见“文件地图·移动（T7）”
- Create: `crates/towermd-devkit/tests/baseline_remote_publication.rs`（由核心 `tests/main_writer.rs` 拆出）
- Modify: 根 `Cargo.toml`、`crates/towermd/Cargo.toml`、`src-tauri/Cargo.toml`、`crates/towermd-core/src/lib.rs`、`crates/towermd-core/src/repo.rs`、`crates/towermd-core/tests/main_writer.rs`、`crates/towermd-devkit/src/task_finish_tests.rs`、`crates/towermd-devkit/src/proposal_model_tests.rs`、`crates/towermd/tests/support/golden.rs`；`use` 路径改写涉及二进制、src-tauri 和被移动的文件
- Lock: `Cargo.lock`、`src-tauri/Cargo.lock`

- [ ] **Step 1：失败检查**

```bash
cargo tree -p towermd-devkit 2>&1 | head -1
```

预期：`error: package ID specification \`towermd-devkit\` did not match any packages`。

- [ ] **Step 2：清单**

根 `Cargo.toml`：`members` 改为 `members = ["crates/towermd-core", "crates/towermd-devkit", "crates/towermd"]`，并在 `towermd-core = { path = "crates/towermd-core" }` 后加 `towermd-devkit = { path = "crates/towermd-devkit" }`。

`crates/towermd-devkit/Cargo.toml`：

```toml
[package]
name = "towermd-devkit"
version.workspace = true
edition.workspace = true
publish.workspace = true

[dependencies]
towermd-core.workspace = true
serde.workspace = true
serde_json.workspace = true
serde_yaml.workspace = true
chrono.workspace = true
sha2.workspace = true
hex.workspace = true
uuid.workspace = true

[dev-dependencies]
tempfile.workspace = true
```

`crates/towermd/Cargo.toml`：在 `towermd-core.workspace = true` 后加 `towermd-devkit.workspace = true`。

`src-tauri/Cargo.toml`：在第 21 行 `towermd-core = { path = "../crates/towermd-core" }` 后加 `towermd-devkit = { path = "../crates/towermd-devkit" }`。

- [ ] **Step 3：`git mv`**

```bash
mkdir -p crates/towermd-devkit/src crates/towermd-devkit/tests
for m in task_model task_finish task_finish_tests task_alerts task_alerts_tests verifier cold_start cold_start_verification baseline baseline_publication draft_store review review_verification proposal_model proposal_model_tests dev_state; do
  git mv crates/towermd-core/src/$m.rs crates/towermd-devkit/src/$m.rs
done
git mv crates/towermd-core/schema crates/towermd-devkit/schema
for t in baseline cold_start cold_start_verification review verifier main_inspection main_resolution project_controls replay_golden; do
  git mv crates/towermd-core/tests/$t.rs crates/towermd-devkit/tests/$t.rs
done
git mv crates/towermd-core/tests/fixtures crates/towermd-devkit/tests/fixtures
```

`task_finish.rs:571`、`task_alerts.rs:32`、`proposal_model.rs:468` 的 `#[path = "..._tests.rs"]` 与测试文件同目录移动，无需修改。`baseline.rs:49` 的 `include_str!("../schema/baseline-submit.schema.json")` 随 `schema/` 移动后仍然成立。

- [ ] **Step 4：两个 `lib.rs`**

`crates/towermd-devkit/src/lib.rs`：

```rust
//! Software development kit for Tower.md: baselines, tasks, runs, finish
//! evidence, proposals, cold start and verification, attached to the core
//! through `towermd_core::kit::Kit`.
pub mod cold_start;
mod cold_start_verification;
pub mod verifier;

pub mod baseline;
pub mod draft_store;
pub mod review;
pub mod task_model;

pub mod baseline_publication;
pub mod review_verification;
pub mod task_finish;
pub mod proposal_model;
pub(crate) mod task_alerts;

pub mod dev_state;
```

核心 `lib.rs`：

```bash
perl -i -ne 'print unless /^(pub(\(crate\))? )?mod (cold_start|cold_start_verification|verifier|baseline|draft_store|review|task_model|baseline_publication|review_verification|task_finish|proposal_model|task_alerts|dev_state);$/' crates/towermd-core/src/lib.rs
perl -pi -e 's/^pub\(crate\) fn (git_timeout_env_output|git_timeout_env_input_output)\(/pub fn $1(/' crates/towermd-core/src/repo.rs
grep -c "^pub fn git_timeout_env" crates/towermd-core/src/repo.rs
```

预期：最后输出 `2`。核心 `lib.rs` 剩 25 个模块声明加 `pub mod kit;`。

- [ ] **Step 5：改写 `use` 路径**

```bash
CORE_MODS=canvas,catalog,config,events,hub,hub_protocol,init,okf,project_view,repo,types,durable_outbox,main_writer,project_state,snapshot,path_guard,project_runtime,main_inspection,snapshot_publication,question_model,question_projection,main_resolution,resolution_projection,project_controls,kit
DEV_MODS=task_model,task_finish,verifier,cold_start,cold_start_verification,baseline,baseline_publication,draft_store,review,review_verification,proposal_model,dev_state
perl target/split-use.pl crate towermd_core "$CORE_MODS" crates/towermd-devkit/src/*.rs
perl target/split-use.pl towermd_core towermd_devkit "$DEV_MODS" $(git ls-files 'crates/towermd/*.rs' 'src-tauri/src/*.rs' 'crates/towermd-devkit/tests/*.rs')
perl -pi -e 's{\.\./towermd-core/tests/fixtures}{../towermd-devkit/tests/fixtures}' crates/towermd/tests/support/golden.rs
grep -rnE "use crate::\{" crates/towermd-devkit/src | grep -E "\b(hub|config|events|repo|main_writer|question_model|question_projection|project_state|kit|path_guard|catalog|okf|types|snapshot)\b" || true
```

预期：最后的 grep 无输出。有输出则说明有 `pub use crate::{...}` 一类脚本没覆盖的形式，手工把核心模块改到 `towermd_core::`。`git ls-files` 的 `*` 会跨目录匹配，覆盖 `crates/towermd/tests/support/golden.rs` 和 src-tauri 子目录。

- [ ] **Step 6：被搬测试里的 `Event` 字面量改为反序列化**

`crates/towermd-devkit/src/task_finish_tests.rs` 中的 `envelope` 函数整体替换为：

```rust
fn envelope(seq: u64, kind: &str, actor: &str, eid: &str, value: &impl Serialize, leases: &[Lease]) -> Event {
    serde_json::from_value(serde_json::json!({
        "seq": seq, "eid": eid, "host": "test-host", "t": "2026-10-21T12:00:00Z",
        "actor": actor, "type": kind, "layer": "L4", "obj": "E-1",
        "data": serde_json::to_value(value).unwrap(),
        "epoch": leases.first().map(|lease| lease.epoch),
        "stale": false, "cost_usd": null,
        "guards": leases.iter().map(Lease::fence).collect::<Vec<_>>(),
    })).unwrap()
}
```

`crates/towermd-devkit/src/proposal_model_tests.rs` 中的 `event` 函数整体替换为：

```rust
fn event(eid:&str,kind:&str,actor:&str,layer:&str,obj:&str,data:serde_json::Value)->Event{
    serde_json::from_value(json!({"seq":1,"eid":eid,"host":"test","t":"2026-10-21T12:00:00Z","actor":actor,"type":kind,
        "layer":layer,"obj":obj,"data":data,"epoch":null,"stale":false,"cost_usd":null,"guards":[]})).unwrap()
}
```

`task_alerts_tests.rs` 已经用 `from_value`，不用改。这两个文件里的 `BTreeMap` 若变成未使用，删掉对应导入。

- [ ] **Step 7：拆 `tests/main_writer.rs`（D14）**

```bash
{
  sed -n '1,100p' crates/towermd-core/tests/main_writer.rs | perl -pe 's/^(\s*)self, Boundary, WriteKind, WriteOutcome, WriteRequest, SNAPSHOT_REF,$/$1self, WriteKind, WriteOutcome, WriteRequest,/'
  echo
  sed -n '310,316p' crates/towermd-core/tests/main_writer.rs
  echo
  sed -n '486,610p' crates/towermd-core/tests/main_writer.rs | perl -pe 's/towermd_core::baseline_publication/towermd_devkit::baseline_publication/g'
} > crates/towermd-devkit/tests/baseline_remote_publication.rs
perl -i -ne 'print unless 486..610' crates/towermd-core/tests/main_writer.rs
grep -n "baseline_publication" crates/towermd-core/tests/main_writer.rs || true
grep -c "^fn \|^#\[test\]" crates/towermd-devkit/tests/baseline_remote_publication.rs
```

预期：第一条 grep 无输出。行号以 T6 结束时的文件为准，T1–T6 都没有改这个文件。执行前先用 `sed -n '484,488p;608,612p'` 确认 486 行是第一个基线出版测试的 `#[test]`，610 行是最后一个这类测试的结尾 `}`；不符就停下重新定位，**需在执行前细化**。新文件里未使用的辅助函数只会产生 `dead_code` 警告，可以在文件顶部加 `#![allow(dead_code)]`。

- [ ] **Step 8：锁文件与编译**

```bash
cargo check --workspace --all-targets 2>&1 | grep -E "^error" -A6 | head -60
cargo metadata --manifest-path src-tauri/Cargo.toml --format-version 1 >/dev/null
cargo check --manifest-path src-tauri/Cargo.toml --all-targets 2>&1 | tail -3
```

预期：无 error。如果出现“private field”或“function is private”，就是第 18 条事实以外的遗漏：把对应项从 `pub(crate)` 改为 `pub`，并在提交说明中列出。

- [ ] **Step 9：测试数与依赖方向**

```bash
cargo test --workspace -- --list 2>/dev/null | grep -c ': test$'
cargo test --workspace 2>&1 | grep -E '^test result' | awk '{s+=$4; f+=$6} END {print s" passed, "f" failed"}'
cargo test --manifest-path src-tauri/Cargo.toml 2>&1 | grep -E '^test result'
cargo tree -p towermd-core | grep -c towermd-devkit
cargo tree -p towermd-devkit --depth 1 | grep towermd-core
```

预期：

| 检查 | 期望值 |
| --- | --- |
| 测试列表数 | 与 T6 结束时相等（394，加上 doctest 计数则与 T6 的同一命令结果相等） |
| workspace 测试 | 394 passed，0 failed |
| src-tauri | 19 passed、1 ignored |
| `cargo tree -p towermd-core` 中的 devkit | `0` |
| `cargo tree -p towermd-devkit` | 显示 `towermd-core` |

- [ ] **Step 10：提交**

```bash
git add Cargo.toml Cargo.lock crates/towermd-core crates/towermd-devkit crates/towermd src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/src
git status --short
git commit -m "refactor: move development kit modules into towermd-devkit" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 8：二进制服务分进 `cli/` 与 `dev/`（设计 A1 的另一半）

**Files:**
- Move: `crates/towermd/src/{business,task,question,proposal,main_resolution,run_details,project_control,human_task,event}_service.rs` → `crates/towermd/src/cli/`；`crates/towermd/src/{baseline,finish}_service.rs` → `crates/towermd/src/dev/`
- Modify: `crates/towermd/src/project_service.rs:3-24`

- [ ] **Step 1：失败检查**

```bash
ls crates/towermd/src/cli crates/towermd/src/dev
```

预期：`No such file or directory`。

- [ ] **Step 2：移动并改 `#[path]`**

```bash
mkdir -p crates/towermd/src/cli crates/towermd/src/dev
for f in business task question proposal main_resolution run_details project_control human_task event; do
  git mv crates/towermd/src/${f}_service.rs crates/towermd/src/cli/${f}_service.rs
done
git mv crates/towermd/src/baseline_service.rs crates/towermd/src/dev/baseline_service.rs
git mv crates/towermd/src/finish_service.rs crates/towermd/src/dev/finish_service.rs
perl -pi -e 's/^#\[path = "(baseline|finish)_service\.rs"\]/#[path = "dev\/$1_service.rs"]/ or s/^#\[path = "(\w+_service)\.rs"\]/#[path = "cli\/$1.rs"]/' crates/towermd/src/project_service.rs
sed -n '1,25p' crates/towermd/src/project_service.rs
```

预期：11 个 `#[path]` 都带上 `cli/` 或 `dev/` 前缀，其中 `baselines` 和 `finish` 指向 `dev/`。这些服务文件内没有嵌套 `mod`，也没有 `include_str!`。

- [ ] **Step 3：运行，确认通过**

```bash
cargo test -p towermd 2>&1 | grep -E '^test result' | awk '{s+=$4; f+=$6} END {print s" passed, "f" failed"}'
cargo test --workspace 2>&1 | grep -E '^test result' | awk '{s+=$4; f+=$6} END {print s" passed, "f" failed"}'
```

预期：workspace 394 passed，0 failed。

- [ ] **Step 4：提交**

```bash
git add crates/towermd/src
git commit -m "refactor(cli): group binary services into cli/ and dev/" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 9：模块租约泛化为工作租约（D8）

**Files:**
- Modify: `crates/towermd-core/src/hub.rs`（第 246–247、624–625、973 行；在 `pub(crate) fn timestamp` 前新增函数）
- Modify: `crates/towermd-core/src/project_runtime.rs`（第 20、300–360、1077–1098 行）
- Modify（改名）：`crates/towermd-core/tests/project_runtime.rs`、`crates/towermd/src/cli/task_service.rs`、`crates/towermd/src/dev/finish_service.rs`
- Test: `crates/towermd-core/tests/hub.rs`（追加 1 个测试）

- [ ] **Step 1：写失败测试**

追加到 `crates/towermd-core/tests/hub.rs` 末尾：

```rust
#[test]
fn work_lease_names_are_kind_colon_id() {
    use towermd_core::hub::is_work_lease;
    assert!(is_work_lease("module:A-one"));
    assert!(is_work_lease("ticket:42"));
    for name in ["main", "maintenance", ":x", "module:", ""] {
        assert!(!is_work_lease(name), "{name}");
    }
}
```

- [ ] **Step 2：运行，确认失败**

```bash
cargo test -p towermd-core --test hub work_lease_names_are_kind_colon_id
```

预期：编译失败，`cannot find function is_work_lease`。

- [ ] **Step 3：实现**

在 `crates/towermd-core/src/hub.rs` 的 `pub(crate) fn timestamp(now: u64)` 之前插入：

```rust
/// Work leases are named `<kind>:<id>`; `main` and `maintenance` are not.
pub fn is_work_lease(name: &str) -> bool {
    name.split_once(':').is_some_and(|(kind, id)| !kind.is_empty() && !id.is_empty())
}

```

然后：

```bash
perl -0pi -e 's/leases\.iter\(\)\.any\(\|lease\|lease\.name\.starts_with\("module:"\)\)/leases.iter().any(|lease|is_work_lease(&lease.name))/; s/"project pause forbids new module leases"/"project pause forbids new work leases"/; s/r\.names\.iter\(\)\.any\(\|name\|name\.starts_with\("module:"\)\)/r.names.iter().any(|name|is_work_lease(name))/; s/"项目已暂停，新任务领取和模块租约申请被拒绝"/"项目已暂停，新任务领取和工作租约申请被拒绝"/; s/or <harness>\/<version>"/or <producer>\/<version>"/' crates/towermd-core/src/hub.rs
perl -0pi -e 's/\bMODULE_TTL_MS\b/WORK_LEASE_TTL_MS/g; s/\bacquire_module_leases\b/acquire_work_leases/g; s/\brenew_module_leases\b/renew_work_leases/g; s/\brelease_module_leases\b/release_work_leases/g; s/\bmodule_leases\b/work_leases/g; s/\bmodule_names\b/work_names/g; s/\bmodule_ttl\b/work_ttl/g; s/\.filter\(\|\(name, _\)\| name\.starts_with\("module:"\)\)/.filter(|(name, _)| crate::hub::is_work_lease(name))/; s/\.any\(\|name\| name\.strip_prefix\("module:"\)\.is_none_or\(str::is_empty\)\)/.any(|name| !crate::hub::is_work_lease(name))/; s/"模块租约只接受非空 module:<id> 名称"/"工作租约只接受非空 <kind>:<id> 名称"/; s/"模块租约 TTL 必须在 1–900000 毫秒内"/"工作租约 TTL 必须在 1–900000 毫秒内"/' crates/towermd-core/src/project_runtime.rs
perl -pi -e 's/\bacquire_module_leases\b/acquire_work_leases/g; s/\brenew_module_leases\b/renew_work_leases/g; s/\brelease_module_leases\b/release_work_leases/g; s/\bmodule_leases\b/work_leases/g' crates/towermd-core/tests/project_runtime.rs crates/towermd/src/cli/task_service.rs crates/towermd/src/dev/finish_service.rs
grep -n 'module' crates/towermd-core/src/hub.rs crates/towermd-core/src/project_runtime.rs | awk -F: '{print $1":"$2}' | head
```

预期：最后的 grep 只剩 `project_runtime.rs` 约第 982 行的注释 `Old main/module guards`（不含 `module:`）。`hub.rs` 中不再有 `module`。二进制的常量 `MODULE_TTL` 不改。测试函数名 `module_leases_are_atomic_...` 因 `\b` 边界不会被改名，这是预期行为。

- [ ] **Step 4：运行，确认通过**

```bash
cargo test -p towermd-core --test hub --test project_runtime
cargo test -p towermd-devkit --test project_controls
```

预期：全部 PASS。`project_controls.rs:146` 的 `["module:A-two","maintenance"]` 在暂停时仍被拒，因为 `module:A-two` 是工作租约。

- [ ] **Step 5：全量回归并提交**

```bash
cargo test --workspace 2>&1 | grep -E '^test result' | awk '{s+=$4; f+=$6} END {print s" passed, "f" failed"}'
git add crates/towermd-core/src/hub.rs crates/towermd-core/src/project_runtime.rs crates/towermd-core/tests/hub.rs crates/towermd-core/tests/project_runtime.rs crates/towermd/src/cli/task_service.rs crates/towermd/src/dev/finish_service.rs
git commit -m "refactor(core): generalize module leases into work leases" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

预期：395 passed，0 failed。

---

### Task 10：问题绑定的 `module` 改称 `scope`（D9）

**Files:**
- Modify: `crates/towermd-core/src/question_model.rs`
- Modify: `crates/towermd/src/cli/question_service.rs`、`crates/towermd/src/cli/run_details_service.rs`、`crates/towermd-core/tests/question_projection.rs`、`crates/towermd-core/tests/question_model.rs`、`crates/towermd-core/tests/kit.rs`、`crates/towermd-devkit/src/dev_state.rs`
- Test: `crates/towermd-core/tests/question_model.rs`（追加 1 个格式锁测试）

- [ ] **Step 1：写失败测试**

追加到 `crates/towermd-core/tests/question_model.rs` 末尾：

```rust
#[test]
fn scope_fields_keep_the_v0_1_1_wire_names() {
    let context=RunContext{run:"R-1".into(),task:"T-1".into(),scope:"A-a".into()};
    assert_eq!(serde_json::to_value(&context).unwrap(),serde_json::json!({"run":"R-1","task":"T-1","module":"A-a"}));
    let scope:PromptScope=serde_json::from_value(serde_json::json!({"kind":"module","module":"A-a"})).unwrap();
    assert_eq!(scope,PromptScope::Scoped{scope:"A-a".into()});
    assert_eq!(serde_json::to_value(&scope).unwrap(),serde_json::json!({"kind":"module","module":"A-a"}));
}
```

- [ ] **Step 2：运行，确认失败**

```bash
cargo test -p towermd-core --test question_model scope_fields_keep_the_v0_1_1_wire_names
```

预期：编译失败，`struct RunContext has no field named scope`。

- [ ] **Step 3：实现（`question_model.rs` 的改写已在 03f15f4 上模拟核对）**

把下面的内容保存为 `target/t10.pl`：

```perl
s/\.module\b/.scope/g;
s/\bmodule:question\./scope:question./g;
s/^(\s*)pub module: String,/$1#[serde(rename = "module")]\n$1pub scope: String,/mg;
s/\bmodule: &str/scope: &str/g;
s/valid_id\(module\)\?;\n/valid_id(scope)?;\n/;
s/to_vec\(&\(module, normalized\)\)/to_vec(&(scope, normalized))/;
s/fingerprint\(module, question\)/fingerprint(scope, question)/;
s/d\.scope==module\)/d.scope==scope)/;
s/decision\.scope == module &&/decision.scope == scope &&/;
s/\?, module, question, decision, accepted_by,/?, scope: module, question, decision, accepted_by,/;
s/pub enum PromptScope \{ Global, Module \{ module: String \}, Task \{ task: String \} \}/pub enum PromptScope {\n    Global,\n    #[serde(rename = "module")]\n    Scoped {\n        #[serde(rename = "module")]\n        scope: String,\n    },\n    Task { task: String },\n}/;
s/PromptScope::Module\{module\}(\s*)=>(\s*)(valid_id\()?module/PromptScope::Scoped{scope}$1=>$2$3scope/g;
```

执行：

```bash
perl -0777 -p -i target/t10.pl crates/towermd-core/src/question_model.rs
perl -pi -e 's/(RunContext\{.*?)\bmodule:/$1scope:/g; s/question\.binding\.module\b/question.binding.scope/g; s/recipient\.module==/recipient.scope==/g; s/PromptScope::Module\{module:/PromptScope::Scoped{scope:/g' \
  crates/towermd/src/cli/question_service.rs crates/towermd/src/cli/run_details_service.rs \
  crates/towermd-core/tests/question_projection.rs crates/towermd-core/tests/question_model.rs \
  crates/towermd-core/tests/kit.rs crates/towermd-devkit/src/dev_state.rs
grep -nE "module:|Module" crates/towermd-core/src/question_model.rs
```

预期：最后的 grep 无输出。`question_model.rs` 里保留的 `"module"` 只出现在 `#[serde(rename = "module")]`、`get_str("module")`、`("module",decision.scope.as_str())` 和错误文案中。`fingerprint` 的哈希输入是 `(scope, normalized)` 这个元组，只是变量改名，值不变，所以已有决策的指纹不变。`Run.module`（`task_model`）不改。

- [ ] **Step 4：运行，确认通过**

```bash
cargo check --workspace --all-targets
cargo test -p towermd-core --test question_model --test question_projection --test kit
cargo test -p towermd-devkit --test replay_golden --test project_controls
```

预期：全部 PASS；replay_golden 4 passed，说明线上格式没有变。

- [ ] **Step 5：全量回归并提交**

```bash
cargo test --workspace 2>&1 | grep -E '^test result' | awk '{s+=$4; f+=$6} END {print s" passed, "f" failed"}'
git add crates/towermd-core/src/question_model.rs crates/towermd-core/tests crates/towermd/src/cli crates/towermd-devkit/src/dev_state.rs
git commit -m "refactor(core): rename question binding module to scope" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

预期：396 passed，0 failed。

---

### Task 11：init 拆分（D10）

**Files:**
- Modify: `crates/towermd-core/src/kit.rs`、`crates/towermd-core/src/init.rs`、`crates/towermd-core/templates/agents.md`（截成前 20 行）、`crates/towermd-core/tests/catalog.rs:720`、`crates/towermd/src/main.rs:436`、`crates/towermd-devkit/src/dev_state.rs`
- Create: `crates/towermd-devkit/templates/agents-dev.md`、`crates/towermd-devkit/tests/init.rs`、`crates/towermd-devkit/tests/fixtures/agents-v0.1.1.md`

- [ ] **Step 1：写失败测试**

```bash
mkdir -p crates/towermd-devkit/tests/fixtures
git show 03f15f4:crates/towermd-core/templates/agents.md > crates/towermd-devkit/tests/fixtures/agents-v0.1.1.md
```

`crates/towermd-devkit/tests/init.rs`：

```rust
use std::process::Command;

use towermd_core::init::{init, render_agents_md, InitOptions};
use towermd_devkit::dev_state::KITS;

#[test]
fn dev_agents_md_is_byte_identical_to_v0_1_1() {
    let expected = include_str!("fixtures/agents-v0.1.1.md").replace("{docs_dir}", "docs/tower");
    assert_eq!(render_agents_md("docs/tower", KITS), expected);
}

#[test]
fn dev_init_creates_the_v0_1_1_skeleton_and_skill_hint() {
    let dir = tempfile::tempdir().unwrap();
    let status = Command::new("git")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .args(["init", "-q", "-b", "main"])
        .arg(dir.path())
        .status()
        .unwrap();
    assert!(status.success());
    let report = init(dir.path(), &InitOptions::default(), KITS).unwrap();
    assert_eq!(report.created.len(), 7);
    for sub in ["intents", "arch", "specs", "baselines", "decisions"] {
        assert!(dir.path().join(&report.docs_dir).join(sub).is_dir(), "{sub}");
    }
    assert!(report.hints[0].contains("towermd-finish"), "{:?}", report.hints);
}
```

- [ ] **Step 2：运行，确认失败**

```bash
cargo test -p towermd-devkit --test init
```

预期：编译失败，`render_agents_md` 和 `init` 的参数个数不符。

- [ ] **Step 3：实现**

`kit.rs`：在 `KitRun` 之后加

```rust
/// What a kit adds to `towermd init`: directories under docs_dir (before the
/// core ones), a section appended to `.towermd/AGENTS.md`, and hints.
#[derive(Debug, Default)]
pub struct InitContribution {
    pub dirs: Vec<&'static str>,
    pub agents_section: &'static str,
    pub hints: Vec<String>,
}
```

并在 trait 末尾（`prove_protected_commit` 之后）加

```rust
    fn init(&self) -> InitContribution {
        InitContribution::default()
    }
```

模板拆分：

```bash
mkdir -p crates/towermd-devkit/templates
tail -n +21 crates/towermd-core/templates/agents.md > crates/towermd-devkit/templates/agents-dev.md
head -n 20 crates/towermd-core/templates/agents.md > target/agents-core.md && mv target/agents-core.md crates/towermd-core/templates/agents.md
head -1 crates/towermd-devkit/templates/agents-dev.md
```

预期：输出 `## 执行状态、完成与合并`。

`init.rs`：

- `use` 区加 `use crate::kit::Kit;`。
- 第 13 行 `const SKELETON: &[&str] = &["intents", "arch", "specs", "baselines", "decisions"];` 改为 `const CORE_SKELETON: &[&str] = &["decisions"];`。
- 第 32–34 行替换为：

```rust
pub fn render_agents_md(docs_dir: &str, kits: &[&dyn Kit]) -> String {
    let mut text = AGENTS_TEMPLATE.to_owned();
    for kit in kits {
        text.push_str(kit.init().agents_section);
    }
    text.replace("{docs_dir}", docs_dir)
}
```

- 第 65 行签名改为 `pub fn init(dir: &Path, opts: &InitOptions, kits: &[&dyn Kit]) -> Result<InitReport, String> {`，并在函数体第一行加 `let contributions: Vec<_> = kits.iter().map(|kit| kit.init()).collect();`。
- 第 92 行 `for subdirectory in SKELETON {` 改为

```rust
    let skeleton: Vec<&str> = contributions
        .iter()
        .flat_map(|contribution| contribution.dirs.iter().copied())
        .chain(CORE_SKELETON.iter().copied())
        .collect();
    for subdirectory in skeleton {
```

- 第 121 行改为 `let agents = render_agents_md(&docs_dir, kits);`。
- 第 127–128 行替换为

```rust
    let mut hints: Vec<String> =
        contributions.iter().flat_map(|contribution| contribution.hints.iter().cloned()).collect();
```

- `#[cfg(test)] mod tests` 内、`use super::*;` 之后加测试用的 Kit 和同名包装函数，原有测试调用不必改：

```rust
    struct TestKit;
    impl Kit for TestKit {
        fn init(&self) -> crate::kit::InitContribution {
            crate::kit::InitContribution {
                dirs: vec!["intents", "arch", "specs", "baselines"],
                agents_section: "",
                hints: vec!["test kit hint".into()],
            }
        }
    }
    fn init(dir: &Path, opts: &InitOptions) -> Result<InitReport, String> {
        super::init(dir, opts, &[&TestKit])
    }
    fn render_agents_md(docs_dir: &str) -> String {
        super::render_agents_md(docs_dir, &[&TestKit])
    }
```

- 在测试 `agents_template_contains_rules_without_availability_promises`（原第 716 行起）的 required 列表中删掉 `"命令行不提供批准类命令"` 和 `"进度只认证据"` 两项，这两句移到了 devkit 段落，由 devkit 的逐字节测试覆盖。

`crates/towermd-devkit/src/dev_state.rs`：

- `use` 的 `kit::{...}` 中加 `InitContribution`。
- 在 `impl Kit for DevKit` 中加：

```rust
    fn init(&self) -> InitContribution {
        InitContribution {
            dirs: vec!["intents", "arch", "specs", "baselines"],
            agents_section: include_str!("../templates/agents-dev.md"),
            hints: vec!["请按 Tower.md 发行包 skills/README.md 安装技能；在会话中使用 towermd-init、towermd-baseline、towermd-claim、towermd-ask 和 towermd-finish。安装由你选择，不修改全局 harness 配置。".into()],
        }
    }
```

调用方：

- `crates/towermd/src/main.rs:436` 改为 `let report = init(&directory, &InitOptions { docs_dir }, towermd_devkit::dev_state::KITS)?;`。
- `crates/towermd-core/tests/catalog.rs:720` 改为 `towermd_core::init::init(root.path(), &Default::default(), &[]).unwrap();`。执行前确认该测试不依赖开发目录（只读 config）；若依赖，改为在测试里定义与上面相同的 `TestKit`。**需在执行前细化**。

- [ ] **Step 4：运行，确认通过**

```bash
cargo test -p towermd-devkit --test init
cargo test -p towermd-core --lib init
cargo test -p towermd-core --test catalog
cargo test -p towermd --test init_cli
```

预期：全部 PASS。`init_cli` 证明真实 CLI 输出的 AGENTS.md、目录和提示与 v0.1.1 一致。如果核心内联测试里有断言提示顺序或第一条提示内容（原第 434、442 行附近），按 TestKit 的提示调整断言，不改实现。

- [ ] **Step 5：全量回归并提交**

```bash
cargo test --workspace 2>&1 | grep -E '^test result' | awk '{s+=$4; f+=$6} END {print s" passed, "f" failed"}'
git add crates/towermd-core/src/kit.rs crates/towermd-core/src/init.rs crates/towermd-core/templates/agents.md crates/towermd-core/tests/catalog.rs crates/towermd/src/main.rs crates/towermd-devkit/src/dev_state.rs crates/towermd-devkit/templates crates/towermd-devkit/tests/init.rs crates/towermd-devkit/tests/fixtures/agents-v0.1.1.md
git commit -m "refactor(init): let kits contribute skeleton and AGENTS rules" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

预期：398 passed，0 failed。

---

### Task 12：边界护栏 `boundary.rs`（D11）

**Files:**
- Create: `crates/towermd-core/tests/boundary.rs`
- Modify: `crates/towermd-core/src/durable_outbox.rs:64`、`crates/towermd-core/src/main_writer.rs:844`

- [ ] **Step 1：写失败测试 `crates/towermd-core/tests/boundary.rs`**

```rust
//! The core must stay free of development-kit vocabulary, and must never
//! depend on the development kit crate.
use std::{fs, path::Path};

const WORDS: [&str; 6] = ["worktree", "module:", "G3", "G5", "task.merged", "harness"];
/// Literal git subcommand: the single writer must see every checkout of main.
const ALLOWED: [&str; 1] = [r#"&["worktree", "list", "--porcelain", "-z"],"#];

#[test]
fn core_sources_do_not_name_kit_concepts() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files: Vec<_> = fs::read_dir(&src)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "rs"))
        .collect();
    files.sort();
    let mut hits = Vec::new();
    for path in files {
        let text = fs::read_to_string(&path).unwrap();
        for (index, line) in text.lines().enumerate() {
            if line.trim_start().starts_with("#[cfg(test)]") {
                break;
            }
            if ALLOWED.contains(&line.trim()) {
                continue;
            }
            for word in WORDS {
                if line.contains(word) {
                    let name = path.file_name().unwrap().to_string_lossy();
                    hits.push(format!("{name}:{}: {word}", index + 1));
                }
            }
        }
    }
    assert!(hits.is_empty(), "core names kit concepts:\n{}", hits.join("\n"));
}

#[test]
fn core_manifest_does_not_depend_on_devkit() {
    let manifest = fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml")).unwrap();
    assert!(!manifest.contains("towermd-devkit"));
}
```

- [ ] **Step 2：运行，确认失败**

```bash
cargo test -p towermd-core --test boundary
```

预期：`core_sources_do_not_name_kit_concepts` FAIL，命中恰好两行：`durable_outbox.rs:64: worktree` 和 `main_writer.rs:844: worktree`。如果还有其他命中，说明 T9–T11 有遗漏，回到对应任务修正，不要扩大白名单。

- [ ] **Step 3：改写两条注释**

```bash
perl -pi -e "s/not a worktree's \`\.git\`/not a linked checkout's \`.git\`/" crates/towermd-core/src/durable_outbox.rs
perl -pi -e 's/never move main if another worktree/never move main if another checkout/' crates/towermd-core/src/main_writer.rs
```

- [ ] **Step 4：运行，确认通过**

```bash
cargo test -p towermd-core --test boundary
```

预期：2 passed。

- [ ] **Step 5：全量回归并提交**

```bash
cargo test --workspace 2>&1 | grep -E '^test result' | awk '{s+=$4; f+=$6} END {print s" passed, "f" failed"}'
cargo test --manifest-path src-tauri/Cargo.toml 2>&1 | grep -E '^test result'
git add crates/towermd-core/tests/boundary.rs crates/towermd-core/src/durable_outbox.rs crates/towermd-core/src/main_writer.rs
git commit -m "test(core): guard core against development kit vocabulary" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

预期：**400 passed**，0 failed；src-tauri 19 passed、1 ignored。

---

### Task 13：更新 `AGENTS.md` 与 `tasks/todo.md`

**Files:**
- Modify: `AGENTS.md:208`（结构约定）、`AGENTS.md:212-223`（初始目录布局代码块）
- Modify: `tasks/todo.md`（在第 397 行之后追加新节，不动已有各节）

- [ ] **Step 1：确认 `AGENTS.md` 已被跟踪**

```bash
git ls-files --error-unmatch AGENTS.md
```

预期：输出 `AGENTS.md`。它已被跟踪，本任务直接提交。

- [ ] **Step 2：改 `AGENTS.md`**

第 208 行整段替换为：

```markdown
**结构约定：一个核心，一个开发套件，三个适配层。** 与工作方式无关的机制（事件流、hub 与租约、快照、问题与插话、项目控制、人定内容保护、`Kit` 接缝）放在 `towermd-core`；软件开发专属的业务（基线、任务与运行、完成证据、提案、冷启动与核验）放在 `towermd-devkit`，经编译期 `Kit` trait 挂到核心上。核心不得依赖开发套件，也不得出现开发词汇，由 `crates/towermd-core/tests/boundary.rs` 守住。守护进程与 CLI、Tauri 画布、MCP 交互网关都只是适配层。这与 Note.md 的 `searchidx` 被 Tauri 命令、CLI、文件监视三方共用是同一做法。
```

代码块中第 213 行替换为

```text
crates/towermd-core/   # 核心：对象模型、事件流、hub 与租约、快照、问题与插话、项目控制、Kit 接缝
crates/towermd-devkit/ # 开发套件：基线、任务与运行、完成证据、提案、冷启动与核验（DevKit）
```

第 214 行保持不变，再在其后改为：

```text
crates/towermd/        # 守护进程与 CLI（clap），含本地 hub；src/cli/ 核心服务，src/dev/ 开发服务；M3 起含 MCP 交互网关
```

- [ ] **Step 3：追加 `tasks/todo.md` 新节**

```bash
{
  printf '\n## 计划：核心与开发套件拆分 A 部分（2026-10-21，已实现）\n\n'
  printf '依据：`docs/superpowers/specs/2026-10-21-core-devkit-split-and-cli-c0-design.md`；计划：`docs/superpowers/plans/2026-10-21-a-core-devkit-split.md`（偏差 D1–D14 见计划首节）。\n\n'
  git log --reverse --format='- [x] %s（%h）' 03f15f4..HEAD
  printf -- '- [x] T13 文档：AGENTS.md 结构约定与目录布局加入 `towermd-devkit`（本条提交）\n'
  printf -- '- [ ] 检查 Note.md 的 `AGENTS.md` 是否需要同步：本次改的是 Tower.md 专属的结构约定，预期不需要，由人确认\n\n'
  printf '### 回顾\n\n'
  printf -- '- 测试：workspace 381 → 400（新增 19：重放样本 4、Kit 7、路径 1、自述来源 1、工作租约 1、scope 格式 1、init 2、边界 2），src-tauri 19 + 1 ignored 不变。\n'
  printf -- '- v0.1.1 四份真实流程样本在每一步都逐字重放一致；磁盘格式与事件名、字段未变。\n'
  printf -- '- 用户可见文案变化：租约暂停与校验错误改称“工作租约”，自述来源校验改称“自述来源名称/版本”；是否写入 CHANGELOG 未发布区由人决定。\n'
  printf -- '- 遗留：types 注册表拆分（D7）、核心 Task 拆分（D4）另行立项。\n'
} >> tasks/todo.md
tail -20 tasks/todo.md
```

预期：新节出现在“计划：命令行优先”节之后，列出 T1–T12 的提交标题和短号；已有各节不变（`git diff tasks/todo.md` 只有新增行）。

- [ ] **Step 4：最终验证并提交**

```bash
git diff --stat AGENTS.md tasks/todo.md
cargo test --workspace 2>&1 | grep -E '^test result' | awk '{s+=$4; f+=$6} END {print s" passed, "f" failed"}'
git add AGENTS.md tasks/todo.md
git commit -m "docs: record core/devkit structure in AGENTS.md and todo" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
git log --oneline 03f15f4..HEAD
```

预期：400 passed；`git log` 列出 13 个提交（T1–T13）。最后提醒人：Note.md 的 `AGENTS.md` 是否同步，需要人确认（AGENTS.md 要求两边改动时互查）。

---

## 风险与待确认

- **R1 重放等价。** 唯一保证是 T1 的四份样本。样本没有覆盖 `recovery.completed` 和 `lease.expired`；这两条路径在 T5 中由 `release_runs` 重写（逻辑与原代码逐行对应），只有现有单元测试（`project_runtime.rs:507` 一组、恢复测试）间接覆盖。
- **R2 `Event` 逐字节往返尚未证实。** 如果 T1 的 `v0_1_1_event_lines_round_trip_byte_for_byte` 失败（例如 `cost_usd` 浮点格式或 `extra` 键序），需要人决定：放宽成 `Value` 相等，或保持严格并排查。
- **R3 `proposal_model` 改在 Kit 兜底中执行。** 原来它在 `project_controls` 之前；两者处理的事件种类不相交，顺序变化不影响结果。
- **R4 工作租约规则变宽。** 任何 `<kind>:<id>` 都会被暂停拦截；目前除 `module:` 外没有带冒号的租约名。
- **R5 `scope` 改名面广**（T10 共 7 个文件）。线上键名由 T10 的格式锁测试和 replay_golden 守住。
- **R6 `repo` 两个函数改为 `pub`，`PathRule` 改为 `pub`。** 核心公开 API 变大。
- **R7 `validate_prompt` 查找方式。** 原代码按 `runs` 映射的键查找，现在按 `binding.id` 查找；投影中两者一致。
- **R8 用户可见文案变化。** 涉及“工作租约”“自述来源名称/版本”“`<producer>/<version>`”。是否写进 CHANGELOG 未发布区由人决定；仓库目前还没有 CHANGELOG。
- **R9 `Assembly` 的 `kits` 与 `reduce` 是否配对不做检查。** 目前只有 `dev_state::ASSEMBLY` 一个装配。
- **R10 核心模板仍提到开发目录。** 前 20 行有 `intents/`、`arch/` 等慢层目录说明；它们属于核心的慢层保护边界（D10），boundary 测试只扫 `.rs`。
- **R11 测试迁移。** 核心集成测试 `main_inspection`、`main_resolution`、`project_controls` 移到 devkit，名称不变（D14）。
- **R12 样本含临时路径。** 只在样本内部自洽，不影响断言。
- **需在执行前细化：** T7 Step 7 的行号复核；T11 中 `catalog.rs:720` 和 init 内联测试的提示断言。

## 自查

**规格覆盖**（设计第 2 节 → 任务）：

| 设计要求 | 对应任务 |
| --- | --- |
| 新 crate 与依赖方向 | T7 |
| Kit trait | T2、T5、T6、T11（D3） |
| 投影扩展 | T2（D2） |
| A1 搬家与测试数相等 | T7、T8（D1、D12） |
| `project_state` 拆分 | T5 |
| `task_model` | T3、T4、T7（D4、D5） |
| `types` 注册表 | 不做（D7） |
| `hub` 租约 | T9（D8） |
| `question_model` 的 `scope` | T4、T10（D9） |
| init | T11（D10） |
| `boundary.rs` 与 `<producer>/<version>` | T9、T12（D11） |
| 兼容性（格式与事件名不变） | T1 样本贯穿 T2–T12，T10 格式锁 |
| A1 验收（`cargo tree`） | T0、T7 |
| A2 验收（护栏与重放） | T12、T1 |
| AGENTS.md 与 todo | T13 |

B 部分（CLI C0）不在本计划内。

**占位扫描：** 计划中没有 TBD/TODO，也没有“类似 Task N”。每个代码步骤都给出完整代码，或给出可执行的确定脚本（行号基于 `03f15f4` 并已模拟核对的，写明了核对方式）。两处行号依赖执行时文件状态的地方标为“需在执行前细化”。

**类型一致性：**

| 名称 | 签名或定义 | 定义/修改的任务 | 后续使用 |
| --- | --- | --- | --- |
| `Kit` | `state_groups`、`reduce -> Result<bool,String>`、`runs -> Result<Vec<KitRun>,String>`、`prove_protected_commit(&Path,&str,&str,&dyn Fn(&str)->Option<Event>)`、`init -> InitContribution` | T2/T5/T6/T11 | `dev_state` 与测试中的实现一致 |
| `project_state::initial_with`、`apply_with`、`core_initial`、`core_apply`、`CORE_GROUPS`、`insert_unique`、`valid_oid` | T2/T5 定义 | | T5–T7 使用 |
| `dev_state::{DevKit, GROUPS, KITS, initial, apply, tasks, ASSEMBLY}` | | T2/T5/T6 | 二进制和 devkit 测试使用 |
| `project_controls::{active, prepare_pause, prepare_resume, apply}` | 最后一个参数统一为 `kits: &[&dyn Kit]` | T5 | |
| `kit::{NO_PROOF, prove_by_kits, Assembly{kits,reduce}, KitRun{context,active}, InitContribution{dirs,agents_section,hints}}` | | T5/T6/T11 | 前后一致 |
| `main_inspection::inspect` 与 `snapshot_publication::prove` | 末参 `assembly: &Assembly` | T6 | 调用方统一传 `&towermd_*::dev_state::ASSEMBLY` |
| `hub::is_work_lease` | | T9 | |
| `question_model::SelfReported` | | T4 | |
| `RunContext.scope`、`PromptScope::Scoped{scope}` | | T10 | |
