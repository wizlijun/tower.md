//! Shared fixed-tree verifier for cold-start S5 and the future finish flow.
//! A detached worktree is isolation from accidental edits, not a security sandbox.
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use chrono::Utc;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::cold_start::{ScanCommand, ScanModule};
use towermd_core::repo;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VerificationPlan {
    pub root: PathBuf,
    pub id: String,
    pub revision: String,
    pub tree: String,
    pub commands: Vec<ScanCommand>,
    pub modules: Vec<ScanModule>,
    pub limits: Limits,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Limits {
    pub command_timeout_ms: u64,
    pub total_timeout_ms: u64,
    pub output_limit_bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            command_timeout_ms: 120_000,
            total_timeout_ms: 600_000,
            output_limit_bytes: 1_048_576,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VerificationReport {
    pub actor: String,
    pub revision: String,
    pub tree: String,
    pub started_at: String,
    pub finished_at: String,
    pub commands: Vec<CommandResult>,
    pub modules: Vec<ModuleResult>,
    pub cleanup_complete: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CommandResult {
    pub id: String,
    pub directory: String,
    pub argv: Vec<String>,
    pub modules: Vec<String>,
    pub status: VerificationStatus,
    pub exit_code: Option<i32>,
    pub duration_ms: u64,
    pub log: Option<String>,
    pub log_sha256: Option<String>,
    pub log_excerpt: String,
    pub log_truncated: bool,
    pub error: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ModuleResult {
    pub module: String,
    pub status: VerificationStatus,
    pub command_ids: Vec<String>,
    pub shared: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerificationStatus {
    Passed,
    Failed,
    NoTests,
    TimedOut,
    Cancelled,
    NotRun,
}

fn git(root: &Path, args: &[&str]) -> Result<String, String> {
    let mut safe = vec!["-c", "core.hooksPath=/dev/null"];
    safe.extend_from_slice(args);
    repo::git_timeout(root, &safe, Duration::from_secs(15))
}
fn ordinary_directory(path: &Path) -> Result<(), String> {
    let metadata =
        fs::symlink_metadata(path).map_err(|e| format!("无法读取目录 {}：{e}", path.display()))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(format!("验证目录不是普通目录：{}", path.display()));
    }
    Ok(())
}
fn create_directory(parent: &Path, name: &str) -> Result<PathBuf, String> {
    ordinary_directory(parent)?;
    let path = parent.join(name);
    match fs::create_dir(&path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            ordinary_directory(&path)?
        }
        Err(error) => return Err(format!("无法创建验证目录 {}：{error}", path.display())),
    }
    ordinary_directory(&path)?;
    Ok(path)
}
fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}
fn valid_directory(directory: &str) -> bool {
    directory.is_empty()
        || (!directory.contains('\\')
            && !directory.contains('\0')
            && directory
                .split('/')
                .all(|part| !part.is_empty() && part != "." && part != "..")
            && Path::new(directory)
                .components()
                .all(|part| matches!(part, Component::Normal(_))))
}
fn command_directory(worktree: &Path, directory: &str) -> Result<PathBuf, String> {
    if !valid_directory(directory) {
        return Err("验证命令目录必须是仓库内的普通相对目录".into());
    }
    ordinary_directory(worktree)?;
    let mut path = worktree.to_owned();
    if !directory.is_empty() {
        for part in directory.split('/') {
            path.push(part);
            ordinary_directory(&path)?;
        }
    }
    if !path
        .canonicalize()
        .map_err(|e| e.to_string())?
        .starts_with(worktree)
    {
        return Err("验证命令目录越出分离工作树".into());
    }
    Ok(path)
}

struct Worktree {
    root: PathBuf,
    path: PathBuf,
    removed: bool,
}
impl Worktree {
    fn cleanup(&mut self) -> Result<(), String> {
        if self.removed {
            return Ok(());
        }
        git(
            &self.root,
            &[
                "worktree",
                "remove",
                "--force",
                self.path.to_str().ok_or("验证目录不是 UTF-8")?,
            ],
        )?;
        if self.path.exists() {
            return Err("Git 删除验证工作树后目录仍存在".into());
        }
        self.removed = true;
        Ok(())
    }
}
impl Drop for Worktree {
    fn drop(&mut self) {
        if !self.removed {
            let _ = self.cleanup();
        }
    }
}
struct FrozenTree {
    revision: String,
    index: PathBuf,
    bytes: Vec<u8>,
}
impl FrozenTree {
    fn capture(worktree: &Path, revision: &str) -> Result<Self, String> {
        let index = PathBuf::from(git(
            worktree,
            &["rev-parse", "--path-format=absolute", "--git-path", "index"],
        )?);
        let state = Self {
            revision: revision.into(),
            bytes: fs::read(&index).map_err(|e| e.to_string())?,
            index,
        };
        state.check(worktree)?;
        Ok(state)
    }
    fn check(&self, worktree: &Path) -> Result<(), String> {
        if git(worktree, &["rev-parse", "HEAD"])? != self.revision {
            return Err("验证命令改变了固定 HEAD".into());
        }
        if git(worktree, &["rev-parse", "--abbrev-ref", "HEAD"])? != "HEAD" {
            return Err("验证工作树已不再是分离 HEAD".into());
        }
        if fs::read(&self.index).map_err(|e| format!("无法复核验证 index：{e}"))? != self.bytes
        {
            return Err("验证命令改变了 index".into());
        }
        if !git(
            worktree,
            &[
                "diff",
                "--no-ext-diff",
                "--no-textconv",
                "--name-only",
                "HEAD",
                "--",
            ],
        )?
        .is_empty()
        {
            return Err("验证命令改变了已跟踪文件".into());
        }
        Ok(())
    }
}

fn normalize(
    plan: &VerificationPlan,
) -> Result<(Vec<ScanCommand>, BTreeMap<String, String>), String> {
    let mut commands: Vec<ScanCommand> = Vec::new();
    let mut ids: BTreeMap<String, (String, String)> = BTreeMap::new();
    let mut aliases = BTreeMap::new();
    for candidate in &plan.commands {
        if !valid_id(&candidate.id)
            || !valid_directory(&candidate.directory)
            || candidate.command.trim().is_empty()
            || candidate.command.contains('\0')
        {
            return Err("验证命令的 ID、目录或正文无效".into());
        }
        let key = (candidate.directory.clone(), candidate.command.clone());
        if ids
            .get(&candidate.id)
            .is_some_and(|previous| previous != &key)
        {
            return Err("同一命令 ID 对应不同命令".into());
        }
        ids.insert(candidate.id.clone(), key);
        if let Some(existing) = commands.iter_mut().find(|existing| {
            existing.directory == candidate.directory && existing.command == candidate.command
        }) {
            existing.modules.extend(candidate.modules.iter().cloned());
            existing.modules.sort();
            existing.modules.dedup();
            aliases.insert(candidate.id.clone(), existing.id.clone());
        } else {
            aliases.insert(candidate.id.clone(), candidate.id.clone());
            commands.push(candidate.clone());
        }
    }
    for module in &plan.modules {
        for id in &module.command_ids {
            if !aliases.contains_key(id) {
                return Err(format!("模块 {} 引用了不存在的验证命令 {id}", module.id));
            }
        }
    }
    Ok((commands, aliases))
}
fn pending(command: &ScanCommand) -> CommandResult {
    CommandResult {
        id: command.id.clone(),
        directory: command.directory.clone(),
        argv: vec!["/bin/sh".into(), "-c".into(), command.command.clone()],
        modules: command.modules.clone(),
        status: VerificationStatus::NotRun,
        exit_code: None,
        duration_ms: 0,
        log: None,
        log_sha256: None,
        log_excerpt: String::new(),
        log_truncated: false,
        error: None,
    }
}
fn module_results(
    plan: &VerificationPlan,
    aliases: &BTreeMap<String, String>,
    results: &[CommandResult],
) -> Vec<ModuleResult> {
    use VerificationStatus::*;
    plan.modules
        .iter()
        .map(|module| {
            let mut seen = BTreeSet::new();
            let command_ids: Vec<_> = module
                .command_ids
                .iter()
                .map(|id| aliases[id].clone())
                .filter(|id| seen.insert(id.clone()))
                .collect();
            let selected: Vec<_> = results
                .iter()
                .filter(|result| command_ids.contains(&result.id))
                .collect();
            let status = if selected.is_empty() {
                NoTests
            } else {
                [Failed, TimedOut, Cancelled, NotRun]
                    .into_iter()
                    .find(|status| selected.iter().any(|result| result.status == *status))
                    .unwrap_or(Passed)
            };
            ModuleResult {
                module: module.id.clone(),
                status,
                command_ids,
                shared: selected.iter().any(|result| result.modules.len() > 1),
            }
        })
        .collect()
}

#[cfg(not(unix))]
pub fn run(_plan: &VerificationPlan, _cancel: &AtomicBool) -> Result<VerificationReport, String> {
    Err("此平台尚不支持验证进程组回收；不会运行或报告通过".into())
}

#[cfg(unix)]
pub fn run(plan: &VerificationPlan, cancel: &AtomicBool) -> Result<VerificationReport, String> {
    if !valid_id(&plan.id)
        || plan.limits.command_timeout_ms == 0
        || plan.limits.total_timeout_ms == 0
        || plan.limits.output_limit_bytes == 0
    {
        return Err("验证 ID 或限制无效".into());
    }
    let (commands, aliases) = normalize(plan)?;
    let started = Instant::now();
    let started_at = Utc::now().to_rfc3339();
    let root = repo::toplevel(&plan.root)?
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let revision = git(
        &root,
        &[
            "rev-parse",
            "--verify",
            &format!("{}^{{commit}}", plan.revision),
        ],
    )?;
    let tree = git(&root, &["rev-parse", &format!("{revision}^{{tree}}")])?;
    if revision != plan.revision || tree != plan.tree {
        return Err("验证计划的固定 commit/tree 不匹配".into());
    }
    let common = PathBuf::from(git(
        &root,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )?);
    ordinary_directory(&common)?;
    let common = common.canonicalize().map_err(|e| e.to_string())?;
    let cache = create_directory(&common, "towermd")?;
    let verify = create_directory(&cache, "verify")?;
    let session = verify.join(&plan.id);
    fs::create_dir(&session).map_err(|e| format!("验证 ID 已使用或无法创建目录：{e}"))?;
    let logs = create_directory(&session, "logs")?;
    let mut worktree = Worktree {
        root: root.clone(),
        path: session.join("worktree"),
        removed: false,
    };
    git(
        &root,
        &[
            "worktree",
            "add",
            "--detach",
            worktree.path.to_str().ok_or("验证目录不是 UTF-8")?,
            &revision,
        ],
    )?;
    let frozen = FrozenTree::capture(&worktree.path, &revision)?;
    let mut results = Vec::new();
    let mut stopped = false;
    let mut cleanup_complete = true;
    for command in &commands {
        let mut result = pending(command);
        if stopped || command.modules.is_empty() {
            results.push(result);
            continue;
        }
        if cancel.load(Ordering::Acquire) {
            result.status = VerificationStatus::Cancelled;
            stopped = true;
            results.push(result);
            continue;
        }
        let remaining =
            Duration::from_millis(plan.limits.total_timeout_ms).saturating_sub(started.elapsed());
        if remaining.is_zero() {
            result.status = VerificationStatus::TimedOut;
            result.error = Some("整批验证超过期限".into());
            stopped = true;
            results.push(result);
            continue;
        }
        if let Err(error) = frozen.check(&worktree.path) {
            result.status = VerificationStatus::Failed;
            result.error = Some(error);
            stopped = true;
            results.push(result);
            continue;
        }
        let cwd = match command_directory(&worktree.path, &command.directory) {
            Ok(path) => path,
            Err(error) => {
                result.status = VerificationStatus::Failed;
                result.error = Some(error);
                stopped = true;
                results.push(result);
                continue;
            }
        };
        let limit = remaining.min(Duration::from_millis(plan.limits.command_timeout_ms));
        let process_clean = execute(
            &cwd,
            &logs,
            command,
            &plan.limits,
            limit,
            cancel,
            &mut result,
        );
        if let Some(path) = result.log.take() {
            result.log = Some(
                Path::new(&path)
                    .strip_prefix(&common)
                    .map_err(|_| "验证日志越出公共 Git 目录")?
                    .to_string_lossy()
                    .into(),
            );
        }
        cleanup_complete &= process_clean;
        if let Err(error) = frozen.check(&worktree.path) {
            result.status = VerificationStatus::Failed;
            result.error = Some(error);
            stopped = true;
        }
        if !process_clean
            || matches!(
                result.status,
                VerificationStatus::TimedOut | VerificationStatus::Cancelled
            )
        {
            stopped = true;
        }
        results.push(result);
    }
    if let Err(error) = worktree.cleanup() {
        cleanup_complete = false;
        for result in &mut results {
            if result.status == VerificationStatus::Passed {
                result.status = VerificationStatus::Failed;
                result.error = Some(format!("验证工作树清理失败：{error}"));
            }
        }
    }
    if !cleanup_complete {
        for result in &mut results {
            if result.status == VerificationStatus::Passed {
                result.status = VerificationStatus::Failed;
                result.error = Some("验证进程或工作树未完整清理".into());
            }
        }
    }
    Ok(VerificationReport {
        actor: "process:towermd-verifier".into(),
        revision,
        tree,
        started_at,
        finished_at: Utc::now().to_rfc3339(),
        modules: module_results(plan, &aliases, &results),
        commands: results,
        cleanup_complete,
    })
}

#[cfg(unix)]
mod unix {
    use super::*;
    use std::os::fd::{AsRawFd, RawFd};
    unsafe extern "C" {
        fn kill(pid: i32, signal: i32) -> i32;
        fn fcntl(fd: i32, command: i32, ...) -> i32;
    }
    #[cfg(any(target_os = "linux", target_os = "android"))]
    const NONBLOCK: i32 = 2048;
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    const NONBLOCK: i32 = 4;
    pub fn nonblocking(fd: RawFd) -> Result<(), String> {
        let flags = unsafe { fcntl(fd, 3) };
        if flags < 0 || unsafe { fcntl(fd, 4, flags | NONBLOCK) } < 0 {
            return Err(format!(
                "无法设置验证输出非阻塞读取：{}",
                std::io::Error::last_os_error()
            ));
        }
        Ok(())
    }
    fn exists(group: i32) -> bool {
        (unsafe { kill(-group, 0) }) == 0
            || std::io::Error::last_os_error().raw_os_error() != Some(3)
    }
    pub struct Process {
        pub child: Child,
        group: i32,
        cleaned: bool,
    }
    impl Process {
        pub fn new(child: Child) -> Self {
            Self {
                group: child.id() as i32,
                child,
                cleaned: false,
            }
        }
        pub fn cleanup(&mut self) -> bool {
            if self.cleaned {
                return true;
            }
            unsafe {
                kill(-self.group, 15);
            }
            let started = Instant::now();
            while started.elapsed() < Duration::from_millis(200) {
                let _ = self.child.try_wait();
                if !exists(self.group) {
                    self.cleaned = true;
                    return true;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            unsafe {
                kill(-self.group, 9);
            }
            let started = Instant::now();
            while started.elapsed() < Duration::from_millis(800) {
                let _ = self.child.try_wait();
                if !exists(self.group) {
                    self.cleaned = true;
                    return true;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            false
        }
    }
    impl Drop for Process {
        fn drop(&mut self) {
            if !self.cleaned {
                let _ = self.cleanup();
            }
        }
    }
    pub fn prepare_pipes(process: &Process) -> Result<(), String> {
        nonblocking(
            process
                .child
                .stdout
                .as_ref()
                .ok_or("验证 stdout 不存在")?
                .as_raw_fd(),
        )?;
        nonblocking(
            process
                .child
                .stderr
                .as_ref()
                .ok_or("验证 stderr 不存在")?
                .as_raw_fd(),
        )
    }
}

struct Log {
    file: File,
    path: PathBuf,
    limit: usize,
    size: usize,
    hash: Sha256,
    excerpt: Vec<u8>,
    truncated: bool,
}
impl Log {
    fn open(logs: &Path, id: &str, limit: usize) -> Result<Self, String> {
        ordinary_directory(logs)?;
        let path = logs.join(format!("{id}.log"));
        let file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&path)
            .map_err(|e| format!("无法创建验证日志：{e}"))?;
        Ok(Self {
            file,
            path,
            limit,
            size: 0,
            hash: Sha256::new(),
            excerpt: Vec::new(),
            truncated: false,
        })
    }
    fn write(&mut self, bytes: &[u8]) -> Result<(), String> {
        let keep = bytes.len().min(self.limit.saturating_sub(self.size));
        self.file
            .write_all(&bytes[..keep])
            .map_err(|e| format!("无法写验证日志：{e}"))?;
        self.hash.update(&bytes[..keep]);
        self.size += keep;
        let preview = keep.min(4096usize.saturating_sub(self.excerpt.len()));
        self.excerpt.extend_from_slice(&bytes[..preview]);
        self.truncated |= keep < bytes.len();
        Ok(())
    }
    fn drain(&mut self, reader: &mut impl Read) -> Result<(), String> {
        let mut buffer = [0u8; 8192];
        // Bound each drain turn so a noisy child cannot starve cancellation.
        for _ in 0..32 {
            match reader.read(&mut buffer) {
                Ok(0) => break,
                Ok(size) => self.write(&buffer[..size])?,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(format!("无法读取验证输出：{error}")),
            }
        }
        Ok(())
    }
    fn finish(mut self, result: &mut CommandResult) {
        if let Err(error) = self.file.flush().and_then(|()| self.file.sync_all()) {
            result.status = VerificationStatus::Failed;
            result.error = Some(format!("验证日志无法持久化：{error}"));
        }
        result.log = Some(self.path.to_string_lossy().into());
        result.log_sha256 = Some(hex::encode(self.hash.finalize()));
        result.log_excerpt = String::from_utf8_lossy(&self.excerpt).into();
        result.log_truncated = self.truncated;
    }
}
#[cfg(unix)]
fn execute(
    cwd: &Path,
    logs: &Path,
    command: &ScanCommand,
    limits: &Limits,
    timeout: Duration,
    cancel: &AtomicBool,
    result: &mut CommandResult,
) -> bool {
    use std::os::unix::process::CommandExt;
    let started = Instant::now();
    let mut log = match Log::open(logs, &command.id, limits.output_limit_bytes) {
        Ok(log) => log,
        Err(error) => {
            result.status = VerificationStatus::Failed;
            result.error = Some(error);
            return true;
        }
    };
    let mut shell = Command::new("/bin/sh");
    shell
        .args(["-c", &command.command])
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0);
    // Preserve user environment (including network credentials), but never let
    // inherited Git location/index/config injection redirect the detached run.
    for name in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_COMMON_DIR",
        "GIT_INDEX_FILE",
        "GIT_OBJECT_DIRECTORY",
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
        "GIT_CEILING_DIRECTORIES",
        "GIT_DISCOVERY_ACROSS_FILESYSTEM",
        "GIT_IMPLICIT_WORK_TREE",
        "GIT_PREFIX",
        "GIT_GRAFT_FILE",
        "GIT_SHALLOW_FILE",
        "GIT_REPLACE_REF_BASE",
        "GIT_NAMESPACE",
        "GIT_CONFIG",
        "GIT_CONFIG_PARAMETERS",
        "GIT_CONFIG_COUNT",
    ] {
        shell.env_remove(name);
    }
    for (name, _) in std::env::vars_os() {
        let name_text = name.to_string_lossy();
        if name_text.starts_with("GIT_CONFIG_KEY_") || name_text.starts_with("GIT_CONFIG_VALUE_") {
            shell.env_remove(name);
        }
    }
    let mut process = match shell.spawn() {
        Ok(child) => unix::Process::new(child),
        Err(error) => {
            result.status = VerificationStatus::Failed;
            result.error = Some(format!("无法启动验证命令：{error}"));
            log.finish(result);
            return true;
        }
    };
    let execution = (|| -> Result<(), String> {
        unix::prepare_pipes(&process)?;
        loop {
            log.drain(process.child.stdout.as_mut().unwrap())?;
            log.drain(process.child.stderr.as_mut().unwrap())?;
            if cancel.load(Ordering::Acquire) {
                result.status = VerificationStatus::Cancelled;
                break;
            }
            if started.elapsed() >= timeout {
                result.status = VerificationStatus::TimedOut;
                result.error = Some("验证命令或整批验证超过期限".into());
                break;
            }
            if let Some(status) = process.child.try_wait().map_err(|e| e.to_string())? {
                result.exit_code = status.code();
                result.status = if status.success() {
                    VerificationStatus::Passed
                } else {
                    VerificationStatus::Failed
                };
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        Ok(())
    })();
    if let Err(error) = execution {
        result.status = VerificationStatus::Failed;
        result.error = Some(error);
    }
    let cleaned = process.cleanup();
    if !cleaned {
        result.status = VerificationStatus::Failed;
        result.error = Some("验证进程组未在期限内完整回收".into());
    }
    if let Some(stdout) = process.child.stdout.as_mut() {
        if let Err(error) = log.drain(stdout) {
            result.status = VerificationStatus::Failed;
            result.error = Some(error);
        }
    }
    if let Some(stderr) = process.child.stderr.as_mut() {
        if let Err(error) = log.drain(stderr) {
            result.status = VerificationStatus::Failed;
            result.error = Some(error);
        }
    }
    result.duration_ms = started.elapsed().as_millis().min(u64::MAX as u128) as u64;
    log.finish(result);
    cleaned
}
