//! Ignore only validated, leading delivery envelopes. Ordinary output remains
//! exact and is parsed in full, including pretty JSON or multi-line trailers.
#![allow(dead_code)]
use serde::de::DeserializeOwned;
use towermd_core::human_intervention::Message;
pub(crate) fn split(bytes: &[u8]) -> Result<(Vec<Message>, &[u8]), String> {
    let mut messages = Vec::new();
    let mut rest = bytes;
    while let Some(end) = rest.iter().position(|byte| *byte == b'\n') {
        let Ok(value) = serde_json::from_slice::<serde_json::Value>(&rest[..end]) else {
            break;
        };
        if value.get("towermd_message").is_none() {
            break;
        }
        if value.as_object().is_none_or(|object| object.len() != 1) {
            return Err("delivery envelope has extra fields".into());
        }
        let message: Message = serde_json::from_value(value["towermd_message"].clone())
            .map_err(|error| format!("invalid delivery envelope: {error}"))?;
        if serde_json::to_value(&message).map_err(|error| error.to_string())?
            != value["towermd_message"]
        {
            return Err("delivery message has unknown fields".into());
        }
        messages.push(message);
        rest = &rest[end + 1..];
    }
    if serde_json::from_slice::<serde_json::Value>(rest)
        .is_ok_and(|value| value.get("towermd_message").is_some())
    {
        return Err("delivery prefix lacks a newline or command result".into());
    }
    Ok((messages, rest))
}
pub(crate) fn decode<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, serde_json::Error> {
    let (_, rest) = split(bytes).map_err(<serde_json::Error as serde::de::Error>::custom)?;
    serde_json::from_slice(rest)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_exact_message_prefix_is_removed_and_entire_result_stays_strict() {
        let line=b"{\"towermd_message\":{\"id\":\"H/A/R\",\"hold\":\"H\",\"run\":\"R\",\"text\":\"line1\\nline2\",\"kind\":\"reply\",\"actionable\":false}}\n";
        let mut bytes = line.to_vec();
        bytes.extend_from_slice(b"{\n  \"ok\": true\n}\n");
        let (messages, rest) = split(&bytes).unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].text, "line1\nline2");
        assert_eq!(rest, b"{\n  \"ok\": true\n}\n");
        assert_eq!(
            decode::<serde_json::Value>(&bytes).unwrap(),
            serde_json::json!({"ok":true})
        );
        for suffix in [
            b"{\"unexpected\":true}\n{\"ok\":true}".as_slice(),
            b"{\"towermd_message\":null}\n{}",
            b"{\"towermd_message\":{},\"extra\":true}\n{}",
        ] {
            assert!(decode::<serde_json::Value>(suffix).is_err());
        }
        let trailers = b"A: 1\nB: 2\nC: 3\nD: 4\nE: 5\n";
        assert_eq!(
            split(trailers).unwrap().1,
            trailers,
            "no empty prelude changes trailers bytes"
        );
    }
}
