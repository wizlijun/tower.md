---
type: Plan
title: M0 整改批次 1：离线读取、续租与纯初始化
description: 仅实施 R1、R2：Catalog 读取已缓存的权威 main，daemon 独立限频刷新，init 脱离 daemon 并尽力预检；保留快照和恢复的同步 fresh-fetch 与 fencing。
tags: [towermd, plan, M0, remediation, offline, init, TDD]
generated: { by: codex/gpt-6, at: 2026-10-17T00:00:00Z }
sources:
  - id: remediation
    resource: docs/superpowers/plans/2026-10-17-m0-remediation.md
    title: 已批准 D0–D6，整改 R1、R2
  - id: spec
    resource: docs/specs/2026-10-16-spec-towermd-layered-agent-canvas-mvp.md
    title: 规格 FR-83、FR-91、FR-98、FR-105、M0 验收
  - id: baseline
    resource: docs/superpowers/plans/2026-10-17-m0-fix-0-baseline-spec.md
    title: D0 基线与 R16 规格同步
status: draft
---

# M0 整改批次 1：离线读取、续租与纯初始化 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 已有缓存权威 main 的项目断网后仍能立即读取与续租；初始化只生成待人检查的文件，不依赖 daemon、租约或可用网络。

**Architecture:** 从 `Catalog::load` 去掉网络同步，将 origin/main 刷新交给 daemon 的独立、每公共 Git 目录一份的后台注册表。注册表与 hub 的 `Project` 生命周期分开，只保存可删除的刷新观测信息；Git tracking ref 与对象库仍是读取来源。快照底层现有 fresh-fetch/fencing 不变，恢复原先隐含在 Catalog 中的 fresh-fetch 改成显式调用。

**Tech Stack:** 现有 Rust、tokio、axum、reqwest、serde、tempfile、Git；不增加依赖、Cargo feature、运行开关或外部服务。

---

## 已冻结的范围与成功条件

D0 四个 checkpoint 为 `be0dff1`、`9f9eee6`、`fdba1fb`、`d68b961`；批次0已验证并提交，实施基线 HEAD 为 `df5a8b5`。实施工作树为 `/private/tmp/towermd-m0-remediation`，独立 target 为 `/private/tmp/towermd-m0-fixes-target`。此文只是计划，不代表已实施或已提交。用户已批准 D0–D6 推荐方案，不再询问安装、目录或执行方式；传入 `--docs-dir` 就是本次目录选择。

Root 已裁决 FR-83 的缺失权威边界：

1. 配置有 `origin` 时，仅从直接引用 `refs/remotes/origin/main` 读取；无 `origin` 时从直接引用 `refs/heads/main` 读取。移除 origin 后的残留 tracking ref 不改变本地权威来源。
2. 有 origin 但首次没有 cached origin/main，或权威提交没有 `.towermd/config.yaml` 时，`status` 返回退出码 0 的 `authority_unavailable` JSON，明确须先完成初始化提交/首次成功 fetch；不能将当前任务分支、HEAD、工作树或本地 main 升级为远端项目的权威。
3. 此状态下 `task show` 与写操作拒绝；因为没有可信慢层，不能计算任务卡或放行。正常已有 cached main 的断网项目读取与续租不受影响。物理输入缺失与网络离线不能混为一谈。
4. tracking/local main 是符号引用、配置无效、blob 损坏等继续报错，不伪装成正常未初始化。
5. 不引入 provisional 状态、永久 Catalog 缓存或新的信任记录。

`main.fetched_at` 定义为**本次 daemon 生命周期中最近一次成功后台刷新 cached origin/main 的 Unix 毫秒**，未知为 `null`；不能用提交时间、ref 文件 mtime、init 时间或当前时间冒充。`main.last_fetch_error` 是最近一次失败的完整诊断，成功后清空；失败保留既有成功时间。重启可丢弃全部这些元数据，保留 Git 缓存并重新安排刷新；这就是重建路径。无 origin 时两字段均为 `null`，`main.reference` 为本地 main。

每项目 60 秒限制指**后台 fetch 的开始时刻**，使用单调 `Instant`；同一 common-dir 的所有工作树共享 single-flight。失败也消耗本轮额度；在途超过 60 秒仍不重入。写 main 前的同步 fresh-fetch 是硬门禁，不受后台限频约束。网络不得持有 hub mutex，不能让维护/续租被阻塞。

不修改 R3 私有快照、R4 App 确认、R5/R6 hub 存储与协议、R7 重试、S5、M1 UI；不顺手修理已知并发首次启动的 FD 泄漏。旧诊断和重现证据保持原状。

## 文件地图

| 文件 | 责任 |
| --- | --- |
| `crates/towermd-core/src/catalog.rs` | 无网络读取、缺失权威的可选加载、引用安全检查 |
| `crates/towermd-core/src/repo.rs` | 复用已有超时/进程组回收，增加内部原始 Output 返回路径，保留既有字符串 API |
| `crates/towermd-core/src/snapshot_git.rs` | 复用 Session 隔离选项；只开放 crate 内的网络 Session 构造，不改变写入逻辑 |
| `crates/towermd-core/src/origin.rs`（新建） | 受保护的 tracking-ref 刷新、dry-run 输出分类 |
| `crates/towermd-core/src/lib.rs` | 导出 origin 给 daemon；不导出 Session |
| `crates/towermd-core/src/recovery.rs` | 恢复前显式 fresh-fetch，保留原双 fetch 锁边界 |
| `crates/towermd/src/main_refresh.rs`（新建） | daemon 的派生刷新注册表、60 秒与 single-flight、关机等待 |
| `crates/towermd/src/daemon.rs` | 所有读取入口调度刷新，status 元数据，私有可信时钟注入用于时间回归 |
| `crates/towermd/src/main.rs` | init 不续租；非交互缺目录零写拒绝；挂接 main_refresh 模块 |
| `crates/towermd-core/src/init.rs` | 尽力预检、真实技能提示 |
| `crates/towermd-core/templates/agents.md` | 只写稳定规则 |
| `crates/towermd-core/tests/catalog.rs` | 缓存、权威来源、符号引用、损坏输入 |
| `crates/towermd-core/tests/init_push.rs` | 离线/认证/无本地 main/保护策略、零真实推送、状态不变 |
| `crates/towermd/tests/daemon_cli.rs` | 真实 CLI/daemon 的离线读取、续租与并发 fetch |
| `crates/towermd/tests/init_cli.rs` | 非交互/重入/daemon 无关/提示 |
| `README.md`、`CHANGELOG.md`、`CHANGELOG.zh-CN.md`、`tasks/todo.md` | 只记录本批可观察行为、验证与提交号 |

实施保持显式文件暂存。每项红测必须先看到预期行为失败；编译失败只算准备阶段，不能作为最终红测证据。下面的代码块是实际待写内容；在现有文件中按列出的函数锚点替换，不按可能漂移的行号操作。

### Task 1: R1/R2 离线与初始化闭环实施

以下十一阶段是同一紧密耦合任务的内部步骤，按依赖顺序推进。阶段验证后可以保存精确中间提交；规格审查与质量审查仅在闭环全部完成后各做一次，不能每个微步骤重复双审。执行者在整改工作树内运行命令，`CARGO_TARGET_DIR=/private/tmp/towermd-m0-fixes-target` 由运行器显式传入。先做阶段1/2/7的核心读取与fresh-fetch，再做阶段8/9/10的init，最后做阶段3/4/5/6的daemon闭环及阶段11总验收；阶段编号保留用于覆盖映射。

#### 阶段 1: Catalog 只读取缓存，明确缺失权威

**Files:** Modify `crates/towermd-core/src/catalog.rs`；Test `crates/towermd-core/tests/catalog.rs`。

- [ ] **Step 1: 加入下列失败测试。** 复用该文件已有 `repository`、`git`、`write`、`commit`。

```rust
#[test]
fn offline_catalog_reads_cached_main_without_fetching() {
    let origin = repository();
    let clone = tempfile::tempdir().unwrap();
    git(clone.path(), &["clone", "-q", origin.path().to_str().unwrap(), "."]);
    let cached = git(clone.path(), &["rev-parse", "origin/main"]);
    git(clone.path(), &["remote", "set-url", "origin", "/nonexistent/towermd-offline"]);
    let index = fs::read(clone.path().join(".git/index")).unwrap();
    let catalog = Catalog::load(clone.path()).unwrap();
    assert_eq!(catalog.revision, cached);
    assert!(catalog.objects.contains_key("module"));
    assert_eq!(fs::read(clone.path().join(".git/index")).unwrap(), index);
    assert!(!clone.path().join(".git/FETCH_HEAD").exists());
}

#[test]
fn local_main_is_not_a_remote_project_bootstrap_authority() {
    let root = repository();
    git(root.path(), &["remote", "add", "origin", "/nonexistent/towermd-origin"]);
    assert!(Catalog::load_optional(root.path()).unwrap().is_none());
    assert!(Catalog::load(root.path()).unwrap_err().contains("权威 main"));
    git(root.path(), &["checkout", "-qb", "t/untrusted"]);
    write(root.path(), "docs/tower/arch/module.md", "---\ntype: Module\nid: evil\n---\n");
    commit(root.path());
    assert!(Catalog::load_optional(root.path()).unwrap().is_none());
}

#[test]
fn removing_origin_uses_local_main_even_with_leftover_tracking_ref() {
    let root = repository();
    let original = git(root.path(), &["rev-parse", "main"]);
    git(root.path(), &["update-ref", "refs/remotes/origin/main", &original]);
    write(root.path(), "docs/tower/arch/new.md", "---\ntype: Module\nid: new\n---\n");
    commit(root.path());
    let catalog = Catalog::load(root.path()).unwrap();
    assert!(catalog.objects.contains_key("new"));
    assert_ne!(catalog.revision, original);
}
```

