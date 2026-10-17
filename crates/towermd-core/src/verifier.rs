//! Run selected-commit commands in an owned detached worktree, retaining sealed local logs.
pub use crate::recovery_contract::verification::{
    commands_hash, report_hash, CommandExecution, VerificationCommand, VerificationReport,
};
use crate::repo;
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    sync::{atomic::AtomicBool, Arc},
    time::Duration,
};

/// Internal execution input. Callers freeze the revision before scheduling this request.
/// `log_root` must be `<canonical common_dir>/towermd/cache/verification`.
pub struct VerificationRequest {
    pub root: PathBuf,
    pub common_dir: PathBuf,
    pub revision: String,
    pub commands: Vec<VerificationCommand>,
    pub timeout: Duration,
    pub log_root: PathBuf,
    pub cancelled: Arc<AtomicBool>,
}

const GIT_TIMEOUT: Duration = Duration::from_secs(10);

pub fn verify(request: &VerificationRequest) -> Result<VerificationReport, String> {
    #[cfg(not(unix))]
    return Err("verification process-group cleanup is not implemented on this platform".into());
    #[cfg(unix)]
    verify_unix(request)
}

#[cfg(unix)]
fn verify_unix(request: &VerificationRequest) -> Result<VerificationReport, String> {
    validate_request(request)?;
    let tree = git(
        &request.root,
        &["rev-parse", &format!("{}^{{tree}}", request.revision)],
    )?;
    crate::recovery_contract::object_id(&tree)?;
    let owned = secure_directories(&request.common_dir, &["towermd"])?;
    let verify_root = secure_directories(&owned, &["verify"])?;
    let log_root = secure_directories(&owned, &["cache", "verification"])?;
    let id = uuid::Uuid::now_v7().to_string();
    let logs = log_root.join(&id);
    fs::create_dir(&logs).map_err(|e| format!("create log directory: {e}"))?;
    sync_dir(&log_root)?;
    let worktree_path = verify_root.join(&id);
    fs::create_dir(&worktree_path).map_err(|e| format!("reserve worktree: {e}"))?;
    let mut worktree = Worktree {
        root: &request.root,
        path: worktree_path,
        registered: false,
        cleanup_allowed: true,
        removed: false,
    };
    let result = (|| {
        let path = worktree.path.to_str().ok_or("worktree path is not UTF-8")?;
        // The directory was reserved by this invocation, never discovered from stale state.
        worktree.registered = true;
        git(
            &request.root,
            &["worktree", "add", "--detach", path, &request.revision],
        )?;
        if git(&worktree.path, &["rev-parse", "HEAD"])? != request.revision
            || git(&worktree.path, &["rev-parse", "HEAD^{tree}"])? != tree
        {
            return Err("detached worktree commit/tree mismatch".into());
        }
        // Resolve every cwd before running anything, including symlinks in the selected tree.
        for command in &request.commands {
            resolve_cwd(&worktree.path, &command.cwd)?;
        }
        let mut report = VerificationReport {
            commit: request.revision.clone(),
            tree,
            executions: Vec::new(),
        };
        for (index, command) in request.commands.iter().enumerate() {
            let cwd = resolve_cwd(&worktree.path, &command.cwd)?;
            report.executions.push(unix::execute(
                request,
                command,
                &cwd,
                &logs,
                index,
                &mut worktree.cleanup_allowed,
            )?);
        }
        let bytes = crate::recovery_contract::canonical_json_bytes(&report)?;
        let mut file = new_file(&logs.join("report.json"))?;
        file.write_all(&bytes)
            .map_err(|e| format!("write report: {e}"))?;
        file.flush()
            .and_then(|()| file.sync_all())
            .map_err(|e| format!("seal report: {e}"))?;
        let digest = report_hash(&report)?;
        let mut checksum = new_file(&logs.join("report.sha256"))?;
        checksum
            .write_all(format!("{digest}\n").as_bytes())
            .map_err(|e| format!("write report digest: {e}"))?;
        checksum
            .flush()
            .and_then(|()| checksum.sync_all())
            .map_err(|e| format!("seal report digest: {e}"))?;
        sync_dir(&logs)?;
        Ok(report)
    })();
    // Even infrastructure failures attempt exact cleanup. A failed process cleanup keeps the
    // worktree for diagnosis instead of deleting a directory still occupied by a command.
    let cleanup = worktree.remove();
    match (result, cleanup) {
        (Ok(report), Ok(())) => Ok(report),
        (Err(error), Ok(())) => Err(error),
        (Ok(_), Err(error)) => Err(error),
        (Err(error), Err(cleanup)) => Err(format!("{error}; {cleanup}")),
    }
}

