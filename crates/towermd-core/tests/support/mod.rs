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

// Test ownership adapter: every business request uses the public shared proof boundary.
pub struct Session {
    pub cell: std::sync::Mutex<ProjectSession>,
    pub root: std::path::PathBuf,
}
impl Session {
    pub fn new(
        hub: Box<dyn towermd_core::hub_protocol::HubProtocol>,
        catalog: &Catalog,
        inputs: &RecoveryInputs,
        now: u64,
    ) -> Result<Self, towermd_core::project_session::SessionSyncError> {
        ProjectSession::new(hub, catalog, inputs, now).map(|session| Self {
            cell: std::sync::Mutex::new(session),
            root: catalog.root.clone(),
        })
    }
    pub fn execute(
        &self,
        c: &Catalog,
        host: &str,
        command: towermd_core::tasks::TaskCommand,
        run: Option<&str>,
        now: u64,
    ) -> Result<serde_json::Value, towermd_core::tasks::TaskFailure> {
        ProjectSession::execute_shared(&self.cell, c, &inputs(c), host, command, run, now)
    }
    pub fn read_status(
        &self,
        c: &Catalog,
        host: &str,
        run: Option<&str>,
        now: u64,
    ) -> Result<serde_json::Value, towermd_core::tasks::TaskFailure> {
        ProjectSession::read_status_shared(&self.cell, c, &inputs(c), host, run, now)
    }
    pub fn projection(&self) -> Result<Box<towermd_core::snapshot::StateProjection>, String> {
        self.cell
            .lock()
            .unwrap()
            .projection()
            .map(|projection| Box::new(projection.clone()))
    }
    pub fn pending_claim(&self) -> Result<Option<towermd_core::tasks::PendingClaim>, String> {
        self.cell
            .lock()
            .unwrap()
            .pending_claim()
            .map(|pending| pending.cloned())
    }
    pub fn last_seq(&self) -> u64 {
        self.cell.lock().unwrap().last_seq()
    }
    pub fn events(&self, after: u64) -> Vec<towermd_core::hub_protocol::Event> {
        self.cell.lock().unwrap().events(after)
    }
    pub fn leases(
        &self,
        now: u64,
    ) -> std::collections::BTreeMap<String, towermd_core::hub_protocol::Lease> {
        self.cell.lock().unwrap().leases(now)
    }
}
pub fn open_session(path: &Path, c: &Catalog, now: u64) -> Session {
    Session {
        cell: std::sync::Mutex::new(open(path, c, now)),
        root: c.root.clone(),
    }
}
pub fn sync_session(session: Session, c: &Catalog, now: u64) -> Session {
    ProjectSession::sync_shared(&session.cell, c, &inputs(c), now).unwrap();
    session
}
