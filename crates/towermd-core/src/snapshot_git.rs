use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::init::normalize_docs_dir;
pub(crate) use crate::repository_writer::Session;
use crate::repository_writer::{RepositoryRef, TreeFile};

pub(crate) const PRIVATE_REF: &str = "refs/towermd/snapshot";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WriteIntent {
    Advance,
    Promote { source_commit: String },
}

pub struct WriteRequest {
    pub base: String,
    pub docs_dir: String,
    pub state_json: String,
    pub archives: BTreeMap<String, String>,
    pub through_seq: u64,
    pub writer_epoch: u64,
}

#[derive(Debug, PartialEq, Eq)]
pub enum WriteOutcome {
    Written {
        commit: String,
    },
    Deferred {
        reason: String,
    },
    Superseded {
        commit: String,
        epoch: u64,
        seq: u64,
    },
}

#[derive(Debug, PartialEq, Eq)]
pub struct SnapshotTrailer {
    pub seq: u64,
    pub epoch: u64,
}

pub fn write_snapshot(
    root: &Path,
    request: &WriteRequest,
    verify_lease: impl FnMut() -> Result<(), String>,
) -> Result<WriteOutcome, String> {
    write_snapshot_with_intent(root, request, &WriteIntent::Advance, verify_lease)
}

pub fn write_snapshot_with_intent(
    root: &Path,
    request: &WriteRequest,
    intent: &WriteIntent,
    mut verify_lease: impl FnMut() -> Result<(), String>,
) -> Result<WriteOutcome, String> {
    let root = canonical_root(root)?;
    let files = snapshot_files(request)?;
    verify_lease()?;
    let remote = read_git(&root, &["remote"])?
        .lines()
        .any(|name| name == "origin");
    let session = Session::new(&root)?;
    let push_url = if remote {
        Some(session.origin_push_url(&root)?)
    } else {
        None
    };
    let initial_context = crate::catalog::Catalog::at_revision(&root, &request.base)?;
    #[cfg(test)]
    tests::observe(&root, &["catalog-materialized", &request.base]);
    for attempt in 1..=3 {
        let cached = if remote {
            Some(cached_origin_main(&session, &root)?)
        } else {
            None
        };
        let private = private_revision(&root)?;
        let mut private_target = false;
        let base = if remote {
            fetch_main(&session, &root)?
        } else {
            require_direct_main(&session, &root)?;
            private_target = main_worktree(&session, &root)?.is_some();
            session.run(&root, &["rev-parse", "--verify", "refs/heads/main"], None)?
        };
        let refreshed_context;
        let context =
            if let Some(initial) = initial_context.as_ref().filter(|_| base == request.base) {
                // Reuse only this write's immutable parse. The actual clone and
                // authoritative config/context must still match at this boundary.
                crate::recovery_outbox::RecoveryContext::from_catalog(initial)?;
                Some(initial)
            } else {
                refreshed_context = crate::catalog::Catalog::at_revision(&root, &base)
                    .map_err(|error| format!("权威上下文已变化，须重新规划快照：{error}"))?;
                #[cfg(test)]
                tests::observe(&root, &["catalog-materialized", &base]);
                refreshed_context.as_ref()
            };
        validate_context(initial_context.as_ref(), context)?;
        let sources = context
            .map(crate::snapshot_service::read_snapshot_sources)
            .transpose()?;
        if let WriteIntent::Promote { source_commit } = intent {
            if private.as_ref() != Some(source_commit) || private_target {
                return Err("私有搬送来源已变化或主干仍不可写".into());
            }
            let sources = sources.as_ref().ok_or("搬送缺少快照来源")?;
            let source = sources.private.as_ref().ok_or("私有搬送来源缺失")?;
            let checkpoint: crate::snapshot::Snapshot =
                serde_json::from_str(&request.state_json).map_err(|e| e.to_string())?;
            if source.snapshot != checkpoint || source.archives != request.archives {
                return Err("私有搬送 state/归档与已核来源不等价".into());
            }
        }
        let selected = sources
            .map(|sources| sources.select())
            .transpose()?
            .flatten();
        if let Some((commit, trailer)) =
            highest_fence_across_sources(&session, &root, &base, private.as_deref())?
        {
            let boundary = (trailer.epoch, trailer.seq);
            let requested = (request.writer_epoch, request.through_seq);
            if boundary > requested
                || (boundary == requested && matches!(intent, WriteIntent::Advance))
            {
                return Ok(WriteOutcome::Superseded {
                    commit,
                    epoch: trailer.epoch,
                    seq: trailer.seq,
                });
            }
        }
        if let Some(main) = selected.as_ref().filter(|saved| {
            matches!(intent, WriteIntent::Promote { .. })
                && saved.source == crate::snapshot_service::SnapshotSource::Main
                && saved.snapshot.writer_epoch == request.writer_epoch
                && saved.snapshot.through_seq == request.through_seq
        }) {
            if remote {
                cache_published_main(&session, &root, cached.as_deref().unwrap(), &base).map_err(
                    |error| {
                        format!(
                            "Git 快照已写入 {}，但 origin/main 缓存未协调：{error}",
                            main.commit
                        )
                    },
                )?;
            }
            return Ok(WriteOutcome::Superseded {
                commit: main.commit.clone(),
                epoch: request.writer_epoch,
                seq: request.through_seq,
            });
        }
        if !remote && base != request.base {
            return Err(
                "main 已推进；caller 须重新 fetch、加载 base 和检查 fence，不可盲重试".into(),
            );
        }
        {
            let prefix = format!("{}/state/", request.docs_dir);
            let paths = session.run(
                &root,
                &["ls-tree", "-r", "--name-only", "-z", &base, "--", &prefix],
                None,
            )?;
            if paths
                .split('\0')
                .filter(|path| !path.is_empty())
                .any(|path| !files.contains_key(path))
            {
                return Err("main 快照有来源以外的文件或月份，拒绝写入".into());
            }
        }
        let tree_files: Vec<_> = files
            .iter()
            .map(|(path, content)| TreeFile {
                path,
                mode: "100644",
                content,
            })
            .collect();
        let tree = session.build_tree(&root, &base, &tree_files, &[])?;
        let message = message(request);
        verify_private_source(&root, private.as_ref())?;
        if let Some(winner) =
            observed_superseding_snapshot(&session, &root, request, initial_context.as_ref())?
        {
            return Ok(winner);
        }
        verify_lease()?;
        verify_private_source(&root, private.as_ref())?;
        let mut parents = vec![base.as_str()];
        if let Some(previous) = private.as_ref().filter(|previous| *previous != &base) {
            parents.push(previous);
        }
        let commit = session.commit_tree(&root, &tree, &parents, &message)?;
        verify_private_source(&root, private.as_ref())?;
        if let Some(winner) =
            observed_superseding_snapshot(&session, &root, request, initial_context.as_ref())?
        {
            return Ok(winner);
        }
        verify_lease()?;
        verify_private_source(&root, private.as_ref())?;
        if remote {
            let output = session.push_commit(
                &root,
                push_url.as_deref().unwrap(),
                &commit,
                &RepositoryRef::main(),
            )?;
            if !output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let stderr = String::from_utf8_lossy(&output.stderr);
                match classify_push(&stdout, &stderr) {
                    PushFailure::NonFastForward if attempt < 3 => continue,
                    PushFailure::NonFastForward => {
                        return Err(format!(
                            "快照三次竞争失败；push 未确认成功：{stdout}\n{stderr}"
                        ))
                    }
                    PushFailure::Other(diagnostic) => {
                        return Err(format!("快照 push 未确认成功：{diagnostic}"))
                    }
                }
            }
            cache_published_main(&session, &root, cached.as_deref().unwrap(), &commit).map_err(|error| {
            format!("Git 快照已写入 {commit}，但 origin/main 缓存未协调；未确认 hub，须重新读取权威 main 后协调：{error}")
        })?;
        } else {
            private_target |= main_worktree(&session, &root)?.is_some();
            require_direct_main(&session, &root)?;
            if private_target && matches!(intent, WriteIntent::Promote { .. }) {
                return Ok(WriteOutcome::Deferred {
                    reason: "main 已检出，稍后搬送私有快照".into(),
                });
            }
            let (mut reference, mut old) = if private_target {
                // Validate again immediately before CAS; --no-deref never follows a
                // competing symbolic ref into a user's branch.
                private_revision(&root)?;
                (
                    PRIVATE_REF,
                    private.clone().unwrap_or_else(|| "0".repeat(base.len())),
                )
            } else {
                ("refs/heads/main", base.clone())
            };
            #[cfg(test)]
            tests::observe(&root, &["before-local-cas-validation"]);
            verify_lease()?;
            verify_private_source(&root, private.as_ref())?;
            // The final lease callback may overlap a user checkout. Only the local
            // branch target needs one last lightweight worktree check before CAS.
            if !private_target && main_worktree(&session, &root)?.is_some() {
                if matches!(intent, WriteIntent::Promote { .. }) {
                    return Ok(WriteOutcome::Deferred {
                        reason: "main 已检出，稍后搬送私有快照".into(),
                    });
                }
                private_revision(&root)?;
                reference = PRIVATE_REF;
                old = private.clone().unwrap_or_else(|| "0".repeat(base.len()));
                verify_lease()?;
                verify_private_source(&root, private.as_ref())?;
            }
            let target = if reference == PRIVATE_REF {
                RepositoryRef::snapshot()
            } else {
                RepositoryRef::main()
            };
            session
                .cas_ref(&root, &target, &commit, &old)
                .map_err(|error| {
                    format!("{reference} CAS 失败；caller 须重新加载并检查 fence：{error}")
                })?;
        }
        return Ok(WriteOutcome::Written { commit });
    }
    unreachable!("the third competing push returns a diagnostic")
}

