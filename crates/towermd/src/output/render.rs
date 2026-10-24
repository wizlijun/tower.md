//! Text and JSON rendering of one envelope. stdout gets the envelope (JSON)
//! or the result and delivered messages (text); stderr gets diagnostics.
//! Raw commands keep text stdout verbatim and move messages and next to stderr.
use std::ffi::OsString;
use std::io::{self, Write};

use serde_json::Value;

use super::lang::{self, Lang};
use super::{Envelope, Next};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Json,
    Text,
}

pub fn format(json: bool, text: bool, tty: bool) -> Format {
    match (json, text) {
        (true, _) => Format::Json,
        (false, true) => Format::Text,
        _ if tty => Format::Text,
        _ => Format::Json,
    }
}

/// Used when clap rejected the line, so the parsed flags are unavailable.
pub fn format_from_args(args: &[OsString], tty: bool) -> Format {
    let flags: Vec<_> = args.iter().skip(1).map(|arg| arg.to_string_lossy()).take_while(|arg| arg != "--").collect();
    format(flags.iter().any(|arg| arg == "--json"), flags.iter().any(|arg| arg == "--text"), tty)
}

/// Writes stdout first. Only a stdout failure is an error: the caller then
/// must not acknowledge delivered messages.
pub fn emit(envelope: &Envelope, format: Format, quiet: bool, command: &str, out: &mut dyn Write, err: &mut dyn Write) -> io::Result<()> {
    let body = match format {
        Format::Json => {
            // `watch` is a stream: every envelope is one line.
            let mut bytes = if command == "watch" { serde_json::to_vec(envelope)? } else { serde_json::to_vec_pretty(envelope)? };
            bytes.push(b'\n');
            bytes
        }
        Format::Text => text_stdout(envelope, quiet, command).into_bytes(),
    };
    out.write_all(&body)?;
    out.flush()?;
    for line in stderr_lines(envelope, format, quiet, command) {
        let _ = writeln!(err, "{line}");
    }
    Ok(())
}

/// Text stdout of these commands is used verbatim: trailers go into a commit
/// message, the template is redirected to a file for `dev draft submit`.
fn raw(command: &str) -> bool {
    matches!(command, "dev.trailers" | "dev.draft.template")
}

fn message_line(message: &Value) -> String {
    let actor = message["actor"].as_str().unwrap_or("?");
    let body = message["text"].as_str().unwrap_or("");
    format!("{} {actor}: {body}", lang::pick("[message]", "[消息]"))
}

fn next_line(next: &Next) -> String {
    format!("{} {}  # {}", lang::pick("next:", "下一步："), next.cmd, next.why)
}

fn text_stdout(envelope: &Envelope, quiet: bool, command: &str) -> String {
    let raw = raw(command);
    let mut text = String::new();
    if !raw {
        envelope.delivered.iter().for_each(|message| text.push_str(&format!("{}\n", message_line(message))));
    }
    if envelope.ok && !quiet {
        if let Some(data) = &envelope.data {
            text.push_str(&data_text(command, data));
        }
        if !raw {
            envelope.next.iter().for_each(|next| text.push_str(&format!("{}\n", next_line(next))));
        }
    }
    text
}

fn stderr_lines(envelope: &Envelope, format: Format, quiet: bool, command: &str) -> Vec<String> {
    let raw = format == Format::Text && raw(command);
    let mut lines: Vec<String> = Vec::new();
    if raw {
        lines.extend(envelope.delivered.iter().map(|message| format!("towermd: {}", message_line(message))));
    }
    lines.extend(envelope.warnings.iter().map(|warning| format!("towermd: {}{warning}", lang::pick("warning: ", "警告："))));
    if raw && envelope.ok && !quiet {
        lines.extend(envelope.next.iter().map(|next| format!("towermd: {}", next_line(next))));
    }
    let Some(error) = &envelope.error else { return lines };
    match format {
        Format::Json => lines.push(format!("towermd: {}: {}", error.code.as_str(), error.message)),
        Format::Text => {
            lines.push(format!("towermd: {}", error.message));
            if let Some(hint) = &error.hint {
                lines.push(format!("towermd: {}{hint}", lang::pick("hint: ", "提示：")));
            }
            if let Some(id) = error.retry.as_ref().filter(|retry| retry.safe).and_then(|retry| retry.request_id.as_deref()) {
                lines.push(format!(
                    "towermd: {}",
                    lang::pick(format!("safe to retry with --request-id {id}"), format!("可沿用 --request-id {id} 安全重试"))
                ));
            }
            lines.extend(envelope.next.iter().map(|next| format!("towermd: {}", next_line(next))));
        }
    }
    lines
}

