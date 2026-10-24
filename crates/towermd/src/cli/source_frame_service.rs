//! Text-source frames in the daemon (core spec FR-163–169, FR-175–178).
//!
//! The file on main is the frame's truth. Every read parses it again; the
//! daemon stores nothing of it. Edits wait in the frame as writes, and this
//! service is the only place that turns them into commits: it reads the
//! file at the authoritative main, patches the smallest lines, has the
//! single main writer commit the result, and records `frame.source_written`
//! under the main fence. A write the file no longer allows is recorded as
//! failed and stays, dashed, for a person to redo or drop (FR-167).
use super::business::{config, envelope, now, ready, runtime_failure, uncertain};
use super::main_resolutions::authoritative_main;
use super::*;
use std::time::{SystemTime, UNIX_EPOCH};
use towermd_core::{
    cards::{self, Op, Why},
    events,
    frames::{self, Frame},
    main_writer::{WriteKind, WriteOutcome, WriteRequest},
    path_guard, plantuml,
    project_runtime::ProjectRuntime,
    source_frames::{self as sf, Change, Read as Reading, Written},
};

const HUMAN: &str = "human:local";
const READ_TIMEOUT: Duration = Duration::from_secs(30);
/// Commits of the file looked at for the last good reading and history.
const HISTORY: usize = 30;
/// A file larger than this is not read whole; the parser refuses past 1 MB.
const READ_CAP: u64 = 4 * plantuml::MAX_BYTES as u64;

/// The file at `revision`, or `None` when it is not a regular file there.
fn file_at(root: &Path, revision: &str, path: &str) -> Result<Option<Vec<u8>>, String> {
    let listed = repo::git(root, &["--literal-pathspecs", "ls-tree", "-z", "--full-tree", "--long", revision, "--", path])?;
    let Some(entry) = listed.split('\0').find(|entry| entry.split_once('\t').is_some_and(|(_, name)| name == path)) else {
        return Ok(None);
    };
    let fields: Vec<&str> = entry.split_once('\t').map(|(head, _)| head.split_whitespace().collect()).unwrap_or_default();
    let (mode, kind, oid, size) = match fields.as_slice() {
        [mode, kind, oid, size] => (*mode, *kind, *oid, size.parse::<u64>().unwrap_or(u64::MAX)),
        _ => return Err("无法读取源文件的树条目".into()),
    };
    if kind != "blob" || !matches!(mode, "100644" | "100755") {
        return Ok(None);
    }
    if size > READ_CAP {
        // Enough for the parser to say it is too large.
        return Ok(Some(vec![b' '; plantuml::MAX_BYTES + 1]));
    }
    repo::git_bytes_timeout(root, &["cat-file", "blob", oid], READ_TIMEOUT).map(Some)
}

/// Repository includes resolve at the same revision; nothing else is read.
fn includer<'a>(root: &'a Path, revision: &'a str) -> impl FnMut(&str) -> Option<Vec<u8>> + 'a {
    move |path: &str| file_at(root, revision, path).ok().flatten()
}

fn reading(root: &Path, revision: &str, path: &str) -> Result<Option<Reading>, String> {
    let Some(bytes) = file_at(root, revision, path)? else { return Ok(None) };
    let parsed = plantuml::parse(path, &bytes, &mut includer(root, revision));
    Ok(Some(Reading { revision: revision.to_owned(), bytes, parsed }))
}

/// The main this daemon already knows, without fetching: reads never touch
/// the network.
fn local_main(root: &Path) -> Result<String, String> {
    repo::git(root, &["rev-parse", "--verify", "-q", "refs/remotes/origin/main"])
        .or_else(|_| repo::git(root, &["rev-parse", "--verify", "refs/heads/main"]))
}

fn source_frame(runtime: &ProjectRuntime, id: &str) -> Result<(Frame, sf::Spec, String), String> {
    let frame = frames::read(runtime.projection())?.remove(id).ok_or_else(|| format!("没有容器 {id}"))?;
    let path = frame.source.as_ref().map(|source| source.path.clone()).ok_or_else(|| format!("容器 {id} 不是文本源容器"))?;
    let spec = sf::spec(&frame)?;
    Ok((frame, spec, path))
}

