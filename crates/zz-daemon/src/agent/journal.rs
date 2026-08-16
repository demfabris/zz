use std::{
    collections::{HashMap, hash_map::Entry},
    ffi::OsStr,
    fs::{self, File, OpenOptions},
    io::{self, Write as _},
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;
use zz_protocol::AgentProvider;

use crate::user_data::{restrict_directory_to_current_user, restrict_to_current_user};

const JOURNAL_EXTENSION: &str = "jsonl";

/// Ceiling on one session's journal. Past it appends are refused rather than
/// rotated: a truncated head would replay a conversation that never happened.
const MAX_JOURNAL_BYTES: u64 = 2 * 9 * 1024 * 1024;
const MAX_JOURNAL_RECORDS: usize = 4_096;

/// Descriptors held across appends. Updates arrive in bursts, so re-opening per
/// line would be wasteful, but a long-lived app must not keep one descriptor
/// per session it ever touched.
const MAX_OPEN_JOURNALS: usize = 16;

/// Longest file stem taken from a session id before it is truncated and tagged.
const MAX_STEM_BYTES: usize = 96;

/// Ceiling on one coalesced record, flushed the moment it is reached whatever
/// arrives next. It bounds the memory a runaway single message can hold, and
/// keeps the merged record clear of the 1 MiB per-update payload cap the replay
/// path enforces even when every byte of the text escapes to six.
const MAX_PENDING_RECORD_BYTES: u64 = 256 * 1024;

/// The `session/update` kinds that arrive token by token and are worth merging.
/// Everything else, `user_message_chunk` included, is one record per update.
const COALESCED_KINDS: [&str; 2] = ["agent_message_chunk", "agent_thought_chunk"];

const SECONDS_PER_DAY: u64 = 24 * 60 * 60;

#[derive(Debug, Error)]
pub(crate) enum JournalError {
    #[error("journal io: {0}")]
    Io(#[from] io::Error),
    #[error("journal encode: {0}")]
    Encode(#[from] serde_json::Error),
    /// The refusal a caller surfaces: the turn keeps streaming, it just stops
    /// being replayable past this point.
    #[error("journal is full at {bytes} bytes and {records} records")]
    Full { bytes: u64, records: usize },
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum JournalEntry {
    Update(Value),
}

#[derive(Serialize)]
struct JournalRecord<'a> {
    seq: u64,
    #[serde(flatten)]
    body: RecordBody<'a>,
}

/// The record's payload and, through the variant name, the key that tags it.
#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "lowercase")]
enum RecordBody<'a> {
    Update(&'a Value),
}

#[derive(Deserialize)]
struct StoredRecord {
    seq: u64,
    #[serde(flatten)]
    body: StoredBody,
}

/// A record is read shape-first and typed second: an event kind this build does
/// not know about costs its own record, never the seq of the ones around it.
#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
enum StoredBody {
    Update(Value),
    Task(Value),
}

/// A record whose seq, record slot and byte budget are already reserved but
/// whose line is not on disk yet, because the next update may still be another
/// chunk of the same message.
///
/// Constraint: a daemon killed while a record is pending loses it. That is the
/// same loss class as the torn trailing line every reader already skips - the
/// tail of one message goes missing, never the head of a conversation - and it
/// is what buys the O(messages) journal. Everything that can observe the file
/// flushes first, so the loss window is a hard kill and nothing else.
struct PendingRecord {
    seq: u64,
    update: Value,
    /// Exact bytes this record will occupy once written, leading and trailing
    /// newline included, tracked incrementally so a merge stays O(added text)
    /// instead of re-encoding the whole message per chunk.
    line_len: u64,
}

struct OpenJournal {
    file: File,
    next_seq: u64,
    bytes: u64,
    records: usize,
    needs_newline: bool,
    pending: Option<PendingRecord>,
}

impl OpenJournal {
    /// Take one update into the journal and answer the seq of the record it
    /// belongs to, writing whatever that forces out first.
    fn record(&mut self, update: &Value) -> Result<u64, JournalError> {
        let Some(chunk) = text_chunk(update) else {
            self.flush_pending()?;
            return self.write_verbatim(RecordBody::Update(update));
        };
        if let Some(seq) = self.extend_pending(&chunk)? {
            self.settle_pending()?;
            return Ok(seq);
        }
        self.flush_pending()?;
        let seq = self.open_pending(update)?;
        self.settle_pending()?;
        Ok(seq)
    }

    /// Merge `next` into the open record when it continues the same stream.
    /// `None` means it does not and the caller must start a fresh record; the
    /// byte cap is charged here so a merge can be refused before it is taken,
    /// never after.
    fn extend_pending(&mut self, next: &TextChunk<'_>) -> Result<Option<u64>, JournalError> {
        let committed = self.bytes;
        let records = self.records;
        let Some(pending) = self.pending.as_mut() else {
            return Ok(None);
        };
        if !text_chunk(&pending.update).is_some_and(|open| open.continues(next)) {
            return Ok(None);
        }
        let growth = escaped_length(next.text);
        if committed
            .saturating_add(pending.line_len)
            .saturating_add(growth)
            > MAX_JOURNAL_BYTES
        {
            return Err(JournalError::Full {
                bytes: committed,
                records,
            });
        }
        let Some(Value::String(text)) = pending.update.pointer_mut("/content/text") else {
            return Ok(None);
        };
        text.push_str(next.text);
        pending.line_len = pending.line_len.saturating_add(growth);
        Ok(Some(pending.seq))
    }

    /// Reserve a seq, a record slot and the byte budget for a new coalescable
    /// record. Reserving up front is what lets the flush be infallible against
    /// the caps: a buffered record can never be refused after `append` already
    /// answered `Ok`, so it can never overshoot `MAX_JOURNAL_BYTES`.
    fn open_pending(&mut self, update: &Value) -> Result<u64, JournalError> {
        let seq = self.next_seq;
        let line_len = self.line_length(seq, update)?;
        if self.bytes.saturating_add(line_len) > MAX_JOURNAL_BYTES
            || self.records >= MAX_JOURNAL_RECORDS
        {
            return Err(JournalError::Full {
                bytes: self.bytes,
                records: self.records,
            });
        }
        self.records = self.records.saturating_add(1);
        self.next_seq = seq.saturating_add(1);
        self.pending = Some(PendingRecord {
            seq,
            update: update.clone(),
            line_len,
        });
        Ok(seq)
    }

    fn settle_pending(&mut self) -> Result<(), JournalError> {
        if self
            .pending
            .as_ref()
            .is_some_and(|pending| pending.line_len >= MAX_PENDING_RECORD_BYTES)
        {
            self.flush_pending()?;
        }
        Ok(())
    }

    /// Put the open record on disk. Every path that lets an outsider observe
    /// the file - replay, handle eviction, drop - goes through here first.
    fn flush_pending(&mut self) -> Result<(), JournalError> {
        let Some(pending) = self.pending.take() else {
            return Ok(());
        };
        let line = self.encode(pending.seq, RecordBody::Update(&pending.update))?;
        self.commit(&line)
    }

    /// Forget the open record without writing it, for the two callers that are
    /// about to delete the file underneath it.
    fn discard_pending(&mut self) {
        self.pending = None;
    }

    fn write_verbatim(&mut self, body: RecordBody<'_>) -> Result<u64, JournalError> {
        let seq = self.next_seq;
        let line = self.encode(seq, body)?;
        let written = u64::try_from(line.len()).unwrap_or(u64::MAX);
        if self.bytes.saturating_add(written) > MAX_JOURNAL_BYTES
            || self.records >= MAX_JOURNAL_RECORDS
        {
            return Err(JournalError::Full {
                bytes: self.bytes,
                records: self.records,
            });
        }
        self.commit(&line)?;
        self.records = self.records.saturating_add(1);
        self.next_seq = seq.saturating_add(1);
        Ok(seq)
    }

    fn encode(&self, seq: u64, body: RecordBody<'_>) -> Result<Vec<u8>, JournalError> {
        let record = serde_json::to_vec(&JournalRecord { seq, body })?;
        let mut line = Vec::with_capacity(record.len() + 2);
        if self.needs_newline {
            line.push(b'\n');
        }
        line.extend_from_slice(&record);
        line.push(b'\n');
        Ok(line)
    }

    /// Reserved through `encode` rather than alongside it, so the budget a
    /// record claims is by construction the length the flush will write.
    fn line_length(&self, seq: u64, update: &Value) -> Result<u64, JournalError> {
        Ok(u64::try_from(self.encode(seq, RecordBody::Update(update))?.len()).unwrap_or(u64::MAX))
    }

    /// A half-written line leaves the cached seq, length and newline state
    /// wrong, so an error here costs the caller its handle and the next append
    /// rescans the tail.
    fn commit(&mut self, line: &[u8]) -> Result<(), JournalError> {
        self.file.write_all(line)?;
        self.file.flush()?;
        self.needs_newline = false;
        self.bytes = self
            .bytes
            .saturating_add(u64::try_from(line.len()).unwrap_or(u64::MAX));
        Ok(())
    }
}

/// Losing a handle must not lose the record it was holding, so eviction, prune
/// and the journal's own drop all land here.
impl Drop for OpenJournal {
    fn drop(&mut self) {
        if self.pending.is_none() {
            return;
        }
        if let Err(error) = self.flush_pending() {
            log::warn!(
                target: "zz::agent::journal",
                "could not flush a coalesced journal record: {error}",
            );
        }
    }
}

/// The pieces of a chunk update that decide whether it may be merged into the
/// record before it.
struct TextChunk<'a> {
    kind: &'a str,
    message_id: Option<&'a str>,
    meta: Option<&'a Value>,
    typed: bool,
    text: &'a str,
}

impl TextChunk<'_> {
    /// The client reducer keys a message on `(role, messageId)`, and an id-less
    /// chunk extends whatever entry the same role last wrote. Merging is
    /// allowed exactly where both rules agree the two chunks already land in
    /// one entry, so a concatenated record reduces to what it replaced.
    fn continues(&self, next: &TextChunk<'_>) -> bool {
        self.kind == next.kind
            && self.message_id == next.message_id
            && self.meta == next.meta
            && self.typed == next.typed
    }
}

/// The one update shape safe to merge: a streamed agent message or thought
/// carrying a plain text content block and nothing the reducer reads
/// positionally.
///
/// Anything else - a tool call, an image or annotated block, a non-string id,
/// an unknown top-level key - answers `None` and is journalled verbatim. The
/// bar is deliberately low: a shape this does not recognise costs records, a
/// shape it merges wrongly costs a transcript.
fn text_chunk(update: &Value) -> Option<TextChunk<'_>> {
    let object = update.as_object()?;
    let kind = object.get("sessionUpdate")?.as_str()?;
    if !COALESCED_KINDS.contains(&kind) {
        return None;
    }
    if object.keys().any(|key| {
        !matches!(
            key.as_str(),
            "sessionUpdate" | "content" | "messageId" | "_meta"
        )
    }) {
        return None;
    }
    let message_id = match object.get("messageId") {
        Some(value) => Some(value.as_str()?),
        None => None,
    };
    let content = object.get("content")?.as_object()?;
    if content
        .keys()
        .any(|key| !matches!(key.as_str(), "type" | "text"))
    {
        return None;
    }
    let typed = match content.get("type") {
        Some(value) => {
            if value.as_str()? != "text" {
                return None;
            }
            true
        }
        None => false,
    };
    Some(TextChunk {
        kind,
        message_id,
        meta: object.get("_meta"),
        typed,
        text: content.get("text")?.as_str()?,
    })
}