fn data_text(command: &str, data: &Value) -> String {
    match command {
        "status" => status_text(lang::current(), data),
        "towermd" if data.get("projects").is_some() => overview_text(data),
        "towermd" => home_text(data),
        "overview" => overview_text(data),
        "needs" => needs_text(data),
        "since" => digest_text(data),
        "ls" | "task.ls" => rows_text(data),
        "history" => history_text(data),
        "init" => init_text(lang::current(), data),
        // Raw trailers so `git commit -m "$(towermd --text dev trailers T-1)"` works.
        "dev.trailers" => data["trailers"].as_str().unwrap_or_default().to_owned(),
        // The template itself, ready for `towermd dev draft submit <file>`.
        "dev.draft.template" => format!("{}\n", serde_json::to_string_pretty(data).unwrap_or_default()),
        _ => value_text(data, 0),
    }
}

fn status_text(lang: Lang, data: &Value) -> String {
    let field = |key: &str| data[key].as_str().unwrap_or("-").to_owned();
    let run = &data["current_run"];
    let current = if run.is_null() {
        lang::pick_in(lang, "none", "无").to_owned()
    } else {
        format!("{} ({})", run["binding"]["id"].as_str().unwrap_or("?"), run["task"].as_str().unwrap_or("?"))
    };
    let labels = lang::pick_in(lang, ["project", "branch", "revision", "state", "current run"], ["项目", "分支", "提交", "状态", "当前运行"]);
    let colon = lang::pick_in(lang, ": ", "：");
    let values = [field("project_id"), field("branch"), field("revision"), field("runtime_status"), current];
    labels.iter().zip(values).map(|(label, value)| format!("{label}{colon}{value}\n")).collect()
}

fn init_text(lang: Lang, data: &Value) -> String {
    let strings = |key: &str| data[key].as_array().into_iter().flatten().filter_map(Value::as_str).map(str::to_owned).collect::<Vec<_>>();
    let reused = data["reused_config"].as_bool().unwrap_or(false);
    let (id, docs, created, warning, hint, closing) = lang::pick_in(
        lang,
        ("Project ID", "User docs directory", "Created:", "Warning: ", "Hint: ", "These files are not committed; review and commit them yourself.\n"),
        ("项目 ID", "用户文档目录", "新建：", "警告：", "提示：", "以上文件未提交，请检查后自行提交。\n"),
    );
    let origin = match (lang, reused) {
        (Lang::En, true) => " (reused)",
        (Lang::En, false) => " (new)",
        (Lang::ZhCn, true) => "（沿用已有）",
        (Lang::ZhCn, false) => "（新生成）",
    };
    let colon = lang::pick_in(lang, ": ", "：");
    let mut text = format!(
        "{id}{colon}{}{origin}\n{docs}{colon}{}\n",
        data["project_id"].as_str().unwrap_or(""),
        data["docs_dir"].as_str().unwrap_or("")
    );
    let files = strings("created");
    if !files.is_empty() {
        text.push_str(&format!("{created}\n"));
        files.iter().for_each(|file| text.push_str(&format!("  {file}\n")));
    }
    strings("warnings").iter().for_each(|item| text.push_str(&format!("{warning}{item}\n")));
    strings("hints").iter().for_each(|item| text.push_str(&format!("{hint}{item}\n")));
    text.push_str(closing);
    text
}

/// One event of `towermd watch`: a compact envelope line in JSON, one
/// plain line in text, flushed at once so a pipe sees it immediately.
pub fn watch_line(event: &Value, format: Format, out: &mut dyn Write) -> io::Result<()> {
    match format {
        Format::Json => {
            let envelope = Envelope::success(super::Done::from(event.clone()), None);
            writeln!(out, "{}", serde_json::to_string(&envelope)?)?;
        }
        Format::Text => writeln!(out, "{}", event_line(event))?,
    }
    out.flush()
}

fn event_line(event: &Value) -> String {
    let field = |key: &str| event[key].as_str().unwrap_or("-").to_owned();
    let stale = if event["stale"] == true { lang::pick(" (stale)", "（过期）") } else { "" };
    format!("#{} {} {} {} {}{stale}", event["seq"], field("at"), field("actor"), field("kind"), field("object"))
}

