use std::path::Path;

use crate::catalog::main_reference;
use crate::repo;
use crate::snapshot_git::{fetch_main, Session};

pub fn refresh_origin_main(root: &Path) -> Result<Option<String>, String> {
    if main_reference(root)? != "refs/remotes/origin/main" {
        return Ok(None);
    }
    let reference = "refs/remotes/origin/main";
    if repo::git(root, &["symbolic-ref", "--quiet", reference]).is_ok() {
        return Err("origin/main 是符号引用；拒绝刷新用户分支".into());
    }
    let record = repo::git(
        root,
        &[
            "for-each-ref",
            "--format=%(objectname)%00%(symref)",
            reference,
        ],
    )?;
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
    session.run(
        root,
        &["update-ref", "--no-deref", reference, &fresh, &old],
        None,
    )?;
    Ok(Some(fresh))
}

pub(crate) fn is_explicit_policy_rejection(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    if [
        "authentication failed",
        "could not read username",
        "could not read password",
        "permission denied (publickey)",
        "invalid username or password",
        "terminal prompts disabled",
    ]
    .iter()
    .any(|part| lower.contains(part))
    {
        return false;
    }
    [
        "[remote rejected]",
        "gh006:",
        "gh013:",
        "protected branch",
        "write access to repository not granted",
    ]
    .iter()
    .any(|part| lower.contains(part))
}

#[derive(Debug)]
pub(crate) enum PreflightError {
    Policy(String),
    Unavailable(String),
}

pub(crate) fn push_dry_run(root: &Path, sha: &str) -> Result<String, PreflightError> {
    let session = Session::for_network(root).map_err(PreflightError::Unavailable)?;
    let args = [
        "push",
        "--dry-run",
        "--porcelain",
        "--no-verify",
        "--no-follow-tags",
        "--no-signed",
        "--recurse-submodules=no",
        "origin",
        &format!("{sha}:refs/heads/main"),
    ];
    let output = session
        .run_output(root, &args)
        .map_err(PreflightError::Unavailable)?;
    let diagnostic = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    if output.status.success() {
        return Ok("origin/main 的 push --dry-run 已完成协商；未实际推送，不保证服务端写入策略、保护规则或 hooks 允许真实推送。".into());
    }
    if is_explicit_policy_rejection(&diagnostic) {
        return Err(PreflightError::Policy(format!(
            "origin/main 明确拒绝推送策略；未生成初始化文件：{diagnostic}"
        )));
    }
    Ok(format!(
        "origin/main 的 dry-run 未能确认：{diagnostic}；快照写入时会再次检查"
    ))
}

#[cfg(test)]
mod tests {
    use super::is_explicit_policy_rejection;

    #[test]
    fn only_explicit_server_policy_rejections_are_hard_errors() {
        for text in [
            "!\tsha:refs/heads/main\t[remote rejected] (protected branch)",
            "remote: error: GH006: Protected branch update failed",
            "remote: Write access to repository not granted.",
        ] {
            assert!(is_explicit_policy_rejection(text), "{text}");
        }
        for text in [
            "Authentication failed",
            "Permission denied (publickey)",
            "HTTP 403",
            "permission denied",
            "could not resolve host",
            "!\tsha:refs/heads/main\t[rejected] (non-fast-forward)",
            "Authentication failed: protected branch endpoint unavailable",
        ] {
            assert!(!is_explicit_policy_rejection(text), "{text}");
        }
    }
}