fn resolve_cwd(worktree: &Path, relative: &Path) -> Result<PathBuf, String> {
    let cwd = worktree
        .join(relative)
        .canonicalize()
        .map_err(|e| format!("cwd cannot be resolved: {e}"))?;
    if !cwd.starts_with(worktree) || !cwd.is_dir() {
        return Err("cwd escapes the verification worktree or is not a directory".into());
    }
    Ok(cwd)
}

fn git(root: &Path, args: &[&str]) -> Result<String, String> {
    repo::git_timeout(root, args, GIT_TIMEOUT)
}

fn validate_request(request: &VerificationRequest) -> Result<(), String> {
    if request
        .root
        .canonicalize()
        .map_err(|e| format!("canonical root: {e}"))?
        .as_os_str()
        != request.root.as_os_str()
    {
        return Err("root must be canonical".into());
    }
    if request
        .common_dir
        .canonicalize()
        .map_err(|e| format!("canonical common_dir: {e}"))?
        .as_os_str()
        != request.common_dir.as_os_str()
    {
        return Err("common_dir must be canonical".into());
    }
    let actual_root = PathBuf::from(git(&request.root, &["rev-parse", "--show-toplevel"])?)
        .canonicalize()
        .map_err(|e| format!("repository root: {e}"))?;
    let actual_common = PathBuf::from(git(
        &request.root,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )?)
    .canonicalize()
    .map_err(|e| format!("repository common_dir: {e}"))?;
    if actual_root != request.root || actual_common != request.common_dir {
        return Err("root/common_dir do not identify the same repository worktree".into());
    }
    crate::recovery_contract::object_id(&request.revision)
        .map_err(|e| format!("selected commit: {e}"))?;
    if git(&request.root, &["cat-file", "-t", &request.revision])? != "commit" {
        return Err("selected commit must be a full commit object ID".into());
    }
    if request.log_root != request.common_dir.join("towermd/cache/verification") {
        return Err("log_root must be the repository verification cache".into());
    }
    for command in &request.commands {
        if command.argv.is_empty()
            || command.argv[0].is_empty()
            || command.argv.iter().any(|arg| arg.contains('\0'))
        {
            return Err("argv must contain a nonempty program and arguments without NUL".into());
        }
        let cwd = command.cwd.to_str().ok_or("cwd must be UTF-8")?;
        if cwd != "."
            && (cwd.is_empty()
                || command
                    .cwd
                    .components()
                    .any(|c| !matches!(c, Component::Normal(_)))
                || cwd
                    .split('/')
                    .any(|part| part.is_empty() || part == "." || part == ".."))
        {
            return Err("cwd must be a normalized relative directory".into());
        }
    }
    Ok(())
}