- [ ] **Step 2: 运行红测。**

```bash
cargo test -p towermd-core --test catalog offline_catalog_reads_cached_main_without_fetching --locked -- --exact
cargo test -p towermd-core --test catalog local_main_is_not_a_remote_project_bootstrap_authority --locked -- --exact
```

第一条应因不存在的 origin 导致同步 fetch 错误；第二条先实现可选 API 后仍须确认“错误采用本地 main”的断言会红。原有 `removing_origin` 测试已绿是权威来源的保留基线。

- [ ] **Step 3: 改加载 API。** 删除 `Duration` import。把现有 `load` 内容移为 `load_optional`，其中对象解析、保护路径、配置校验代码原样保留。`impl Catalog` 顶部变为：

```rust
pub fn load(dir: &Path) -> Result<Self, String> {
    Self::load_optional(dir)?.ok_or_else(|| {
        "权威 main 不可读；请先将 init 结果提交到 main，有 origin 时须首次成功 fetch origin/main".into()
    })
}

pub fn load_optional(dir: &Path) -> Result<Option<Self>, String> {
```

在 `load_optional` 中保留 root/common-dir/branch 计算；将 `let remotes` 至 `let revision` 的旧 fetch 分支完整替换为：

```rust
let reference = main_reference(&root)?;
let record = repo::git(&root, &[
    "for-each-ref", "--format=%(objectname)%00%(symref)", reference,
])?;
if record.is_empty() {
    return Ok(None);
}
let (_, symbolic) = record.split_once('\0').ok_or("无法解析权威 main 引用")?;
if !symbolic.is_empty() {
    return Err(format!("{reference} 是符号引用；拒绝将用户分支当作权威 main"));
}
let revision = repo::git(&root, &[
    "rev-parse", "--verify", &format!("{reference}^{{commit}}"),
])?;
```

将 config entry 的 `ok_or_else` 改为缺文件返回 `Ok(None)`，非普通文件明确拒绝，不允许 symlink 退化为未初始化：

```rust
let Some(entry) = entries.iter().find(|entry| entry.path == CONFIG_PATH) else {
    return Ok(None);
};
if !entry.is_file() {
    return Err(format!("权威 main 的 {CONFIG_PATH} 不是普通文件"));
}
```

函数尾部 `Ok(Self {` 的起始表达式改为 `Ok(Some(Self {`，对应末尾 `})` 改为 `}))`。字段不变，`tests/tasks.rs` 的 Catalog literal 无需变更。增加模块级函数：

```rust
pub fn main_reference(root: &Path) -> Result<&'static str, String> {
    let remotes = repo::git(root, &["remote"])?;
    Ok(if remotes.lines().any(|remote| remote == "origin") {
        "refs/remotes/origin/main"
    } else {
        "refs/heads/main"
    })
}
```

- [ ] **Step 4: 改旧 fetch 测试的责任。** 将 `fetches_latest_origin_main_without_moving_branch_or_index` 改名 `cached_origin_main_does_not_follow_remote_until_explicit_refresh`。remote 增加 `latest` 后第一次 Catalog 断言 `!objects.contains_key("latest")`；阶段 2 的 `refresh_origin_main` 后再加载才有 latest。删除“Catalog fetch 失败”的断言，改成刷新失败但 Catalog 的 revision 与上一成功 cached SHA 相等。`fetch_has_a_bounded_timeout` 移为 origin 刷新测试；Catalog 对 `sleep 45; git-upload-pack` 的读取须小于 2 秒成功，不能留 30 秒红测在读取路径。

- [ ] **Step 5: 绿测并提交该独立改动。**

```bash
cargo test -p towermd-core --test catalog --locked
git add crates/towermd-core/src/catalog.rs crates/towermd-core/tests/catalog.rs
git commit -m "fix: read authoritative catalog from cached git objects"
```

本 阶段 的旧 fetch 测试涉及 阶段 2 API 时，可与 阶段 2 合成一次提交；不可提交编译不通过的中间态。

#### 阶段 2: 复用安全 Git Session，提供独立刷新与原始协商结果

**Files:** Modify `repo.rs`、`snapshot_git.rs`、`lib.rs`；Create `origin.rs`；Test `catalog.rs`、`repo_timeout.rs`、`snapshot_git.rs`。

- [ ] **Step 1: 写刷新失败不破坏缓存、恶意 refmap 不移动 main 的红测。** 在 `tests/catalog.rs` 增加：

```rust
#[test]
fn explicit_refresh_only_moves_direct_cached_origin_main() {
    let origin = repository();
    let clone = tempfile::tempdir().unwrap();
    git(clone.path(), &["clone", "-q", origin.path().to_str().unwrap(), "."]);
    let local_main = git(clone.path(), &["rev-parse", "main"]);
    let index = fs::read(clone.path().join(".git/index")).unwrap();
    write(origin.path(), "docs/tower/arch/latest.md", "---\ntype: Module\nid: latest\n---\n");
    commit(origin.path());
    git(clone.path(), &["config", "remote.origin.fetch", "+refs/heads/*:refs/heads/*"]);
    let fresh = towermd_core::origin::refresh_origin_main(clone.path()).unwrap().unwrap();
    assert_eq!(fresh, git(origin.path(), &["rev-parse", "main"]));
    assert_eq!(git(clone.path(), &["rev-parse", "main"]), local_main);
    assert_eq!(fs::read(clone.path().join(".git/index")).unwrap(), index);
    assert!(Catalog::load(clone.path()).unwrap().objects.contains_key("latest"));
    git(clone.path(), &["remote", "set-url", "origin", "/nonexistent/towermd-refresh"]);
    assert!(towermd_core::origin::refresh_origin_main(clone.path()).is_err());
    assert_eq!(Catalog::load(clone.path()).unwrap().revision, fresh);
    assert!(git(clone.path(), &["for-each-ref", "refs/towermd/snapshot-fetch/"]).is_empty());
}
```

扩展原 `remote_tracking_symref_cannot_move_a_user_branch_during_fetch`：对 `refresh_origin_main` 也断言失败，且 `human-feature` SHA 不动。阶段 1 的 Catalog 原断言保留。

- [ ] **Step 2: 红测。**

```bash
cargo test -p towermd-core --test catalog explicit_refresh_only_moves_direct_cached_origin_main --locked -- --exact
```

准备 API 后把实现暂设为 `Ok(None)`，确认断言因未刷新而失败，再写下列实际实现；不把“函数不存在”当完整红测证据。

- [ ] **Step 3: 将 repo 现有进程执行分成 raw 与 decode 两层。** `git_timeout_env` 保留签名，body 改为：

```rust
decode_output(args, git_timeout_env_output(dir, args, timeout, environment)?)
```

原 body 移为 `pub(crate) fn git_timeout_env_output(dir: &Path, args: &[&str], timeout: Duration, environment: &[(&str, &std::ffi::OsStr)]) -> Result<Output, String>`；原最后 match 的 `Ok(output) => decode_output(args, output)` 改成 `Ok(output) => Ok(output)`，超时、terminate_git、管道读取与所有环境清理不变。`decode_output` 改为 `pub(crate)`。这让 stderr 与 `--porcelain` stdout 都可判定，不能丢弃 stdout 中的 `[remote rejected]`。

- [ ] **Step 4: Session 只做受控可见性与构造拆分。** `Session`、`run`、`fetch_main` 改为 `pub(crate)`；`temporary/options/date` 不公开。`Session::new` 创建 git-common-dir 下临时目录的原逻辑保留，将 `let hooks` 起至原构造返回的代码抽入 `fn with_temporary(root: &Path, temporary: tempfile::TempDir) -> Result<Self, String>`，`new` 创建 temporary 后调用它。增加：

```rust
pub(crate) fn for_network(root: &Path) -> Result<Self, String> {
    let temporary = tempfile::Builder::new()
        .prefix("towermd-origin-")
        .tempdir()
        .map_err(|error| error.to_string())?;
    Self::with_temporary(root, temporary)
}

pub(crate) fn run_output(&self, root: &Path, args: &[&str]) -> Result<std::process::Output, String> {
    let mut options: Vec<&str> = self.options.iter().map(String::as_str).collect();
    options.extend_from_slice(args);
    crate::repo::git_timeout_env_output(root, &options, TIMEOUT, &[
        ("GIT_OPTIONAL_LOCKS", OsStr::new("0")),
        ("GIT_LITERAL_PATHSPECS", OsStr::new("1")),
        ("LC_ALL", OsStr::new("C")),
    ])
}
```