#[derive(Debug, PartialEq, Eq)]
enum PushFailure {
    NonFastForward,
    Other(String),
}

fn classify_push(stdout: &str, stderr: &str) -> PushFailure {
    let statuses: Vec<_> = stdout
        .lines()
        .filter(|line| line.starts_with("!\t"))
        .collect();
    if statuses.len() == 1 {
        let fields: Vec<_> = statuses[0].split('\t').collect();
        if fields.len() == 3
            && fields[1]
                .split_once(':')
                .is_some_and(|(_, target)| target == "refs/heads/main")
            && matches!(
                fields[2],
                "[rejected] (non-fast-forward)" | "[rejected] (fetch first)"
            )
        {
            return PushFailure::NonFastForward;
        }
    }
    PushFailure::Other(format!("{stdout}\n{stderr}"))
}

// A strictly higher verified source stops a losing writer; it never installs or acknowledges it.
// This only reads current local observations. Remote NFF still uses the original fresh-fetch loop.
fn observed_superseding_snapshot(
    session: &Session,
    root: &Path,
    request: &WriteRequest,
    initial: Option<&crate::catalog::Catalog>,
) -> Result<Option<WriteOutcome>, String> {
    let current = crate::catalog::Catalog::load_optional(root)?;
    validate_context(initial, current.as_ref())?;
    let Some(current) = current else {
        return Ok(None);
    };
    let sources = crate::snapshot_service::read_snapshot_sources(&current)?;
    let private = sources.private_revision.clone();
    let selected = sources.select()?;
    let highest =
        highest_fence_across_sources(session, root, &current.revision, private.as_deref())?;
    if let Some((_, boundary)) = highest.filter(|(_, boundary)| {
        (boundary.epoch, boundary.seq) > (request.writer_epoch, request.through_seq)
    }) {
        let saved = selected.ok_or("higher observed snapshot fence has no verified source")?;
        if (saved.snapshot.writer_epoch, saved.snapshot.through_seq)
            != (boundary.epoch, boundary.seq)
        {
            return Err("higher observed snapshot fence differs from verified source".into());
        }
        return Ok(Some(WriteOutcome::Superseded {
            commit: saved.commit,
            epoch: boundary.epoch,
            seq: boundary.seq,
        }));
    }
    Ok(None)
}

fn validate_context(
    previous: Option<&crate::catalog::Catalog>,
    current: Option<&crate::catalog::Catalog>,
) -> Result<(), String> {
    let read = |catalog: &crate::catalog::Catalog, path: &str| {
        #[cfg(test)]
        tests::observe(
            &catalog.root,
            &["context-file-read", &catalog.revision, path],
        );
        catalog.read_file(path)
    };
    let equivalent = match (previous, current) {
        (None, None) => true,
        (Some(previous), Some(current)) => {
            previous.config.to_yaml() == current.config.to_yaml()
                && previous.object_paths == current.object_paths
                && previous
                    .objects
                    .iter()
                    .map(|(id, doc)| (id, doc.to_text()))
                    .eq(current.objects.iter().map(|(id, doc)| (id, doc.to_text())))
                && previous.protected_paths == current.protected_paths
                && if previous.root == current.root
                    && previous.common_dir == current.common_dir
                    && previous.revision == current.revision
                    && previous.protected_paths == current.protected_paths
                {
                    // Exact committed inputs share this one actual read only
                    // within this comparison. Missing/corrupt input still fails.
                    for path in &previous.protected_paths {
                        read(previous, path)?;
                    }
                    true
                } else {
                    previous
                        .protected_paths
                        .iter()
                        .map(|path| read(previous, path))
                        .collect::<Result<Vec<_>, _>>()?
                        == current
                            .protected_paths
                            .iter()
                            .map(|path| read(current, path))
                            .collect::<Result<Vec<_>, _>>()?
                }
        }
        _ => false,
    };
    if equivalent {
        Ok(())
    } else {
        Err("权威上下文已变化，须重新规划快照".into())
    }
}

fn verify_private_source(root: &Path, expected: Option<&String>) -> Result<(), String> {
    if private_revision(root)?.as_ref() != expected {
        return Err("私有快照来源已变化，CAS fence 拒绝发布；须重新读取".into());
    }
    Ok(())
}

/// A private snapshot is always a direct commit ref, including when dangling.
/// Readers use this lightweight lookup in their authority cache key.
pub(crate) fn private_revision(root: &Path) -> Result<Option<String>, String> {
    crate::repository_writer::direct_revision(
        root,
        &crate::repository_writer::RepositoryRef::snapshot(),
    )
}

#[cfg(test)]
fn private_ref_exists_supported(root: &Path) -> Result<bool, String> {
    crate::repository_writer::ref_exists_supported(root)
}

pub(crate) fn main_writable(root: &Path) -> Result<bool, String> {
    if read_git(root, &["remote"])?
        .lines()
        .any(|name| name == "origin")
    {
        return Ok(true);
    }
    let session = Session::for_network(root)?;
    require_direct_main(&session, root)?;
    Ok(main_worktree(&session, root)?.is_none())
}

