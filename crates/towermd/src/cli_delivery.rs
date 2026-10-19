//! Printing is not processing. Only a complete stdout write and flush may ack.
use std::io::Write;
use towermd_core::human_intervention::Message;

pub fn print_then_ack<W: Write>(
    out: &mut W,
    messages: &[Message],
    ack: impl FnOnce(&[String]) -> Result<(), String>,
) -> Result<(), String> {
    if messages.is_empty() {
        return Ok(());
    }
    let mut ids = Vec::with_capacity(messages.len());
    for message in messages {
        let mut line = serde_json::to_vec(&serde_json::json!({"towermd_message": message}))
            .map_err(|error| error.to_string())?;
        line.push(b'\n');
        out.write_all(&line).map_err(|error| {
            format!("inbox stdout write failed; no delivery acknowledged: {error}")
        })?;
        ids.push(message.id.clone());
    }
    out.flush()
        .map_err(|error| format!("inbox stdout flush failed; no delivery acknowledged: {error}"))?;
    ack(&ids).map_err(|error| {
        format!(
            "inbox delivery acknowledgement uncertain; stable IDs may be printed again: {error}"
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::Cell, io};

    fn message() -> Message {
        Message {
            id: "H-1/A-1/R-1".into(),
            hold: "H-1".into(),
            run: "R-1".into(),
            text: "answer\nwith a second line".into(),
            kind: "reply".into(),
            actionable: false,
        }
    }
    struct Partial {
        remaining: usize,
        bytes: Vec<u8>,
    }
    impl Write for Partial {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if self.remaining == 0 {
                return Err(io::Error::new(io::ErrorKind::BrokenPipe, "partial"));
            }
            let count = bytes.len().min(self.remaining);
            self.bytes.extend_from_slice(&bytes[..count]);
            self.remaining -= count;
            Ok(count)
        }
        fn flush(&mut self) -> io::Result<()> {
            panic!("partial stdout cannot flush");
        }
    }
    #[test]
    fn failed_stdout_does_not_ack() {
        let called = Cell::new(false);
        let mut out = Partial {
            remaining: 7,
            bytes: vec![],
        };
        assert!(print_then_ack(&mut out, &[message()], |_| {
            called.set(true);
            Ok(())
        })
        .is_err());
        assert_eq!(out.bytes.len(), 7);
        assert!(!called.get());
    }
    struct FlushFailure(Vec<u8>);
    impl Write for FlushFailure {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Err(io::Error::new(io::ErrorKind::BrokenPipe, "flush"))
        }
    }
    #[test]
    fn failed_flush_does_not_ack_even_after_complete_output() {
        let called = Cell::new(false);
        let mut out = FlushFailure(vec![]);
        assert!(print_then_ack(&mut out, &[message()], |_| {
            called.set(true);
            Ok(())
        })
        .is_err());
        assert!(out.0.ends_with(b"\n"));
        assert!(!called.get());
    }
    struct Observed<'a> {
        bytes: Vec<u8>,
        flushed: &'a Cell<bool>,
    }
    impl Write for Observed<'_> {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.bytes.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            self.flushed.set(true);
            Ok(())
        }
    }
    #[test]
    fn all_jsonl_flush_before_ack_and_failed_ack_preserves_stable_ids() {
        let flushed = Cell::new(false);
        let mut out = Observed {
            bytes: vec![],
            flushed: &flushed,
        };
        let mut second = message();
        second.id = "H-2/A-2/R-1".into();
        second.hold = "H-2".into();
        let messages = vec![message(), second];
        let error = print_then_ack(&mut out, &messages, |ids| {
            assert!(flushed.get());
            assert_eq!(ids, &[messages[0].id.clone(), messages[1].id.clone()]);
            Err("ack reply lost".into())
        })
        .unwrap_err();
        assert!(error.contains("uncertain"));
        let lines: Vec<serde_json::Value> = out
            .bytes
            .split(|byte| *byte == b'\n')
            .filter(|line| !line.is_empty())
            .map(|line| serde_json::from_slice(line).unwrap())
            .collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0]["towermd_message"]["text"], messages[0].text);
        let mut again = Vec::new();
        print_then_ack(&mut again, &messages, |ids| {
            assert_eq!(ids[0], messages[0].id);
            Ok(())
        })
        .unwrap();
        assert_eq!(again, out.bytes);
    }
    #[test]
    fn no_messages_do_not_flush_or_ack() {
        let flushed = Cell::new(false);
        let mut out = Observed {
            bytes: vec![],
            flushed: &flushed,
        };
        print_then_ack(&mut out, &[], |_| panic!("empty ack is forbidden")).unwrap();
        assert!(out.bytes.is_empty());
        assert!(!flushed.get());
    }
}
