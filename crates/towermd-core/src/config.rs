//! `.towermd/config.yaml`：项目 ID、用户文档目录、hub 地址（FR-97、FR-98）。不含任何密钥。

use std::fs;
use std::path::Path;

use serde_yaml::{Mapping, Value};

pub const CONFIG_PATH: &str = ".towermd/config.yaml";
pub const DEFAULT_DOCS_DIR: &str = "docs/tower";

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
}
