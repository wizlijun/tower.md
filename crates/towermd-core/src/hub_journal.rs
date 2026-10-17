use crate::hub::{Event, SystemCheckpoint};
use crate::hub_protocol::HubError;
use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Header {
    pub version: u32,
    pub checkpoint: SystemCheckpoint,
}
pub(crate) struct Journal {
    dir: PathBuf,
    file: File,
    pub poisoned: bool,
    #[cfg(test)]
    pub fault: Option<bool>,
    #[cfg(test)]
    pub serialized_events: u64,
    #[cfg(test)]
    pub written_events: u64,
}
pub(crate) fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>, HubError> {
    let mut bytes = serde_json::to_vec(value).map_err(|e| HubError::Invalid(e.to_string()))?;
    bytes.push(b'\n');
    Ok(bytes)
}
pub(crate) fn decode(bytes: &[u8]) -> Result<(Header, Vec<Event>), HubError> {
    if !bytes.ends_with(b"\n") {
        return Err(HubError::Invalid(
            "invalid journal header or missing newline".into(),
        ));
    }
    let mut lines = bytes.split(|b| *b == b'\n');
    let header: Header = serde_json::from_slice(lines.next().unwrap())
        .map_err(|e| HubError::Invalid(format!("invalid journal header: {e}")))?;
    if header.version != 2 {
        return Err(HubError::Invalid("unsupported journal version".into()));
    }
    let mut events = Vec::new();
    let remaining = lines.collect::<Vec<_>>();
    for (index, line) in remaining.iter().enumerate() {
        if line.is_empty() && index + 1 == remaining.len() {
            continue;
        }
        events.push(
            serde_json::from_slice(line)
                .map_err(|e| HubError::Invalid(format!("invalid complete journal event: {e}")))?,
        );
    }
    Ok((header, events))
}

pub(crate) fn read_only(dir: &Path) -> Result<Option<(Header, Vec<Event>)>, HubError> {
    match fs::read(dir.join("journal.jsonl")) {
        Ok(bytes) => decode(&bytes).map(Some),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => crate::hub_legacy::load(dir)
            .map(|saved| {
                saved.map(|(checkpoint, events)| {
                    (
                        Header {
                            version: 2,
                            checkpoint,
                        },
                        events,
                    )
                })
            }),
        Err(error) => Err(io(error)),
    }
}

impl Journal {
    pub fn open(dir: &Path) -> Result<(Self, Header, Vec<Event>), HubError> {
        let path = dir.join("journal.jsonl");
        if !path.exists() {
            let header = match crate::hub_legacy::load(dir)? {
                Some((checkpoint, events)) => {
                    Self::install_initial(
                        dir,
                        &Header {
                            version: 2,
                            checkpoint: checkpoint.clone(),
                        },
                        &events,
                    )?;
                    return Self::open(dir);
                }
                None => Header {
                    version: 2,
                    checkpoint: SystemCheckpoint::default(),
                },
            };
            Self::install_initial(dir, &header, &[])?;
        }
        let mut file = OpenOptions::new()
            .read(true)
            .append(true)
            .open(&path)
            .map_err(io)?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).map_err(io)?;
        let end = bytes.iter().rposition(|b| *b == b'\n').map_or(0, |p| p + 1);
        if end == 0 {
            return Err(HubError::Invalid(
                "invalid journal header or missing newline".into(),
            ));
        }
        if end < bytes.len() {
            file.set_len(end as u64).map_err(io)?;
            file.sync_all().map_err(io)?;
            bytes.truncate(end);
        }
        let (header, events) = decode(&bytes)?;
        file.seek(SeekFrom::End(0)).map_err(io)?;
        Ok((
            Self {
                dir: dir.into(),
                file,
                poisoned: false,
                #[cfg(test)]
                fault: None,
                #[cfg(test)]
                serialized_events: 0,
                #[cfg(test)]
                written_events: 0,
            },
            header,
            events,
        ))
    }
    fn install_initial(dir: &Path, header: &Header, events: &[Event]) -> Result<(), HubError> {
        Self::install_initial_with_sync(dir, header, events, File::sync_all, File::sync_all)
    }
    fn install_initial_with_sync(
        dir: &Path,
        header: &Header,
        events: &[Event],
        file_sync: impl FnOnce(&File) -> std::io::Result<()>,
        parent_sync: impl FnOnce(&File) -> std::io::Result<()>,
    ) -> Result<(), HubError> {
        let parent = File::open(dir).map_err(io)?;
        let temporary = dir.join(format!(".journal-{}.tmp", uuid::Uuid::now_v7()));
        let result = (|| {
            let mut file = OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&temporary)
                .map_err(io)?;
            file.write_all(&encode(header)?).map_err(io)?;
            for event in events {
                file.write_all(&encode(event)?).map_err(io)?;
            }
            file_sync(&file).map_err(io)?;
            fs::rename(&temporary, dir.join("journal.jsonl")).map_err(io)?;
            parent_sync(&parent).map_err(|error| {
                HubError::Uncertain(format!(
                    "journal installed but directory sync failed; reopen hub: {error}"
                ))
            })
        })();
        if result.is_err() {
            let _ = fs::remove_file(temporary);
        }
        result
    }
    pub fn certain(&self) -> Result<(), HubError> {
        if self.poisoned {
            Err(HubError::Uncertain(
                "hub commit outcome is uncertain; drop and reopen hub before writing".into(),
            ))
        } else {
            Ok(())
        }
    }
    pub fn append(&mut self, event: &Event) -> Result<(), HubError> {
        #[cfg(test)]
        {
            self.serialized_events += 1;
        }
        let bytes = encode(event)?;
        #[cfg(test)]
        if let Some(partial) = self.fault.take() {
            return self.append_bytes(&bytes, |file, bytes| {
                file.write_all(if partial {
                    &bytes[..bytes.len() / 2]
                } else {
                    bytes
                })?;
                Err(std::io::Error::other("injected write/fsync failure"))
            });
        }
        let result = self.append_bytes(&bytes, |file, bytes| {
            file.write_all(bytes)?;
            file.sync_all()
        });
        #[cfg(test)]
        if result.is_ok() {
            self.written_events += 1;
        }
        result
    }
    fn append_bytes(
        &mut self,
        bytes: &[u8],
        write: impl FnOnce(&mut File, &[u8]) -> std::io::Result<()>,
    ) -> Result<(), HubError> {
        self.certain()?;
        if let Err(error) = write(&mut self.file, bytes) {
            self.poisoned = true;
            return Err(HubError::Uncertain(format!(
                "journal write/fsync outcome uncertain; reopen hub: {error}"
            )));
        }
        Ok(())
    }
    pub fn replace(&mut self, header: &Header, events: &[Event]) -> Result<(), HubError> {
        self.replace_with_sync(header, events, File::sync_all)
    }
    pub(crate) fn replace_with_sync(
        &mut self,
        header: &Header,
        events: &[Event],
        sync: impl FnOnce(&File) -> std::io::Result<()>,
    ) -> Result<(), HubError> {
        self.certain()?;
        let parent = File::open(&self.dir).map_err(io)?;
        let temporary = self
            .dir
            .join(format!(".journal-{}.tmp", uuid::Uuid::now_v7()));
        let result = (|| {
            let mut file = OpenOptions::new()
                .create_new(true)
                .read(true)
                .append(true)
                .open(&temporary)
                .map_err(io)?;
            file.write_all(&encode(header)?).map_err(io)?;
            for event in events {
                file.write_all(&encode(event)?).map_err(io)?;
            }
            file.sync_all().map_err(io)?;
            fs::rename(&temporary, self.dir.join("journal.jsonl")).map_err(io)?;
            if let Err(error) = sync(&parent) {
                self.poisoned = true;
                return Err(HubError::Uncertain(format!(
                    "journal renamed but directory sync failed; reopen hub: {error}"
                )));
            }
            self.file = file;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(temporary);
        }
        result
    }
}
fn io(error: std::io::Error) -> HubError {
    HubError::Io(error.to_string())
}

