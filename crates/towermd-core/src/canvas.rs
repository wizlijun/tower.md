use serde::de::{DeserializeSeed, IgnoredAny, MapAccess, SeqAccess, Visitor};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::fmt;

const MAX_SAFE_INTEGER: i64 = 9_007_199_254_740_991;

#[derive(Debug, Clone)]
pub struct CanvasEntry {
    pub index: usize,
    pub raw: Value,
    pub opaque: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct CanvasDiagnostic {
    pub code: &'static str,
    pub collection: Option<&'static str>,
    pub index: Option<usize>,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct CanvasDocument {
    original: String,
    raw: Option<Value>,
    nodes: Vec<CanvasEntry>,
    edges: Vec<CanvasEntry>,
    diagnostics: Vec<CanvasDiagnostic>,
}

impl CanvasDocument {
    pub fn from_value(value: Value) -> Result<Self, String> {
        let text =
            serde_json::to_string(&value).map_err(|error| format!("无法编码候选画布：{error}"))?;
        let document = Self::decode(&text)?;
        if !document.diagnostics.is_empty() {
            return Err(format!(
                "候选画布不合法：{}",
                document.diagnostics[0].message
            ));
        }
        allowed_keys(&value, &["nodes", "edges"])?;
        for entry in &document.nodes {
            let mut allowed = vec!["id", "type", "x", "y", "width", "height", "towermd"];
            allowed.extend_from_slice(optional_fields(&entry.raw, "nodes"));
            match entry.raw["type"].as_str() {
                Some("text") => allowed.push("text"),
                Some("file") => allowed.push("file"),
                Some("link") => allowed.push("url"),
                _ => {}
            }
            allowed_keys(&entry.raw, &allowed)?;
            validate_extension(&entry.raw)?;
        }
        for entry in &document.edges {
            let mut allowed = vec!["id", "fromNode", "toNode", "towermd"];
            allowed.extend_from_slice(optional_fields(&entry.raw, "edges"));
            allowed_keys(&entry.raw, &allowed)?;
            validate_extension(&entry.raw)?;
        }
        Ok(document)
    }

    pub fn raw(&self) -> Option<&Value> {
        self.raw.as_ref()
    }

    pub fn decode(text: &str) -> Result<Self, String> {
        let mut shape = serde_json::Deserializer::from_str(text);
        serde::Deserializer::deserialize_map(&mut shape, RootShape)
            .map_err(|error| format!("画布根或数组不是合法 JSON：{error}"))?;
        shape
            .end()
            .map_err(|error| format!("画布包含多余内容：{error}"))?;
        let mut diagnostics = Vec::new();
        let mut deserializer = serde_json::Deserializer::from_str(text);
        let audit = Audit {
            diagnostics: &mut diagnostics,
            path: Vec::new(),
        };
        if audit.deserialize(&mut deserializer).is_err() {
            diagnostics.push(CanvasDiagnostic {
                code: "unrepresentable_number",
                collection: None,
                index: None,
                message: "数字超出可安全建立投影的范围，仅保留原文".into(),
            });
        }
        if !diagnostics.is_empty() {
            return Ok(Self {
                original: text.to_owned(),
                raw: None,
                nodes: Vec::new(),
                edges: Vec::new(),
                diagnostics,
            });
        }
        let root: Value =
            serde_json::from_str(text).map_err(|error| format!("画布不是合法 JSON：{error}"))?;
        if !root.is_object() {
            return Err("画布根必须是对象".into());
        }
        let entries = |collection: &str| -> Result<Vec<CanvasEntry>, String> {
            match root.get(collection) {
                None => Ok(Vec::new()),
                Some(Value::Array(values)) => Ok(values
                    .iter()
                    .enumerate()
                    .map(|(index, raw)| CanvasEntry {
                        index,
                        raw: raw.clone(),
                        opaque: false,
                    })
                    .collect()),
                Some(_) => Err(format!("画布 {collection} 必须是数组")),
            }
        };
        let mut document = Self {
            original: text.to_owned(),
            nodes: entries("nodes")?,
            edges: entries("edges")?,
            raw: Some(root.clone()),
            diagnostics: Vec::new(),
        };
        diagnose_runtime_fields(&root, None, None, &mut document.diagnostics);
        validate_entries(&mut document.nodes, "nodes", &mut document.diagnostics);
        validate_entries(&mut document.edges, "edges", &mut document.diagnostics);
        let node_ids = id_counts(&document.nodes);
        for edge in &document.edges {
            if edge.opaque {
                continue;
            }
            for field in ["fromNode", "toNode"] {
                let endpoint = edge.raw[field].as_str().unwrap();
                let code = match node_ids.get(endpoint) {
                    None => "dangling_edge",
                    Some(1) => continue,
                    Some(_) => "ambiguous_endpoint",
                };
                document.diagnostics.push(diagnostic(
                    code,
                    "edges",
                    edge.index,
                    format!("端点 {field} `{endpoint}` 不存在或不唯一"),
                ));
            }
        }
        Ok(document)
    }

    pub fn encode(&self) -> String {
        self.original.clone()
    }

    pub fn nodes(&self) -> &[CanvasEntry] {
        &self.nodes
    }

    pub fn edges(&self) -> &[CanvasEntry] {
        &self.edges
    }

    pub fn diagnostics(&self) -> &[CanvasDiagnostic] {
        &self.diagnostics
    }
}

fn allowed_keys(value: &Value, allowed: &[&str]) -> Result<(), String> {
    let object = value.as_object().ok_or("候选画布字段必须是对象")?;
    for field in object.keys() {
        if !allowed.contains(&field.as_str()) {
            return Err(format!("候选画布不允许字段 `{field}`"));
        }
    }
    Ok(())
}

fn validate_extension(value: &Value) -> Result<(), String> {
    if let Some(extension) = value.get("towermd") {
        // FR-154: `container`, `template` and `card` mark a materialized frame.
        let fields = ["obj", "region", "container", "template", "card"];
        allowed_keys(extension, &fields)?;
        for field in fields {
            if extension.get(field).is_some_and(|value| !value.is_string()) {
                return Err(format!("towermd.{field} 必须是字符串"));
            }
        }
    }
    Ok(())
}

struct RootShape;

impl<'de> Visitor<'de> for RootShape {
    type Value = ();

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("含可选 nodes/edges 数组的画布对象")
    }

    fn visit_map<Access: MapAccess<'de>>(self, mut access: Access) -> Result<(), Access::Error> {
        while let Some(key) = access.next_key::<String>()? {
            if matches!(key.as_str(), "nodes" | "edges") {
                access.next_value::<Vec<IgnoredAny>>()?;
            } else {
                access.next_value::<IgnoredAny>()?;
            }
        }
        Ok(())
    }
}

