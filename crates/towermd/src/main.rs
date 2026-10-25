mod app_launch;
mod args;
mod daemon;
mod output;
mod project_service;
mod read;
mod remote_hub;
mod templates;
mod views;
mod worker;

use std::ffi::OsString;
use std::io::{self, BufRead, IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{CommandFactory, FromArgMatches};
use serde_json::{json, Value};
use towermd_core::catalog::Catalog;
use towermd_core::config::{Config, DEFAULT_DOCS_DIR};
use towermd_core::init::{init, InitOptions};
use towermd_core::repo;

use args::{Cli, Command, DaemonArgs, DecisionArgs, DevArgs, DraftArgs, ExtArgs, FrameArgs, HubArgs, IdeaArgs, RuleArgs, ScanArgs, TaskArgs, TemplateArgs, TowerArgs};
use output::render::{self, Format};
use output::{lang, Done, Envelope, ErrorCode, Failure, Next};

const MAX_INPUT: u64 = 2 * 1024 * 1024;

fn main() -> ExitCode {
    let argv: Vec<OsString> = std::env::args_os().collect();
    lang::init(&argv);
    let tty = io::stdout().is_terminal();
    let matches = output::help::localized(Cli::command(), lang::current()).try_get_matches_from(&argv);
    let cli = match matches.and_then(|matches| Cli::from_arg_matches(&matches)) {
        Ok(cli) => cli,
        Err(error) => return clap_exit(error, render::format_from_args(&argv, tty)),
    };
    // Internal process protocols keep their own plain stdout.
    match &cli.command {
        Some(Command::VerificationWorker) => return plain_exit(worker::serve()),
        Some(Command::Daemon { command: DaemonArgs::Serve }) => return plain_exit(daemon::serve()),
        // A shell script for `source`, so never an envelope.
        Some(Command::Completion { shell }) => {
            let mut command = output::help::localized(Cli::command(), lang::current());
            clap_complete::generate(*shell, &mut command, "towermd", &mut io::stdout());
            return ExitCode::SUCCESS;
        }
        _ => {}
    }
    let format = render::format(cli.json, cli.text, tty);
    let name = cli.command.as_ref().map_or("towermd", Command::name);
    let mut delivery = Delivery::default();
    let request_id = match request_id(cli.request_id, cli.command.as_ref().is_some_and(Command::is_write)) {
        Ok(id) => id,
        Err(failure) => return finish(Err(failure), delivery, None, format, cli.quiet, name),
    };
    let context = Context { run: cli.run.as_deref(), request_id: request_id.as_deref(), format, tty };
    let result = execute(cli.command, cli.project.as_deref(), context, &mut delivery);
    finish(result, delivery, request_id.as_deref(), format, cli.quiet, name)
}

fn plain_exit(result: Result<(), String>) -> ExitCode {
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            let _ = writeln!(io::stderr().lock(), "towermd: {error}");
            ExitCode::FAILURE
        }
    }
}

fn clap_exit(error: clap::Error, format: Format) -> ExitCode {
    if !error.use_stderr() {
        // --help and --version are plain text by design.
        return match error.print() {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                let _ = writeln!(io::stderr().lock(), "towermd: {error}");
                ExitCode::FAILURE
            }
        };
    }
    let message = if error.kind() == clap::error::ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand {
        lang::pick("missing command", "缺少命令").to_owned()
    } else {
        let rendered = error.render().to_string();
        rendered.lines().next().unwrap_or_default().trim_start_matches("error: ").to_owned()
    };
    let failure = Failure::new(ErrorCode::Usage, message).hint(lang::pick("run `towermd --help` for usage", "运行 `towermd --help` 查看用法"));
    finish(Err(failure), Delivery::default(), None, format, false, "")
}

fn valid_request_id(id: &str) -> bool {
    uuid::Uuid::parse_str(id).is_ok_and(|uuid| uuid.get_version_num() == 7 && uuid.hyphenated().to_string() == id)
}

fn request_id(given: Option<String>, write: bool) -> Result<Option<String>, Failure> {
    match (given, write) {
        (None, false) => Ok(None),
        (None, true) => Ok(Some(uuid::Uuid::now_v7().to_string())),
        (Some(_), false) => Err(Failure::new(
            ErrorCode::Usage,
            lang::pick("--request-id only applies to commands that write", "--request-id 只用于写命令"),
        )),
        (Some(id), true) if valid_request_id(&id) => Ok(Some(id)),
        (Some(id), true) => Err(Failure::new(
            ErrorCode::Usage,
            lang::pick(format!("--request-id must be a lowercase UUID v7: {id}"), format!("--request-id 必须是小写 UUID v7：{id}")),
        )),
    }
}

/// Answers and interjections read for this call. They are acknowledged only
/// after the envelope carrying them reached stdout.
#[derive(Default)]
struct Delivery {
    messages: Vec<Value>,
    warnings: Vec<String>,
}

impl Delivery {
    fn fetch(&mut self, run: Option<&str>) -> Result<(), Failure> {
        let value = daemon::fetch_messages(run)?;
        if let Some(review) = value.get("needs_review").filter(|review| !review.is_null()) {
            let task = review["task"].as_str().unwrap_or("?");
            let baseline = review["baseline"].as_str().unwrap_or("?");
            let warning = lang::pick(
                format!("task {task} needs review: the new version affects its grant {baseline}; review the task before continuing."),
                format!("任务 {task} 需复核：新版本影响了原授权 {baseline}，请先审阅任务再继续执行。"),
            );
            if !self.warnings.contains(&warning) {
                self.warnings.push(warning);
            }
        }
        for message in value["messages"].as_array().ok_or("消息响应格式无效")? {
            let seen = self.messages.iter().any(|known| known["message"] == message["message"] && known["run"] == message["run"]);
            if !seen {
                self.messages.push(message.clone());
            }
        }
        Ok(())
    }

    /// After ask/wait/answer the result itself may have produced messages.
    fn refresh(&mut self, run: Option<&str>) {
        if let Err(failure) = self.fetch(run) {
            self.warnings.push(failure.message);
        }
    }

    fn acknowledge(&self) -> Vec<String> {
        self.messages
            .iter()
            // Report requests repeat until `towermd reply`, temporary rules
            // until they expire; there is nothing to ack.
            .filter(|message| !matches!(message["message"]["kind"].as_str(), Some("request_report" | "rule")))
            .filter_map(|message| daemon::ack_message(message).err().map(|failure| (message, failure)))
            .map(|(message, failure)| {
                let id = message["message"].as_str().unwrap_or("?");
                lang::pick(
                    format!("message {id} was not acknowledged ({}); it will be delivered again", failure.message),
                    format!("消息 {id} 未确认送达（{}），下次会再次送出", failure.message),
                )
            })
            .collect()
    }
}