/// Bytes `text` adds to an encoded record when appended to a JSON string,
/// mirroring `serde_json`'s escape table so the reserved budget is exact rather
/// than merely safe.
fn escaped_length(text: &str) -> u64 {
    text.bytes()
        .map(|byte| match byte {
            b'"' | b'\\' | 0x08 | 0x09 | 0x0a | 0x0c | 0x0d => 2,
            0x00..=0x1f => 6,
            _ => 1,
        })
        .sum()
}

struct JournalTail {
    next_seq: u64,
    bytes: u64,
    records: usize,
    needs_newline: bool,
}

/// Append-only JSONL store, one file per ACP session id.
pub(crate) struct AgentJournal {
    directory: PathBuf,
    handles: Mutex<HashMap<String, OpenJournal>>,
}

impl AgentJournal {
    pub(crate) fn open(directory: &Path) -> Result<Self, JournalError> {
        fs::create_dir_all(directory)?;
        restrict_directory_to_current_user(directory)?;
        Ok(Self {
            directory: directory.to_path_buf(),
            handles: Mutex::new(HashMap::new()),
        })
    }

    /// Record one `session/update` payload; returns the seq of the record it
    /// landed in. A chunk merged into the record already open answers that
    /// record's seq, so the value stays the answer to "where does replay find
    /// this update" rather than a count of calls.
    pub(crate) fn append_for(
        &self,
        provider: AgentProvider,
        session_id: &str,
        update: &Value,
    ) -> Result<u64, JournalError> {
        self.record_into(provider, session_id, |journal| journal.record(update))
    }

