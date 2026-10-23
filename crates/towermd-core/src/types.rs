use serde::{Deserialize, Serialize};

use crate::okf::Document;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Layer {
    L0,
    L1,
    L2,
    L3,
    L4,
    L5,
}

impl Layer {
    pub fn parse(value: &str) -> Option<Self> {
        Some(match value.trim() {
            "L0" => Self::L0,
            "L1" => Self::L1,
            "L2" => Self::L2,
            "L3" => Self::L3,
            "L4" => Self::L4,
            "L5" => Self::L5,
            _ => return None,
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::L0 => "L0",
            Self::L1 => "L1",
            Self::L2 => "L2",
            Self::L3 => "L3",
            Self::L4 => "L4",
            Self::L5 => "L5",
        }
    }

    pub fn is_slow(self) -> bool {
        self <= Self::L2
    }

    /// FR-188: L0–L3 are set by people, L4–L5 are moved by delegates. This
    /// is what people see; protection stays with `is_slow` (L0–L2).
    pub fn set_by(self) -> SetBy {
        if self <= Self::L3 {
            SetBy::Human
        } else {
            SetBy::Delegate
        }
    }
}

/// Who set an item: the only ownership shown to people (FR-188). `layer`
/// stays internal (FR-01).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SetBy {
    Human,
    Delegate,
}

impl SetBy {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Human => "human",
            Self::Delegate => "delegate",
        }
    }
}

#[derive(Debug)]
pub struct TypeInfo {
    pub name: &'static str,
    pub default_layer: Layer,
}

pub const TYPES: &[TypeInfo] = &[
    TypeInfo {
        name: "Intent",
        default_layer: Layer::L0,
    },
    TypeInfo {
        name: "Module",
        default_layer: Layer::L1,
    },
    TypeInfo {
        name: "Interface",
        default_layer: Layer::L1,
    },
    TypeInfo {
        name: "Acceptance",
        default_layer: Layer::L2,
    },
    TypeInfo {
        name: "Baseline",
        default_layer: Layer::L1,
    },
    TypeInfo {
        name: "Decision",
        default_layer: Layer::L1,
    },
    TypeInfo {
        name: "Proposal",
        default_layer: Layer::L4,
    },
    TypeInfo {
        name: "Task",
        default_layer: Layer::L4,
    },
    TypeInfo {
        name: "Run",
        default_layer: Layer::L5,
    },
    TypeInfo {
        name: "Evidence",
        default_layer: Layer::L5,
    },
    TypeInfo {
        name: "Hold",
        default_layer: Layer::L5,
    },
    TypeInfo {
        name: "Gate",
        default_layer: Layer::L3,
    },
    TypeInfo {
        name: "SkillPack",
        default_layer: Layer::L3,
    },
    TypeInfo {
        name: "Rule",
        default_layer: Layer::L3,
    },
];

pub fn lookup(name: &str) -> Option<&'static TypeInfo> {
    TYPES.iter().find(|entry| entry.name == name)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayerResolution {
    Valid(Layer),
    Missing(Option<Layer>),
    Invalid,
}

impl LayerResolution {
    pub fn is_protected(self) -> bool {
        match self {
            Self::Valid(layer) | Self::Missing(Some(layer)) => layer.is_slow(),
            Self::Missing(None) => false,
            Self::Invalid => true,
        }
    }

    /// An invalid layer is protected, so it shows as set by people.
    pub fn set_by(self) -> SetBy {
        match self {
            Self::Valid(layer) | Self::Missing(Some(layer)) => layer.set_by(),
            Self::Missing(None) => SetBy::Delegate,
            Self::Invalid => SetBy::Human,
        }
    }
}