fn finish(result: Result<Done, Failure>, delivery: Delivery, request_id: Option<&str>, format: Format, quiet: bool, command: &str) -> ExitCode {
    let mut envelope = match result {
        Ok(done) => Envelope::success(done, request_id),
        Err(failure) => Envelope::failure(failure, request_id),
    };
    envelope.delivered = delivery.messages.clone();
    for warning in &delivery.warnings {
        if !envelope.warnings.contains(warning) {
            envelope.warnings.push(warning.clone());
        }
    }
    let written = render::emit(&envelope, format, quiet, command, &mut io::stdout().lock(), &mut io::stderr().lock());
    if let Err(error) = written {
        // Nothing reached the caller, so nothing is acknowledged.
        let _ = writeln!(io::stderr().lock(), "towermd: {}{error}", lang::pick("could not write the result: ", "无法写出结果："));
        return ExitCode::FAILURE;
    }
    for warning in delivery.acknowledge() {
        let _ = writeln!(io::stderr().lock(), "towermd: {}{warning}", lang::pick("warning: ", "警告："));
    }
    ExitCode::from(envelope.exit())
}

fn in_project(run: Option<&str>) -> Result<bool, Failure> {
    if run.is_some() {
        return Ok(true);
    }
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    repo::toplevel(&cwd).map_err(|error| {
        Failure::new(ErrorCode::NotFound, error).hint(lang::pick("run inside a git repository", "请在 git 仓库内运行"))
    })?;
    Ok(Catalog::load_optional(&cwd)?.is_some())
}

/// The project of the current repository, when it has been initialized.
fn current_project() -> Result<Option<(String, PathBuf)>, Failure> {
    let cwd = std::env::current_dir().map_err(|error| error.to_string())?;
    let Ok(root) = repo::toplevel(&cwd) else { return Ok(None) };
    Ok(Config::load(&root)?.map(|config| (config.project_id().to_owned(), root)))
}

/// `--project <path|ID>` (§3.1): a directory inside a repository, the
/// current repository's ID, or the ID of a project this user already ran a
/// command in (remembered in `views.json`). The command then runs there.
fn enter_project(project: &str) -> Result<(), Failure> {
    let unknown = || {
        Failure::new(ErrorCode::NotFound, lang::pick(format!("unknown project {project}"), format!("未知项目 {project}")))
            .hint(lang::pick(
                "pass the project's path; an ID is known after a towermd command ran inside that project",
                "请传项目路径；在该项目内运行过 towermd 命令后才能用 ID 指代它",
            ))
            .next(Next::new("towermd overview", lang::pick("list known projects", "列出已知项目")))
    };
    let path = Path::new(project);
    let root = if path.is_dir() {
        repo::toplevel(path).map_err(|error| Failure::new(ErrorCode::NotFound, error))?
    } else if current_project()?.is_some_and(|(id, _)| id == project) {
        return Ok(());
    } else {
        let root = views::root(project).filter(|root| root.is_dir()).ok_or_else(unknown)?;
        if !Config::load(&root)?.is_some_and(|config| config.project_id() == project) {
            return Err(unknown());
        }
        root
    };
    std::env::set_current_dir(&root).map_err(|error| Failure::new(ErrorCode::NotFound, format!("{}: {error}", root.display())))
}

#[derive(Clone, Copy)]
struct Context<'a> {
    run: Option<&'a str>,
    request_id: Option<&'a str>,
    format: Format,
    tty: bool,
}

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |elapsed| elapsed.as_millis() as u64)
}