/// The writer never touches what only the slow layer may change.
fn writable(path: &str, docs_dir: &str) -> Result<(), String> {
    if path_guard::protected_path(path, docs_dir) {
        return Err(format!("{path} 在受保护路径下，写入者不改；请换一个位置"));
    }
    Ok(())
}

/// A delegate learns at once that the file will not take its edit, instead
/// of leaving a dashed item for a person (FR-167 is for edits that went
/// stale while waiting).
pub(super) fn preflight(project: &Project, runtime: &ProjectRuntime, frame_id: &str, change: &Change) -> Result<(), String> {
    let (_, spec, path) = source_frame(runtime, frame_id)?;
    let main = local_main(&project.root)?;
    let current = file_at(&project.root, &main, &path)?;
    let written = sf::write(&spec, &path, current.as_deref(), change, &mut includer(&project.root, &main));
    written.map(|_| ())
}

fn marker(frame: &str, eid: &str) -> String {
    format!("Tower.md frame write: {frame}#{eid}")
}

/// Writes every queued edit of the frame into its file, one commit each.
/// Returns what happened to each; an infrastructure failure stops the run
/// and leaves the rest queued for the next flush.
pub(super) fn flush(server: &Server, project: &Project, runtime: &mut ProjectRuntime, frame_id: &str) -> Result<Vec<Value>, String> {
    let mut done = Vec::new();
    let mut moved = false;
    let result = loop {
        let Ok((frame, spec, path)) = source_frame(runtime, frame_id) else { break Ok(()) };
        let Some(item) = frame.pending.iter().find(|item| item.why == Why::Write && item.note.is_none()).cloned() else { break Ok(()) };
        let Op::Source { change } = &item.op else { break Err("排队的写入不是文本源修改".to_owned()) };
        let step = (|| -> Result<Written, String> {
            let config = config(server, project)?;
            let main = authoritative_main(project)?;
            let mark = marker(frame_id, &item.by.eid);
            let earlier = repo::git(&project.root, &["log", "-F", &format!("--grep={mark}"), "--format=%H", "-n", "1", &main])?;
            let failed = |reason: String| Written { pending: item.id, path: path.clone(), commit: None, sha256: None, failed: Some(reason) };
            if !earlier.trim().is_empty() {
                // Written before a crash: record it, do not write it twice.
                let commit = earlier.trim().to_owned();
                let bytes = file_at(&project.root, &commit, &path)?.unwrap_or_default();
                return Ok(Written { pending: item.id, path: path.clone(), commit: Some(commit), sha256: Some(sf::sha256(&bytes)), failed: None });
            }
            if let Err(reason) = writable(&path, &config.docs_dir) {
                return Ok(failed(reason));
            }
            let current = file_at(&project.root, &main, &path)?;
            let bytes = match sf::write(&spec, &path, current.as_deref(), change, &mut includer(&project.root, &main)) {
                Err(reason) => return Ok(failed(reason)),
                Ok(None) => current.clone().unwrap_or_default(),
                Ok(Some(bytes)) => bytes,
            };
            let sha256 = sf::sha256(&bytes);
            if current.as_ref() == Some(&bytes) {
                return Ok(Written { pending: item.id, path: path.clone(), commit: None, sha256: Some(sha256), failed: None });
            }
            let request = WriteRequest {
                docs_dir: config.docs_dir.clone(),
                epoch: runtime.main_fence().map_err(|e| e.to_string())?.epoch,
                kind: WriteKind::Slow { expected_base: main.clone(), tag: None },
                changes: BTreeMap::from([(path.clone(), Some(bytes))]),
                message: format!("Update {path} for frame {frame_id}\n\n{mark}"),
            };
            match runtime.publish(&request, now).map_err(|e| e.to_string())? {
                WriteOutcome::Written { commit, .. } => Ok(Written { pending: item.id, path: path.clone(), commit: Some(commit), sha256: Some(sha256), failed: None }),
                _ => Err("main 已变化，这项写入留在队列中，稍后重试".into()),
            }
        })();
        let written = match step {
            Ok(written) => written,
            Err(error) => break Err(error),
        };
        moved |= written.commit.is_some();
        let eid = match &written.commit {
            Some(commit) => events::derived_eid(commit, sf::WRITTEN)?,
            None => Uuid::now_v7().to_string(),
        };
        let data = serde_json::to_value(&written).map_err(|e| e.to_string())?;
        let pending = runtime.pending().map_err(|e| e.to_string())?.into_iter().find(|event| event.eid == eid);
        let request = match pending {
            Some(pending) if pending.kind == sf::WRITTEN && pending.data == data => pending,
            Some(_) => break Err("原写回回执正文冲突".into()),
            None => {
                let mut event = envelope(server, eid, sf::WRITER, sf::WRITTEN, frame_id, data.clone());
                event.layer = frames::LAYER.into();
                event.guards = vec![runtime.main_fence().map_err(|e| e.to_string())?];
                event
            }
        };
        runtime.append(request, now).map_err(|e| e.to_string())?;
        done.push(data);
    };
    if moved {
        // Main moved: let the finish observer see the new authority now.
        if let Ok(mut cursor) = project.finish_observed_main.lock() {
            *cursor = None;
        }
        super::finish::observe_authority(server, project, runtime)?;
    }
    result.map(|_| done)
}