fn cached_origin_main(session: &Session, root: &Path) -> Result<String, String> {
    let reference = "refs/remotes/origin/main";
    if session
        .run(root, &["symbolic-ref", "--quiet", reference], None)
        .is_ok()
    {
        return Err("origin/main 是符号引用；拒绝协调用户分支".into());
    }
    let records = session.run(
        root,
        &[
            "for-each-ref",
            "--format=%(refname)%00%(objectname)%00%(symref)",
            reference,
        ],
        None,
    )?;
    let record = records
        .lines()
        .find_map(|line| line.strip_prefix("refs/remotes/origin/main\0"))
        .ok_or("cached origin/main 缺失")?;
    let (oid, symbolic) = record
        .split_once('\0')
        .ok_or("无法解析 cached origin/main")?;
    if !symbolic.is_empty() {
        return Err("origin/main 是符号引用；拒绝协调用户分支".into());
    }
    Ok(oid.into())
}

fn cache_published_main(
    session: &Session,
    root: &Path,
    base: &str,
    commit: &str,
) -> Result<(), String> {
    let includes_publication = |current: &str| {
        current == commit
            || session
                .run(
                    root,
                    &["merge-base", "--is-ancestor", commit, current],
                    None,
                )
                .is_ok()
    };
    let current = cached_origin_main(session, root)?;
    if includes_publication(&current) {
        return Ok(());
    }
    if current != base {
        return Err(format!("cached origin/main 已变化为 {current}，拒绝覆盖"));
    }
    match session.run(
        root,
        &[
            "update-ref",
            "--no-deref",
            "refs/remotes/origin/main",
            commit,
            base,
        ],
        None,
    ) {
        Ok(_) => Ok(()),
        Err(error) => {
            // A competing refresh may have installed this publication or a successor.
            // Never retry the CAS with a newly observed old value.
            let current = cached_origin_main(session, root)?;
            if includes_publication(&current) {
                Ok(())
            } else {
                Err(error)
            }
        }
    }
}

fn require_direct_main(session: &Session, root: &Path) -> Result<(), String> {
    let references = session.run(
        root,
        &[
            "for-each-ref",
            "--format=%(refname)%00%(symref)",
            "refs/heads/main",
        ],
        None,
    )?;
    match references
        .lines()
        .find_map(|line| line.strip_prefix("refs/heads/main\0"))
    {
        Some("") => Ok(()),
        Some(target) => Err(format!(
            "本地 main 是指向 {target} 的符号引用；拒绝写快照，以免移动用户分支"
        )),
        None => Err("本地 main 缺失、损坏或为悬空符号引用；拒绝写快照".into()),
    }
}

pub fn read_trailer(root: &Path, commit: &str) -> Result<Option<SnapshotTrailer>, String> {
    let root = canonical_root(root)?;
    let oid = read_git(
        &root,
        &[
            "rev-parse",
            "--verify",
            "--end-of-options",
            &format!("{commit}^{{commit}}"),
        ],
    )?;
    let message = read_git(&root, &["show", "--no-patch", "--format=%B", &oid])?;
    parse_trailer(&message)
}

fn parse_trailer(message: &str) -> Result<Option<SnapshotTrailer>, String> {
    if !message
        .trim_end()
        .lines()
        .any(|line| line.trim().is_empty())
        || !crate::recovery_git::has_snapshot_trailer(message)
    {
        return Ok(None);
    }
    match crate::recovery_git::parse_trailer_context(message)
        .map_err(|error| format!("invalid Towermd trailer: {error}"))?
    {
        crate::recovery_git::TrailerContext::Snapshot { seq, epoch } => {
            Ok(Some(SnapshotTrailer { seq, epoch }))
        }
        _ => Ok(None),
    }
}

pub(crate) fn highest_snapshot_fence(
    root: &Path,
    main: &str,
) -> Result<Option<(String, SnapshotTrailer)>, String> {
    let session = Session::for_network(root)?;
    let private = private_revision(root)?;
    highest_fence_across_sources(&session, root, main, private.as_deref())
}

fn highest_fence_across_sources(
    session: &Session,
    root: &Path,
    main: &str,
    private: Option<&str>,
) -> Result<Option<(String, SnapshotTrailer)>, String> {
    let mut highest: Option<(String, SnapshotTrailer)> = None;
    for revision in std::iter::once(main).chain(private) {
        if let Some(candidate) = highest_fence(session, root, revision)? {
            if highest.as_ref().is_none_or(|(_, previous)| {
                (candidate.1.epoch, candidate.1.seq) > (previous.epoch, previous.seq)
            }) {
                highest = Some(candidate);
            }
        }
    }
    Ok(highest)
}

fn highest_fence(
    session: &Session,
    root: &Path,
    base: &str,
) -> Result<Option<(String, SnapshotTrailer)>, String> {
    if session.run(root, &["rev-parse", "--is-shallow-repository"], None)? != "false" {
        return Err("浅克隆无法验证完整 main 历史的 fencing，拒绝写快照".into());
    }
    let history = session.run(
        root,
        &[
            "log",
            "--no-show-signature",
            "--format=%H%x00%B%x00",
            base,
            "--",
        ],
        None,
    )?;
    let mut fields = history.split_terminator('\0');
    let mut highest: Option<(String, SnapshotTrailer)> = None;
    while let Some(commit) = fields.next() {
        let commit = commit.trim();
        if commit.len() != base.len() || !commit.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err("main 历史输出含无效提交标识，拒绝检查 fencing".into());
        }
        let message = fields.next().ok_or("main 历史输出缺少提交消息")?;
        if let Some(trailer) =
            parse_trailer(message).map_err(|error| format!("提交 {commit}：{error}"))?
        {
            if highest.as_ref().is_none_or(|(_, previous)| {
                (trailer.epoch, trailer.seq) > (previous.epoch, previous.seq)
            }) {
                highest = Some((commit.to_string(), trailer));
            }
        }
    }
    Ok(highest)
}

pub(crate) fn fetch_main(session: &Session, root: &Path) -> Result<String, String> {
    let reference = format!("refs/towermd/snapshot-fetch/{}", uuid::Uuid::now_v7());
    let fetched = (|| {
        session.run(
            root,
            &[
                "fetch",
                "--no-tags",
                "--no-recurse-submodules",
                "--no-write-fetch-head",
                "--no-auto-maintenance",
                "--no-write-commit-graph",
                "--no-prune",
                "--refmap=",
                "origin",
                &format!("refs/heads/main:{reference}"),
            ],
            None,
        )?;
        session.run(
            root,
            &["rev-parse", "--verify", &format!("{reference}^{{commit}}")],
            None,
        )
    })();
    let cleanup = session.run(root, &["update-ref", "--no-deref", "-d", &reference], None);
    match (fetched, cleanup) {
        (Ok(commit), Ok(_)) => Ok(commit),
        (Err(error), Ok(_)) => Err(error),
        (result, Err(error)) => Err(format!(
            "临时 fetch ref 清理失败：{error}；fetch 结果：{result:?}"
        )),
    }
}

fn read_git(root: &Path, args: &[&str]) -> Result<String, String> {
    crate::repository_writer::read_git(root, args)
}

fn canonical_root(root: &Path) -> Result<PathBuf, String> {
    let root = root.canonicalize().map_err(|error| error.to_string())?;
    let top = PathBuf::from(read_git(&root, &["rev-parse", "--show-toplevel"])?);
    if top.canonicalize().map_err(|error| error.to_string())? != root {
        return Err("root 必须是 canonical Git toplevel".into());
    }
    Ok(root)
}

