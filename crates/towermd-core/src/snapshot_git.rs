use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::init::normalize_docs_dir;
use crate::repo::git_timeout_env;

const TIMEOUT: Duration = Duration::from_secs(30);

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
        let urls = session.run(
            &root,
            &["remote", "get-url", "--push", "--all", "origin"],
            None,
        )?;
        if urls.lines().count() != 1 {
            return Err("origin 必须只有一个 push URL，无法原子更新多个远端".into());
        }
        Some(urls)
    } else {
        None
    };
    let base = if remote {
        fetch_main(&session, &root)?
    } else {
        require_direct_main(&session, &root)?;
        if let Some(worktree) = main_worktree(&session, &root)? {
            return Ok(WriteOutcome::Deferred {
                reason: format!(
                    "main 已在工作树 {} 检出；请自行切换到其他分支后重试快照",
                    worktree.display()
                ),
            });
        }
        session.run(&root, &["rev-parse", "--verify", "refs/heads/main"], None)?
    };
    if let Some((commit, trailer)) = highest_fence(&session, &root, &base)? {
        if (trailer.epoch, trailer.seq) >= (request.writer_epoch, request.through_seq) {
            return Ok(WriteOutcome::Superseded {
                commit,
                epoch: trailer.epoch,
                seq: trailer.seq,
            });
        }
    }
    if base != request.base {
        return Err("main 已推进；caller 须重新 fetch、加载 base 和检查 fence，不可盲重试".into());
    }
    validate_tree_paths(&session, &root, &base, &files)?;
    let index = session.temporary.path().join("index");
    session.run(&root, &["read-tree", &base], Some(&index))?;
    for (path, content) in &files {
        let blob = session.blob(&root, content)?;
        session.run(
            &root,
            &[
                "update-index",
                "--add",
                "--cacheinfo",
                "100644",
                &blob,
                path,
            ],
            Some(&index),
        )?;
    }
    let tree = session.run(&root, &["write-tree"], Some(&index))?;
    validate_changes(&session, &root, &base, &tree, &files)?;
    let message = message(request);
    verify_lease()?;
    let commit = session.run(
        &root,
        &["commit-tree", &tree, "-p", &base, "-m", &message],
        Some(&index),
    )?;
    verify_lease()?;
    if remote {
        session.run(&root, &["push", "--no-verify", "--no-follow-tags", "--no-signed", "--recurse-submodules=no", "--", push_url.as_deref().unwrap(), &format!("{commit}:refs/heads/main")], Some(&index))
            .map_err(|error| format!("快照 push 未确认成功（可能被抢先）；caller 下次须 fresh fetch 检查 fence：{error}"))?;
    } else {
        if let Some(worktree) = main_worktree(&session, &root)? {
            return Ok(WriteOutcome::Deferred {
                reason: format!(
                    "main 在写入期间于工作树 {} 被检出；请自行切换到其他分支后重试快照",
                    worktree.display()
                ),
            });
        }
        require_direct_main(&session, &root)?;
        session
            .run(
                &root,
                &[
                    "update-ref",
                    "--no-deref",
                    "refs/heads/main",
                    &commit,
                    &base,
                ],
                Some(&index),
            )
            .map_err(|error| format!("main CAS 失败；caller 须重新加载并检查 fence：{error}"))?;
    }
    Ok(WriteOutcome::Written { commit })
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
    let lines: Vec<_> = message.trim_end().lines().collect();
    let Some(separator) = lines.iter().rposition(|line| line.trim().is_empty()) else {
        return Ok(None);
    };
    let mut seq = None;
    let mut epoch = None;
    let mut invalid = false;
    let mut mentions_snapshot = false;
    for line in &lines[separator + 1..] {
        mentions_snapshot |= line.trim_start().starts_with("Towermd-Snapshot")
            || line.trim_start().starts_with("Towermd-Epoch");
        let Some((key, value)) = line.split_once(':') else {
            invalid = true;
            continue;
        };
        if key.is_empty()
            || !key
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        {
            invalid = true;
        }
        let target = match key {
            "Towermd-Snapshot" => &mut seq,
            "Towermd-Epoch" => &mut epoch,
            _ => continue,
        };
        let value = value.trim();
        if target.is_some() || value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit())
        {
            return Err("快照 footer 重复或数值不合法".into());
        }
        *target = Some(value.parse::<u64>().map_err(|_| "快照 footer 数值越界")?);
    }
    if !mentions_snapshot {
        return Ok(None);
    }
    match (seq, epoch, invalid) {
        (Some(seq), Some(epoch), false) => Ok(Some(SnapshotTrailer { seq, epoch })),
        _ => Err("快照 footer 必须包含唯一且合法的 Towermd-Snapshot/Towermd-Epoch 一对".into()),
    }
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
    let mut options = vec!["-c", "core.fsmonitor=false", "-c", "core.quotePath=false"];
    options.extend_from_slice(args);
    git_timeout_env(
        root,
        &options,
        TIMEOUT,
        &[
            ("GIT_OPTIONAL_LOCKS", OsStr::new("0")),
            ("GIT_LITERAL_PATHSPECS", OsStr::new("1")),
        ],
    )
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

