//! Language of CLI-local text. Order: --lang, TOWERMD_LANG, LC_ALL, LANG,
//! macOS AppleLanguages, then English. Daemon messages are not translated.
use std::{ffi::OsString, sync::OnceLock};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lang {
    En,
    ZhCn,
}

static CURRENT: OnceLock<Lang> = OnceLock::new();

pub fn current() -> Lang {
    *CURRENT.get().unwrap_or(&Lang::En)
}

pub fn pick<T>(en: T, zh: T) -> T {
    pick_in(current(), en, zh)
}

pub fn pick_in<T>(lang: Lang, en: T, zh: T) -> T {
    match lang {
        Lang::En => en,
        Lang::ZhCn => zh,
    }
}

/// `zh_CN.UTF-8`, `zh-Hans-CN`, `zh` → Chinese; any other real tag → English.
/// Empty values and the POSIX defaults `C`/`POSIX` decide nothing.
pub fn from_tag(tag: &str) -> Option<Lang> {
    let base = tag.split(['.', '@']).next().unwrap_or("").trim().replace('_', "-").to_ascii_lowercase();
    if base.is_empty() || base == "c" || base == "posix" {
        return None;
    }
    let chinese = base == "zh" || ["zh-cn", "zh-sg", "zh-hans"].iter().any(|prefix| base.starts_with(prefix));
    Some(if chinese { Lang::ZhCn } else { Lang::En })
}

pub fn resolve(flag: Option<&str>, env: impl Fn(&str) -> Option<String>, apple: impl FnOnce() -> Option<String>) -> Lang {
    flag.and_then(from_tag)
        .or_else(|| ["TOWERMD_LANG", "LC_ALL", "LANG"].into_iter().find_map(|key| env(key).as_deref().and_then(from_tag)))
        .or_else(|| apple().as_deref().and_then(from_tag))
        .unwrap_or(Lang::En)
}

/// First entry of `defaults read -g AppleLanguages`, e.g. `(\n    "zh-Hans-CN",\n    en\n)`.
pub fn first_apple_language(output: &str) -> Option<String> {
    output
        .lines()
        .map(|line| line.trim().trim_end_matches(',').trim_matches('"'))
        .find(|line| !line.is_empty() && *line != "(" && *line != ")")
        .map(str::to_owned)
}

#[cfg(target_os = "macos")]
fn apple_languages() -> Option<String> {
    let output = std::process::Command::new("/usr/bin/defaults")
        .args(["read", "-g", "AppleLanguages"])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    first_apple_language(&String::from_utf8_lossy(&output.stdout))
}

#[cfg(not(target_os = "macos"))]
fn apple_languages() -> Option<String> {
    None
}

/// `--lang` must be honoured even when clap rejects the rest of the line.
pub fn flag_from_args(args: &[OsString]) -> Option<String> {
    let mut iter = args.iter().skip(1).map(|arg| arg.to_string_lossy());
    while let Some(arg) = iter.next() {
        if arg == "--" {
            break;
        }
        if arg == "--lang" {
            return iter.next().map(|value| value.into_owned());
        }
        if let Some(value) = arg.strip_prefix("--lang=") {
            return Some(value.to_owned());
        }
    }
    None
}

pub fn init(args: &[OsString]) -> Lang {
    let lang = resolve(flag_from_args(args).as_deref(), |key| std::env::var(key).ok(), apple_languages);
    let _ = CURRENT.set(lang);
    lang
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(pairs: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<String> {
        move |key| pairs.iter().find(|(k, _)| *k == key).map(|(_, v)| (*v).to_owned())
    }

    #[test]
    fn tags_map_to_languages() {
        for (tag, lang) in [
            ("zh_CN.UTF-8", Some(Lang::ZhCn)),
            ("zh-Hans-CN", Some(Lang::ZhCn)),
            ("zh", Some(Lang::ZhCn)),
            ("zh_SG", Some(Lang::ZhCn)),
            ("zh-CN", Some(Lang::ZhCn)),
            ("zh_TW.UTF-8", Some(Lang::En)),
            ("en_US.UTF-8", Some(Lang::En)),
            ("fr", Some(Lang::En)),
            ("", None),
            ("C", None),
            ("POSIX", None),
            ("C.UTF-8", None),
        ] {
            assert_eq!(from_tag(tag), lang, "{tag}");
        }
    }

    #[test]
    fn resolution_follows_flag_then_environment_then_apple() {
        let none = || None;
        assert_eq!(resolve(Some("en"), env(&[("TOWERMD_LANG", "zh-CN")]), none), Lang::En);
        assert_eq!(resolve(None, env(&[("TOWERMD_LANG", "zh-CN"), ("LC_ALL", "en_US.UTF-8")]), none), Lang::ZhCn);
        assert_eq!(resolve(None, env(&[("LC_ALL", "en_US.UTF-8"), ("LANG", "zh_CN.UTF-8")]), none), Lang::En);
        assert_eq!(resolve(None, env(&[("LANG", "zh_CN.UTF-8")]), || Some("en".into())), Lang::ZhCn);
        assert_eq!(resolve(None, env(&[]), || Some("zh-Hans-CN".into())), Lang::ZhCn);
        assert_eq!(resolve(None, env(&[]), none), Lang::En);
    }

    #[test]
    fn empty_and_posix_values_do_not_decide() {
        assert_eq!(resolve(None, env(&[("TOWERMD_LANG", ""), ("LC_ALL", "C"), ("LANG", "zh_CN.UTF-8")]), || None), Lang::ZhCn);
        assert_eq!(resolve(Some(""), env(&[]), || Some("zh-Hans".into())), Lang::ZhCn);
    }

    #[test]
    fn apple_languages_use_the_first_entry() {
        assert_eq!(first_apple_language("(\n    \"zh-Hans-CN\",\n    en\n)\n").as_deref(), Some("zh-Hans-CN"));
        assert_eq!(first_apple_language("(\n    en,\n    \"zh-Hans-CN\"\n)\n").as_deref(), Some("en"));
        assert_eq!(first_apple_language("(\n)\n"), None);
    }

    #[test]
    fn lang_flag_is_found_before_the_separator_only() {
        let args = |list: &[&str]| list.iter().map(OsString::from).collect::<Vec<_>>();
        assert_eq!(flag_from_args(&args(&["towermd", "status", "--lang", "zh-CN"])).as_deref(), Some("zh-CN"));
        assert_eq!(flag_from_args(&args(&["towermd", "--lang=en", "status"])).as_deref(), Some("en"));
        assert_eq!(flag_from_args(&args(&["towermd", "ask", "--", "--lang", "en"])), None);
    }
}