fn secure_directories(root: &Path, names: &[&str]) -> Result<PathBuf, String> {
    let mut path = root.to_path_buf();
    for name in names {
        path.push(name);
        match fs::symlink_metadata(&path) {
            Ok(meta) if meta.file_type().is_symlink() => {
                return Err(format!(
                    "cache/worktree symlink rejected: {}",
                    path.display()
                ))
            }
            Ok(meta) if meta.is_dir() => {}
            Ok(_) => {
                return Err(format!(
                    "cache/worktree path is not a directory: {}",
                    path.display()
                ))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => match fs::create_dir(&path) {
                Ok(()) => sync_dir(path.parent().unwrap())?,
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    let meta = fs::symlink_metadata(&path)
                        .map_err(|e| format!("inspect cache directory: {e}"))?;
                    if meta.file_type().is_symlink() || !meta.is_dir() {
                        return Err("cache/worktree directory changed or is a symlink".into());
                    }
                }
                Err(e) => return Err(format!("create cache/worktree directory: {e}")),
            },
            Err(e) => return Err(format!("inspect cache/worktree directory: {e}")),
        }
    }
    Ok(path)
}
fn sync_dir(path: &Path) -> Result<(), String> {
    File::open(path)
        .and_then(|file| file.sync_all())
        .map_err(|e| format!("sync directory {}: {e}", path.display()))
}
// Successful publication checks errors explicitly. On early returns this guard also
// syncs partial diagnostics and directory entries; a failed scan never loses them merely
// because an output reader or a later file creation failed.
struct DurableFile {
    file: File,
    directory: PathBuf,
}
impl std::ops::Deref for DurableFile {
    type Target = File;
    fn deref(&self) -> &File {
        &self.file
    }
}
impl std::ops::DerefMut for DurableFile {
    fn deref_mut(&mut self) -> &mut File {
        &mut self.file
    }
}
impl Drop for DurableFile {
    fn drop(&mut self) {
        let _ = self.file.flush().and_then(|()| self.file.sync_all());
        let _ = sync_dir(&self.directory);
    }
}
fn new_file(path: &Path) -> Result<DurableFile, String> {
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| format!("create immutable log {}: {e}", path.display()))?;
    Ok(DurableFile {
        file,
        directory: path.parent().ok_or("log directory missing")?.to_path_buf(),
    })
}

struct Worktree<'a> {
    root: &'a Path,
    path: PathBuf,
    registered: bool,
    cleanup_allowed: bool,
    removed: bool,
}
impl Worktree<'_> {
    fn remove(&mut self) -> Result<(), String> {
        if self.removed {
            return Ok(());
        }
        if !self.cleanup_allowed {
            return Err(format!(
                "process cleanup unconfirmed; retained worktree {}",
                self.path.display()
            ));
        }
        if self.registered {
            if let Err(error) = git(
                self.root,
                &[
                    "worktree",
                    "remove",
                    "--force",
                    self.path.to_str().ok_or("worktree path is not UTF-8")?,
                ],
            ) {
                match fs::remove_dir(&self.path) {
                    Ok(()) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) => {
                        return Err(format!(
                            "{error}; retained nonempty or inaccessible worktree: {e}"
                        ))
                    }
                }
            }
        } else {
            fs::remove_dir(&self.path).map_err(|e| format!("remove reserved worktree: {e}"))?;
        }
        self.removed = true;
        sync_dir(self.path.parent().unwrap())
    }
}
impl Drop for Worktree<'_> {
    fn drop(&mut self) {
        let _ = self.remove();
    }
}