    fn record_into(
        &self,
        provider: AgentProvider,
        session_id: &str,
        record: impl FnOnce(&mut OpenJournal) -> Result<u64, JournalError>,
    ) -> Result<u64, JournalError> {
        let key = journal_key(provider, session_id);
        let mut handles = self.handles.lock();
        if handles.len() >= MAX_OPEN_JOURNALS && !handles.contains_key(&key) {
            handles.clear();
        }
        let journal = match handles.entry(key.clone()) {
            Entry::Occupied(entry) => entry.into_mut(),
            Entry::Vacant(entry) => {
                let path = self.directory.join(journal_file_name(&key));
                let tail = scan(&path)?;
                let file = OpenOptions::new().create(true).append(true).open(&path)?;
                restrict_to_current_user(&path)?;
                entry.insert(OpenJournal {
                    file,
                    next_seq: tail.next_seq,
                    bytes: tail.bytes,
                    records: tail.records,
                    needs_newline: tail.needs_newline,
                    pending: None,
                })
            }
        };

        match record(journal) {
            Ok(seq) => Ok(seq),
            Err(error) => {
                if matches!(error, JournalError::Io(_)) {
                    handles.remove(&key);
                }
                Err(error)
            }
        }
    }

    /// Every recorded entry in order, torn or malformed lines skipped. A
    /// reader sees everything appended so far, so the record still being
    /// coalesced is written out before the file is read.
    pub(crate) fn replay_for(
        &self,
        provider: AgentProvider,
        session_id: &str,
    ) -> Result<Vec<(u64, JournalEntry)>, JournalError> {
        let key = journal_key(provider, session_id);
        {
            let mut handles = self.handles.lock();
            let flushed = handles
                .get_mut(&key)
                .map(OpenJournal::flush_pending)
                .transpose();
            if let Err(error) = flushed {
                handles.remove(&key);
                return Err(error);
            }
        }
        read_records(&self.directory.join(journal_file_name(&key)))
    }

    /// Seq of the last recorded update, 0 when nothing is journalled.
    #[cfg(test)]
    pub(crate) fn last_seq_for(
        &self,
        provider: AgentProvider,
        session_id: &str,
    ) -> Result<u64, JournalError> {
        let key = journal_key(provider, session_id);
        if let Some(journal) = self.handles.lock().get(&key) {
            return Ok(journal.next_seq.saturating_sub(1));
        }
        let tail = scan(&self.directory.join(journal_file_name(&key)))?;
        Ok(tail.next_seq.saturating_sub(1))
    }

    pub(crate) fn remove_for(
        &self,
        provider: AgentProvider,
        session_id: &str,
    ) -> Result<(), JournalError> {
        let key = journal_key(provider, session_id);
        if let Some(mut journal) = self.handles.lock().remove(&key) {
            journal.discard_pending();
        }
        match fs::remove_file(self.directory.join(journal_file_name(&key))) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        }
    }

    #[cfg(test)]
    pub(crate) fn append(&self, session_id: &str, update: &Value) -> Result<u64, JournalError> {
        self.append_for(AgentProvider::Codex, session_id, update)
    }

    #[cfg(test)]
    pub(crate) fn replay(
        &self,
        session_id: &str,
    ) -> Result<Vec<(u64, JournalEntry)>, JournalError> {
        self.replay_for(AgentProvider::Codex, session_id)
    }

    #[cfg(test)]
    pub(crate) fn last_seq(&self, session_id: &str) -> Result<u64, JournalError> {
        self.last_seq_for(AgentProvider::Codex, session_id)
    }

    #[cfg(test)]
    pub(crate) fn remove(&self, session_id: &str) -> Result<(), JournalError> {
        self.remove_for(AgentProvider::Codex, session_id)
    }

    /// Drop journals untouched for longer than `retain_days`; returns how many
    /// files went.
    pub(crate) fn prune(&self, retain_days: u64) -> Result<usize, JournalError> {
        let horizon = Duration::from_secs(retain_days.saturating_mul(SECONDS_PER_DAY));
        let Some(cutoff) = SystemTime::now().checked_sub(horizon) else {
            return Ok(0);
        };
        let mut handles = self.handles.lock();
        let mut removed = 0;
        let mut pruned = Vec::new();
        for entry in fs::read_dir(&self.directory)? {
            let path = entry?.path();
            if path.extension().and_then(OsStr::to_str) != Some(JOURNAL_EXTENSION) {
                continue;
            }
            let modified = fs::metadata(&path).and_then(|metadata| metadata.modified());
            let Ok(modified) = modified else { continue };
            if modified >= cutoff {
                continue;
            }
            if let Err(error) = fs::remove_file(&path) {
                log::warn!(
                    target: "zz::agent::journal",
                    "could not prune journal path={} error={error}",
                    path.display(),
                );
                continue;
            }
            if let Some(name) = path.file_name().and_then(OsStr::to_str) {
                pruned.push(name.to_owned());
            }
            removed += 1;
        }
        if removed > 0 {
            for (key, journal) in handles.iter_mut() {
                if pruned.contains(&journal_file_name(key)) {
                    journal.discard_pending();
                }
            }
            handles.clear();
        }
        Ok(removed)
    }
}

fn journal_key(provider: AgentProvider, session_id: &str) -> String {
    format!("{}:{session_id}", provider.as_str())
}

fn journal_file_name(session_id: &str) -> String {
    format!("{}.{JOURNAL_EXTENSION}", file_stem(session_id))
}

/// Session ids are adapter-controlled strings, so they never reach the
/// filesystem unfiltered: anything outside `[A-Za-z0-9_-]` is replaced, and an
/// id that needed filtering (or was too long) carries a digest of the original
/// so two different ids cannot land in one file.
fn file_stem(session_id: &str) -> String {
    let jailed: String = session_id
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_') {
                character
            } else {
                '_'
            }
        })
        .collect();
    if jailed == session_id && !jailed.is_empty() && jailed.len() <= MAX_STEM_BYTES {
        return jailed;
    }
    let digest = digest(session_id);
    let truncated: String = jailed.chars().take(MAX_STEM_BYTES).collect();
    format!("{truncated}-{digest:016x}")
}

