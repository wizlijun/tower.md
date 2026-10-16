---
type: Plan
title: M0 整改批次 5：身份重绑、层级保护、画布与任务一致性及真实文档
description: R10–R13 与 R15 的最小 TDD 实施步骤；保持已批准契约，不提前宣称其他整改批次完成。
status: draft
sources:
  - resource: docs/superpowers/plans/2026-10-17-m0-remediation.md
  - resource: docs/specs/2026-10-16-spec-towermd-layered-agent-canvas-mvp.md
  - resource: AGENTS.md
---

# M0 整改批次 5 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 修正 R10–R13 的身份、保护与诊断偏差，并让 README、两语未发布日志和任务记录只描述已有源码与验收证据支持的行为。

**Architecture:** 保留现有两个 crate，不新增依赖或目录层级。身份仍由用户运行目录的项目锁协调；对象层级只由一个三态函数解释；画布继续保留原文字节，几何校验按条目处理。任务测试只验证命令结果、事件顺序和恢复后的业务状态，不固定投影内部实现。

**Tech Stack:** 现有 Rust 2021、serde/serde_json/serde_yaml、fs2、Git CLI、tempfile；现有 cargo 测试、fmt、Clippy。

---

## 执行边界与基线

- 人已批准整改总表 D0–D6 推荐方案；R16 已同步。此计划基于主仓库 HEAD `a781b29`，源码基线 320 项通过；这不是本计划实现后的验证证据。
- 本文件仅为计划，所有实现步骤尚未执行；本次编写只新增本文件，不修改源码或提交。
- R10–R12 与 R13 接口诊断可独立实施。任务状态部分最好在批次 2 的 `HubProtocol`、常驻增量投影合入后实施；若先实施，后续迁移保持本计划的业务断言，不能恢复全量 replay。
- 不写基线/批准命令，不新增绕过慢层保护的路径，不修改用户全局 Git/harness 配置；本轮不推送。
- R15 分两次核对：先更新本批已完成行为；其他批次完成后再补对应行为。没有源码、测试和提交证据的批次保持未完成。
- 计划中的代码块是明确插入/替换单元，保留函数其余内容。批次 2 改过的 Hub 调用以当时 `HubProtocol` 的签名映射；不修改业务测试断言迁就旧实现。

## 文件责任与拆分

| 任务 | 修改/测试精确路径 | 责任 |
| --- | --- | --- |
| 1 | `crates/towermd/src/daemon.rs`；`crates/towermd/tests/daemon_cli.rs` | 持久 clone owner 判定、锁内重绑记录、内存旧项目安全退役 |
| 2 | `crates/towermd-core/src/types.rs`、`crates/towermd-core/src/catalog.rs`；`crates/towermd-core/tests/catalog.rs`、`crates/towermd-core/tests/tasks.rs` | 层级三态、非法字段诊断及路径保护 |
| 3 | `crates/towermd-core/src/canvas.rs`；`crates/towermd-core/tests/canvas.rs` | 限制只作用于几何，原文与逐节点 opaque 保留 |
| 4 | `crates/towermd-core/src/tasks.rs`；`crates/towermd-core/tests/tasks.rs` | 本机运行提示只列活跃运行 |
| 5 | `crates/towermd-core/src/tasks.rs`；`crates/towermd-core/tests/tasks.rs` | G0 后记录 proposed→ready，再领取 |
| 6 | `crates/towermd-core/src/canvas_view.rs`；`crates/towermd-core/tests/canvas_view.rs` | 未加载接口时给未解析诊断 |
| 7 | `README.md`、`CHANGELOG.md`、`CHANGELOG.zh-CN.md`、`tasks/todo.md` | 能力说明、双语增量日志、历史状态与提交证据 |

不新建测试框架、仓库常驻检查脚本或接口 YAML 解析器。上表每一项都可独立测试并独立提交；不能把其他在途任务文件加入提交。

## Task 1: R10 — 失效 clone 绑定可重绑，活跃身份仍互斥

**Files:**
- Modify/Test: `crates/towermd/src/daemon.rs`（`bind_project`、`validate_project_owner`、`Server::project`、`renew_existing_read`、`mod tests`）
- Test: `crates/towermd/tests/daemon_cli.rs`（已有 `Fixture` 与 clone 拒绝测试）

### 1A. 纯身份判定与锁内记录

- [ ] **Step 1: 写失败测试。** 在 daemon 单测模块新增真实 Git owner 夹具；不再用两个空临时目录冒充同项目 clone。保留项目文件锁竞争测试，但替换 `clone_identity_binding_survives_daemon_restart` 的永久拒绝断言。

```rust
fn owner_repository(id: &str) -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    let run = |args: &[&str]| {
        let output = Command::new("git")
            .args(["-c", "user.name=Test", "-c", "user.email=test@example.invalid",
                   "-c", "commit.gpgsign=false"])
            .args(args).current_dir(root.path())
            .env("GIT_CONFIG_GLOBAL", "/dev/null").env("GIT_CONFIG_NOSYSTEM", "1")
            .output().unwrap();
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    };
    run(&["init", "-qb", "main"]);
    fs::create_dir(root.path().join(".towermd")).unwrap();
    fs::write(root.path().join(".towermd/config.yaml"),
        towermd_core::config::Config::new(id, "docs/tower").unwrap().to_yaml()).unwrap();
    run(&["add", ".towermd/config.yaml"]);
    run(&["commit", "-qm", "owner"]);
    root
}

#[test]
fn stale_owner_rebinds_and_records_both_common_dirs() {
    let runtime = tempfile::tempdir().unwrap();
    let first = owner_repository("same-id");
    let second = owner_repository("same-id");
    let old = first.path().join(".git").canonicalize().unwrap();
    let new = second.path().join(".git").canonicalize().unwrap();
    let lock = bind_project(runtime.path(), "same-id", &old).unwrap();
    assert!(bind_project(runtime.path(), "same-id", &old).is_err());
    drop(lock);
    first.close().unwrap();
    let _lock = bind_project(runtime.path(), "same-id", &new).unwrap();
    let name = hex::encode(Sha256::digest(b"same-id"));
    let saved: PathBuf = serde_json::from_slice(
        &fs::read(runtime.path().join(format!("project-{name}.json"))).unwrap()).unwrap();
    assert_eq!(saved, new);
    let records = fs::read_to_string(
        runtime.path().join(format!("project-{name}-rebindings.jsonl"))).unwrap();
    let record: Value = serde_json::from_str(records.lines().next().unwrap()).unwrap();
    assert_eq!(record["actor"], "process:towermd-daemon");
    assert_eq!(record["old_common_dir"], json!(old));
    assert_eq!(record["new_common_dir"], json!(new));
    assert!(record["at_ms"].as_u64().is_some());
}

#[test]
fn living_same_project_owner_blocks_but_changed_project_owner_does_not() {
    let first = owner_repository("same-id");
    let second = owner_repository("same-id");
    let old = first.path().join(".git").canonicalize().unwrap();
    let new = second.path().join(".git").canonicalize().unwrap();
    let bytes = serde_json::to_vec(&old).unwrap();
    assert!(validate_project_owner(&bytes, "same-id", &new).is_err());
    assert_eq!(validate_project_owner(&bytes, "same-id", &old).unwrap(), None);
    assert_eq!(validate_project_owner(&bytes, "other-id", &new).unwrap(), Some(old));
}
```

