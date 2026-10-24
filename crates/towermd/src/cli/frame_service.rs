//! Templates and frames (core spec FR-141–146, FR-153). Anyone the daemon
//! knows may list templates. Creating, moving or resizing a frame is a
//! person's act: only the App reaches those routes, signed `human:local`,
//! and the CLI has no command for them (CLI spec, 2026-10-23 ruling). The
//! App names a template; the daemon reads it from what ships and what is
//! installed on main, and freezes it into the event, so the App can never
//! supply a definition of its own.
//!
//! A person's card writes, rule changes and decisions on waiting items
//! (H2) and card pins (H3) are App routes too, signed `human:local`, without the frame lease.
//! Delegates write cards through the Tower API's `card` verb instead.
use super::business::{now, ready, uncertain};
use super::*;
use towermd_core::{
    cards::{self, Op, Pinned, Resolved, RulesChanged},
    extensions::Registry,
    frame_history, frame_text, frame_views,
    frames::{self, Created, Frozen, Mode, Place, Placed, Rules, RulesInput, SetBy},
    hub_protocol::NewEvent,
    project_runtime::RuntimeError,
};

const HUMAN: &str = "human:local";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Create {
    request_id: String,
    id: String,
    title: String,
    template: String,
    #[serde(default)]
    set_by: Option<SetBy>,
    #[serde(default)]
    rules: RulesInput,
    place: Place,
    /// FR-148; free when unstated.
    #[serde(default)]
    mode: Option<Mode>,
    #[serde(default)]
    objects: Vec<String>,
    /// FR-144: pasted Markdown or Mermaid the new free frame starts with.
    #[serde(default)]
    text: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Move {
    request_id: String,
    place: Place,
}

fn invalid(error: RuntimeError) -> ApiError {
    match error {
        RuntimeError::Invalid(message) => bad(message),
        other => uncertain(other),
    }
}

pub async fn templates(State(server): State<Arc<Server>>, headers: HeaderMap, RoutePath(handle): RoutePath<String>) -> Result<Json<Value>, ApiError> {
    if !authorized_client(&headers, &server) {
        return Err(denied());
    }
    let project = project(&server, &handle).map_err(bad)?;
    tokio::task::spawn_blocking(move || {
        let registry = Registry::installed(&project.root).map_err(uncertain)?;
        Ok(Json(crate::templates::list(&registry)))
    }).await.map_err(uncertain)?
}

/// The `frame.created` body for this request, from the template as it is now.
fn created(project: &Project, input: &Create) -> Result<Created, ApiError> {
    let registry = Registry::installed(&project.root).map_err(uncertain)?;
    let entry = crate::templates::available(&registry).into_iter().find(|entry| entry.id == input.template)
        .ok_or_else(|| rejected_as(ErrorCode::NotFound)(format!("没有模板 {}", input.template)))?;
    let template = entry.template.map_err(|problems| bad(format!("模板 {} 有问题，不能使用：{}", input.template, problems.join("；"))))?;
    let rules = template.rules(input.rules.clone()).map_err(rejected_as(ErrorCode::Usage))?;
    let body = Created {
        title: input.title.clone(),
        template: Frozen { id: template.id.clone(), version: template.version.clone(), definition: entry.definition },
        set_by: input.set_by.unwrap_or(template.default_set_by),
        mode: input.mode.unwrap_or(Mode::Free),
        rules,
        place: input.place,
        objects: input.objects.clone(),
        cards: Vec::new(),
    };
    let mut body = body;
    // FR-144: the text is read once, here; the event carries the writes, so
    // replay never depends on the reader.
    if let Some(text) = input.text.as_deref().filter(|text| !text.trim().is_empty()) {
        if body.mode != Mode::Free {
            return Err(rejected_as(ErrorCode::Usage)("只有自由容器可以从文本导入"));
        }
        let fields: Vec<String> = template.card_fields.keys().cloned().collect();
        body.cards = frame_text::parse(text).and_then(|parsed| frame_text::writes(&parsed, &input.id, &template, &fields))
            .map_err(|problems| rejected_as(ErrorCode::Usage)(format!("文本不能导入：{}", problems.join("；"))))?;
    }
    frames::check_created(&body).map_err(rejected_as(ErrorCode::Usage))?;
    Ok(body)
}

pub async fn create(State(server): State<Arc<Server>>, headers: HeaderMap, RoutePath(handle): RoutePath<String>, Json(input): Json<Create>) -> Result<Json<Value>, ApiError> {
    if !crate::daemon::authorized_app(&headers, &server) {
        return Err(denied());
    }
    valid_scan_id(&input.request_id).map_err(bad)?;
    frames::check_id(&input.id).map_err(rejected_as(ErrorCode::Usage))?;
    frames::check_title(&input.title).map_err(rejected_as(ErrorCode::Usage))?;
    let project = project(&server, &handle).map_err(bad)?;
    tokio::task::spawn_blocking(move || {
        let mut slot = project.runtime.lock().map_err(|_| bad("项目会话不可用"))?;
        let runtime = ready(&mut slot, &server, &project).map_err(bad)?;
        let request = match human_tasks::original(runtime, &input.request_id).map_err(bad)? {
            // A retry is the same person's same request, whatever main holds now.
            Some(old) => {
                let same = serde_json::from_value::<Created>(old.data.clone()).is_ok_and(|body| body.title == input.title
                    && body.template.id == input.template && body.place == input.place
                    && input.set_by.is_none_or(|set_by| set_by == body.set_by)
                    && body.cards.is_empty() == input.text.as_deref().is_none_or(|text| text.trim().is_empty()));
                if old.host != server.endpoint.host || old.actor != HUMAN || old.kind != frames::CREATED || old.obj != input.id || !same {
                    return Err(rejected_as(ErrorCode::Conflict)("请求 ID 已用于其他内容"));
                }
                old
            }
            None => {
                if runtime.projection().get(frames::GROUP).is_some_and(|group| group.get(&input.id).is_some()) {
                    return Err(rejected_as(ErrorCode::Conflict)("容器ID已存在"));
                }
                let data = serde_json::to_value(created(&project, &input)?).map_err(bad)?;
                NewEvent { eid: input.request_id.clone(), host: server.endpoint.host.clone(), actor: HUMAN.into(),
                    kind: frames::CREATED.into(), layer: frames::LAYER.into(), obj: input.id.clone(), data, guards: vec![] }
            }
        };
        let event = runtime.append(request, now).map_err(invalid)?;
        // A frame a person placed is published at once, like a human task.
        runtime.snapshot(now).map_err(uncertain)?;
        Ok(Json(json!({"request_id":event.eid,"event":event.eid,"seq":event.seq,"status":"accepted",
            "frame":runtime.projection()[frames::GROUP][&event.obj]})))
    }).await.map_err(uncertain)?
}

pub async fn place(State(server): State<Arc<Server>>, headers: HeaderMap, RoutePath((handle, id)): RoutePath<(String, String)>, Json(input): Json<Move>) -> Result<Json<Value>, ApiError> {
    if !crate::daemon::authorized_app(&headers, &server) {
        return Err(denied());
    }
    valid_scan_id(&input.request_id).map_err(bad)?;
    frames::check_id(&id).map_err(rejected_as(ErrorCode::Usage))?;
    frames::check_place(&input.place).map_err(rejected_as(ErrorCode::Usage))?;
    let project = project(&server, &handle).map_err(bad)?;
    tokio::task::spawn_blocking(move || {
        let mut slot = project.runtime.lock().map_err(|_| bad("项目会话不可用"))?;
        let runtime = ready(&mut slot, &server, &project).map_err(bad)?;
        let data = serde_json::to_value(Placed { place: input.place }).map_err(bad)?;
        let request = match human_tasks::original(runtime, &input.request_id).map_err(bad)? {
            Some(old) if old.host == server.endpoint.host && old.actor == HUMAN && old.kind == frames::PLACED
                && old.obj == id && old.data == data => old,
            Some(_) => return Err(rejected_as(ErrorCode::Conflict)("请求 ID 已用于其他内容")),
            None => NewEvent { eid: input.request_id, host: server.endpoint.host.clone(), actor: HUMAN.into(),
                kind: frames::PLACED.into(), layer: frames::LAYER.into(), obj: id, data, guards: vec![] },
        };
        // Moving a frame waits for the next due snapshot, like a signal.
        let event = runtime.append(request, now).map_err(invalid)?;
        Ok(Json(json!({"request_id":event.eid,"event":event.eid,"seq":event.seq,"status":"accepted",
            "frame":runtime.projection()[frames::GROUP][&event.obj]})))
    }).await.map_err(uncertain)?
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CardWrite {
    request_id: String,
    /// A card of this frame; the frame itself when adding.
    #[serde(default)]
    target: Option<String>,
    op: Op,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RulesWrite {
    request_id: String,
    rules: Rules,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PinsWrite {
    request_id: String,
    pins: BTreeMap<String, Option<frames::Pin>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Decision {
    request_id: String,
    pending: u64,
    accept: bool,
}

/// One human event on a frame, retried by request ID. Structure changes
/// (rules) are published at once; card content waits for the next snapshot.
async fn human(server: Arc<Server>, headers: HeaderMap, handle: String, frame: String, request_id: String,
    kind: &'static str, layer: &'static str, obj: String, data: Value, publish: bool) -> Result<Json<Value>, ApiError> {
    if !crate::daemon::authorized_app(&headers, &server) {
        return Err(denied());
    }
    valid_scan_id(&request_id).map_err(bad)?;
    frames::check_id(&frame).map_err(rejected_as(ErrorCode::Usage))?;
    let project = project(&server, &handle).map_err(bad)?;
    tokio::task::spawn_blocking(move || {
        let mut slot = project.runtime.lock().map_err(|_| bad("项目会话不可用"))?;
        let runtime = ready(&mut slot, &server, &project).map_err(bad)?;
        let request = match human_tasks::original(runtime, &request_id).map_err(bad)? {
            Some(old) if old.host == server.endpoint.host && old.actor == HUMAN && old.kind == kind && old.obj == obj && old.data == data => old,
            Some(_) => return Err(rejected_as(ErrorCode::Conflict)("请求 ID 已用于其他内容")),
            None => NewEvent { eid: request_id, host: server.endpoint.host.clone(), actor: HUMAN.into(),
                kind: kind.into(), layer: layer.into(), obj, data, guards: vec![] },
        };
        let event = runtime.append(request, now).map_err(invalid)?;
        if publish {
            runtime.snapshot(now).map_err(uncertain)?;
        }
        let receipt = cards::outcome(runtime.projection(), &frame, event.seq).map_err(bad)?;
        Ok(Json(json!({"request_id":event.eid,"event":event.eid,"seq":event.seq,"status":"accepted",
            "receipt":receipt,"frame":runtime.projection()[frames::GROUP][&frame]})))
    }).await.map_err(uncertain)?
}

/// A person adds, edits, moves, links or archives a card (FR-147 in the App).
pub async fn card(State(server): State<Arc<Server>>, headers: HeaderMap, RoutePath((handle, id)): RoutePath<(String, String)>, Json(input): Json<CardWrite>) -> Result<Json<Value>, ApiError> {
    if matches!(input.op, Op::Rules { .. }) {
        return Err(rejected_as(ErrorCode::Usage)("人在 App 里直接改规则"));
    }
    let target = input.target.unwrap_or_else(|| id.clone());
    if cards::frame_of(&input.op, &target).map_err(rejected_as(ErrorCode::Usage))? != id {
        return Err(rejected_as(ErrorCode::Usage)(format!("{target} 不在容器 {id} 里")));
    }
    let kind = input.op.kind();
    let data = serde_json::to_value(&input.op).map_err(bad)?;
    human(server, headers, handle, id, input.request_id, kind, cards::LAYER, target, data, false).await
}

/// FR-146: a person changes the frame's rules.
pub async fn rules(State(server): State<Arc<Server>>, headers: HeaderMap, RoutePath((handle, id)): RoutePath<(String, String)>, Json(input): Json<RulesWrite>) -> Result<Json<Value>, ApiError> {
    let data = serde_json::to_value(RulesChanged { rules: input.rules }).map_err(bad)?;
    human(server, headers, handle, id.clone(), input.request_id, cards::RULES_CHANGED, frames::LAYER, id, data, true).await
}

/// A person accepts or ignores one proposal, suggestion, confirmation or
/// rule proposal. Nothing waiting ever takes effect without this.
pub async fn resolve(State(server): State<Arc<Server>>, headers: HeaderMap, RoutePath((handle, id)): RoutePath<(String, String)>, Json(input): Json<Decision>) -> Result<Json<Value>, ApiError> {
    let data = serde_json::to_value(Resolved { pending: input.pending, accept: input.accept }).map_err(bad)?;
    human(server, headers, handle, id.clone(), input.request_id, cards::RESOLVED, frames::LAYER, id, data, input.accept).await
}

/// FR-149: a person drags cards to fixed places, or lets them go back to
/// the derived layout. Like moving a frame, it waits for the next snapshot.
pub async fn pins(State(server): State<Arc<Server>>, headers: HeaderMap, RoutePath((handle, id)): RoutePath<(String, String)>, Json(input): Json<PinsWrite>) -> Result<Json<Value>, ApiError> {
    let data = serde_json::to_value(Pinned { pins: input.pins }).map_err(bad)?;
    human(server, headers, handle, id.clone(), input.request_id, cards::PINNED, frames::LAYER, id, data, false).await
}

/// FR-157: every step of a frame, replayed read-only from the canonical
/// events. Anyone the daemon knows may read it; nothing is written.
pub async fn history(State(server): State<Arc<Server>>, headers: HeaderMap, RoutePath((handle, id)): RoutePath<(String, String)>) -> Result<Json<Value>, ApiError> {
    replayed(server, headers, handle, id, None).await
}

/// FR-157: the frame as it read right after one step.
pub async fn history_at(State(server): State<Arc<Server>>, headers: HeaderMap, RoutePath((handle, id, seq)): RoutePath<(String, String, u64)>) -> Result<Json<Value>, ApiError> {
    replayed(server, headers, handle, id, Some(seq)).await
}

async fn replayed(server: Arc<Server>, headers: HeaderMap, handle: String, id: String, seq: Option<u64>) -> Result<Json<Value>, ApiError> {
    if !authorized_client(&headers, &server) {
        return Err(denied());
    }
    frames::check_id(&id).map_err(rejected_as(ErrorCode::Usage))?;
    let project = project(&server, &handle).map_err(bad)?;
    tokio::task::spawn_blocking(move || {
        let mut slot = project.runtime.lock().map_err(|_| bad("项目会话不可用"))?;
        let runtime = ready(&mut slot, &server, &project).map_err(bad)?;
        let initial = towermd_devkit::dev_state::initial();
        let reduce: towermd_core::project_runtime::Reducer = towermd_devkit::dev_state::apply;
        let steps = frame_history::steps(runtime.canonical_receipts(), &id, &initial, reduce).map_err(uncertain)?;
        let Some(seq) = seq else {
            return Ok(Json(json!({"frame":id,"seq":runtime.contiguous_seq(),"steps":steps})));
        };
        let step = steps.iter().rev().find(|step| step.seq <= seq)
            .ok_or_else(|| rejected_as(ErrorCode::NotFound)(format!("容器 {id} 在 {seq} 时还不存在")))?;
        let state = frame_history::state_at(runtime.canonical_receipts(), seq, &initial, reduce).map_err(uncertain)?;
        let view = cards::view(&state, towermd_devkit::dev_state::KITS, step.at.get(..10).unwrap_or_default()).map_err(uncertain)?
            .into_iter().find(|view| view.frame.id == id);
        Ok(Json(json!({"frame":id,"seq":seq,"step":step,"view":view})))
    }).await.map_err(uncertain)?
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExportQuery {
    #[serde(default)]
    format: Option<String>,
}

/// FR-155: the frame as Markdown or Mermaid text, derived on every read and
/// never stored. Anyone the daemon knows may read it.
pub async fn export(State(server): State<Arc<Server>>, headers: HeaderMap, RoutePath((handle, id)): RoutePath<(String, String)>,
    Query(query): Query<ExportQuery>) -> Result<Json<Value>, ApiError> {
    if !authorized_client(&headers, &server) {
        return Err(denied());
    }
    frames::check_id(&id).map_err(rejected_as(ErrorCode::Usage))?;
    let project = project(&server, &handle).map_err(bad)?;
    tokio::task::spawn_blocking(move || {
        let mut slot = project.runtime.lock().map_err(|_| bad("项目会话不可用"))?;
        let runtime = ready(&mut slot, &server, &project).map_err(bad)?;
        let today = towermd_core::tower_api::timestamp(now()).unwrap_or_default();
        let view = cards::view(runtime.projection(), towermd_devkit::dev_state::KITS, today.get(..10).unwrap_or_default())
            .map_err(uncertain)?.into_iter().find(|view| view.frame.id == id)
            .ok_or_else(|| rejected_as(ErrorCode::NotFound)(format!("没有容器 {id}")))?;
        let declared = frames::Template::parse(&view.frame.template.definition).map(|template| template.export).unwrap_or_default();
        let formats = frame_text::formats(&view.frame.layout, &declared);
        let format = query.format.unwrap_or_else(|| formats[0].to_owned());
        let text = frame_text::export(&view, &format).map_err(rejected_as(ErrorCode::Usage))?;
        Ok(Json(json!({"frame":id,"format":format,"formats":formats,"text":text})))
    }).await.map_err(uncertain)?
}

/// FR-159: a view derived from time that the frame's template declares,
/// computed from the canonical events on every read and never stored.
/// Anyone the daemon knows may read it.
pub async fn view(State(server): State<Arc<Server>>, headers: HeaderMap,
    RoutePath((handle, id, name)): RoutePath<(String, String, String)>) -> Result<Json<Value>, ApiError> {
    if !authorized_client(&headers, &server) {
        return Err(denied());
    }
    frames::check_id(&id).map_err(rejected_as(ErrorCode::Usage))?;
    let project = project(&server, &handle).map_err(bad)?;
    tokio::task::spawn_blocking(move || {
        let mut slot = project.runtime.lock().map_err(|_| bad("项目会话不可用"))?;
        let runtime = ready(&mut slot, &server, &project).map_err(bad)?;
        let frame = frames::read(runtime.projection()).map_err(uncertain)?.remove(&id)
            .ok_or_else(|| rejected_as(ErrorCode::NotFound)(format!("没有容器 {id}")))?;
        if !frame_views::offered(&frame).contains(&name.as_str()) {
            return Err(rejected_as(ErrorCode::Usage)(format!("容器 {id} 的模板没有提供视图 {name}")));
        }
        let today = towermd_core::tower_api::timestamp(now()).unwrap_or_default();
        let initial = towermd_devkit::dev_state::initial();
        let reduce: towermd_core::project_runtime::Reducer = towermd_devkit::dev_state::apply;
        let flow = frame_views::cumulative_flow(runtime.canonical_receipts(), &id, &initial, reduce,
            towermd_devkit::dev_state::KITS, today.get(..10).unwrap_or_default()).map_err(uncertain)?
            .ok_or_else(|| rejected_as(ErrorCode::NotFound)(format!("没有容器 {id}")))?;
        Ok(Json(json!({"view":name,"flow":flow})))
    }).await.map_err(uncertain)?
}