fn snapshot_files(request: &WriteRequest) -> Result<BTreeMap<String, String>, String> {
    let docs_dir = normalize_docs_dir(&request.docs_dir)?;
    let state: serde_json::Value = serde_json::from_str(&request.state_json)
        .map_err(|error| format!("state JSON 无效：{error}"))?;
    if state.get("through_seq").and_then(|value| value.as_u64()) != Some(request.through_seq)
        || state.get("writer_epoch").and_then(|value| value.as_u64()) != Some(request.writer_epoch)
    {
        return Err("state JSON 的 through_seq/writer_epoch 与请求不匹配".into());
    }
    let mut files = BTreeMap::from([(
        format!("{docs_dir}/state/state.json"),
        request.state_json.clone(),
    )]);
    for (month, content) in &request.archives {
        let bytes = month.as_bytes();
        if bytes.len() != 7
            || bytes[4] != b'-'
            || !bytes[..4].iter().all(u8::is_ascii_digit)
            || !bytes[5..].iter().all(u8::is_ascii_digit)
            || &month[..4] == "0000"
            || !("01"..="12").contains(&&month[5..])
        {
            return Err(format!("归档键 `{month}` 须为合法 YYYY-MM，不能包含路径"));
        }
        files.insert(
            format!("{docs_dir}/state/events/{month}.jsonl"),
            content.clone(),
        );
    }
    Ok(files)
}

fn message(request: &WriteRequest) -> String {
    format!(
        "Tower.md snapshot\n\nTowermd-Snapshot: {}\nTowermd-Epoch: {}",
        request.through_seq, request.writer_epoch
    )
}

