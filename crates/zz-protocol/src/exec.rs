use std::{collections::BTreeMap, fmt, sync::OnceLock};

use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Visitor};

use crate::{
    ClientInstanceId, ClientPath, CommandInvocation, PaneId, PreparedCommand, RawText, ServerError,
    message::{
        MAX_CLIENT_ENVIRONMENT_BYTES, MAX_CLIENT_ENVIRONMENT_ENTRIES,
        MAX_CLIENT_ENVIRONMENT_ENTRY_BYTES, MAX_CLIENT_WORKING_DIRECTORY_BYTES,
    },
};

pub const MAX_EXEC_TTY_BYTES: usize = 1024;
pub const EXEC_CAPABILITY: &str = "exec-v1";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecFlags(u8);

impl ExecFlags {
    pub const UTF8: Self = Self(1);
    pub const STDIN_AVAILABLE: Self = Self(1 << 1);
    pub const NESTED: Self = Self(1 << 2);
    pub const RESUME: Self = Self(1 << 3);
    pub const PREPARED: Self = Self(1 << 4);
    pub const LAST: Self = Self(1 << 5);

    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    pub fn set(&mut self, other: Self, enabled: bool) {
        if enabled {
            self.0 |= other.0;
        } else {
            self.0 &= !other.0;
        }
    }
}

#[derive(Default)]
pub struct ClientEnvironmentBlob {
    bytes: Vec<u8>,
    parsed: OnceLock<BTreeMap<RawText, RawText>>,
}

impl ClientEnvironmentBlob {
    #[must_use]
    pub fn from_bytes(bytes: Vec<u8>) -> Self {
        Self {
            bytes,
            parsed: OnceLock::new(),
        }
    }

    #[must_use]
    pub fn from_map(map: BTreeMap<RawText, RawText>) -> Self {
        let mut bytes = Vec::new();
        for (name, value) in &map {
            bytes.extend_from_slice(name.as_bytes());
            bytes.push(b'=');
            bytes.extend_from_slice(value.as_bytes());
            bytes.push(0);
        }
        Self {
            bytes,
            parsed: OnceLock::from(map),
        }
    }

    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn entries(&self) -> impl Iterator<Item = &[u8]> {
        self.bytes
            .split(|byte| *byte == 0)
            .filter(|entry| !entry.is_empty())
    }

    #[must_use]
    pub fn get(&self, name: &str) -> Option<&[u8]> {
        if let Some(parsed) = self.parsed.get() {
            return parsed.get(name).map(RawText::as_bytes);
        }
        let name = name.as_bytes();
        self.entries()
            .filter_map(|entry| {
                let value = entry.strip_prefix(name)?.strip_prefix(b"=")?;
                (!name.is_empty()).then_some(value)
            })
            .last()
    }

    pub fn map(&self) -> &BTreeMap<RawText, RawText> {
        self.parsed.get_or_init(|| self.parse())
    }

    #[must_use]
    pub fn into_map(mut self) -> BTreeMap<RawText, RawText> {
        self.parsed.take().unwrap_or_else(|| self.parse())
    }

    fn parse(&self) -> BTreeMap<RawText, RawText> {
        self.entries()
            .filter_map(|entry| {
                let separator = entry.iter().position(|byte| *byte == b'=')?;
                (separator > 0).then(|| {
                    (
                        RawText::from(&entry[..separator]),
                        RawText::from(&entry[separator + 1..]),
                    )
                })
            })
            .collect()
    }

    #[must_use]
    pub fn is_valid(&self) -> bool {
        if self.bytes.len() > MAX_CLIENT_ENVIRONMENT_BYTES + MAX_CLIENT_ENVIRONMENT_ENTRIES {
            return false;
        }
        let mut count = 0usize;
        let mut total = 0usize;
        for entry in self.entries() {
            count += 1;
            total += entry.len();
            if count > MAX_CLIENT_ENVIRONMENT_ENTRIES
                || entry.len() > MAX_CLIENT_ENVIRONMENT_ENTRY_BYTES
                || entry
                    .iter()
                    .position(|byte| *byte == b'=')
                    .is_none_or(|separator| separator == 0)
            {
                return false;
            }
        }
        total <= MAX_CLIENT_ENVIRONMENT_BYTES
    }
}