fn execute(command: Option<Command>, project: Option<&str>, context: Context, delivery: &mut Delivery) -> Result<Done, Failure> {
    let run = context.run;
    if let Some(project) = project.filter(|_| !command.as_ref().is_some_and(Command::repo_free)) {
        enter_project(project)?;
    }
    let Some(command) = command else { return home(run, context.tty, delivery) };
    if command.repo_bound() && in_project(run)? {
        delivery.fetch(run)?;
        if command.renews_run() {
            daemon::task_status(run)?;
        }
        if let Some((id, root)) = current_project()? {
            views::remember(&id, &root);
        }
    }
    let request_id = context.request_id;
    let id = || request_id.expect("write commands always carry a request ID");
    let value = match command {
        Command::VerificationWorker | Command::Daemon { command: DaemonArgs::Serve } => unreachable!("handled in main"),
        Command::Up => app_launch::up()?,
        Command::Init { path, docs_dir } => run_init(path, docs_dir)?,
        Command::Show { id: None } => daemon::project_view()?,
        Command::Show { id: Some(id) } if current_project()?.is_some_and(|(project, _)| project == id) => daemon::project_view()?,
        Command::Show { id: Some(id) } => read::object(&daemon::project_state()?, &id).ok_or_else(|| not_found(&id))?,
        Command::Needs { all } => {
            let value = read::needs(&daemon::project_state()?, all);
            looked(context.tty, now_ms());
            value
        }
        Command::Since { from } => {
            let now = now_ms();
            let since = match from {
                Some(from) => Some(when(&from, "--from", now)?),
                None => current_project()?.and_then(|(project, _)| views::last_view(&project)),
            };
            let value = read::since(&daemon::project_state()?, since, now);
            looked(context.tty, now);
            value
        }
        Command::Ls { kind, state, since } => {
            let since = since.map(|since| when(&since, "--since", now_ms())).transpose()?;
            let view = if kind == "decisions" { Some(daemon::project_view()?) } else { None };
            let value = read::list(&daemon::project_state()?, &kind, state.as_deref(), since, view.as_ref());
            return Ok(listed(value));
        }
        Command::History { id, since } => {
            let since = since.map(|since| when(&since, "--since", now_ms())).transpose()?;
            history(&id, since)?
        }
        Command::Watch { scope, exit_after } => watch(scope.as_deref(), exit_after, context.format)?,
        Command::Open { id } => {
            let mut value = app_launch::up()?;
            value["focus"] = json!(id);
            if let Some(id) = &id {
                let located = read::object(&daemon::project_state()?, id).is_some();
                value["located"] = json!(located);
                let warning = lang::pick(
                    format!("the App opened on the project; it cannot jump to {id} yet"),
                    format!("App 已打开到项目；暂不能直接定位到 {id}"),
                );
                return Ok(Done { data: value, next: vec![], warnings: vec![warning] });
            }
            value
        }
        Command::Decision { command } => {
            let decisions = read::decisions(&daemon::project_state()?, Some(&daemon::project_view()?));
            match command {
                DecisionArgs::Ls => json!({"items": decisions}),
                DecisionArgs::Show { id } => decisions.into_iter().find(|decision| decision["id"] == id.as_str()).ok_or_else(|| not_found(&id))?,
            }
        }
        Command::Rule { command: RuleArgs::Ls } => json!({"items": read::rules(&daemon::project_state()?, now_ms())}),
        Command::RebuildIndex => rebuild_index()?,
        Command::Doctor => doctor()?,
        Command::Whoami => whoami(run),
        Command::Overview => return overview(),
        Command::Completion { .. } => unreachable!("handled in main"),
        Command::Schema { name } => schema(name.as_deref())?,
        Command::Dev { command } => match command {
            DevArgs::Check => daemon::task_check(run)?,
            DevArgs::Finish { wait: None, claims, report } => {
                let claims = claims.map(|path| read_json_input(&path)).transpose()?.unwrap_or_else(|| json!([]));
                let report = report.map(|path| read_json_input(&path)).transpose()?;
                daemon::finish_start(id(), run, claims, report)?
            }
            DevArgs::Clarify { task, input, source } => {
                let mut clarification = read_json_input(&input)?;
                if let (Some(fields), Some(source)) = (clarification.as_object_mut(), reported(source)) {
                    fields.insert("self_reported".into(), json!(source));
                }
                daemon::clarify(&task, id(), clarification)?
            }
            DevArgs::Finish { wait: Some(evidence), .. } => {
                let again = format!("towermd dev finish --wait {evidence}");
                return classify_finish(daemon::finish_wait(&evidence, WAIT_SECONDS)?, again).map(Done::from);
            }
            DevArgs::Trailers { task } => daemon::task_trailers(&task, id(), run)?,
            DevArgs::Seed { sentence } => daemon::seed(sentence.as_deref())?,
            DevArgs::Run { task, harness, model } => daemon::dispatch_run(&task, id(), harness.as_deref(), model.as_deref())?,
            DevArgs::Queue => daemon::dispatch_queue()?,
            DevArgs::Dequeue { task, reason } => daemon::dispatch_remove(&task, id(), reason.as_deref())?,
            DevArgs::Adapters => daemon::adapters()?,
            DevArgs::Scan { command } => match command {
                ScanArgs::Start { from_baseline } => daemon::scan_start(id(), from_baseline)?,
                ScanArgs::Wait { id: scan } => {
                    let again = format!("towermd dev scan wait {scan}");
                    return classify_progress(daemon::scan_wait(&scan)?, again).map(Done::from);
                }
                ScanArgs::Show { id: scan } => daemon::scan_show(&scan)?,
                ScanArgs::Cancel { id: scan } => daemon::scan_cancel(&scan)?,
            },
            DevArgs::Draft { command } => match command {
                DraftArgs::Template { scan } => daemon::draft_template(&scan)?,
                DraftArgs::Submit { input, producer } => daemon::draft_submit(read_json_input(&input)?, &producer)?,
                DraftArgs::Show { id: draft } => daemon::draft_show(&draft)?,
                DraftArgs::Ls => daemon::draft_list()?,
                DraftArgs::Compare { id: draft } => daemon::draft_compare(&draft)?,
            },
        },
        Command::Task { command } => match command {
            TaskArgs::Add { input } => daemon::task_add(read_json_input(&input)?, id(), run)?,
            TaskArgs::Edit { input } => daemon::task_edit(read_json_input(&input)?, id(), run)?,
            TaskArgs::Claim { id: task, source } => {
                if run.is_some() {
                    daemon::task_status(run)?;
                }
                let task = match task {
                    Some(task) => task,
                    None => only_claimable(&daemon::project_state()?)?,
                };
                daemon::task_claim(&task, id(), reported(source))?
            }
            TaskArgs::Show { id: task } => daemon::task_show(task.as_deref(), run)?,
            TaskArgs::Ls { state } => return Ok(listed(read::list(&daemon::project_state()?, "tasks", state.as_deref(), None, None))),
            TaskArgs::Release => daemon::task_release(id(), run)?,
        },
        Command::Propose { input } => daemon::propose(read_json_input(&input)?, id(), run)?,
        Command::Pause { .. } => daemon::pause_all(id())?,
        Command::Resume { pause_event, .. } => daemon::request_resume_all(id(), pause_event.as_deref())?,
        Command::Status => daemon::task_status(run)?,
        Command::Ask { text, review: Some(stage), artifact, context, .. } => {
            let mut artifacts = artifact;
            artifacts.extend(context);
            let value = daemon::request_review(json!({"request_id": id(), "stage": stage, "summary": text, "artifacts": artifacts}), run)?;
            delivery.refresh(run);
            return classify_review(value).map(Done::from);
        }
        Command::Wait { hold, seconds } if hold.starts_with("V-") => {
            let value = daemon::wait_for_review(&hold, run, seconds.unwrap_or(WAIT_SECONDS))?;
            delivery.refresh(run);
            return classify_review(value).map(Done::from);
        }
        Command::Ask { text, options, recommended, context, target, parked, .. } => {
            let value = daemon::ask(
                json!({"request_id": id(), "text": text, "options": options, "recommended": recommended,
                    "context": context, "targets": target, "mode": if parked { "parked" } else { "live" }}),
                run,
            )?;
            delivery.refresh(run);
            return classify_wait(value, None, parked).map(Done::from);
        }
        // One verb waits for all three: an answer (H-…), a review (V-…, above) or a verification (E-…).
        Command::Wait { hold, seconds } if hold.starts_with("E-") => {
            let again = format!("towermd wait {hold}");
            return classify_finish(daemon::finish_wait(&hold, seconds.unwrap_or(WAIT_SECONDS))?, again).map(Done::from);
        }
        Command::Wait { hold, seconds } => {
            let value = daemon::wait_for_answer(&hold, run, seconds.unwrap_or(WAIT_SECONDS))?;
            delivery.refresh(run);
            return classify_wait(value, Some(&hold), false).map(Done::from);
        }
        Command::Answer { hold, text, relayed, harness, harness_version } => {
            if !relayed {
                return Err(needs_human());
            }
            let value = daemon::relay_answer(
                json!({"request_id": id(), "hold": hold, "text": text, "self_reported": self_reported(harness, harness_version)}),
                run,
            )?;
            delivery.refresh(run);
            value
        }
        Command::Handled { prompt, note } => {
            if let Some(note) = &note {
                if note.trim().is_empty() || note.chars().any(char::is_control) {
                    return Err(Failure::new(ErrorCode::Usage, lang::pick("--note is one nonempty line", "--note 须是非空的一行")));
                }
                fits(note, "--note", towermd_core::question_model::HANDLED_NOTE_CHARS)?;
            }
            daemon::handled(id(), &prompt, note.as_deref(), run)?
        }
        Command::Reply { request, state, summary, next, refs, from } => {
            fits(&summary, "--summary", towermd_core::delegation_projection::SUMMARY_CHARS)?;
            let value = daemon::reply(&request, json!({"request_id": id(), "state": state, "summary": summary, "next": next, "refs": refs, "from": from}), run)?;
            // Replied requests stop being delivered, this envelope included.
            delivery.messages.retain(|message| message["request"].as_str() != Some(request.as_str()));
            value
        }
        Command::Report { state, summary, next, progress, unit, answers, no_answers, about } => {
            fits(&summary, "--summary", towermd_core::delegation_projection::SUMMARY_CHARS)?;
            let progress = progress.as_deref().map(|text| parse_progress(text, unit)).transpose()?;
            let data = json!({"state": state, "summary": summary, "next": next, "progress": progress});
            let mut body = tower_body(id(), "report", data, about);
            // Added after `tower_body`, which drops `false`.
            if answers || no_answers {
                body["data"]["answers"] = json!(answers);
            }
            daemon::tower(body, run)?
        }
        Command::Signal { kind, level, title, detail, value, threshold, deterministic, about } => {
            fits(&title, "--title", towermd_core::tower_api::TITLE_CHARS)?;
            if let Some(detail) = &detail {
                fits(detail, "--detail", towermd_core::tower_api::DETAIL_CHARS)?;
            }
            let data = json!({"kind": kind, "level": level, "title": title, "detail": detail,
                "value": value.map(scalar), "threshold": threshold.map(scalar), "deterministic": deterministic});
            daemon::tower(tower_body(id(), "signal", data, about), run)?
        }
        Command::Summary { lines, about } => {
            let lines = lines.into_iter().map(summary_line).collect::<Result<Vec<_>, _>>()?;
            daemon::tower(tower_body(id(), "summary", json!({"lines": lines}), about), run)?
        }
        Command::Idea { command: IdeaArgs::Add { text } } => {
            if text.trim().is_empty() || text.chars().any(char::is_control) {
                return Err(Failure::new(ErrorCode::Usage, lang::pick("an idea is one nonempty line", "想法须是非空的一行")));
            }
            fits(&text, "TEXT", towermd_core::ideas::TEXT_CHARS)?;
            daemon::idea_add(id(), &text)?
        }
        Command::Ext { command } => extensions(command)?,
        Command::Template { command: TemplateArgs::Ls } => {
            let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
            templates::list(&towermd_core::extensions::Registry::installed(&cwd)?)
        }
        Command::Frame { command: FrameArgs::Show { frame } } => daemon::frame_show(&frame)?,
        Command::Frame { command: FrameArgs::Export { frame, format } } => daemon::frame_export(&frame, format.as_deref())?,
        Command::Frame { command } => daemon::tower(card_body(id(), command)?, run)?,
        Command::Snapshot => daemon::snapshot(id())?,
        Command::Hub { command: HubArgs::Rebuild } => daemon::rebuild_request(id())?,
        Command::Daemon { command } => match command {
            DaemonArgs::Ensure => daemon::ensure()?,
            DaemonArgs::Status => daemon::status()?,
            DaemonArgs::Stop => daemon::stop()?,
            DaemonArgs::Serve => unreachable!("handled in main"),
        },
    };
    Ok(Done::from(value))
}

