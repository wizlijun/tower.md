use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use super::*;

fn catalog() -> serde_json::Map<String, serde_json::Value> {
    serde_json::from_str(CATALOG).unwrap()
}

fn en(message: &str) -> String {
    in_lang(Lang::En, message)
}

#[test]
fn every_entry_is_english_and_uses_each_hole() {
    for (chinese, english) in catalog() {
        let english = english.as_str().unwrap_or_else(|| panic!("{chinese}: not a string"));
        assert!(!english.chars().any(is_chinese), "{chinese} -> {english}: Chinese left in the English");
        let holes = chinese.matches("{}").count();
        let parts = parse_english(english, holes).unwrap_or_else(|| panic!("{chinese} -> {english}: hole out of range"));
        let used: BTreeSet<usize> = parts.iter().filter_map(|part| match part {
            Part::Hole(index) => Some(*index),
            Part::Text(_) => None,
        }).collect();
        assert_eq!(used, (0..holes).collect(), "{chinese} -> {english}: every hole must appear");
    }
}

#[test]
fn holes_are_filled_in_order_or_by_index() {
    assert_eq!(parse_english("a {} b {}", 2).unwrap(), vec![Part::Text("a ".into()), Part::Hole(0), Part::Text(" b ".into()), Part::Hole(1)]);
    assert_eq!(parse_english("{1} of {0}", 2).unwrap(), vec![Part::Hole(1), Part::Text(" of ".into()), Part::Hole(0)]);
    assert_eq!(parse_english("{2}", 2), None);
    assert_eq!(parse_english("{x}", 0).unwrap(), vec![Part::Text("{x}".into())]);
    let pieces = |template: &str| template.split("{}").map(str::to_owned).collect::<Vec<_>>();
    assert_eq!(captures(&pieces("扫描 {} 失败：{}"), "扫描 s-1 失败：a：b"), Some(vec!["s-1", "a：b"]));
    assert_eq!(captures(&pieces("{}：{}"), "a：b：c"), Some(vec!["a", "b：c"]));
    assert_eq!(captures(&pieces("任务不存在"), "任务不存在"), Some(vec![]));
    assert_eq!(captures(&pieces("任务不存在"), "任务不存在。"), None);
    assert_eq!(captures(&pieces("x{}y"), "xy"), Some(vec![""]));
}

#[test]
fn english_requests_translate_and_chinese_ones_do_not() {
    let (chinese, english) = catalog().into_iter().find(|(chinese, _)| !chinese.contains("{}")).unwrap();
    assert_eq!(en(&chinese), english.as_str().unwrap());
    assert_eq!(in_lang(Lang::ZhCn, &chinese), chinese);
    // Unknown and already English text passes through unchanged.
    assert_eq!(en("lease held"), "lease held");
    assert_eq!(en("从未登记的消息"), "从未登记的消息");
}

#[test]
fn wrapped_messages_and_lists_translate_as_a_whole() {
    let wrapped = en("无法读取当前目录：守护进程未启动");
    assert!(!wrapped.chars().any(is_chinese), "{wrapped}");
    let list = en("守护进程未启动；守护进程未启动");
    assert!(!list.chars().any(is_chinese) && list.contains("; "), "{list}");
    // Arguments that are data stay as they are.
    let path = en("无法读取当前目录：/tmp/数据");
    assert!(path.ends_with("/tmp/数据"), "{path}");
}

// ---------------------------------------------------------------------------
// Drift guard. Rule: every string literal in non-test code under
// `crates/*/src` that contains a CJK ideograph or CJK/full-width punctuation
// may become an error message (errors are built with `format!`, `.into()`,
// `ok_or`, `map_err`, `Err(...)`, typed errors' `Display`, and joined into
// other messages), so each one needs English in `messages.en.json`. This
// over-collects (a few literals are data, not errors); that only costs a
// translation. Excluded, because they are already bilingual:
// - the second and later arguments of `pick(..)` / `pick_in(..)`, at any depth;
// - `crates/towermd/src/output/help.rs`, whose tables hold both languages.
// Test code is excluded: `*_tests.rs` and `tests.rs` files and `#[cfg(test)] mod name { .. }`.
// Comments, char literals and lifetimes are skipped by a small Rust lexer.
// Each literal is read as a format string: `{{`/`}}` become braces and any
// `{..}` hole becomes `{}`, the catalog's key form.
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
enum Token {
    Str(String),
    Ident(String),
    Punct(char),
}