fn digest_text(digest: &Value) -> String {
    let mut text = format!("{}\n", digest["heading"].as_str().unwrap_or_default());
    for line in digest["lines"].as_array().into_iter().flatten().filter_map(Value::as_str) {
        text.push_str(&format!("  {line}\n"));
    }
    text
}

/// The first screen: at most three cards, then how many wait under "later".
fn needs_text(needs: &Value) -> String {
    let items = needs["items"].as_array().cloned().unwrap_or_default();
    if items.is_empty() {
        return format!("{}\n", lang::pick("Nothing needs you.", "没有需要你处理的事。"));
    }
    let mut text = String::new();
    for item in &items {
        text.push_str(&format!("{} {}  -> {}\n", item["symbol"].as_str().unwrap_or("-"), item["card"].as_str().unwrap_or_default(), item["cmd"].as_str().unwrap_or_default()));
    }
    let later = needs["total"].as_u64().unwrap_or(0).saturating_sub(items.len() as u64);
    if later > 0 {
        text.push_str(&format!("{}\n", lang::pick(format!("{later} more later (towermd needs --all)"), format!("另有 {later} 件稍后（towermd needs --all）"))));
    }
    text
}

fn home_text(home: &Value) -> String {
    let mut text = format!("{}{} {}\n", lang::pick("project ", "项目 "), home["project"].as_str().unwrap_or("?"), home["status"].as_str().unwrap_or("?"));
    text.push_str(&digest_text(&home["digest"]));
    text.push_str(&needs_text(&home["needs"]));
    text
}

fn overview_text(overview: &Value) -> String {
    let projects = overview["projects"].as_array().cloned().unwrap_or_default();
    if projects.is_empty() {
        return format!("{}\n", lang::pick("No known projects.", "没有已知项目。"));
    }
    projects.iter().map(|project| {
        let needs = project["needs"].as_u64().map_or_else(|| "-".to_owned(), |count| count.to_string());
        format!("{}  {}  {}{needs}  {}\n", project["id"].as_str().unwrap_or("?"), project["status"].as_str().unwrap_or("?"),
            lang::pick("needs ", "待处理 "), project["root"].as_str().unwrap_or_default())
    }).collect()
}

fn rows_text(list: &Value) -> String {
    let items = list["items"].as_array().cloned().unwrap_or_default();
    if items.is_empty() {
        return format!("{}\n", lang::pick("None.", "无。"));
    }
    items.iter().map(|row| {
        let at = row["at"].as_str().map(|at| format!("  {at}")).unwrap_or_default();
        format!("{}  [{}]  {}{at}\n", row["id"].as_str().unwrap_or("?"), row["state"].as_str().unwrap_or("?"), row["title"].as_str().unwrap_or_default())
    }).collect()
}

fn history_text(history: &Value) -> String {
    let mut text: String = history["events"].as_array().into_iter().flatten().map(|event| format!("{}\n", event_line(event))).collect();
    if history["more"] == true {
        text.push_str(&format!("{}\n", lang::pick("(older events not shown)", "（更早的事件未显示）")));
    }
    text
}

/// Indented key/value text for every result without a dedicated renderer.
fn value_text(value: &Value, indent: usize) -> String {
    let pad = "  ".repeat(indent);
    let nested = |value: &Value| matches!(value, Value::Object(map) if !map.is_empty()) || matches!(value, Value::Array(items) if !items.is_empty());
    match value {
        Value::Object(map) => map
            .iter()
            .map(|(key, item)| if nested(item) { format!("{pad}{key}:\n{}", value_text(item, indent + 1)) } else { format!("{pad}{key}: {}\n", scalar(item)) })
            .collect(),
        Value::Array(items) => items
            .iter()
            .map(|item| if nested(item) { format!("{pad}-\n{}", value_text(item, indent + 1)) } else { format!("{pad}- {}\n", scalar(item)) })
            .collect(),
        other => format!("{pad}{}\n", scalar(other)),
    }
}