/// `ext ls` and `ext show` read the same authoritative main that decides
/// what an extension may send; nothing is written.
fn extensions(command: ExtArgs) -> Result<Value, Failure> {
    use towermd_core::extensions::Registry;
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    let registry = Registry::installed(&cwd)?;
    match command {
        ExtArgs::Ls => {
            let extensions: Vec<Value> = registry.extensions.iter().map(|manifest| json!({
                "id": manifest.id, "version": manifest.version, "type": manifest.kind, "path": manifest.path,
                "kinds": manifest.emits.iter().map(|emit| emit.kind.as_str()).collect::<Vec<_>>(),
                "problems": manifest.problems,
            })).collect();
            Ok(json!({"revision": registry.revision, "extensions": extensions}))
        }
        ExtArgs::Show { id } => {
            let manifest = registry.get(&id).ok_or_else(|| {
                Failure::new(ErrorCode::NotFound, lang::pick(format!("extension {id} is not installed on main"), format!("权威主干上没有安装扩展 {id}")))
                    .next(Next::new("towermd ext ls", lang::pick("list installed extensions", "列出已安装的扩展")))
            })?;
            let mut value = serde_json::to_value(manifest).map_err(|e| e.to_string())?;
            value["revision"] = json!(registry.revision);
            Ok(value)
        }
    }
}

/// The longest single wait: under the 90 s a harness tool call allows.
const WAIT_SECONDS: u64 = 85;
/// How many events `history` shows: the newest ones.
const HISTORY_EVENTS: usize = 200;

fn not_found(id: &str) -> Failure {
    Failure::new(ErrorCode::NotFound, lang::pick(format!("no object {id} in this project"), format!("本项目中没有对象 {id}")))
        .next(Next::new("towermd needs --all", lang::pick("list what needs attention", "列出需要关注的事")))
}

fn when(text: &str, flag: &str, now: u64) -> Result<u64, Failure> {
    read::parse_when(text, now).ok_or_else(|| {
        Failure::new(ErrorCode::Usage, lang::pick(format!("{flag} cannot read {text:?}"), format!("{flag} 无法识别 {text:?}")))
            .hint(lang::pick("use 30m, 12h, 2d, a date such as 2026-10-20, or an RFC 3339 time", "可用 30m、12h、2d、日期（如 2026-10-20）或 RFC 3339 时间"))
    })
}

/// The person looked at the project now; only a terminal counts as looking,
/// so a script reading `needs` does not move the "since" mark.
fn looked(tty: bool, now: u64) {
    if !tty {
        return;
    }
    if let Ok(Some((project, root))) = current_project() {
        views::mark_viewed(&project, &root, now);
    }
}