fn lex(source: &str) -> Vec<Token> {
    let chars: Vec<char> = source.chars().collect();
    let mut tokens = Vec::new();
    let mut i = 0;
    let at = |i: usize| chars.get(i).copied().unwrap_or('\0');
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
        } else if c == '/' && at(i + 1) == '/' {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
        } else if c == '/' && at(i + 1) == '*' {
            let mut depth = 0;
            loop {
                if at(i) == '/' && at(i + 1) == '*' {
                    depth += 1;
                    i += 2;
                } else if at(i) == '*' && at(i + 1) == '/' {
                    depth -= 1;
                    i += 2;
                    if depth == 0 {
                        break;
                    }
                } else {
                    i += 1;
                }
            }
        } else if c.is_alphabetic() || c == '_' {
            let start = i;
            while at(i).is_alphanumeric() || at(i) == '_' {
                i += 1;
            }
            let ident: String = chars[start..i].iter().collect();
            if (ident == "r" || ident == "br") && (at(i) == '"' || at(i) == '#') {
                let hashes = (i..).take_while(|&j| at(j) == '#').count();
                assert_eq!(at(i + hashes), '"', "raw string");
                let body = i + hashes + 1;
                let mut end = body;
                while !(at(end) == '"' && (1..=hashes).all(|k| at(end + k) == '#')) {
                    end += 1;
                }
                tokens.push(Token::Str(chars[body..end].iter().collect()));
                i = end + 1 + hashes;
            } else if ident == "b" && at(i) == '\'' {
                i = skip_char(&chars, i);
            } else if ident == "b" && at(i) == '"' {
                let (text, next) = string(&chars, i);
                tokens.push(Token::Str(text));
                i = next;
            } else {
                tokens.push(Token::Ident(ident));
            }
        } else if c == '"' {
            let (text, next) = string(&chars, i);
            tokens.push(Token::Str(text));
            i = next;
        } else if c == '\'' {
            // A char literal closes within one (escaped) character; a lifetime does not.
            if at(i + 1) == '\\' || at(i + 2) == '\'' {
                i = skip_char(&chars, i);
            } else {
                i += 1;
            }
        } else {
            tokens.push(Token::Punct(c));
            i += 1;
        }
    }
    tokens
}

fn skip_char(chars: &[char], start: usize) -> usize {
    let mut i = start + 1;
    if chars[i] == '\\' {
        i += 2;
    } else {
        i += 1;
    }
    while chars[i] != '\'' {
        i += 1;
    }
    i + 1
}

/// A `"…"` literal starting at `start`, unescaped; returns the index after it.
fn string(chars: &[char], start: usize) -> (String, usize) {
    let mut text = String::new();
    let mut i = start + 1;
    while chars[i] != '"' {
        if chars[i] != '\\' {
            text.push(chars[i]);
            i += 1;
            continue;
        }
        let escape = chars[i + 1];
        i += 2;
        match escape {
            'n' => text.push('\n'),
            't' => text.push('\t'),
            'r' => text.push('\r'),
            '0' => text.push('\0'),
            'x' => {
                let hex: String = chars[i..i + 2].iter().collect();
                text.push(char::from(u8::from_str_radix(&hex, 16).unwrap()));
                i += 2;
            }
            'u' => {
                let close = (i..).find(|&j| chars[j] == '}').unwrap();
                let hex: String = chars[i + 1..close].iter().collect();
                text.push(char::from_u32(u32::from_str_radix(&hex, 16).unwrap()).unwrap());
                i = close + 1;
            }
            '\n' | '\r' => {
                while chars[i].is_whitespace() {
                    i += 1;
                }
            }
            other => text.push(other),
        }
    }
    (text, i + 1)
}

/// Drops `#[cfg(test)]` (plus further attributes and `pub(..)`) `mod name { .. }`.
fn without_test_modules(tokens: Vec<Token>) -> Vec<Token> {
    let punct = |token: Option<&Token>, c: char| token == Some(&Token::Punct(c));
    let ident = |token: Option<&Token>, name: &str| matches!(token, Some(Token::Ident(found)) if found == name);
    let attribute_end = |tokens: &[Token], i: usize| -> Option<usize> {
        if !punct(tokens.get(i), '#') || !punct(tokens.get(i + 1), '[') {
            return None;
        }
        let mut depth = 0;
        for (j, token) in tokens.iter().enumerate().skip(i + 1) {
            match token {
                Token::Punct('[') => depth += 1,
                Token::Punct(']') => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(j + 1);
                    }
                }
                _ => {}
            }
        }
        None
    };
    let cfg_test = [Token::Punct('#'), Token::Punct('['), Token::Ident("cfg".into()), Token::Punct('('), Token::Ident("test".into()), Token::Punct(')'), Token::Punct(']')];
    let mut kept = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        if tokens[i..].starts_with(&cfg_test) {
            let mut j = i + cfg_test.len();
            while let Some(end) = attribute_end(&tokens, j) {
                j = end;
            }
            if ident(tokens.get(j), "pub") {
                j += 1;
                if punct(tokens.get(j), '(') {
                    while !punct(tokens.get(j), ')') {
                        j += 1;
                    }
                    j += 1;
                }
            }
            if ident(tokens.get(j), "mod") && matches!(tokens.get(j + 1), Some(Token::Ident(_))) && punct(tokens.get(j + 2), '{') {
                let mut depth = 0;
                let mut k = j + 2;
                loop {
                    match tokens[k] {
                        Token::Punct('{') => depth += 1,
                        Token::Punct('}') => depth -= 1,
                        _ => {}
                    }
                    k += 1;
                    if depth == 0 {
                        break;
                    }
                }
                i = k;
                continue;
            }
        }
        kept.push(tokens[i].clone());
        i += 1;
    }
    kept
}

