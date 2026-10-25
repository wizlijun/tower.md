//! Decision review (core spec FR-186). Keeping or voiding a decision that
//! came due is a person's act: only the App reaches the route, signed
//! `human:local`; the CLI has none. Rewriting is answering the question anew.
use super::business::{now, ready, uncertain};
use super::*;
use towermd_core::{catalog, config::{Config, CONFIG_PATH}, decision_review::{self, Reviewed}, human_decisions, hub_protocol::NewEvent,
    okf::Document, project_runtime::RuntimeError};

use std::time::Instant;

const HUMAN: &str = "human:local";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Review { request_id: String, outcome: Outcome, #[serde(default)] review_on: Option<String> }

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum Outcome { Keep, Void }

impl Review {
    fn outcome(&self, document: bool) -> Result<Reviewed, String> {
        match (&self.outcome, &self.review_on) {
            (Outcome::Keep, review_on) => Ok(Reviewed::Keep { review_on: review_on.clone(), document }),
            (Outcome::Void, None) => Ok(Reviewed::Void { document }),
            (Outcome::Void, Some(_)) => Err("作废的决策不再复查，不能带复查日期".into()),
        }
    }
}

/// Decision documents with `review_on`, read once per main revision; main is
/// looked up at most every `RECHECK` on reads, so polling the state does not
/// spawn git each time.
static DOCUMENTS: Mutex<BTreeMap<PathBuf, (Instant, String, Vec<decision_review::Document>)>> = Mutex::new(BTreeMap::new());
const RECHECK: Duration = Duration::from_secs(2);

/// FR-186: the decision documents under `<docs>/decisions/` at the local
/// main whose frontmatter has an `id` and a `review_on` day, such as those an
/// approved proposal wrote. Nothing is stored; an unreadable main has none.
pub(super) fn documents(project: &Project) -> Vec<decision_review::Document> {
    read_cached(project, RECHECK)
}

fn read_cached(project: &Project, recheck: Duration) -> Vec<decision_review::Document> {
    let mut cache = match DOCUMENTS.lock() { Ok(cache) => cache, Err(_) => return vec![] };
    if let Some((checked, _, found)) = cache.get(&project.root) {
        if checked.elapsed() < recheck { return found.clone() }
    }
    let Ok(main) = super::source_frames::local_main(&project.root) else { cache.remove(&project.root); return vec![] };
    let found = match cache.get(&project.root) {
        Some((_, revision, found)) if *revision == main => found.clone(),
        _ => read_documents(&project.root, &main).unwrap_or_default(),
    };
    cache.insert(project.root.clone(), (Instant::now(), main, found.clone()));
    found
}

fn read_documents(root: &Path, main: &str) -> Result<Vec<decision_review::Document>, String> {
    let Some(config) = catalog::read_file_at(root, main, CONFIG_PATH)? else { return Ok(vec![]) };
    let dir = format!("{}/decisions", Config::parse(&config)?.docs_dir());
    Ok(catalog::files_at(root, main, &dir)?.into_iter().filter(|(file, _)| file.ends_with(".md")).filter_map(|(file, text)| {
        let document = Document::parse(&text?).ok()?;
        decision_review::Document::new(document.get_str("id")?, document.get_str("title"), document.get_str("review_on")?, file)
    }).collect())
}

fn invalid(error: RuntimeError) -> ApiError {
    match error { RuntimeError::Invalid(message) => bad(message), other => uncertain(other) }
}

pub async fn review(State(server): State<Arc<Server>>, headers: HeaderMap, RoutePath((handle, id)): RoutePath<(String, String)>, Json(input): Json<Review>) -> Result<Json<Value>, ApiError> {
    if !crate::daemon::authorized_app(&headers, &server) { return Err(denied()); }
    valid_scan_id(&input.request_id).map_err(bad)?;
    input.outcome(false).map_err(rejected_as(ErrorCode::Usage))?;
    let project = project(&server, &handle).map_err(bad)?;
    tokio::task::spawn_blocking(move || {
        let mut slot = project.runtime.lock().map_err(|_| bad("项目会话不可用"))?;
        let runtime = ready(&mut slot, &server, &project).map_err(bad)?;
        // Not a recorded decision: a decision document on main, marked so on the event.
        let recorded = [decision_review::DECISIONS, human_decisions::GROUP].iter()
            .any(|group| runtime.projection().get(group).and_then(|group| group.get(&id)).is_some());
        let data = serde_json::to_value(input.outcome(!recorded).map_err(rejected_as(ErrorCode::Usage))?).map_err(bad)?;
        let request = match human_tasks::original(runtime, &input.request_id).map_err(bad)? {
            Some(old) if old.host == server.endpoint.host && old.actor == HUMAN && old.kind == decision_review::REVIEWED && old.obj == id && old.data == data => old,
            Some(_) => return Err(rejected_as(ErrorCode::Conflict)("请求 ID 已用于其他内容")),
            None => {
                // Checked against main as it is now, not the last read.
                if !recorded && !read_cached(&project, Duration::ZERO).iter().any(|document| document.id == id) {
                    return Err(bad("要复查的决策不存在"));
                }
                NewEvent { eid: input.request_id.clone(), host: server.endpoint.host.clone(), actor: HUMAN.into(),
                    kind: decision_review::REVIEWED.into(), layer: decision_review::LAYER.into(), obj: id, data, guards: vec![] }
            }
        };
        // The reducer refuses a decision that is missing, superseded or already voided.
        let event = runtime.append(request, now).map_err(invalid)?;
        Ok(Json(json!({"request_id":event.eid,"event":event.eid,"seq":event.seq,"status":"accepted",
            "review":runtime.projection().get(decision_review::GROUP).and_then(|group| group.get(&event.obj)).cloned()})))
    }).await.map_err(uncertain)?
}
