//! S6 endpoint adapter. Core owns validation, projection and private Git storage.
use super::*;
use towermd_core::{baseline, draft_store};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Submit {
    producer: String,
    submission: Value,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Review {
    action: towermd_core::review::ReviewAction,
}

pub async fn review(
    State(server): State<Arc<Server>>,
    headers: HeaderMap,
    RoutePath((handle, id)): RoutePath<(String, String)>,
    Json(input): Json<Review>,
) -> Result<Json<Value>, ApiError> {
    if !crate::daemon::authorized_app(&headers, &server) {
        return Err(denied());
    }
    draft_store::valid_id(&id).map_err(bad)?;
    let project = project(&server, &handle).map_err(bad)?;
    tokio::task::spawn_blocking(move || {
        let catalog = current_catalog(&project).map_err(bad)?;
        let prepared = draft_store::prepare_review(&catalog, &id, input.action).map_err(bad)?;
        let uncertain = |error: String| (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("审阅保存结果未确认，请用原草稿和相同操作重试：{error}")})),
        );
        let draft = draft_store::persist_review(&catalog, prepared).map_err(uncertain)?;
        draft_store::push_if_remote(&catalog, &draft.id).map_err(uncertain)?;
        response(&project, &current_catalog(&project).map_err(uncertain)?, draft)
            .map(Json).map_err(uncertain)
    })
    .await
    .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": error.to_string()}))))?
}

fn response(
    project: &Project,
    catalog: &Catalog,
    draft: baseline::BaselineDraft,
) -> Result<Value, String> {
    let evidence_error = cold_start::validate_verification_logs(&draft.scan, &project.common).err();
    let stale = catalog.revision != draft.scan.revision;
    let mut value = serde_json::to_value(draft).map_err(|e| e.to_string())?;
    value["evidence_error"] = json!(evidence_error);
    value["stale"] = json!(stale);
    Ok(value)
}

pub async fn submit(
    State(server): State<Arc<Server>>,
    headers: HeaderMap,
    RoutePath(handle): RoutePath<String>,
    Json(input): Json<Submit>,
) -> Result<Json<Value>, ApiError> {
    if !authorized_client(&headers, &server) {
        return Err(denied());
    }
    let project = project(&server, &handle).map_err(bad)?;
    tokio::task::spawn_blocking(move || {
        let catalog = current_catalog(&project)?;
        let submission = baseline::parse(input.submission)?;
        let id = draft_store::submission_id(&submission, &input.producer)?;
        let draft = if draft_store::private_commit(&project.root, &id)?.is_some() {
            // A lost reply can be retried even after scan caches have been removed.
            draft_store::load(&catalog, &id)?
        } else {
            let scan = load_report(&project, &submission.scan_id)?;
            if scan.revision != catalog.revision || !catalog.matches_current_source()? {
                return Err("扫描已过期，请重新扫描当前权威提交后综合".into());
            }
            draft_store::save(&catalog, &scan, submission, &input.producer)?
        };
        draft_store::push_if_remote(&catalog, &id)?;
        response(&project, &current_catalog(&project)?, draft).map(Json)
    })
    .await
    .map_err(bad)?
    .map_err(bad)
}

pub async fn list(
    State(server): State<Arc<Server>>,
    headers: HeaderMap,
    RoutePath(handle): RoutePath<String>,
) -> Result<Json<Value>, ApiError> {
    if !authorized_client(&headers, &server) {
        return Err(denied());
    }
    let project = project(&server, &handle).map_err(bad)?;
    tokio::task::spawn_blocking(move || {
        let catalog = current_catalog(&project)?;
        let drafts = draft_store::list(&catalog)?;
        Ok::<_, String>(Json(json!(drafts
            .into_iter()
            .map(|draft| json!({
                "id": draft.id, "producer": draft.producer, "created_at": draft.created_at,
                "source_revision": draft.scan.revision, "title": draft.view.title,
                "stale": draft.scan.revision != catalog.revision,
                "reviewed": draft.review.is_some(),
            }))
            .collect::<Vec<_>>())))
    })
    .await
    .map_err(bad)?
    .map_err(bad)
}

pub async fn get(
    State(server): State<Arc<Server>>,
    headers: HeaderMap,
    RoutePath((handle, id)): RoutePath<(String, String)>,
) -> Result<Json<Value>, ApiError> {
    if !authorized_client(&headers, &server) {
        return Err(denied());
    }
    draft_store::valid_id(&id).map_err(bad)?;
    let project = project(&server, &handle).map_err(bad)?;
    tokio::task::spawn_blocking(move || {
        let catalog = current_catalog(&project)?;
        response(&project, &catalog, draft_store::load(&catalog, &id)?).map(Json)
    })
    .await
    .map_err(bad)?
    .map_err(bad)
}

pub async fn template(
    State(server): State<Arc<Server>>,
    headers: HeaderMap,
    RoutePath((handle, id)): RoutePath<(String, String)>,
) -> Result<Json<Value>, ApiError> {
    if !authorized_client(&headers, &server) {
        return Err(denied());
    }
    valid_scan_id(&id).map_err(bad)?;
    let project = project(&server, &handle).map_err(bad)?;
    tokio::task::spawn_blocking(move || {
        let catalog = current_catalog(&project)?;
        let scan = load_report(&project, &id)?;
        if scan.revision != catalog.revision {
            return Err("扫描已过期，请重新扫描".into());
        }
        serde_json::to_value(baseline::template(&scan)?)
            .map(Json)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(bad)?
    .map_err(bad)
}