pub(crate) struct Session {
    temporary: tempfile::TempDir,
    options: Vec<String>,
    date: String,
}

impl Session {
    fn new(root: &Path) -> Result<Self, String> {
        let common = PathBuf::from(read_git(
            root,
            &["rev-parse", "--path-format=absolute", "--git-common-dir"],
        )?);
        let parent = common.join("towermd");
        if let Ok(metadata) = fs::symlink_metadata(&parent) {
            if !metadata.is_dir() || metadata.file_type().is_symlink() {
                return Err("git-common-dir/towermd 不是安全目录".into());
            }
        }
        fs::create_dir_all(&parent).map_err(|error| error.to_string())?;
        let temporary = tempfile::Builder::new()
            .prefix("snapshot-")
            .tempdir_in(parent)
            .map_err(|error| error.to_string())?;
        Self::with_temporary(root, temporary)
    }

    pub(crate) fn for_network(root: &Path) -> Result<Self, String> {
        let temporary = tempfile::Builder::new()
            .prefix("towermd-origin-")
            .tempdir()
            .map_err(|error| error.to_string())?;
        Self::with_temporary(root, temporary)
    }

    pub(crate) fn run_output(
        &self,
        root: &Path,
        args: &[&str],
    ) -> Result<std::process::Output, String> {
        let mut options: Vec<&str> = self.options.iter().map(String::as_str).collect();
        options.extend_from_slice(args);
        crate::repo::git_timeout_env_output(
            root,
            &options,
            TIMEOUT,
            &[
                ("GIT_OPTIONAL_LOCKS", OsStr::new("0")),
                ("GIT_LITERAL_PATHSPECS", OsStr::new("1")),
                ("LC_ALL", OsStr::new("C")),
            ],
        )
    }