/// `ls` warns when `--since` left out rows that carry no time at all.
fn listed(value: Value) -> Done {
    let untimed = value["untimed"].as_u64().unwrap_or(0);
    let kind = value["kind"].as_str().unwrap_or_default().to_owned();
    let warnings = if untimed > 0 {
        vec![lang::pick(
            format!("{untimed} {kind} keep no time and were left out by --since"),
            format!("{untimed} 条 {kind} 没有时间记录，--since 未列出它们"),
        )]
    } else {
        vec![]
    };
    Done { data: value, next: vec![], warnings }
}

/// FR-193: a task claim without an ID only proceeds when the choice is not a
/// choice. The daemon still runs every gate on the claim itself.
fn only_claimable(state: &Value) -> Result<String, Failure> {
    let candidates = read::claimable(state);
    match candidates.as_slice() {
        [only] => Ok(only["id"].as_str().unwrap_or_default().to_owned()),
        [] => Err(Failure::new(ErrorCode::NotFound, lang::pick("no task can be claimed now", "当前没有可领取的任务"))
            .next(Next::new("towermd ls tasks", lang::pick("list tasks and their states", "列出任务及其状态")))),
        [first, ..] => {
            let first = first["id"].as_str().unwrap_or("<id>");
            Err(Failure::new(
                ErrorCode::Usage,
                lang::pick(format!("{} tasks can be claimed; name one", candidates.len()), format!("有 {} 个任务可领取，请指明一个", candidates.len())),
            )
            .next(Next::new(format!("towermd task claim {first}"), lang::pick("claim this one", "领取这一个")))
            .data(json!({"candidates": candidates})))
        }
    }
}

/// Event metadata for one object, newest last; bodies stay in the hub.
fn history(id: &str, since: Option<u64>) -> Result<Value, Failure> {
    let mut page = daemon::history(Some(id), None, HISTORY_EVENTS)?;
    let events = page["events"].as_array().cloned().unwrap_or_default();
    if events.is_empty() {
        return Err(not_found(id));
    }
    let kept: Vec<Value> = events
        .into_iter()
        .filter(|event| since.is_none_or(|since| event["at"].as_str().map(towermd_core::attention_list::parse_time).is_some_and(|at| at >= since)))
        .collect();
    page["object"] = json!(id);
    page["events"] = json!(kept);
    Ok(page)
}

/// `watch` follows the event stream from now on: one line per event, read
/// only, so it neither delivers messages nor renews a run. It ends on
/// Ctrl-C, or after `--exit-after` seconds with a closing envelope.
fn watch(scope: Option<&str>, exit_after: Option<u64>, format: Format) -> Result<Value, Failure> {
    use std::time::{Duration, Instant};
    let deadline = exit_after.map(|seconds| Instant::now() + Duration::from_secs(seconds));
    let mut after = daemon::history(scope, None, 1)?["seq"].as_u64().unwrap_or(0);
    loop {
        let page = daemon::history(scope, Some(after), HISTORY_EVENTS)?;
        for event in page["events"].as_array().into_iter().flatten() {
            render::watch_line(event, format, &mut io::stdout().lock()).map_err(|error| error.to_string())?;
            after = after.max(event["seq"].as_u64().unwrap_or(after));
        }
        if page["more"] == true {
            continue;
        }
        if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            return Ok(json!({"seq": after, "stopped": true}));
        }
        std::thread::sleep(Duration::from_secs(1));
    }
}

/// Bare `towermd`: inside a project, three lines since the last look and the
/// first screen (at most three items); outside one, the known projects.
fn home(run: Option<&str>, tty: bool, delivery: &mut Delivery) -> Result<Done, Failure> {
    let Some((project, root)) = current_project()? else { return overview() };
    delivery.fetch(run)?;
    views::remember(&project, &root);
    let now = now_ms();
    let value = read::home(&project, &daemon::project_state()?, views::last_view(&project), now);
    looked(tty, now);
    Ok(Done::from(value))
}

/// FR-133: every project this user opened, with its health, first-screen
/// count and latest activity, and one cross-project list of at most three.
/// The daemon computes it; this never starts one: without it, states are unknown.
fn overview() -> Result<Done, Failure> {
    if let Some((project, root)) = current_project()? {
        views::remember(&project, &root);
    }
    let mut warnings = vec![];
    let data = match daemon::overview()? {
        Some(data) => data,
        None => {
            warnings.push(lang::pick("the daemon is not running; states are unknown (towermd up)", "守护进程未运行，状态未知（towermd up）").to_owned());
            let projects: Vec<Value> = views::load().into_iter().map(|(project, view)| {
                let present = Config::load(&view.root).ok().flatten().is_some_and(|config| config.project_id() == project);
                json!({"id": project, "root": view.root, "present": present, "status": "unknown", "health": null, "needs": null, "last_activity": null, "top": null})
            }).collect();
            json!({"projects": projects, "attention": null})
        }
    };
    let next = if data["projects"].as_array().is_none_or(Vec::is_empty) {
        vec![Next::new("towermd init", lang::pick("initialize this repository as a project", "把当前仓库初始化为项目"))]
    } else {
        vec![]
    };
    Ok(Done { data, next, warnings })
}

/// v0.1 keeps no separate local index: the projection is rebuilt from the
/// snapshot and the event stream every time the daemon opens a project.
fn rebuild_index() -> Result<Value, Failure> {
    Ok(json!({"indexes": [], "rebuilt": 0,
        "note": lang::pick("no local index to rebuild; the daemon rebuilds state from snapshots", "本机没有需要重建的索引；守护进程每次都从快照重建状态")}))
}

/// Local checks only; nothing leaves this machine and nothing is started.
fn doctor() -> Result<Value, Failure> {
    let git = std::process::Command::new("git").arg("--version").output().ok().filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned());
    let daemon = daemon::probe()?;
    let mut project = Value::Null;
    if let Some((id, root)) = current_project()? {
        let config = Config::load(&root)?;
        let hub = config.as_ref().map(|config| match config.hub() {
            Ok(towermd_core::config::HubSetting::Local) => json!({"kind": "local"}),
            Ok(towermd_core::config::HubSetting::Remote { url }) => json!({"kind": "remote", "url": url}),
            Err(error) => json!({"kind": "invalid", "error": error}),
        });
        let status = if daemon["current"] == true {
            match daemon::state_at(&root) {
                Ok(state) => state.map_or(Value::Null, |state| state["status"].clone()),
                Err(failure) => json!(failure.message),
            }
        } else {
            Value::Null
        };
        project = json!({"id": id, "root": root, "hub": hub, "status": status});
    }
    // A stopped daemon is fine (the next call starts it); an outdated one is not.
    let outdated = daemon["running"] == true && daemon["current"] != true;
    Ok(json!({"ok": git.is_some() && !outdated, "git": git, "daemon": daemon, "project": project,
        "presence": {"available": false, "reason": lang::pick("human actions are App-only (2026-10-23 ruling)", "人的动作只在 App 中发出（2026-10-23 裁定）")}}))
}