`for_network` 不创建 hub、journal 或 `.git/towermd`。原快照 Session 的 git-common-dir 临时 index 和署名环境不变。不要借此提炼跨模块新 trait 或改进 snapshot writer。

- [ ] **Step 5: 写 `src/origin.rs`，并在 `lib.rs` 加 `pub mod origin;`。**

```rust
use std::path::Path;

use crate::catalog::main_reference;
use crate::repo;
use crate::snapshot_git::{fetch_main, Session};

pub fn refresh_origin_main(root: &Path) -> Result<Option<String>, String> {
    if main_reference(root)? != "refs/remotes/origin/main" {
        return Ok(None);
    }
    let reference = "refs/remotes/origin/main";
    let record = repo::git(root, &[
        "for-each-ref", "--format=%(objectname)%00%(symref)", reference,
    ])?;
    let old = if record.is_empty() {
        String::new()
    } else {
        let (oid, symbolic) = record.split_once('\0').ok_or("无法解析 origin/main 引用")?;
        if !symbolic.is_empty() {
            return Err("origin/main 是符号引用；拒绝刷新用户分支".into());
        }
        oid.to_string()
    };
    let session = Session::for_network(root)?;
    let fresh = fetch_main(&session, root)?;
    session.run(root, &[
        "update-ref", "--no-deref", reference, &fresh, &old,
    ], None)?;
    Ok(Some(fresh))
}

pub(crate) fn is_explicit_policy_rejection(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    if ["authentication failed", "could not read username", "could not read password",
        "permission denied (publickey)", "invalid username or password", "terminal prompts disabled"]
        .iter().any(|part| lower.contains(part))
    {
        return false;
    }
    ["[remote rejected]", "gh006:", "gh013:", "protected branch",
        "write access to repository not granted"]
        .iter().any(|part| lower.contains(part))
}

#[derive(Debug)]
pub(crate) enum PreflightError {
    Policy(String),
    Unavailable(String),
}

pub(crate) fn push_dry_run(root: &Path, sha: &str) -> Result<String, PreflightError> {
    let session = Session::for_network(root).map_err(PreflightError::Unavailable)?;
    let args = ["push", "--dry-run", "--porcelain", "--no-verify",
        "--no-follow-tags", "--no-signed", "--recurse-submodules=no",
        "origin", &format!("{sha}:refs/heads/main")];
    let output = session.run_output(root, &args).map_err(PreflightError::Unavailable)?;
    let diagnostic = format!("{}\n{}",
        String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    if output.status.success() {
        return Ok("origin/main 的 push --dry-run 已完成协商；未实际推送，不保证服务端写入策略、保护规则或 hooks 允许真实推送。".into());
    }
    if is_explicit_policy_rejection(&diagnostic) {
        return Err(PreflightError::Policy(format!("origin/main 明确拒绝推送策略；未生成初始化文件：{diagnostic}")));
    }
    Ok(format!("origin/main 的 dry-run 未能确认：{diagnostic}；快照写入时会再次检查"))
}
```

临时 fetch ref 沿用已验证的 `fetch_main` 创建/清理，不接收用户 `remote.origin.fetch`。目标 tracking ref 以 `--no-deref` CAS 更新；并发改动引起 CAS 失败只算本轮 fetch 错误，不能重试覆盖其他刷新结果，也不能移动用户本地分支。

`[remote rejected]` 是服务端明确拒绝；普通 `[rejected] (non-fast-forward)`、连接失败、HTTP 401/403、模糊 permission denied 不足以证明策略，均警告。认证特征优先于策略词。**不增加“错误码白名单即有推送权”的反向判断。**

- [ ] **Step 6: 测分类表与原超时/安全基线。** 在 `origin.rs` 加内部测试：

```rust
#[cfg(test)]
mod tests {
    use super::is_explicit_policy_rejection;

    #[test]
    fn only_explicit_server_policy_rejections_are_hard_errors() {
        for text in ["!\tsha:refs/heads/main\t[remote rejected] (protected branch)",
            "remote: error: GH006: Protected branch update failed",
            "remote: Write access to repository not granted."] {
            assert!(is_explicit_policy_rejection(text), "{text}");
        }
        for text in ["Authentication failed", "Permission denied (publickey)",
            "HTTP 403", "permission denied", "could not resolve host",
            "!\tsha:refs/heads/main\t[rejected] (non-fast-forward)",
            "Authentication failed: protected branch endpoint unavailable"] {
            assert!(!is_explicit_policy_rejection(text), "{text}");
        }
    }
}
```

```bash
cargo test -p towermd-core --test catalog --locked
cargo test -p towermd-core --test repo_timeout --locked
cargo test -p towermd-core --test snapshot_git --locked
cargo test -p towermd-core origin::tests --locked
git add crates/towermd-core/src/catalog.rs crates/towermd-core/src/repo.rs crates/towermd-core/src/snapshot_git.rs crates/towermd-core/src/origin.rs crates/towermd-core/src/lib.rs crates/towermd-core/tests/catalog.rs
git commit -m "fix: refresh cached main with isolated git negotiation"
```

#### 阶段 3: daemon 刷新注册表与后台限频

**Files:** Create `crates/towermd/src/main_refresh.rs`；Modify `main.rs`、`daemon.rs`。

- [ ] **Step 1: 先加入下面模块末尾的调度测试，运行红测。** 先写模块的数据结构及默认实现，把 `due` 暂时返回 `true`，使 59 秒/在途断言红；再写实际函数。测试不依赖真实网络。

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_attempts_are_rate_limited_and_inflight_never_overlaps() {
        let start = Instant::now();
        assert!(due(None, false, start));
        assert!(!due(Some(start), false, start + Duration::from_secs(59)));
        assert!(due(Some(start), false, start + Duration::from_secs(60)));
        assert!(!due(Some(start), true, start + Duration::from_secs(120)));
    }
}
```

```bash
cargo test -p towermd --bin towermd main_refresh::tests --locked
```

- [ ] **Step 2: 写完整刷新模块。** 原测试保留在下列源码之后。

```rust
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tokio::task::JoinHandle;
use towermd_core::{catalog, origin, repo};

#[derive(Clone, Debug, Serialize)]
pub(crate) struct MainStatus {
    pub reference: String,
    pub fetched_at: Option<u64>,
    pub last_fetch_error: Option<String>,
}

#[derive(Default)]
struct FetchState {
    last_started: Option<Instant>,
    in_flight: bool,
    fetched_at: Option<u64>,
    last_fetch_error: Option<String>,
    job: Option<JoinHandle<()>>,
}

struct Entry {
    root: Mutex<PathBuf>,
    state: Mutex<FetchState>,
}

#[derive(Default)]
pub(crate) struct MainRefresh {
    entries: Mutex<BTreeMap<PathBuf, Arc<Entry>>>,
}

fn due(last: Option<Instant>, in_flight: bool, now: Instant) -> bool {
    !in_flight && last.map_or(true, |last| now.saturating_duration_since(last) >= Duration::from_secs(60))
}

impl MainRefresh {
    pub(crate) fn observe(&self, directory: &Path) -> Result<MainStatus, String> {
        let root = repo::toplevel(directory)?;
        let common = repo::git(&root, &["rev-parse", "--path-format=absolute", "--git-common-dir"])?;
        let common = PathBuf::from(common).canonicalize().map_err(|error| error.to_string())?;
        let reference = catalog::main_reference(&root)?.to_string();
        if reference == "refs/heads/main" {
            return Ok(MainStatus { reference, fetched_at: None, last_fetch_error: None });
        }
        let entry = {
            let mut entries = self.entries.lock().map_err(|_| "main 刷新注册表锁损坏")?;
            entries.entry(common).or_insert_with(|| Arc::new(Entry {
                root: Mutex::new(root.clone()), state: Mutex::new(FetchState::default()),
            })).clone()
        };
        *entry.root.lock().map_err(|_| "main 刷新路径锁损坏")? = root;
        Self::schedule(&entry)?;
        let state = entry.state.lock().map_err(|_| "main 刷新状态锁损坏")?;
        Ok(MainStatus { reference, fetched_at: state.fetched_at, last_fetch_error: state.last_fetch_error.clone() })
    }

    fn schedule(entry: &Arc<Entry>) -> Result<(), String> {
        let root = entry.root.lock().map_err(|_| "main 刷新路径锁损坏")?.clone();
        let mut state = entry.state.lock().map_err(|_| "main 刷新状态锁损坏")?;
        let now = Instant::now();
        if !due(state.last_started, state.in_flight, now) {
            return Ok(());
        }
        state.last_started = Some(now);
        state.in_flight = true;
        let worker = entry.clone();
        state.job = Some(tokio::task::spawn_blocking(move || {
            let result = origin::refresh_origin_main(&root).and_then(|sha| {
                if sha.is_none() { return Ok(None); }
                let millis = SystemTime::now().duration_since(UNIX_EPOCH)
                    .map_err(|error| error.to_string())?.as_millis();
                Ok(Some(u64::try_from(millis).map_err(|_| "fetch 时间溢出")?))
            });
            if let Ok(mut state) = worker.state.lock() {
                state.in_flight = false;
                match result {
                    Ok(Some(at)) => { state.fetched_at = Some(at); state.last_fetch_error = None; }
                    Ok(None) => { state.fetched_at = None; state.last_fetch_error = None; }
                    Err(error) => {
                        eprintln!("towermd 警告：后台 fetch 失败（{}）：{error}", root.display());
                        state.last_fetch_error = Some(error);
                    }
                }
            }
        }));
        Ok(())
    }