补 remote 权威测试；旧 main/缓存配置无法读取或解析时报包含旧 common 和所选 ref 的诊断，不能把错误伪装成 same-id 或静默重绑。

```rust
#[test]
fn owner_identity_uses_cached_origin_main_without_fetch() {
    let first = owner_repository("same-id");
    let second = owner_repository("same-id");
    let root = first.path();
    let git = |args: &[&str]| {
        towermd_core::repo::git(root, args).unwrap()
    };
    git(&["update-ref", "refs/remotes/origin/main", "main"]);
    git(&["remote", "add", "origin", "/does/not/exist"]);
    let authority = git(&["rev-parse", "refs/remotes/origin/main"]);
    fs::write(root.join(".towermd/config.yaml"),
        towermd_core::config::Config::new("other-id", "docs/tower").unwrap().to_yaml()).unwrap();
    git(&["add", ".towermd/config.yaml"]);
    git(&["-c", "user.name=Test", "-c", "user.email=test@example.invalid",
           "-c", "commit.gpgsign=false", "commit", "-qm", "different local identity"]);
    let old = root.join(".git").canonicalize().unwrap();
    let new = second.path().join(".git").canonicalize().unwrap();
    let bytes = serde_json::to_vec(&old).unwrap();
    assert!(validate_project_owner(&bytes, "same-id", &new).is_err());
    assert_eq!(validate_project_owner(&bytes, "other-id", &new).unwrap(), Some(old));
    assert_eq!(git(&["rev-parse", "refs/remotes/origin/main"]), authority);
}
```

- [ ] **Step 2: 跑红。** 使用过滤命令并确认各自实际执行一项测试：

```sh
cargo test -p towermd --locked stale_owner_rebinds_and_records_both_common_dirs -- --nocapture
cargo test -p towermd --locked living_same_project_owner_blocks_but_changed_project_owner_does_not -- --nocapture
cargo test -p towermd --locked owner_identity_uses_cached_origin_main_without_fetch -- --nocapture
```

预期：旧签名编译失败；改签名但未改逻辑时第一个测试报 clone 永久绑定。测试不得零匹配。

- [ ] **Step 3: 最小实现纯判定。** 保留 owner 文件的 JSON PathBuf 兼容格式。`validate_project_owner` 返回 `None` 表示同 common，`Some(old)` 表示允许改绑；只在旧 common 存在、权威配置为同项目、common 不同三个条件同时满足时拒绝身份冲突。使用现有 `repo::git`，不 fetch，不读任意工作树配置。

```rust
fn validate_project_owner(
    bytes: &[u8], project_id: &str, common_dir: &Path,
) -> Result<Option<PathBuf>, String> {
    let owner: PathBuf = serde_json::from_slice(bytes)
        .map_err(|error| format!("项目 clone 绑定损坏：{error}"))?;
    if owner == common_dir { return Ok(None); }
    if !owner.exists() { return Ok(Some(owner)); }
    let remotes = towermd_core::repo::git(&owner, &["remote"])
        .map_err(|error| format!("无法读取旧 clone {}：{error}", owner.display()))?;
    let reference = if remotes.lines().any(|name| name == "origin") {
        "refs/remotes/origin/main:.towermd/config.yaml"
    } else {
        "refs/heads/main:.towermd/config.yaml"
    };
    let text = towermd_core::repo::git(&owner, &["show", reference])
        .map_err(|error| format!("无法读取旧 clone {} 的 {reference}：{error}", owner.display()))?;
    let config = towermd_core::config::Config::parse(&text)
        .map_err(|error| format!("旧 clone {} 的 {reference} 配置无效：{error}", owner.display()))?;
    if config.project_id() == project_id {
        return Err("同一项目 ID 已由另一个 clone 占用；请使用 git worktree 或云 hub".into());
    }
    Ok(Some(owner))
}
```

在 `bind_project` 已取得独占项目文件锁后，替换 `Ok(bytes)` 分支。先同步记录再覆盖 owner；记录是运行时审计，不是新的业务事件，也不写 `.towermd/` 或用户文档。

```rust
Ok(bytes) => {
    if let Some(old) = validate_project_owner(&bytes, project_id, common_dir)? {
        record_project_rebinding(runtime, &lock_name, project_id, &old, common_dir)?;
        write_json(&owner_path, &common_dir)?;
    }
}
```

```rust
fn record_project_rebinding(
    runtime: &Path, lock_name: &str, project_id: &str, old: &Path, new: &Path,
) -> Result<(), String> {
    let path = runtime.join(format!("project-{lock_name}-rebindings.jsonl"));
    let mut file = OpenOptions::new().create(true).append(true).open(path)
        .map_err(|error| format!("无法记录 clone 重绑：{error}"))?;
    serde_json::to_writer(&mut file, &json!({
        "actor":"process:towermd-daemon", "at_ms":now_ms()?, "project_id":project_id,
        "old_common_dir":old, "new_common_dir":new,
    })).map_err(|error| format!("无法编码 clone 重绑记录：{error}"))?;
    file.write_all(b"\n").and_then(|_| file.sync_all())
        .map_err(|error| format!("无法同步 clone 重绑记录：{error}"))
}
```

记录失败不改 owner；owner 更新失败允许重复记录尝试，记录不得称作成功完成的业务事件。`renew_existing_read` 调用新判定签名；读取路径只判断，不取得新项目写入权，也不覆盖 owner。

- [ ] **Step 4: 跑绿。** 运行上面两个过滤测试和 remote 权威测试，再运行 `cargo test -p towermd --locked --bin towermd`。预期全部通过；活跃旧 clone、同 common 文件锁竞争仍拒绝。

### 1B. daemon 生命周期与产品路径