fn diagnostic(
    code: &'static str,
    collection: &'static str,
    index: usize,
    message: String,
) -> CanvasDiagnostic {
    CanvasDiagnostic {
        code,
        collection: Some(collection),
        index: Some(index),
        message,
    }
}

fn id_counts(entries: &[CanvasEntry]) -> HashMap<&str, usize> {
    let mut counts = HashMap::new();
    for entry in entries {
        if let Some(id) = entry.raw.get("id").and_then(Value::as_str) {
            *counts.entry(id).or_default() += 1;
        }
    }
    counts
}

fn validate_entries(
    entries: &mut [CanvasEntry],
    collection: &'static str,
    diagnostics: &mut Vec<CanvasDiagnostic>,
) {
    for entry in entries.iter_mut() {
        diagnose_runtime_fields(&entry.raw, Some(collection), Some(entry.index), diagnostics);
        if let Err(message) = validate_required(&entry.raw, collection) {
            entry.opaque = true;
            diagnostics.push(diagnostic(
                "invalid_entry",
                collection,
                entry.index,
                message,
            ));
            continue;
        }
        for field in optional_fields(&entry.raw, collection) {
            let Some(value) = entry.raw.get(*field) else {
                continue;
            };
            let valid = value.as_str().is_some_and(|value| match *field {
                "fromSide" | "toSide" => ["top", "right", "bottom", "left"].contains(&value),
                "fromEnd" | "toEnd" => ["none", "arrow"].contains(&value),
                "backgroundStyle" => ["cover", "ratio", "repeat"].contains(&value),
                _ => true,
            });
            if !valid {
                diagnostics.push(diagnostic(
                    "invalid_optional_field",
                    collection,
                    entry.index,
                    format!("可选字段 `{field}` 非法，保留原值"),
                ));
            }
        }
    }
    let counts = id_counts(entries);
    for entry in entries.iter() {
        if let Some(id) = entry.raw.get("id").and_then(Value::as_str) {
            if counts[id] > 1 {
                diagnostics.push(diagnostic(
                    "duplicate_id",
                    collection,
                    entry.index,
                    format!("重复 ID `{id}`"),
                ));
            }
        }
    }
}

