use serde_yaml::{Mapping, Value};

use crate::types;

#[derive(Debug, Clone)]
pub struct Document {
    front: Mapping,
    body: String,
    original: Option<String>,
}

impl Document {
    pub fn parse(text: &str) -> Result<Self, String> {
        let (yaml, body) = leading_frontmatter(text)?;
        let front = match serde_yaml::from_str::<Value>(yaml)
            .map_err(|error| format!("frontmatter 不是合法 YAML：{error}"))?
        {
            Value::Mapping(mapping) => mapping,
            Value::Null => Mapping::new(),
            _ => return Err("frontmatter 必须是映射".into()),
        };
        let document = Self {
            front,
            body: body.to_string(),
            original: Some(text.to_string()),
        };
        if document.doc_type().is_none() {
            return Err("frontmatter 缺少非空的 `type`".into());
        }
        Ok(document)
    }

    pub fn new(doc_type: &str, body: &str) -> Result<Self, String> {
        let mut document = Self {
            front: Mapping::new(),
            body: body.to_string(),
            original: None,
        };
        document.set("type", Value::from(doc_type))?;
        Ok(document)
    }

    pub fn doc_type(&self) -> Option<&str> {
        self.get_str("type")
            .filter(|doc_type| !doc_type.trim().is_empty())
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        self.front.get(key)
    }

    pub fn get_str(&self, key: &str) -> Option<&str> {
        self.get(key).and_then(Value::as_str)
    }

    pub fn front(&self) -> &Mapping {
        &self.front
    }

    pub fn body(&self) -> &str {
        &self.body
    }

    pub fn set(&mut self, key: &str, value: Value) -> Result<(), String> {
        if key == "type" {
            let name = value.as_str().unwrap_or_default();
            if !types::registered(name) {
                return Err(format!("未登记的对象类型 `{name}`"));
            }
        }
        let mut candidate = self.front.clone();
        candidate.insert(Value::from(key), value);
        serde_yaml::to_string(&candidate)
            .map_err(|error| format!("frontmatter 无法序列化：{error}"))?;
        self.front = candidate;
        self.original = None;
        Ok(())
    }

    pub fn to_text(&self) -> String {
        if let Some(original) = &self.original {
            return original.clone();
        }
        let yaml = serde_yaml::to_string(&self.front).expect("已验证的 frontmatter 应能序列化");
        format!("---\n{yaml}---\n{}", self.body)
    }
}