- [ ] **Step 5: 写产品路径失败测试。** 使用 `daemon_cli.rs` 已有 `Fixture`。测试内容为两个独立场景，每个都用同一个 runtime：第一种先停止 daemon 再删除旧 clone；第二种旧 daemon 保持运行再删除旧 clone。克隆时移除新 clone 的 origin，避免测试依赖已删除路径。每种场景调用新 clone 的 `status`，成功并保持项目 ID；检查运行时改绑记录，检查新 clone 工作树及 index 的 `git status --porcelain=v1` 前后相同。原 `duplicate_clone_identity_is_rejected_but_other_project_works` 继续通过。

测试主体可插入以下函数（fixture 自带 Drop 清理服务；末尾从新 clone 显式停止）：

```rust
#[test]
fn removed_clone_can_rebind_after_restart_or_in_same_daemon() {
    for restart in [true, false] {
        let fixture = Fixture::new();
        let before = fixture.json(fixture.root.path(), &["status"]);
        let next = tempfile::tempdir().unwrap();
        fixture.git(&["clone", "-q", fixture.root.path().to_str().unwrap(),
                      next.path().to_str().unwrap()]);
        let run_git = |args: &[&str]| {
            let output = Command::new("git").args(args).current_dir(next.path())
                .env("GIT_CONFIG_GLOBAL", "/dev/null").env("GIT_CONFIG_NOSYSTEM", "1")
                .output().unwrap();
            assert!(output.status.success());
            output.stdout
        };
        run_git(&["remote", "remove", "origin"]);
        let clean = run_git(&["status", "--porcelain=v1"]);
        if restart { fixture.success(fixture.root.path(), &["daemon", "stop"]); }
        fs::remove_dir_all(fixture.root.path()).unwrap();
        let mut after = None;
        wait_until(|| {
            let output = fixture.command(next.path(), &["status"]).output().unwrap();
            if output.status.success() {
                after = Some(serde_json::from_slice::<Value>(&output.stdout).unwrap());
                true
            } else {
                assert!(String::from_utf8_lossy(&output.stderr)
                    .contains("旧 clone 的操作尚未结束"));
                false
            }
        });
        assert_eq!(before["project_id"], after.unwrap()["project_id"]);
        assert_eq!(run_git(&["status", "--porcelain=v1"]), clean);
        fixture.success(next.path(), &["daemon", "stop"]);
    }
}
```

本测试用现有 `wait_until` 在 10 秒内只重试明确 busy，禁止吞掉其他错误。新增单测验证“Hub Arc 尚被其他操作持有时不能丢弃项目锁”，插入 daemon 单测模块：

```rust
#[test]
fn project_retirement_waits_for_project_and_hub_users() {
    let runtime = tempfile::tempdir().unwrap();
    let root = owner_repository("project");
    let common = root.path().join(".git").canonicalize().unwrap();
    let project = Arc::new(Project {
        common_dir: common.clone(), root: Mutex::new(root.path().to_owned()),
        hub: Arc::new(Mutex::new(Hub::open(&common.join("towermd")).unwrap())),
        writer: Arc::new(SnapshotWriter::default()),
        snapshot_interval: AtomicU64::new(1_800_000),
        reconcile: AtomicBool::new(false), background: Mutex::new(None),
        _lock: bind_project(runtime.path(), "project", &common).unwrap(),
    });
    assert!(project_is_idle(&project));
    let request = project.clone();
    assert!(!project_is_idle(&project));
    drop(request);
    let hub_user = project.hub.clone();
    assert!(!project_is_idle(&project));
    assert!(bind_project(runtime.path(), "project", &common).is_err());
    drop(hub_user);
    assert!(project_is_idle(&project));
}
```

- [ ] **Step 6: 跑红。**

```sh
cargo test -p towermd --test daemon_cli --locked removed_clone_can_rebind_after_restart_or_in_same_daemon -- --nocapture
```

预期原来的内存 `existing.common_dir != catalog.common_dir` 永久拒绝场景失败。

- [ ] **Step 7: 最小安全退役。** 在 `Server::project` 持 `projects` 互斥锁时，用 Task 1A 同一个 owner 判定检查旧 common；仅允许失效身份进入退役分支。现有 `Server::execute` 可能只持有 Hub Arc，检查必须同时包含 Project 与 Hub 的在途引用。代码单元为：

```rust
fn project_is_idle(project: &Arc<Project>) -> bool {
    Arc::strong_count(project) == 1 && Arc::strong_count(&project.hub) == 1
}
```

在已有 `if let Some(existing)` 分支前插入：

```rust
if let Some(existing) = projects.get(project_id) {
    if existing.common_dir != catalog.common_dir {
        let bytes = serde_json::to_vec(&existing.common_dir)
            .map_err(|error| error.to_string())?;
        validate_project_owner(&bytes, project_id, &catalog.common_dir)?;
        if !project_is_idle(existing) {
            return Err("旧 clone 的操作尚未结束，请稍后重试重绑".into());
        }
        projects.remove(project_id);
    }
}
```

之后沿用原来“无注册项→bind_project→打开新 hub”路径；`remove` 后必须立即释放旧 Project 及文件锁，不能先 clone 旧 Arc。持注册表锁检查引用并移除，避免检查之后又产生新的旧 Project 调用；在途 tick/写快照/Hub 请求持有 Arc 时只报忙，不强制解锁、不并行启动新定序源。若批次 2 已统一所有操作持有项目 Session，用其已有生命周期计数替代以上两个计数，断言保持不变。

`renew_existing_read` 内存分支替换单纯的 common 不同错误；下一次项目命令负责持锁重绑，不能在只读续租函数中直接替换 hub。已有同 common 的工作树更新逻辑保持。

```rust
if project.common_dir != catalog.common_dir {
    let owner = serde_json::to_vec(&project.common_dir).map_err(|error| error.to_string())?;
    validate_project_owner(&owner, catalog.config.project_id(), &catalog.common_dir)?;
    return Err("尚未注册新 clone，请先调用 status 完成重绑".into());
}
```

无内存项目时的旧 owner 校验调用改为：

```rust
validate_project_owner(&owner, catalog.config.project_id(), &catalog.common_dir)?;
```

- [ ] **Step 8: 跑绿并提交。**

```sh
cargo test -p towermd --locked --bin towermd
cargo test -p towermd --test daemon_cli --locked removed_clone -- --nocapture
cargo test -p towermd --test daemon_cli --locked duplicate_clone_identity -- --nocapture
cargo fmt --all -- --check
git add crates/towermd/src/daemon.rs crates/towermd/tests/daemon_cli.rs
git commit -m "fix: allow stale local clone bindings to rebind safely"
```

预期测试全部通过。提交前显式检查两个文件没有包含批次 1/2/3 的未验证改动；有重叠时只暂存本任务补丁或在已隔离工作树提交，不整文件夹带。

