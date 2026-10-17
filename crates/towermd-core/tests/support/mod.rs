#![allow(dead_code)]
use std::path::Path;
use towermd_core::{
    catalog::Catalog, project_session::ProjectSession, recovery_outbox::RecoveryInputs,
};
pub fn git(root: &Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git")
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
        out.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap().trim_end().into()
}
pub fn catalog(root: &Path) -> Catalog {
    if !root.join(".git").exists() {
        git(root, &["init", "-qb", "main"]);
        towermd_core::init::init(root, &towermd_core::init::InitOptions { docs_dir: None })
            .unwrap();
        std::fs::write(
            root.join(".towermd/config.yaml"),
            towermd_core::config::Config::new("project", "docs/tower")
                .unwrap()
                .to_yaml(),
        )
        .unwrap();
        for (id, kind, fields) in [
            ("A", "Module", "paths: ['src/**']"),
            ("B", "Module", "paths: ['src/b/**']"),
            ("AC", "Acceptance", "module: A\ncriteria: [works]"),
        ] {
            std::fs::write(
                root.join(format!("docs/tower/{id}.md")),
                format!("---\ntype: {kind}\nid: {id}\n{fields}\n---\n"),
            )
            .unwrap();
        }
        git(root, &["add", ".towermd", "docs"]);
        git(root, &["commit", "-qm", "actual session test context"]);
        git(root, &["switch", "-qc", "t/T"]);
    }
    Catalog::load(root).unwrap()
}
pub fn inputs(c: &Catalog) -> RecoveryInputs {
    RecoveryInputs::read(&c.root.join("test-outbox"), c).unwrap()
}
pub fn open(path: &Path, c: &Catalog, now: u64) -> ProjectSession {
    ProjectSession::open(path, c, &inputs(c), now).unwrap()
}
pub fn sync(session: ProjectSession, c: &Catalog, now: u64) -> ProjectSession {
    let cell = std::sync::Mutex::new(session);
    ProjectSession::sync_shared(&cell, c, &inputs(c), now).unwrap();
    cell.into_inner().unwrap()
}