fn scalar(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Null => "-".into(),
        Value::Array(_) => "[]".into(),
        Value::Object(_) => "{}".into(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::output::{ErrorCode, Failure, Next};
    use serde_json::json;

    fn emit_to(envelope: &Envelope, format: Format, quiet: bool, command: &str) -> (String, String) {
        let (mut out, mut err) = (Vec::new(), Vec::new());
        emit(envelope, format, quiet, command, &mut out, &mut err).unwrap();
        (String::from_utf8(out).unwrap(), String::from_utf8(err).unwrap())
    }

    // Unit tests never call lang::init, so lang::current() is English.
    #[test]
    fn text_puts_messages_and_data_on_stdout_and_failures_on_stderr() {
        let mut ok = Envelope::success(json!({"hold": "h-1"}).into(), None);
        ok.delivered.push(json!({"actor": "human:bruce", "text": "use CSV"}));
        ok.next.push(Next::new("towermd wait h-1", "wait for the answer"));
        let (out, err) = emit_to(&ok, Format::Text, false, "ask");
        assert_eq!(out, "[message] human:bruce: use CSV\nhold: h-1\nnext: towermd wait h-1  # wait for the answer\n");
        assert!(err.is_empty());
        assert_eq!(emit_to(&ok, Format::Text, true, "ask").0, "[message] human:bruce: use CSV\n");
        let failed = Envelope::failure(Failure::new(ErrorCode::LeaseBusy, "taken").hint("try later"), Some("id-1"));
        let (out, err) = emit_to(&failed, Format::Text, false, "task.claim");
        assert!(out.is_empty());
        assert_eq!(err, "towermd: taken\ntowermd: hint: try later\ntowermd: safe to retry with --request-id id-1\n");
        let (out, err) = emit_to(&failed, Format::Json, false, "task.claim");
        assert_eq!(serde_json::from_str::<Value>(&out).unwrap()["error"]["code"], "lease_busy");
        assert_eq!(err, "towermd: lease_busy: taken\n");
        // A deterministic failure echoes its ID in JSON but is not offered as a retry.
        let conflict = Envelope::failure(Failure::new(ErrorCode::Conflict, "taken"), Some("id-1"));
        assert_eq!(emit_to(&conflict, Format::Text, false, "task.claim").1, "towermd: taken\n");
    }

    #[test]
    fn raw_commands_keep_text_stdout_verbatim_and_move_messages_and_next_to_stderr() {
        let trailers = "Towermd-Task: T-1\nTowermd-Epoch: 3\n";
        let template = json!({"scan_id": "s-1", "modules": [{"id": "A-1"}]});
        for (command, data, expected) in [
            ("dev.trailers", json!({"trailers": trailers}), trailers.to_owned()),
            ("dev.draft.template", template.clone(), format!("{}\n", serde_json::to_string_pretty(&template).unwrap())),
        ] {
            let mut ok = Envelope::success(data.into(), None);
            ok.delivered.push(json!({"actor": "human:bruce", "text": "use CSV"}));
            ok.next.push(Next::new("towermd dev draft submit f.json", "submit"));
            ok.warnings.push("slow hub".into());
            let (out, err) = emit_to(&ok, Format::Text, false, command);
            assert_eq!(out, expected, "{command}");
            assert!(!out.contains("[message]") && !out.contains("next:"), "{command}");
            assert_eq!(
                err,
                "towermd: [message] human:bruce: use CSV\ntowermd: warning: slow hub\ntowermd: next: towermd dev draft submit f.json  # submit\n",
                "{command}"
            );
            let (out, err) = emit_to(&ok, Format::Text, true, command);
            assert!(out.is_empty(), "{command}");
            assert_eq!(err, "towermd: [message] human:bruce: use CSV\ntowermd: warning: slow hub\n", "{command}");
            // JSON keeps messages and next inside the envelope on stdout.
            let (out, err) = emit_to(&ok, Format::Json, false, command);
            let envelope: Value = serde_json::from_str(&out).unwrap();
            assert_eq!(envelope["delivered"][0]["text"], "use CSV");
            assert_eq!(envelope["next"][0]["why"], "submit");
            assert_eq!(err, "towermd: warning: slow hub\n");
        }
        let parsed: Value = serde_json::from_str(&text_stdout(&Envelope::success(template.clone().into(), None), false, "dev.draft.template")).unwrap();
        assert_eq!(parsed, template);
    }

    #[test]
    fn init_text_keeps_v0_1_1_wording() {
        let data = json!({"project_id": "p", "docs_dir": "docs/tower", "reused_config": false,
            "created": ["AGENTS.md"], "warnings": ["w"], "hints": ["h"]});
        assert_eq!(
            init_text(Lang::ZhCn, &data),
            "项目 ID：p（新生成）\n用户文档目录：docs/tower\n新建：\n  AGENTS.md\n警告：w\n提示：h\n以上文件未提交，请检查后自行提交。\n"
        );
        let reused = json!({"project_id": "existing-project", "docs_dir": "docs/tower", "reused_config": true, "created": [], "warnings": [], "hints": []});
        assert!(init_text(Lang::ZhCn, &reused).starts_with("项目 ID：existing-project（沿用已有）\n"));
    }
}