## Task 2: R11 — 层级三态统一，非法层级受保护且可诊断

**Files:**
- Modify/Test: `crates/towermd-core/src/types.rs`
- Modify: `crates/towermd-core/src/catalog.rs`
- Test: `crates/towermd-core/tests/catalog.rs`
- Modify/Test: `crates/towermd-core/tests/tasks.rs`（Catalog 夹具初始化新字段）

- [ ] **Step 1: 写三态红测。** 替换把非法 layer 当默认层的旧测试，同时更新有效/未知 type 测试的期望类型。

```rust
#[test]
fn layer_resolution_distinguishes_missing_valid_and_invalid() {
    assert_eq!(layer_of(&doc("---\ntype: Task\n---\n")),
        LayerResolution::Missing(Some(Layer::L4)));
    assert_eq!(layer_of(&doc("---\ntype: FutureThing\n---\n")),
        LayerResolution::Missing(None));
    assert_eq!(layer_of(&doc("---\ntype: Task\nlayer: L2\n---\n")),
        LayerResolution::Valid(Layer::L2));
    for value in ["L9", "null", "42", "[]", "{}", "''"] {
        let document = doc(&format!("---\ntype: Task\nlayer: {value}\n---\n"));
        assert_eq!(layer_of(&document), LayerResolution::Invalid);
        assert!(layer_of(&document).is_protected());
    }
    assert!(!layer_of(&doc("---\ntype: Task\n---\n")).is_protected());
    assert!(layer_of(&doc("---\ntype: Decision\n---\n")).is_protected());
}
```

有效/未知 type 两个既有测试替换为以下代码；旧 `missing_or_invalid_layer_falls_back_to_default` 由前面的三态测试取代，防止遗留 Option 断言导致编译失败。

```rust
#[test]
fn layer_field_wins_over_default() {
    assert_eq!(layer_of(&doc("---\ntype: Decision\nlayer: L4\n---\n")),
        LayerResolution::Valid(Layer::L4));
}

#[test]
fn unknown_type_preserves_missing_valid_and_invalid_layer_states() {
    assert_eq!(layer_of(&doc("---\ntype: FutureThing\n---\n")),
        LayerResolution::Missing(None));
    assert_eq!(layer_of(&doc("---\ntype: FutureThing\nlayer: L2\n---\n")),
        LayerResolution::Valid(Layer::L2));
    assert_eq!(layer_of(&doc("---\ntype: FutureThing\nlayer: L9\n---\n")),
        LayerResolution::Invalid);
}
```

- [ ] **Step 2: 验证红。** `cargo test -p towermd-core --locked layer_resolution_distinguishes_missing_valid_and_invalid`，预期 `LayerResolution` 尚不存在而编译失败。

- [ ] **Step 3: 最小三态实现。** `types.rs` 替换 `layer_of` 并添加结果类型；不新增第二个解析函数。

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayerResolution {
    Valid(Layer),
    Missing(Option<Layer>),
    Invalid,
}

impl LayerResolution {
    pub fn is_protected(self) -> bool {
        match self {
            Self::Valid(layer) | Self::Missing(Some(layer)) => layer.is_slow(),
            Self::Missing(None) => false,
            Self::Invalid => true,
        }
    }
}

pub fn layer_of(document: &Document) -> LayerResolution {
    match document.get("layer") {
        None => LayerResolution::Missing(document.doc_type()
            .and_then(lookup).map(|entry| entry.default_layer)),
        Some(_) => document.get_str("layer").and_then(Layer::parse)
            .map_or(LayerResolution::Invalid, LayerResolution::Valid),
    }
}
```

- [ ] **Step 4: 写 Catalog 红测。** 使用 `tests/catalog.rs` 的 `repository`、`write`、`commit`；必须同时验证目录外非法 Task、路径守卫和原文未变。

```rust
#[test]
fn invalid_layer_is_diagnosed_and_protected_instead_of_defaulting_fast() {
    let root = repository();
    let text = "---\ntype: Task\nlayer: L9\nid: T-invalid\n---\nkeep\n";
    write(root.path(), "src/invalid.md", text);
    commit(root.path());
    let catalog = Catalog::load(root.path()).unwrap();
    assert!(catalog.protected_paths.contains(&"src/invalid.md".into()));
    assert!(validate_protected_paths(&["src/**".into()], &catalog.protected_paths).is_err());
    assert!(catalog.diagnostics.iter().any(|diagnostic|
        diagnostic.code == "invalid_layer" && diagnostic.path == "src/invalid.md"));
    assert_eq!(fs::read_to_string(root.path().join("src/invalid.md")).unwrap(), text);
}
```

把现有 `rejects_ambiguous_external_okf_layers_with_path_context` 拆成两类：合法 OKF、非法 layer（L9、数组）→加载成功、有诊断、受保护；缺 type / YAML 破损→保留带路径错误。不能把解析不了的文档无声当作快层。

- [ ] **Step 5: 验证红。** `cargo test -p towermd-core --test catalog --locked invalid_layer_is_diagnosed_and_protected_instead_of_defaulting_fast`，预期原 Catalog 拒绝加载，或新诊断字段尚未定义。

- [ ] **Step 6: 统一 Catalog 判断。** 删除独立的 `document.get("layer")...Layer::parse` 分支、移除不再使用的 `Layer` import。新增下列类型/字段/局部初始化；唯一手工 Catalog 构造器 `tests/tasks.rs::catalog` 增加 `diagnostics: Vec::new()`。

```rust
#[derive(Debug, Clone, serde::Serialize)]
pub struct CatalogDiagnostic {
    pub code: &'static str,
    pub path: String,
    pub message: String,
}
```

```rust
// Catalog 字段
pub diagnostics: Vec<CatalogDiagnostic>,
```

```rust
// load 中与 protected_paths 同时初始化；Self 返回同名字段
let mut diagnostics = Vec::new();
```

```rust
let layer = types::layer_of(&document);
if layer.is_protected() {
    protected_paths.push(entry.path.clone());
}
if layer == types::LayerResolution::Invalid {
    diagnostics.push(CatalogDiagnostic {
        code: "invalid_layer", path: entry.path.clone(),
        message: "非法 layer；按慢层保护，不采用 type 默认层".into(),
    });
}
```

诊断字段与 `protected_paths` 同源；Catalog 允许读取不等于允许 Agent 写入。Task 路径守卫继续通过 `catalog.protected_paths` 执行，不另写 layer 逻辑。诊断的响应暴露在 Task 6 与 Canvas 诊断统一处理，避免新增 UI 功能。

- [ ] **Step 7: 跑绿并提交。**

```sh
cargo test -p towermd-core --locked types::tests
cargo test -p towermd-core --test catalog --locked
cargo test -p towermd-core --test tasks --locked protected_slow
cargo fmt --all -- --check
git add crates/towermd-core/src/types.rs crates/towermd-core/src/catalog.rs crates/towermd-core/tests/catalog.rs crates/towermd-core/tests/tasks.rs
git commit -m "fix: protect and diagnose invalid object layers consistently"
```

## Task 3: R12 — 扩展小数正常投影，非法几何只影响本节点

**Files:**
- Modify: `crates/towermd-core/src/canvas.rs`（Audit 数字 visitor）
- Test: `crates/towermd-core/tests/canvas.rs`

- [ ] **Step 1: 写红测。** 使用已有 `fixture`，同时覆盖根扩展、节点扩展、边扩展及 CRLF 原文。

```rust
#[test]
fn fractional_unknown_fields_do_not_disable_projection() {
    let mut value = fixture();
    value["future"] = json!({"ratio":0.5});
    value["nodes"][0]["future"] = json!({"ratio":0.5});
    value["edges"][0]["future"] = json!({"ratio":0.5});
    let source = serde_json::to_string_pretty(&value).unwrap().replace('\n', "\r\n");
    let document = CanvasDocument::decode(&source).unwrap();
    assert!(document.raw().is_some());
    assert_eq!(document.nodes().len(), 4);
    assert_eq!(document.edges().len(), 2);
    assert!(document.nodes().iter().all(|entry| !entry.opaque));
    assert!(document.diagnostics().is_empty());
    assert_eq!(document.encode().as_bytes(), source.as_bytes());
}

