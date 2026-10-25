//! Maintenance (spec 9.3): the growth signals, raised signed
//! `process:towermd` once a project has an approved baseline. Only changes
//! are raised: a signal crossing its threshold, and one falling back. The
//! layout of `main` is measured once per inspected revision and kept in
//! memory only; it is rebuilt from Git after a restart.
use super::business::now;
use super::*;
use towermd_core::{config::Config, project_runtime::ProjectRuntime, tower_api};
use towermd_devkit::{growth::{self, Layout, Settings}, run_record};

/// Signals written per maintenance pass; the next pass continues.
const BATCH: usize = 16;

/// Per project root: the baseline and inspected `main` the layout was measured on.
static LAYOUTS: Mutex<BTreeMap<PathBuf, (String, String, Layout)>> = Mutex::new(BTreeMap::new());

fn layout(project: &Project, runtime: &ProjectRuntime) -> Result<Option<Layout>, String> {
    let Some(baseline) = runtime.projection()["Baseline"].as_object().and_then(|records| records.keys().max_by_key(|id| {
        id.strip_prefix('B').and_then(|number| number.parse::<u64>().ok())
    })).cloned() else { return Ok(None) };
    let Some(main) = runtime.projection()["MainInspection"]["main"].as_str().map(str::to_owned) else { return Ok(None) };
    let mut layouts = LAYOUTS.lock().map_err(|_| "生长信号缓存不可用")?;
    if let Some((cached_baseline, cached_main, layout)) = layouts.get(&project.root) {
        if *cached_baseline == baseline && *cached_main == main {
            return Ok(Some(layout.clone()));
        }
    }
    let approved = super::tasks::baseline(project, runtime)?;
    let catalog = current_catalog(project)?;
    let layout = growth::layout(&catalog, approved.modules())?;
    layouts.insert(project.root.clone(), (baseline, main, layout.clone()));
    Ok(Some(layout))
}

/// Whether each task declared `touches`, in the order the hub created them.
fn recent(runtime: &ProjectRuntime) -> Vec<bool> {
    let mut created: Vec<_> = runtime.canonical_receipts().filter(|event| event.kind == "task.created").map(|event| (event.seq, event.obj.clone())).collect();
    created.sort();
    created.into_iter().map(|(_, task)| {
        runtime.projection()["Task"][&task]["input"]["touches"].as_array().is_some_and(|touches| !touches.is_empty())
    }).collect()
}

pub(super) fn write(server: &Server, project: &Project, runtime: &mut ProjectRuntime) -> Result<usize, String> {
    let Some(project_id) = project.project_id.as_deref() else { return Ok(0) };
    let Some(layout) = layout(project, runtime)? else { return Ok(0) };
    let config = Config::load(&project.root).ok().flatten();
    let settings = config.as_ref().map_or_else(Settings::default, |config| Settings::from_config(config.section("signals"), config.section("growth")));
    let records = run_record::records(runtime.projection())?;
    let found = growth::findings(&settings, project_id, &layout, &recent(runtime), &records, now());
    let raises = growth::transitions(&tower_api::read(runtime.projection())?, found);
    let mut written = 0;
    for raise in raises.into_iter().take(BATCH) {
        let refs = vec![format!("main:{}", layout.revision)];
        super::requests::raise(runtime, &server.endpoint.host, raise.eid,
            || tower_api::tower_signal(project_id, &raise.target, refs, tower_api::timestamp(now())?, &raise.signal))?;
        written += 1;
    }
    Ok(written)
}