#[cfg(unix)]
mod unix {
    use super::*;
    use std::{
        io,
        os::{fd::AsRawFd, unix::process::CommandExt},
        process::{Child, Command, ExitStatus, Stdio},
        sync::atomic::Ordering,
        thread,
        time::Instant,
    };
    // libc ABI only; no new dependency. These fcntl commands are shared by supported Unix targets.
    unsafe extern "C" {
        fn fcntl(fd: i32, command: i32, ...) -> i32;
        fn kill(pid: i32, signal: i32) -> i32;
    }
    #[cfg(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "freebsd",
        target_os = "openbsd",
        target_os = "netbsd",
        target_os = "dragonfly"
    ))]
    const NONBLOCK: i32 = 4;
    #[cfg(not(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "freebsd",
        target_os = "openbsd",
        target_os = "netbsd",
        target_os = "dragonfly"
    )))]
    const NONBLOCK: i32 = 2048;
    const CLEANUP: Duration = Duration::from_secs(2);

    fn nonblocking(pipe: &impl AsRawFd) -> Result<(), String> {
        // SAFETY: owned pipe descriptor remains live throughout both calls.
        let flags = unsafe { fcntl(pipe.as_raw_fd(), 3) };
        if flags == -1 || unsafe { fcntl(pipe.as_raw_fd(), 4, flags | NONBLOCK) } == -1 {
            return Err(format!(
                "configure output pipe: {}",
                io::Error::last_os_error()
            ));
        }
        Ok(())
    }
    struct Process {
        child: Child,
        cleaned: bool,
    }
    impl Process {
        fn cleanup(&mut self) -> Result<(), String> {
            if self.cleaned {
                return Ok(());
            }
            // SAFETY: this child was spawned with process_group(0); only its owned group is signaled.
            let killed = unsafe { kill(-(self.child.id() as i32), 9) };
            if killed == -1 && io::Error::last_os_error().raw_os_error() != Some(3) {
                return Err(format!(
                    "kill verification process group: {}",
                    io::Error::last_os_error()
                ));
            }
            let started = Instant::now();
            loop {
                if self
                    .child
                    .try_wait()
                    .map_err(|e| format!("reap verification process: {e}"))?
                    .is_some()
                {
                    // Reaping the leader does not prove background descendants have exited.
                    let alive = unsafe { kill(-(self.child.id() as i32), 0) };
                    if alive == -1 && io::Error::last_os_error().raw_os_error() == Some(3) {
                        self.cleaned = true;
                        return Ok(());
                    }
                }
                if started.elapsed() >= CLEANUP {
                    return Err("verification process-group cleanup timeout".into());
                }
                thread::sleep(Duration::from_millis(5));
            }
        }
    }
    impl Drop for Process {
        fn drop(&mut self) {
            let _ = self.cleanup();
        }
    }

    fn isolate_git(command: &mut Command) {
        command.env("GIT_NO_REPLACE_OBJECTS", "1");
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
            command.env_remove(name);
        }
        for (name, _) in std::env::vars_os() {
            if name.to_str().is_some_and(|n| {
                n.starts_with("GIT_CONFIG_KEY_") || n.starts_with("GIT_CONFIG_VALUE_")
            }) {
                command.env_remove(name);
            }
        }
    }
    // Each turn reads a fixed number of chunks, so a noisy stdout cannot starve stderr,
    // cancellation or the deadline. No output-size-dependent memory allocation is needed.
    fn drain(pipe: &mut impl Read, file: &mut File, eof: &mut bool) -> Result<(), String> {
        if *eof {
            return Ok(());
        }
        let mut buffer = [0u8; 8192];
        for _ in 0..8 {
            match pipe.read(&mut buffer) {
                Ok(0) => {
                    *eof = true;
                    break;
                }
                Ok(n) => file
                    .write_all(&buffer[..n])
                    .map_err(|e| format!("write command log: {e}"))?,
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(format!("read command output: {e}")),
            }
        }
        Ok(())
    }
    fn seal(file: &mut File, path: &Path) -> Result<String, String> {
        file.flush()
            .and_then(|()| file.sync_all())
            .map_err(|e| format!("seal command log: {e}"))?;
        let mut reader = File::open(path).map_err(|e| format!("open sealed log: {e}"))?;
        let mut hasher = Sha256::new();
        let mut buffer = [0u8; 8192];
        loop {
            let n = reader
                .read(&mut buffer)
                .map_err(|e| format!("hash sealed log: {e}"))?;
            if n == 0 {
                break;
            }
            hasher.update(&buffer[..n]);
        }
        Ok(hex::encode(hasher.finalize()))
    }
    pub(super) fn execute(
        request: &VerificationRequest,
        command: &VerificationCommand,
        cwd: &Path,
        logs: &Path,
        index: usize,
        safe_to_remove: &mut bool,
    ) -> Result<CommandExecution, String> {
        let stdout_path = logs.join(format!("{index}-stdout.log"));
        let stderr_path = logs.join(format!("{index}-stderr.log"));
        let mut stdout_log = new_file(&stdout_path)?;
        let mut stderr_log = new_file(&stderr_path)?;
        let mut execution = CommandExecution {
            command: command.clone(),
            result: "failed".into(),
            exit_code: None,
            timed_out: false,
            stdout_log: stdout_path.to_str().ok_or("log path is not UTF-8")?.into(),
            stderr_log: stderr_path.to_str().ok_or("log path is not UTF-8")?.into(),
            stdout_sha256: String::new(),
            stderr_sha256: String::new(),
            diagnostic: None,
        };
        if request.cancelled.load(Ordering::Acquire) {
            execution.diagnostic = Some("cancelled before command execution".into());
        } else {
            let mut child_command = Command::new(&command.argv[0]);
            child_command
                .args(&command.argv[1..])
                .current_dir(cwd)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .process_group(0);
            isolate_git(&mut child_command);
            match child_command.spawn() {
                Err(e) => execution.diagnostic = Some(format!("spawn failed: {e}")),
                Ok(child) => {
                    *safe_to_remove = false;
                    let mut process = Process {
                        child,
                        cleaned: false,
                    };
                    let result = capture(&mut process, request, &mut stdout_log, &mut stderr_log);
                    let cleanup = process.cleanup();
                    if cleanup.is_ok() {
                        *safe_to_remove = true;
                    }
                    cleanup?;
                    let (status, timed_out, cancelled) = result?;
                    execution.exit_code = status.and_then(|s| s.code());
                    execution.timed_out = timed_out;
                    if cancelled {
                        execution.diagnostic = Some("cancelled during command execution".into());
                    } else if timed_out {
                        execution.diagnostic = Some("command timeout".into());
                    } else if status.is_some_and(|s| s.success()) {
                        execution.result = "passed".into();
                    } else {
                        execution.diagnostic = Some(format!("command exit failure: {status:?}"));
                    }
                }
            }
        }
        execution.stdout_sha256 = seal(&mut stdout_log, &stdout_path)?;
        execution.stderr_sha256 = seal(&mut stderr_log, &stderr_path)?;
        sync_dir(logs)?;
        Ok(execution)
    }
    fn capture(
        process: &mut Process,
        request: &VerificationRequest,
        stdout_log: &mut File,
        stderr_log: &mut File,
    ) -> Result<(Option<ExitStatus>, bool, bool), String> {
        let mut stdout = process.child.stdout.take().ok_or("missing stdout pipe")?;
        let mut stderr = process.child.stderr.take().ok_or("missing stderr pipe")?;
        nonblocking(&stdout)?;
        nonblocking(&stderr)?;
        let started = Instant::now();
        let (mut stdout_eof, mut stderr_eof) = (false, false);
        let (status, timed_out, cancelled) = loop {
            drain(&mut stdout, stdout_log, &mut stdout_eof)?;
            drain(&mut stderr, stderr_log, &mut stderr_eof)?;
            let status = process
                .child
                .try_wait()
                .map_err(|e| format!("wait command: {e}"))?;
            let cancelled = request.cancelled.load(Ordering::Acquire);
            let timed_out = started.elapsed() >= request.timeout;
            if status.is_some() || cancelled || timed_out {
                break (status, timed_out && status.is_none(), cancelled);
            }
            thread::sleep(Duration::from_millis(2));
        };
        process.cleanup()?;
        let draining = Instant::now();
        while !stdout_eof || !stderr_eof {
            drain(&mut stdout, stdout_log, &mut stdout_eof)?;
            drain(&mut stderr, stderr_log, &mut stderr_eof)?;
            if draining.elapsed() >= CLEANUP {
                return Err("command output drain timeout after process cleanup".into());
            }
            if !stdout_eof || !stderr_eof {
                thread::sleep(Duration::from_millis(2));
            }
        }
        Ok((status, timed_out, cancelled))
    }
}
