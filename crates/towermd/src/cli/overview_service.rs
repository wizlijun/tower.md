//! Multi-project overview (core spec FR-133): every Tower.md project this
//! user opened on this machine, each with its health colour, the number of
//! things that need the person and the latest activity, plus one
//! cross-project attention list that still shows at most three. Read-only and
//! derived on every call from `views.json`, the open projects and each
//! project's state body; nothing is stored. A project that was never
//! activated stays unactivated: its state read creates no hub.
use super::business::{now, read_state};
use super::*;
use towermd_core::{attention_list, config::Config};
use towermd_devkit::dev_state::KITS;

pub async fn overview(State(server): State<Arc<Server>>, headers: HeaderMap) -> Result<Json<Value>, ApiError> {
    if !authorized_client(&headers, &server) {
        return Err(denied());
    }
    tokio::task::spawn_blocking(move || Json(collect(&server))).await.map_err(super::business::uncertain)
}

fn state_of(server: &Server, root: &Path) -> Result<Value, String> {
    let handle = open_root(server, root)?;
    let project = project(server, &handle)?;
    read_state(server, &project).map_err(|(_, Json(body))| body["error"].as_str().unwrap_or("项目状态读取失败").to_owned())
}

/// The command that opens an item of `project` from any directory.
fn command(project: &str, state: &Value, entry: &Value) -> String {
    let local = crate::read::command(state, entry);
    format!("towermd --project {project} {}", local.strip_prefix("towermd ").unwrap_or(&local))
}

fn collect(server: &Server) -> Value {
    let mut known = crate::views::load();
    if let Ok(registry) = server.projects.lock() {
        for project in registry.handles.values() {
            if let Some(id) = &project.project_id {
                known.entry(id.clone()).or_insert_with(|| crate::views::View { root: project.root.clone(), last_view: None });
            }
        }
    }
    let now = now();
    let mut states = BTreeMap::new();
    let mut lists = Vec::new();
    let projects: Vec<Value> = known.into_iter().map(|(id, view)| {
        let present = Config::load(&view.root).ok().flatten().is_some_and(|config| config.project_id() == id);
        let mut entry = json!({"id": id, "root": view.root, "present": present, "status": "unknown", "health": null,
            "needs": null, "last_activity": null, "top": null});
        if !present {
            return entry;
        }
        match state_of(server, &view.root) {
            Ok(state) => {
                entry["status"] = state["status"].clone();
                if state["status"] == "ready" {
                    let needs = crate::read::needs(&state, false);
                    entry["health"] = state["health"].clone();
                    entry["needs"] = needs["total"].clone();
                    entry["last_activity"] = state["last_activity"].clone();
                    if let Some(top) = needs["items"].get(0) {
                        let mut top = top.clone();
                        top["cmd"] = json!(command(&id, &state, &top));
                        entry["top"] = top;
                    }
                    lists.push((id.clone(), attention_list::list(&state, KITS, now)));
                    states.insert(id, state);
                }
            }
            Err(error) => entry["error"] = json!(error),
        }
        entry
    }).collect();
    let across = attention_list::across(&lists);
    let total = across.items.len();
    let items: Vec<Value> = across.items.iter().take(across.shown).map(|item| {
        let mut value = serde_json::to_value(item).unwrap_or(Value::Null);
        value["cmd"] = json!(command(&item.project, &states[&item.project], &value));
        value
    }).collect();
    json!({"projects": projects, "attention": {"items": items, "shown": across.shown, "total": total,
        "later": total - across.shown, "budget": across.budget}})
}
