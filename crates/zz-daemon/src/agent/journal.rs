//! Append-only per-pane JSONL journal of ACP session traffic, for replay when
//! `session/load` is unavailable.
//!
//! One file per ACP session id under a caller-supplied directory; every line is
//! `{"seq": n, "update": <session/update payload>}` with `seq` counting from 1
//! and flushed before the append returns. Adapters own the session-id string,
//! so it is jailed into a file stem before it reaches the filesystem, and a
//! crash mid-write leaves a torn trailing line that readers skip and the next
//! append isolates behind a fresh newline.

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

#[derive(Serialize)]
struct JournalRecord<'a> {
    seq: u64,
    update: &'a Value,
}

#[derive(Deserialize)]
struct StoredRecord {
    seq: u64,
    update: Value,
}

struct OpenJournal {
    file: File,
    next_seq: u64,
    bytes: u64,
    records: usize,
    needs_newline: bool,
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

    /// Record one `session/update` payload; returns the seq it was written as.
    pub(crate) fn append_for(
        &self,
        provider: AgentProvider,
        session_id: &str,
        update: &Value,
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
                })
            }
        };

        let seq = journal.next_seq;
        let record = serde_json::to_vec(&JournalRecord { seq, update })?;
        let mut line = Vec::with_capacity(record.len() + 2);
        if journal.needs_newline {
            line.push(b'\n');
        }
        line.extend_from_slice(&record);
        line.push(b'\n');
        let written = u64::try_from(line.len()).unwrap_or(u64::MAX);
        if journal.bytes.saturating_add(written) > MAX_JOURNAL_BYTES
            || journal.records >= MAX_JOURNAL_RECORDS
        {
            return Err(JournalError::Full {
                bytes: journal.bytes,
                records: journal.records,
            });
        }

        // A half-written line leaves the cached seq, length and newline state
        // wrong, so the handle is dropped and the next append rescans the tail.
        if let Err(error) = journal
            .file
            .write_all(&line)
            .and_then(|()| journal.file.flush())
        {
            handles.remove(&key);
            return Err(error.into());
        }
        journal.needs_newline = false;
        journal.bytes = journal.bytes.saturating_add(written);
        journal.records = journal.records.saturating_add(1);
        journal.next_seq = seq.saturating_add(1);
        Ok(seq)
    }

    /// Every recorded update in order, torn or malformed lines skipped.
    pub(crate) fn replay_for(
        &self,
        provider: AgentProvider,
        session_id: &str,
    ) -> Result<Vec<(u64, Value)>, JournalError> {
        read_records(
            &self
                .directory
                .join(journal_file_name(&journal_key(provider, session_id))),
        )
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
        self.handles.lock().remove(&key);
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
    pub(crate) fn replay(&self, session_id: &str) -> Result<Vec<(u64, Value)>, JournalError> {
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
            removed += 1;
        }
        if removed > 0 {
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

fn read_records(path: &Path) -> Result<Vec<(u64, Value)>, JournalError> {
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
        match serde_json::from_slice::<StoredRecord>(line) {
            Ok(record) => records.push((record.seq, record.update)),
            Err(error) => log::warn!(
                target: "zz::agent::journal",
                "skipping malformed journal line path={} error={error}",
                path.display(),
            ),
        }
    }
    Ok(records)
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use tempfile::tempdir;

    use super::*;

    fn update(text: &str) -> Value {
        json!({ "sessionUpdate": "agent_message_chunk", "content": { "text": text } })
    }

    fn journal_entries(directory: &Path) -> Vec<PathBuf> {
        let mut entries: Vec<PathBuf> = fs::read_dir(directory)
            .expect("read journal directory")
            .map(|entry| entry.expect("directory entry").path())
            .collect();
        entries.sort();
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
        assert_eq!(replayed[0], (1, update("a")));
        assert_eq!(replayed[1], (2, update("b")));
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
        assert_eq!(replayed[1], (2, update("b")));
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
        assert_eq!(journal.replay("a/b").expect("replay")[0].1, update("slash"));
        assert_eq!(
            journal.replay("a_b").expect("replay")[0].1,
            update("underscore")
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
                journal.append("sess-1", &update("x")).expect("append"),
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
            journal
                .replay_for(AgentProvider::Codex, "same")
                .expect("replay codex")[0]
                .1,
            update("codex")
        );
        assert_eq!(
            journal
                .replay_for(AgentProvider::ClaudeCode, "same")
                .expect("replay claude")[0]
                .1,
            update("claude")
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
}