#[test]
fn fractional_geometry_only_makes_its_node_opaque_with_unknown_decimals() {
    let mut value = fixture();
    value["future"] = json!(0.5);
    value["nodes"][1]["x"] = json!(0.5);
    let source = serde_json::to_string(&value).unwrap();
    let document = CanvasDocument::decode(&source).unwrap();
    assert!(document.raw().is_some());
    assert_eq!(document.nodes().iter().map(|entry| entry.opaque).collect::<Vec<_>>(),
        [false, true, false, false]);
    assert!(document.diagnostics().iter().any(|diagnostic|
        diagnostic.code == "invalid_entry" && diagnostic.collection == Some("nodes")
            && diagnostic.index == Some(1)));
    assert_eq!(document.encode(), source);
}
```

- [ ] **Step 2: 验证红。**

```sh
cargo test -p towermd-core --test canvas --locked fractional_unknown_fields
cargo test -p towermd-core --test canvas --locked fractional_geometry_only
```

预期原 Audit 在扩展 `0.5` 上报告 `unrepresentable_number`，整份 raw 丢失，测试失败。

- [ ] **Step 3: 最小数字 visitor。** 替换三个 visitor：未知键的有限 JSON 数字不触发 JS 安全整数限制；删除变成孤儿的 `Audit::is_geometry`。几何合法性继续只由 `validate_required` 的 `.as_i64()`、JS 安全范围和正尺寸判断。重复属性仍按现有保守策略禁用整份投影，不在本轮改它。

```rust
fn visit_f64<Error: serde::de::Error>(self, _value: f64) -> Result<(), Error> { Ok(()) }
fn visit_i64<Error: serde::de::Error>(self, _value: i64) -> Result<(), Error> { Ok(()) }
fn visit_u64<Error: serde::de::Error>(self, _value: u64) -> Result<(), Error> { Ok(()) }
```

未知数字永远通过 `encode()` 返回 original，不把 `Value` 重新序列化写回。serde_json 无法建立 `Value` 的 `1e999` 等仍保留原文并给 `unrepresentable_number`，不加 arbitrary_precision 依赖特性、不静默归零。

更新 `large_unknown_numbers_round_trip_verbatim`：有限大整数/高精度小数仍逐字节不变，但不再要求误导性的全画布精度诊断；保留 `1e999` 的不透明诊断。完整替换测试为：

```rust
#[test]
fn large_unknown_numbers_round_trip_verbatim() {
    for token in ["184467440737095516160001", "0.12345678901234567890123456789",
                  "1.234567890123456789e80", "1e999"] {
        let source = format!("{{\"nodes\":[],\"extension\":{token}}}");
        let document = CanvasDocument::decode(&source).unwrap();
        assert_eq!(document.encode(), source);
        if token == "1e999" {
            assert!(document.raw().is_none());
            assert!(document.diagnostics().iter()
                .any(|diagnostic| diagnostic.code == "unrepresentable_number"));
        } else {
            assert!(document.raw().is_some());
            assert!(document.diagnostics().is_empty());
        }
    }
}
```

- [ ] **Step 4: 跑绿并提交。**

```sh
cargo test -p towermd-core --test canvas --locked
cargo test -p towermd-core --test canvas_view --locked
cargo fmt --all -- --check
git add crates/towermd-core/src/canvas.rs crates/towermd-core/tests/canvas.rs
git commit -m "fix: limit canvas integer checks to node geometry"
```

全部原有几何、重复属性、非法根、未知字段往返测试必须保持通过。

## Task 4: R13.1 — 找不到运行时只列本机活跃运行

**Files:**
- Modify: `crates/towermd-core/src/tasks.rs`（当前 `Projection::local_runs`；批次 2 后为同职责的常驻投影方法）
- Test: `crates/towermd-core/tests/tasks.rs`

- [ ] **Step 1: 写行为红测。** 使用现有 `catalog`、`add`、`claim`、`call`；`host` 的已释放 run 不应出现在 explicit run 错误的 JSON 列表。

```rust
#[test]
fn missing_run_diagnostic_lists_only_active_local_runs() {
    let temporary = tempfile::tempdir().unwrap();
    let mut hub = Hub::open(&temporary.path().join("hub")).unwrap();
    let released_catalog = catalog(&temporary.path().join("released"), "t/T-1");
    add(&mut hub, &released_catalog, task("T-1", "A"), 1);
    let released = claim(&mut hub, &released_catalog, "T-1", 2);
    call(&mut hub, &released_catalog, TaskCommand::Release, 3).unwrap();
    let active_catalog = catalog(&temporary.path().join("active"), "t/T-2");
    add(&mut hub, &active_catalog, task("T-2", "B"), 4);
    let active = claim(&mut hub, &active_catalog, "T-2", 5);
    let error = execute(&mut hub, &active_catalog, "host", TaskCommand::Show { id: None },
                        Some("missing"), 6).unwrap_err();
    let list: Value = serde_json::from_str(error.split("local runs: ").nth(1).unwrap()).unwrap();
    assert_eq!(list.as_array().unwrap().len(), 1);
    assert_eq!(list[0]["id"], active["id"]);
    assert!(!list.as_array().unwrap().iter().any(|run| run["id"] == released["id"]));
}
```

- [ ] **Step 2: 跑红。** `cargo test -p towermd-core --test tasks --locked missing_run_diagnostic_lists_only_active_local_runs`，预期列表包含两条运行。

- [ ] **Step 3: 最小实现。** 保留原 JSON 字段，将筛选谓词替换为：

```rust
.filter(|run| run.host == host && run.state == "running")
```

活跃状态来自命令开始前现有过期处理；不因报错提示额外写事件、强制续租或删除历史 Run。

- [ ] **Step 4: 跑绿并提交。**

```sh
cargo test -p towermd-core --test tasks --locked missing_run_diagnostic
cargo test -p towermd-core --test tasks --locked
git add crates/towermd-core/src/tasks.rs crates/towermd-core/tests/tasks.rs
git commit -m "fix: list only active local runs in binding diagnostics"
```

## Task 5: R13.2 — G0 通过后先记录 ready，再取得运行

**Files:**
- Modify: `crates/towermd-core/src/tasks.rs`（Claim 分支）
- Test: `crates/towermd-core/tests/tasks.rs`

依赖保护范围：不新增“标记合并”命令，不把 Agent 自报当合并证据。fixture 注入事件只用于复现已满足依赖的持久投影输入，不是产品新入口。批次 2 的增量投影必须能消费这些经过 Hub 的事件；测试不能调用私有 replay 或直接篡改内存 map。

- [ ] **Step 1: 写红测。** 为已有 fixtures 加一条最小事件注入助手；这是当前 `NewEvent`/Hub 的公开测试输入。批次 2 已合入时，使用那批协议测试的公开 append 接口映射最后一行，仍保留同样的信封与 data。

```rust
fn append_task_fixture(hub: &mut Hub, task: &Value, now: u64) {
    hub.append(NewEvent {
        eid: uuid::Uuid::now_v7().to_string(), host: "host".into(),
        actor: "process:test-fixture".into(), kind: "task.state".into(),
        layer: "L4".into(), obj: task["id"].as_str().unwrap().into(),
        data: json!({"task":task}), fence: None,
    }, None, now).unwrap();
}

