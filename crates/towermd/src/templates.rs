//! Templates a person can place on the canvas (core spec FR-142, FR-143).
//!
//! The starter templates ship with `towermd` as first-party extension
//! manifests; they are not part of the core. A `type: template` extension
//! installed on the authoritative main is listed too, and one that reuses a
//! starter's ID takes its place: the repository wins over what ships.
use serde_json::{json, Value};
use towermd_core::{extensions::Registry, frames::{Template, TEMPLATE_KIND}};

const STARTERS: [(&str, &str); 5] = [
    ("kanban", include_str!("../templates/kanban/towermd-plugin.yaml")),
    ("quadrant", include_str!("../templates/quadrant/towermd-plugin.yaml")),
    ("timeline", include_str!("../templates/timeline/towermd-plugin.yaml")),
    ("mindmap", include_str!("../templates/mindmap/towermd-plugin.yaml")),
    // Appendix A.2: a text-source template over a repository .puml file.
    ("plantuml", include_str!("../templates/plantuml/towermd-plugin.yaml")),
];

pub(crate) const STARTER: &str = "starter";
pub(crate) const INSTALLED: &str = "installed";

pub(crate) struct Entry {
    pub id: String,
    pub origin: &'static str,
    pub path: String,
    /// The manifest as written; frozen into a frame on creation.
    pub definition: Value,
    pub template: Result<Template, Vec<String>>,
}

impl Entry {
    /// What `template ls` and the App's template panel show.
    pub fn summary(&self) -> Value {
        let definition = &self.definition;
        let mut summary = json!({
            "id": self.id, "origin": self.origin, "path": self.path,
            "version": definition.get("version"), "title": definition.get("title"),
        });
        match &self.template {
            Ok(template) => {
                summary["layout"] = json!(template.layout);
                summary["states"] = json!(template.states);
                summary["done_states"] = json!(template.done_states);
                summary["card_fields"] = json!(template.card_fields);
                summary["limits"] = json!(template.limits);
                summary["default_set_by"] = json!(template.default_set_by);
                // FR-173: the rules text a frame starts with.
                if !template.agent_rules.is_empty() {
                    summary["agent_rules"] = json!(template.agent_rules);
                }
                if !template.views.is_empty() {
                    summary["views"] = json!(template.views);
                }
                // FR-163: a text-source template asks for a file of these types.
                if definition.get("source").and_then(Value::as_str) == Some("file") {
                    summary["file_types"] = definition.get("file_types").cloned().unwrap_or(json!([]));
                }
                summary["problems"] = json!([]);
            }
            Err(problems) => summary["problems"] = json!(problems),
        }
        summary
    }
}

/// Starter templates first, in their fixed order, then installed ones by ID.
pub(crate) fn available(registry: &Registry) -> Vec<Entry> {
    let installed: Vec<Entry> = registry
        .extensions
        .iter()
        .filter(|manifest| manifest.kind.as_deref() == Some(TEMPLATE_KIND))
        .map(|manifest| {
            let template = if manifest.problems.is_empty() {
                Template::parse(&manifest.manifest)
            } else {
                Err(manifest.problems.clone())
            };
            Entry { id: manifest.id.clone(), origin: INSTALLED, path: manifest.path.clone(), definition: manifest.manifest.clone(), template }
        })
        .collect();
    let mut entries: Vec<Entry> = STARTERS
        .iter()
        .filter(|(id, _)| !installed.iter().any(|entry| entry.id == *id))
        .map(|(id, text)| {
            let (definition, template) = Template::from_yaml(text);
            Entry { id: (*id).to_owned(), origin: STARTER, path: format!("starter:{id}"), definition, template }
        })
        .collect();
    entries.extend(installed);
    entries
}

pub(crate) fn list(registry: &Registry) -> Value {
    let templates: Vec<Value> = available(registry).iter().map(Entry::summary).collect();
    json!({"revision": registry.revision, "templates": templates})
}

#[cfg(test)]
mod tests {
    use super::*;
    use towermd_core::frames::{Axis, SetBy, Structure};

    #[test]
    fn every_starter_template_reads_cleanly() {
        let entries = available(&Registry::default());
        let ids: Vec<&str> = entries.iter().map(|entry| entry.id.as_str()).collect();
        assert_eq!(ids, ["kanban", "quadrant", "timeline", "mindmap", "plantuml"]);
        for entry in &entries {
            let template = entry.template.as_ref().unwrap_or_else(|problems| panic!("{}: {problems:?}", entry.id));
            assert_eq!(template.id, entry.id);
            assert_eq!(entry.origin, STARTER);
        }
        let kanban = entries[0].template.as_ref().unwrap();
        assert!(matches!(&kanban.layout.x, Axis::Category { field, values } if field == "state" && values.len() == 4));
        assert_eq!(kanban.default_set_by, SetBy::Delegate);
        let quadrant = entries[1].template.as_ref().unwrap();
        assert!(matches!(&quadrant.layout.y, Axis::Category { values, .. } if values.len() == 2));
        assert_eq!(entries[3].template.as_ref().unwrap().layout.structure, vec![Structure::Tree]);
        let plantuml = &entries[4];
        let spec = towermd_core::source_frames::Spec::read(&plantuml.definition, &plantuml.template.as_ref().unwrap().states).unwrap().unwrap();
        assert_eq!((spec.parser.as_str(), spec.token("Done")), (towermd_core::plantuml::PARSER, Some("<<done>>")));
        // FR-173: the PlantUML template ships rules for whoever updates it.
        let template = plantuml.template.as_ref().unwrap();
        for part in ["别名", "<<why>>", "skinparam", "本次进展"] {
            assert!(template.agent_rules.contains(part), "{part}");
        }
        assert_eq!(template.rules(Default::default()).unwrap().text, template.agent_rules);
        assert_eq!(plantuml.summary()["agent_rules"], json!(template.agent_rules));
    }

    #[test]
    fn an_installed_template_replaces_the_starter_and_a_broken_one_lists_its_problems() {
        let path = |id: &str| format!(".towermd/extensions/{id}/towermd-plugin.yaml");
        let registry = Registry::from_files(Some("abc".into()), vec![
            (path("kanban"), Some("id: kanban\nversion: 2.0.0\ntype: template\nlayout: { x: { order: added } }\n".into())),
            (path("broken"), Some("id: broken\nversion: 1.0.0\ntype: template\nlayout: { x: { category: state } }\n".into())),
            (path("lint"), Some("id: lint\nversion: 1.0.0\ntype: check\n".into())),
        ]);
        let listed = list(&registry);
        let ids: Vec<&str> = listed["templates"].as_array().unwrap().iter().map(|entry| entry["id"].as_str().unwrap()).collect();
        assert_eq!(ids, ["quadrant", "timeline", "mindmap", "plantuml", "broken", "kanban"]);
        let kanban = &listed["templates"][5];
        assert_eq!((kanban["origin"].as_str(), kanban["version"].as_str()), (Some(INSTALLED), Some("2.0.0")));
        assert_eq!(listed["templates"][4]["problems"][0], "按状态分栏的模板须声明 states");
        assert!(listed["templates"][4].get("layout").is_none());
    }
}