    pub(crate) fn tick(&self) -> Result<(), String> {
        let entries: Vec<_> = self.entries.lock().map_err(|_| "main 刷新注册表锁损坏")?
            .values().cloned().collect();
        for entry in entries { Self::schedule(&entry)?; }
        Ok(())
    }

    pub(crate) async fn drain(&self) -> Result<(), String> {
        let jobs: Vec<_> = {
            let entries = self.entries.lock().map_err(|_| "main 刷新注册表锁损坏")?;
            let mut jobs = Vec::new();
            for entry in entries.values() {
                if let Some(job) = entry.state.lock().map_err(|_| "main 刷新状态锁损坏")?.job.take() {
                    jobs.push(job);
                }
            }
            jobs
        };
        for job in jobs { job.await.map_err(|error| error.to_string())?; }
        Ok(())
    }
}
```

不改 toolchain 或 MSRV；调度仅使用既有标准库 API。

- [ ] **Step 3: 挂入 daemon。** `main.rs` 增加 `mod main_refresh;`；`daemon.rs` import `crate::main_refresh::MainRefresh`。`Server` 增加 `main_refresh: MainRefresh`，唯一生产构造点设 `MainRefresh::default()`。`tick` 开头、hub 维护循环之前调用 `self.main_refresh.tick()?`；仅在 `! *self.shutdown.borrow()` 时调用。关机收到 shutdown 后先停止 ticker，再等待后台 fetch 与快照：

```rust
let _ = server.shutdown.send(true);
ticker.abort();
let fetched = server.main_refresh.drain().await;
let drained = server.drain_snapshots().await;
result.and(fetched).and(drained)
```

不再让 ticker 在 drain 时创建新后台任务。fresh-fetch 仍由原 snapshot job 自己控制，不并入此注册表。

- [ ] **Step 4: 绿测与精确提交。**

```bash
cargo test -p towermd --bin towermd main_refresh::tests --locked
git add crates/towermd/src/main_refresh.rs crates/towermd/src/main.rs crates/towermd/src/daemon.rs
git commit -m "fix: refresh project main in a single flight background job"
```

#### 阶段 4: 所有 daemon 读入口使用缓存并暴露刷新诊断

**Files:** Modify `crates/towermd/src/daemon.rs`；Test `crates/towermd/tests/daemon_cli.rs`。

- [ ] **Step 1: 写真实 CLI 红测。** 复用 `Fixture`，新增辅助函数与两条测试：

```rust
fn offline_cached_origin(fixture: &Fixture) {
    let main = fixture.git(&["rev-parse", "main"]);
    fixture.git(&["update-ref", "refs/remotes/origin/main", &main]);
    fixture.git(&["remote", "add", "origin", "/nonexistent/towermd-offline-daemon"]);
}

#[test]
fn offline_cached_main_keeps_status_show_and_implicit_renewal_available() {
    let fixture = Fixture::new();
    fixture.add("T-1", "A");
    let worktree = fixture.worktree("T-1");
    let run = fixture.json(&worktree, &["task", "claim", "T-1"]);
    offline_cached_origin(&fixture);
    let before = journal(&fixture)["events"].as_array().unwrap().len();
    let start = std::time::Instant::now();
    let status = fixture.json(&worktree, &["status"]);
    assert!(start.elapsed() < Duration::from_secs(2));
    assert_eq!(status["main"]["reference"], "refs/remotes/origin/main");
    let shown = fixture.json(&worktree, &["task", "show"]);
    assert_eq!(shown["id"], run["id"]);
    let after = journal(&fixture);
    assert!(after["events"].as_array().unwrap()[before..].iter()
        .any(|event| event["type"] == "lease.renewed" && event["obj"] == run["id"]));
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        let value = fixture.json(&worktree, &["status"]);
        if value["main"]["last_fetch_error"].is_string() {
            assert!(value["warnings"].as_array().unwrap().iter()
                .any(|warning| warning.as_str().unwrap().contains("fetch")));
            break;
        }
        assert!(std::time::Instant::now() < deadline, "fetch 失败诊断未出现：{value}");
        thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn remote_project_without_cached_authority_reports_status_but_refuses_task_reads() {
    let fixture = Fixture::new();
    fixture.git(&["remote", "add", "origin", "/nonexistent/towermd-first-fetch"]);
    let status = fixture.json(fixture.root.path(), &["status"]);
    assert_eq!(status["status"], "authority_unavailable");
    assert_eq!(status["main"]["reference"], "refs/remotes/origin/main");
    assert!(status["main"]["fetched_at"].is_null());
    let output = fixture.command(fixture.root.path(), &["task", "show", "T-1"]).output().unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("权威 main"));
    assert!(!fixture.root.path().join(".git/towermd/journal.json").exists());
}
```

第二条 Fixture 只生成/提交文件，不运行 status/add，确保测的是不创建 hub 的缺失权威路径。

- [ ] **Step 2: 红测命令。**

```bash
cargo test -p towermd --test daemon_cli offline_cached_main_keeps_status_show_and_implicit_renewal_available --locked -- --exact
cargo test -p towermd --test daemon_cli remote_project_without_cached_authority_reports_status_but_refuses_task_reads --locked -- --exact
```

预期第一条失败于没有 main 字段/旧同步 fetch，第二条失败于 `status` 被 Catalog 拒绝。

- [ ] **Step 3: 加两个 daemon 私有 helper，完整替换 execute。**

```rust
fn catalog(&self, directory: &Path) -> Result<Catalog, String> {
    self.main_refresh.observe(directory)?;
    Catalog::load(directory)
}

fn main_diagnostics(&self, directory: &Path, mut value: Value) -> Result<Value, String> {
    let main = self.main_refresh.observe(directory)?;
    if let Some(error) = &main.last_fetch_error {
        let warning = json!(format!("后台 fetch 失败：{error}；已有缓存时继续读取与续租"));
        let mut warnings = value.get("warnings").and_then(Value::as_array).cloned().unwrap_or_default();
        if !warnings.contains(&warning) { warnings.push(warning); }
        value["warnings"] = json!(warnings);
    }
    value["main"] = serde_json::to_value(main).map_err(|error| error.to_string())?;
    Ok(value)
}

fn execute(&self, request: Request) -> Result<Value, String> {
    self.main_refresh.observe(&request.directory)?;
    let catalog = match Catalog::load_optional(&request.directory)? {
        Some(catalog) => catalog,
        None if matches!(&request.command, TaskCommand::Status) => {
            return self.main_diagnostics(&request.directory, json!({
                "status": "authority_unavailable",
                "warnings": ["权威 main 不可读；请先将 init 结果提交到 main，有 origin 时须首次成功 fetch origin/main；未读取工作树或任务分支，也未续租"]
            }));
        }
        None => return Err("权威 main 不可读；须完成初始化提交及首次成功 fetch，不能据工作树或任务分支执行".into()),
    };
    let hub = self.hub(&catalog)?;
    let value = {
        let mut hub = hub.lock().map_err(|_| "项目 hub 锁损坏")?;
        snapshot_service::validate_authority(&hub, &catalog)?;
        tasks::execute(&mut hub, &catalog, &self.endpoint.host, request.command,
            request.run.as_deref(), now_ms()?)?
    };
    self.main_diagnostics(&request.directory, value)
}
```

普通 execute 仍对可信 hub 调用原 validate_authority；不把 R4 的“hub 为空但有快照”一并改了。

将 `coldstart`、`canvas`、`object`、`snapshot`、`rebuild_request` 中的 `Catalog::load(&request.directory)`/`Catalog::load(&query.directory)` 分别改为 `self.catalog(&request.directory)`、`self.catalog(&query.directory)`；`subscribe` 的 blocking closure 改为 `loader.catalog(&subscription.directory)`。后台快照仍用直接 Catalog 缓存加载；上层 tick 已负责刷新。

`finish_read` 原 `Ok(value)` 改为 `self.main_diagnostics(&catalog.root, value)`，保留显式 run 错误与原隐式续租 warnings 合并。`snapshot` 和 `rebuild_request` 原最终 response 用 `main_diagnostics` 包装。对象不存在的 404 保持不变，不为缺对象触碰 hub。所有读取只调度后台，不能 `await`、`join` 或同步 fetch。

- [ ] **Step 4: 验证元数据可删重建。** 补充 CLI 测试：先 local bare origin 成功刷新，轮询到 `fetched_at` 为整数且 error 为 null；将 URL 改为不存在的路径，在 main_refresh 内部测试中将 last_started 移到 60 秒以前再 observe，失败后 fetched_at 不变/error 非空；`MainRefresh::default()` 对同一 Git common-dir 的第一份响应 fetched_at 必须 null（成功 worker 有竞态时用被阻塞的 upload-pack），后续仍可 Catalog 读取原 SHA。此检查不落盘 metadata，不依赖重建唯一状态文件。

内部调度状态推进使用：

```rust
entry.state.lock().unwrap().last_started = Some(Instant::now() - Duration::from_secs(60));
```

该代码只能位于 `#[cfg(test)]` 模块，不添加生产配置项。

