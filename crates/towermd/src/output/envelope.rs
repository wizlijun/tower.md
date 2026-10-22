//! The one JSON document every CLI call prints on stdout (contract C0, v1).
use serde::Serialize;
use serde_json::Value;

use super::ErrorCode;

pub const VERSION: u8 = 1;

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Next {
    pub cmd: String,
    pub why: String,
}

impl Next {
    pub fn new(cmd: impl Into<String>, why: impl Into<String>) -> Self {
        Self { cmd: cmd.into(), why: why.into() }
    }
}

/// A successful command result before delivery and request ID are attached.
#[derive(Debug, Default)]
pub struct Done {
    pub data: Value,
    pub next: Vec<Next>,
    pub warnings: Vec<String>,
}

impl From<Value> for Done {
    fn from(data: Value) -> Self {
        Self { data, ..Self::default() }
    }
}

#[derive(Debug)]
pub struct Failure {
    pub code: ErrorCode,
    pub message: String,
    pub hint: Option<String>,
    pub data: Option<Value>,
    pub next: Vec<Next>,
    /// The condition does not clear by itself, even under a transient code.
    pub lasting: bool,
}

impl Failure {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self { code, message: message.into(), hint: None, data: None, next: Vec::new(), lasting: false }
    }
    /// Repeating the call unchanged cannot succeed until something else is done first.
    pub fn lasting(mut self) -> Self {
        self.lasting = true;
        self
    }
    pub fn hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }
    pub fn data(mut self, data: Value) -> Self {
        self.data = Some(data);
        self
    }
    pub fn next(mut self, next: Next) -> Self {
        self.next.push(next);
        self
    }
}

/// Plain error strings from shared helpers are internal until classified.
impl From<String> for Failure {
    fn from(message: String) -> Self {
        Self::new(ErrorCode::Internal, message)
    }
}

impl From<&str> for Failure {
    fn from(message: &str) -> Self {
        Self::new(ErrorCode::Internal, message)
    }
}

#[derive(Debug, PartialEq, Serialize)]
pub struct Retry {
    pub safe: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ErrorBody {
    pub code: ErrorCode,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retry: Option<Retry>,
}

#[derive(Debug, Serialize)]
pub struct Envelope {
    pub v: u8,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorBody>,
    pub delivered: Vec<Value>,
    pub next: Vec<Next>,
    pub warnings: Vec<String>,
}

impl Envelope {
    pub fn success(done: Done, request_id: Option<&str>) -> Self {
        let mut data = done.data;
        if let (Some(id), Value::Object(map)) = (request_id, &mut data) {
            map.entry("request_id").or_insert_with(|| Value::String(id.to_owned()));
        }
        Self {
            v: VERSION,
            ok: true,
            data: Some(data),
            error: None,
            delivered: Vec::new(),
            next: done.next,
            warnings: done.warnings,
        }
    }

    pub fn failure(failure: Failure, request_id: Option<&str>) -> Self {
        // Only a transient condition is worth repeating unchanged; a write
        // repeats with the same request ID because the hub deduplicates by
        // eid. A deterministic failure of a write echoes its request ID but
        // is not safe to repeat as is. A malformed call or one that needs a
        // person is not offered as a retry at all.
        let request_id = request_id.map(str::to_owned);
        let retry = match failure.code {
            ErrorCode::Usage | ErrorCode::TooLong | ErrorCode::NeedsHuman => None,
            code if code.transient() && !failure.lasting => Some(Retry { safe: true, request_id }),
            _ if request_id.is_some() => Some(Retry { safe: false, request_id }),
            _ => None,
        };
        Self {
            v: VERSION,
            ok: false,
            data: failure.data,
            error: Some(ErrorBody { code: failure.code, message: failure.message, hint: failure.hint, retry }),
            delivered: Vec::new(),
            next: failure.next,
            warnings: Vec::new(),
        }
    }

    pub fn exit(&self) -> u8 {
        self.error.as_ref().map_or(0, |error| error.code.exit())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn success_echoes_request_id_without_overwriting_data() {
        let envelope = Envelope::success(json!({"event": "e"}).into(), Some("id-1"));
        let value = serde_json::to_value(&envelope).unwrap();
        assert_eq!(value, json!({"v":1,"ok":true,"data":{"event":"e","request_id":"id-1"},"delivered":[],"next":[],"warnings":[]}));
        let kept = Envelope::success(json!({"request_id": "server"}).into(), Some("id-1"));
        assert_eq!(kept.data.unwrap()["request_id"], "server");
        let list = Envelope::success(json!([1]).into(), Some("id-1"));
        assert_eq!(list.data.unwrap(), json!([1]));
        assert_eq!(Envelope::success(json!([1]).into(), None).exit(), 0);
    }

    #[test]
    fn failure_retry_follows_code_and_request_id() {
        let usage = Envelope::failure(Failure::new(ErrorCode::Usage, "bad"), Some("id"));
        assert_eq!(usage.error.as_ref().unwrap().retry, None);
        assert_eq!(usage.exit(), 2);
        let busy = Envelope::failure(Failure::new(ErrorCode::LeaseBusy, "held"), Some("id"));
        assert_eq!(busy.error.as_ref().unwrap().retry, Some(Retry { safe: true, request_id: Some("id".into()) }));
        let offline = Envelope::failure(Failure::new(ErrorCode::DaemonUnavailable, "down"), None);
        assert_eq!(offline.error.as_ref().unwrap().retry, Some(Retry { safe: true, request_id: None }));
        let missing = Envelope::failure(Failure::new(ErrorCode::NotFound, "gone").hint("h"), None);
        let value = serde_json::to_value(&missing).unwrap();
        assert_eq!(value["error"], json!({"code":"not_found","message":"gone","hint":"h"}));
        assert!(value.get("data").is_none());
        assert_eq!(missing.exit(), 3);
    }

    #[test]
    fn deterministic_write_failures_echo_the_request_id_but_are_not_safe() {
        for code in [ErrorCode::GateFailed, ErrorCode::Conflict, ErrorCode::Rejected, ErrorCode::Deferred, ErrorCode::Internal] {
            let envelope = Envelope::failure(Failure::new(code, "no"), Some("id"));
            assert_eq!(envelope.error.as_ref().unwrap().retry, Some(Retry { safe: false, request_id: Some("id".into()) }), "{code:?}");
            assert_eq!(Envelope::failure(Failure::new(code, "no"), None).error.unwrap().retry, None, "{code:?}");
        }
    }

    #[test]
    fn a_lasting_condition_is_not_safe_to_retry_even_under_a_transient_code() {
        let outdated = Envelope::failure(Failure::new(ErrorCode::DaemonUnavailable, "old").lasting(), None);
        assert_eq!(outdated.error.as_ref().unwrap().retry, None);
        assert_eq!(outdated.exit(), 8);
        let write = Envelope::failure(Failure::new(ErrorCode::DaemonUnavailable, "old").lasting(), Some("id"));
        assert_eq!(write.error.unwrap().retry, Some(Retry { safe: false, request_id: Some("id".into()) }));
    }

    #[test]
    fn usage_and_needs_human_are_never_offered_as_retries() {
        for code in [ErrorCode::Usage, ErrorCode::NeedsHuman] {
            let envelope = Envelope::failure(Failure::new(code, "no"), Some("id"));
            assert_eq!(envelope.error.as_ref().unwrap().retry, None, "{code:?}");
            let value = serde_json::to_value(&envelope).unwrap();
            assert!(value["error"].get("retry").is_none(), "{code:?}");
        }
    }
}