impl Clone for ClientEnvironmentBlob {
    fn clone(&self) -> Self {
        Self::from_bytes(self.bytes.clone())
    }
}

impl PartialEq for ClientEnvironmentBlob {
    fn eq(&self, other: &Self) -> bool {
        self.bytes == other.bytes
    }
}

impl Eq for ClientEnvironmentBlob {}

impl fmt::Debug for ClientEnvironmentBlob {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ClientEnvironmentBlob")
            .field("entries", &self.entries().count())
            .field("bytes", &self.bytes.len())
            .finish_non_exhaustive()
    }
}

impl Serialize for ClientEnvironmentBlob {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_bytes(&self.bytes)
    }
}

impl<'de> Deserialize<'de> for ClientEnvironmentBlob {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct BlobVisitor;

        impl Visitor<'_> for BlobVisitor {
            type Value = Vec<u8>;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(
                    formatter,
                    "an environment blob of at most {} bytes",
                    MAX_CLIENT_ENVIRONMENT_BYTES + MAX_CLIENT_ENVIRONMENT_ENTRIES
                )
            }

            fn visit_bytes<E>(self, value: &[u8]) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                self.visit_byte_buf(value.to_vec())
            }

            fn visit_byte_buf<E>(self, value: Vec<u8>) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                if value.len() > MAX_CLIENT_ENVIRONMENT_BYTES + MAX_CLIENT_ENVIRONMENT_ENTRIES {
                    return Err(E::invalid_length(value.len(), &self));
                }
                Ok(value)
            }
        }

        deserializer
            .deserialize_byte_buf(BlobVisitor)
            .map(ClientEnvironmentBlob::from_bytes)
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecRequest {
    pub protocol_version: u16,
    pub flags: ExecFlags,
    pub client_instance_id: ClientInstanceId,
    pub origin: Option<PaneId>,
    pub working_directory: Option<ClientPath>,
    pub tty: Option<String>,
    pub size: Option<(u16, u16)>,
    pub features: u32,
    pub startup_reentry: Option<u64>,
    pub spawned_server_id: Option<u64>,
    pub expect_server_id: Option<u64>,
    pub process_id: u32,
    pub environment: ClientEnvironmentBlob,
    pub commands: Vec<CommandInvocation>,
}

impl ExecRequest {
    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.working_directory
            .as_ref()
            .is_none_or(|path| path.len() <= MAX_CLIENT_WORKING_DIRECTORY_BYTES)
            && self
                .tty
                .as_ref()
                .is_none_or(|tty| tty.len() <= MAX_EXEC_TTY_BYTES)
            && self.environment.is_valid()
    }
}

