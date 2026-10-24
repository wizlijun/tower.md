//! One Git publication path for the daemon. Callers retain the live main lease;
//! this module never checks out a branch or uses a user's index.
use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::{config::Config, init::normalize_docs_dir, repo};

const TIMEOUT: Duration = Duration::from_secs(30);
pub const SNAPSHOT_REF: &str = "refs/towermd/snapshot";

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Boundary {
    pub epoch: u64,
    pub seq: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot {
    pub commit: String,
    pub boundary: Boundary,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PublicationReadback {
    pub tag: String,
    pub commit: String,
    pub main_revision: String,
}

#[derive(Clone, Debug)]
pub enum WriteKind {
    Snapshot {
        seq: u64,
        /// Exact private snapshot being moved to main, with unchanged state files.
        promote_from: Option<String>,
    },
    Slow {
        expected_base: String,
        /// A full baseline tag name, such as refs/tags/towermd/B0.
        tag: Option<String>,
    },
    /// Fast-forward main to an already built commit that descends from
    /// `expected_base`. The caller proves what the commit may contain; this
    /// path only refuses snapshot paths and never rewrites the commit.
    Advance {
        expected_base: String,
        commit: String,
    },
}

#[derive(Clone, Debug)]
pub struct WriteRequest {
    pub docs_dir: String,
    pub epoch: u64,
    pub kind: WriteKind,
    /// Repository-relative ordinary files. None removes a file.
    pub changes: BTreeMap<String, Option<Vec<u8>>>,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WriteOutcome {
    Written {
        commit: String,
        reference: String,
        tag: Option<String>,
    },
    Superseded {
        commit: String,
        boundary: Boundary,
    },
}

fn git(
    root: &Path,
    args: &[&str],
    index: Option<&Path>,
    input: Option<&[u8]>,
    snapshot: bool,
) -> Result<std::process::Output, String> {
    let null = if cfg!(windows) { "NUL" } else { "/dev/null" };
    let hooks = format!("core.hooksPath={null}");
    let mut options = vec![
        "-c",
        &hooks,
        "-c",
        "commit.gpgsign=false",
        "-c",
        "core.fsmonitor=false",
        "-c",
        "trailer.separators=:",
    ];
    options.extend(args);
    let actor = if snapshot {
        "process:towermd-snapshot"
    } else {
        "process:towermd"
    };
    let mut env = vec![
        ("GIT_AUTHOR_NAME", OsStr::new(actor)),
        ("GIT_AUTHOR_EMAIL", OsStr::new("towermd@localhost")),
        ("GIT_COMMITTER_NAME", OsStr::new(actor)),
        ("GIT_COMMITTER_EMAIL", OsStr::new("towermd@localhost")),
    ];
    if let Some(index) = index {
        env.push(("GIT_INDEX_FILE", index.as_os_str()));
    }
    repo::git_timeout_env_input_output(root, &options, TIMEOUT, &env, input)
}

fn checked(
    root: &Path,
    args: &[&str],
    index: Option<&Path>,
    input: Option<&[u8]>,
    snapshot: bool,
) -> Result<Vec<u8>, String> {
    let output = git(root, args, index, input, snapshot)?;
    if !output.status.success() {
        return Err(format!(
            "主干 Git 写入失败：{}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(output.stdout)
}

fn oid(bytes: &[u8]) -> Result<String, String> {
    let value = std::str::from_utf8(bytes)
        .map_err(|e| e.to_string())?
        .trim();
    if !matches!(value.len(), 40 | 64) || !value.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("Git 对象 ID 无效".into());
    }
    Ok(value.into())
}

fn reference(root: &Path, name: &str) -> Result<Option<String>, String> {
    let bytes = checked(
        root,
        &["for-each-ref", "--format=%(refname) %(objectname)", name],
        None,
        None,
        false,
    )?;
    let value = String::from_utf8(bytes).map_err(|e| e.to_string())?;
    for line in value.lines() {
        if let Some((actual, sha)) = line.split_once(' ') {
            if actual == name {
                return oid(sha.as_bytes()).map(Some);
            }
        }
    }
    Ok(None)
}

fn has_origin(root: &Path) -> Result<bool, String> {
    let bytes = checked(root, &["remote"], None, None, false)?;
    Ok(bytes.split(|b| *b == b'\n').any(|name| name == b"origin"))
}

fn main_ref(origin: bool) -> &'static str {
    if origin {
        "refs/remotes/origin/main"
    } else {
        "refs/heads/main"
    }
}

fn valid_baseline_tag(tag: &str) -> Result<(), String> {
    let number = tag
        .strip_prefix("refs/tags/towermd/B")
        .ok_or("基线 tag 无效")?;
    let parsed: u64 = number.parse().map_err(|_| "基线 tag 无效")?;
    if number != parsed.to_string() {
        return Err("基线 tag 无效".into());
    }
    Ok(())
}

/// Observe the actual publication authority, including after an uncertain push.
/// Remote tags are never inferred from local cached tags. Fetching advertised
/// object IDs writes no branch, tag, index, working file, or FETCH_HEAD.
pub fn read_publication(root: &Path, tag: &str) -> Result<Option<PublicationReadback>, String> {
    valid_baseline_tag(tag)?;
    let (main, tag_object) = if has_origin(root)? {
        let bytes = checked(
            root,
            &["ls-remote", "--refs", "origin", "refs/heads/main", tag],
            None,
            None,
            false,
        )?;
        let text = String::from_utf8(bytes).map_err(|e| e.to_string())?;
        let mut advertised = BTreeMap::new();
        for line in text.lines() {
            let (sha, name) = line.split_once('\t').ok_or("远端引用响应无效")?;
            if name != "refs/heads/main" && name != tag {
                return Err("远端返回了未请求的引用".into());
            }
            if advertised
                .insert(name.to_owned(), oid(sha.as_bytes())?)
                .is_some()
            {
                return Err("远端返回了重复引用".into());
            }
        }
        let main = advertised
            .remove("refs/heads/main")
            .ok_or("远端 main 不存在")?;
        let Some(tag_object) = advertised.remove(tag) else {
            return Ok(None);
        };
        // An existing conflicting local tag is a user-visible inconsistency.
        // Do not silently overwrite it or certify from its cached value.
        if reference(root, tag)?.is_some_and(|local| local != tag_object) {
            return Err("本地基线标签与 origin 冲突；未覆盖任何标签".into());
        }
        checked(
            root,
            &[
                "fetch",
                "--no-tags",
                "--no-write-fetch-head",
                "--recurse-submodules=no",
                "origin",
                &main,
                &tag_object,
            ],
            None,
            None,
            false,
        )?;
        (main, tag_object)
    } else {
        let main = reference(root, "refs/heads/main")?.ok_or("本地 main 不存在")?;
        let Some(tag_object) = reference(root, tag)? else {
            return Ok(None);
        };
        (main, tag_object)
    };
    let main_commit = oid(&checked(
        root,
        &["rev-parse", "--verify", &format!("{main}^{{commit}}")],
        None,
        None,
        false,
    )?)?;
    if main_commit != main {
        return Err("权威 main 必须直接指向提交".into());
    }
    let commit = oid(&checked(
        root,
        &["rev-parse", "--verify", &format!("{tag_object}^{{commit}}")],
        None,
        None,
        false,
    )?)?;
    let ancestry = git(
        root,
        &["merge-base", "--is-ancestor", &commit, &main],
        None,
        None,
        false,
    )?;
    if !ancestry.status.success() {
        return Err(if ancestry.status.code() == Some(1) {
            "基线标签提交尚未进入权威 main".into()
        } else {
            format!(
                "无法核对基线的 main 可达性：{}",
                String::from_utf8_lossy(&ancestry.stderr).trim()
            )
        });
    }
    Ok(Some(PublicationReadback {
        tag: tag.into(),
        commit,
        main_revision: main,
    }))
}

fn positive(value: &str) -> Result<u64, String> {
    if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
        return Err("Towermd 快照 trailer 无效或重复".into());
    }
    value
        .parse::<u64>()
        .ok()
        .filter(|n| *n > 0)
        .ok_or_else(|| "Towermd 快照 trailer 必须为正整数".into())
}

/// A per-operation cache only: immutable candidate commits can be shared by
/// main and the private ref. No proof reads another history or trusts a cache
/// persisted outside Git.
struct SnapshotDiscovery {
    config: Option<Config>,
    checked: BTreeMap<String, Option<Snapshot>>,
}
impl SnapshotDiscovery {
    fn new(root: &Path, main: &str) -> Result<Self, String> {
        let path = ".towermd/config.yaml";
        let entry = checked(root, &["ls-tree", main, "--", path], None, None, false)?;
        let config = if entry.is_empty() {
            None
        } else {
            if !entry.starts_with(b"100644 blob ") {
                return Err("权威项目配置不是普通文件".into());
            }
            let bytes = checked(root, &["cat-file", "blob", &format!("{main}:{path}")], None, None, false)?;
            Some(Config::parse(std::str::from_utf8(&bytes).map_err(|_| "项目配置不是 UTF-8")?)?)
        };
        Ok(Self { config, checked: BTreeMap::new() })
    }

    fn prove(&mut self, root: &Path, commit: &str, boundary: Boundary) -> Option<Snapshot> {
        if let Some(found) = self.checked.get(commit) { return found.clone(); }
        let proof = (|| -> Result<Snapshot, String> {
            let config = self.config.as_ref().ok_or("快照缺少当前项目配置")?;
            let raw = crate::snapshot_publication::bytes(root, &["cat-file", "blob",
                &format!("{commit}:{}/state/state.json", config.docs_dir())])?;
            let state: crate::snapshot::SnapshotState = serde_json::from_slice(&raw).map_err(|error|error.to_string())?;
            if state.project_id != config.project_id() || state.epoch != boundary.epoch || state.seq != boundary.seq {
                return Err("快照候选的实际项目或水位不一致".into());
            }
            // Discovery checks the generic archive envelope, not an assumed
            // business schema. Runtime subsequently replays the selected
            // snapshot with its actual reducer and never falls back on failure.
            let snapshot = crate::snapshot::Snapshot::read(root, commit, config.docs_dir(),
                config.project_id(), |_| Ok(state.projection.clone()))?;
            let originals = crate::project_runtime::canonical_events(&snapshot.events)?;
            for original in originals.values().filter(|event|!event.acquired_leases().is_empty()) {
                // Reuse Hub's system-event validator, including producer,
                // UUID, contiguous grant epochs and actual lease delta.
                // Omitted renewals prevent replaying every live deadline; a
                // grant's own envelope remains independently checkable.
                let mut system = crate::hub::SystemState {
                    epoch: original.acquired_leases()[0].epoch.checked_sub(1).ok_or("归档租约 epoch 为零")?,
                    ..Default::default()
                };
                system.replay(original).map_err(|error|error.to_string())?;
            }
            let proof = crate::snapshot_publication::prove_checked(root, commit, config.docs_dir(),
                config.project_id(), &snapshot, &|eid|originals.get(eid).cloned())?;
            if originals.values().any(|event| !event.stale && event.acquired_leases().iter()
                .any(|lease|lease.name=="main" && lease.epoch>proof.epoch)) {
                return Err("快照归档已经包含更高的主干租约，旧写入者无权出版".into());
            }
            Ok(Snapshot { commit: commit.into(), boundary })
        })().ok();
        self.checked.insert(commit.into(), proof.clone());
        proof
    }
}

/// Final trailers locate candidates only. A malformed or forged trailer is
/// diagnosed by main inspection; it cannot advance a writer's fencing bound.
fn history(root: &Path, commit: &str, discovery: &mut SnapshotDiscovery) -> Result<(Option<Snapshot>, u64), String> {
    let bytes = checked(
        root,
        &[
            "log",
            "--format=%H%x00%(trailers:key=Towermd-Snapshot,valueonly)%x00%(trailers:key=Towermd-Epoch,valueonly)%x00%(trailers:key=Towermd-Baseline,valueonly)%x00%an%x00",
            commit,
            "--",
        ],
        None,
        None,
        false,
    )?;
    let text = String::from_utf8(bytes).map_err(|e| e.to_string())?;
    let fields = text.split('\0').collect::<Vec<_>>();
    let mut candidates = Vec::new();
    let mut highest_slow_epoch = 0;
    for record in fields.chunks(5) {
        if record.len() < 5 {
            if record.iter().any(|field| !field.trim().is_empty()) {
                return Err("Git trailer 查询结果不完整".into());
            }
            continue;
        }
        let sha = oid(record[0].as_bytes())?;
        let seq = record[1].trim();
        let epoch = record[2].trim();
        let author = record[4].trim();
        // Keep the existing protection for a slow publication whose following
        // snapshot has not yet been archived. This watermark is fencing only;
        // it never makes the commit a recoverable snapshot or an approved
        // baseline. Its canonical approval is checked by the business layer.
        if seq.is_empty() && author == "process:towermd" {
            if let Ok(epoch) = positive(epoch) {
                highest_slow_epoch = highest_slow_epoch.max(epoch);
            }
            continue;
        }
        if author != "process:towermd-snapshot" {
            continue;
        }
        if let (Ok(epoch), Ok(seq)) = (positive(epoch), positive(seq)) {
            candidates.push((Boundary { epoch, seq }, sha));
        }
    }
    candidates.sort_unstable_by(|left,right|right.cmp(left));
    for (boundary, sha) in candidates {
        if let Some(snapshot) = discovery.prove(root, &sha, boundary) {
            return Ok((Some(snapshot), boundary.epoch.max(highest_slow_epoch)));
        }
    }
    Ok((None, highest_slow_epoch))
}

/// Reads the already fetched authority and private snapshot. It does not fetch.
pub fn latest_snapshot(root: &Path) -> Result<Option<Snapshot>, String> {
    let origin = has_origin(root)?;
    let Some(main) = reference(root, main_ref(origin))? else { return Ok(None); };
    let mut discovery = SnapshotDiscovery::new(root, &main)?;
    let mut latest: Option<Snapshot> = None;
    for name in [main_ref(origin), SNAPSHOT_REF] {
        if let Some(commit) = reference(root, name)? {
            if let Some(snapshot) = history(root, &commit, &mut discovery)?.0 {
                if latest
                    .as_ref()
                    .is_none_or(|old| snapshot.boundary > old.boundary)
                {
                    latest = Some(snapshot);
                }
            }
        }
    }
    Ok(latest)
}

/// A periodic probe only. If publication becomes possible, the runtime writes
/// a new checked snapshot with its current lease and continuous event prefix.
pub fn private_snapshot_needs_publication(root: &Path, commit: &str) -> Result<bool, String> {
    if reference(root, SNAPSHOT_REF)?.as_deref() != Some(commit) {
        return Ok(false);
    }
    let origin = has_origin(root)?;
    if !origin && main_checked_out(root)? {
        return Ok(false);
    }
    let main = reference(root, main_ref(origin))?.ok_or("权威 main 不存在")?;
    let result = git(root, &["merge-base", "--is-ancestor", commit, &main], None, None, false)?;
    match result.status.code() {
        Some(0) => Ok(false),
        Some(1) => Ok(true),
        _ => Err("无法核对私有快照是否已进入 main".into()),
    }
}

fn safe_path(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && !path.contains(['\\', ':'])
        && !path.chars().any(char::is_control)
        && path.split('/').all(|part| {
            !part.is_empty() && part != "." && part != ".." && !part.eq_ignore_ascii_case(".git")
        })
}

fn validate(request: &WriteRequest) -> Result<(), String> {
    if let WriteKind::Advance { expected_base, commit } = &request.kind {
        oid(expected_base.as_bytes())?;
        oid(commit.as_bytes())?;
        if normalize_docs_dir(&request.docs_dir)? != request.docs_dir || request.epoch == 0
            || !request.changes.is_empty() || !request.message.is_empty() || expected_base == commit
        {
            return Err("快进写入只接受已有提交，不带文件或说明".into());
        }
        return Ok(());
    }
    if normalize_docs_dir(&request.docs_dir)? != request.docs_dir
        || request.epoch == 0
        || request.changes.is_empty()
    {
        return Err("主干写入目录、epoch 或文件集合无效".into());
    }
    if request.message.trim().is_empty()
        || request.message.lines().any(|line| {
            let key = line
                .split_once(':')
                .map(|(key, _)| key.trim().to_ascii_lowercase());
            matches!(key.as_deref(), Some("towermd-epoch" | "towermd-snapshot"))
        })
    {
        return Err("提交说明为空或重复提供系统 trailer".into());
    }
    let state = format!("{}/state/", request.docs_dir);
    for path in request.changes.keys() {
        if !safe_path(path) {
            return Err(format!("非法写入路径：{path}"));
        }
        if matches!(request.kind, WriteKind::Snapshot { .. }) {
            let suffix = path.strip_prefix(&state).ok_or("快照只能改 state/")?;
            let event = suffix
                .strip_prefix("events/")
                .and_then(|s| s.strip_suffix(".jsonl"));
            let valid_month = event.is_some_and(|month| {
                month.len() == 7
                    && month.as_bytes()[4] == b'-'
                    && month[..4].bytes().all(|b| b.is_ascii_digit())
                    && month[5..]
                        .parse::<u8>()
                        .is_ok_and(|m| (1..=12).contains(&m))
            });
            if suffix != "state.json" && !valid_month {
                return Err("快照只能写 state/state.json 与 state/events/YYYY-MM.jsonl".into());
            }
            if request.changes[path].is_none() {
                return Err("快照不能删除状态或事件归档".into());
            }
        } else if path.starts_with(&state) {
            return Err("慢层提交不能兼作快照；state/ 必须经快照写入".into());
        }
    }
    match &request.kind {
        WriteKind::Snapshot { seq, promote_from } => {
            if *seq == 0
                || request
                    .changes
                    .get(&format!("{state}state.json"))
                    .is_none_or(Option::is_none)
            {
                return Err("快照必须有正整数 seq 与 state.json".into());
            }
            if let Some(commit) = promote_from {
                oid(commit.as_bytes())?;
            }
        }
        WriteKind::Slow { expected_base, tag } => {
            oid(expected_base.as_bytes())?;
            if let Some(tag) = tag {
                valid_baseline_tag(tag)?;
            }
        }
        WriteKind::Advance { .. } => unreachable!("checked above"),
    }
    Ok(())
}

/// An advance must descend from the base and leave the snapshot path alone.
fn check_advance(root: &Path, docs_dir: &str, base: &str, commit: &str) -> Result<(), String> {
    let kind = checked(root, &["cat-file", "-t", commit], None, None, false)?;
    if kind.trim_ascii() != b"commit" {
        return Err("快进目标不是提交".into());
    }
    let ancestor = git(root, &["merge-base", "--is-ancestor", base, commit], None, None, false)?;
    if !ancestor.status.success() {
        return Err("快进目标不在当前 main 之后；须先变基".into());
    }
    let names = checked(root, &["diff", "--no-renames", "--name-only", "-z", base, commit, "--"], None, None, false)?;
    let state = format!("{docs_dir}/state/");
    if names.split(|b| *b == 0).any(|name| name.starts_with(state.as_bytes())) {
        return Err("快进不能改快照路径 state/".into());
    }
    Ok(())
}

fn main_checked_out(root: &Path) -> Result<bool, String> {
    let bytes = checked(
        root,
        &["worktree", "list", "--porcelain", "-z"],
        None,
        None,
        false,
    )?;
    Ok(bytes
        .split(|b| *b == 0)
        .any(|field| field == b"branch refs/heads/main"))
}

struct TemporaryIndex(PathBuf);
impl Drop for TemporaryIndex {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
        let _ = fs::remove_file(self.0.with_extension("index.lock"));
    }
}

fn verify_archives(root: &Path, source: &str, tree: &str, docs: &str) -> Result<(), String> {
    let directory = format!("{docs}/state/events/");
    let entries = checked(
        root,
        &["ls-tree", "-r", "-z", source, "--", &directory],
        None,
        None,
        true,
    )?;
    for entry in entries.split(|b| *b == 0).filter(|entry| !entry.is_empty()) {
        let entry = std::str::from_utf8(entry).map_err(|e| e.to_string())?;
        let (metadata, path) = entry.split_once('\t').ok_or("事件归档 tree 条目无效")?;
        let mut fields = metadata.split_whitespace();
        let mode = fields.next();
        if !matches!(mode, Some("100644" | "100755")) || fields.next() != Some("blob") {
            return Err("事件归档必须为普通文件".into());
        }
        let blob = fields.next().ok_or("事件归档缺少 blob")?;
        let old = checked(root, &["cat-file", "blob", blob], None, None, true)?;
        let new = checked(
            root,
            &["cat-file", "blob", &format!("{tree}:{path}")],
            None,
            None,
            true,
        )?;
        if !new.starts_with(&old) {
            return Err(format!("事件归档只能追加，不能丢失或覆盖：{path}"));
        }
    }
    Ok(())
}

fn build_commit(
    root: &Path,
    base: &str,
    request: &WriteRequest,
    archive_sources: &[String],
) -> Result<String, String> {
    let common = repo::git_timeout(root, &["rev-parse", "--git-common-dir"], TIMEOUT)?;
    let index = TemporaryIndex(
        root.join(common)
            .join(format!("towermd-write-{}.index", uuid::Uuid::now_v7())),
    );
    let snapshot = matches!(request.kind, WriteKind::Snapshot { .. });
    checked(root, &["read-tree", base], Some(&index.0), None, snapshot)?;
    for (path, bytes) in &request.changes {
        if let Some(bytes) = bytes {
            let blob = oid(&checked(
                root,
                &["hash-object", "-w", "--stdin"],
                None,
                Some(bytes),
                snapshot,
            )?)?;
            checked(
                root,
                &[
                    "update-index",
                    "--add",
                    "--cacheinfo",
                    "100644",
                    &blob,
                    path,
                ],
                Some(&index.0),
                None,
                snapshot,
            )?;
        } else {
            checked(
                root,
                &["update-index", "--force-remove", "--", path],
                Some(&index.0),
                None,
                snapshot,
            )?;
        }
    }
    let tree = oid(&checked(
        root,
        &["write-tree"],
        Some(&index.0),
        None,
        snapshot,
    )?)?;
    if snapshot {
        for source in archive_sources {
            verify_archives(root, source, &tree, &request.docs_dir)?;
        }
    }
    if let WriteKind::Snapshot {
        promote_from: Some(source),
        ..
    } = &request.kind
    {
        let path = format!("{}/state/", request.docs_dir);
        let delta = checked(
            root,
            &["diff", "--name-only", source, &tree, "--", &path],
            None,
            None,
            snapshot,
        )?;
        if !delta.is_empty() {
            return Err("搬送私有快照不得改变原快照内容".into());
        }
    }
    let body = request.message.trim_end();
    let last_paragraph = body.rsplit("\n\n").next().unwrap_or(body);
    let has_trailers = last_paragraph.lines().all(|line| {
        line.split_once(':').is_some_and(|(key, _)| {
            !key.is_empty() && key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        })
    });
    // Do not use interpret-trailers to add fields: repository trailer.*.cmd
    // configuration may execute commands. Our generated trailer block is plain.
    let mut message = format!(
        "{body}{}Towermd-Epoch: {}\n",
        if has_trailers { "\n" } else { "\n\n" },
        request.epoch
    );
    if let WriteKind::Snapshot { seq, .. } = request.kind {
        message.push_str(&format!("Towermd-Snapshot: {seq}\n"));
    }
    oid(&checked(
        root,
        &["commit-tree", &tree, "-p", base],
        None,
        Some(message.as_bytes()),
        snapshot,
    )?)
}

/// The closure must validate the current authoritative main lease at each call.
/// It is deliberately not a serialized token or an App-supplied boolean.
pub fn write(
    root: &Path,
    request: &WriteRequest,
    mut check_fence: impl FnMut() -> Result<(), String>,
) -> Result<WriteOutcome, String> {
    validate(request)?;
    let origin = has_origin(root)?;
    for _ in 0..3 {
        check_fence()?;
        if origin {
            checked(
                root,
                &[
                    "fetch",
                    "--no-tags",
                    "--recurse-submodules=no",
                    "origin",
                    "+refs/heads/main:refs/remotes/origin/main",
                ],
                None,
                None,
                false,
            )?;
        }
        check_fence()?;
        let main = reference(root, main_ref(origin))?.ok_or("权威 main 不存在")?;
        let private = reference(root, SNAPSHOT_REF)?;
        let mut discovery = SnapshotDiscovery::new(root, &main)?;
        let (main_snapshot, main_epoch) = history(root, &main, &mut discovery)?;
        let (private_snapshot, private_epoch) = match &private {
            Some(commit) => history(root, commit, &mut discovery)?,
            None => (None, 0),
        };
        if main_epoch.max(private_epoch) > request.epoch {
            return Err("主干已有更高 epoch，旧写入者必须停止".into());
        }
        let checked_out = !origin && main_checked_out(root)?;
        if let WriteKind::Advance { expected_base, commit } = &request.kind {
            if checked_out {
                return Err("无 origin 且 main 已在工作树检出；不能快进 main。请配置远端或切换 main 工作树后重试".into());
            }
            if expected_base != &main {
                return Err("权威 main 已改变；须重新变基后再合并".into());
            }
            check_advance(root, &request.docs_dir, &main, commit)?;
        }
        let (destination, old, tag) = match &request.kind {
            WriteKind::Slow { expected_base, tag } => {
                if checked_out {
                    return Err("无 origin 且 main 已在工作树检出；不能出版慢层。请配置远端或切换 main 工作树后重试".into());
                }
                if expected_base != &main {
                    return Err("权威 main 已改变；请重新审阅当前基底后批准".into());
                }
                ("refs/heads/main", Some(main.clone()), tag.clone())
            }
            WriteKind::Snapshot { seq, promote_from } => {
                let boundary = Boundary {
                    epoch: request.epoch,
                    seq: *seq,
                };
                if let Some(source) = promote_from {
                    if checked_out
                        || private.as_ref() != Some(source)
                        || private_snapshot.as_ref().is_none_or(|s| {
                            s.commit != *source
                                || s.boundary.seq != boundary.seq
                                || s.boundary.epoch > boundary.epoch
                        })
                    {
                        return Err("只能把当前私有快照原样搬送到可写 main".into());
                    }
                }
                for current in [main_snapshot.as_ref(), private_snapshot.as_ref()]
                    .into_iter()
                    .flatten()
                {
                    let moving_self = promote_from
                        .as_ref()
                        .is_some_and(|source| source == &current.commit)
                        && main_snapshot.as_ref().is_none_or(|s| s.boundary < boundary);
                    if current.boundary >= boundary && !moving_self {
                        return Ok(WriteOutcome::Superseded {
                            commit: current.commit.clone(),
                            boundary: current.boundary,
                        });
                    }
                }
                if checked_out {
                    (SNAPSHOT_REF, private.clone(), None)
                } else {
                    ("refs/heads/main", Some(main.clone()), None)
                }
            }
            WriteKind::Advance { .. } => ("refs/heads/main", Some(main.clone()), None),
        };
        check_fence()?;
        let commit = if let WriteKind::Advance { commit, .. } = &request.kind {
            commit.clone()
        } else {
            let mut archive_sources = vec![main.clone()];
            archive_sources.extend(private);
            build_commit(root, &main, request, &archive_sources)?
        };
        check_fence()?;
        if origin {
            if let Some(tag) = &tag {
                if reference(root, tag)?.is_some_and(|existing| existing != commit) {
                    return Err("本地已有冲突的基线标签；未推送或覆盖标签".into());
                }
            }
            let main_push = format!("{commit}:refs/heads/main");
            let tag_push = tag.as_ref().map(|tag| format!("{commit}:{tag}"));
            let mut args = vec![
                "push",
                "--atomic",
                "--porcelain",
                "--recurse-submodules=no",
                "origin",
                &main_push,
            ];
            if let Some(value) = &tag_push {
                args.push(value);
            }
            let output = git(root, &args, None, None, false)?;
            if output.status.success() {
                if let Some(tag) = &tag {
                    let readback = read_publication(root, tag)
                        .map_err(|e| {
                            format!("基线已推送，但权威读回未确认；请使用原批准重试：{e}")
                        })?
                        .ok_or("基线已推送，但权威标签尚未确认；请使用原批准重试")?;
                    if readback.commit != commit {
                        return Err(
                            "基线已推送，但权威标签指向不同提交；请核对原批准，不要重新批准".into(),
                        );
                    }
                }
                checked(
                    root,
                    &[
                        "fetch",
                        "--no-tags",
                        "--recurse-submodules=no",
                        "origin",
                        "+refs/heads/main:refs/remotes/origin/main",
                    ],
                    None,
                    None,
                    false,
                )
                .map_err(|e| format!("已推送，但 main 读回未完成；请核对原写入后重试：{e}"))?;
                return Ok(WriteOutcome::Written {
                    commit,
                    reference: destination.into(),
                    tag,
                });
            }
            let diagnostic = format!(
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            if diagnostic.contains("(fetch first)") || diagnostic.contains("(non-fast-forward)") {
                continue;
            }
            return Err(format!(
                "主干推送未确认成功；请先核对远端再重试：{}",
                diagnostic.trim()
            ));
        }
        // Recheck after object creation; never move main if another checkout
        // started using it while we were preparing the commit.
        if destination == "refs/heads/main" && main_checked_out(root)? {
            return Err("main 已被工作树检出，停止写入".into());
        }
        let old = old.unwrap_or_else(|| "0".repeat(commit.len()));
        let mut transaction = format!("start\nupdate {destination} {commit} {old}\n");
        if let Some(tag) = &tag {
            transaction.push_str(&format!("create {tag} {commit}\n"));
        }
        transaction.push_str("prepare\ncommit\n");
        let output = git(
            root,
            &["update-ref", "--stdin"],
            None,
            Some(transaction.as_bytes()),
            false,
        )?;
        if output.status.success() {
            return Ok(WriteOutcome::Written {
                commit,
                reference: destination.into(),
                tag,
            });
        }
        if reference(root, destination)?.as_deref() != Some(old.as_str()) {
            continue;
        }
        return Err(format!(
            "主干 CAS 失败：{}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Err("主干连续 3 次被并发推进；停止并等待重试".into())
}