- [ ] **Step 5: 绿测并提交。**

```bash
cargo test -p towermd --test daemon_cli offline_cached_main_keeps_status_show_and_implicit_renewal_available --locked -- --exact
cargo test -p towermd --test daemon_cli remote_project_without_cached_authority_reports_status_but_refuses_task_reads --locked -- --exact
cargo test -p towermd --bin towermd main_refresh::tests --locked
git add crates/towermd/src/daemon.rs crates/towermd/src/main_refresh.rs crates/towermd/tests/daemon_cli.rs
git commit -m "fix: keep daemon reads and renewals available with cached main"
```

#### 阶段 5: 十个并发 status 只启动一次后台 fetch

**Files:** Test `crates/towermd/tests/daemon_cli.rs`；有失败才修改 `main_refresh.rs`。

- [ ] **Step 1: 写本地阻塞 upload-pack 的真实 daemon 回归。** 此测试仅 macOS/Unix，命令调用本机 Git 与临时裸仓库，不访问真实网络。放到 `daemon_cli.rs`：

```rust
#[cfg(unix)]
#[test]
fn ten_status_requests_share_one_nonblocking_background_fetch() {
    use std::os::unix::fs::PermissionsExt;
    let fixture = Fixture::new();
    // 预热实际项目命令，daemon status 本身不会启动 daemon。
    fixture.success(fixture.root.path(), &["status"]);
    let remote = fixture.runtime.path().join("remote.git");
    fixture.git(&["clone", "--bare", ".", remote.to_str().unwrap()]);
    let cached = fixture.git(&["rev-parse", "main"]);
    fixture.git(&["update-ref", "refs/remotes/origin/main", &cached]);
    fixture.git(&["remote", "add", "origin", remote.to_str().unwrap()]);
    let counter = fixture.runtime.path().join("fetch-count");
    let release = fixture.runtime.path().join("fetch-release");
    let upload = fixture.runtime.path().join("count-upload-pack");
    fs::write(&upload, format!(
        "#!/bin/sh\nprintf 'fetch\\n' >> '{}'\ni=0\nwhile [ ! -f '{}' ] && [ \"$i\" -lt 300 ]; do sleep 0.01; i=$((i+1)); done\nexec git-upload-pack \"$@\"\n",
        counter.display(), release.display())).unwrap();
    fs::set_permissions(&upload, fs::Permissions::from_mode(0o755)).unwrap();
    fixture.git(&["config", "remote.origin.uploadpack", upload.to_str().unwrap()]);
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(10));
    let started = std::time::Instant::now();
    let workers: Vec<_> = (0..10).map(|_| {
        let barrier = barrier.clone();
        let mut command = fixture.command(fixture.root.path(), &["status"]);
        thread::spawn(move || { barrier.wait(); command.output().unwrap() })
    }).collect();
    for worker in workers {
        let output = worker.join().unwrap();
        assert!(output.status.success(), "{output:?}");
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(value["main"]["reference"], "refs/remotes/origin/main");
    }
    assert!(started.elapsed() < Duration::from_secs(2), "status 等待了 fetch");
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while !counter.exists() {
        assert!(std::time::Instant::now() < deadline, "后台 fetch 未启动");
        thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(fs::read_to_string(&counter).unwrap().lines().count(), 1);
    fs::write(&release, "release").unwrap();
    fixture.success(fixture.root.path(), &["status"]);
    assert_eq!(fs::read_to_string(&counter).unwrap().lines().count(), 1);
}
```

临时目录名由 tempfile 生成，不含用户输入；如 fixture 路径未来允许单引号，脚本参数改为 argv 传递，不能把任意路径拼入 shell。上传脚本有 3 秒自释放上限；断言提前失败也不会留下永久阻塞子进程。

- [ ] **Step 2: 红绿运行。** 基线旧 Catalog fetch 会阻塞并触发十次；修复后每条 status 先成功、计数为 1。不能在十个 CLI worker 外包一把测试锁来“保证”通过。

```bash
cargo test -p towermd --test daemon_cli ten_status_requests_share_one_nonblocking_background_fetch --locked -- --exact
git add crates/towermd/tests/daemon_cli.rs
git commit -m "test: cover concurrent offline-safe daemon reads"
```

#### 阶段 6: 真实 daemon 时钟回归证明断网二十分钟仍续租

**Files:** Modify/Test `crates/towermd/src/daemon.rs`。

- [ ] **Step 1: 加私有可信 clock 字段。** `Server` 增加：

```rust
clock: Arc<dyn Fn() -> Result<u64, String> + Send + Sync>,
```

唯一生产构造点设 `clock: Arc::new(now_ms)`。只替换 Server 实例方法内的时间读取：`renew_read_hub`、`execute`、`snapshot`、`rebuild_request`、`tick` 的 `now_ms()?` 改为 `(self.clock)()?`；snapshot writer 的 `now_ms` 参数改为 `|| (self.clock)()`。后台快照 spawn 前 `let clock = self.clock.clone();`，writer 参数改为 `|| clock()`。不改全局 `now_ms`、hub 的生产 TTL、事件时钟或 HTTP 请求 schema，不提供环境变量或命令行时钟偏移。

- [ ] **Step 2: daemon 的 tests 模块中增加真实 HTTP 回归。** 同一个 Server、原 `command` handler、原 `tasks::execute`、原 Hub 与生产 15 分钟 TTL；只用可信 AtomicU64 推进时钟。测试不用 daemon 的 1 秒 ticker，直接在关键时刻调用同一 `Server::tick`，避免 wall-clock 20 分钟。

```rust
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn offline_twenty_minutes_of_http_activity_preserve_run_epoch() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join("repo");
    let runtime = temporary.path().join("runtime");
    fs::create_dir(&root).unwrap();
    fs::create_dir(&runtime).unwrap();
    fn git(root: &Path, args: &[&str]) -> String {
        let output = Command::new("git").current_dir(root)
            .args(["-c", "user.name=Test", "-c", "user.email=test@example.invalid",
                "-c", "commit.gpgsign=false"]).args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null").env("GIT_CONFIG_NOSYSTEM", "1")
            .output().unwrap();
        assert!(output.status.success(), "{output:?}");
        String::from_utf8(output.stdout).unwrap().trim().into()
    }
    git(&root, &["init", "-qb", "main"]);
    towermd_core::init::init(&root, &towermd_core::init::InitOptions {
        docs_dir: Some("docs/tower".into()),
    }).unwrap();
    fs::write(root.join("docs/tower/arch/A.md"),
        "---\ntype: Module\nid: A\npaths: [src/a/**]\n---\n").unwrap();
    fs::write(root.join("docs/tower/specs/AC-A.md"),
        "---\ntype: Acceptance\nid: AC-A\nmodule: A\ncriteria: [works]\n---\n").unwrap();
    git(&root, &["add", "."]);
    git(&root, &["commit", "-qm", "fixture"]);
    let revision = git(&root, &["rev-parse", "main"]);
    git(&root, &["update-ref", "refs/remotes/origin/main", &revision]);
    git(&root, &["remote", "add", "origin", "/nonexistent/towermd-twenty-minutes"]);
    let worktree = temporary.path().join("run");
    git(&root, &["worktree", "add", "-qb", "t/T-1", worktree.to_str().unwrap(), "main"]);
    let start = now_ms().unwrap();
    let time = Arc::new(AtomicU64::new(start));
    let clock = time.clone();
    let (shutdown, _shutdown_receiver) = tokio::sync::watch::channel(false);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = Arc::new(Server {
        endpoint: Endpoint { address: "127.0.0.1".into(), port, pid: std::process::id(),
            instance: "twenty-minute-test".into(), host: "test-host".into() },
        runtime, projects: Mutex::new(BTreeMap::new()), shutdown,
        main_refresh: MainRefresh::default(),
        clock: Arc::new(move || Ok(clock.load(Ordering::SeqCst))),
    });
    let app = Router::new().route("/command", post(command)).with_state(server.clone());
    let serving = tokio::spawn(async move { axum::serve(listener, app).await.unwrap(); });
    let address = format!("http://127.0.0.1:{port}/command");
    async fn call(address: &str, directory: &Path, action: Value) -> Value {
        let address = address.to_string();
        let directory = directory.to_path_buf();
        tokio::task::spawn_blocking(move || {
            let response = reqwest::blocking::Client::builder().no_proxy()
                .timeout(Duration::from_secs(2)).build().unwrap()
                .post(address).header("x-towermd-instance", "twenty-minute-test")
                .json(&json!({"directory": directory, "run": null, "command": action}))
                .send().unwrap();
            let status = response.status();
            let value: Value = response.json().unwrap();
            assert!(status.is_success(), "{value}");
            value
        }).await.unwrap()
    }
    call(&address, &root, json!({"action":"add", "task": {
        "id":"T-1", "title":"test", "module":"A", "touches":[],
        "allowed_paths":["src/a/**"], "depends_on":[], "acceptance":["AC-A"], "decisions":[]
    }})).await;
    let run = call(&address, &worktree,
        json!({"action":"claim", "id":"T-1", "harness":null, "harness_version":null})).await;
    let run_id = run["id"].clone();
    let fences = run["leases"].as_array().unwrap().clone();
    assert!(!fences.is_empty());
    for fence in &fences {
        assert!(fence["name"].is_string());
        assert!(fence["epoch"].is_u64());
    }
    for minute in [5_u64, 10, 15, 20] {
        time.store(start + minute * 60_000, Ordering::SeqCst);
        let status = call(&address, &worktree, json!({"action":"status"})).await;
        let active = status["runs"].as_array().unwrap().iter()
            .find(|value| value["id"] == run_id).unwrap();
        assert_eq!(active["state"], "running");
        assert_eq!(active["leases"], run["leases"]);
        let leases = status["leases"].as_array().unwrap();
        for fence in &fences {
            let lease = leases.iter().find(|lease|
                lease["name"] == fence["name"] && lease["holder"] == run_id).unwrap();
            assert_eq!(lease["epoch"], fence["epoch"]);
            assert!(lease["expires_at"].as_u64().unwrap() > start + minute * 60_000);
        }
        let maintenance = server.clone();
        tokio::task::spawn_blocking(move || maintenance.tick()).await.unwrap().unwrap();
    }
    let shown = call(&address, &worktree, json!({"action":"show", "id":null})).await;
    assert_eq!(shown["id"], run_id);
    assert_eq!(shown["state"], "running");
    // 不再有活动时仍按生产 TTL 过期，证明注入没有绕过租约硬门禁。
    time.store(start + 36 * 60_000, Ordering::SeqCst);
    let maintenance = server.clone();
    tokio::task::spawn_blocking(move || maintenance.tick()).await.unwrap().unwrap();
    let status = call(&address, &root, json!({"action":"status"})).await;
    let expired = status["runs"].as_array().unwrap().iter()
        .find(|value| value["id"] == run_id).unwrap();
    assert_eq!(expired["state"], "released");
    server.shutdown.send(true).unwrap();
    server.main_refresh.drain().await.unwrap();
    server.drain_snapshots().await.unwrap();
    serving.abort();
}
```

