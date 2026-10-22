//! Delegation card, evidence kinds and ledger settings (FR-184, FR-201–211).
//! Only the core defines the card's fields; a kit may fill values, never add
//! or remove fields.
use chrono::{DateTime, FixedOffset};
use serde::{Deserialize, Serialize};

use crate::question_model::SelfReported;

/// A stop point on the card. With `due` (RFC 3339) it becomes an update
/// request when the delegation takes effect (FR-211).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Checkpoint {
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub due: Option<String>,
}

/// The five fixed fields of a delegation card, in their fixed order.
/// Only `done_when` is required before delegating (G0).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Brief {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
    #[serde(default)]
    pub done_when: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limits: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub checkpoints: Vec<Checkpoint>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub self_check: Option<String>,
}

impl Brief {
    /// Field-level validity. `require_done_when` is the G0 check for a card
    /// that is about to be delegated.
    pub fn check(&self, require_done_when: bool) -> Result<(), String> {
        if require_done_when && self.done_when.is_empty() {
            return Err("G0：委托卡缺少完成标准".into());
        }
        if self.done_when.iter().any(|item| item.trim().is_empty()) {
            return Err("完成标准条目不能为空".into());
        }
        if [&self.why, &self.limits, &self.self_check].into_iter().flatten().any(|text| text.trim().is_empty()) {
            return Err("委托卡的目的、边界或交付前自检不能为空字符串".into());
        }
        for checkpoint in &self.checkpoints {
            if checkpoint.text.trim().is_empty() {
                return Err("检查点不能为空".into());
            }
            if let Some(due) = &checkpoint.due {
                parse_time(due)?;
            }
        }
        Ok(())
    }

    /// The latest checkpoint deadline, if any checkpoint has one.
    pub fn last_due(&self) -> Option<DateTime<FixedOffset>> {
        self.checkpoints.iter().filter_map(|checkpoint| checkpoint.due.as_deref())
            .filter_map(|due| parse_time(due).ok()).max()
    }
}

/// The only three kinds of evidence (FR-209).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    /// Produced by a gate or check program.
    Check,
    /// The delegate's own account; never completes anything alone.
    Claimed,
    /// A person used the result; only a `human:` action produces it.
    Used,
}

pub const DEFAULT_REVIEW_BACKLOG: usize = 5;
pub const DEFAULT_SPOT_CHECK_MIN: f64 = 0.05;
/// Spot checks never drop to zero (FR-204).
pub const SPOT_CHECK_FLOOR: f64 = 0.01;
pub const DEFAULT_TRACK_RECORD_WINDOW: usize = 20;
pub const DEFAULT_DELEGATION_LEVELS: [&str; 3] = ["report_only", "answer_by_decision", "approve_minor"];

/// Ledger settings read from `config.yaml`; missing keys take the defaults.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DelegationSettings {
    pub review_backlog: usize,
    pub spot_check_min: f64,
    pub track_record_window: usize,
    pub delegation_levels: Vec<String>,
}

impl Default for DelegationSettings {
    fn default() -> Self {
        Self {
            review_backlog: DEFAULT_REVIEW_BACKLOG,
            spot_check_min: DEFAULT_SPOT_CHECK_MIN,
            track_record_window: DEFAULT_TRACK_RECORD_WINDOW,
            delegation_levels: DEFAULT_DELEGATION_LEVELS.iter().map(|level| (*level).to_owned()).collect(),
        }
    }
}

/// Canonical JSON of the five self-reported fields (FR-203): absent fields are
/// `null`, keys sorted. An unknown delegate has the key `unknown`.
pub fn fingerprint(source: Option<&SelfReported>) -> String {
    let Some(source) = source else { return "unknown".into() };
    serde_json::json!({
        "model": source.model, "name": source.name, "skills": source.skills,
        "vendor": source.vendor, "version": source.version,
    }).to_string()
}

