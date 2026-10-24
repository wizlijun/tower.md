//! The command line's own memory of which projects this user looked at, kept
//! in `<runtime>/views.json` as `{project_id: {root, last_view}}`. Derived and
//! disposable: deleting it only forgets `--project <ID>` names and the
//! "since your last look" mark, which falls back to the last seven days.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct View {
    pub root: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_view: Option<u64>,
}

pub type Views = BTreeMap<String, View>;

fn path() -> Option<PathBuf> {
    crate::daemon::views_path().ok()
}

pub fn load() -> Views {
    path().map(|path| load_from(&path)).unwrap_or_default()
}

fn load_from(path: &Path) -> Views {
    std::fs::read(path).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default()
}

/// Written only on change, through a temporary file and a rename, so a
/// concurrent reader never sees half a file. Failures are ignored: this file
/// is a convenience, never state.
fn save(path: &Path, views: &Views) {
    let Ok(bytes) = serde_json::to_vec_pretty(views) else { return };
    let temporary = path.with_extension(format!("json.{}.tmp", std::process::id()));
    if std::fs::write(&temporary, bytes).is_ok() && std::fs::rename(&temporary, path).is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
}

fn update(change: impl FnOnce(&mut Views)) {
    let Some(path) = path() else { return };
    let before = load_from(&path);
    let mut after = before.clone();
    change(&mut after);
    if after != before {
        save(&path, &after);
    }
}

/// The root last seen for this project ID.
pub fn root(project: &str) -> Option<PathBuf> {
    load().remove(project).map(|view| view.root)
}

pub fn last_view(project: &str) -> Option<u64> {
    load().get(project).and_then(|view| view.last_view)
}

pub fn remember(project: &str, root: &Path) {
    update(|views| {
        let view = views.entry(project.to_owned()).or_default();
        view.root = root.to_owned();
    });
}

pub fn mark_viewed(project: &str, root: &Path, at: u64) {
    update(|views| {
        views.insert(project.to_owned(), View { root: root.to_owned(), last_view: Some(at) });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unreadable_views_are_empty_and_rewrite_cleanly() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("views.json");
        std::fs::write(&path, b"{not json").unwrap();
        assert!(load_from(&path).is_empty());
        let mut views = Views::new();
        views.insert("p".into(), View { root: "/r".into(), last_view: Some(7) });
        save(&path, &views);
        assert_eq!(load_from(&path), views);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1, "no temporary file is left behind");
    }
}