/// Who this call is recorded as. The command line never acts as the person.
fn whoami(run: Option<&str>) -> Value {
    let reported = run.and_then(|run| {
        let state = daemon::project_state().ok()?;
        Some(state["projection"]["Run"][run]["binding"]["self_reported"].clone())
    });
    json!({"actor": "process:towermd-cli", "run": run, "self_reported": reported, "can_act_as_human": false,
        "human_actions": lang::pick("only from the App, signed human:<id>", "只能从 App 发出，署名 human:<id>")})
}

/// CLI §3.5: over-long text is refused, never truncated.
fn fits(text: &str, flag: &str, chars: usize) -> Result<(), Failure> {
    if text.chars().count() > chars {
        return Err(Failure::new(ErrorCode::TooLong, lang::pick(format!("{flag} exceeds {chars} characters"), format!("{flag} 超过 {chars} 字")))
            .hint(lang::pick(format!("keep {flag} within {chars} characters; put details in a referenced document"), format!("{flag} 最多 {chars} 字；细节放进引用的文档"))));
    }
    Ok(())
}

/// `--progress 3/7` with an optional `--unit`.
fn parse_progress(text: &str, unit: Option<String>) -> Result<Value, Failure> {
    let parsed = text.split_once('/').and_then(|(done, total)| Some((done.parse::<u64>().ok()?, total.parse::<u64>().ok()?)));
    match parsed {
        Some((done, total)) if total > 0 && done <= total => Ok(json!({"done": done, "total": total, "unit": unit})),
        _ => Err(Failure::new(ErrorCode::Usage, lang::pick("--progress takes DONE/TOTAL, e.g. 3/7", "--progress 须为 已完成/总数，如 3/7"))),
    }
}

/// A `card` message (FR-147): the frame or card it names, and the operation.
fn card_body(request_id: &str, command: FrameArgs) -> Result<Value, Failure> {
    let (target, op, from) = match command {
        FrameArgs::Show { .. } | FrameArgs::Export { .. } => unreachable!("frame show and export are reads"),
        FrameArgs::Add { frame, title, body, state, fields, parent, from } => {
            (frame, json!({"op": "add", "title": title, "body": body, "state": state, "fields": card_fields(fields)?, "parent": parent}), from)
        }
        FrameArgs::Update { card, title, body, fields, clear, from } => {
            let mut values = card_fields(fields)?;
            for key in clear {
                values.insert(key, Value::Null);
            }
            if title.is_none() && body.is_none() && values.is_empty() {
                return Err(Failure::new(ErrorCode::Usage, lang::pick("say what changes: --title, --body, --field or --clear", "须说明改什么：--title、--body、--field 或 --clear")));
            }
            (card, json!({"op": "update", "title": title, "body": body, "fields": values}), from)
        }
        FrameArgs::Move { card, state, from } => (card, json!({"op": "move", "state": state}), from),
        FrameArgs::Link { card, to, from } => (card, json!({"op": "link", "to": to}), from),
        FrameArgs::Archive { card, from } => (card, json!({"op": "archive"}), from),
        FrameArgs::Propose { frame, rules, from } => (frame, json!({"op": "rules", "rules": read_json_input(&rules)?}), from),
        FrameArgs::Edit { frame, change, from } => (frame, json!({"op": "source", "change": read_json_input(&change)?}), from),
    };
    let mut data = op;
    if let Some(fields) = data.as_object_mut() {
        fields.retain(|key, value| !value.is_null() && !(key == "fields" && value.as_object().is_some_and(|map| map.is_empty())));
    }
    let about = TowerArgs { target: Some(target), from: from.from, from_kind: from.from_kind, refs: Vec::new() };
    Ok(tower_body(request_id, "card", data, about))
}

/// `--field KEY=VALUE` pairs; the value is read like `signal --value`.
fn card_fields(pairs: Vec<String>) -> Result<serde_json::Map<String, Value>, Failure> {
    let mut fields = serde_json::Map::new();
    for pair in pairs {
        let (key, value) = pair.split_once('=').filter(|(key, _)| !key.is_empty())
            .ok_or_else(|| Failure::new(ErrorCode::Usage, lang::pick(format!("--field takes KEY=VALUE: {pair}"), format!("--field 须写成 KEY=VALUE：{pair}"))))?;
        fields.insert(key.to_owned(), scalar(value.to_owned()));
    }
    Ok(fields)
}

/// `--value 42` is a number, `--value true` a boolean, anything else text.
fn scalar(text: String) -> Value {
    serde_json::from_str::<Value>(&text).ok().filter(|value| value.is_number() || value.is_boolean()).unwrap_or(Value::String(text))
}

/// `--line LABEL=VALUE`: a number stays a number, anything else is text.
/// Limits are the core's (FR-139); it refuses rather than cuts.
fn summary_line(text: String) -> Result<Value, Failure> {
    let (label, value) = text.split_once('=').ok_or_else(|| Failure::new(ErrorCode::Usage,
        lang::pick("--line takes LABEL=VALUE", "--line 须写成 标签=值")))?;
    let value = serde_json::from_str::<Value>(value).ok().filter(Value::is_number).unwrap_or_else(|| Value::String(value.to_owned()));
    Ok(json!({"label": label, "value": value}))
}

/// The daemon fills in the project, the date and, inside a run, the
/// delegate and the task.
fn tower_body(request_id: &str, verb: &str, mut data: Value, about: TowerArgs) -> Value {
    if let Some(fields) = data.as_object_mut() {
        fields.retain(|_, value| !value.is_null() && *value != json!(false));
    }
    let from = about.from.map(|id| json!({"id": id, "kind": about.from_kind.as_deref().unwrap_or("source")}));
    json!({"request_id": request_id, "verb": verb, "data": data, "target": about.target, "from": from, "refs": about.refs})
}