#[test]
fn proposed_task_records_ready_before_claim_when_g0_passes() {
    let temporary = tempfile::tempdir().unwrap();
    let mut hub = Hub::open(&temporary.path().join("hub")).unwrap();
    let catalog = catalog(temporary.path(), "t/T-2");
    let mut dependency = add(&mut hub, &catalog, task("T-1", "A"), 1);
    let mut spec = task("T-2", "B");
    spec.depends_on.push("T-1".into());
    assert_eq!(add(&mut hub, &catalog, spec, 2)["state"], "proposed");
    dependency["state"] = "merged".into();
    append_task_fixture(&mut hub, &dependency, 3);
    let before = hub.last_seq();
    let run = claim(&mut hub, &catalog, "T-2", 4);
    let events = hub.events(before);
    let business: Vec<_> = events.iter().filter(|event|
        event.obj == "T-2" && matches!(event.kind.as_str(), "task.state" | "task.claimed"))
        .collect();
    assert_eq!(business.len(), 2);
    assert_eq!(business[0].kind, "task.state");
    assert_eq!(business[0].data["task"]["state"], "ready");
    assert_eq!(business[1].kind, "task.claimed");
    assert_eq!(business[1].data["task"]["state"], "running");
    assert!(business[0].seq < business[1].seq);
    assert_eq!(run["state"], "running");
    drop(hub);
    let mut reopened = Hub::open(&temporary.path().join("hub")).unwrap();
    let status = call(&mut reopened, &catalog, TaskCommand::Status, 5).unwrap();
    assert!(status["tasks"].as_array().unwrap().iter()
        .any(|task| task["id"] == "T-2" && task["state"] == "running"));
}
```

对批次 2 的事件订阅过滤只匹配目标业务事件，允许其间存在协议产生的 `lease.*`；不假定 `last_seq` 只加 2、不假定 `task.claimed` 无 epoch/L4。

- [ ] **Step 2: 跑红。** `cargo test -p towermd-core --test tasks --locked proposed_task_records_ready_before_claim_when_g0_passes`，预期只有 claimed、没有 ready 事件。

- [ ] **Step 3: 最小实现。** 保留输入校验、依赖合并、权威验收/决策/路径校验、目标分支、运行互斥和模块租约检查。以上 G0 条件全部通过后、取得租约前插入下列状态转换，再只允许 ready 领取。

```rust
if task.state == "proposed" {
    task.state = "ready".into();
    append_change(hub, host, "task.state", &task, None, None, now_ms)?;
}
if task.state != "ready" {
    return Err(format!("task {id} is not ready"));
}
task.state = "running".into();
```

此处 `append_change` 是当前主基线已有函数。若批次 2 已去掉 `LeaseChange` 参数，调用其等价无租约任务状态追加函数；不能为了本修复恢复旧 Hub 耦合。常驻投影必须在 ready 追加成功后立即更新，后续 claimed 追加失败时保留 ready，不能留 proposed 或假造 running；沿用批次 2 “租约先取、claimed 失败即释放”保证。Task card 与 Run 构造保持现有逻辑。

- [ ] **Step 4: 增加两个失败路径断言。** 在现有 `dependencies_stay_proposed_until_evidence_of_merge_exists` 中，保存领取前水位并检查失败后没有目标 ready/claimed 事件；在已经 ready 的正常领取测试中，断言不重复产生 ready 事件。断言代码分别为：

```rust
assert!(!hub.events(before).iter().any(|event| event.obj == "T-2"
    && matches!(event.kind.as_str(), "task.state" | "task.claimed")));
```

```rust
assert!(!hub.events(before).iter().any(|event| event.obj == "T-1"
    && event.kind == "task.state" && event.data["task"]["state"] == "ready"));
