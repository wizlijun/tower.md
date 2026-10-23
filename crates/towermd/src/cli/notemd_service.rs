//! FR-80 writeback: merged tasks and accepted decisions become plain `.md`
//! summaries in the Note.md vault named by `notemd.vault` on main. Nothing
//! is written while no vault is configured; the vault's git is never run.
use super::*;
use towermd_core::{catalog, config::{Config, CONFIG_PATH}, notemd::{self, Writeback}, project_runtime::ProjectRuntime};
use towermd_devkit::notemd_summary;

pub(super) fn writeback(server: &Server, project: &Project, runtime: &ProjectRuntime) -> Result<(), String> {
    let Some(revision) = catalog::authority_revision(&project.root)? else { return Ok(()) };
    let Some(text) = catalog::read_file_at(&project.root, &revision, CONFIG_PATH)? else { return Ok(()) };
    let config = Config::parse(&text)?;
    let Some(raw) = config.notemd_vault() else { return Ok(()) };
    let vault = notemd::resolve_vault(raw, &project.root, dirs::home_dir().as_deref())
        .ok_or("无法解析 notemd.vault：找不到用户主目录")?;
    let key = format!("{:x}", Sha256::digest(project.common.to_string_lossy().as_bytes()));
    let ledger = server.runtime.join("notemd").join(format!("{key}.json"));
    let mut writeback = Writeback::open(&vault, &ledger, &project.root)?;
    let mut errors = Vec::new();
    for capture in notemd_summary::task_summaries(runtime.projection(), config.project_id()) {
        offer(&mut writeback, &capture.name, || Ok(capture.clone()), &mut errors);
    }
    // Decisions only change with main, so they are offered once per commit.
    if writeback.scanned() != Some(revision.as_str()) {
        let catalog = Catalog::load(&project.root)?;
        let before = errors.len();
        for (id, document) in &catalog.objects {
            let Some(name) = notemd::decision_name(document) else { continue };
            offer(&mut writeback, &name, || {
                let path = catalog.object_paths.get(id).ok_or("决策路径缺失")?;
                let commit = repo::git(&catalog.root, &["log", "-1", "--format=%H", &catalog.revision, "--", path])?;
                notemd::decision_summary(config.project_id(), document, &commit).ok_or_else(|| format!("决策 {id} 无法生成摘要"))
            }, &mut errors);
        }
        if errors.len() == before {
            writeback.set_scanned(&catalog.revision);
        }
    }
    writeback.save()?;
    if errors.is_empty() { Ok(()) } else { Err(errors.join("；")) }
}

fn offer(
    writeback: &mut Writeback,
    name: &str,
    build: impl FnOnce() -> Result<notemd::Capture, String>,
    errors: &mut Vec<String>,
) {
    if writeback.baseline() {
        writeback.mark(name);
    } else if writeback.pending(name) {
        if let Err(error) = build().and_then(|capture| writeback.write(&capture)) {
            errors.push(error);
        }
    }
}
