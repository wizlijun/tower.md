//! English for the Chinese messages that the daemon and the shared crates
//! produce (FR-197). Errors are built in Chinese in many places; they are
//! translated once, where they leave for a client that asked for English:
//! the daemon's error bodies and the CLI envelope.
//!
//! `messages.en.json` maps each Chinese format string, with every hole written
//! `{}`, to its English template. In the English, `{}` takes the holes in
//! order and `{0}`, `{1}`… by index. A hole's text is translated again, so a
//! Chinese message wrapped in another one comes out English as a whole.
//! `messages_tests.rs` keeps the file in step with the source.
use std::sync::OnceLock;

use super::lang::Lang;

const CATALOG: &str = include_str!("messages.en.json");
/// Wrapped messages nest only a few levels deep; this bounds a pathological input.
const MAX_DEPTH: usize = 4;

#[derive(Debug, PartialEq)]
enum Part {
    Text(String),
    Hole(usize),
}

struct Template {
    /// Literal text around the holes; one more piece than holes.
    pieces: Vec<String>,
    english: Vec<Part>,
    /// Literal length: a longer template is the more specific match.
    weight: usize,
}

fn templates() -> &'static [Template] {
    static TEMPLATES: OnceLock<Vec<Template>> = OnceLock::new();
    TEMPLATES.get_or_init(|| {
        let map: serde_json::Map<String, serde_json::Value> = serde_json::from_str(CATALOG).expect("messages.en.json is a JSON object");
        let mut templates: Vec<Template> = map
            .iter()
            .filter_map(|(chinese, english)| {
                let pieces: Vec<String> = chinese.split("{}").map(str::to_owned).collect();
                let english = parse_english(english.as_str()?, pieces.len() - 1)?;
                let weight = pieces.iter().map(|piece| piece.chars().count()).sum();
                Some(Template { pieces, english, weight })
            })
            .collect();
        templates.sort_by(|a, b| b.weight.cmp(&a.weight));
        templates
    })
}

/// `{}` fills holes in order, `{N}` by index; anything else is text. `None`
/// when a hole is out of range, so a broken entry never panics at runtime.
fn parse_english(english: &str, holes: usize) -> Option<Vec<Part>> {
    let mut parts = Vec::new();
    let mut text = String::new();
    let mut next = 0;
    let mut rest = english;
    while let Some(open) = rest.find('{') {
        let close = rest[open..].find('}').map(|close| open + close);
        let index = match close.map(|close| &rest[open + 1..close]) {
            Some("") => Some(next),
            Some(digits) if !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()) => digits.parse().ok(),
            _ => None,
        };
        match (index, close) {
            (Some(index), Some(close)) => {
                if index >= holes {
                    return None;
                }
                text.push_str(&rest[..open]);
                if !text.is_empty() {
                    parts.push(Part::Text(std::mem::take(&mut text)));
                }
                parts.push(Part::Hole(index));
                next = index + 1;
                rest = &rest[close + 1..];
            }
            _ => {
                text.push_str(&rest[..=open]);
                rest = &rest[open + 1..];
            }
        }
    }
    text.push_str(rest);
    if !text.is_empty() {
        parts.push(Part::Text(text));
    }
    Some(parts)
}

/// Characters that mark text as Chinese: CJK ideographs and full-width punctuation.
pub(crate) fn is_chinese(c: char) -> bool {
    matches!(c, '\u{3000}'..='\u{303f}' | '\u{4e00}'..='\u{9fff}' | '\u{ff00}'..='\u{ffef}')
}

/// Holes in order, or `None` if `message` does not have the template's shape.
fn captures<'m>(pieces: &[String], message: &'m str) -> Option<Vec<&'m str>> {
    let (first, rest) = pieces.split_first()?;
    holes(rest, message.strip_prefix(first.as_str())?)
}

/// `message` starts at a hole and `rest` holds the piece after each remaining
/// hole. An earlier hole takes the shortest text that lets the rest match.
fn holes<'m>(rest: &[String], message: &'m str) -> Option<Vec<&'m str>> {
    let Some((next, after)) = rest.split_first() else {
        return message.is_empty().then(Vec::new);
    };
    if after.is_empty() {
        return message.strip_suffix(next.as_str()).map(|hole| vec![hole]);
    }
    message.match_indices(next.as_str()).find_map(|(at, _)| {
        let mut tail = holes(after, &message[at + next.len()..])?;
        tail.insert(0, &message[..at]);
        Some(tail)
    })
}

fn translate(message: &str, depth: usize) -> Option<String> {
    if depth > MAX_DEPTH || !message.chars().any(is_chinese) {
        return None;
    }
    for template in templates() {
        if let Some(holes) = captures(&template.pieces, message) {
            let mut out = String::new();
            for part in &template.english {
                match part {
                    Part::Text(text) => out.push_str(text),
                    Part::Hole(index) => out.push_str(&localize(holes[*index], depth + 1)),
                }
            }
            return Some(out);
        }
    }
    // Lists joined with Chinese separators: translate each item.
    for (separator, english) in [('；', "; "), ('、', ", ")] {
        if message.contains(separator) {
            return Some(message.split(separator).map(|item| localize(item, depth + 1)).collect::<Vec<_>>().join(english));
        }
    }
    None
}

fn localize(message: &str, depth: usize) -> String {
    translate(message, depth).unwrap_or_else(|| message.to_owned())
}

/// `message` in `lang`. Messages are written in Chinese, so only English
/// translates; text the catalog does not know is returned unchanged.
pub fn in_lang(lang: Lang, message: &str) -> String {
    match lang {
        Lang::En => localize(message, 0),
        Lang::ZhCn => message.to_owned(),
    }
}

#[cfg(test)]
#[path = "messages_tests.rs"]
mod tests;