/// Flushes after a write was recorded; the request itself has already
/// succeeded, so a flush failure is reported, not raised.
pub(super) fn after(server: &Server, project: &Project, runtime: &mut ProjectRuntime, frame_id: &str) -> Value {
    if frames::read(runtime.projection()).ok().and_then(|all| all.get(frame_id).map(|frame| frame.source.is_some())) != Some(true) {
        return Value::Null;
    }
    match flush(server, project, runtime, frame_id) {
        Ok(written) => json!({"written": written}),
        Err(error) => json!({"written": [], "error": error}),
    }
}

fn hours_since(seconds: i64) -> i64 {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(seconds);
    (now - seconds).max(0) / 3600
}

/// The commits that changed the file, newest first, with their times.
fn file_commits(root: &Path, revision: &str, path: &str) -> Result<Vec<(String, i64, String, String)>, String> {
    let limit = HISTORY.to_string();
    let log = repo::git(root, &["log", "--format=%H%x09%ct%x09%an%x09%s", "-n", &limit, revision, "--", path])?;
    Ok(log.lines().filter_map(|line| {
        let mut parts = line.splitn(4, '\t');
        Some((parts.next()?.to_owned(), parts.next()?.parse().ok()?, parts.next()?.to_owned(), parts.next().unwrap_or_default().to_owned()))
    }).collect())
}

/// Where git's rename detection says the file went, when it is gone.
fn renamed(root: &Path, revision: &str, path: &str) -> Option<String> {
    let last = repo::git(root, &["log", "-n", "1", "--format=%H", revision, "--", path]).ok()?;
    let changes = repo::git(root, &["show", "-M", "--name-status", "--format=", last.trim()]).ok()?;
    changes.lines().find_map(|line| {
        let parts: Vec<&str> = line.split('\t').collect();
        (parts.len() == 3 && parts[0].starts_with('R') && parts[1] == path).then(|| parts[2].to_owned())
    })
}

/// The frame's file as `revision` has it, with the last good reading while
/// it does not parse (FR-175) and the passive or active signal (FR-177).
fn view_at(root: &Path, state: &Value, frame: &Frame, spec: &sf::Spec, path: &str, revision: &str) -> Result<sf::View, String> {
    let current = reading(root, revision, path)?;
    let parses = current.as_ref().is_some_and(|read| read.parsed.status == plantuml::Status::Ok);
    let (mut last_good, mut failing_since) = (None, None);
    if !parses {
        for (commit, at, _, _) in file_commits(root, revision, path)? {
            match reading(root, &commit, path)? {
                Some(read) if read.parsed.status == plantuml::Status::Ok => {
                    last_good = Some(read);
                    break;
                }
                _ => failing_since = Some(at),
            }
        }
    }
    let bound_open = match &frame.rules.update_until {
        Some(frames::Until::Object(id)) => cards::objects(state, towermd_devkit::dev_state::KITS)?.iter().any(|object| object.id == *id && !object.done),
        _ => false,
    };
    let renamed_to = if current.is_none() { renamed(root, revision, path) } else { None };
    Ok(sf::view(frame, spec, current, last_good, renamed_to, failing_since.map(hours_since), bound_open))
}

