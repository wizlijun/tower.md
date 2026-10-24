//! `.towermd/config.yaml`：项目 ID、用户文档目录、hub 地址（FR-97、FR-98）。不含任何密钥。

use std::fs;
use std::path::Path;

use serde_yaml::{Mapping, Value};

pub const CONFIG_PATH: &str = ".towermd/config.yaml";
pub const DEFAULT_DOCS_DIR: &str = "docs/tower";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HubSetting {
    Local,
    Remote { url: String },
}

#[derive(Debug, Clone)]
pub struct Config {
    map: Mapping,
}

impl Config {
    pub fn new(project_id: &str, docs_dir: &str) -> Result<Self, String> {
        let mut map = Mapping::new();
        map.insert("project_id".into(), project_id.into());
        map.insert("docs_dir".into(), docs_dir.into());
        map.insert("hub".into(), "local".into());
        Self::from_mapping(map)
    }

    pub fn parse(text: &str) -> Result<Self, String> {
        let value: Value = serde_yaml::from_str(text)
            .map_err(|error| format!("{CONFIG_PATH} 不是合法 YAML：{error}"))?;
        let Value::Mapping(map) = value else {
            return Err(format!("{CONFIG_PATH} 顶层必须是映射"));
        };
        Self::from_mapping(map)
    }

    pub fn load(root: &Path) -> Result<Option<Self>, String> {
        let path = root.join(CONFIG_PATH);
        match fs::read_to_string(&path) {
            Ok(text) => Self::parse(&text).map(Some),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(format!("无法读取 {}：{error}", path.display())),
        }
    }

    pub fn to_yaml(&self) -> String {
        serde_yaml::to_string(&self.map).expect("YAML 映射总能序列化")
    }

    pub fn project_id(&self) -> &str {
        self.str_field("project_id").expect("构造时已检查")
    }

    pub fn docs_dir(&self) -> &str {
        self.str_field("docs_dir").expect("构造时已检查")
    }

    /// FR-210 threshold; missing or invalid values fall back to the default.
    pub fn review_backlog(&self) -> usize {
        self.positive("review_backlog").unwrap_or(crate::delegation::DEFAULT_REVIEW_BACKLOG)
    }

    /// FR-204 minimum spot-check rate, clamped to [0.01, 1].
    pub fn spot_check_min(&self) -> f64 {
        use crate::delegation::{DEFAULT_SPOT_CHECK_MIN, SPOT_CHECK_FLOOR};
        match self.map.get("spot_check_min").and_then(Value::as_f64) {
            Some(rate) if rate.is_finite() => rate.clamp(SPOT_CHECK_FLOOR, 1.0),
            _ => DEFAULT_SPOT_CHECK_MIN,
        }
    }

    /// FR-202 number of recent delegations the ledger summarizes.
    pub fn track_record_window(&self) -> usize {
        self.positive("track_record_window").unwrap_or(crate::delegation::DEFAULT_TRACK_RECORD_WINDOW)
    }

    /// A top-level section a kit reads with its own defaults (thresholds
    /// the core does not interpret).
    pub fn section(&self, key: &str) -> Option<&Value> {
        self.map.get(key)
    }

    /// FR-185 ordered level names, lowest first; an absent, empty or
    /// malformed list falls back to the default three.
    pub fn delegation_levels(&self) -> Vec<String> {
        let levels: Option<Vec<String>> = self.map.get("delegation_levels").and_then(Value::as_sequence)
            .and_then(|items| items.iter().map(|item| {
                item.as_str().map(str::trim).filter(|level| !level.is_empty()).map(str::to_owned)
            }).collect());
        match levels {
            Some(levels) if !levels.is_empty() => levels,
            _ => crate::delegation::DelegationSettings::default().delegation_levels,
        }
    }

    pub fn delegation_settings(&self) -> crate::delegation::DelegationSettings {
        crate::delegation::DelegationSettings {
            review_backlog: self.review_backlog(),
            spot_check_min: self.spot_check_min(),
            track_record_window: self.track_record_window(),
            delegation_levels: self.delegation_levels(),
        }
    }