当前模型的 Run 使用 `leases: Vec<Fence>`，Fence/Lease 的键是 `name`、`holder`、`epoch`，Lease 的到期字段为 `expires_at`；以上断言已按实际类型固定，禁止 `null == null` 假证据。

- [ ] **Step 3: 红绿与时间上界。**

```bash
cargo test -p towermd --bin towermd offline_twenty_minutes_of_http_activity_preserve_run_epoch --locked -- --nocapture
```

先在旧同步 fetch Catalog 路径上验证第一个 HTTP call 因离线失败（或 2 秒超时）；再恢复无网络读取后通过。全部测试应在十秒量级完成，不使用 20 分钟 sleep。另跑原有 `tasks` TTL 测试，确认无人活动时仍过期。

```bash
cargo test -p towermd-core --test tasks --locked
git add crates/towermd/src/daemon.rs
git commit -m "test: prove offline renewals across twenty minutes of daemon time"
```

#### 阶段 7: 恢复前显式 fresh-fetch，保持快照 fencing

**Files:** Modify `crates/towermd-core/src/recovery.rs`；Test 原 `recovery::tests`、`tests/snapshot_git.rs`。

- [ ] **Step 1: 运行既有恢复 fetch 栅栏测试作为红测。**

```bash
cargo test -p towermd-core rebuild_fetch_releases_hub_but_keeps_single_writer --locked -- --nocapture
cargo test -p towermd-core rebuild_fetch_concurrent_write_preserves_target_without_seed --locked -- --nocapture
```

Catalog 去 fetch 后，既有 wrapper 期望的第一阶段 fetch 不出现，测试应失败；不得删除第一阶段测试或把 barrier 次数从二改一来掩盖安全行为改变。

- [ ] **Step 2: 在 recovery::execute 的 watermark 检查之后、`Catalog::load(&catalog.root)` 之前加入：**

```rust
crate::origin::refresh_origin_main(&catalog.root)?;
```

保持它处于 `SnapshotWriter::rebuild` 的 writer 锁内、hub mutex 外。之后仍重新读 Catalog、比较当前 request；底层 `snapshot_git::write_snapshot` 仍再次 fresh-fetch、验证 lease、扫描历史 fencing。无 origin 时 refresh 返回 `Ok(None)`，本地行为不变。

- [ ] **Step 3: 绿测与安全基线。**

```bash
cargo test -p towermd-core recovery::tests --locked
cargo test -p towermd-core --test snapshot_git --locked
cargo test -p towermd-core --test snapshot_service --locked
git add crates/towermd-core/src/recovery.rs
git commit -m "fix: retain fresh main authority checks before recovery writes"
```

快照 `fresh_fetch_fences_new_epoch_and_stale_base`、`second_lease_check_prevents_remote_and_unchecked_publication`、historical fencing 与 symref 测试必须通过；不改变 R7 的竞争推送重试现状。

#### 阶段 8: CLI init 无 daemon、副作用之前确认非交互目录

**Files:** Modify `crates/towermd/src/main.rs`；Test `init_cli.rs`、`daemon_cli.rs`。

- [ ] **Step 1: 写两条失败测试。** 在 `init_cli.rs`：

```rust
#[test]
fn noninteractive_new_init_without_docs_dir_fails_without_any_writes() {
    let repo = git_repo();
    let before: Vec<_> = fs::read_dir(repo.path()).unwrap()
        .map(|entry| entry.unwrap().file_name()).collect();
    let output = towermd_with_open_stdin(repo.path(), &["init"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr)
        .contains("非交互模式须显式传 --docs-dir，请先与人确认目录"));
    let after: Vec<_> = fs::read_dir(repo.path()).unwrap()
        .map(|entry| entry.unwrap().file_name()).collect();
    assert_eq!(after, before);
    assert!(!repo.path().join(".towermd").exists());
    assert!(!repo.path().join("docs").exists());
    assert!(!repo.path().join(".git/index").exists());
    assert!(!repo.path().join(".git/towermd").exists());
}
```

在 `daemon_cli.rs`：

```rust
#[test]
fn init_does_not_validate_run_or_renew_when_daemon_is_live() {
    let fixture = Fixture::new();
    fixture.add("T-1", "A");
    let worktree = fixture.worktree("T-1");
    fixture.json(&worktree, &["task", "claim", "T-1"]);
    let before = journal(&fixture);
    fixture.success(&worktree, &["--run", "missing-run", "init", "--docs-dir", "docs/tower"]);
    let after = journal(&fixture);
    let old_count = before["events"].as_array().unwrap().len();
    assert!(!after["events"].as_array().unwrap()[old_count..].iter()
        .any(|event| event["type"] == "lease.renewed" && event["obj"] != "main"));
}
```

允许后台主干心跳变化，不用全 journal 逐字节相等制造 flaky 断言。

- [ ] **Step 2: 红测。**

```bash
cargo test -p towermd --test init_cli noninteractive_new_init_without_docs_dir_fails_without_any_writes --locked -- --exact
cargo test -p towermd --test daemon_cli init_does_not_validate_run_or_renew_when_daemon_is_live --locked -- --exact
```

- [ ] **Step 3: main.rs 最小替换。** `run` 前置续租 `matches!` 删除 `Command::Init { .. } |`。daemon status/stop 续租行为保留。`--docs-dir` help 改为 `用户文档目录，相对仓库根；非交互新配置必须显式指定`。`run_init` 的 docs_dir match 改为：

```rust
let existing = Config::load(&repo::toplevel(&directory)?)?;
let docs_dir = match docs_dir {
    Some(docs_dir) => Some(docs_dir),
    None if existing.is_some() => None,
    None if io::stdin().is_terminal() => ask_docs_dir()?,
    None => return Err("非交互模式须显式传 --docs-dir，请先与人确认目录".into()),
};
```

发生在 core `init` 前，因而没有 temp、fetch、daemon/runtime 目录或用户文件写入。TTY 的空行明确接受默认目录，既有 read_docs_dir 单元测试保留。core `InitOptions::default` 仍代表库调用者已选择默认值；本批不改库 API 或所有核心 fixture。

- [ ] **Step 4: 修正受要求改变的旧 CLI fixture，避免让测试依赖被删除的默认。**

`daemon_cli.rs` 的 `Fixture::new`：

```rust
fixture.success(fixture.root.path(), &["init", "--docs-dir", "docs/tower"]);
```

`lifecycle_commands_renew_the_calling_worktree_not_init_target` 改名 `daemon_management_commands_renew_the_calling_worktree`，数组删除 init 一项，保留 daemon status/stop。`init_cli.rs` 的 `path_argument_selects_the_repo` 使用 `init <path> --docs-dir docs/tower`；`existing_agents_files_emit_warning_and_hint_without_overwrite` 新项目调用加显式 docs-dir；`closed_stdout_reports_error_without_panicking` 的 init 项传显式 docs-dir，以继续验证 broken pipe 而非被目录校验短路。把 `default_init_does_not_wait_for_open_stdin` 替换为 Step 1 的新零写测试，reinit/explicit open-stdin 测试保留。

- [ ] **Step 5: 绿测并提交。**

