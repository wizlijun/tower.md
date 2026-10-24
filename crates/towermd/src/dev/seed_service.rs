//! `POST /seed` (spec 9.1): the seed plan for a project with no approved
//! version yet. It reads and writes nothing: the delegate builds the skeleton
//! and the S6 submission from it, and only the person's approval makes B0.
use super::business::{ready, uncertain, open_failure};
use super::*;
use towermd_core::ideas;
use towermd_devkit::seed;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SeedBody {
    #[serde(default)]
    text: Option<String>,
}

pub async fn seed(State(server): State<Arc<Server>>, headers: HeaderMap, RoutePath(handle): RoutePath<String>, Json(input): Json<SeedBody>) -> Result<Json<Value>, ApiError> {
    if !crate::daemon::authorized_client(&headers, &server) {
        return Err(denied());
    }
    let project = project(&server, &handle).map_err(bad)?;
    tokio::task::spawn_blocking(move || {
        let mut slot = project.runtime.lock().map_err(|_| bad("项目会话不可用"))?;
        let runtime = ready(&mut slot, &server, &project).map_err(open_failure)?;
        if runtime.projection()["Baseline"].as_object().is_some_and(|records| !records.is_empty()) {
            return Err(rejected_as(ErrorCode::NeedsProposal)("项目已有经批准的版本：一句话生长只用于第一个版本，之后请用提案生长（towermd propose）"));
        }
        let ideas = ideas::read(runtime.projection()).map_err(bad)?;
        let plan = seed::plan(input.text.as_deref(), &ideas).map_err(rejected_as(ErrorCode::Usage))?;
        Ok(Json(json!(plan)))
    }).await.map_err(uncertain)?
}