impl fmt::Debug for ExecRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ExecRequest")
            .field("protocol_version", &self.protocol_version)
            .field("flags", &self.flags)
            .field("client_instance_id", &self.client_instance_id)
            .field("origin", &self.origin)
            .field("tty", &self.tty)
            .field("size", &self.size)
            .field("features", &self.features)
            .field("startup_reentry", &self.startup_reentry)
            .field("spawned_server_id", &self.spawned_server_id)
            .field("expect_server_id", &self.expect_server_id)
            .field("process_id", &self.process_id)
            .field("environment", &self.environment)
            .field("commands", &self.commands)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExecResumeKind {
    NewSession,
    NativeAttach,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecResume {
    pub kind: ExecResumeKind,
    pub commands: Vec<PreparedCommand>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExecOutcome {
    Ran,
    Resume(ExecResume),
    Rejected(ServerError),
    ServerMismatch,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecExit {
    pub server_id: u64,
    pub outcome: ExecOutcome,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        PROTOCOL_VERSION, PreparedCommandResult, ProtocolError, ProtocolMessage,
        decode_protocol_frame, encode_protocol_message,
    };

    fn request(environment: &[u8]) -> ExecRequest {
        let mut flags = ExecFlags::default();
        flags.set(ExecFlags::UTF8, true);
        flags.set(ExecFlags::RESUME, true);
        ExecRequest {
            protocol_version: PROTOCOL_VERSION,
            flags,
            client_instance_id: ClientInstanceId(9),
            origin: Some(PaneId(4)),
            working_directory: ClientPath::from_path(std::path::Path::new("/tmp/exec")),
            tty: Some("/dev/ttys004".to_owned()),
            size: Some((80, 24)),
            features: 5,
            startup_reentry: None,
            spawned_server_id: Some(77),
            expect_server_id: None,
            process_id: 321,
            environment: ClientEnvironmentBlob::from_bytes(environment.to_vec()),
            commands: vec![
                CommandInvocation::new("display-message", ["-p", "#{pane_id}"]),
                CommandInvocation::new("list-sessions", Vec::<String>::new()),
            ],
        }
    }

    #[test]
    fn exec_request_round_trips_with_its_environment_blob() {
        let message = ProtocolMessage::Exec(request(b"TERM=xterm\0HOME=/home/u\0"));
        let frame = encode_protocol_message(&message).expect("encode exec");
        assert_eq!(decode_protocol_frame(&frame).expect("decode exec"), message);
    }

    #[test]
    fn exec_exit_round_trips_every_outcome() {
        let prepared = PreparedCommand {
            invocation: CommandInvocation::new("new-session", Vec::<String>::new()),
            canonical_name: Some("new-session".to_owned()),
            alias_matched: false,
            result: PreparedCommandResult::Ready,
        };
        for outcome in [
            ExecOutcome::Ran,
            ExecOutcome::ServerMismatch,
            ExecOutcome::Rejected(ServerError::CommandParse("unknown command: x".to_owned())),
            ExecOutcome::Resume(ExecResume {
                kind: ExecResumeKind::NewSession,
                commands: vec![prepared.clone()],
            }),
        ] {
            let message = ProtocolMessage::ExecExit(ExecExit {
                server_id: 42,
                outcome,
            });
            let frame = encode_protocol_message(&message).expect("encode exit");
            assert_eq!(decode_protocol_frame(&frame).expect("decode exit"), message);
        }
    }

    #[test]
    fn environment_lookups_match_a_parsed_map_where_the_last_entry_wins() {
        let blob =
            ClientEnvironmentBlob::from_bytes(b"A=1\0TERM=screen\0=bad\0NOEQ\0A=2\0B=\0".to_vec());
        assert_eq!(blob.get("A"), Some(&b"2"[..]));
        assert_eq!(blob.get("B"), Some(&b""[..]));
        assert_eq!(blob.get("TERM"), Some(&b"screen"[..]));
        assert_eq!(blob.get("NOEQ"), None);
        assert_eq!(blob.get(""), None);
        let map = blob.map();
        assert_eq!(map.len(), 3);
        assert_eq!(map.get("A").map(RawText::as_bytes), Some(&b"2"[..]));
        assert_eq!(blob.get("A"), Some(&b"2"[..]));
    }

    #[test]
    fn malformed_environment_entries_reject_the_request() {
        let message = ProtocolMessage::Exec(request(b"TERM=xterm\0=nameless\0"));
        assert!(matches!(
            encode_protocol_message(&message),
            Err(ProtocolError::InvalidClientHello(_))
        ));
        let oversized = vec![b'x'; MAX_CLIENT_ENVIRONMENT_ENTRY_BYTES + 1];
        let mut entry = b"K=".to_vec();
        entry.extend_from_slice(&oversized);
        assert!(!ClientEnvironmentBlob::from_bytes(entry).is_valid());
    }
}
