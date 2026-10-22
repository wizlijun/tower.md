//! Text and JSON rendering of one envelope. stdout gets the envelope (JSON)
//! or the result and delivered messages (text); stderr gets diagnostics.
use std::ffi::OsString;
use std::io::{self, Write};

use serde_json::Value;

use super::lang::{self, Lang};
use super::Envelope;

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
            let mut bytes = serde_json::to_vec_pretty(envelope)?;
            bytes.push(b'\n');
            bytes
        }
        Format::Text => text_stdout(envelope, quiet, command).into_bytes(),
    };
    out.write_all(&body)?;
    out.flush()?;
    for line in stderr_lines(envelope, format) {
        let _ = writeln!(err, "{line}");
    }
    Ok(())
}

fn text_stdout(envelope: &Envelope, quiet: bool, command: &str) -> String {
    let mut text = String::new();
    for message in &envelope.delivered {
        let actor = message["actor"].as_str().unwrap_or("?");
        let body = message["text"].as_str().unwrap_or("");
        text.push_str(&format!("{} {actor}: {body}\n", lang::pick("[message]", "[消息]")));
    }
    if envelope.ok && !quiet {
        if let Some(data) = &envelope.data {
            text.push_str(&data_text(command, data));
        }
        for next in &envelope.next {
            text.push_str(&format!("{} {}  # {}\n", lang::pick("next:", "下一步："), next.cmd, next.why));
        }
    }
    text
}

fn stderr_lines(envelope: &Envelope, format: Format) -> Vec<String> {
    let mut lines: Vec<String> =
        envelope.warnings.iter().map(|warning| format!("towermd: {}{warning}", lang::pick("warning: ", "警告："))).collect();
    let Some(error) = &envelope.error else { return lines };
    match format {
        Format::Json => lines.push(format!("towermd: {}: {}", error.code.as_str(), error.message)),
        Format::Text => {
            lines.push(format!("towermd: {}", error.message));
            if let Some(hint) = &error.hint {
                lines.push(format!("towermd: {}{hint}", lang::pick("hint: ", "提示：")));
            }
            if let Some(id) = error.retry.as_ref().and_then(|retry| retry.request_id.as_deref()) {
                lines.push(format!(
                    "towermd: {}",
                    lang::pick(format!("safe to retry with --request-id {id}"), format!("可沿用 --request-id {id} 安全重试"))
                ));
            }
            for next in &envelope.next {
                lines.push(format!("towermd: {} {}  # {}", lang::pick("next:", "下一步："), next.cmd, next.why));
            }
        }
    }
    lines
}

fn data_text(command: &str, data: &Value) -> String {
    match command {
        "status" => status_text(lang::current(), data),
        "init" => init_text(lang::current(), data),
        // Raw trailers so `git commit -m "$(towermd --text dev trailers T-1)"` works.
        "dev.trailers" => data["trailers"].as_str().unwrap_or_default().to_owned(),
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
    let labels = lang::pick_in(lang, ["project", "branch", "revision", "state", "current run"], ["项目", "分支", "版本", "状态", "当前运行"]);
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
        let failed = Envelope::failure(Failure::new(ErrorCode::Conflict, "taken").hint("try later"), Some("id-1"));
        let (out, err) = emit_to(&failed, Format::Text, false, "task.claim");
        assert!(out.is_empty());
        assert_eq!(err, "towermd: taken\ntowermd: hint: try later\ntowermd: safe to retry with --request-id id-1\n");
        let (out, err) = emit_to(&failed, Format::Json, false, "task.claim");
        assert_eq!(serde_json::from_str::<Value>(&out).unwrap()["error"]["code"], "conflict");
        assert_eq!(err, "towermd: conflict: taken\n");
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