    /// FR-126 stuck thresholds under `stuck:`; each missing, non-positive
    /// or malformed value keeps its default, so a typo never silences it.
    pub fn stuck(&self) -> crate::stuck::Thresholds {
        let defaults = crate::stuck::Thresholds::default();
        let Some(map) = self.map.get("stuck").and_then(Value::as_mapping) else { return defaults };
        let count = |key: &str, default: u32| map.get(key).and_then(Value::as_u64).filter(|value| *value > 0)
            .and_then(|value| u32::try_from(value).ok()).unwrap_or(default);
        let factor = map.get("duration_factor").and_then(Value::as_f64).filter(|value| value.is_finite() && *value > 1.0);
        crate::stuck::Thresholds {
            same_next: count("same_next", defaults.same_next),
            check_failures: count("check_failures", defaults.check_failures),
            asks: count("asks", defaults.asks),
            ask_window_hours: count("ask_window_hours", defaults.ask_window_hours),
            duration_factor: factor.unwrap_or(defaults.duration_factor),
        }
    }
    /// FR-40, FR-128, FR-130 under `attention:`: the first-screen budget and
    /// focus time, with defaults for anything missing or malformed.
    pub fn attention(&self) -> crate::attention::Settings {
        crate::attention::settings(self.map.get("attention"))
    }
    /// FR-135, FR-136 under `quiet:`: `dormant_after` and per-object
    /// `check_every`, in days, with defaults for anything missing or malformed.
    pub fn quiet(&self) -> crate::quiet::Settings {
        crate::quiet::settings(self.map.get("quiet"))
    }
    /// FR-80 Note.md vault, as written under `notemd.vault`; `None` means
    /// not connected. Resolving it to a directory is the connector's job.
    pub fn notemd_vault(&self) -> Option<&str> {
        self.map.get("notemd").and_then(|notemd| notemd.get("vault")).and_then(Value::as_str)
            .map(str::trim).filter(|vault| !vault.is_empty())
    }

    /// FR-84 sequencer choice. A missing `hub` means the local hub; a mapping
    /// with `url` names a self-deployed remote hub. Anything else is an error,
    /// never a silent fallback, so one project cannot end up with two
    /// sequencers. Extra keys under `hub` are ignored here and kept on save.
    pub fn hub(&self) -> Result<HubSetting, String> {
        let Some(value) = self.map.get("hub") else { return Ok(HubSetting::Local) };
        if value.as_str() == Some("local") {
            return Ok(HubSetting::Local);
        }
        let url = value.get("url").and_then(Value::as_str).map(str::trim)
            .ok_or_else(|| format!("{CONFIG_PATH} 的 hub 只接受 `local` 或 `{{ url: ws://… }}`"))?;
        let rest = url.strip_prefix("wss://").or_else(|| url.strip_prefix("ws://"))
            .ok_or_else(|| "hub.url 必须以 ws:// 或 wss:// 开头".to_string())?;
        let authority = rest.split('/').next().unwrap_or_default();
        if authority.is_empty() || authority.contains('@') || url.contains(['?', '#'])
            || url.chars().any(|c| c.is_whitespace() || c.is_control()) {
            return Err("hub.url 须含主机名，且不得带用户信息、查询串或片段（密钥不写入仓库）".into());
        }
        Ok(HubSetting::Remote { url: url.trim_end_matches('/').into() })
    }

    fn positive(&self, key: &str) -> Option<usize> {
        self.map.get(key).and_then(Value::as_u64).filter(|value| *value > 0)
            .and_then(|value| usize::try_from(value).ok())
    }

    fn from_mapping(map: Mapping) -> Result<Self, String> {
        let config = Self { map };
        for key in ["project_id", "docs_dir"] {
            if config.str_field(key).is_none() {
                return Err(format!("{CONFIG_PATH} 缺少非空的 `{key}`"));
            }
        }
        Ok(config)
    }