```

每次保存 `before` 紧邻对应 `call/claim` 前，不能把 Add 本身事件包含进去。再覆盖 G0 因显式 Decision 缺失/保护路径/错误分支失败：沿用现有对应 fixture，把任务状态输入设 proposed，断言无 ready、无运行、无新模块租约。并发条件下曾通过 G0 后租约竞争失败可保留 ready；不能误报已领取。

- [ ] **Step 5: 跑绿并提交。**

```sh
cargo test -p towermd-core --test tasks --locked proposed_task_records_ready
cargo test -p towermd-core --test tasks --locked dependencies_stay_proposed
cargo test -p towermd-core --test tasks --locked
cargo fmt --all -- --check
git add crates/towermd-core/src/tasks.rs crates/towermd-core/tests/tasks.rs
git commit -m "fix: record ready transition before claiming proposed tasks"
```

## Task 6: R13.3 — 未加载接口给未解析诊断，不冒称断链

**Files:**
- Modify: `crates/towermd-core/src/canvas_view.rs`
- Test: `crates/towermd-core/tests/canvas_view.rs`

不在本任务实现 `interfaces.yaml` 加载器；也不因该 YAML 文件存在就谎称“接口已加载”。现状只有 Markdown Interface 对象可从 Catalog 查到：已找到正确类型仍 bound，找到其他类型仍 mismatch；没有该对象且接口清单未加载时为 `unresolved`，而非 `dangling`。

- [ ] **Step 1: 写红测。** 使用已有 `fixture`、`node`、`write`、`commit`。

```rust
#[test]
fn interface_binding_is_unresolved_when_interface_catalog_is_not_loaded() {
    let root = fixture();
    let canvas = json!({
        "nodes":[node("a", "docs/tower/arch/A.md", Some("A")),
                 node("b", "docs/tower/arch/B.md", Some("B"))],
        "edges":[{"id":"edge", "fromNode":"a", "toNode":"b",
                  "towermd":{"obj":"IF-1"}}],
    }).to_string();
    write(root.path(), "docs/tower/tower.canvas", &canvas);
    write(root.path(), "docs/tower/arch/interfaces.yaml",
        "- type: Interface\n  id: IF-1\n");
    commit(root.path());
    let view = CanvasView::load(&Catalog::load(root.path()).unwrap()).unwrap();
    assert_eq!(view["edges"][0]["status"], "unresolved");
    assert!(view["diagnostics"].as_array().unwrap().iter().any(|diagnostic|
        diagnostic["code"] == "interface_unresolved"
            && diagnostic["collection"] == "edges" && diagnostic["index"] == 0));
    assert_eq!(view["canvas_text"], canvas);
}
```

再在同一 fixture 添加 `type: Interface / id: IF-1` 的 Markdown，提交后仍要求 bound；绑定 A 模块的边仍 mismatch，未绑定的注释边仍 annotation，非法边仍 opaque。这些已有行为不改变。

- [ ] **Step 2: 跑红。** `cargo test -p towermd-core --test canvas_view --locked interface_binding_is_unresolved_when_interface_catalog_is_not_loaded`，预期实际 dangling。

- [ ] **Step 3: 最小实现与诊断汇合。** 开始就把 Catalog 的诊断放进一个 Vec；画布解析的诊断在结尾 extend 进去，不能覆盖 layer 诊断。已有 invalid/unsafe 提前返回分支同样 append 错误诊断，保持原错误状态。

```rust
let mut diagnostics: Vec<Value> = catalog.diagnostics.iter()
    .map(|diagnostic| json!(diagnostic)).collect();
```

把边的 `.map` closure 改成可变捕获 `diagnostics`，仅替换 `None => "dangling"`：

```rust
None => {
    diagnostics.push(json!({
        "code":"interface_unresolved", "collection":"edges", "index":entry.index,
        "message":"接口未解析：当前未加载 interfaces.yaml，不能判定接口是否存在",
        "object":object,
    }));
    "unresolved"
}
```

结尾替换原诊断赋值：

```rust
diagnostics.extend(canvas.diagnostics().iter().map(|diagnostic| json!(diagnostic)));
view["diagnostics"] = json!(diagnostics);
```

初始化 view 的 `diagnostics` 为 `json!(diagnostics)`。对于 `unsafe_canvas`/`invalid_canvas` 两个错误分支使用下列单元分别写入现有 code/reason，不把 Catalog 诊断冲掉：

```rust
diagnostics.push(json!({"code":"unsafe_canvas", "reason":error}));
view["diagnostics"] = json!(diagnostics);
```

```rust
diagnostics.push(json!({"code":"invalid_canvas", "reason":error}));
view["diagnostics"] = json!(diagnostics);
```

- [ ] **Step 4: 跑绿并提交。**

```sh
cargo test -p towermd-core --test canvas_view --locked
cargo test -p towermd-core --test catalog --locked
cargo fmt --all -- --check
git add crates/towermd-core/src/canvas_view.rs crates/towermd-core/tests/canvas_view.rs
git commit -m "fix: distinguish unresolved interface bindings from dangling edges"
```

## Task 7: R15 — 文档、未发布日志与实际提交一一对应

**Files:**
- Modify: `README.md`
- Modify: `CHANGELOG.md`
- Modify: `CHANGELOG.zh-CN.md`
- Modify: `tasks/todo.md`

这是低风险文本校对，不新建永久测试文件。先跑事实核对使错误断言显现，再改文本并重复核对；不能用字符串检查替代产品验收。

- [ ] **Step 1: 冻结当前证据。** 在已集成本批实现的工作树运行：

```sh
git status --short
git log -15 --format='%H %s'
git worktree list --porcelain
git branch --no-merged main
git stash list
```

对 R10–R13 的提交逐个 `git show --stat <从上一命令取得的实际完整提交号>`，再按 Task 1–6 的精确测试命令复核。记录真实 SHA、测试结果和工作树；若实现未提交明确写“未提交”。不能用本文件的规划基线 `a781b29` 冒充整改提交。

- [ ] **Step 2: 显示当前文档事实冲突。**

```sh
rg -n '非交互调用使用默认目录|init <其他目录>|尚不压缩|main.*一律|M0 尚无 App 确认|S5 始终未运行|未提交|待.*裁决|需人确认' README.md CHANGELOG.md CHANGELOG.zh-CN.md tasks/todo.md
```

预期命中旧文字；逐条核对当前对应批次的实际提交和验收，不机械删除所有命中。历史记录可以保留，但节首要明确其已被当前状态替代。

- [ ] **Step 3: 更新 README 当前能力。** R10–R13 本批通过后的最小文案可直接使用：

```markdown
同一 clone 的工作树共享 git-common-dir 下的 hub。已有绑定指向的 clone 仍存在、权威配置为同项目 ID 且 git 公共目录不同时，拒绝另一个 clone；旧 clone 删除、迁移或权威项目 ID 改变后可重绑，运行目录保留改绑记录。在途旧操作结束后才释放原项目锁并启用新绑定。

非法 layer 不回退到 type 默认层：目录读取返回诊断，该对象路径按慢层保护。导入画布的未知扩展数字保留原文字节；只有几何非法的节点成为不透明条目。尚未加载接口清单时，绑定接口的边报告“接口未解析”。