    fn with_temporary(root: &Path, temporary: tempfile::TempDir) -> Result<Self, String> {
        let hooks = temporary.path().join("hooks");
        fs::create_dir(&hooks).map_err(|error| error.to_string())?;
        let mut options = Vec::new();
        for option in [
            format!("core.hooksPath={}", path_text(&hooks)?),
            "core.fsmonitor=false".into(),
            "commit.gpgSign=false".into(),
            "gc.auto=0".into(),
            "maintenance.auto=false".into(),
            "core.splitIndex=false".into(),
            "core.autocrlf=false".into(),
            "core.safecrlf=false".into(),
            "push.followTags=false".into(),
            "remote.origin.mirror=false".into(),
            "push.gpgSign=false".into(),
        ] {
            options.extend(["-c".into(), option]);
        }
        let configuration = read_git(root, &["config", "--null", "--list"])?;
        for entry in configuration.split('\0') {
            let key = entry.split('\n').next().unwrap_or("");
            if key.starts_with("filter.") {
                if key.ends_with(".clean") || key.ends_with(".process") {
                    options.extend(["-c".into(), format!("{key}=")]);
                } else if key.ends_with(".required") {
                    options.extend(["-c".into(), format!("{key}=false")]);
                }
            }
        }
        let date = format!(
            "{} +0000",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|error| error.to_string())?
                .as_secs()
        );
        Ok(Self {
            temporary,
            options,
            date,
        })
    }

    pub(crate) fn run(
        &self,
        root: &Path,
        args: &[&str],
        index: Option<&Path>,
    ) -> Result<String, String> {
        let mut options: Vec<&str> = self.options.iter().map(String::as_str).collect();
        options.extend_from_slice(args);
        let mut environment = vec![
            ("GIT_AUTHOR_NAME", OsStr::new("process:towermd-snapshot")),
            ("GIT_AUTHOR_EMAIL", OsStr::new("towermd-snapshot@localhost")),
            ("GIT_COMMITTER_NAME", OsStr::new("process:towermd-snapshot")),
            (
                "GIT_COMMITTER_EMAIL",
                OsStr::new("towermd-snapshot@localhost"),
            ),
            ("GIT_AUTHOR_DATE", OsStr::new(&self.date)),
            ("GIT_COMMITTER_DATE", OsStr::new(&self.date)),
            ("GIT_OPTIONAL_LOCKS", OsStr::new("0")),
            ("GIT_LITERAL_PATHSPECS", OsStr::new("1")),
        ];
        if let Some(index) = index {
            environment.push(("GIT_INDEX_FILE", index.as_os_str()));
        }
        git_timeout_env(root, &options, TIMEOUT, &environment)
    }

    fn blob(&self, root: &Path, content: &str) -> Result<String, String> {
        let file = self.temporary.path().join("blob");
        fs::write(&file, content).map_err(|error| error.to_string())?;
        self.run(
            root,
            &["hash-object", "--no-filters", "-w", "--", path_text(&file)?],
            None,
        )
    }
}

fn path_text(path: &Path) -> Result<&str, String> {
    path.to_str().ok_or_else(|| "Git 路径不是 UTF-8".into())
}

fn message(request: &WriteRequest) -> String {
    format!(
        "Tower.md snapshot\n\nTowermd-Snapshot: {}\nTowermd-Epoch: {}",
        request.through_seq, request.writer_epoch
    )
}

fn validate_tree_paths(
    session: &Session,
    root: &Path,
    base: &str,
    files: &BTreeMap<String, String>,
) -> Result<(), String> {
    for path in files.keys() {
        let mut prefix = String::new();
        let components: Vec<_> = path.split('/').collect();
        for (offset, component) in components.iter().enumerate() {
            if !prefix.is_empty() {
                prefix.push('/');
            }
            prefix.push_str(component);
            let entry = session.run(root, &["ls-tree", "-z", base, "--", &prefix], None)?;
            if entry.is_empty() {
                break;
            }
            let mode = entry.split_once(' ').map(|(mode, _)| mode).unwrap_or("");
            let valid = if offset + 1 == components.len() {
                matches!(mode, "100644" | "100755")
            } else {
                mode == "040000"
            };
            if !valid {
                return Err(format!(
                    "快照路径 `{prefix}` 被 symlink、子模块或非预期对象占用"
                ));
            }
        }
    }
    Ok(())
}

fn validate_changes(
    session: &Session,
    root: &Path,
    base: &str,
    tree: &str,
    files: &BTreeMap<String, String>,
) -> Result<(), String> {
    let changes = session.run(
        root,
        &[
            "diff-tree",
            "--no-commit-id",
            "--name-only",
            "--no-renames",
            "-r",
            "-z",
            base,
            tree,
        ],
        None,
    )?;
    if changes
        .split('\0')
        .filter(|path| !path.is_empty())
        .any(|path| !files.contains_key(path))
    {
        return Err("快照改变了允许范围外的路径".into());
    }
    Ok(())
}

fn main_worktree(session: &Session, root: &Path) -> Result<Option<PathBuf>, String> {
    let listing = session.run(root, &["worktree", "list", "--porcelain", "-z"], None)?;
    let mut worktree = None;
    for field in listing.split('\0') {
        if let Some(path) = field.strip_prefix("worktree ") {
            worktree = Some(PathBuf::from(path));
        }
        if field == "branch refs/heads/main" {
            return Ok(worktree);
        }
    }
    Ok(None)
}
