mod app_launch;
mod args;
mod daemon;
mod output;
mod project_service;
mod worker;

use std::ffi::OsString;
use std::io::{self, BufRead, IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::Parser;
use serde_json::{json, Value};
use towermd_core::catalog::Catalog;
use towermd_core::config::{Config, DEFAULT_DOCS_DIR};
use towermd_core::init::{init, InitOptions};
use towermd_core::repo;

use args::{BaselineArgs, Cli, Command, DaemonArgs, HubArgs, ProjectArgs, TaskArgs};
use output::render::{self, Format};
use output::{lang, Done, Envelope, ErrorCode, Failure};

const MAX_INPUT: u64 = 2 * 1024 * 1024;

fn main() -> ExitCode {
    let argv: Vec<OsString> = std::env::args_os().collect();
    lang::init(&argv);
    let tty = io::stdout().is_terminal();
    let cli = match Cli::try_parse_from(&argv) {
        Ok(cli) => cli,
        Err(error) => return clap_exit(error, render::format_from_args(&argv, tty)),
    };
    // Internal process protocols keep their own plain stdout.
    match &cli.command {
        Command::VerificationWorker => return plain_exit(worker::serve()),
        Command::Daemon { command: DaemonArgs::Serve } => return plain_exit(daemon::serve()),
        _ => {}
    }
    let format = render::format(cli.json, cli.text, tty);
    let name = cli.command.name();
    let mut delivery = Delivery::default();
    let request_id = match request_id(cli.request_id, cli.command.is_write()) {
        Ok(id) => id,
        Err(failure) => return finish(Err(failure), delivery, None, format, cli.quiet, name),
    };
    let result = execute(cli.command, cli.run.as_deref(), request_id.as_deref(), &mut delivery);
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
                format!("task {task} needs review: the new baseline affects its grant {baseline}; review the task before continuing."),
                format!("任务 {task} 需复核：新基线影响了原授权 {baseline}，请先审阅任务再继续执行。"),
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

fn execute(command: Command, run: Option<&str>, request_id: Option<&str>, delivery: &mut Delivery) -> Result<Done, Failure> {
    if command.repo_bound() && in_project(run)? {
        delivery.fetch(run)?;
        if command.renews_run() {
            daemon::task_status(run)?;
        }
    }
    let id = || request_id.expect("write commands always carry a request ID");
    let value = match command {
        Command::VerificationWorker | Command::Daemon { command: DaemonArgs::Serve } => unreachable!("handled in main"),
        Command::Up => app_launch::up()?,
        Command::Init { path, docs_dir } => run_init(path, docs_dir)?,
        Command::Project { command: ProjectArgs::View } => daemon::project_view()?,
        Command::Baseline { command } => match command {
            BaselineArgs::Schema => towermd_devkit::baseline::schema(),
            BaselineArgs::Template { id } => daemon::baseline_template(&id)?,
            BaselineArgs::List => daemon::baseline_list()?,
            BaselineArgs::Submit { input, producer } => daemon::baseline_submit(read_json_input(&input)?, &producer)?,
            BaselineArgs::Scan { wait } => daemon::baseline_scan(wait.as_deref())?,
            BaselineArgs::Show { id } => daemon::baseline_show(&id)?,
            BaselineArgs::Cancel { id } => daemon::baseline_cancel(&id)?,
        },
        Command::Task { command } => match command {
            TaskArgs::Add { input } => daemon::task_add(read_json_input(&input)?, id(), run)?,
            TaskArgs::Claim { id: task, harness, harness_version } => {
                if run.is_some() {
                    daemon::task_status(run)?;
                }
                daemon::task_claim(&task, id(), self_reported(harness, harness_version))?
            }
            TaskArgs::Show { id: task } => daemon::task_show(task.as_deref(), run)?,
            TaskArgs::Release => daemon::task_release(id(), run)?,
        },
        Command::Propose { input } => daemon::propose(read_json_input(&input)?, id(), run)?,
        Command::Pause { .. } => daemon::pause_all(id())?,
        Command::Resume { pause_event, .. } => daemon::request_resume_all(id(), pause_event.as_deref())?,
        Command::Status => daemon::task_status(run)?,
        Command::Check => daemon::task_check(run)?,
        Command::Finish { wait } => daemon::task_finish(id(), run, wait.as_deref())?,
        Command::Trailers { task } => daemon::task_trailers(&task, id(), run)?,
        Command::Ask { text, wait, options, recommended, context, target, parked } => {
            let value = match wait {
                Some(hold) => daemon::wait_for_answer(&hold, run)?,
                None => daemon::ask(
                    json!({"request_id": id(), "text": text.ok_or("ask 缺少问题正文")?, "options": options,
                        "recommended": recommended, "context": context, "targets": target,
                        "mode": if parked { "parked" } else { "live" }}),
                    run,
                )?,
            };
            delivery.refresh(run);
            value
        }
        Command::Wait { hold } => {
            let value = daemon::wait_for_answer(&hold, run)?;
            delivery.refresh(run);
            value
        }
        Command::Answer { hold, text, harness, harness_version } => {
            let value = daemon::relay_answer(
                json!({"request_id": id(), "hold": hold, "text": text, "self_reported": self_reported(harness, harness_version)}),
                run,
            )?;
            delivery.refresh(run);
            value
        }
        Command::Snapshot => daemon::snapshot()?,
        Command::Hub { command: HubArgs::Rebuild } => daemon::rebuild_request()?,
        Command::Daemon { command } => match command {
            DaemonArgs::Ensure => daemon::ensure()?,
            DaemonArgs::Status => daemon::status()?,
            DaemonArgs::Stop => daemon::stop()?,
            DaemonArgs::Serve => unreachable!("handled in main"),
        },
    };
    Ok(Done::from(value))
}

fn self_reported(name: Option<String>, version: Option<String>) -> Option<towermd_devkit::task_model::HarnessSource> {
    name.zip(version).map(|(name, version)| towermd_devkit::task_model::HarnessSource { name, version })
}

fn read_json_input(path: &Path) -> Result<Value, Failure> {
    let file = std::fs::File::open(path).map_err(|error| {
        let code = if error.kind() == io::ErrorKind::NotFound { ErrorCode::NotFound } else { ErrorCode::Usage };
        Failure::new(code, format!("{}: {error}", path.display()))
    })?;
    read_json_from(&mut io::BufReader::new(file), &path.display().to_string())
}

fn read_json_from(reader: &mut impl Read, name: &str) -> Result<Value, Failure> {
    let mut bytes = Vec::new();
    reader.take(MAX_INPUT + 1).read_to_end(&mut bytes).map_err(|error| format!("{name}: {error}"))?;
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
    use super::read_docs_dir;
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
}