fn validate_required(raw: &Value, collection: &str) -> Result<(), String> {
    if !raw.is_object() {
        return Err("条目必须是对象".into());
    }
    let string = |field: &str| {
        raw.get(field)
            .and_then(Value::as_str)
            .ok_or_else(|| format!("必填字段 `{field}` 必须是字符串"))
    };
    string("id")?;
    if collection == "edges" {
        string("fromNode")?;
        string("toNode")?;
        return Ok(());
    }
    match string("type")? {
        "text" => {
            string("text")?;
        }
        "file" => {
            string("file")?;
        }
        "link" => {
            string("url")?;
        }
        "group" => {}
        other => return Err(format!("未知节点类型 `{other}`")),
    }
    for field in ["x", "y", "width", "height"] {
        let number = raw
            .get(field)
            .and_then(Value::as_i64)
            .filter(|number| (-MAX_SAFE_INTEGER..=MAX_SAFE_INTEGER).contains(number))
            .ok_or_else(|| format!("几何字段 `{field}` 必须是 JS 安全整数"))?;
        if matches!(field, "width" | "height") && number <= 0 {
            return Err(format!("几何字段 `{field}` 必须大于 0"));
        }
    }
    Ok(())
}

fn optional_fields(raw: &Value, collection: &str) -> &'static [&'static str] {
    if collection == "edges" {
        return &["color", "label", "fromSide", "toSide", "fromEnd", "toEnd"];
    }
    match raw["type"].as_str() {
        Some("file") => &["color", "subpath"],
        Some("group") => &["color", "label", "background", "backgroundStyle"],
        _ => &["color"],
    }
}

fn diagnose_runtime_fields(
    value: &Value,
    collection: Option<&'static str>,
    index: Option<usize>,
    diagnostics: &mut Vec<CanvasDiagnostic>,
) {
    const RUNTIME_FIELDS: &[&str] = &[
        "viewport",
        "selection",
        "selected",
        "hover",
        "dragging",
        "resizing",
        "measured",
        "parentId",
        "extent",
        "zIndex",
        "health",
        "healthColor",
        "cost",
        "badges",
        "taskGraph",
    ];
    for field in RUNTIME_FIELDS {
        if value.get(*field).is_some()
            || value
                .get("towermd")
                .and_then(|extension| extension.get(*field))
                .is_some()
        {
            diagnostics.push(CanvasDiagnostic {
                code: "runtime_field",
                collection,
                index,
                message: format!("运行时字段 `{field}` 仅保留原文，不参与画布投影"),
            });
        }
    }
}

struct Audit<'a> {
    diagnostics: &'a mut Vec<CanvasDiagnostic>,
    path: Vec<String>,
}

impl Audit<'_> {
    fn report(&mut self, code: &'static str, message: String) {
        let collection = match self.path.first().map(String::as_str) {
            Some("nodes") => Some("nodes"),
            Some("edges") => Some("edges"),
            _ => None,
        };
        let index = collection.and_then(|_| self.path.get(1).and_then(|part| part.parse().ok()));
        self.diagnostics.push(CanvasDiagnostic {
            code,
            collection,
            index,
            message,
        });
    }
}

impl<'de> DeserializeSeed<'de> for Audit<'_> {
    type Value = ();

    fn deserialize<Deserializer: serde::Deserializer<'de>>(
        self,
        deserializer: Deserializer,
    ) -> Result<(), Deserializer::Error> {
        deserializer.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for Audit<'_> {
    type Value = ();

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("JSON 值")
    }

    fn visit_map<Access: MapAccess<'de>>(
        mut self,
        mut access: Access,
    ) -> Result<(), Access::Error> {
        let mut keys = HashSet::new();
        while let Some(key) = access.next_key::<String>()? {
            if !keys.insert(key.clone()) {
                self.report(
                    "duplicate_property",
                    format!("重复 JSON 属性 `{key}`，仅保留原文"),
                );
            }
            let mut path = self.path.clone();
            path.push(key);
            access.next_value_seed(Audit {
                diagnostics: self.diagnostics,
                path,
            })?;
        }
        Ok(())
    }

    fn visit_seq<Access: SeqAccess<'de>>(self, mut access: Access) -> Result<(), Access::Error> {
        let mut index = 0;
        loop {
            let mut path = self.path.clone();
            path.push(index.to_string());
            if access
                .next_element_seed(Audit {
                    diagnostics: self.diagnostics,
                    path,
                })?
                .is_none()
            {
                return Ok(());
            }
            index += 1;
        }
    }

    fn visit_f64<Error: serde::de::Error>(self, _value: f64) -> Result<(), Error> {
        Ok(())
    }

    fn visit_i64<Error: serde::de::Error>(self, _value: i64) -> Result<(), Error> {
        Ok(())
    }

    fn visit_u64<Error: serde::de::Error>(self, _value: u64) -> Result<(), Error> {
        Ok(())
    }

    fn visit_str<Error: serde::de::Error>(self, _value: &str) -> Result<(), Error> {
        Ok(())
    }
    fn visit_bool<Error: serde::de::Error>(self, _value: bool) -> Result<(), Error> {
        Ok(())
    }
    fn visit_unit<Error: serde::de::Error>(self) -> Result<(), Error> {
        Ok(())
    }
}