运行绑定报错只列本机活跃运行。待定任务的依赖和 G0 检查通过后，先记录转为就绪，再取得租约并领取；失败的 G0 检查不产生运行。
```

以下按**已完成批次**替换旧段，不一次写入全部未来能力：

| 对应批次 | 应删除/改写的当前断言 | 通过真实端到端验收后可写的内容 |
| --- | --- | --- |
| 1/R1–R2 | 非交互默认目录；init 也隐式续租；fetch 失败命令失败 | 首次非交互必须显式 docs-dir；init 独立于 daemon/续租；后台尽力 fetch、读缓存权威提交并给警告 |
| 2/R5–R6 | “本阶段尚不压缩”；心跳 eid 无限保留；租约附在业务追加 | ack 后压缩、常驻增量投影；独立租约协议事件、正确层与 epoch；不向用户暴露内部 trait 名称 |
| 3/R3–R4/R7/R14 | main 检出一律推迟；无 App 确认 HTTP；未知 trailer 一律拒绝 | 无远端使用私有快照引用；限次重做与 fencing；请求后仅 App 身份能确认恢复；已知 trailer 与发件箱恢复；业务事件触发去抖快照 |
| 4/R8–R9 | S5 始终不运行；只 stdout JSON；不产生草稿文件 | 异步 scan/wait；固定提交分离工作树、继承环境、逐模块初始证据；运行目录草稿与 GET 读取，批准前不写用户慢层 |

若该批未完成，README 继续明确它未实现；不能因为 D1–D6 已批准就写成已经可用。README 的 M0 完成状态只在全部 M0 条目真实验收后改变。

- [ ] **Step 4: 双语未发布区添加用户变化。** 本批每一条只写一次，并与已有过时条目合并；保留两份非空未发布区及相同版本序列。

```markdown
### Fixed

- Rebind stale local clone identities safely while preserving exclusion between live clones of the same project.
- Protect and diagnose objects with invalid layer values instead of silently treating them as fast-layer objects.
- Preserve Canvas projection with fractional extension values; invalid geometry affects only its node and original bytes remain unchanged.
- Show only active local runs in binding errors, record ready before claiming a proposed task, and distinguish unresolved interface bindings from dangling references.
```

```markdown
### 修复

- 安全重绑失效的本地 clone 身份，同时保持同项目活跃 clone 互斥。
- 非法 layer 的对象给出诊断并按慢层保护，不再静默视为快层。
- 画布扩展中的小数不再使整份投影失效；非法几何仅影响对应节点，原文字节保持不变。
- 运行绑定报错只列活跃的本机运行；待定任务先转就绪再领取；接口未加载时区分“未解析”与断链。
```

其他批次的行为按上表证据逐批添加双语对应条目；删除其已失效旧说法。不能保留一条说“main 检出一律推迟”又增加一条说“私有 ref 已可用”。未发布条目不能写成已发布版本。

- [ ] **Step 5: 清理 todo 的误导状态并记录 SHA。** 顶部已批准整改清单是唯一当前状态；下面“对照规格 v0.5 评审后”的未裁决/未提交段改为历史并引用顶部。旧 S5 沙箱、旧 77/163/320 项验收和旧 HEAD 等原始事实保留，但明确是当时状态、已被 D0/R16 替代，不能继续称主仓库未提交。文案为：

```markdown
本节保留 2026-10-17 整改裁决前的历史记录；当前裁决、实现状态与提交证据以上方“D0–D6 已批准”节为准。历史未提交描述不代表当前主仓库状态。
```

在顶部“批次 5”下记录每个独立实现提交的真实完整 SHA、对应测试结果；只在已全部合入和本批完整验证通过时勾选批次 5。文档提交不能自填尚不存在的自身 SHA：先提交本次已验证文档，再追加一条记录实际 SHA 的任务记录提交；该收尾记录提交的 SHA 用最终交付消息报告即可，不递归自引用。

- [ ] **Step 6: 核对用户点名行为的实际证据。** 目前不发版。对任何“已经发布/进入产物”的历史说法，只有冻结 release tag 上 `git show <实际tag>:<精确源码路径>`、对应验收结果和版本日志同时支持才保留。没有 release tag 或未纳入产物则明确“未发布”；提交过不等于发布过。枚举所有 worktree、未合入分支和 stash 只用于范围核对，不自动清理、合并或提交它们。

- [ ] **Step 7: 最终验证。**

```sh
cargo test --workspace --locked
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
git diff --check
```

预期 workspace 全部通过、0 跳过；记录**实际数量**，不能写死规划时的 320。运行可能需要本机环回端口权限时依现有已授权验证方式执行；若阻断明确记录，不把部分通过称全量通过。核对 README 例子与 `towermd --help`/子命令帮助一致；R15 文本复核与独立规格/质量审查均通过后提交。

```sh
git add README.md CHANGELOG.md CHANGELOG.zh-CN.md tasks/todo.md
git commit -m "docs: align M0 remediation status with verified behavior"
git rev-parse HEAD
```

将命令返回的真实文档提交号追加到顶部回顾，再只提交 `tasks/todo.md`：

```sh
git add tasks/todo.md
git commit -m "docs: record verified M0 documentation checkpoint"
git rev-parse HEAD
```

只提交本任务范围和已验证内容，本轮不推送。

## 自审结果（计划编写时）

- 覆盖：R10→Task 1（持久/内存锁、删除、同 ID 拒绝、不同权威 ID、cached origin/main、记录）；R11→Task 2（Valid/Missing/Invalid、目录外守卫、诊断同源）；R12→Task 3（未知 0.5、逐节点几何、逐字节保留）；R13 三项→Task 4/5/6；R15→Task 7（README、双语未发布、历史误导、完整 SHA、实际发布证据）。
- 类型：`LayerResolution::Missing(Option<Layer>)` 明确已知 type 默认层和未知 type；所有调用者迁移；Catalog 夹具补新字段；CanvasView 汇合 Catalog 与 Canvas 诊断。未新增相互矛盾的 layer 推断。
- 并发：R10 退役同时考虑 Project 与 Hub 引用，旧在途操作不解锁；新注册仍取得原项目文件锁，继续一处定序。
- 跨批次：Task 5 使用公开业务事件，不访问私有 replay，不规定旧 append/lease 原子接口、不固定旧 task.claimed 层或 seq 增量；批次 2 的常驻增量投影仍为基准。
- 最小范围：不引依赖、不新增接口解析器、不实现其他批次、不动 App/批准边界、不发布。未知数值保留 original，不能用 Value 的数值序列化替代原文。
- 文档状态：此自审只确认计划覆盖与内部一致性；未执行红绿测试、未实现、未提交。执行者必须补实际审查与验证证据。
