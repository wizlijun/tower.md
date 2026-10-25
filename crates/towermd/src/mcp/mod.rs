//! `towermd mcp`: the interaction gateway (spec 16.4, FR-60, FR-63), a
//! minimal hand-written stdio JSON-RPC server one headless run's harness
//! starts. It is only another client of the local daemon: it writes nothing
//! itself, and the daemon signs its events as the run's agent.
mod dispatch;
mod protocol;

use std::cell::RefCell;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::Value;

use crate::daemon::GatewayLink;
use dispatch::{Daemon, Session};

/// How long a review is waited on before the delegate ends its turn.
const REVIEW_MS: u64 = 600_000;

/// Connects on the first tool call, so a harness can list the tools before
/// the daemon answers; a failed connection is tried again next call.
struct Lazy {
    worktree: PathBuf,
    link: RefCell<Option<GatewayLink>>,
}

impl Daemon for Lazy {
    fn post(&self, suffix: &str, body: &Value, timeout: Duration) -> Result<Value, String> {
        let mut link = self.link.borrow_mut();
        if link.is_none() {
            *link = Some(GatewayLink::open(&self.worktree).map_err(|failure| failure.message)?);
        }
        link.as_ref().expect("opened above").post(suffix, body, timeout).map_err(|failure| failure.message)
    }
}

pub fn serve(run: Option<&str>, worktree: &Path, harness: &str) -> Result<(), String> {
    let run = run.ok_or("交互网关须用 --run 指明运行")?;
    let adapter = towermd_devkit::harness::find(harness).ok_or_else(|| format!("未知的 harness 适配器：{harness}"))?;
    let daemon = Lazy { worktree: worktree.to_path_buf(), link: RefCell::new(None) };
    let session = Session { run: run.to_owned(), worktree: worktree.to_path_buf(), adapter, daemon: &daemon, review_ms: REVIEW_MS };
    let stdin = io::stdin();
    let mut stdout = io::stdout().lock();
    for line in stdin.lock().lines() {
        let line = line.map_err(|e| e.to_string())?;
        if line.trim().is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<Value>(&line) {
            Ok(message) => dispatch::handle(&session, &message),
            Err(error) => Some(protocol::error(&Value::Null, -32700, &format!("parse error: {error}"))),
        };
        if let Some(reply) = reply {
            writeln!(stdout, "{reply}").and_then(|()| stdout.flush()).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}