/// Chinese literals outside the bilingual arguments of `pick`/`pick_in`.
fn chinese_literals(tokens: &[Token]) -> Vec<String> {
    // (opened by a call to pick/pick_in, commas seen at this level)
    let mut frames: Vec<(bool, usize)> = Vec::new();
    let mut found = Vec::new();
    for (i, token) in tokens.iter().enumerate() {
        match token {
            Token::Punct(open @ ('(' | '[' | '{')) => {
                let pick = *open == '(' && matches!(i.checked_sub(1).map(|j| &tokens[j]), Some(Token::Ident(name)) if name == "pick" || name == "pick_in");
                frames.push((pick, 0));
            }
            Token::Punct(')' | ']' | '}') => {
                frames.pop();
            }
            Token::Punct(',') => {
                if let Some(frame) = frames.last_mut() {
                    frame.1 += 1;
                }
            }
            Token::Str(text) if text.chars().any(is_chinese) => {
                if !frames.iter().any(|&(pick, commas)| pick && commas >= 1) {
                    found.push(template(text));
                }
            }
            _ => {}
        }
    }
    found
}

/// A literal read as a format string, with every hole written `{}`.
fn template(literal: &str) -> String {
    let mut out = String::new();
    let mut chars = literal.chars().peekable();
    while let Some(c) = chars.next() {
        match (c, chars.peek()) {
            ('{', Some('{')) | ('}', Some('}')) => {
                chars.next();
                out.push(c);
            }
            ('{', _) => {
                for inner in chars.by_ref() {
                    if inner == '}' {
                        break;
                    }
                }
                out.push_str("{}");
            }
            _ => out.push(c),
        }
    }
    out
}

fn rust_files(directory: &Path, files: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(directory).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            rust_files(&path, files);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            files.push(path);
        }
    }
}

fn source_templates() -> BTreeSet<String> {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let mut files = Vec::new();
    for entry in std::fs::read_dir(crates).unwrap() {
        let source = entry.unwrap().path().join("src");
        if source.is_dir() {
            rust_files(&source, &mut files);
        }
    }
    files.sort();
    assert!(files.len() > 40, "found only {} source files", files.len());
    let bilingual = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/output/help.rs");
    files
        .iter()
        .filter(|path| {
            let stem = path.file_stem().unwrap().to_string_lossy();
            **path != bilingual && stem != "tests" && !stem.ends_with("_tests")
        })
        .flat_map(|path| chinese_literals(&without_test_modules(lex(&std::fs::read_to_string(path).unwrap()))))
        .collect()
}

#[test]
fn lexer_skips_comments_chars_tests_and_bilingual_arguments() {
    let source = r##"
        // 注释
        /* 块 /* 嵌套 */ 注释 */
        fn f<'a>(x: &'a str) -> char { let _ = r#"原始"#; let _ = "转义\"{name}{{x}}\
            续行"; '；' }
        let _ = lang::pick("en", "中文"); let _ = pick_in(lang, ("a", "b"), ("甲", format!("乙{x}")));
        let _ = pick(format!("{}", "保留"), "丢弃");
        #[cfg(test)]
        #[path = "x"]
        mod tests { "测试" }
        #[cfg(test)]
        let kept = "非模块";
    "##;
    let found = chinese_literals(&without_test_modules(lex(source)));
    assert_eq!(found, vec!["原始", "转义\"{}{x}续行", "保留", "非模块"]);
}

#[test]
fn every_chinese_message_in_the_source_has_english() {
    let source = source_templates();
    let catalog: BTreeSet<String> = catalog().keys().cloned().collect();
    let missing: Vec<_> = source.difference(&catalog).collect();
    assert!(
        missing.is_empty(),
        "add these to crates/towermd/src/output/messages.en.json (key: the Chinese with holes as {{}}):\n{}",
        missing.iter().map(|text| serde_json::to_string(text).unwrap()).collect::<Vec<_>>().join("\n")
    );
    let stale: Vec<_> = catalog.difference(&source).collect();
    assert!(stale.is_empty(), "no longer in the source; remove from messages.en.json:\n{stale:#?}");
}
