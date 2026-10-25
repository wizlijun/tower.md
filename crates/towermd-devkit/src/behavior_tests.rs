//! G3's optional check that a behavior change carries tests (development
//! kit spec 11.1). Off unless a person declares it in main's
//! `.towermd/gates.yaml`, which is an L3 change (FR-55):
//!
//! ```yaml
//! behavior_tests:
//!   code: [src/, lib/]                         # a change here is a behavior change
//!   tests: [tests/, "*_test.go", "*.test.ts"]  # test paths, or `*<suffix>` file names
//! ```
//!
//! `code` and `tests` take the path syntax of `allowed_paths` (a literal
//! file, `dir/` or `dir/**`); a `tests` entry starting with `*` instead
//! matches the end of a file name anywhere. A finish whose task adds or
//! modifies a non-test file under `code` fails G3 unless the task also adds
//! or modifies a test. Deleting code alone needs no test. An unreadable
//! declaration fails closed, like the rest of `gates.yaml`.
use serde_yaml::Value as Yaml;
use towermd_core::path_guard::{path_is_allowed, PathRule};

pub const KEY: &str = "behavior_tests";
/// How a G3 stop by this check starts.
pub const UNTESTED: &str = "G3 行为改动缺测试";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rule {
    code: Vec<String>,
    tests: Vec<String>,
}

fn list(entry: &serde_yaml::Mapping, key: &str) -> Result<Vec<String>, String> {
    let broken = || format!("gates.yaml 的 {KEY}.{key} 须是非空的路径列表");
    let Some(Yaml::Sequence(items)) = entry.get(key) else { return Err(broken()) };
    let values = items.iter().map(|item| item.as_str().map(str::trim).filter(|text| !text.is_empty()).map(str::to_owned))
        .collect::<Option<Vec<_>>>().ok_or_else(broken)?;
    if values.is_empty() { return Err(broken()); }
    Ok(values)
}

impl Rule {
    /// The declaration in the text of `gates.yaml`; `None` when it is absent
    /// or the file is missing, which is the default.
    pub fn from_gates(text: Option<&str>) -> Result<Option<Self>, String> {
        let Some(text) = text else { return Ok(None) };
        let file: Yaml = serde_yaml::from_str(text).map_err(|error| format!("gates.yaml 不是合法 YAML：{error}"))?;
        let entry = match file.get(KEY) {
            None | Some(Yaml::Null) => return Ok(None),
            Some(Yaml::Mapping(entry)) => entry,
            Some(_) => return Err(format!("gates.yaml 的 {KEY} 须是含 code 与 tests 的映射")),
        };
        let rule = Self { code: list(entry, "code")?, tests: list(entry, "tests")? };
        for path in rule.code.iter().chain(rule.tests.iter().filter(|test| !test.starts_with('*'))) {
            PathRule::parse(path).map_err(|error| format!("gates.yaml 的 {KEY}：{error}"))?;
        }
        if rule.tests.iter().filter_map(|test| test.strip_prefix('*')).any(|suffix| suffix.is_empty() || suffix.contains(['*', '/'])) {
            return Err(format!("gates.yaml 的 {KEY}.tests 中以 * 开头的项只能匹配文件名结尾，如 \"*_test.go\""));
        }
        Ok(Some(rule))
    }

    fn is_test(&self, path: &str) -> bool {
        let name = path.rsplit('/').next().unwrap_or(path);
        self.tests.iter().any(|test| match test.strip_prefix('*') {
            Some(suffix) => name.ends_with(suffix),
            None => path_is_allowed(path, std::slice::from_ref(test)).unwrap_or(false),
        })
    }

    /// `changed`: the files the task adds or modifies, net of its commits.
    pub fn check(&self, changed: &[String]) -> Result<(), String> {
        if changed.iter().any(|path| self.is_test(path)) { return Ok(()); }
        let behavior: Vec<&str> = changed.iter().map(String::as_str)
            .filter(|path| path_is_allowed(path, &self.code).unwrap_or(false)).collect();
        match behavior.as_slice() {
            [] => Ok(()),
            [first, rest @ ..] => Err(format!("{UNTESTED}：{first}{} 改了行为，却没有新增或修改测试（{KEY}.tests：{}）；补上测试再收尾，确属无需测试的改动请找人调整 gates.yaml",
                if rest.is_empty() { String::new() } else { format!(" 等 {} 个文件", behavior.len()) }, self.tests.join("、"))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(text: &str) -> Rule {
        Rule::from_gates(Some(text)).unwrap().unwrap()
    }
    fn paths(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| (*item).to_owned()).collect()
    }

    #[test]
    fn off_unless_declared() {
        assert_eq!(Rule::from_gates(None), Ok(None));
        assert_eq!(Rule::from_gates(Some("gates: {}\n")), Ok(None));
        assert_eq!(Rule::from_gates(Some("behavior_tests:\n")), Ok(None));
    }

    #[test]
    fn a_broken_declaration_fails_closed() {
        for broken in ["behavior_tests: yes", "behavior_tests: {code: [src/]}", "behavior_tests: {code: [], tests: [t/]}",
            "behavior_tests: {code: [/etc], tests: [t/]}", "behavior_tests: {code: [src/], tests: [\"src/*.rs\"]}",
            "behavior_tests: {code: [src/], tests: [\"*\"]}", "behavior_tests: {code: [src/], tests: [\"*a/b\"]}",
            "behavior_tests: {code: [src/], tests: [1]}", "behavior_tests: ["] {
            assert!(Rule::from_gates(Some(broken)).is_err(), "{broken}");
        }
    }

    #[test]
    fn code_changes_need_a_changed_test() {
        let rule = rule("behavior_tests:\n  code: [src/, lib/**]\n  tests: [tests/, \"*_tests.rs\"]\n");
        let error = rule.check(&paths(&["src/app.rs", "lib/x.rs", "README.md"])).unwrap_err();
        assert!(error.starts_with(UNTESTED) && error.contains("src/app.rs 等 2 个文件"), "{error}");
        assert!(rule.check(&paths(&["src/app.rs", "tests/app.rs"])).is_ok());
        assert!(rule.check(&paths(&["src/app.rs", "src/app_tests.rs"])).is_ok());
        // Not a behavior change: outside `code`, or only tests.
        assert!(rule.check(&paths(&["README.md", "docs/a.md"])).is_ok());
        assert!(rule.check(&paths(&["src/app_tests.rs"])).is_ok());
        assert!(rule.check(&[]).is_ok());
    }
}
