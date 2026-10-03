use std::{
    collections::BTreeMap,
    fmt::Write as _,
    hash::{DefaultHasher, Hasher},
};

use serde::{Deserialize, Serialize};

use crate::{
    ClientEnvironmentBlob, ClientHello, ClientId, ClientInstanceId, ClientKind, KeyTableSnapshot,
    MuxOptionKey, PreparedCommand, ProtocolError, ProtocolMessage, SessionId, TreeOp, WindowId,
};

pub const CONTROL_CAPABILITY: &str = "control-plane-v2";
pub const CONTROL_STDIO_CAPABILITY: &str = "control-stdio-v1";
pub const MAX_BATCH_FRAMES: usize = 4096;

pub const CAPABILITY_NAMES: &[&str] = &[
    "mux-v1",
    "terminal-viewport-v3",
    "terminal-row-patches",
    "terminal-visible-window-subscriptions",
    "terminal-native-selection",
    "terminal-async-regex-search",
    "terminal-osc8-links",
    "terminal-appearance-v2",
    "terminal-appearance-reload",
    "config-overrides-v1",
    "terminal-copy-pipe",
    "native-synchronize-panes",
    "native-command-prompt",
    "native-command-output-view",
    "native-choose-tree",
    "native-choose-buffer",
    "native-display-panes",
    "native-display-popup",
    "native-display-menu",
    "native-confirm-before",
    "native-split-resize",
    "native-pane-swap",
    "native-pane-relocation",
    "native-preset-layouts",
    "native-pane-rotation",
    "browser-panes",
    "tmux-config-subset",
    "new-session-attach-v1",
    "exec-v1",
    "pane-frame-v1",
    CONTROL_CAPABILITY,
    CONTROL_STDIO_CAPABILITY,
];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum TreeSubscription {
    None,
    Attached,
    #[default]
    All,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum KeySubscription {
    None,
    Hash,
    #[default]
    Full,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Subscriptions {
    pub tree: TreeSubscription,
    pub status: bool,
    pub options: u64,
    pub keys: KeySubscription,
    pub pane_stream: bool,
}

impl Default for Subscriptions {
    fn default() -> Self {
        Self {
            tree: TreeSubscription::All,
            status: true,
            options: Self::ALL_OPTIONS,
            keys: KeySubscription::Full,
            pane_stream: true,
        }
    }
}

impl Subscriptions {
    pub const ALL_OPTIONS: u64 = (1 << MuxOptionKey::ALL.len()) - 1;

    #[must_use]
    pub const fn includes_option(self, key: MuxOptionKey) -> bool {
        self.options & (1 << key as u32) != 0
    }

    #[must_use]
    pub const fn terminal() -> Self {
        Self {
            tree: TreeSubscription::Attached,
            status: true,
            options: Self::ALL_OPTIONS,
            keys: KeySubscription::Hash,
            pane_stream: true,
        }
    }

    #[must_use]
    pub const fn control() -> Self {
        Self {
            tree: TreeSubscription::None,
            status: false,
            options: 0,
            keys: KeySubscription::None,
            pane_stream: false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientViewport {
    pub columns: u16,
    pub rows: u16,
    pub cell_width_px: u32,
    pub cell_height_px: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AttachOperation {
    Session(String),
    Commands(Vec<PreparedCommand>),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hello {
    pub client: ClientHello,
    pub viewport: Option<ClientViewport>,
    pub subscriptions: Subscriptions,
    pub attach: Option<AttachOperation>,
    pub environment: ClientEnvironmentBlob,
}

impl Hello {
    #[must_use]
    pub fn from_client(mut client: ClientHello) -> Self {
        let mut bytes = Vec::new();
        for entry in client.environment.drain(..) {
            bytes.extend_from_slice(entry.as_bytes());
            bytes.push(0);
        }
        let subscriptions = if client.kind == ClientKind::Control {
            Subscriptions::control()
        } else {
            Subscriptions::default()
        };
        Self {
            client,
            viewport: None,
            subscriptions,
            attach: None,
            environment: ClientEnvironmentBlob::from_bytes(bytes),
        }
    }

    #[must_use]
    pub fn into_client(mut self) -> ClientHello {
        self.client.environment = self
            .environment
            .entries()
            .map(crate::RawText::from)
            .collect();
        self.client
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Welcome {
    pub protocol_version: u16,
    pub server_id: u64,
    pub client_id: ClientId,
    pub client_instance_id: ClientInstanceId,
    pub caps: u64,
}

impl Welcome {
    #[must_use]
    pub fn has_capability(self, capability: &str) -> bool {
        CAPABILITY_NAMES
            .iter()
            .position(|name| *name == capability)
            .is_some_and(|index| self.caps & (1 << index) != 0)
    }

    #[must_use]
    pub fn caps_from_strings(capabilities: &[String]) -> u64 {
        CAPABILITY_NAMES
            .iter()
            .enumerate()
            .fold(0, |caps, (index, name)| {
                if capabilities.iter().any(|capability| capability == name) {
                    caps | (1 << index)
                } else {
                    caps
                }
            })
    }

    #[must_use]
    pub fn capability_strings(self) -> Vec<String> {
        CAPABILITY_NAMES
            .iter()
            .enumerate()
            .filter(|(index, _)| self.caps & (1 << index) != 0)
            .map(|(_, name)| (*name).to_owned())
            .collect()
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ClientView {
    pub session: Option<SessionId>,
    pub focused_window: Option<WindowId>,
    pub read_only: bool,
    pub client_flags: String,
    pub layout_generation: u64,
    pub attachment_generation: u64,
    pub overlay: Vec<TreeOp>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Batch {
    pub sequence: u64,
    #[serde(
        serialize_with = "crate::message::serialize_byte_vecs",
        deserialize_with = "crate::message::deserialize_byte_vecs"
    )]
    pub frames: Vec<Vec<u8>>,
}

#[cfg(test)]
#[path = "ctrl_bulk_tests.rs"]
mod bulk_tests;

impl Batch {
    pub fn from_messages(
        sequence: u64,
        messages: impl IntoIterator<Item = ProtocolMessage>,
    ) -> Result<Self, ProtocolError> {
        let frames = messages
            .into_iter()
            .map(|message| {
                if matches!(message, ProtocolMessage::Batch(_)) {
                    return Err(ProtocolError::InvalidServerHello(
                        "nested batches are forbidden".to_owned(),
                    ));
                }
                crate::encode_protocol_message(&message)
            })
            .collect::<Result<Vec<_>, _>>()?;
        if frames.len() > MAX_BATCH_FRAMES {
            return Err(ProtocolError::InvalidServerHello(
                "batch contains too many frames".to_owned(),
            ));
        }
        Ok(Self { sequence, frames })
    }

    pub fn messages(&self) -> Result<Vec<ProtocolMessage>, ProtocolError> {
        if self.frames.len() > MAX_BATCH_FRAMES {
            return Err(ProtocolError::InvalidServerHello(
                "batch contains too many frames".to_owned(),
            ));
        }
        let mut messages = Vec::with_capacity(self.frames.len());
        for frame in &self.frames {
            let message = crate::decode_protocol_frame(frame)?;
            if matches!(message, ProtocolMessage::Batch(_)) {
                return Err(ProtocolError::InvalidServerHello(
                    "nested batches are forbidden".to_owned(),
                ));
            }
            messages.push(message);
        }
        Ok(messages)
    }
}

const MOUSE_LOCATIONS: &[&str] = &[
    "Pane",
    "Border",
    "Status",
    "StatusLeft",
    "StatusRight",
    "StatusDefault",
];
const MOUSE_KINDS: &[&str] = &[
    "MouseDown1",
    "MouseDown2",
    "MouseDown3",
    "MouseUp1",
    "MouseUp2",
    "MouseUp3",
    "MouseDrag1",
    "MouseDrag2",
    "MouseDrag3",
    "MouseDragEnd1",
    "MouseDragEnd2",
    "MouseDragEnd3",
    "DoubleClick1",
    "DoubleClick2",
    "DoubleClick3",
    "TripleClick1",
    "TripleClick2",
    "TripleClick3",
    "WheelUp",
    "WheelDown",
];
const MOUSE_LOCATION_COUNT: usize = MOUSE_LOCATIONS.len() + 256;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MouseBindings {
    #[serde(deserialize_with = "deserialize_mouse_words")]
    root: Vec<(u16, u64)>,
    #[serde(deserialize_with = "deserialize_mouse_words")]
    copy: Vec<(u16, u64)>,
}

fn deserialize_mouse_words<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<(u16, u64)>, D::Error> {
    use serde::de::{Error, SeqAccess, Visitor};

    struct WordsVisitor;
    impl<'de> Visitor<'de> for WordsVisitor {
        type Value = Vec<(u16, u64)>;
        fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("sorted nonempty mouse bitset words")
        }
        fn visit_seq<A: SeqAccess<'de>>(self, mut sequence: A) -> Result<Self::Value, A::Error> {
            let limit = (MOUSE_LOCATION_COUNT * MOUSE_KINDS.len() * 8).div_ceil(64);
            if sequence.size_hint().is_some_and(|length| length > limit) {
                return Err(A::Error::custom("too many mouse bitset words"));
            }
            let mut words = Vec::new();
            while let Some((word, mask)) = sequence.next_element::<(u16, u64)>()? {
                if words.len() >= limit
                    || usize::from(word) >= limit
                    || mask == 0
                    || words.last().is_some_and(|&(previous, _)| previous >= word)
                {
                    return Err(A::Error::custom("invalid mouse bitset word"));
                }
                words.push((word, mask));
            }
            Ok(words)
        }
    }
    deserializer.deserialize_seq(WordsVisitor)
}

impl MouseBindings {
    #[must_use]
    pub fn from_tables(tables: &[KeyTableSnapshot]) -> Self {
        Self::from_names(tables.iter().flat_map(|table| {
            table
                .bindings
                .iter()
                .map(move |binding| (table.name.as_str(), binding.key.as_str()))
        }))
    }

    #[must_use]
    pub fn from_key_tables(tables: &crate::KeyTables) -> Self {
        Self::from_names(tables.list(None).map(|(table, key, _)| (table, key)))
    }

    fn from_names<'a>(bindings: impl Iterator<Item = (&'a str, &'a str)>) -> Self {
        let mut root = BTreeMap::new();
        let mut copy = BTreeMap::new();
        for (table, key) in bindings {
            let words = match table {
                "root" => &mut root,
                "copy-mode" | "copy-mode-vi" => &mut copy,
                _ => continue,
            };
            if let Some(index) = mouse_key_index(key) {
                *words.entry((index / 64) as u16).or_insert(0) |= 1 << (index % 64);
            }
        }
        Self {
            root: root.into_iter().collect(),
            copy: copy.into_iter().collect(),
        }
    }

    #[must_use]
    pub fn contains(&self, key: &str) -> bool {
        let Some(index) = mouse_key_index(key) else {
            return false;
        };
        self.root
            .binary_search_by_key(&((index / 64) as u16), |(word, _)| *word)
            .is_ok_and(|at| self.root[at].1 & (1 << (index % 64)) != 0)
    }

    #[must_use]
    pub fn keys(&self) -> Vec<String> {
        Self::word_keys(&self.root)
    }

    #[must_use]
    pub fn copy_keys(&self) -> Vec<String> {
        Self::word_keys(&self.copy)
    }

    fn word_keys(words: &[(u16, u64)]) -> Vec<String> {
        let mut keys = Vec::new();
        for &(word, mask) in words {
            let mut bits = mask;
            while bits != 0 {
                let bit = bits.trailing_zeros() as usize;
                bits &= bits - 1;
                let index = usize::from(word) * 64 + bit;
                let location = index / MOUSE_KINDS.len() % MOUSE_LOCATION_COUNT;
                let kind = index % MOUSE_KINDS.len();
                let modifiers = index / (MOUSE_LOCATION_COUNT * MOUSE_KINDS.len());
                if modifiers >= 8 {
                    continue;
                }
                let mut key = String::new();
                for (mask, prefix) in [(1, "C-"), (2, "M-"), (4, "S-")] {
                    if modifiers & mask != 0 {
                        key.push_str(prefix);
                    }
                }
                key.push_str(MOUSE_KINDS[kind]);
                if location < MOUSE_LOCATIONS.len() {
                    key.push_str(MOUSE_LOCATIONS[location]);
                } else {
                    let _ = write!(key, "Control{}", location - MOUSE_LOCATIONS.len());
                }
                keys.push(key);
            }
        }
        keys
    }
}

fn mouse_key_index(mut key: &str) -> Option<usize> {
    let mut modifiers = 0;
    while let Some((prefix, rest)) = key.split_once('-') {
        modifiers |= match prefix {
            "C" => 1,
            "M" => 2,
            "S" => 4,
            _ => return None,
        };
        key = rest;
    }
    let (kind, location) = MOUSE_KINDS
        .iter()
        .enumerate()
        .find_map(|(kind, prefix)| key.strip_prefix(prefix).map(|location| (kind, location)))?;
    let location = MOUSE_LOCATIONS
        .iter()
        .position(|candidate| *candidate == location)
        .or_else(|| {
            location
                .strip_prefix("Control")?
                .parse::<u8>()
                .ok()
                .map(|control| MOUSE_LOCATIONS.len() + usize::from(control))
        })?;
    Some((modifiers * MOUSE_LOCATION_COUNT + location) * MOUSE_KINDS.len() + kind)
}

#[must_use]
pub fn key_tables_hash(tables: &[KeyTableSnapshot]) -> u64 {
    let mut hasher = DefaultHasher::new();
    let _ = postcard::serialize_with_flavor(tables, HashFlavor(&mut hasher));
    hasher.finish()
}

struct HashFlavor<'a>(&'a mut DefaultHasher);

impl postcard::ser_flavors::Flavor for HashFlavor<'_> {
    type Output = ();
    fn try_push(&mut self, byte: u8) -> postcard::Result<()> {
        self.0.write_u8(byte);
        Ok(())
    }
    fn try_extend(&mut self, bytes: &[u8]) -> postcard::Result<()> {
        self.0.write(bytes);
        Ok(())
    }
    fn finalize(self) -> postcard::Result<Self::Output> {
        Ok(())
    }
}