```bash
cargo test -p towermd --test init_cli --locked
cargo test -p towermd --test daemon_cli init_does_not_validate_run_or_renew_when_daemon_is_live --locked -- --exact
cargo test -p towermd --test daemon_cli daemon_management_commands_renew_the_calling_worktree --locked -- --exact
git add crates/towermd/src/main.rs crates/towermd/tests/init_cli.rs crates/towermd/tests/daemon_cli.rs
git commit -m "fix: keep init independent of daemon leases and implicit directories"
```

#### 阶段 9: init 尽力 preflight，remote SHA 协商，只有明确策略拒绝硬错

**Files:** Modify `crates/towermd-core/src/init.rs`；Test `crates/towermd-core/tests/init_push.rs`。

- [ ] **Step 1: 先改已有失败预期，得到真正行为红测。** 将原 unreachable/no-local-main 两测试替换为：

```rust
#[test]
fn unreachable_origin_warns_but_initializes_without_local_main() {
    let fixture = Fixture::new();
    git(&fixture.root, &["remote", "add", "origin", fixture.remote.to_str().unwrap()]);
    let report = init(&fixture.root, &InitOptions { docs_dir: Some("docs/tower".into()) }).unwrap();
    assert!(fixture.root.join(CONFIG_PATH).is_file());
    assert!(report.warnings.iter().any(|warning| warning.contains("fetch")
        && warning.contains("快照写入时会再次检查")));
    assert!(report.warnings.iter().any(|warning| warning.contains("本地 main")));
    assert!(!fixture.root.join(".git/index").exists());
    assert!(!fixture.remote.exists());
}

#[test]
fn divergent_local_main_negotiates_remote_sha_without_publishing() {
    let fixture = Fixture::new();
    fixture.commit();
    fixture.add_origin();
    let tree = git(&fixture.remote, &["rev-parse", "main^{tree}"]);
    let next = git(&fixture.remote, &["commit-tree", &tree, "-p", "main", "-m", "remote advance"]);
    git(&fixture.remote, &["update-ref", "refs/heads/main", &next]);
    fixture.commit();
    let local_main = git(&fixture.root, &["rev-parse", "main"]);
    let report = init(&fixture.root, &InitOptions::default()).unwrap();
    assert!(report.warnings.iter().any(|warning| warning.contains("dry-run") && warning.contains("不保证")));
    assert_eq!(git(&fixture.root, &["rev-parse", "refs/remotes/origin/main"]), next);
    assert_eq!(git(&fixture.root, &["rev-parse", "main"]), local_main);
    assert_eq!(git(&fixture.remote, &["rev-parse", "main"]), next);
    assert!(!fixture.root.join(".git/FETCH_HEAD").exists());
}
```

- [ ] **Step 2: 红测。**

```bash
cargo test -p towermd-core --test init_push unreachable_origin_warns_but_initializes_without_local_main --locked -- --exact
cargo test -p towermd-core --test init_push divergent_local_main_negotiates_remote_sha_without_publishing --locked -- --exact
```

旧实现第一条没有本地 main 即失败，第二条拿本地分叉 SHA dry-run 非快进失败。

- [ ] **Step 3: push_preflight 返回 Vec，不以网络失败拒绝 init。** 完整替换 `init.rs` 的旧 preflight；调用点从 `if let Some(diagnostic)` 改成 `warnings.extend(push_preflight(&root)?);`。

```rust
fn push_preflight(root: &Path) -> Result<Vec<String>, String> {
    let remotes = repo::git(root, &["remote"])?;
    if !remotes.lines().any(|remote| remote == "origin") {
        return Ok(Vec::new());
    }
    let mut warnings = Vec::new();
    if repo::git(root, &["rev-parse", "--verify", "refs/heads/main^{commit}"]).is_err() {
        warnings.push("没有本地 main 提交；init 仍生成文件，快照写入时会再次检查".into());
    }
    match crate::origin::refresh_origin_main(root) {
        Ok(Some(sha)) => match crate::origin::push_dry_run(root, &sha) {
            Ok(diagnostic) => warnings.push(diagnostic),
            Err(crate::origin::PreflightError::Policy(error)) => return Err(error),
            Err(crate::origin::PreflightError::Unavailable(error)) => warnings.push(format!(
                "origin/main dry-run 未完成：{error}；快照写入时会再次检查")),
        },
        Ok(None) => {}
        Err(error) => warnings.push(format!(
            "origin/main fetch 预检未完成：{error}；继续本地初始化，快照写入时会再次检查")),
    }
    Ok(warnings)
}
```

删除 init.rs 不再用的 `Duration` import。`push_dry_run` 使用阶段2定义的 `PreflightError` 区分明确策略与不可用；不依赖字符串前缀协议，不扩展成全局错误重构。

fetch 失败时跳过 dry-run，不能拿本地 main 或旧 tracking SHA 假称完成本轮协商。没有远端 main（空 origin）同样只警告；不主动创建远端 main。成功 fetch 只更新 tracking ref；允许这一个明确派生引用变化，本地 heads/worktrees/index/global config/hooks 必须不变。

- [ ] **Step 4: 写策略/auth 的本地 receive-pack 协商测试。** 在 init_push.rs 复用 executable 与 Fixture，加入：

```rust
#[cfg(unix)]
#[test]
fn explicit_policy_rejection_is_fatal_but_authentication_failure_is_only_warning() {
    for (diagnostic, fatal) in [
        ("remote: error: GH006: Protected branch update failed", true),
        ("fatal: Authentication failed", false),
        ("Permission denied (publickey)", false),
        ("fatal: repository access unavailable", false),
    ] {
        let fixture = Fixture::new();
        fixture.commit();
        fixture.add_origin();
        let receiver = fixture.root.parent().unwrap().join("reject-receive-pack");
        executable(&receiver, &format!("#!/bin/sh\nprintf '%s\\n' '{}' >&2\nexit 1\n", diagnostic));
        git(&fixture.root, &["config", "remote.origin.receivepack", receiver.to_str().unwrap()]);
        let before = git(&fixture.root, &["rev-parse", "main"]);
        let remote_before = git(&fixture.remote, &["show-ref"]);
        let result = init(&fixture.root, &InitOptions { docs_dir: Some("docs/tower".into()) });
        if fatal {
            assert!(result.unwrap_err().contains("明确拒绝推送策略"));
            fixture.assert_uninitialized();
        } else {
            let report = result.unwrap();
            assert!(report.warnings.iter().any(|warning| warning.contains("快照写入时会再次检查")));
            assert!(fixture.root.join(CONFIG_PATH).is_file());
        }
        assert_eq!(git(&fixture.root, &["rev-parse", "main"]), before);
        assert_eq!(git(&fixture.remote, &["show-ref"]), remote_before);
        assert!(!fixture.root.join(".git/index").exists());
    }
}
```

这是真实 git dry-run 的 stderr 分类回归；阶段 2 的 unit table 负责 stdout porcelain 分类，不能声称 dry-run 执行了远端 pre-receive hook。

- [ ] **Step 5: 将原 init_push 全部反向行为改为新契约，保留安全断言。**

| 现有测试 | 新预期 |
| --- | --- |
| `dry_run_negotiates_without_publishing_local_commits_or_changing_git_state` | 只比较 `for-each-ref refs/heads/` 不变；tracking ref 可更新，index/dirty文件/remote全部ref/FETCH_HEAD断言保留 |
| `remote_non_fast_forward_rejection_precedes_all_init_writes` | 改为 Step 1 divergent 测试，remote SHA 协商成功，不能再期待非快进 |
| `empty_origin_is_only_negotiated_and_never_initialized_by_push` | 初始化成功、warning 含 fetch 与再次检查，remote refs 仍空 |
| `failed_reinit_preserves_existing_config_and_does_not_fill_missing_docs` | 改为离线 reinit 成功、config 原样、补齐缺失骨架、warning 可见 |
| `explicit_main_refspec_ignores_default_push_and_mirror_configuration` | 成功；remote refs 不变、本地 heads不变、cached SHA 等于远端 main |
| `preflight_uses_origin_push_url` | fetch 成功但 pushurl 不可用，成功+dry-run warning，remote不变 |
| `push_negotiation_has_a_finite_timeout_and_leaves_init_unwritten` | 改名 `push_negotiation_timeout_warns_and_still_initializes`，30秒有限超时后成功，warning含超时/再次检查；保留40秒上界 |
| `hooks_are_not_executed_and_server_policy_is_not_claimed_as_verified` | 原安全断言完整保留；加 fetch 完成也未执行任何本地 hooks 的断言 |

同步检查本地 main、所有工作树 HEAD/index、用户 global config 与 hooks 字节。global config 只设 `GIT_CONFIG_GLOBAL` 指向 fixture 文件后比较，不能读改用户真实配置。

- [ ] **Step 6: 绿测并提交。**

```bash
cargo test -p towermd-core --test init_push --locked
cargo test -p towermd-core init::tests --locked
cargo test -p towermd --test init_cli --locked
git add crates/towermd-core/src/init.rs crates/towermd-core/src/origin.rs crates/towermd-core/tests/init_push.rs
git commit -m "fix: make init remote preflight best effort"
```

#### 阶段 10: 稳定 AGENTS 规则与真实技能提示