/// FNV-1a, not `DefaultHasher`: the digest lands in a file name, so it has to
/// survive a toolchain bump.
fn digest(value: &str) -> u64 {
    const OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;

    value.bytes().fold(OFFSET_BASIS, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(PRIME)
    })
}

/// Seq, size and newline state of an existing journal, read from the tail so a
/// full file is never parsed just to find where to resume.
fn scan(path: &Path) -> Result<JournalTail, JournalError> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(JournalTail {
                next_seq: 1,
                bytes: 0,
                records: 0,
                needs_newline: false,
            });
        }
        Err(error) => return Err(error.into()),
    };
    let records = bytes
        .split(|byte| *byte == b'\n')
        .filter_map(|line| serde_json::from_slice::<StoredRecord>(line).ok())
        .collect::<Vec<_>>();
    let next_seq = records
        .last()
        .map_or(1, |record| record.seq.saturating_add(1));
    Ok(JournalTail {
        next_seq,
        bytes: u64::try_from(bytes.len()).unwrap_or(u64::MAX),
        records: records.len(),
        needs_newline: bytes.last().is_some_and(|byte| *byte != b'\n'),
    })
}

fn read_records(path: &Path) -> Result<Vec<(u64, JournalEntry)>, JournalError> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
    };
    let mut records = Vec::new();
    for line in bytes.split(|byte| *byte == b'\n') {
        if records.len() >= MAX_JOURNAL_RECORDS {
            break;
        }
        if line.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        let record = match serde_json::from_slice::<StoredRecord>(line) {
            Ok(record) => record,
            Err(error) => {
                log::warn!(
                    target: "zz::agent::journal",
                    "skipping malformed journal line path={} error={error}",
                    path.display(),
                );
                continue;
            }
        };
        let entry = match record.body {
            StoredBody::Update(update) => JournalEntry::Update(update),
            StoredBody::Task(task) => {
                drop(task);
                continue;
            }
        };
        records.push((record.seq, entry));
    }
    Ok(records)
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use tempfile::tempdir;

    use super::*;
    fn journaled(entry: &JournalEntry) -> &Value {
        match entry {
            JournalEntry::Update(update) => update,
        }
    }

    /// A chunk that never coalesces with another, because its id is its text.
    fn update(text: &str) -> Value {
        json!({
            "sessionUpdate": "agent_message_chunk",
            "messageId": text,
            "content": { "type": "text", "text": text }
        })
    }

    fn chunk(message_id: &str, text: &str) -> Value {
        json!({
            "sessionUpdate": "agent_message_chunk",
            "messageId": message_id,
            "content": { "type": "text", "text": text }
        })
    }

    fn thought(message_id: &str, text: &str) -> Value {
        json!({
            "sessionUpdate": "agent_thought_chunk",
            "messageId": message_id,
            "content": { "type": "text", "text": text }
        })
    }

    fn anonymous(text: &str) -> Value {
        json!({ "sessionUpdate": "agent_message_chunk", "content": { "type": "text", "text": text } })
    }

    fn tool_call(id: &str) -> Value {
        json!({ "sessionUpdate": "tool_call", "toolCallId": id, "status": "pending" })
    }

    fn texts(records: &[(u64, JournalEntry)]) -> Vec<String> {
        records
            .iter()
            .map(|(_, entry)| {
                journaled(entry)
                    .pointer("/content/text")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned()
            })
            .collect()
    }

    fn journal_entries(directory: &Path) -> Vec<PathBuf> {
        let mut entries: Vec<PathBuf> = fs::read_dir(directory)
            .expect("read journal directory")
            .map(|entry| entry.expect("directory entry").path())
            .collect();
        entries.sort();
        entries
    }

    fn journal_path(directory: &Path, session_id: &str) -> PathBuf {
        directory.join(journal_file_name(&journal_key(
            AgentProvider::Codex,
            session_id,
        )))
    }

    /// What is on disk right now, read past `replay_for` so a test can tell a
    /// record that was flushed from one a reader would have flushed for it.
    fn stored(directory: &Path, session_id: &str) -> Vec<(u64, JournalEntry)> {
        read_records(&journal_path(directory, session_id)).expect("read records")
    }

    /// The client reducer's grouping, at the JSON level: a chunk with an id
    /// joins the entry that id already owns, an id-less chunk extends the entry
    /// the same kind last wrote, and anything else ends the run.
    fn reduce(records: &[(u64, JournalEntry)]) -> Vec<(String, Option<String>, String)> {
        let mut entries: Vec<(String, Option<String>, String)> = Vec::new();
        let mut by_id: HashMap<(String, String), usize> = HashMap::new();
        let mut active: Option<usize> = None;
        for (_, entry) in records {
            let update = journaled(entry);
            let kind = update
                .get("sessionUpdate")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let text = update
                .pointer("/content/text")
                .and_then(Value::as_str)
                .filter(|_| matches!(kind, "agent_message_chunk" | "agent_thought_chunk"));
            let Some(text) = text else {
                active = None;
                continue;
            };
            let message_id = update.get("messageId").and_then(Value::as_str);
            let index = match message_id {
                Some(message_id) => *by_id
                    .entry((kind.to_owned(), message_id.to_owned()))
                    .or_insert_with(|| {
                        entries.push((kind.to_owned(), Some(message_id.to_owned()), String::new()));
                        entries.len() - 1
                    }),
                None => match active {
                    Some(index) if entries[index].0 == kind && entries[index].1.is_none() => index,
                    _ => {
                        entries.push((kind.to_owned(), None, String::new()));
                        entries.len() - 1
                    }
                },
            };
            entries[index].2.push_str(text);
            active = Some(index);
        }
        entries
    }

    #[test]
    fn appends_round_trip_in_order() {
        let directory = tempdir().expect("temporary directory");
        let journal = AgentJournal::open(directory.path()).expect("open journal");

        assert_eq!(journal.append("sess-1", &update("a")).expect("append"), 1);
        assert_eq!(journal.append("sess-1", &update("b")).expect("append"), 2);
        assert_eq!(
            journal.append("sess-2", &update("other")).expect("append"),
            1
        );

        let replayed = journal.replay("sess-1").expect("replay");
        assert_eq!(replayed.len(), 2);
        assert_eq!(replayed[0], (1, JournalEntry::Update(update("a"))));
        assert_eq!(replayed[1], (2, JournalEntry::Update(update("b"))));
        assert_eq!(journal.last_seq("sess-1").expect("last seq"), 2);
        assert_eq!(journal.last_seq("sess-2").expect("last seq"), 1);
        assert_eq!(journal.last_seq("sess-missing").expect("last seq"), 0);
        assert!(journal.replay("sess-missing").expect("replay").is_empty());

        journal.remove("sess-1").expect("remove");
        assert!(journal.replay("sess-1").expect("replay").is_empty());
        assert_eq!(journal.last_seq("sess-1").expect("last seq"), 0);
        journal.remove("sess-1").expect("remove is idempotent");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;

            let path = directory.path().join(journal_file_name(&journal_key(
                AgentProvider::Codex,
                "sess-2",
            )));
            assert_eq!(
                fs::metadata(path).expect("metadata").permissions().mode() & 0o777,
                0o600
            );
            assert_eq!(
                fs::metadata(directory.path())
                    .expect("directory metadata")
                    .permissions()
                    .mode()
                    & 0o777,
                0o700
            );
        }
    }

    #[test]
    fn seq_continues_across_reopen() {
        let directory = tempdir().expect("temporary directory");
        {
            let journal = AgentJournal::open(directory.path()).expect("open journal");
            journal.append("sess-1", &update("a")).expect("append");
            journal.append("sess-1", &update("b")).expect("append");
        }

        let journal = AgentJournal::open(directory.path()).expect("reopen journal");
        assert_eq!(journal.last_seq("sess-1").expect("last seq"), 2);
        assert_eq!(journal.append("sess-1", &update("c")).expect("append"), 3);
        let seqs: Vec<u64> = journal
            .replay("sess-1")
            .expect("replay")
            .into_iter()
            .map(|(seq, _)| seq)
            .collect();
        assert_eq!(seqs, vec![1, 2, 3]);
    }

    #[test]
    fn torn_trailing_line_is_tolerated() {
        let directory = tempdir().expect("temporary directory");
        let path = directory.path().join(journal_file_name(&journal_key(
            AgentProvider::Codex,
            "sess-1",
        )));
        {
            let journal = AgentJournal::open(directory.path()).expect("open journal");
            journal.append("sess-1", &update("a")).expect("append");
        }
        let mut file = OpenOptions::new().append(true).open(&path).expect("reopen");
        file.write_all(br#"{"seq":2,"update":{"sessionUpd"#)
            .expect("write torn line");
        drop(file);

        let journal = AgentJournal::open(directory.path()).expect("reopen journal");
        assert_eq!(journal.replay("sess-1").expect("replay").len(), 1);
        assert_eq!(journal.last_seq("sess-1").expect("last seq"), 1);
        assert_eq!(journal.append("sess-1", &update("b")).expect("append"), 2);

        let replayed = journal.replay("sess-1").expect("replay");
        assert_eq!(replayed.len(), 2);
        assert_eq!(replayed[1], (2, JournalEntry::Update(update("b"))));
        let contents = fs::read_to_string(&path).expect("read journal");
        assert!(contents.ends_with('\n'));
        assert_eq!(contents.lines().count(), 3);
    }

    #[test]
    fn hostile_session_ids_stay_inside_the_directory() {
        let directory = tempdir().expect("temporary directory");
        let journal = AgentJournal::open(directory.path()).expect("open journal");

        journal
            .append("../../escape", &update("escaped"))
            .expect("append");
        journal.append("a/b", &update("slash")).expect("append");
        journal
            .append("a_b", &update("underscore"))
            .expect("append");
        journal
            .append(&"x".repeat(512), &update("long"))
            .expect("append");

        let entries = journal_entries(directory.path());
        assert_eq!(entries.len(), 4);
        for path in &entries {
            assert_eq!(path.parent(), Some(directory.path()));
            let name = path.file_name().and_then(OsStr::to_str).expect("file name");
            assert!(!name.contains('/') && !name.contains('\\') && !name.starts_with('.'));
            assert!(name.len() <= MAX_STEM_BYTES + JOURNAL_EXTENSION.len() + 18);
        }
        assert_eq!(
            journaled(&journal.replay("a/b").expect("replay")[0].1),
            &update("slash")
        );
        assert_eq!(
            journaled(&journal.replay("a_b").expect("replay")[0].1),
            &update("underscore")
        );
        assert_eq!(file_stem("plain-id_9"), "plain-id_9");
    }

    #[test]
    fn append_is_refused_past_the_size_cap() {
        let directory = tempdir().expect("temporary directory");
        let path = directory.path().join(journal_file_name(&journal_key(
            AgentProvider::Codex,
            "sess-1",
        )));
        File::create(&path)
            .expect("create journal")
            .set_len(MAX_JOURNAL_BYTES)
            .expect("grow journal");

        let journal = AgentJournal::open(directory.path()).expect("open journal");
        let error = journal
            .append("sess-1", &update("a"))
            .expect_err("append past the cap");
        assert!(matches!(error, JournalError::Full { bytes, .. } if bytes == MAX_JOURNAL_BYTES));
        assert!(matches!(
            journal.append("sess-1", &update("b")),
            Err(JournalError::Full { .. })
        ));
        assert_eq!(
            fs::metadata(&path).expect("metadata").len(),
            MAX_JOURNAL_BYTES
        );
    }

    #[test]
    fn append_is_refused_past_the_record_cap() {
        let directory = tempdir().expect("temporary directory");
        let journal = AgentJournal::open(directory.path()).expect("open journal");
        for index in 0..MAX_JOURNAL_RECORDS {
            assert_eq!(
                journal
                    .append("sess-1", &update(&index.to_string()))
                    .expect("append"),
                u64::try_from(index).expect("index") + 1
            );
        }
        let error = journal
            .append("sess-1", &update("overflow"))
            .expect_err("append past the record cap");
        assert!(matches!(
            error,
            JournalError::Full { records, .. } if records == MAX_JOURNAL_RECORDS
        ));
        assert_eq!(
            journal.replay("sess-1").expect("replay").len(),
            MAX_JOURNAL_RECORDS
        );
    }

    #[test]
    fn providers_with_the_same_session_id_have_separate_journals() {
        let directory = tempdir().expect("temporary directory");
        let journal = AgentJournal::open(directory.path()).expect("open journal");
        journal
            .append_for(AgentProvider::Codex, "same", &update("codex"))
            .expect("append codex");
        journal
            .append_for(AgentProvider::ClaudeCode, "same", &update("claude"))
            .expect("append claude");

        assert_eq!(
            journaled(
                &journal
                    .replay_for(AgentProvider::Codex, "same")
                    .expect("replay codex")[0]
                    .1
            ),
            &update("codex")
        );
        assert_eq!(
            journaled(
                &journal
                    .replay_for(AgentProvider::ClaudeCode, "same")
                    .expect("replay claude")[0]
                    .1
            ),
            &update("claude")
        );
        assert_eq!(journal_entries(directory.path()).len(), 2);
    }

    #[test]
    fn prune_drops_journals_past_their_retention() {
        let directory = tempdir().expect("temporary directory");
        let journal = AgentJournal::open(directory.path()).expect("open journal");
        journal.append("sess-1", &update("a")).expect("append");
        fs::write(directory.path().join("unrelated.json"), b"{}").expect("write neighbour");

        assert_eq!(journal.prune(30).expect("prune"), 0);
        assert_eq!(journal.replay("sess-1").expect("replay").len(), 1);

        std::thread::sleep(Duration::from_millis(20));
        assert_eq!(journal.prune(0).expect("prune"), 1);
        assert!(journal.replay("sess-1").expect("replay").is_empty());
        assert_eq!(journal.append("sess-1", &update("b")).expect("append"), 1);
        assert!(directory.path().join("unrelated.json").exists());
    }

    #[test]
    fn adjacent_chunks_of_one_message_become_one_record() {
        let directory = tempdir().expect("temporary directory");
        let journal = AgentJournal::open(directory.path()).expect("open journal");

        for piece in ["Hel", "lo, ", "wor", "ld"] {
            journal
                .append("sess-1", &chunk("msg-1", piece))
                .expect("append");
        }
        journal
            .append("sess-1", &thought("think-1", "one "))
            .expect("append");
        journal
            .append("sess-1", &thought("think-1", "two"))
            .expect("append");

        let replayed = journal.replay("sess-1").expect("replay");
        assert_eq!(texts(&replayed), vec!["Hello, world", "one two"]);
        assert_eq!(journaled(&replayed[0].1), &chunk("msg-1", "Hello, world"));
        assert_eq!(journaled(&replayed[1].1), &thought("think-1", "one two"));
        assert_eq!(
            replayed.iter().map(|(seq, _)| *seq).collect::<Vec<_>>(),
            vec![1, 2]
        );
    }

    #[test]
    fn a_coalesced_record_consumes_one_seq() {
        let directory = tempdir().expect("temporary directory");
        let journal = AgentJournal::open(directory.path()).expect("open journal");

        assert_eq!(
            journal.append("sess-1", &chunk("msg-1", "a")).expect("a"),
            1
        );
        assert_eq!(
            journal.append("sess-1", &chunk("msg-1", "b")).expect("b"),
            1,
            "a merged chunk answers the seq of the record it joined"
        );
        assert_eq!(
            journal.append("sess-1", &chunk("msg-1", "c")).expect("c"),
            1
        );
        assert_eq!(journal.last_seq("sess-1").expect("last seq"), 1);
        assert_eq!(
            journal.append("sess-1", &tool_call("t-1")).expect("tool"),
            2
        );
        assert_eq!(
            journal.append("sess-1", &chunk("msg-2", "d")).expect("d"),
            3
        );
        assert_eq!(journal.last_seq("sess-1").expect("last seq"), 3);

        let replayed = journal.replay("sess-1").expect("replay");
        assert_eq!(
            replayed.iter().map(|(seq, _)| *seq).collect::<Vec<_>>(),
            vec![1, 2, 3],
            "seqs stay gapless and monotonic across a coalesced run"
        );
        assert_eq!(journal.last_seq("sess-1").expect("last seq"), 3);
    }

    #[test]
    fn chunks_merge_only_within_one_coalescing_identity() {
        let directory = tempdir().expect("temporary directory");
        let journal = AgentJournal::open(directory.path()).expect("open journal");

        let sequence = [
            chunk("msg-1", "a"),
            chunk("msg-1", "b"),
            chunk("msg-2", "c"),
            thought("msg-2", "d"),
            json!({
                "sessionUpdate": "agent_message_chunk",
                "messageId": "msg-3",
                "content": { "type": "image", "data": "AA==", "mimeType": "image/png" }
            }),
            json!({
                "sessionUpdate": "agent_message_chunk",
                "messageId": "msg-3",
                "content": { "type": "text", "text": "e", "annotations": { "audience": [] } }
            }),
            json!({
                "sessionUpdate": "agent_message_chunk",
                "messageId": 7,
                "content": { "type": "text", "text": "f" }
            }),
            json!({
                "sessionUpdate": "agent_message_chunk",
                "messageId": "msg-4",
                "content": { "text": "g" }
            }),
            json!({
                "sessionUpdate": "agent_message_chunk",
                "messageId": "msg-4",
                "content": { "type": "text", "text": "h" }
            }),
            json!({
                "sessionUpdate": "user_message_chunk",
                "messageId": "msg-5",
                "content": { "type": "text", "text": "i" }
            }),
            json!({
                "sessionUpdate": "user_message_chunk",
                "messageId": "msg-5",
                "content": { "type": "text", "text": "j" }
            }),
        ];
        for update in &sequence {
            journal.append("sess-1", update).expect("append");
        }

        let replayed = journal.replay("sess-1").expect("replay");
        assert_eq!(
            replayed.len(),
            sequence.len() - 1,
            "only the two chunks sharing every identity field merged"
        );
        assert_eq!(journaled(&replayed[0].1), &chunk("msg-1", "ab"));
        for (index, (_, entry)) in replayed.iter().enumerate().skip(1) {
            assert_eq!(
                journaled(entry),
                &sequence[index + 1],
                "record {index} is verbatim"
            );
        }
    }

    #[test]
    fn id_less_chunks_merge_only_while_their_run_lasts() {
        let directory = tempdir().expect("temporary directory");
        let journal = AgentJournal::open(directory.path()).expect("open journal");

        journal
            .append("sess-1", &anonymous("one "))
            .expect("append");
        journal.append("sess-1", &anonymous("two")).expect("append");
        journal
            .append("sess-1", &tool_call("t-1"))
            .expect("interrupt");
        journal
            .append("sess-1", &anonymous("three"))
            .expect("append");
        journal
            .append("sess-1", &chunk("msg-1", "four"))
            .expect("append");
        journal
            .append("sess-1", &anonymous("five"))
            .expect("append");

        let replayed = journal.replay("sess-1").expect("replay");
        assert_eq!(
            texts(&replayed),
            vec!["one two", "", "three", "four", "five"],
            "a tool call and an id change both end an id-less run"
        );
    }

    #[test]
    fn matching_metadata_merges_and_differing_metadata_does_not() {
        let directory = tempdir().expect("temporary directory");
        let journal = AgentJournal::open(directory.path()).expect("open journal");

        let with_meta = |parent: &str, text: &str| {
            json!({
                "sessionUpdate": "agent_message_chunk",
                "messageId": "msg-1",
                "_meta": { "claudeCode": { "parentToolUseId": parent } },
                "content": { "type": "text", "text": text }
            })
        };
        journal
            .append("sess-1", &with_meta("child-a", "a"))
            .expect("append");
        journal
            .append("sess-1", &with_meta("child-a", "b"))
            .expect("append");
        journal
            .append("sess-1", &with_meta("child-b", "c"))
            .expect("append");
        journal
            .append("sess-1", &chunk("msg-1", "d"))
            .expect("append");

        let replayed = journal.replay("sess-1").expect("replay");
        assert_eq!(texts(&replayed), vec!["ab", "c", "d"]);
        assert_eq!(journaled(&replayed[0].1), &with_meta("child-a", "ab"));
    }

    #[test]
    fn replay_flushes_the_open_record() {
        let directory = tempdir().expect("temporary directory");
        let journal = AgentJournal::open(directory.path()).expect("open journal");
        journal.append("sess-1", &chunk("msg-1", "a")).expect("a");
        journal.append("sess-1", &chunk("msg-1", "b")).expect("b");

        assert!(
            stored(directory.path(), "sess-1").is_empty(),
            "the record is still being coalesced"
        );
        assert_eq!(
            texts(&journal.replay("sess-1").expect("replay")),
            vec!["ab"]
        );
        assert_eq!(
            texts(&stored(directory.path(), "sess-1")),
            vec!["ab"],
            "replay put it on disk before reading"
        );

        journal.append("sess-1", &chunk("msg-1", "c")).expect("c");
        assert_eq!(
            texts(&journal.replay("sess-1").expect("replay")),
            vec!["ab", "c"],
            "a flushed record never reopens for later chunks of the same message"
        );
    }

    #[test]
    fn dropping_the_journal_flushes_the_open_record() {
        let directory = tempdir().expect("temporary directory");
        {
            let journal = AgentJournal::open(directory.path()).expect("open journal");
            journal.append("sess-1", &chunk("msg-1", "a")).expect("a");
            journal.append("sess-1", &chunk("msg-1", "b")).expect("b");
            assert!(stored(directory.path(), "sess-1").is_empty());
        }
        assert_eq!(texts(&stored(directory.path(), "sess-1")), vec!["ab"]);

        let journal = AgentJournal::open(directory.path()).expect("reopen journal");
        assert_eq!(
            journal.append("sess-1", &chunk("msg-1", "c")).expect("c"),
            2
        );
    }

    #[test]
    fn evicting_a_handle_flushes_its_open_record() {
        let directory = tempdir().expect("temporary directory");
        let journal = AgentJournal::open(directory.path()).expect("open journal");
        for index in 0..=MAX_OPEN_JOURNALS {
            let session = format!("sess-{index}");
            journal
                .append(&session, &chunk("msg-1", "a"))
                .expect("append");
        }

        assert_eq!(
            texts(&stored(directory.path(), "sess-0")),
            vec!["a"],
            "eviction wrote the record out instead of dropping it"
        );
        assert_eq!(
            journal.last_seq("sess-0").expect("last seq"),
            1,
            "the evicted journal resumes from what eviction wrote"
        );
    }

    #[test]
    fn removal_and_prune_drop_the_open_record_with_its_file() {
        let directory = tempdir().expect("temporary directory");
        let journal = AgentJournal::open(directory.path()).expect("open journal");
        journal
            .append("sess-1", &chunk("msg-1", "a"))
            .expect("append");
        journal.remove("sess-1").expect("remove");
        assert!(!journal_path(directory.path(), "sess-1").exists());
        assert!(journal.replay("sess-1").expect("replay").is_empty());

        journal
            .append("sess-2", &chunk("msg-1", "b"))
            .expect("append");
        std::thread::sleep(Duration::from_millis(20));
        assert_eq!(journal.prune(0).expect("prune"), 1);
        drop(journal);
        assert!(
            !journal_path(directory.path(), "sess-2").exists(),
            "a pruned journal must not be recreated by its own open record"
        );
    }

    #[test]
    fn a_runaway_message_is_flushed_at_the_pending_bound() {
        let directory = tempdir().expect("temporary directory");
        let journal = AgentJournal::open(directory.path()).expect("open journal");
        let piece = "z".repeat(4096);
        for _ in 0..200 {
            journal
                .append("sess-1", &chunk("msg-1", &piece))
                .expect("append");
        }

        let on_disk = stored(directory.path(), "sess-1");
        assert!(
            on_disk.len() >= 3,
            "800 KiB of one message must not sit in memory as one record, got {} records",
            on_disk.len()
        );
        let replayed = journal.replay("sess-1").expect("replay");
        let bound = usize::try_from(MAX_PENDING_RECORD_BYTES).expect("bound") + piece.len();
        for (_, entry) in &replayed {
            let length = serde_json::to_vec(journaled(entry)).expect("encode").len();
            assert!(length < bound, "record of {length} bytes past the bound");
        }
        assert_eq!(
            texts(&replayed).concat().len(),
            200 * piece.len(),
            "splitting a runaway message loses nothing"
        );
    }

    #[test]
    fn coalesced_length_accounting_matches_the_encoder() {
        let addition = "\"\\ \n\r\t\u{0b}\u{7f} café 🙂 \u{1}\u{1f}";
        let before = serde_json::to_vec(&JournalRecord {
            seq: 4_096,
            body: RecordBody::Update(&chunk("msg-1", "plain")),
        })
        .expect("encode");
        let after = serde_json::to_vec(&JournalRecord {
            seq: 4_096,
            body: RecordBody::Update(&chunk("msg-1", &format!("plain{addition}"))),
        })
        .expect("encode");

        assert_eq!(
            u64::try_from(after.len() - before.len()).expect("growth"),
            escaped_length(addition),
            "the reserved byte budget has to be exact, not merely close"
        );
    }

    #[test]
    fn coalescing_never_overshoots_the_size_cap() {
        let directory = tempdir().expect("temporary directory");
        let path = journal_path(directory.path(), "sess-1");
        File::create(&path)
            .expect("create journal")
            .set_len(MAX_JOURNAL_BYTES - 256)
            .expect("grow journal");

        let journal = AgentJournal::open(directory.path()).expect("open journal");
        journal
            .append("sess-1", &chunk("msg-1", "seed"))
            .expect("the first chunk still fits");
        let refused = (0..64).any(|_| {
            matches!(
                journal.append("sess-1", &chunk("msg-1", "0123456789")),
                Err(JournalError::Full { .. })
            )
        });

        assert!(
            refused,
            "a merge past the cap is refused before it is taken"
        );
        journal.replay("sess-1").expect("replay");
        assert!(
            fs::metadata(&path).expect("metadata").len() <= MAX_JOURNAL_BYTES,
            "flushing a reserved record can never cross the cap"
        );
    }

    #[test]
    fn an_open_record_holds_its_slot_against_the_record_cap() {
        let directory = tempdir().expect("temporary directory");
        let journal = AgentJournal::open(directory.path()).expect("open journal");
        for index in 0..MAX_JOURNAL_RECORDS - 1 {
            journal
                .append("sess-1", &update(&index.to_string()))
                .expect("append");
        }

        let seq = u64::try_from(MAX_JOURNAL_RECORDS).expect("cap");
        assert_eq!(
            journal.append("sess-1", &chunk("live", "a")).expect("a"),
            seq
        );
        assert_eq!(
            journal.append("sess-1", &chunk("live", "b")).expect("b"),
            seq,
            "merging into the open record does not claim another slot"
        );
        assert!(matches!(
            journal.append("sess-1", &tool_call("t-1")),
            Err(JournalError::Full { records, .. }) if records == MAX_JOURNAL_RECORDS
        ));

        let replayed = journal.replay("sess-1").expect("replay");
        assert_eq!(replayed.len(), MAX_JOURNAL_RECORDS);
        assert_eq!(texts(&replayed).last().map(String::as_str), Some("ab"));
    }

    #[test]
    fn a_coalesced_journal_reduces_like_the_chunks_it_replaced() {
        let directory = tempdir().expect("temporary directory");
        let journal = AgentJournal::open(directory.path()).expect("open journal");

        let mut sequence = Vec::new();
        for index in 0..3 {
            let message = format!("msg-{index}");
            for token in 0..40 {
                sequence.push(thought(&message, &format!("t{token} ")));
            }
            for token in 0..120 {
                sequence.push(chunk(&message, &format!("w{token} ")));
            }
            sequence.push(tool_call(&format!("tool-{index}")));
            sequence.push(anonymous("trailing "));
            sequence.push(anonymous("prose"));
            sequence.push(chunk(&message, " reopened"));
        }
        for update in &sequence {
            journal.append("sess-1", update).expect("append");
        }

        let verbatim: Vec<(u64, JournalEntry)> = sequence
            .iter()
            .enumerate()
            .map(|(index, update)| {
                (
                    u64::try_from(index).expect("index") + 1,
                    JournalEntry::Update(update.clone()),
                )
            })
            .collect();
        let replayed = journal.replay("sess-1").expect("replay");

        assert_eq!(
            reduce(&replayed),
            reduce(&verbatim),
            "a coalesced journal must reduce to exactly what the chunk stream did"
        );
        assert!(
            replayed.len() < verbatim.len() / 20,
            "{} records for {} updates is not a coalesced journal",
            replayed.len(),
            verbatim.len()
        );
    }

    #[test]
    fn a_token_stream_collapses_to_one_record_per_message() {
        let directory = tempdir().expect("temporary directory");
        let journal = AgentJournal::open(directory.path()).expect("open journal");

        let mut updates = 0;
        for turn in 0..8 {
            let message = format!("msg-{turn}");
            for token in 0..50 {
                journal
                    .append("sess-1", &thought(&message, &format!("r{token} ")))
                    .expect("append");
                updates += 1;
            }
            for token in 0..200 {
                journal
                    .append("sess-1", &chunk(&message, &format!("t{token} ")))
                    .expect("append");
                updates += 1;
            }
            for call in 0..3 {
                journal
                    .append("sess-1", &tool_call(&format!("tool-{turn}-{call}")))
                    .expect("append");
                updates += 1;
            }
        }

        let replayed = journal.replay("sess-1").expect("replay");
        assert_eq!(updates, 2_024);
        assert_eq!(
            replayed.len(),
            40,
            "8 turns of thought, prose and three tool calls each"
        );
        assert!(
            fs::metadata(journal_path(directory.path(), "sess-1"))
                .expect("metadata")
                .len()
                < 16 * 1024,
            "one record per chunk would be 245 KiB of journal for this session"
        );
    }

    #[test]
    fn a_journal_of_updates_alone_reads_exactly_as_it_did() {
        let directory = tempdir().expect("temporary directory");
        let path = journal_path(directory.path(), "sess-1");
        fs::write(
            &path,
            format!(
                "{}\n{}\n",
                json!({"seq": 1, "update": update("a")}),
                json!({"seq": 2, "update": tool_call("t-1")}),
            ),
        )
        .expect("write journal");

        let journal = AgentJournal::open(directory.path()).expect("open journal");
        assert_eq!(
            journal.replay("sess-1").expect("replay"),
            vec![
                (1, JournalEntry::Update(update("a"))),
                (2, JournalEntry::Update(tool_call("t-1"))),
            ]
        );
        assert_eq!(journal.last_seq("sess-1").expect("last seq"), 2);
        assert_eq!(
            journal.append("sess-1", &update("b")).expect("append"),
            3,
            "an old journal keeps counting where it left off"
        );
    }

    #[test]
    fn legacy_task_records_are_skipped_without_hiding_later_updates() {
        let directory = tempdir().expect("temporary directory");
        let path = journal_path(directory.path(), "sess-1");
        fs::write(
            &path,
            format!(
                "{}\n{}\n{}\n",
                json!({"seq": 1, "update": update("a")}),
                json!({"seq": 2, "task": {"kind": "invented-later", "task_id": "t-1"}}),
                json!({"seq": 3, "update": update("b")}),
            ),
        )
        .expect("write journal");

        let journal = AgentJournal::open(directory.path()).expect("open journal");
        assert_eq!(
            journal.replay("sess-1").expect("replay"),
            vec![
                (1, JournalEntry::Update(update("a"))),
                (3, JournalEntry::Update(update("b"))),
            ]
        );
        assert_eq!(
            journal.last_seq("sess-1").expect("last seq"),
            3,
            "a legacy record still holds its place in the sequence"
        );
    }
}