    fn str_field(&self, key: &str) -> Option<&str> {
        self.map
            .get(key)
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_config_round_trips() {
        let config = Config::new("0192f0aa-0000-7000-8000-000000000001", DEFAULT_DOCS_DIR).unwrap();
        let text = config.to_yaml();
        assert_eq!(
            text,
            "project_id: 0192f0aa-0000-7000-8000-000000000001\ndocs_dir: docs/tower\nhub: local\n"
        );
        let restored = Config::parse(&text).unwrap();
        assert_eq!(
            restored.project_id(),
            "0192f0aa-0000-7000-8000-000000000001"
        );
        assert_eq!(restored.docs_dir(), DEFAULT_DOCS_DIR);
    }

    #[test]
    fn new_rejects_empty_or_whitespace_required_fields() {
        for invalid in ["", " ", "\t\n", "\u{3000}"] {
            assert!(Config::new(invalid, "docs/tower").is_err());
            assert!(Config::new("project", invalid).is_err());
        }
    }

    #[test]
    fn required_strings_are_preserved_without_uuid_or_path_validation() {
        let config = Config::new(" project ", "../external docs/").unwrap();
        assert_eq!(config.project_id(), " project ");
        assert_eq!(config.docs_dir(), "../external docs/");
        let restored = Config::parse(&config.to_yaml()).unwrap();
        assert_eq!(restored.project_id(), config.project_id());
        assert_eq!(restored.docs_dir(), config.docs_dir());
    }

    #[test]
    fn unknown_keys_are_preserved_in_order() {
        let text = "budget: { usd: 5, currency: USD }\nproject_id: p1\nroles: [reviewer, builder]\ndocs_dir: d\nhub: { url: 'wss://hub.example', retries: 3 }\ncustom: { enabled: true, optional: null }\n";
        let original: Mapping = serde_yaml::from_str(text).unwrap();
        let config = Config::parse(text).unwrap();
        let restored: Mapping = serde_yaml::from_str(&config.to_yaml()).unwrap();
        assert_eq!(restored, original);
        assert_eq!(
            restored.keys().collect::<Vec<_>>(),
            original.keys().collect::<Vec<_>>()
        );
        for key in ["budget", "hub", "custom"] {
            assert_eq!(
                restored
                    .get(key)
                    .unwrap()
                    .as_mapping()
                    .unwrap()
                    .keys()
                    .collect::<Vec<_>>(),
                original
                    .get(key)
                    .unwrap()
                    .as_mapping()
                    .unwrap()
                    .keys()
                    .collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn notemd_vault_is_optional_and_trimmed() {
        assert_eq!(Config::parse("project_id: p\ndocs_dir: d\n").unwrap().notemd_vault(), None);
        for text in ["notemd: x\n", "notemd: { vault: ' ' }\n", "notemd: { vault: 3 }\n", "notemd: {}\n"] {
            let config = Config::parse(&format!("project_id: p\ndocs_dir: d\n{text}")).unwrap();
            assert_eq!(config.notemd_vault(), None, "{text}");
        }
        let config = Config::parse("project_id: p\ndocs_dir: d\nnotemd:\n  vault: ' ~/notes '\n").unwrap();
        assert_eq!(config.notemd_vault(), Some("~/notes"));
    }

    #[test]
    fn parse_rejects_missing_required_fields() {
        assert!(Config::parse("docs_dir: d\n").is_err());
        assert!(Config::parse("project_id: p\n").is_err());
    }

    #[test]
    fn parse_rejects_empty_whitespace_or_non_string_required_fields() {
        for invalid in [
            "''",
            "' '",
            "\"\\t\\n\"",
            "'\u{3000}'",
            "null",
            "42",
            "true",
            "[]",
            "{}",
        ] {
            assert!(Config::parse(&format!("project_id: {invalid}\ndocs_dir: d\n")).is_err());
            assert!(Config::parse(&format!("project_id: p\ndocs_dir: {invalid}\n")).is_err());
        }
    }

    #[test]
    fn parse_rejects_invalid_yaml_or_non_mapping_documents() {
        for text in ["project_id: [", "- a\n", "text", "null", ""] {
            assert!(Config::parse(text).is_err());
        }
    }

    #[test]
    fn parse_does_not_insert_default_hub() {
        let config = Config::parse("project_id: p\ndocs_dir: d\n").unwrap();
        let restored: Mapping = serde_yaml::from_str(&config.to_yaml()).unwrap();
        assert_eq!(restored.len(), 2);
        assert!(!restored.contains_key("hub"));
        assert_eq!(config.hub().unwrap(), HubSetting::Local);
    }

    #[test]
    fn hub_accepts_local_or_a_plain_websocket_url_and_rejects_the_rest() {
        let hub = |value: &str| Config::parse(&format!("project_id: p\ndocs_dir: d\nhub: {value}\n")).unwrap().hub();
        assert_eq!(hub("local").unwrap(), HubSetting::Local);
        assert_eq!(hub("{ url: 'wss://hub.example/', retries: 3 }").unwrap(),
            HubSetting::Remote { url: "wss://hub.example".into() });
        assert_eq!(hub("{ url: 'ws://127.0.0.1:8787' }").unwrap(),
            HubSetting::Remote { url: "ws://127.0.0.1:8787".into() });
        for invalid in ["remote", "null", "{ url: 'https://hub.example' }", "{ url: 'wss://' }",
            "{ url: 'wss://user:secret@hub.example' }", "{ url: 'wss://hub.example/?token=x' }", "{ retries: 3 }"] {
            assert!(hub(invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    fn delegation_settings_default_when_keys_are_missing_or_invalid() {
        let config = Config::parse("project_id: p\ndocs_dir: d\n").unwrap();
        assert_eq!(config.delegation_settings(), crate::delegation::DelegationSettings::default());
        assert_eq!(config.review_backlog(), 5);
        assert_eq!(config.spot_check_min(), 0.05);
        assert_eq!(config.track_record_window(), 20);
        assert_eq!(config.delegation_levels(), ["report_only", "answer_by_decision", "approve_minor"]);
        let invalid = Config::parse("project_id: p\ndocs_dir: d\nreview_backlog: -1\nspot_check_min: x\n\
            track_record_window: 0\ndelegation_levels: [a, '']\n").unwrap();
        assert_eq!(invalid.delegation_settings(), crate::delegation::DelegationSettings::default());
    }

    #[test]
    fn delegation_settings_read_values_and_floor_the_spot_check_rate() {
        let config = Config::parse("project_id: p\ndocs_dir: d\nreview_backlog: 8\nspot_check_min: 0\n\
            track_record_window: 10\ndelegation_levels: [watch, act]\n").unwrap();
        assert_eq!(config.review_backlog(), 8);
        assert_eq!(config.spot_check_min(), 0.01);
        assert_eq!(config.track_record_window(), 10);
        assert_eq!(config.delegation_levels(), ["watch", "act"]);
        for (text, rate) in [("0.2", 0.2), ("3", 1.0), ("1", 1.0), ("0.001", 0.01)] {
            let config = Config::parse(&format!("project_id: p\ndocs_dir: d\nspot_check_min: {text}\n")).unwrap();
            assert_eq!(config.spot_check_min(), rate, "{text}");
        }
    }

    #[test]
    fn load_returns_none_when_absent() {
        let directory = tempfile::tempdir().unwrap();
        assert!(Config::load(directory.path()).unwrap().is_none());
        fs::create_dir_all(directory.path().join(".towermd")).unwrap();
        fs::write(
            directory.path().join(CONFIG_PATH),
            "project_id: p\ndocs_dir: d\n",
        )
        .unwrap();
        let config = Config::load(directory.path()).unwrap().unwrap();
        assert_eq!(config.project_id(), "p");
        assert_eq!(config.docs_dir(), "d");
    }

    #[test]
    fn load_reports_invalid_config_and_read_errors() {
        let directory = tempfile::tempdir().unwrap();
        fs::create_dir_all(directory.path().join(".towermd")).unwrap();
        let path = directory.path().join(CONFIG_PATH);
        fs::write(&path, "project_id: p\n").unwrap();
        assert!(Config::load(directory.path()).is_err());
        fs::remove_file(&path).unwrap();
        fs::create_dir(&path).unwrap();
        assert!(Config::load(directory.path()).is_err());
    }

    #[test]
    fn stuck_thresholds_read_leniently_with_defaults() {
        let base = "project_id: p\ndocs_dir: d\n";
        let config = Config::parse(base).unwrap();
        assert_eq!(config.stuck(), crate::stuck::Thresholds::default());
        let set = Config::parse(&format!("{base}stuck:\n  same_next: 5\n  asks: 0\n  ask_window_hours: x\n  duration_factor: 2.5\n")).unwrap();
        let stuck = set.stuck();
        assert_eq!((stuck.same_next, stuck.check_failures, stuck.asks, stuck.ask_window_hours, stuck.duration_factor), (5, 3, 3, 24, 2.5));
        let broken = Config::parse(&format!("{base}stuck: [1]\n")).unwrap();
        assert_eq!(broken.stuck(), crate::stuck::Thresholds::default());
    }

    #[test]
    fn attention_settings_read_from_their_own_section() {
        let base = "project_id: p\ndocs_dir: d\n";
        assert_eq!(Config::parse(base).unwrap().attention(), crate::attention::Settings::default());
        let set = Config::parse(&format!("{base}attention:\n  budget: 2\n  focus_time: [\"22:00-07:30\"]\n")).unwrap().attention();
        assert_eq!(set.budget, 2);
        assert_eq!(set.focus_time, vec![crate::attention::Window { start: 1320, end: 450 }]);
    }

    #[test]
    fn quiet_settings_read_from_their_own_section() {
        let base = "project_id: p\ndocs_dir: d\n";
        assert_eq!(Config::parse(base).unwrap().quiet(), crate::quiet::Settings::default());
        let set = Config::parse(&format!("{base}quiet:\n  dormant_after: 30\n  check_every:\n    T-1: 7\n")).unwrap().quiet();
        assert_eq!((set.dormant_after, set.check_every.get("T-1")), (30, Some(&7)));
    }
}