fn main_worktree(session: &Session, root: &Path) -> Result<Option<PathBuf>, String> {
    let listing = session.run(root, &["worktree", "list", "--porcelain", "-z"], None)?;
    let mut worktree = None;
    let mut detached = Vec::new();
    for field in listing.split('\0') {
        if let Some(path) = field.strip_prefix("worktree ") {
            worktree = Some(PathBuf::from(path));
        }
        if field == "branch refs/heads/main" {
            return Ok(worktree);
        }
        if field == "detached" {
            if let Some(path) = &worktree {
                detached.push(path.clone());
            }
        }
    }
    // Git reports a suspended main as detached during rebase and bisect.
    // These files are per-worktree, including for linked worktrees.
    for worktree in detached {
        let git_dir =
            PathBuf::from(session.run(&worktree, &["rev-parse", "--absolute-git-dir"], None)?);
        for marker in [
            "rebase-merge/head-name",
            "rebase-apply/head-name",
            "BISECT_START",
        ] {
            match fs::read(git_dir.join(marker)) {
                Ok(bytes) => {
                    let branch = bytes.strip_suffix(b"\n").unwrap_or(&bytes);
                    if matches!(branch, b"refs/heads/main" | b"main") {
                        return Ok(Some(worktree));
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(format!("无法检查工作树的暂停分支：{error}")),
            }
        }
    }
    Ok(None)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn only_non_fast_forward_push_errors_are_retryable() {
        for reason in ["non-fast-forward", "fetch first"] {
            assert_eq!(
                classify_push(
                    &format!("!\t123:refs/heads/main\t[rejected] ({reason})\n"),
                    ""
                ),
                PushFailure::NonFastForward
            );
        }
        for output in [
            "!\t123:refs/heads/main\t[remote rejected] (protected branch)\n",
            "!\t123:refs/heads/main\t[remote rejected] (pre-receive hook declined)\n",
            "!\t123:refs/heads/other\t[rejected] (fetch first)\n",
            "!\t123:refs/heads/main\t[rejected] (unknown)\n",
            "!\t123:refs/heads/main\t[rejected] (fetch first)\n!\t456:refs/heads/other\t[rejected] (fetch first)\n",
            "fatal: Authentication failed\n",
        ] {
            assert!(matches!(classify_push(output, "transport error"), PushFailure::Other(_)));
        }
    }
    pub(crate) type OperationHook = std::sync::Arc<dyn Fn(&[&str]) + Send + Sync>;
    pub(crate) fn set_hook(root: PathBuf, hook: Option<OperationHook>) {
        let mut hooks = OPERATIONS.get_or_init(Default::default).lock().unwrap();
        if let Some(hook) = hook {
            hooks.insert(root, hook);
        } else {
            hooks.remove(&root);
        }
    }
    static OPERATIONS: std::sync::OnceLock<std::sync::Mutex<BTreeMap<PathBuf, OperationHook>>> =
        std::sync::OnceLock::new();

    pub(crate) fn observe(root: &Path, args: &[&str]) {
        let hook = OPERATIONS
            .get_or_init(Default::default)
            .lock()
            .unwrap()
            .get(root)
            .cloned();
        if let Some(hook) = hook {
            hook(args);
        }
    }

    fn git(root: &Path, args: &[&str]) -> String {
        let output = std::process::Command::new("git")
            .current_dir(root)
            .args([
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.invalid",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap().trim().into()
    }

    #[test]
    fn q1_same_base_materializes_catalog_once() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::{Arc, Mutex};
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        git(&root, &["init", "-qb", "main"]);
        crate::init::init(&root, &crate::init::InitOptions { docs_dir: None }).unwrap();
        git(&root, &["add", "."]);
        git(&root, &["commit", "-qm", "same-base catalog fixture"]);
        git(&root, &["switch", "-qc", "q1-snapshot"]);
        let catalog = crate::catalog::Catalog::load(&root).unwrap();
        let inputs =
            crate::recovery_outbox::RecoveryInputs::read(&root.join("outbox"), &catalog).unwrap();
        let hub = Mutex::new(
            crate::project_session::ProjectSession::new(
                Box::new(crate::hub::Hub::open(&root.join(".git/towermd")).unwrap()),
                &catalog,
                &inputs,
                1_000,
            )
            .unwrap(),
        );
        let count = Arc::new(AtomicUsize::new(0));
        let observed = count.clone();
        let revision = catalog.revision.clone();
        set_hook(
            root.clone(),
            Some(Arc::new(move |args| {
                if args[0] == "catalog-materialized" {
                    assert_eq!(args[1], revision);
                    observed.fetch_add(1, Ordering::SeqCst);
                }
            })),
        );
        let result = crate::snapshot_service::SnapshotWriter::default().write(
            &hub,
            &catalog,
            "q1-host",
            true,
            || Ok(1_000),
        );
        set_hook(root, None);
        assert_eq!(result.unwrap().status, "written");
        let actual = count.load(Ordering::SeqCst);
        eprintln!("Q1_ACTUAL_CATALOG_MATERIALIZATIONS same_base={actual}");
        assert_eq!(
            actual, 1,
            "the actual initial/fresh same-OID Catalog must be parsed once in this write attempt"
        );
    }

    #[test]
    fn q1_same_base_reuse_rejects_actual_common_and_blob_io_changes() {
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;
        for mutation in ["common", "config", "module"] {
            let temp = tempfile::tempdir().unwrap();
            let root = temp.path().canonicalize().unwrap().join("repo");
            fs::create_dir(&root).unwrap();
            git(&root, &["init", "-qb", "main"]);
            crate::init::init(&root, &crate::init::InitOptions { docs_dir: None }).unwrap();
            fs::write(
                root.join("docs/tower/arch/A.md"),
                "---\ntype: Module\nid: A\npaths: [src/**]\n---\n",
            )
            .unwrap();
            git(&root, &["add", "."]);
            git(&root, &["commit", "-qm", "actual binding fixture"]);
            git(&root, &["switch", "-qc", "q1-snapshot"]);
            let request = WriteRequest {
                base: git(&root, &["rev-parse", "main"]),
                docs_dir: "docs/tower".into(),
                state_json: r#"{"through_seq":7,"writer_epoch":2}"#.into(),
                archives: BTreeMap::new(),
                through_seq: 7,
                writer_epoch: 2,
            };
            let refs = git(&root, &["show-ref"]);
            let held = temp.path().join("held");
            let actual = if mutation == "common" {
                let foreign = temp.path().join("foreign");
                git(
                    &root,
                    &[
                        "clone",
                        "--no-hardlinks",
                        "-q",
                        ".",
                        foreign.to_str().unwrap(),
                    ],
                );
                git(&foreign, &["update-ref", "refs/heads/main", &request.base]);
                assert_eq!(git(&foreign, &["rev-parse", "main"]), request.base);
                foreign.join(".git")
            } else {
                let path = if mutation == "config" {
                    ".towermd/config.yaml"
                } else {
                    "docs/tower/arch/A.md"
                };
                let oid = git(&root, &["rev-parse", &format!("{}:{path}", request.base)]);
                root.join(".git/objects").join(&oid[..2]).join(&oid[2..])
            };
            let changed = Arc::new(AtomicBool::new(false));
            let entered = changed.clone();
            let directory = root.clone();
            let saved = held.clone();
            let object = actual.clone();
            set_hook(
                root.clone(),
                Some(Arc::new(move |args| {
                    if args[0] == "catalog-materialized" && !entered.swap(true, Ordering::SeqCst) {
                        if mutation == "common" {
                            fs::rename(directory.join(".git"), &saved).unwrap();
                            fs::write(
                                directory.join(".git"),
                                format!("gitdir: {}\n", object.display()),
                            )
                            .unwrap();
                        } else {
                            fs::rename(&object, &saved).unwrap();
                        }
                    }
                })),
            );
            let result = write_snapshot(&root, &request, || Ok(()));
            set_hook(root.clone(), None);
            assert!(
                changed.load(Ordering::SeqCst),
                "actual initial Catalog must precede mutation"
            );
            if mutation == "common" {
                fs::remove_file(root.join(".git")).unwrap();
                fs::rename(&held, root.join(".git")).unwrap();
            } else {
                fs::rename(&held, &actual).unwrap();
            }
            let error = result.unwrap_err();
            eprintln!("Q1_ACTUAL_REUSE_REJECTION mutation={mutation} error={error}");
            if mutation == "common" {
                assert!(error.contains("context differs"), "{error}");
            } else {
                assert!(
                    error.contains("cat-file")
                        || (error.contains("无法解析 git ls-tree 记录") && error.contains("BAD")),
                    "real missing blob must retain the actual Git inspection error: {error}"
                );
            }
            assert_eq!(git(&root, &["show-ref"]), refs);
        }
    }

    #[test]
    fn q1_changed_base_still_materializes_fresh_catalog_and_refuses_stale_local_base() {
        use std::sync::{Arc, Mutex};
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        git(&root, &["init", "-qb", "main"]);
        crate::init::init(&root, &crate::init::InitOptions { docs_dir: None }).unwrap();
        git(&root, &["add", "."]);
        git(&root, &["commit", "-qm", "changed base fixture"]);
        git(&root, &["switch", "-qc", "q1-snapshot"]);
        let base = git(&root, &["rev-parse", "main"]);
        let tree = git(&root, &["rev-parse", "main^{tree}"]);
        let winner = git(
            &root,
            &["commit-tree", &tree, "-p", &base, "-m", "ordinary advance"],
        );
        let request = WriteRequest {
            base: base.clone(),
            docs_dir: "docs/tower".into(),
            state_json: r#"{"through_seq":7,"writer_epoch":2}"#.into(),
            archives: BTreeMap::new(),
            through_seq: 7,
            writer_epoch: 2,
        };
        let materialized = Arc::new(Mutex::new(Vec::new()));
        let observed = materialized.clone();
        let directory = root.clone();
        let advanced = winner.clone();
        set_hook(
            root.clone(),
            Some(Arc::new(move |args| {
                if args[0] == "catalog-materialized" {
                    let mut revisions = observed.lock().unwrap();
                    revisions.push(args[1].to_owned());
                    if revisions.len() == 1 {
                        git(&directory, &["update-ref", "refs/heads/main", &advanced]);
                    }
                }
            })),
        );
        let result = write_snapshot(&root, &request, || Ok(()));
        set_hook(root.clone(), None);
        assert!(result.unwrap_err().contains("main 已推进"));
        assert_eq!(*materialized.lock().unwrap(), vec![base, winner.clone()]);
        assert_eq!(git(&root, &["rev-parse", "main"]), winner);
    }

    fn q1_context_fixture() -> (tempfile::TempDir, crate::catalog::Catalog) {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        git(&root, &["init", "-qb", "main"]);
        crate::init::init(&root, &crate::init::InitOptions { docs_dir: None }).unwrap();
        fs::write(
            root.join("docs/tower/arch/A.md"),
            "---\ntype: Module\nid: A\npaths: [src/**]\n---\n",
        )
        .unwrap();
        git(&root, &["add", "."]);
        git(&root, &["commit", "-qm", "context read fixture"]);
        let catalog = crate::catalog::Catalog::load(&root).unwrap();
        assert_eq!(catalog.protected_paths, vec!["docs/tower/arch/A.md"]);
        (temp, catalog)
    }

    #[test]
    fn q1_context_exact_input_reads_committed_file_once() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;
        let (_temp, catalog) = q1_context_fixture();
        let count = Arc::new(AtomicUsize::new(0));
        let observed = count.clone();
        set_hook(
            catalog.root.clone(),
            Some(Arc::new(move |args| {
                if args[0] == "context-file-read" {
                    observed.fetch_add(1, Ordering::SeqCst);
                }
            })),
        );
        let result = validate_context(Some(&catalog), Some(&catalog));
        set_hook(catalog.root.clone(), None);
        result.unwrap();
        let reads = count.load(Ordering::SeqCst);
        eprintln!("Q1_ACTUAL_COMMITTED_FILE_READS exact_input={reads}");
        assert_eq!(
            reads, 1,
            "one validate_context must read the identical committed input once"
        );
    }

    #[test]
    fn q1_context_different_revision_reads_both_committed_inputs() {
        use std::sync::{Arc, Mutex};
        let (_temp, previous) = q1_context_fixture();
        git(
            &previous.root,
            &["commit", "--allow-empty", "-qm", "ordinary context advance"],
        );
        let current = crate::catalog::Catalog::load(&previous.root).unwrap();
        assert_ne!(previous.revision, current.revision);
        let reads = Arc::new(Mutex::new(Vec::new()));
        let observed = reads.clone();
        set_hook(
            previous.root.clone(),
            Some(Arc::new(move |args| {
                if args[0] == "context-file-read" {
                    observed
                        .lock()
                        .unwrap()
                        .push((args[1].to_string(), args[2].to_string()));
                }
            })),
        );
        let result = validate_context(Some(&previous), Some(&current));
        set_hook(previous.root.clone(), None);
        result.unwrap();
        assert_eq!(
            *reads.lock().unwrap(),
            vec![
                (previous.revision, "docs/tower/arch/A.md".into()),
                (current.revision, "docs/tower/arch/A.md".into()),
            ]
        );
    }

    #[test]
    fn q1_context_first_actual_missing_or_corrupt_blob_still_refuses() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;
        for corrupt in [false, true] {
            let (_temp, catalog) = q1_context_fixture();
            let oid = git(
                &catalog.root,
                &[
                    "rev-parse",
                    &format!("{}:docs/tower/arch/A.md", catalog.revision),
                ],
            );
            let object = catalog
                .common_dir
                .join("objects")
                .join(&oid[..2])
                .join(&oid[2..]);
            let bytes = fs::read(&object).unwrap();
            fs::remove_file(&object).unwrap();
            if corrupt {
                fs::write(&object, b"corrupt real Git object").unwrap();
            }
            let reads = Arc::new(AtomicUsize::new(0));
            let observed = reads.clone();
            set_hook(
                catalog.root.clone(),
                Some(Arc::new(move |args| {
                    if args[0] == "context-file-read" {
                        observed.fetch_add(1, Ordering::SeqCst);
                    }
                })),
            );
            let result = validate_context(Some(&catalog), Some(&catalog));
            set_hook(catalog.root.clone(), None);
            fs::write(&object, bytes).unwrap();
            let error = result.unwrap_err();
            eprintln!(
                "Q1_ACTUAL_FIRST_READ_FAILURE corrupt={corrupt} reads={} error={error}",
                reads.load(Ordering::SeqCst)
            );
            assert_eq!(reads.load(Ordering::SeqCst), 1);
            assert!(
                error.contains("cat-file") || error.contains("BAD") || error.contains("corrupt"),
                "{error}"
            );
        }
    }

    #[test]
    fn q1_absent_private_ref_uses_one_actual_material_git_probe() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        git(&root, &["init", "-qb", "main"]);
        let count = Arc::new(AtomicUsize::new(0));
        let observed = count.clone();
        set_hook(
            root.clone(),
            Some(Arc::new(move |args| {
                if args[0] == "symbolic-ref"
                    || args[0] == "for-each-ref"
                    || (args[0] == "show-ref" && args.get(1) == Some(&"--exists"))
                {
                    observed.fetch_add(1, Ordering::SeqCst);
                }
            })),
        );
        let result = private_revision(&root);
        set_hook(root, None);
        assert_eq!(result.unwrap(), None);
        let actual = count.load(Ordering::SeqCst);
        eprintln!("Q1_ACTUAL_PRIVATE_MATERIAL_PROBES absent={actual}");
        let expected = if private_ref_exists_supported(temp.path()).unwrap() {
            1
        } else {
            2
        };
        assert_eq!(
            actual, expected,
            "every observation needs actual material Git I/O"
        );
    }

    #[cfg(unix)]
    #[test]
    fn q1_private_ref_capability_and_error_boundaries() {
        use std::os::unix::fs::{symlink, PermissionsExt};
        use std::process::Command;
        let Ok(case) = std::env::var("Q1_PRIVATE_PROBE_CASE") else {
            for case in [
                "legacy",
                "modern",
                "switch",
                "help-failure",
                "help-invalid",
                "spawn-failure",
                "lookup",
                "diagnostic",
                "stdout",
                "filesystem",
            ] {
                let output = Command::new(std::env::current_exe().unwrap())
                    .args([
                        "--exact",
                        "snapshot_git::tests::q1_private_ref_capability_and_error_boundaries",
                        "--nocapture",
                    ])
                    .env("Q1_PRIVATE_PROBE_CASE", case)
                    .output()
                    .unwrap();
                eprintln!(
                    "Q1_PRIVATE_CHILD {case}\n{}{}",
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                );
                assert!(output.status.success(), "case {case}: {}", output.status);
            }
            return;
        };
        // Only this exact child changes PATH. All fixture Git operations precede it.
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        git(&root, &["init", "-qb", "main"]);
        fs::write(root.join("code"), "code").unwrap();
        git(&root, &["add", "."]);
        git(&root, &["commit", "-qm", "initial"]);
        let commit = git(&root, &["rev-parse", "HEAD"]);
        let blob = git(&root, &["rev-parse", "HEAD:code"]);
        git(&root, &["tag", "-a", "annotated", "-m", "tag"]);
        let tag = git(&root, &["rev-parse", "annotated"]);
        let real = Command::new("sh")
            .args(["-c", "command -v git"])
            .output()
            .unwrap();
        assert!(real.status.success());
        let real = String::from_utf8(real.stdout).unwrap().trim().to_owned();
        let bin = root.join("bin");
        fs::create_dir(&bin).unwrap();
        let wrapper = bin.join("git");
        let mode = root.join("mode");
        let log = root.join("probe.log");
        fs::write(
            &mode,
            match case.as_str() {
                "legacy" => "legacy",
                "help-failure" => "help-failure",
                "help-invalid" => "help-invalid",
                _ => "real",
            },
        )
        .unwrap();
        let script = r#"#!/bin/sh
mode=$(/bin/cat "$Q1_GIT_MODE_FILE")
case "$*" in
  *' show-ref -h')
    echo help >> "$Q1_GIT_LOG"
    case "$mode" in
      legacy) echo 'usage: git show-ref [--head]' ; exit 129 ;;
      help-failure) echo 'fatal: actual help I/O failure' >&2 ; exit 128 ;;
      help-invalid) echo 'fatal: not a help document' >&2 ; exit 129 ;;
    esac ;;
  *' show-ref --exists refs/towermd/snapshot')
    echo exists >> "$Q1_GIT_LOG"
    case "$mode" in
      unsupported) echo 'error: unknown option exists' >&2 ; exit 129 ;;
      lookup) echo 'error: reference lookup failed' >&2 ; exit 1 ;;
      diagnostic) echo 'fatal: actual ref I/O failure' >&2 ; exit 2 ;;
      stdout) echo 'unexpected data' ; exit 2 ;;
    esac ;;
  *' for-each-ref '*)
    case "$mode" in
      typed-nonzero) "$Q1_GIT_REAL" "$@" ; echo 'fatal: post-output type failure' >&2 ; exit 1 ;;
      typed-unsupported) echo 'error: unsupported exclude' >&2 ; exit 129 ;;
      typed-empty-oid) printf 'refs/towermd/snapshot\000\000\000commit\n' ; exit 0 ;;
      typed-missing-field) printf 'refs/towermd/snapshot\000%s\000\n' "$Q1_GIT_COMMIT" ; exit 0 ;;
      typed-extra-field) printf 'refs/towermd/snapshot\000%s\000\000commit\000extra\n' "$Q1_GIT_COMMIT" ; exit 0 ;;
      typed-spaced) printf 'refs/towermd/snapshot\000%s\000\000commit \n' "$Q1_GIT_COMMIT" ; exit 0 ;;
      typed-cr) printf 'refs/towermd/snapshot\000%s\000\000commit\r\n' "$Q1_GIT_COMMIT" ; exit 0 ;;
      typed-cr-interior) printf 'refs/towermd/snapshot\000%s\000\000commit\r\nunexpected trailing line\n' "$Q1_GIT_COMMIT" ; exit 0 ;;
    esac ;;