/// FR-163: each text-source frame of the state view gets its file's
/// reading at the local main. A frame whose file cannot be read says so.
pub(super) fn attach(project: &Project, runtime: &ProjectRuntime, views: &mut Value) {
    let Some(views) = views.as_array_mut() else { return };
    // Most projects have no text-source frame: no git call for them.
    if !views.iter().any(|view| view["source"].is_object()) {
        return;
    }
    let Ok(main) = local_main(&project.root) else { return };
    for view in views {
        if !view["source"].is_object() {
            continue;
        }
        let Some(id) = view["id"].as_str().map(str::to_owned) else { continue };
        let Ok((frame, spec, path)) = source_frame(runtime, &id) else { continue };
        view["source"] = match view_at(&project.root, runtime.projection(), &frame, &spec, &path, &main) {
            Ok(source) => serde_json::to_value(source).unwrap_or(Value::Null),
            Err(error) => json!({"path": path, "error": error}),
        };
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct At {
    #[serde(default)]
    at: Option<String>,
}

fn revision(at: Option<String>) -> Result<Option<String>, ApiError> {
    match at {
        Some(at) if (at.len() == 40 || at.len() == 64) && at.bytes().all(|b| b.is_ascii_hexdigit()) => Ok(Some(at)),
        Some(_) => Err(rejected_as(ErrorCode::Usage)("at 须是完整的提交号")),
        None => Ok(None),
    }
}

/// FR-163, FR-168: the frame's file read at main, or at one commit for the
/// time slider. Anyone the daemon knows may read it; nothing is written.
pub async fn show(State(server): State<Arc<Server>>, headers: HeaderMap, RoutePath((handle, id)): RoutePath<(String, String)>, Query(at): Query<At>) -> Result<Json<Value>, ApiError> {
    if !authorized_client(&headers, &server) {
        return Err(denied());
    }
    frames::check_id(&id).map_err(rejected_as(ErrorCode::Usage))?;
    let at = revision(at.at)?;
    let project = project(&server, &handle).map_err(bad)?;
    tokio::task::spawn_blocking(move || {
        let mut slot = project.runtime.lock().map_err(|_| bad("项目会话不可用"))?;
        let runtime = ready(&mut slot, &server, &project).map_err(bad)?;
        let (frame, spec, path) = source_frame(runtime, &id).map_err(rejected_as(ErrorCode::NotFound))?;
        let revision = match at {
            Some(at) => at,
            None => local_main(&project.root).map_err(uncertain)?,
        };
        let view = view_at(&project.root, runtime.projection(), &frame, &spec, &path, &revision).map_err(bad)?;
        Ok(Json(json!({"frame": id, "source": view, "write": frame.source_write})))
    }).await.map_err(uncertain)?
}

/// FR-169: the file's commits, each summarized from the parse results
/// alone ("2 elements added; Parser from To Do to Done").
pub async fn history(State(server): State<Arc<Server>>, headers: HeaderMap, RoutePath((handle, id)): RoutePath<(String, String)>) -> Result<Json<Value>, ApiError> {
    if !authorized_client(&headers, &server) {
        return Err(denied());
    }
    frames::check_id(&id).map_err(rejected_as(ErrorCode::Usage))?;
    let project = project(&server, &handle).map_err(bad)?;
    tokio::task::spawn_blocking(move || {
        let mut slot = project.runtime.lock().map_err(|_| bad("项目会话不可用"))?;
        let runtime = ready(&mut slot, &server, &project).map_err(bad)?;
        let (frame, spec, path) = source_frame(runtime, &id).map_err(rejected_as(ErrorCode::NotFound))?;
        let root = &project.root;
        let main = local_main(root).map_err(uncertain)?;
        let commits = file_commits(root, &main, &path).map_err(uncertain)?;
        let shown = |commit: &str| -> Result<Option<sf::Shown>, String> {
            Ok(reading(root, commit, &path)?.filter(|read| read.parsed.status == plantuml::Status::Ok)
                .map(|read| sf::shown(&frame, &spec, &read.parsed)))
        };
        let mut steps = Vec::new();
        let mut after = commits.first().map(|(commit, ..)| shown(commit)).transpose().map_err(uncertain)?.flatten();
        for (index, (commit, at, author, subject)) in commits.iter().enumerate() {
            let before = commits.get(index + 1).map(|(older, ..)| shown(older)).transpose().map_err(uncertain)?.flatten();
            let summary = match &after {
                Some(after) => serde_json::to_value(sf::summary(before.as_ref(), after)).map_err(uncertain)?,
                None => json!({"text": "这一版无法解析"}),
            };
            steps.push(json!({"commit": commit, "at": at, "author": author, "subject": subject, "summary": summary}));
            after = before;
        }
        Ok(Json(json!({"frame": id, "path": path, "main": main, "steps": steps})))
    }).await.map_err(uncertain)?
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Relink {
    request_id: String,
    path: String,
}

/// FR-178: a person points the frame at the file's new place, in the App.
pub async fn relink(State(server): State<Arc<Server>>, headers: HeaderMap, RoutePath((handle, id)): RoutePath<(String, String)>, Json(input): Json<Relink>) -> Result<Json<Value>, ApiError> {
    if !crate::daemon::authorized_app(&headers, &server) {
        return Err(denied());
    }
    valid_scan_id(&input.request_id).map_err(bad)?;
    frames::check_id(&id).map_err(rejected_as(ErrorCode::Usage))?;
    let project = project(&server, &handle).map_err(bad)?;
    tokio::task::spawn_blocking(move || {
        let mut slot = project.runtime.lock().map_err(|_| bad("项目会话不可用"))?;
        let runtime = ready(&mut slot, &server, &project).map_err(bad)?;
        let config = config(&server, &project).map_err(bad)?;
        writable(&input.path, &config.docs_dir).map_err(rejected_as(ErrorCode::Usage))?;
        let data = serde_json::to_value(sf::Relinked { path: input.path }).map_err(bad)?;
        let request = match human_tasks::original(runtime, &input.request_id).map_err(bad)? {
            Some(old) if old.host == server.endpoint.host && old.actor == HUMAN && old.kind == sf::RELINKED && old.obj == id && old.data == data => old,
            Some(_) => return Err(rejected_as(ErrorCode::Conflict)("请求 ID 已用于其他内容")),
            None => {
                let mut event = envelope(&server, input.request_id, HUMAN, sf::RELINKED, &id, data);
                event.layer = frames::LAYER.into();
                event
            }
        };
        let event = runtime.append(request, now).map_err(|error| bad(error.to_string()))?;
        runtime.snapshot(now).map_err(uncertain)?;
        Ok(Json(json!({"request_id": event.eid, "event": event.eid, "seq": event.seq, "status": "accepted",
            "frame": runtime.projection()[frames::GROUP][&id]})))
    }).await.map_err(uncertain)?
}

/// The `source` part of a frame-creating request.
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Create {
    pub path: String,
    /// Create the file with an empty diagram when main has none.
    #[serde(default)]
    pub create: bool,
}

/// Before a source frame is created: the path is writable, and the file
/// either exists or is to be created.
pub(super) fn check_create(server: &Server, project: &Project, input: &Create) -> Result<bool, ApiError> {
    let config = config(server, project).map_err(bad)?;
    writable(&input.path, &config.docs_dir).map_err(rejected_as(ErrorCode::Usage))?;
    let main = local_main(&project.root).map_err(uncertain)?;
    let exists = file_at(&project.root, &main, &input.path).map_err(uncertain)?.is_some();
    if !exists && !input.create {
        return Err(rejected_as(ErrorCode::NotFound)(format!("main 上没有 {}；要新建请设 create", input.path)));
    }
    Ok(!exists)
}

/// A person's `init` for a new frame's missing file, then the write.
pub(super) fn init(server: &Server, project: &Project, runtime: &mut ProjectRuntime, frame_id: &str, request_id: &str) -> Result<Value, ApiError> {
    let op = Op::Source { change: Change::Init {} };
    let data = serde_json::to_value(&op).map_err(bad)?;
    let eid = events::derived_eid(&format!("{:x}", Sha256::digest(request_id.as_bytes())), "frame.source_init").map_err(bad)?;
    if runtime.receipt(&eid).is_none() {
        let mut event = envelope(server, eid, HUMAN, cards::SOURCE, frame_id, data);
        event.layer = cards::LAYER.into();
        runtime.append(event, now).map_err(runtime_failure)?;
    }
    Ok(after(server, project, runtime, frame_id))
}