pub fn parse_time(value: &str) -> Result<DateTime<FixedOffset>, String> {
    DateTime::parse_from_rfc3339(value).map_err(|_| format!("时间须为 RFC 3339：{value}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_optional_fields_are_omitted() {
        let brief = Brief { done_when: vec!["a".into()], ..Brief::default() };
        assert_eq!(serde_json::to_string(&brief).unwrap(), r#"{"done_when":["a"]}"#);
        let full: Brief = serde_json::from_str(r#"{"why":"w","done_when":["a"],"limits":"l",
            "checkpoints":[{"text":"c","due":"2026-10-25T18:00:00+08:00"},{"text":"d"}],"self_check":"s"}"#).unwrap();
        assert_eq!(full.checkpoints[1].due, None);
        assert!(full.check(true).is_ok());
        assert!(serde_json::from_str::<Brief>(r#"{"done_when":[],"extra":1}"#).is_err());
    }

    #[test]
    fn card_checks_reject_blank_fields_and_bad_times() {
        assert!(Brief::default().check(true).unwrap_err().contains("G0"));
        assert!(Brief::default().check(false).is_ok());
        let blank = Brief { done_when: vec![" ".into()], ..Brief::default() };
        assert!(blank.check(false).is_err());
        let blank = Brief { done_when: vec!["a".into()], why: Some("".into()), ..Brief::default() };
        assert!(blank.check(true).is_err());
        let bad = Brief { done_when: vec!["a".into()],
            checkpoints: vec![Checkpoint { text: "c".into(), due: Some("tomorrow".into()) }], ..Brief::default() };
        assert!(bad.check(true).unwrap_err().contains("RFC 3339"));
    }

    #[test]
    fn last_due_is_the_latest_dated_checkpoint() {
        let brief = Brief { done_when: vec!["a".into()], checkpoints: vec![
            Checkpoint { text: "x".into(), due: Some("2026-10-25T10:00:00Z".into()) },
            Checkpoint { text: "y".into(), due: None },
            Checkpoint { text: "z".into(), due: Some("2026-10-26T10:00:00+08:00".into()) },
        ], ..Brief::default() };
        assert_eq!(brief.last_due().unwrap().to_rfc3339(), "2026-10-26T10:00:00+08:00");
        assert!(Brief::default().last_due().is_none());
    }

    #[test]
    fn fingerprint_is_canonical_and_changes_with_any_field() {
        let base = SelfReported { name: "a".into(), version: "1".into(), ..SelfReported::default() };
        assert_eq!(fingerprint(Some(&base)),
            r#"{"model":null,"name":"a","skills":null,"vendor":null,"version":"1"}"#);
        let mut model = base.clone();
        model.model = Some("m".into());
        assert_ne!(fingerprint(Some(&base)), fingerprint(Some(&model)));
        assert_eq!(fingerprint(None), "unknown");
    }

    #[test]
    fn self_reported_round_trips_old_and_new_forms() {
        let old = r#"{"name":"a","version":"1"}"#;
        let parsed: SelfReported = serde_json::from_str(old).unwrap();
        assert_eq!(serde_json::to_string(&parsed).unwrap(), old);
        let new = r#"{"name":"a","version":"1","vendor":"v","model":"m","skills":"0.2.0"}"#;
        let parsed: SelfReported = serde_json::from_str(new).unwrap();
        assert_eq!(serde_json::to_string(&parsed).unwrap(), new);
        assert!(parsed.check().is_ok());
        assert!(SelfReported { model: Some(" ".into()), ..parsed }.check().is_err());
        assert!(serde_json::from_str::<SelfReported>(r#"{"name":"a","version":"1","x":1}"#).is_err());
    }

    #[test]
    fn source_kinds_have_three_wire_names() {
        assert_eq!(serde_json::to_value([SourceKind::Check, SourceKind::Claimed, SourceKind::Used]).unwrap(),
            serde_json::json!(["check", "claimed", "used"]));
    }
}