fn leading_frontmatter(text: &str) -> Result<(&str, &str), String> {
    let mut offset = 0usize;
    let mut yaml_start = None;
    for line in text.split_inclusive('\n') {
        let start = offset;
        offset += line.len();
        let is_delimiter = line.trim_end_matches(['\r', '\n']) == "---";
        match yaml_start {
            None if is_delimiter => yaml_start = Some(offset),
            None => return Err("文档必须以 `---` frontmatter 开头".into()),
            Some(yaml_offset) if is_delimiter => {
                return Ok((&text[yaml_offset..start], &text[offset..]));
            }
            Some(_) => {}
        }
    }
    Err("frontmatter 没有闭合的 `---`".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_yaml::value::{Tag, TaggedValue};

    const MODULE: &str = "---\ntype: Module\nid: A-sync\nlayer: L1\n# 注释保留\nx-custom: { keep: [1, 2] }\npaths: ['src/sync/**']\n---\n负责同步。\n";

    #[test]
    fn unmodified_round_trip_is_byte_identical() {
        let document = Document::parse(MODULE).unwrap();
        assert_eq!(document.to_text(), MODULE);
    }

    #[test]
    fn crlf_round_trip_is_byte_identical() {
        let text = MODULE.replace('\n', "\r\n");
        let document = Document::parse(&text).unwrap();
        assert_eq!(document.to_text(), text);
    }

    #[test]
    fn reads_fields_and_body() {
        let document = Document::parse(MODULE).unwrap();
        assert_eq!(document.doc_type(), Some("Module"));
        assert_eq!(document.get_str("id"), Some("A-sync"));
        assert_eq!(document.body(), "负责同步。\n");
        assert_eq!(document.front().get("id"), document.get("id"));
        assert_eq!(document.get("missing"), None);
        assert_eq!(document.get_str("paths"), None);
    }

    #[test]
    fn unknown_type_is_accepted() {
        let document = Document::parse("---\ntype: FutureThing\n---\n").unwrap();
        assert_eq!(document.doc_type(), Some("FutureThing"));
    }

    #[test]
    fn missing_or_empty_type_is_rejected() {
        assert!(Document::parse("---\nid: A\n---\n").is_err());
        assert!(Document::parse("---\ntype: ''\n---\n").is_err());
        assert!(Document::parse("---\n---\n").is_err());
    }

    #[test]
    fn missing_or_unclosed_frontmatter_is_rejected() {
        assert!(Document::parse("# 没有 frontmatter\n").is_err());
        assert!(Document::parse("---\ntype: Module\n").is_err());
        assert!(Document::parse("---\n- a\n- b\n---\n").is_err());
        assert!(Document::parse("").is_err());
    }

    #[test]
    fn edit_keeps_unknown_keys_order_and_body() {
        let mut document = Document::parse(MODULE).unwrap();
        document.set("title", Value::from("同步")).unwrap();
        let output = document.to_text();
        assert!(
            output.starts_with("---\ntype: Module\nid: A-sync\nlayer: L1\nx-custom:"),
            "{output}"
        );
        assert!(output.contains("title: 同步\n"), "{output}");
        assert!(output.ends_with("---\n负责同步。\n"), "{output}");
        let reparsed = Document::parse(&output).unwrap();
        assert_eq!(reparsed.get("x-custom"), document.get("x-custom"));
        let keys: Vec<_> = reparsed.front().keys().filter_map(Value::as_str).collect();
        assert_eq!(keys, ["type", "id", "layer", "x-custom", "paths", "title"]);
    }

    #[test]
    fn non_string_type_is_rejected() {
        for value in ["42", "true", "null", "[]", "{}"] {
            let text = format!("---\ntype: {value}\n---\n");
            assert!(Document::parse(&text).is_err(), "{text}");
        }
    }

    #[test]
    fn whitespace_type_is_rejected() {
        assert!(Document::parse("---\ntype: '   '\n---\n").is_err());
    }

    #[test]
    fn invalid_yaml_is_rejected() {
        assert!(Document::parse("---\ntype: [\n---\n").is_err());
    }

    #[test]
    fn round_trip_without_trailing_newline_is_byte_identical() {
        for text in ["---\ntype: Module\n---", "---\ntype: Module\n---\n正文"] {
            let document = Document::parse(text).unwrap();
            assert_eq!(document.to_text(), text);
        }
    }

    #[test]
    fn edit_replaces_existing_key_in_place() {
        let mut document = Document::parse(MODULE).unwrap();
        document.set("id", Value::from("B-sync")).unwrap();
        let output = document.to_text();
        assert!(
            output.starts_with("---\ntype: Module\nid: B-sync\nlayer: L1\nx-custom:"),
            "{output}"
        );
        let reparsed = Document::parse(&output).unwrap();
        assert_eq!(reparsed.get_str("id"), Some("B-sync"));
        assert_eq!(reparsed.front().len(), 5);
        assert_eq!(reparsed.get("x-custom"), document.get("x-custom"));
        assert_eq!(reparsed.body(), "负责同步。\n");
    }

    #[test]
    fn edit_preserves_crlf_body() {
        let text = MODULE.replace('\n', "\r\n");
        let mut document = Document::parse(&text).unwrap();
        document.set("title", Value::from("同步")).unwrap();
        let output = document.to_text();
        assert!(output.ends_with("---\n负责同步。\r\n"), "{output}");
        assert_eq!(Document::parse(&output).unwrap().body(), "负责同步。\r\n");
    }

    #[test]
    fn edit_preserves_body_without_trailing_newline() {
        let mut document = Document::parse("---\ntype: Module\n---\n正文\n---\n结尾").unwrap();
        document.set("title", Value::from("同步")).unwrap();
        let output = document.to_text();
        assert_eq!(Document::parse(&output).unwrap().body(), "正文\n---\n结尾");
    }

    #[test]
    fn unserializable_set_preserves_front_and_original() {
        let text = MODULE.replace('\n', "\r\n");
        let mut document = Document::parse(&text).unwrap();
        let original_front = document.front().clone();
        let nested_tag = Value::Tagged(Box::new(TaggedValue {
            tag: Tag::new("outer"),
            value: Value::Tagged(Box::new(TaggedValue {
                tag: Tag::new("inner"),
                value: Value::from("payload"),
            })),
        }));
        assert!(serde_yaml::to_string(&nested_tag).is_err());
        for key in ["id", "new-field"] {
            let result = document.set(key, nested_tag.clone());
            assert!(
                result.is_err(),
                "不可序列化的值必须被拒绝，实际为 {result:?}"
            );
            assert_eq!(document.front(), &original_front);
            assert_eq!(document.original.as_deref(), Some(text.as_str()));
            assert_eq!(document.to_text().as_bytes(), text.as_bytes());
        }
    }

    #[test]
    fn invalid_type_set_preserves_front_and_original() {
        let text = MODULE.replace('\n', "\r\n");
        let mut document = Document::parse(&text).unwrap();
        let original_front = document.front().clone();
        for value in [
            Value::from("Bogus"),
            Value::from("module"),
            Value::from(" Module "),
            Value::from(""),
            Value::from(" "),
            Value::Null,
            Value::from(42),
            Value::from(true),
            Value::Sequence(Vec::new()),
            Value::Mapping(Mapping::new()),
        ] {
            assert!(document.set("type", value).is_err());
            assert_eq!(document.front(), &original_front);
            assert_eq!(document.original.as_deref(), Some(text.as_str()));
            assert_eq!(document.to_text().as_bytes(), text.as_bytes());
        }
    }
}