pub(crate) fn unique_map<'de, D, V>(
    deserializer: D,
) -> Result<std::collections::BTreeMap<String, V>, D::Error>
where
    D: serde::Deserializer<'de>,
    V: serde::Deserialize<'de>,
{
    struct Unique<V>(std::marker::PhantomData<V>);
    impl<'de, V: serde::Deserialize<'de>> serde::de::Visitor<'de> for Unique<V> {
        type Value = std::collections::BTreeMap<String, V>;
        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("map with unique keys")
        }
        fn visit_map<M: serde::de::MapAccess<'de>>(
            self,
            mut map: M,
        ) -> Result<Self::Value, M::Error> {
            let mut values = std::collections::BTreeMap::new();
            while let Some((key, value)) = map.next_entry::<String, V>()? {
                if values.insert(key, value).is_some() {
                    return Err(serde::de::Error::custom("duplicate system checkpoint key"));
                }
            }
            Ok(values)
        }
    }
    deserializer.deserialize_map(Unique(std::marker::PhantomData))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn migration_install_failures_keep_v1_and_publish_only_a_complete_generation() {
        for phase in 0..3 {
            let dir = tempfile::tempdir().unwrap();
            let old = b"{\"version\":1,\"events\":[]}";
            fs::write(dir.path().join("journal.json"), old).unwrap();
            let (checkpoint, events) = crate::hub_legacy::load(dir.path()).unwrap().unwrap();
            let header = Header {
                version: 2,
                checkpoint,
            };
            if phase == 2 {
                fs::create_dir(dir.path().join("journal.jsonl")).unwrap();
            }
            let result = Journal::install_initial_with_sync(
                dir.path(),
                &header,
                &events,
                |file| {
                    if phase == 0 {
                        Err(std::io::Error::other("injected temp sync failure"))
                    } else {
                        file.sync_all()
                    }
                },
                |parent| {
                    if phase == 1 {
                        Err(std::io::Error::other("injected directory sync failure"))
                    } else {
                        parent.sync_all()
                    }
                },
            );
            assert!(result.is_err(), "phase {phase}");
            assert_eq!(fs::read(dir.path().join("journal.json")).unwrap(), old);
            assert!(!fs::read_dir(dir.path()).unwrap().any(|entry| entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .ends_with(".tmp")));
            match phase {
                0 => assert!(!dir.path().join("journal.jsonl").exists()),
                1 => {
                    assert!(matches!(result, Err(HubError::Uncertain(_))));
                    assert!(crate::hub::Hub::open(dir.path()).is_ok());
                }
                _ => {
                    fs::remove_dir(dir.path().join("journal.jsonl")).unwrap();
                    assert!(crate::hub::Hub::open(dir.path()).is_ok());
                }
            }
        }
    }
}