/// `ask` returns `{hold, wait:{status}}`; `wait` returns the status directly.
/// "Not yet" is exit 5 so a script can tell it from failure (D6). Only
/// `pending` offers another wait: a deferred question (deadline passed,
/// parked, or an answer awaiting a proposal) answers by a later message, so
/// waiting again would only spin.
fn classify_wait(value: Value, hold: Option<&str>, parked: bool) -> Result<Value, Failure> {
    let status = value.get("wait").unwrap_or(&value)["status"].as_str().unwrap_or_default().to_owned();
    let hold = hold.or(value["hold"].as_str()).unwrap_or("<hold>").to_owned();
    match status.as_str() {
        "answered" => Ok(value),
        "pending" | "deferred" if parked => Ok(value),
        "pending" => Err(Failure::new(ErrorCode::Pending, lang::pick(format!("question {hold} is pending"), format!("问题 {hold} 仍在等待回答")))
            .next(Next::new(format!("towermd wait {hold}"), lang::pick("wait for the answer again", "继续等待回答")))
            .data(value)),
        "deferred" => Err(Failure::new(ErrorCode::Deferred, lang::pick(format!("question {hold} is deferred"), format!("问题 {hold} 已挂起")))
            .hint(lang::pick(
                "end the current turn; the answer arrives in `delivered` of a later towermd call, so do not wait again",
                "结束当前回合；回答会通过后续 towermd 调用的 `delivered` 送达，不要再次等待",
            ))
            .data(value)),
        _ => Err(Failure::new(
            ErrorCode::Internal,
            lang::pick(
                format!("the daemon returned an unknown question status {status:?}; the daemon and CLI versions may differ"),
                format!("守护进程返回了未知的问题状态 {status:?}，可能是守护进程与 CLI 版本不一致"),
            ),
        )
        .next(Next::new("towermd daemon status", lang::pick("check the daemon version", "查看守护进程版本")))
        .data(value)),
    }
}

/// A review is pending until a person decides in the App; it never passes by
/// waiting. A decision other than a pass is still a result, not a failure.
fn classify_review(value: Value) -> Result<Value, Failure> {
    if value["status"] != "pending" {
        return Ok(value);
    }
    let review = value["review"].as_str().unwrap_or("<review>").to_owned();
    Err(Failure::new(ErrorCode::Pending, lang::pick(format!("review {review} is pending"), format!("评审 {review} 仍在等待人处理")))
        .next(Next::new(format!("towermd wait {review}"), lang::pick("wait for the review again", "继续等待评审结论")))
        .data(value))
}

/// A wait that ends while work is still running is exit 5 (D6).
fn classify_progress(value: Value, again: String) -> Result<Value, Failure> {
    if !matches!(value["status"].as_str(), Some("queued" | "running")) {
        return Ok(value);
    }
    Err(Failure::new(ErrorCode::Pending, lang::pick("still running", "仍在运行"))
        .next(Next::new(again, lang::pick("wait again (at most 90 s per call)", "再次等待（每次最多 90 秒）")))
        .data(value))
}

/// A finished verification that did not pass is a failed gate, not a report.
fn classify_finish(value: Value, again: String) -> Result<Value, Failure> {
    let value = classify_progress(value, again)?;
    if !matches!(value["status"].as_str(), Some("failed" | "interrupted")) {
        return Ok(value);
    }
    let message = value["error"].as_str().map(str::to_owned)
        .unwrap_or_else(|| lang::pick("verification did not pass", "验证未通过").to_owned());
    Err(Failure::new(ErrorCode::GateFailed, message).data(value))
}

fn schema(name: Option<&str>) -> Result<Value, Failure> {
    use output::schema;
    let Some(name) = name else {
        return Ok(json!({"commands": schema::COMMANDS.iter().map(|(name, _)| *name).collect::<Vec<_>>()}));
    };
    let output = match name {
        "envelope" => serde_json::from_str(schema::ENVELOPE).ok(),
        "towermd" => serde_json::from_str(schema::ROOT).ok(),
        _ => schema::find(name),
    };
    let Some(output) = output else {
        return Err(Failure::new(ErrorCode::Usage, format!("{name}: {}", lang::pick("no such command", "没有这个命令")))
            .next(Next::new("towermd schema", lang::pick("list command names", "列出命令名"))));
    };
    let mut value = json!({"command": name, "output": output});
    if name == "dev.draft.submit" {
        value["input"] = towermd_devkit::baseline::schema();
    }
    Ok(value)
}

fn needs_human() -> Failure {
    Failure::new(
        ErrorCode::NeedsHuman,
        lang::pick(
            "only relay an answer the person gave in this session; pass --relayed to confirm",
            "只能转述人在本次会话中亲口给出的回答；确认时加 --relayed",
        ),
    )
    .next(Next::new("towermd up", lang::pick("let the person answer in the App", "请人在 App 中回答")))
}

fn self_reported(name: Option<String>, version: Option<String>) -> Option<towermd_devkit::task_model::HarnessSource> {
    name.zip(version).map(|(name, version)| towermd_devkit::task_model::HarnessSource { name, version, ..Default::default() })
}

fn reported(source: args::SourceArgs) -> Option<towermd_devkit::task_model::HarnessSource> {
    self_reported(source.harness, source.harness_version).map(|reported| towermd_devkit::task_model::HarnessSource {
        vendor: source.vendor, model: source.model, skills: source.skills_version, ..reported
    })
}

fn read_json_input(path: &Path) -> Result<Value, Failure> {
    if path == Path::new("-") {
        return read_json_from(&mut io::stdin().lock(), "stdin");
    }
    let file = std::fs::File::open(path).map_err(|error| {
        let code = if error.kind() == io::ErrorKind::NotFound { ErrorCode::NotFound } else { ErrorCode::Usage };
        Failure::new(code, format!("{}: {error}", path.display()))
    })?;
    read_json_from(&mut io::BufReader::new(file), &path.display().to_string())
}

fn read_json_from(reader: &mut impl Read, name: &str) -> Result<Value, Failure> {
    let mut bytes = Vec::new();
    reader
        .take(MAX_INPUT + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| Failure::new(ErrorCode::Usage, format!("{name}: {error}")))?;
    if bytes.len() as u64 > MAX_INPUT {
        return Err(Failure::new(ErrorCode::TooLong, lang::pick(format!("{name}: input exceeds 2 MiB"), format!("{name}：输入超过 2 MiB"))));
    }
    serde_json::from_slice(&bytes).map_err(|error| {
        Failure::new(ErrorCode::Usage, lang::pick(format!("{name}: invalid JSON: {error}"), format!("{name}：JSON 无效：{error}")))
    })
}