esac
exec "$Q1_GIT_REAL" "$@"
"#;
        fs::write(&wrapper, script).unwrap();
        fs::set_permissions(&wrapper, fs::Permissions::from_mode(0o755)).unwrap();
        std::env::set_var("Q1_GIT_MODE_FILE", &mode);
        std::env::set_var("Q1_GIT_LOG", &log);
        std::env::set_var("Q1_GIT_REAL", real);
        std::env::set_var("Q1_GIT_COMMIT", &commit);
        let original_path = std::env::var_os("PATH").unwrap();
        let mut path = vec![bin.clone()];
        path.extend(std::env::split_paths(&original_path));
        std::env::set_var("PATH", std::env::join_paths(path).unwrap());
        if case == "spawn-failure" {
            fs::remove_file(&wrapper).unwrap();
            std::env::set_var("PATH", &bin);
            assert!(private_revision(&root).unwrap_err().contains("git"));
            fs::write(&wrapper, script).unwrap();
            fs::set_permissions(&wrapper, fs::Permissions::from_mode(0o755)).unwrap();
            assert_eq!(private_revision(&root).unwrap(), None);
            return;
        }
        if case == "help-failure" || case == "help-invalid" {
            assert!(private_revision(&root).is_err());
            fs::write(&mode, "real").unwrap();
            assert_eq!(private_revision(&root).unwrap(), None);
            let lines = fs::read_to_string(&log).unwrap();
            assert_eq!(
                lines.lines().filter(|line| *line == "help").count(),
                2,
                "failed help must not install capability"
            );
            return;
        }
        assert_eq!(private_revision(&root).unwrap(), None);
        if case == "legacy" {
            fs::write(&mode, "unsupported").unwrap();
            assert_eq!(private_revision(&root).unwrap(), None);
            assert_eq!(
                fs::read_to_string(&log).unwrap(),
                "help\n",
                "false capability preserves the complete legacy path"
            );
        }
        if matches!(case.as_str(), "switch" | "lookup" | "diagnostic" | "stdout") {
            fs::write(
                &mode,
                if case == "switch" {
                    "unsupported"
                } else {
                    &case
                },
            )
            .unwrap();
            assert!(
                private_revision(&root).is_err(),
                "{case} must fail closed after capability installation"
            );
            assert_eq!(fs::read_to_string(&log).unwrap(), "help\nexists\nexists\n");
            return;
        }
        let reference = root.join(".git/refs/towermd/snapshot");
        fs::create_dir_all(reference.parent().unwrap()).unwrap();
        fs::write(&reference, format!("{commit}\n")).unwrap();
        let material = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let observed = material.clone();
        set_hook(
            root.clone(),
            Some(std::sync::Arc::new(move |args| {
                if matches!(
                    args[0],
                    "show-ref" | "for-each-ref" | "cat-file" | "symbolic-ref"
                ) {
                    observed.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                }
            })),
        );
        assert_eq!(private_revision(&root).unwrap(), Some(commit.clone()));
        set_hook(root.clone(), None);
        let actual = material.load(std::sync::atomic::Ordering::SeqCst);
        eprintln!("Q1_ACTUAL_PRIVATE_MATERIAL_PROBES existing={actual}");
        assert_eq!(
            actual,
            if case == "legacy" { 3 } else { 2 },
            "each observation reads the exact ref and actual object type"
        );
        if case != "legacy" {
            for failure in [
                "typed-nonzero",
                "typed-unsupported",
                "typed-empty-oid",
                "typed-missing-field",
                "typed-extra-field",
                "typed-spaced",
                "typed-cr",
                "typed-cr-interior",
            ] {
                fs::write(&mode, failure).unwrap();
                let result = private_revision(&root);
                eprintln!("Q1_TYPED_OUTPUT mode={failure} result={result:?}");
                assert!(
                    result.is_err(),
                    "{failure} must reject malformed or failed type material"
                );
            }
            fs::write(&mode, "real").unwrap();
        }
        for invalid in [
            format!("{blob}\n"),
            format!("{tag}\n"),
            "1111111111111111111111111111111111111111\n".into(),
            "ref: refs/heads/main\n".into(),
            "ref: refs/heads/dangling\n".into(),
            "invalid ref\n".into(),
        ] {
            fs::write(&reference, &invalid).unwrap();
            let result = private_revision(&root);
            if case == "legacy" && invalid == "invalid ref\n" {
                assert_eq!(
                    result.unwrap(),
                    None,
                    "legacy ignores an invalid loose ref omitted by foreach"
                );
            } else {
                assert!(
                    result.is_err(),
                    "case={case} ref={invalid:?} result={result:?}"
                );
            }
        }
        fs::write(&reference, format!("{commit}\n")).unwrap();
        let object = root
            .join(".git/objects")
            .join(&commit[..2])
            .join(&commit[2..]);
        let bytes = fs::read(&object).unwrap();
        fs::remove_file(&object).unwrap();
        assert!(private_revision(&root).is_err(), "real missing object");
        fs::write(&object, b"corrupt object").unwrap();
        assert!(private_revision(&root).is_err(), "real corrupt object");
        fs::write(&object, bytes).unwrap();
        fs::remove_file(&reference).unwrap();
        if case == "filesystem" {
            symlink("refs/heads/dangling", &reference).unwrap();
            assert!(private_revision(&root).is_err());
            fs::remove_file(&reference).unwrap();
            symlink(root.join("outside-missing"), &reference).unwrap();
            assert_eq!(
                private_revision(&root).unwrap(),
                None,
                "inherits Git/legacy dangling OS path semantics"
            );
            fs::remove_file(&reference).unwrap();
            fs::create_dir(&reference).unwrap();
            fs::write(reference.join("child"), format!("{commit}\n")).unwrap();
            assert_eq!(
                private_revision(&root).unwrap(),
                None,
                "legacy also ignores nonexact child records"
            );
            fs::remove_dir_all(&reference).unwrap();
            fs::write(root.join(".git/packed-refs"), "broken packed ref\n").unwrap();
            assert!(
                private_revision(&root).is_err(),
                "real packed-ref I/O/parse failure"
            );
        }
        assert_eq!(
            fs::read_to_string(&log)
                .unwrap()
                .lines()
                .filter(|line| *line == "help")
                .count(),
            1
        );
    }

    #[test]
    fn q1_exact_private_type_excludes_descendants_and_rejects_races() {
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;
        let (_temp, catalog) = q1_context_fixture();
        let root = &catalog.root;
        if !private_ref_exists_supported(root).unwrap() {
            eprintln!("Q1_EXACT_TYPE modern query unavailable; legacy boundary is checked in the fresh child");
            return;
        }
        let commit = catalog.revision.clone();
        let input = root.join("orphan-input");
        fs::write(&input, "independent descendant object").unwrap();
        let orphan = git(
            root,
            &["hash-object", "--no-filters", "-w", input.to_str().unwrap()],
        );
        let object = catalog
            .common_dir
            .join("objects")
            .join(&orphan[..2])
            .join(&orphan[2..]);
        let bytes = fs::read(&object).unwrap();
        let packed = catalog.common_dir.join("packed-refs");
        for descendant in ["child", "child/grandchild"] {
            for corrupt in [false, true] {
                if object.exists() {
                    fs::remove_file(&object).unwrap();
                }
                if corrupt {
                    fs::write(&object, b"actual corrupt independent object").unwrap();
                }
                fs::write(&packed, format!("# pack-refs with: peeled fully-peeled sorted\n{commit} {PRIVATE_REF}\n{orphan} {PRIVATE_REF}/{descendant}\n")).unwrap();
                assert_eq!(private_revision(root).unwrap(), Some(commit.clone()));
                eprintln!("Q1_EXACT_TYPE descendant={descendant} corrupt={corrupt} exact=Some");
            }
        }
        fs::remove_file(&object).unwrap();
        fs::write(&object, &bytes).unwrap();
        let changed = Arc::new(AtomicBool::new(false));
        let observed = changed.clone();
        let mutate = packed.clone();
        let remaining = format!("# pack-refs with: peeled fully-peeled sorted\n{orphan} {PRIVATE_REF}/child/grandchild\n");
        set_hook(
            root.clone(),
            Some(Arc::new(move |args| {
                if args[0] == "for-each-ref" && args.last() == Some(&PRIVATE_REF) {
                    assert!(!observed.swap(true, Ordering::SeqCst));
                    fs::write(&mutate, &remaining).unwrap();
                }
            })),
        );
        let result = private_revision(root);
        set_hook(root.clone(), None);
        assert!(changed.load(Ordering::SeqCst));
        assert!(
            result.unwrap_err().contains("精确引用"),
            "exists0 followed by real exact deletion must never become None"
        );
        // Type metadata is not a full commit or snapshot-content proof.
        fs::write(&input, "invalid commit body").unwrap();
        let malformed = git(
            root,
            &[
                "hash-object",
                "--literally",
                "-t",
                "commit",
                "-w",
                input.to_str().unwrap(),
            ],
        );
        fs::write(
            &packed,
            format!("# pack-refs with: peeled fully-peeled sorted\n{malformed} {PRIVATE_REF}\n"),
        )
        .unwrap();
        assert_eq!(git(root, &["cat-file", "-t", &malformed]), "commit");
        assert_eq!(private_revision(root).unwrap(), Some(malformed));
        assert!(
            crate::snapshot_service::read_snapshot(&catalog).is_err(),
            "the unchanged real snapshot reader must reject the invalid body"
        );
        fs::write(
            &packed,
            format!("# pack-refs with: peeled fully-peeled sorted\n{commit} {PRIVATE_REF}\n"),
        )
        .unwrap();
        git(root, &["gc", "--prune=now"]);
        let loose_commit = catalog
            .common_dir
            .join("objects")
            .join(&commit[..2])
            .join(&commit[2..]);
        assert!(
            !loose_commit.exists(),
            "packed test must not fall back to a loose copy"
        );
        assert_eq!(private_revision(root).unwrap(), Some(commit.clone()));
        let packs: Vec<_> = fs::read_dir(catalog.common_dir.join("objects/pack"))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "pack"))
            .collect();
        assert!(!packs.is_empty());
        for pack in &packs {
            fs::rename(pack, pack.with_extension("held")).unwrap();
        }
        let missing = private_revision(root);
        for pack in &packs {
            fs::rename(pack.with_extension("held"), pack).unwrap();
        }
        assert!(
            missing.is_err(),
            "actual packed object source is unavailable"
        );
        assert_eq!(private_revision(root).unwrap(), Some(commit));
        eprintln!("Q1_PACKED_TYPE actual_packs={} missing_pack_rejected=true body_only_metadata_not_fsck=true", packs.len());
    }

    #[test]
    fn real_git_attempts_rebuild_and_push_at_most_three_times() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::{Arc, Mutex};
        for races in [1, 3] {
            let temp = tempfile::tempdir().unwrap();
            let root = temp.path().canonicalize().unwrap().join("repo");
            fs::create_dir(&root).unwrap();
            git(&root, &["init", "-qb", "main"]);
            fs::write(root.join("code"), "code").unwrap();
            git(&root, &["add", "."]);
            git(&root, &["commit", "-qm", "initial"]);
            let base = git(&root, &["rev-parse", "main"]);
            let remote = temp.path().join("remote.git");
            git(&root, &["clone", "--bare", ".", remote.to_str().unwrap()]);
            git(
                &root,
                &["remote", "add", "origin", remote.to_str().unwrap()],
            );
            git(&root, &["fetch", "origin"]);
            let counts = Arc::new([
                AtomicUsize::new(0),
                AtomicUsize::new(0),
                AtomicUsize::new(0),
            ]);
            let parents = Arc::new(Mutex::new(Vec::new()));
            let observer_counts = counts.clone();
            let observer_parents = parents.clone();
            let observer_root = root.clone();
            let winner = Arc::new(Mutex::new(String::new()));
            let observer_winner = winner.clone();
            OPERATIONS
                .get_or_init(Default::default)
                .lock()
                .unwrap()
                .insert(
                    root.clone(),
                    Arc::new(move |args| match args[0] {
                        "fetch" => {
                            observer_counts[0].fetch_add(1, Ordering::SeqCst);
                        }
                        "commit-tree" => {
                            observer_counts[1].fetch_add(1, Ordering::SeqCst);
                            observer_parents.lock().unwrap().push(args[3].to_string());
                        }
                        "push" => {
                            let attempt = observer_counts[2].fetch_add(1, Ordering::SeqCst) + 1;
                            if attempt <= races {
                                let parent = git(&observer_root, &["rev-parse", "origin/main"]);
                                let tree = git(
                                    &observer_root,
                                    &["rev-parse", &format!("{parent}^{{tree}}")],
                                );
                                let commit = git(
                                    &observer_root,
                                    &[
                                        "commit-tree",
                                        &tree,
                                        "-p",
                                        &parent,
                                        "-m",
                                        &format!("winner {attempt}"),
                                    ],
                                );
                                git(
                                    &observer_root,
                                    &["push", "origin", &format!("{commit}:main")],
                                );
                                *observer_winner.lock().unwrap() = commit;
                            }
                        }
                        _ => {}
                    }),
                );
            let request = WriteRequest {
                base: base.clone(),
                docs_dir: "docs/tower".into(),
                state_json: r#"{"through_seq":7,"writer_epoch":2}"#.into(),
                archives: BTreeMap::new(),
                through_seq: 7,
                writer_epoch: 2,
            };
            let mut lease_checks = 0;
            let result = write_snapshot(&root, &request, || {
                lease_checks += 1;
                Ok(())
            });
            OPERATIONS.get().unwrap().lock().unwrap().remove(&root);
            let attempts = if races == 1 { 2 } else { 3 };
            assert_eq!(
                counts
                    .iter()
                    .map(|n| n.load(Ordering::SeqCst))
                    .collect::<Vec<_>>(),
                vec![attempts; 3]
            );
            assert_eq!(lease_checks, 1 + 2 * attempts);
            assert_eq!(parents.lock().unwrap()[0], base);
            if races == 1 {
                let commit = match result.unwrap() {
                    WriteOutcome::Written { commit } => commit,
                    other => panic!("{other:?}"),
                };
                assert_eq!(
                    git(&root, &["rev-parse", &format!("{commit}^1")]),
                    *winner.lock().unwrap()
                );
            } else {
                assert!(result.unwrap_err().contains("三次竞争失败"));
            }
        }
    }
    #[test]
    fn draft_reference_is_persistent_without_touching_checked_out_main() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        git(root, &["init", "-qb", "main"]);
        fs::write(root.join("README.md"), "keep\n").unwrap();
        git(root, &["add", "README.md"]);
        git(root, &["commit", "-qm", "fixture"]);
        let before = git(root, &["rev-parse", "HEAD"]);
        let status = git(root, &["status", "--porcelain=v1"]);
        let index = fs::read(root.join(".git/index")).unwrap();
        let files = BTreeMap::from([(
            "docs/tower/arch/A.md".into(),
            "---\ntype: Module\nid: A\n---\nA\n".into(),
        )]);
        let revision = crate::repository_writer::save_draft_tree_for_test(
            root, "draft-1", &before, &files, None,
        )
        .unwrap();
        assert_eq!(git(root, &["rev-parse", "HEAD"]), before);
        assert_eq!(git(root, &["status", "--porcelain=v1"]), status);
        assert_eq!(fs::read(root.join(".git/index")).unwrap(), index);
        assert!(!root.join("docs/tower/arch/A.md").exists());
        assert_eq!(
            git(root, &["rev-parse", "refs/towermd/drafts/draft-1"]),
            revision
        );
        assert!(git(
            root,
            &["show", "refs/towermd/drafts/draft-1:docs/tower/arch/A.md"]
        )
        .contains("type: Module"));
    }
}