**Files:** Modify `templates/agents.md`、`src/init.rs`；Test `init::tests`、`tests/init_cli.rs`。

- [ ] **Step 1: 将旧 availability 测试替换为规则测试，增加提示测试。**

```rust
#[test]
fn agents_template_contains_rules_without_availability_promises() {
    let rendered = render_agents_md("docs/custom");
    for required in ["L0–L2", "L3", "快照专用", "docs/custom/state/", "命令行不提供批准类命令", "进度只认证据"] {
        assert!(rendered.contains(required), "{required}");
    }
    for obsolete in ["尚未提供", "当前能力", "后续的", "init` 与守护进程管理命令也按调用位置续租"] {
        assert!(!rendered.contains(obsolete), "{obsolete}");
    }
}
```

`init_cli.rs` 新增：

```rust
#[test]
fn init_explains_manual_skill_installation_without_inventing_a_command() {
    let repo = git_repo();
    let output = towermd(repo.path(), &["init", "--docs-dir", "docs/tower"]);
    assert!(output.status.success(), "{output:?}");
    let text = stdout(&output);
    assert!(text.contains("技能包"));
    assert!(text.contains("尚未发布"));
    assert!(text.contains(".agents/skills/"));
    assert!(text.contains(".claude/skills/"));
    assert!(!text.contains("towermd skills install"));
    assert!(!repo.path().join(".agents").exists());
    assert!(!repo.path().join(".claude").exists());
}
```

- [ ] **Step 2: 红测运行。**

```bash
cargo test -p towermd-core agents_template_contains_rules_without_availability_promises --locked
cargo test -p towermd --test init_cli init_explains_manual_skill_installation_without_inventing_a_command --locked -- --exact
```

- [ ] **Step 3: 模板整体替换为以下稳定规矩。**

```markdown
# Tower.md 仓库规矩

本文件由 `towermd init` 生成，写给在本仓库里工作的任何 Agent。修改本文件属于 L3 流程变更，需要人登记。

## L0–L2 慢层：只能经人在 Tower.md App 中批准修改

- `{docs_dir}/intents/`、`{docs_dir}/arch/`、`{docs_dir}/specs/`、`{docs_dir}/baselines/` 下的全部文件
- `{docs_dir}/tower.canvas`
- 其他位置 frontmatter 中 `layer` 为 `L0`–`L2` 的对象

需要改慢层时，提出附带 diff 的提案，经人批准后由 Tower.md 写入主干。不得直接修改或绕过批准。

## 快照专用

`{docs_dir}/state/` 只由持有主干租约的 Tower.md 守护进程经快照写入模块创建和写入。`init` 不创建它；不要手写快照、伪造事件或自行删除 hub 数据。

## L3 配置与流程

`.towermd/` 下的配置及本文件属于 L3 配置与流程，不归为 L0–L2 慢层。变更需要人登记；`init` 沿用已有项目 ID 和配置，不覆盖人工修改的规矩。`init` 只在当前工作树生成文件，不提交，不为运行续租。

## 执行状态、完成与合并

任务、运行和租约通过守护进程访问 hub。慢层对象与配置只从权威 main 读取，不以任务分支、工作树或 Agent 自述代替权威。先创建目标工作树和 `t/<task-id>` 分支，再领取任务；查询运行按工作树与分支选择绑定，长操作期间定期查询状态以续租。租约 TTL 为 15 分钟，网络远端不可用不阻止已有缓存项目的本地读取与续租。

- 只改任务卡 `allowed_paths` 范围内的文件。
- 完成须提交绑定代码的验证证据，经门禁检查才能合并。
- Agent 自己报告“完成”不会改变任务状态，进度只认证据。
- 命令行不提供批准类命令，评审与授权不得超时自动放行。
```

- [ ] **Step 4: 在 init report hints 无条件追加实际提示。** 不假装仓库现在有可复制的技能文件：

```rust
hints.push("Tower.md 技能包尚未发布：本仓库预定包位置为 skills/，当前没有可安装的 Tower.md 技能文件。发布后请由用户按 harness 原生方式安装：Codex 的项目技能目录 .agents/skills/，Claude Code 的项目技能目录 .claude/skills/；每个技能放入含 SKILL.md 的独立子目录。init 不创建这些目录、不修改全局 harness 配置，也不执行安装。".into());
```

项目 `.agents/skills/`、`.claude/skills/` 是真实原生目录；若实施前要核对 harness 当前文档，使用已安装本地文档或官方来源，不能执行不存在的 towermd 安装命令。此批只打印提示；不下载、创建或修改 harness 文件。

已有 `.towermd/AGENTS.md` 内容不同仍原样保留并警告。重入不迁移人工文件；模板更新不是批量改用户规则的授权。

- [ ] **Step 5: 绿测并提交。**

```bash
cargo test -p towermd-core init::tests --locked
cargo test -p towermd --test init_cli --locked
git add crates/towermd-core/templates/agents.md crates/towermd-core/src/init.rs crates/towermd/tests/init_cli.rs
git commit -m "fix: generate stable rules and honest skill installation hints"
```

#### 阶段 11: 文档、完整验证与精确证据

**Files:** Modify `README.md`、`CHANGELOG.md`、`CHANGELOG.zh-CN.md`、`tasks/todo.md`。

- [ ] **Step 1: 用户文档增量。** 两份未发布区各加对应三条，不混入未完成的 R3/R4：

```markdown
- Read and renew active runs offline from the most recently cached authoritative main; background fetch failures are reported without blocking commands.
- Initialization no longer contacts the daemon or renews runs. New non-interactive projects require an explicit `--docs-dir`.
- Remote push preflight during initialization is best effort, negotiates the fetched remote main, and reports manual skill installation guidance without changing harness configuration.
```

```markdown
- 已缓存权威 main 的项目可离线读取和续租；后台 fetch 失败会提示，不阻塞命令。
- 初始化不再访问守护进程或续租；非交互新项目必须显式传入 `--docs-dir`。
- 初始化的远端推送预检改为尽力协商远端 main，并提示手工安装技能，不修改 harness 配置。
```

README 的初始化示例统一用 `towermd init --docs-dir docs/tower`，描述 TTY 询问、已有 config 重入、未提交供人检查、缺首次 cached authority 的限制。不要把 status 中 `fetched_at: null` 解释成刚刚 fetch 成功。

- [ ] **Step 2: 有界验证计划。** 使用 Root 预先隔离的 target 目录或串行验证，保留已知 FD 初始并发问题，不以重跑掩盖新的失败。

```bash
cargo fmt --all -- --check
cargo test --workspace --locked -- --test-threads=1
cargo clippy --workspace --all-targets --locked -- -D warnings
git diff --check
```

另单独运行十并发 status 回归；其中并发发生在测试内部，不受测试线程数为一影响。所有网络只指向临时本机 bare 仓库或刻意不存在的路径，不能以用户 origin 作验收。

- [ ] **Step 3: 规格与质量审查。** 独立审查须逐项确认：Catalog 没有 fetch；所有 daemon 入口都调度而不等待；后台 single-flight 与失败限频；ref symref/CAS；20分钟真实 daemon HTTP 续租及16分钟不活动仍过期；init无daemon与零写非交互拒绝；policy/auth分类；fetch/dry-run不碰heads/index/hooks；恢复同步fetch与原fencing仍在。未通过不得宣称完成。

- [ ] **Step 4: tasks/todo.md 写结果。** 记录本批每个精确 commit、实际测试条数/命令、已知原始 FD 诊断与范围限制。独立 commit 后再写任务记录 commit；不能将未提交文件写成已纳入发布。此批不发版、不打 tag、不 push。

```bash
git add README.md CHANGELOG.md CHANGELOG.zh-CN.md tasks/todo.md
git commit -m "docs: record offline operation and initialization behavior"
git status --short
git log -12 --oneline
```

## 覆盖检查与交接

| 要求 | 实施与证据 |
| --- | --- |
| cached main 读，不逐命令 fetch | 阶段 1、4 |
| daemon 60秒/同common-dir single-flight/失败限频 | 阶段 3、5 |
| fetch metadata 定义与可删除重建 | 阶段 3、4 |
| 缺远端权威不提升任务分支或本地 main | 阶段 1、4 |
| 不存在 origin 路径、cached ref 仍读与续租 | 阶段 4 |
| 十并发status只fetch一次 | 阶段 5 |
| 模拟20分钟断网生产TTL不失效 | 阶段 6 |
| 快照/恢复freshfetch与fencing | 阶段 7、原snapshot安全基线 |
| init不续租、不依赖daemon、非交互新配置零写失败 | 阶段 8 |
| 已有配置重入沿旧目录 | 阶段 8原reinit测试 |
| fetch后远端SHA dry-run，只有明确policy硬错 | 阶段 2、9 |
| 无真实push、用户main/worktree/index/hooks/config不变 | 阶段 2、9 |
| 稳定模板、FR105真实提示、不改harness | 阶段 10 |
| 验证、用户文档、精确提交证据 | 阶段 11 |

Root 已裁决权威缺失边界；Run/Lease 字段与 preflight 类型接口已按现有代码锁定，无待用户重复批准的产品问题。Root 复核此计划后可在整改工作树实施。本计划本身未执行源码修改、提交或真实网络。