fn run_init(path: Option<PathBuf>, docs_dir: Option<String>) -> Result<Value, Failure> {
    let directory = match path {
        Some(path) => path,
        None => std::env::current_dir().map_err(|error| format!("无法读取当前目录：{error}"))?,
    };
    let root = repo::toplevel(&directory).map_err(|error| Failure::new(ErrorCode::NotFound, error))?;
    let existing = Config::load(&root)?;
    let docs_dir = match docs_dir {
        Some(docs_dir) => Some(docs_dir),
        None if existing.is_some() => None,
        None if io::stdin().is_terminal() => ask_docs_dir()?,
        None => return Err(Failure::new(ErrorCode::Usage, "非交互模式须显式传 --docs-dir，请先与人确认目录")),
    };
    let report = init(&directory, &InitOptions { docs_dir }, towermd_devkit::dev_state::KITS)?;
    Ok(serde_json::to_value(report).map_err(|error| error.to_string())?)
}

/// The prompt goes to stderr: stdout carries only the envelope.
fn ask_docs_dir() -> Result<Option<String>, String> {
    let mut stderr = io::stderr().lock();
    write!(stderr, "用户文档目录（相对仓库根）[{DEFAULT_DOCS_DIR}]: ")
        .and_then(|()| stderr.flush())
        .map_err(|error| error.to_string())?;
    read_docs_dir(&mut io::stdin().lock())
}

fn read_docs_dir(reader: &mut impl BufRead) -> Result<Option<String>, String> {
    let mut line = String::new();
    if reader
        .read_line(&mut line)
        .map_err(|error| error.to_string())?
        == 0
    {
        return Err("未收到用户文档目录输入，已取消初始化".into());
    }
    let line = line.trim();
    Ok((!line.is_empty()).then(|| line.to_string()))
}

#[cfg(test)]
mod tests {
    use super::{classify_finish, classify_progress, classify_wait, read_docs_dir, read_json_input, Envelope, ErrorCode};
    use serde_json::json;
    use std::io::Cursor;

    #[test]
    fn eof_cancels_instead_of_accepting_default() {
        let result = read_docs_dir(&mut Cursor::new(b""));
        assert!(result.is_err(), "EOF 不应批准默认目录：{result:?}");
        assert!(result.unwrap_err().contains("取消"));
    }

    #[test]
    fn enter_accepts_default() {
        assert_eq!(read_docs_dir(&mut Cursor::new(b"\n")).unwrap(), None);
    }

    #[test]
    fn input_selects_trimmed_docs_dir() {
        assert_eq!(
            read_docs_dir(&mut Cursor::new(b" docs/plan \n")).unwrap(),
            Some("docs/plan".into())
        );
    }

    #[test]
    fn only_pending_answers_offer_another_wait() {
        let pending = classify_wait(json!({"hold": "H-1", "wait": {"status": "pending"}}), None, false).unwrap_err();
        assert_eq!(pending.code, ErrorCode::Pending);
        assert_eq!(pending.next[0].cmd, "towermd wait H-1");
        assert_eq!(pending.data.as_ref().unwrap()["hold"], "H-1");
        assert!(classify_wait(json!({"status": "answered"}), Some("H-2"), false).is_ok());
        assert!(classify_wait(json!({"hold": "H-3", "wait": {"status": "deferred"}}), None, true).is_ok());
    }

    #[test]
    fn a_deferred_question_ends_the_turn_instead_of_waiting_again() {
        // Deadline passed, parked, or an answer still behind a proposal.
        for value in [json!({"status": "deferred", "hold": "H-2", "proposal": null}), json!({"status": "deferred", "hold": "H-2", "proposal": "P-1"})] {
            let deferred = classify_wait(value.clone(), Some("H-2"), false).unwrap_err();
            assert_eq!(deferred.code, ErrorCode::Deferred);
            assert!(deferred.next.is_empty(), "{:?}", deferred.next);
            assert!(deferred.hint.as_deref().unwrap().contains("delivered"));
            assert_eq!(deferred.data.as_ref().unwrap()["proposal"], value["proposal"]);
            let envelope = serde_json::to_value(Envelope::failure(deferred, None)).unwrap();
            assert_eq!(envelope["next"], json!([]));
            assert!(envelope["error"].get("retry").is_none(), "{envelope}");
            assert_eq!(Envelope::failure(classify_wait(value, Some("H-2"), false).unwrap_err(), None).exit(), 5);
        }
        // `ask` is a write: its request ID is echoed, but not as a safe retry.
        let asked = classify_wait(json!({"hold": "H-4", "wait": {"status": "deferred"}}), None, false).unwrap_err();
        let envelope = serde_json::to_value(Envelope::failure(asked, Some("id"))).unwrap();
        assert_eq!(envelope["error"]["retry"], json!({"safe": false, "request_id": "id"}));
        assert_eq!(envelope["next"], json!([]));
    }

    #[test]
    fn an_unknown_or_missing_wait_status_is_internal() {
        for value in [json!({"status": "approved"}), json!({}), json!({"hold": "H-5", "wait": {}})] {
            let failure = classify_wait(value.clone(), None, false).unwrap_err();
            assert_eq!(failure.code, ErrorCode::Internal, "{value}");
            assert!(classify_wait(value, None, true).is_err(), "parking does not hide an unknown status");
        }
    }

    #[test]
    fn unreadable_input_paths_are_usage_errors() {
        let directory = tempfile::tempdir().unwrap();
        assert_eq!(read_json_input(directory.path()).unwrap_err().code, ErrorCode::Usage);
        assert_eq!(read_json_input(&directory.path().join("missing.json")).unwrap_err().code, ErrorCode::NotFound);
    }

    #[test]
    fn progress_waits_are_pending_and_failed_verification_is_gate_failed() {
        let again = "towermd dev finish --wait E-1".to_owned();
        let running = classify_progress(json!({"status": "running"}), again.clone()).unwrap_err();
        assert_eq!(running.code, ErrorCode::Pending);
        assert_eq!(running.next[0].cmd, again);
        for status in ["failed", "interrupted"] {
            let failed = classify_finish(json!({"status": status, "error": "G2"}), again.clone()).unwrap_err();
            assert_eq!(failed.code, ErrorCode::GateFailed);
            assert_eq!(failed.message, "G2");
            assert_eq!(failed.data.unwrap()["status"], status);
        }
        assert!(classify_finish(json!({"status": "passed"}), again.clone()).is_ok());
        assert!(classify_progress(json!({"status": "failed"}), again).is_ok(), "a failed scan is still a report");
    }
}
