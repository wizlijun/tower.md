//! A separate CLI process owns verification children. Its stdin remains open
//! only while the daemon is alive; EOF requests cooperative process-group and
//! worktree cleanup even when the daemon itself is killed.
use std::fs::OpenOptions;
use std::io::{BufRead, Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use fs2::FileExt;
use towermd_devkit::verifier::{self, VerificationPlan, VerificationReport};

pub fn serve() -> Result<(), String> {
    // Do not hold StdinLock while running verification. The monitor owns stdin
    // after the first line and is intentionally not joined on normal completion.
    let mut first = String::new();
    std::io::stdin()
        .lock()
        .read_line(&mut first)
        .map_err(|e| e.to_string())?;
    if first.len() > 16 * 1024 * 1024 {
        return Err("验证计划过大".into());
    }
    let plan: VerificationPlan =
        serde_json::from_str(&first).map_err(|e| format!("验证计划无效：{e}"))?;
    let cancelled = Arc::new(AtomicBool::new(false));
    let monitor = Arc::clone(&cancelled);
    std::thread::spawn(move || {
        let mut line = String::new();
        // EOF, cancellation, malformed control data and I/O failure all stop
        // execution. There is no command for disabling the parent monitor.
        let _ = std::io::stdin().lock().read_line(&mut line);
        monitor.store(true, Ordering::SeqCst);
    });
    let result = (|| {
        let runtime = std::env::var_os("TOWERMD_RUNTIME_DIR").ok_or("验证 worker 缺少运行目录")?;
        let lock_path = Path::new(&runtime).join("verification.lock");
        if std::fs::symlink_metadata(&lock_path).is_ok_and(|m| !m.file_type().is_file()) {
            return Err("验证锁不是普通文件".into());
        }
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(lock_path)
            .map_err(|e| e.to_string())?;
        loop {
            if cancelled.load(Ordering::SeqCst) {
                return Err("等待验证执行锁时已取消".into());
            }
            match FileExt::try_lock_exclusive(&lock) {
                Ok(()) => break,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(50))
                }
                Err(error) => return Err(format!("无法取得验证执行锁：{error}")),
            }
        }
        // Keep the cross-process lock until the verifier has also cleaned up.
        let report = verifier::run(&plan, &cancelled);
        drop(lock);
        report
    })();
    serde_json::to_writer(std::io::stdout().lock(), &result).map_err(|e| e.to_string())?;
    std::io::stdout()
        .lock()
        .write_all(b"\n")
        .map_err(|e| e.to_string())
}

fn read_bounded(mut stream: impl Read, limit: usize) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    let mut block = [0u8; 8192];
    let mut exceeded = false;
    loop {
        let count = stream.read(&mut block).map_err(|e| e.to_string())?;
        if count == 0 {
            break;
        }
        if bytes.len() + count <= limit {
            bytes.extend_from_slice(&block[..count]);
        } else {
            exceeded = true;
        }
    }
    if exceeded {
        return Err("验证 worker 输出超出上限".into());
    }
    Ok(bytes)
}

pub fn run(
    plan: &VerificationPlan,
    cancelled: &AtomicBool,
    runtime: &Path,
) -> Result<VerificationReport, String> {
    // This module is compiled into the CLI only. Never invoke the App binary.
    let executable = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut child = Command::new(executable)
        .arg("verification-worker")
        .env("TOWERMD_RUNTIME_DIR", runtime)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("无法启动验证 worker：{e}"))?;
    let stdout = child.stdout.take().ok_or("验证 worker 缺少输出管道")?;
    let stderr = child.stderr.take().ok_or("验证 worker 缺少错误管道")?;
    let output_reader = std::thread::spawn(move || read_bounded(stdout, 16 * 1024 * 1024));
    let error_reader = std::thread::spawn(move || read_bounded(stderr, 1024 * 1024));
    let mut input = child.stdin.take().ok_or("验证 worker 缺少控制管道")?;
    let mut bytes = serde_json::to_vec(plan).map_err(|e| e.to_string())?;
    bytes.push(b'\n');
    if let Err(error) = input.write_all(&bytes).and_then(|()| input.flush()) {
        drop(input);
        let _ = child.wait();
        let _ = output_reader.join();
        let _ = error_reader.join();
        return Err(format!("无法发送验证计划：{error}"));
    }
    let mut cancellation_sent = false;
    let status = loop {
        if cancelled.load(Ordering::SeqCst) && !cancellation_sent {
            let _ = input.write_all(b"cancel\n").and_then(|()| input.flush());
            cancellation_sent = true;
        }
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(error) => {
                // Dropping the control pipe asks the worker to clean up. Do not
                // kill it, because it owns the verification process group.
                drop(input);
                let _ = child.wait();
                break Err(error.to_string());
            }
        }
    };
    let output = output_reader.join().map_err(|_| "验证输出线程退出")??;
    let errors = error_reader.join().map_err(|_| "验证错误线程退出")??;
    if !status?.success() {
        return Err(format!(
            "验证 worker 意外退出：{}",
            String::from_utf8_lossy(&errors).trim()
        ));
    }
    serde_json::from_slice::<Result<VerificationReport, String>>(&output)
        .map_err(|e| format!("验证 worker 返回格式无效：{e}"))?
}