pub fn layer_of(document: &Document) -> LayerResolution {
    if document.get("layer").is_some() {
        return document
            .get_str("layer")
            .and_then(Layer::parse)
            .map(LayerResolution::Valid)
            .unwrap_or(LayerResolution::Invalid);
    }
    LayerResolution::Missing(
        document
            .doc_type()
            .and_then(lookup)
            .map(|entry| entry.default_layer),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(text: &str) -> Document {
        Document::parse(text).unwrap()
    }

    #[test]
    fn registry_matches_spec_table() {
        let names: Vec<&str> = TYPES.iter().map(|entry| entry.name).collect();
        assert_eq!(
            names,
            [
                "Intent",
                "Module",
                "Interface",
                "Acceptance",
                "Baseline",
                "Decision",
                "Proposal",
                "Task",
                "Run",
                "Evidence",
                "Hold",
                "Gate",
                "SkillPack",
                "Rule",
            ]
        );
        let layers: Vec<Layer> = TYPES.iter().map(|entry| entry.default_layer).collect();
        assert_eq!(
            layers,
            [
                Layer::L0,
                Layer::L1,
                Layer::L1,
                Layer::L2,
                Layer::L1,
                Layer::L1,
                Layer::L4,
                Layer::L4,
                Layer::L5,
                Layer::L5,
                Layer::L5,
                Layer::L3,
                Layer::L3,
                Layer::L3,
            ]
        );
    }

    #[test]
    fn lookup_known_and_unknown() {
        assert_eq!(lookup("Module").unwrap().default_layer, Layer::L1);
        assert_eq!(lookup("Acceptance").unwrap().default_layer, Layer::L2);
        assert!(lookup("module").is_none());
        assert!(lookup("FutureThing").is_none());
    }

    #[test]
    fn layer_field_wins_over_default() {
        let document = doc("---\ntype: Decision\nlayer: L4\n---\n");
        assert_eq!(layer_of(&document), LayerResolution::Valid(Layer::L4));
    }

    #[test]
    fn missing_layer_uses_type_default_but_explicit_invalid_is_protected() {
        assert_eq!(
            layer_of(&doc("---\ntype: Decision\n---\n")),
            LayerResolution::Missing(Some(Layer::L1))
        );
        assert_eq!(
            layer_of(&doc("---\ntype: Task\n---\n")),
            LayerResolution::Missing(Some(Layer::L4))
        );
        for kind in ["Task", "Module", "FutureThing"] {
            for value in ["L9", "null", "42", "true", "[]", "{}", "''", "'  '"] {
                let resolution =
                    layer_of(&doc(&format!("---\ntype: {kind}\nlayer: {value}\n---\n")));
                assert_eq!(resolution, LayerResolution::Invalid, "{kind}: {value}");
                assert!(resolution.is_protected());
            }
        }
        for resolution in [
            LayerResolution::Valid(Layer::L1),
            LayerResolution::Missing(Some(Layer::L2)),
        ] {
            assert!(resolution.is_protected());
        }
        for resolution in [
            LayerResolution::Valid(Layer::L4),
            LayerResolution::Missing(Some(Layer::L5)),
            LayerResolution::Missing(None),
        ] {
            assert!(!resolution.is_protected());
        }
    }

    #[test]
    fn unknown_type_without_layer_has_no_layer() {
        assert_eq!(
            layer_of(&doc("---\ntype: FutureThing\n---\n")),
            LayerResolution::Missing(None)
        );
        assert_eq!(
            layer_of(&doc("---\ntype: FutureThing\nlayer: L2\n---\n")),
            LayerResolution::Valid(Layer::L2)
        );
        assert_eq!(
            layer_of(&doc("---\ntype: FutureThing\nlayer: L9\n---\n")),
            LayerResolution::Invalid
        );
    }

    #[test]
    fn slow_layers_are_l0_to_l2() {
        for layer in [Layer::L0, Layer::L1, Layer::L2] {
            assert!(layer.is_slow());
        }
        for layer in [Layer::L3, Layer::L4, Layer::L5] {
            assert!(!layer.is_slow());
        }
    }

    #[test]
    fn set_by_is_human_for_l0_to_l3_without_widening_protection() {
        for layer in [Layer::L0, Layer::L1, Layer::L2, Layer::L3] {
            assert_eq!(layer.set_by(), SetBy::Human);
        }
        for layer in [Layer::L4, Layer::L5] {
            assert_eq!(layer.set_by(), SetBy::Delegate);
        }
        assert!(!Layer::L3.is_slow());
        assert_eq!(layer_of(&doc("---\ntype: Rule\n---\n")).set_by(), SetBy::Human);
        assert_eq!(layer_of(&doc("---\ntype: Decision\nlayer: L5\n---\n")).set_by(), SetBy::Delegate);
        assert_eq!(layer_of(&doc("---\ntype: Task\nlayer: L9\n---\n")).set_by(), SetBy::Human);
        assert_eq!(layer_of(&doc("---\ntype: FutureThing\n---\n")).set_by(), SetBy::Delegate);
        assert_eq!(serde_json::to_value(SetBy::Delegate).unwrap(), "delegate");
        assert_eq!((SetBy::Human.as_str(), SetBy::Delegate.as_str()), ("human", "delegate"));
    }

    #[test]
    fn layer_names_parse_and_round_trip() {
        for (name, layer) in [
            ("L0", Layer::L0),
            ("L1", Layer::L1),
            ("L2", Layer::L2),
            ("L3", Layer::L3),
            ("L4", Layer::L4),
            ("L5", Layer::L5),
        ] {
            assert_eq!(Layer::parse(name), Some(layer));
            assert_eq!(Layer::parse(&format!(" {name}\n")), Some(layer));
            assert_eq!(layer.as_str(), name);
        }
        for name in ["", "l1", "L6", "L9"] {
            assert_eq!(Layer::parse(name), None);
        }
    }

    #[test]
    fn writes_require_registered_type() {
        for entry in TYPES {
            assert!(Document::new(entry.name, "正文\n").is_ok());
        }
        for name in ["FutureThing", "module", "", " ", " Module "] {
            assert!(Document::new(name, "").is_err());
        }
        let mut document = doc("---\ntype: FutureThing\n---\n");
        assert!(
            document.set("title", "x".into()).is_ok(),
            "未知 type 的对象仍可改其他字段"
        );
        assert_eq!(
            Document::parse(&document.to_text()).unwrap().doc_type(),
            Some("FutureThing")
        );
        assert!(document.set("type", "Bogus".into()).is_err());
        for entry in TYPES {
            assert!(document.set("type", entry.name.into()).is_ok());
            assert_eq!(document.doc_type(), Some(entry.name));
        }
    }

    #[test]
    fn new_document_renders_minimal_frontmatter() {
        let document = Document::new("Intent", "一句话。\n").unwrap();
        assert_eq!(document.to_text(), "---\ntype: Intent\n---\n一句话。\n");
    }
}
