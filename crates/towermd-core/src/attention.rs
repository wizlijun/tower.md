//! Attention budget and interruption timing settings (core spec FR-40,
//! FR-128, FR-130), read leniently from `attention:` in `.towermd/config.yaml`.
//! The three timings themselves (now, next visit, summary) are derived from
//! each item's level wherever the list is shown; nothing here is stored.
use serde::Serialize;
use serde_yaml::Value;

/// FR-40: the first screen never shows more than this many items.
pub const FIRST_SCREEN: usize = 3;

/// A local-time window in minutes after midnight; `end < start` crosses midnight.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Window {
    pub start: u16,
    pub end: u16,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Settings {
    /// First-screen items, 1 to `FIRST_SCREEN`; the rest wait under "later".
    pub budget: usize,
    /// FR-130 focus time: inside a window only `time_sensitive` interrupts.
    pub focus_time: Vec<Window>,
}

impl Default for Settings {
    fn default() -> Self {
        Self { budget: FIRST_SCREEN, focus_time: Vec::new() }
    }
}

fn minutes(text: &str) -> Option<u16> {
    let (hours, mins) = text.trim().split_once(':')?;
    let (hours, mins): (u16, u16) = (hours.parse().ok()?, mins.parse().ok()?);
    (hours < 24 && mins < 60 && text.trim().len() == 5).then_some(hours * 60 + mins)
}

/// `"HH:MM-HH:MM"`; an empty window (same start and end) means nothing.
pub fn window(text: &str) -> Option<Window> {
    let (start, end) = text.split_once('-')?;
    let window = Window { start: minutes(start)?, end: minutes(end)? };
    (window.start != window.end).then_some(window)
}

/// A missing, non-mapping or malformed section keeps the defaults; a budget
/// above three is held at three so the first screen never grows (FR-40), and
/// unreadable windows are skipped rather than widening anything.
pub fn settings(section: Option<&Value>) -> Settings {
    let defaults = Settings::default();
    let Some(map) = section.and_then(Value::as_mapping) else { return defaults };
    let budget = map.get("budget").and_then(Value::as_u64).filter(|budget| *budget > 0)
        .map_or(defaults.budget, |budget| budget.min(FIRST_SCREEN as u64) as usize);
    let focus_time = map.get("focus_time").and_then(Value::as_sequence).map(|items| {
        items.iter().filter_map(Value::as_str).filter_map(window).collect()
    }).unwrap_or_default();
    Settings { budget, focus_time }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(text: &str) -> Settings {
        settings(serde_yaml::from_str::<Value>(text).unwrap().get("attention"))
    }

    #[test]
    fn missing_or_malformed_sections_keep_the_defaults() {
        assert_eq!(settings(None), Settings { budget: 3, focus_time: vec![] });
        assert_eq!(read("attention: [1]\n"), Settings::default());
        assert_eq!(read("attention:\n  budget: 0\n  focus_time: x\n"), Settings::default());
        assert_eq!(read("attention:\n  budget: -1\n"), Settings::default());
    }

    #[test]
    fn the_budget_never_grows_the_first_screen() {
        assert_eq!(read("attention:\n  budget: 1\n").budget, 1);
        assert_eq!(read("attention:\n  budget: 2\n").budget, 2);
        assert_eq!(read("attention:\n  budget: 9\n").budget, 3);
    }

    #[test]
    fn focus_windows_read_local_times_and_skip_bad_ones() {
        let focus = read("attention:\n  focus_time:\n    - \"22:00-08:00\"\n    - \"13:30-14:00\"\n    - \"9:00-10:00\"\n    - \"25:00-26:00\"\n    - \"10:00-10:00\"\n    - 7\n").focus_time;
        assert_eq!(focus, vec![Window { start: 1320, end: 480 }, Window { start: 810, end: 840 }]);
        assert_eq!(window(" 08:00 - 09:15 "), Some(Window { start: 480, end: 555 }));
        assert_eq!(window("8:00-09:15"), None, "HH:MM on both sides");
    }
}
