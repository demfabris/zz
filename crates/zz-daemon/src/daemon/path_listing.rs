use super::*;
use std::{collections::HashSet, path::Component, sync::atomic::AtomicUsize};

use ignore::WalkBuilder;
use zz_protocol::{
    GitMark, InsertStyle, MAX_PATH_LIST_CHUNK_BYTES, MAX_PATH_LIST_DEPTH, MAX_PATH_LIST_ENTRIES,
    MAX_PATH_LIST_TEXT_BYTES, PathEntry, PathKind, PathListRoot, ShellKind,
};

const PATH_LIST_BUDGET: Duration = Duration::from_secs(3);
const PATH_LIST_HIGH_WATER_BYTES: usize = 256 * 1024;
const PATH_LIST_POLL_INTERVAL: Duration = Duration::from_millis(1);
const PATH_LIST_MAX_POLL_INTERVAL: Duration = Duration::from_millis(50);
const PATH_LIST_STALL_LIMIT: Duration = Duration::from_secs(5);
const PATH_LIST_TURN_POLL: Duration = Duration::from_millis(10);
const PATH_LIST_FLUSH_INTERVAL: Duration = Duration::from_millis(30);
const PATH_LIST_MESSAGE_OVERHEAD: usize = 32;
const PATH_ENTRY_OVERHEAD: usize = 8;
const MAX_PATH_LIST_WALKERS: usize = 4;
const GIT_MARK_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_GIT_MARK_OUTPUT_BYTES: usize = 2 * 1024 * 1024;
const LISTED_ONLY_NAMES: &[&str] = &["node_modules", "target", "__pycache__", "venv"];
const HOME_MEDIA_ROOTS: &[&str] = &[
    "Applications",
    "Library",
    "Movies",
    "Music",
    "Pictures",
    "Public",
    "Templates",
    "Videos",
    "snap",
];
const REMOTE_FOREGROUND_COMMANDS: &[&str] = &["ssh", "mosh", "mosh-client", "tmux", "zz_cli"];
const PROMPT_SHELLS: &[&str] = &[
    "sh",
    "bash",
    "zsh",
    "fish",
    "dash",
    "ksh",
    "nu",
    "pwsh",
    "powershell",
];

static LIVE_PATH_WALKERS: AtomicUsize = AtomicUsize::new(0);

pub fn path_walk_enters(relative: &Path, home_root_child: bool) -> bool {
    let name = relative
        .file_name()
        .and_then(OsStr::to_str)
        .unwrap_or_default();
    !(LISTED_ONLY_NAMES.contains(&name)
        || relative.ends_with("go/pkg/mod")
        || home_root_child && HOME_MEDIA_ROOTS.contains(&name))
}

struct PathWalkerSlot(&'static AtomicUsize);

impl PathWalkerSlot {
    fn acquire(counter: &'static AtomicUsize) -> Option<Self> {
        counter
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |live| {
                (live < MAX_PATH_LIST_WALKERS).then_some(live + 1)
            })
            .ok()
            .map(|_| Self(counter))
    }
}

impl Drop for PathWalkerSlot {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

#[derive(Clone, Copy, Debug)]
struct WalkLimits {
    depth: usize,
    entries: usize,
    budget: Duration,
}

impl Default for WalkLimits {
    fn default() -> Self {
        Self {
            depth: MAX_PATH_LIST_DEPTH,
            entries: MAX_PATH_LIST_ENTRIES,
            budget: PATH_LIST_BUDGET,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WalkEnd {
    Complete,
    Truncated,
    Stopped,
}

fn listable_text(text: &str) -> bool {
    !text.chars().any(char::is_control)
}

fn listable_name(name: &OsStr) -> bool {
    name.to_str()
        .is_some_and(|name| listable_text(name) && !matches!(name, ".git" | ".jj"))
}

fn wire_relative(relative: &Path) -> Option<String> {
    let text = relative.to_str()?;
    #[cfg(windows)]
    let text = text.replace('\\', "/");
    #[cfg(not(windows))]
    let text = text.to_owned();
    (!text.is_empty() && text.len() <= MAX_PATH_LIST_TEXT_BYTES && listable_text(&text))
        .then_some(text)
}

#[cfg(unix)]
fn device_of(_path: &Path, metadata: &fs::Metadata) -> u64 {
    use std::os::unix::fs::MetadataExt as _;

    metadata.dev()
}

#[cfg(not(unix))]
fn device_of(_path: &Path, _metadata: &fs::Metadata) -> u64 {
    0
}

fn walk_paths(
    root: &Path,
    root_is_home: bool,
    limits: WalkLimits,
    device: &dyn Fn(&Path, &fs::Metadata) -> u64,
    cancelled: &dyn Fn() -> bool,
    emit: &mut dyn FnMut(PathEntry) -> bool,
) -> WalkEnd {
    let started = Instant::now();
    let inside_repo = root
        .ancestors()
        .any(|directory| directory.join(".git").exists());
    let children_only = root.parent().is_none();
    let root_device = fs::metadata(root)
        .ok()
        .map(|metadata| device(root, &metadata));
    let mut pending = VecDeque::from([(root.to_path_buf(), 0_usize)]);
    let mut emitted = 0_usize;
    let mut depth_capped = false;
    while let Some((directory, depth)) = pending.pop_front() {
        if cancelled() {
            return WalkEnd::Stopped;
        }
        let mut builder = WalkBuilder::new(&directory);
        builder
            .follow_links(false)
            .max_depth(Some(1))
            .hidden(!inside_repo)
            .git_ignore(true)
            .git_exclude(true)
            .git_global(true)
            .filter_entry(|entry| listable_name(entry.file_name()));
        for result in builder.build() {
            if cancelled() {
                return WalkEnd::Stopped;
            }
            if emitted >= limits.entries || started.elapsed() >= limits.budget {
                return WalkEnd::Truncated;
            }
            let Ok(entry) = result else { continue };
            if entry.depth() == 0 {
                continue;
            }
            let Ok(relative) = entry.path().strip_prefix(root) else {
                continue;
            };
            let Some(rel) = wire_relative(relative) else {
                continue;
            };
            let symlink = entry.path_is_symlink();
            let directory_entry = if symlink {
                fs::metadata(entry.path()).is_ok_and(|metadata| metadata.is_dir())
            } else {
                entry.file_type().is_some_and(|kind| kind.is_dir())
            };
            if directory_entry
                && !symlink
                && !children_only
                && path_walk_enters(relative, depth == 0 && root_is_home)
                && (root_device.is_none()
                    || entry
                        .metadata()
                        .ok()
                        .map(|metadata| device(entry.path(), &metadata))
                        == root_device)
            {
                if depth + 1 < limits.depth {
                    pending.push_back((entry.path().to_path_buf(), depth + 1));
                } else {
                    depth_capped = true;
                }
            }
            emitted += 1;
            if !emit(PathEntry {
                rel,
                kind: if directory_entry {
                    PathKind::Dir
                } else {
                    PathKind::File
                },
                symlink,
            }) {
                return WalkEnd::Stopped;
            }
        }
    }
    if depth_capped {
        WalkEnd::Truncated
    } else {
        WalkEnd::Complete
    }
}

fn classify_git_status(x: u8, y: u8) -> Option<GitMark> {
    if x == b'?' && y == b'?' {
        return Some(GitMark::Untracked);
    }
    if x == b'U' || y == b'U' || (x, y) == (b'A', b'A') || (x, y) == (b'D', b'D') {
        return Some(GitMark::Conflicted);
    }
    if x == b'A' {
        return Some(GitMark::Added);
    }
    (matches!(x, b'M' | b'T' | b'A') || matches!(y, b'M' | b'T' | b'A'))
        .then_some(GitMark::Modified)
}

fn parse_git_status(output: &[u8], prefix: &str) -> Vec<(String, GitMark)> {
    output
        .split(|byte| *byte == 0)
        .filter_map(|record| {
            if record.len() < 4 || record[2] != b' ' {
                return None;
            }
            let mark = classify_git_status(record[0], record[1])?;
            let path = std::str::from_utf8(&record[3..]).ok()?;
            let rel = path.strip_prefix(prefix)?;
            (!rel.is_empty() && rel.len() <= MAX_PATH_LIST_TEXT_BYTES && listable_text(rel))
                .then(|| (rel.to_owned(), mark))
        })
        .collect()
}

fn git_command(root: &Path, args: &[&str]) -> std::process::Command {
    let mut command = std::process::Command::new("git");
    command
        .args(["-c", "core.fsmonitor=false"])
        .arg("-C")
        .arg(root)
        .args(args)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .stdin(Stdio::null());
    command
}

fn git_filter_drivers(names: &[u8]) -> Option<BTreeSet<&str>> {
    let mut drivers = BTreeSet::new();
    for key in names.split(|byte| *byte == 0).filter(|key| !key.is_empty()) {
        let key = std::str::from_utf8(key).ok()?;
        let (driver, _) = key.strip_prefix("filter.")?.rsplit_once('.')?;
        if driver.contains('=') {
            return None;
        }
        drivers.insert(driver);
    }
    Some(drivers)
}

fn git_marks(root: &Path, cancelled: &dyn Fn() -> bool) -> Vec<(String, GitMark)> {
    let deadline = Instant::now() + GIT_MARK_TIMEOUT;
    let run = |args: &[&str]| {
        crate::bounded_command::run_output_until_cancelled(
            git_command(root, args),
            MAX_GIT_MARK_OUTPUT_BYTES,
            deadline,
            cancelled,
        )
        .ok()
    };
    let Some(prefix) = run(&["rev-parse", "--show-prefix"]) else {
        return Vec::new();
    };
    if !prefix.status.success() {
        return Vec::new();
    }
    let Ok(prefix) = String::from_utf8(prefix.stdout) else {
        return Vec::new();
    };
    let Some(filters) = run(&[
        "config",
        "-z",
        "--name-only",
        "--get-regexp",
        r"^filter\..*\.(clean|process)$",
    ]) else {
        return Vec::new();
    };
    if !(filters.status.success() || filters.status.code() == Some(1)) {
        return Vec::new();
    }
    let Some(drivers) = git_filter_drivers(&filters.stdout) else {
        return Vec::new();
    };
    let overrides = drivers
        .iter()
        .flat_map(|driver| {
            [
                format!("filter.{driver}.clean="),
                format!("filter.{driver}.process="),
                format!("filter.{driver}.required=false"),
            ]
        })
        .collect::<Vec<_>>();
    let mut args = overrides
        .iter()
        .flat_map(|setting| ["-c", setting.as_str()])
        .collect::<Vec<_>>();
    args.extend([
        "status",
        "--porcelain=v1",
        "-z",
        "--no-renames",
        "--ignore-submodules=dirty",
        "--",
        ".",
    ]);
    let Some(status) = run(&args) else {
        return Vec::new();
    };
    if !status.status.success() {
        return Vec::new();
    }
    parse_git_status(&status.stdout, prefix.trim_end_matches(['\n', '\r']))
}

fn entry_wire_bytes(entry: &PathEntry) -> usize {
    entry.rel.len() + PATH_ENTRY_OVERHEAD
}

struct PathListStream<'a> {
    outbound: &'a OutboundMailbox,
    cancel: &'a AtomicBool,
    request_id: u64,
}

impl PathListStream<'_> {
    fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Acquire)
    }

    fn send(&self, message: &ProtocolMessage) -> bool {
        let mut pause = PATH_LIST_POLL_INTERVAL;
        let mut lowest = usize::MAX;
        let mut progressed = Instant::now();
        loop {
            if self.cancelled() {
                return false;
            }
            match self.outbound.queued_reliable() {
                None => return false,
                Some((bytes, messages))
                    if messages < CONTROL_PENDING_MESSAGE_LIMIT
                        && bytes < PATH_LIST_HIGH_WATER_BYTES =>
                {
                    break;
                }
                Some((bytes, _)) => {
                    if bytes < lowest {
                        lowest = bytes;
                        progressed = Instant::now();
                    } else if progressed.elapsed() >= PATH_LIST_STALL_LIMIT {
                        return false;
                    }
                    thread::sleep(pause);
                    pause = (pause * 2).min(PATH_LIST_MAX_POLL_INTERVAL);
                }
            }
        }
        self.outbound.enqueue_reliable(message)
    }

    fn begin(&self, result: Result<PathListRoot, String>) -> bool {
        self.send(&ProtocolMessage::PathListBegin {
            request_id: self.request_id,
            result,
        })
    }

    fn chunk(&self, entries: Vec<PathEntry>, done: bool, truncated: bool) -> bool {
        self.send(&ProtocolMessage::PathListChunk {
            request_id: self.request_id,
            entries,
            done,
            truncated,
        })
    }

    fn git(&self, marks: Vec<(String, GitMark)>) -> bool {
        let mut batch = Vec::new();
        let mut bytes = PATH_LIST_MESSAGE_OVERHEAD;
        for (rel, mark) in marks {
            let size = rel.len() + PATH_ENTRY_OVERHEAD;
            if !batch.is_empty() && bytes + size > MAX_PATH_LIST_CHUNK_BYTES {
                if !self.send(&ProtocolMessage::PathListGit {
                    request_id: self.request_id,
                    marks: std::mem::take(&mut batch),
                }) {
                    return false;
                }
                bytes = PATH_LIST_MESSAGE_OVERHEAD;
            }
            bytes += size;
            batch.push((rel, mark));
        }
        batch.is_empty()
            || self.send(&ProtocolMessage::PathListGit {
                request_id: self.request_id,
                marks: batch,
            })
    }

    fn wait_for_git(
        &self,
        marks: &mpsc::Receiver<Vec<(String, GitMark)>>,
    ) -> Option<Vec<(String, GitMark)>> {
        loop {
            if self.cancelled() {
                return None;
            }
            match marks.recv_timeout(Duration::from_millis(10)) {
                Ok(marks) => return Some(marks),
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => return Some(Vec::new()),
            }
        }
    }

    fn walk(
        &self,
        root: &Path,
        root_is_home: bool,
        limits: WalkLimits,
        marks: Option<&mpsc::Receiver<Vec<(String, GitMark)>>>,
    ) -> bool {
        let mut batch = Vec::new();
        let mut bytes = PATH_LIST_MESSAGE_OVERHEAD;
        let mut last_flush = Instant::now();
        let mut sent = HashSet::new();
        let end = walk_paths(
            root,
            root_is_home,
            limits,
            &device_of,
            &|| self.cancelled(),
            &mut |entry| {
                let size = entry_wire_bytes(&entry);
                if !batch.is_empty()
                    && (bytes + size > MAX_PATH_LIST_CHUNK_BYTES
                        || last_flush.elapsed() >= PATH_LIST_FLUSH_INTERVAL)
                {
                    if !self.chunk(std::mem::take(&mut batch), false, false) {
                        return false;
                    }
                    bytes = PATH_LIST_MESSAGE_OVERHEAD;
                    last_flush = Instant::now();
                }
                if marks.is_some() {
                    sent.insert(entry.rel.clone());
                }
                bytes += size;
                batch.push(entry);
                true
            },
        );
        if end == WalkEnd::Stopped {
            return false;
        }
        let truncated = end == WalkEnd::Truncated;
        let marks = match marks {
            Some(marks) => match self.wait_for_git(marks) {
                Some(marks) => marks,
                None => return false,
            },
            None => Vec::new(),
        };
        let marks = marks
            .into_iter()
            .filter(|(rel, _)| sent.contains(rel.trim_end_matches('/')))
            .collect::<Vec<_>>();
        if marks.is_empty() {
            return self.chunk(batch, true, truncated);
        }
        (batch.is_empty() || self.chunk(batch, false, false))
            && self.git(marks)
            && self.chunk(Vec::new(), true, truncated)
    }
}

fn percent_decode(bytes: &[u8]) -> Vec<u8> {
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%'
            && let Some(high) = bytes
                .get(index + 1)
                .and_then(|byte| (*byte as char).to_digit(16))
            && let Some(low) = bytes
                .get(index + 2)
                .and_then(|byte| (*byte as char).to_digit(16))
        {
            decoded.push(u8::try_from(high * 16 + low).unwrap_or_default());
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    decoded
}

fn strip_drive_slash(bytes: &[u8]) -> &[u8] {
    if bytes.len() >= 3 && bytes[0] == b'/' && bytes[1].is_ascii_alphabetic() && bytes[2] == b':' {
        &bytes[1..]
    } else {
        bytes
    }
}

#[cfg(unix)]
fn path_from_bytes(bytes: &[u8]) -> PathBuf {
    use std::os::unix::ffi::OsStrExt as _;

    PathBuf::from(OsStr::from_bytes(bytes))
}

#[cfg(not(unix))]
fn path_from_bytes(bytes: &[u8]) -> PathBuf {
    PathBuf::from(String::from_utf8_lossy(bytes).into_owned())
}

fn osc7_candidates(reported: &str) -> Vec<PathBuf> {
    let Some(rest) = reported
        .strip_prefix("file://")
        .or_else(|| reported.strip_prefix("kitty-shell-cwd://"))
    else {
        return Vec::new();
    };
    let Some(slash) = rest.find('/') else {
        return Vec::new();
    };
    let raw = &rest.as_bytes()[slash..];
    let decoded = percent_decode(raw);
    let mut candidates = Vec::new();
    for bytes in [raw, decoded.as_slice()] {
        let path = path_from_bytes(strip_drive_slash(bytes));
        if !candidates.contains(&path) {
            candidates.push(path);
        }
    }
    candidates
}

fn osc7_directory(reported: &str) -> Option<PathBuf> {
    osc7_candidates(reported)
        .into_iter()
        .find(|path| path.is_dir())
}

fn normalize_lexically(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if matches!(
                    normalized.components().next_back(),
                    Some(Component::Normal(_))
                ) {
                    normalized.pop();
                } else if !normalized.has_root() {
                    normalized.push(component);
                }
            }
            component => normalized.push(component),
        }
    }
    normalized
}

fn expand_directory(
    dir: &str,
    base: Option<&Path>,
    home_of: impl Fn(&str) -> Option<String>,
) -> Option<PathBuf> {
    let path = if let Some(tilde) = dir.strip_prefix('~') {
        let (user, rest) = tilde.split_once('/').unwrap_or((tilde, ""));
        let mut path = PathBuf::from(home_of(user)?);
        if !rest.is_empty() {
            path.push(rest);
        }
        path
    } else if Path::new(dir).is_absolute() {
        PathBuf::from(dir)
    } else {
        base?.join(dir)
    };
    Some(normalize_lexically(&path))
}

fn display_root(root: &Path, home: Option<&str>) -> String {
    let home = home.map(Path::new).filter(|home| home.parent().is_some());
    let text = match home.and_then(|home| root.strip_prefix(home).ok()) {
        Some(rest) if rest.as_os_str().is_empty() => "~".to_owned(),
        Some(rest) => format!("~/{}", rest.to_string_lossy()),
        None => root.to_string_lossy().into_owned(),
    };
    if text.ends_with('/') || text.ends_with('\\') {
        text
    } else {
        format!("{text}/")
    }
}

fn foreground_basename(command: &str) -> String {
    let name = command.rsplit(['/', '\\']).next().unwrap_or(command);
    let name = name.trim_start_matches('-').to_lowercase();
    name.strip_suffix(".exe").map(str::to_owned).unwrap_or(name)
}

fn shell_kind(basename: &str) -> ShellKind {
    match basename {
        "fish" => ShellKind::Fish,
        "pwsh" | "powershell" => ShellKind::Pwsh,
        "nu" => ShellKind::Nu,
        _ => ShellKind::Posix,
    }
}

#[cfg(all(feature = "agent", unix))]
fn claude_record_in_group(
    mut records: Vec<crate::agent::claude_peers::PeerRecord>,
    pane: PaneId,
    pane_pid: Option<u32>,
    foreground: u32,
    group_of: impl Fn(u32) -> Option<u32>,
) -> bool {
    records.retain(|record| record.zz.is_none() && group_of(record.pid) == Some(foreground));
    crate::agent::claude_peers::record_for_pane(&records, &pane.to_string(), pane_pid).is_some()
}

#[cfg(all(feature = "agent", unix))]
fn agent_insert_style(pane: PaneId, pane_pid: Option<u32>, foreground: u32) -> Option<InsertStyle> {
    let records = crate::agent::claude_peers::read_records().unwrap_or_default();
    if claude_record_in_group(
        records,
        pane,
        pane_pid,
        foreground,
        crate::process_info::process_group,
    ) {
        return Some(InsertStyle::Claude);
    }
    crate::agent::codex_queue::group_runs_codex(foreground).then_some(InsertStyle::Codex)
}

#[cfg(not(all(feature = "agent", unix)))]
fn agent_insert_style(
    _pane: PaneId,
    _pane_pid: Option<u32>,
    _foreground: u32,
) -> Option<InsertStyle> {
    None
}

fn insert_style(
    pane: PaneId,
    pane_pid: Option<u32>,
    foreground: Option<u32>,
    basename: &str,
) -> InsertStyle {
    foreground
        .filter(|_| !PROMPT_SHELLS.contains(&basename))
        .and_then(|foreground| agent_insert_style(pane, pane_pid, foreground))
        .unwrap_or(InsertStyle::Shell(shell_kind(basename)))
}

struct ResolvedListing {
    root: PathBuf,
    root_is_home: bool,
    begin: PathListRoot,
}

impl Shared {
    pub(super) fn start_path_list(
        self: &Arc<Self>,
        client: ClientId,
        kind: ClientKind,
        request: (u64, PaneId, Option<String>),
        outbound: &Arc<OutboundMailbox>,
        walk: (&Arc<AtomicBool>, &Arc<Mutex<()>>),
    ) {
        let (request_id, pane, dir) = request;
        let (cancel, turn) = walk;
        let refusal = {
            let inner = self.inner.lock();
            if kind != ClientKind::Interactive {
                Some("path listing needs an interactive client".to_owned())
            } else if !inner.path_picker_clients.contains(&client) {
                Some("path listing needs the zz desktop app".to_owned())
            } else if inner.client_flags.contains(client) {
                Some("client is read-only".to_owned())
            } else if !client_is_attached_to_pane(&inner, client, pane) {
                Some(format!("{pane} is not in an attached session"))
            } else {
                None
            }
        };
        if let Some(refusal) = refusal {
            let _ = outbound.enqueue_reliable(&ProtocolMessage::PathListBegin {
                request_id,
                result: Err(refusal),
            });
            return;
        }
        let (expanded, previous) =
            self.record_requested_path_list_root(client, request_id, dir.as_deref());
        let shared = self.server_owner();
        let walker_outbound = Arc::clone(outbound);
        let walker_cancel = Arc::clone(cancel);
        let walker_turn = Arc::clone(turn);
        let spawned = thread::Builder::new()
            .name(format!("zz-path-list-{}", client.0))
            .spawn(move || {
                let stream = PathListStream {
                    outbound: &walker_outbound,
                    cancel: &walker_cancel,
                    request_id,
                };
                let waited = Instant::now();
                let _turn = loop {
                    if stream.cancelled() {
                        return;
                    }
                    if let Some(turn) = walker_turn.try_lock_for(PATH_LIST_TURN_POLL) {
                        break turn;
                    }
                    if waited.elapsed() >= PATH_LIST_BUDGET {
                        shared.restore_path_list_root(client, request_id, previous);
                        let _ = stream
                            .begin(Err("an earlier path listing is still running".to_owned()));
                        return;
                    }
                };
                if stream.cancelled() {
                    return;
                }
                let listing = match shared.resolve_path_list(pane, dir.as_deref(), expanded) {
                    Ok(listing) => listing,
                    Err(error) => {
                        shared.restore_path_list_root(client, request_id, previous);
                        let _ = stream.begin(Err(error));
                        return;
                    }
                };
                shared.record_resolved_path_list_root(
                    client,
                    request_id,
                    &listing.root,
                    &walker_cancel,
                );
                let Some(_slot) = PathWalkerSlot::acquire(&LIVE_PATH_WALKERS) else {
                    let _ = stream.begin(Ok(listing.begin)) && stream.chunk(Vec::new(), true, true);
                    return;
                };
                if !stream.begin(Ok(listing.begin)) {
                    return;
                }
                let (marks_sender, marks) = mpsc::sync_channel(1);
                let git_root = listing.root.clone();
                let git_cancel = Arc::clone(&walker_cancel);
                let git = thread::Builder::new()
                    .name(format!("zz-path-git-{}", client.0))
                    .spawn(move || {
                        let _ = marks_sender
                            .send(git_marks(&git_root, &|| git_cancel.load(Ordering::Acquire)));
                    })
                    .is_ok();
                let _ = stream.walk(
                    &listing.root,
                    listing.root_is_home,
                    WalkLimits::default(),
                    git.then_some(&marks),
                );
            });
        if let Err(error) = spawned {
            let _ = outbound.enqueue_reliable(&ProtocolMessage::PathListBegin {
                request_id,
                result: Err(format!("path listing could not start: {error}")),
            });
        }
    }

    fn record_requested_path_list_root(
        &self,
        client: ClientId,
        request_id: u64,
        dir: Option<&str>,
    ) -> (Option<PathBuf>, Option<PathBuf>) {
        let mut inner = self.inner.lock();
        let base = inner
            .path_list_roots
            .get(&client)
            .and_then(|(_, root)| root.clone());
        let expanded = dir.and_then(|dir| {
            expand_directory(dir, base.as_deref(), |user| {
                home_directory_for(&inner.engine, user)
            })
        });
        inner
            .path_list_roots
            .insert(client, (request_id, expanded.clone()));
        (expanded, base)
    }

    fn restore_path_list_root(&self, client: ClientId, request_id: u64, root: Option<PathBuf>) {
        let mut inner = self.inner.lock();
        if let Some((current, stored)) = inner.path_list_roots.get_mut(&client)
            && *current == request_id
        {
            *stored = root;
        }
    }

    fn record_resolved_path_list_root(
        &self,
        client: ClientId,
        request_id: u64,
        root: &Path,
        cancel: &AtomicBool,
    ) {
        let mut inner = self.inner.lock();
        if let Some((current, stored)) = inner.path_list_roots.get_mut(&client)
            && *current == request_id
            && !cancel.load(Ordering::Acquire)
        {
            *stored = Some(root.to_path_buf());
        }
    }

    fn resolve_path_list(
        &self,
        pane: PaneId,
        dir: Option<&str>,
        expanded: Option<PathBuf>,
    ) -> Result<ResolvedListing, String> {
        let (terminal, start_path, reported_path, home) = {
            let inner = self.inner.lock();
            let terminal = inner
                .terminals
                .get(&pane)
                .cloned()
                .ok_or_else(|| format!("{pane} is not a terminal pane"))?;
            let facts = inner.engine.pane_runtime_facts(pane);
            (
                terminal,
                facts
                    .map(|facts| facts.start_path.clone())
                    .unwrap_or_default(),
                facts
                    .map(|facts| facts.reported_path.clone())
                    .unwrap_or_default(),
                home_directory_for(&inner.engine, ""),
            )
        };
        let foreground = terminal.foreground_process_id();
        let basename = foreground_basename(&terminal_current_command(&terminal));
        if REMOTE_FOREGROUND_COMMANDS.contains(&basename.as_str()) {
            return Err(format!(
                "{basename} is in the foreground, so its files are on another machine"
            ));
        }
        let live = terminal_working_directory(&terminal).filter(|path| path.is_dir());
        let fallback = live
            .clone()
            .or_else(|| osc7_directory(&reported_path))
            .or_else(|| {
                (!start_path.is_empty())
                    .then(|| PathBuf::from(&start_path))
                    .filter(|path| path.is_dir())
            });
        let requested = match dir {
            Some(dir) => Some(
                expanded
                    .or_else(|| expand_directory(dir, fallback.as_deref(), |_| None))
                    .filter(|path| path.is_dir())
                    .ok_or_else(|| format!("{dir} is not a directory"))?,
            ),
            None => None,
        };
        let root = requested
            .or(fallback)
            .ok_or_else(|| format!("{pane} has no working directory"))?;
        let root_text = root
            .to_str()
            .filter(|text| text.len() <= MAX_PATH_LIST_TEXT_BYTES && listable_text(text))
            .ok_or_else(|| "the directory name cannot be listed".to_owned())?
            .to_owned();
        let cwd = live
            .as_deref()
            .and_then(Path::to_str)
            .filter(|text| text.len() <= MAX_PATH_LIST_TEXT_BYTES && listable_text(text));
        let begin = PathListRoot {
            root: root_text,
            display_root: display_root(&root, home.as_deref()),
            cwd: cwd.map(str::to_owned),
            insert: insert_style(pane, terminal.process_id(), foreground, &basename),
        };
        Ok(ResolvedListing {
            root_is_home: home.as_deref().is_some_and(|home| Path::new(home) == root),
            root,
            begin,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(root: &Path, relative: &str) {
        let path = root.join(relative);
        fs::create_dir_all(path.parent().expect("parent")).expect("create parent");
        fs::write(path, b"x").expect("write file");
    }

    fn walk_all(root: &Path, root_is_home: bool, limits: WalkLimits) -> (Vec<PathEntry>, WalkEnd) {
        let mut entries = Vec::new();
        let end = walk_paths(
            root,
            root_is_home,
            limits,
            &device_of,
            &|| false,
            &mut |entry| {
                entries.push(entry);
                true
            },
        );
        (entries, end)
    }

    fn rels(entries: &[PathEntry]) -> BTreeSet<String> {
        entries.iter().map(|entry| entry.rel.clone()).collect()
    }

    #[test]
    fn walker_honours_ignore_files_hidden_rules_and_listed_only_names() {
        let scratch = tempfile::tempdir().expect("temp dir");
        let root = scratch.path();
        fs::create_dir_all(root.join(".git/objects")).expect("git dir");
        fs::create_dir_all(root.join(".jj/repo")).expect("jj dir");
        fs::write(root.join(".gitignore"), "ignored.txt\nbuild/\n").expect("gitignore");
        for file in [
            "ignored.txt",
            "build/out.o",
            ".env",
            "src/main.rs",
            "node_modules/pkg/index.js",
            "target/debug/zz",
            "go/pkg/mod/cache/x",
            "a\nb.txt",
            "esc\u{1b}dir/child.txt",
        ] {
            touch(root, file);
        }

        let (entries, end) = walk_all(root, false, WalkLimits::default());
        let names = rels(&entries);

        assert_eq!(end, WalkEnd::Complete);
        for present in [
            ".env",
            ".gitignore",
            "src",
            "src/main.rs",
            "node_modules",
            "target",
            "go",
            "go/pkg",
            "go/pkg/mod",
        ] {
            assert!(names.contains(present), "{present} missing from {names:?}");
        }
        for absent in [
            ".git",
            ".git/objects",
            ".jj",
            "ignored.txt",
            "build",
            "node_modules/pkg",
            "target/debug",
            "go/pkg/mod/cache",
            "a\nb.txt",
            "esc\u{1b}dir",
            "esc\u{1b}dir/child.txt",
        ] {
            assert!(!names.contains(absent), "{absent:?} leaked into {names:?}");
        }
        assert!(names.iter().all(|rel| listable_text(rel)));
        let kinds = entries
            .iter()
            .map(|entry| (entry.rel.as_str(), entry.kind))
            .collect::<BTreeMap<_, _>>();
        assert_eq!(kinds["src"], PathKind::Dir);
        assert_eq!(kinds["src/main.rs"], PathKind::File);
        let src = entries.iter().position(|entry| entry.rel == "src");
        let nested = entries.iter().position(|entry| entry.rel == "src/main.rs");
        assert!(src < nested, "breadth first order");
    }

    #[test]
    fn walker_hides_dotfiles_outside_a_repository() {
        let scratch = tempfile::tempdir().expect("temp dir");
        touch(scratch.path(), ".env");
        touch(scratch.path(), "visible.txt");

        let (entries, _) = walk_all(scratch.path(), false, WalkLimits::default());

        assert_eq!(rels(&entries), BTreeSet::from(["visible.txt".to_owned()]));
    }

    #[test]
    fn home_media_roots_are_listed_but_not_entered_only_from_home() {
        let scratch = tempfile::tempdir().expect("temp dir");
        touch(scratch.path(), "Library/Caches/blob");
        touch(scratch.path(), "dev/Library/notes.txt");

        let (home, _) = walk_all(scratch.path(), true, WalkLimits::default());
        let (plain, _) = walk_all(scratch.path(), false, WalkLimits::default());

        assert!(rels(&home).contains("Library"));
        assert!(!rels(&home).contains("Library/Caches"));
        assert!(rels(&home).contains("dev/Library/notes.txt"));
        assert!(rels(&plain).contains("Library/Caches/blob"));
        assert!(path_walk_enters(Path::new("dev"), true));
        assert!(!path_walk_enters(Path::new("Movies"), true));
        assert!(path_walk_enters(Path::new("Movies"), false));
        assert!(!path_walk_enters(Path::new("a/node_modules"), false));
        assert!(!path_walk_enters(Path::new("x/go/pkg/mod"), false));
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_report_their_target_kind_and_are_never_entered() {
        let scratch = tempfile::tempdir().expect("temp dir");
        let root = scratch.path();
        touch(root, "real/inside.txt");
        touch(root, "file.txt");
        std::os::unix::fs::symlink(root.join("real"), root.join("dir-link")).expect("dir link");
        std::os::unix::fs::symlink(root.join("file.txt"), root.join("file-link"))
            .expect("file link");
        std::os::unix::fs::symlink(root.join("missing"), root.join("broken")).expect("broken");
        std::os::unix::fs::symlink(root, root.join("real/loop")).expect("loop link");

        let (entries, end) = walk_all(root, false, WalkLimits::default());
        let by_rel = entries
            .iter()
            .map(|entry| (entry.rel.as_str(), (entry.kind, entry.symlink)))
            .collect::<BTreeMap<_, _>>();

        assert_eq!(end, WalkEnd::Complete);
        assert_eq!(by_rel["dir-link"], (PathKind::Dir, true));
        assert_eq!(by_rel["file-link"], (PathKind::File, true));
        assert_eq!(by_rel["broken"], (PathKind::File, true));
        assert_eq!(by_rel["real/loop"], (PathKind::Dir, true));
        assert!(!by_rel.contains_key("dir-link/inside.txt"));
        assert!(!by_rel.keys().any(|rel| rel.starts_with("real/loop/")));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn non_utf8_names_are_dropped() {
        use std::os::unix::ffi::OsStrExt as _;

        let scratch = tempfile::tempdir().expect("temp dir");
        fs::create_dir(scratch.path().join(OsStr::from_bytes(b"bad\xffdir"))).expect("bad dir");
        fs::write(
            scratch
                .path()
                .join(OsStr::from_bytes(b"bad\xffdir"))
                .join("child.txt"),
            b"x",
        )
        .expect("child");
        touch(scratch.path(), "good.txt");

        let (entries, _) = walk_all(scratch.path(), false, WalkLimits::default());

        assert_eq!(rels(&entries), BTreeSet::from(["good.txt".to_owned()]));
    }

    #[test]
    fn filesystem_root_lists_its_children_only() {
        let root = Path::new("/");
        let (entries, _) = walk_all(
            root,
            false,
            WalkLimits {
                budget: Duration::from_secs(2),
                ..WalkLimits::default()
            },
        );

        assert!(!entries.is_empty());
        assert!(entries.iter().all(|entry| !entry.rel.contains('/')));
    }

    #[test]
    fn child_directories_on_another_device_are_listed_but_not_entered() {
        let scratch = tempfile::tempdir().expect("temp dir");
        touch(scratch.path(), "local/a.txt");
        touch(scratch.path(), "mount/b.txt");
        let mut entries = Vec::new();
        let end = walk_paths(
            scratch.path(),
            false,
            WalkLimits::default(),
            &|path, _| u64::from(path.ends_with("mount")),
            &|| false,
            &mut |entry| {
                entries.push(entry);
                true
            },
        );

        assert_eq!(end, WalkEnd::Complete);
        assert_eq!(
            rels(&entries),
            BTreeSet::from([
                "local".to_owned(),
                "local/a.txt".to_owned(),
                "mount".to_owned(),
            ])
        );
    }

    #[test]
    fn depth_limit_marks_the_walk_truncated_only_when_a_directory_is_skipped() {
        let scratch = tempfile::tempdir().expect("temp dir");
        touch(scratch.path(), "a/b.txt");
        let limits = WalkLimits {
            depth: 2,
            ..WalkLimits::default()
        };

        assert_eq!(walk_all(scratch.path(), false, limits).1, WalkEnd::Complete);
        touch(scratch.path(), "node_modules/pkg/index.js");
        assert_eq!(walk_all(scratch.path(), false, limits).1, WalkEnd::Complete);
        fs::create_dir_all(scratch.path().join("a/deeper")).expect("deeper dir");
        assert_eq!(
            walk_all(scratch.path(), false, limits).1,
            WalkEnd::Truncated
        );
    }

    #[test]
    fn limits_truncate_and_cancel_stops() {
        let scratch = tempfile::tempdir().expect("temp dir");
        for index in 0..10 {
            touch(scratch.path(), &format!("f{index}"));
        }
        touch(scratch.path(), "a/b/c/d.txt");

        let (entries, end) = walk_all(
            scratch.path(),
            false,
            WalkLimits {
                entries: 3,
                ..WalkLimits::default()
            },
        );
        assert_eq!(end, WalkEnd::Truncated);
        assert_eq!(entries.len(), 3);

        let (entries, end) = walk_all(
            scratch.path(),
            false,
            WalkLimits {
                depth: 2,
                ..WalkLimits::default()
            },
        );
        assert_eq!(end, WalkEnd::Truncated);
        assert!(rels(&entries).contains("a/b"));
        assert!(!rels(&entries).contains("a/b/c"));

        let (_, end) = walk_all(
            scratch.path(),
            false,
            WalkLimits {
                budget: Duration::ZERO,
                ..WalkLimits::default()
            },
        );
        assert_eq!(end, WalkEnd::Truncated);

        let end = walk_paths(
            scratch.path(),
            false,
            WalkLimits::default(),
            &device_of,
            &|| true,
            &mut |_| panic!("a cancelled walk emitted"),
        );
        assert_eq!(end, WalkEnd::Stopped);
    }

    #[test]
    fn walker_slots_are_capped_and_released() {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let slots = (0..MAX_PATH_LIST_WALKERS)
            .map(|_| PathWalkerSlot::acquire(&COUNTER).expect("slot under the cap"))
            .collect::<Vec<_>>();
        assert!(PathWalkerSlot::acquire(&COUNTER).is_none());
        drop(slots);
        assert_eq!(COUNTER.load(Ordering::Acquire), 0);
        assert!(PathWalkerSlot::acquire(&COUNTER).is_some());
    }

    fn wide_tree(files: usize) -> tempfile::TempDir {
        let scratch = tempfile::tempdir().expect("temp dir");
        for index in 0..files {
            touch(scratch.path(), &format!("{index:04}-{}", "n".repeat(180)));
        }
        scratch
    }

    fn decode_all(mailbox: &OutboundMailbox) -> Vec<ProtocolMessage> {
        let frames = {
            let mut state = mailbox.state.lock();
            let frames = state.reliable.drain(..).collect::<Vec<_>>();
            state.queued_bytes = 0;
            frames
        };
        frames
            .iter()
            .map(|frame| zz_protocol::decode_protocol_frame(frame).expect("decode"))
            .collect()
    }

    #[test]
    fn stream_sends_bounded_chunks_then_done() {
        let tree = wide_tree(700);
        let mailbox = OutboundMailbox::new();
        let drained = Arc::new(Mutex::new(Vec::new()));
        let reader_mailbox = Arc::clone(&mailbox);
        let reader_drained = Arc::clone(&drained);
        let reader = thread::spawn(move || {
            while let Some(frame) = reader_mailbox.recv() {
                let message = zz_protocol::decode_protocol_frame(&frame).expect("decode");
                let done = matches!(message, ProtocolMessage::PathListChunk { done: true, .. });
                reader_drained.lock().push((frame.len(), message));
                if done {
                    break;
                }
            }
        });
        let cancel = AtomicBool::new(false);
        let stream = PathListStream {
            outbound: &mailbox,
            cancel: &cancel,
            request_id: 9,
        };

        assert!(stream.walk(tree.path(), false, WalkLimits::default(), None));
        reader.join().expect("reader");
        let drained = drained.lock();
        let mut total = 0;
        for (bytes, message) in drained.iter() {
            assert!(
                *bytes <= MAX_PATH_LIST_CHUNK_BYTES + 64,
                "chunk of {bytes} bytes"
            );
            let ProtocolMessage::PathListChunk {
                request_id,
                entries,
                ..
            } = message
            else {
                panic!("unexpected {message:?}");
            };
            assert_eq!(*request_id, 9);
            total += entries.len();
        }
        assert_eq!(total, 700);
        assert!(drained.len() >= 2);
        assert!(mailbox.is_open());
    }

    #[test]
    fn stalled_outbound_never_closes_the_mailbox_and_cancel_stops_the_walk() {
        let tree = wide_tree(3000);
        let mailbox = OutboundMailbox::new();
        let cancel = Arc::new(AtomicBool::new(false));
        let walker_mailbox = Arc::clone(&mailbox);
        let walker_cancel = Arc::clone(&cancel);
        let root = tree.path().to_path_buf();
        let walker = thread::spawn(move || {
            PathListStream {
                outbound: &walker_mailbox,
                cancel: &walker_cancel,
                request_id: 1,
            }
            .walk(&root, false, WalkLimits::default(), None)
        });
        let deadline = Instant::now() + Duration::from_secs(5);
        while mailbox
            .queued_reliable()
            .is_some_and(|(bytes, _)| bytes < PATH_LIST_HIGH_WATER_BYTES)
        {
            assert!(Instant::now() < deadline, "the walk never filled the queue");
            thread::sleep(Duration::from_millis(5));
        }
        thread::sleep(Duration::from_millis(100));
        let (_, messages) = mailbox.queued_reliable().expect("mailbox stays open");
        assert!(messages < CONTROL_PENDING_MESSAGE_LIMIT);
        assert!(!walker.is_finished());

        let cancelled = Instant::now();
        cancel.store(true, Ordering::Release);
        assert!(!walker.join().expect("walker"));
        assert!(cancelled.elapsed() < Duration::from_secs(1));
        assert!(mailbox.is_open());
        assert!(
            decode_all(&mailbox).iter().all(|message| !matches!(
                message,
                ProtocolMessage::PathListChunk { done: true, .. }
            ))
        );
    }

    #[test]
    fn back_to_back_walks_stay_under_the_pending_limit() {
        let tree = wide_tree(1500);
        let mailbox = OutboundMailbox::new();
        let peak = Arc::new(AtomicUsize::new(0));
        let reader_mailbox = Arc::clone(&mailbox);
        let reader_peak = Arc::clone(&peak);
        let reader = thread::spawn(move || {
            loop {
                if let Some((_, messages)) = reader_mailbox.queued_reliable() {
                    reader_peak.fetch_max(messages, Ordering::AcqRel);
                }
                thread::sleep(Duration::from_millis(2));
                if reader_mailbox.recv().is_none() {
                    break;
                }
            }
        });
        let mut previous: Option<(Arc<AtomicBool>, thread::JoinHandle<bool>)> = None;
        for request_id in 1..=3 {
            if let Some((cancel, walker)) = previous.take() {
                thread::sleep(Duration::from_millis(20));
                cancel.store(true, Ordering::Release);
                let _ = walker.join().expect("walker");
            }
            let cancel = Arc::new(AtomicBool::new(false));
            let walker_cancel = Arc::clone(&cancel);
            let walker_mailbox = Arc::clone(&mailbox);
            let root = tree.path().to_path_buf();
            previous = Some((
                cancel,
                thread::spawn(move || {
                    PathListStream {
                        outbound: &walker_mailbox,
                        cancel: &walker_cancel,
                        request_id,
                    }
                    .walk(&root, false, WalkLimits::default(), None)
                }),
            ));
        }
        let (_, last) = previous.expect("last walk");
        assert!(last.join().expect("last walker"));
        assert!(mailbox.is_open());
        mailbox.close();
        reader.join().expect("reader");
        assert!(peak.load(Ordering::Acquire) <= CONTROL_PENDING_MESSAGE_LIMIT);
    }

    #[test]
    fn git_status_records_map_to_marks_under_the_prefix() {
        let output = b" M sub/a.rs\0?? sub/new/\0A  sub/added.rs\0AM sub/added-edited.rs\0UU sub/conf.rs\0AA sub/both.rs\0DD sub/gone-both.rs\0 D sub/gone.rs\0D  sub/staged-gone.rs\0 T sub/type.rs\0M  other/x.rs\0?? sub/esc\x1bname\0";

        assert_eq!(
            parse_git_status(output, "sub/"),
            vec![
                ("a.rs".to_owned(), GitMark::Modified),
                ("new/".to_owned(), GitMark::Untracked),
                ("added.rs".to_owned(), GitMark::Added),
                ("added-edited.rs".to_owned(), GitMark::Added),
                ("conf.rs".to_owned(), GitMark::Conflicted),
                ("both.rs".to_owned(), GitMark::Conflicted),
                ("gone-both.rs".to_owned(), GitMark::Conflicted),
                ("type.rs".to_owned(), GitMark::Modified),
            ]
        );
        assert_eq!(
            parse_git_status(b"?? top.txt\0", ""),
            vec![("top.txt".to_owned(), GitMark::Untracked)]
        );
    }

    fn git(root: &Path, args: &[&str]) -> bool {
        std::process::Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    }

    fn git_commit_all(root: &Path) {
        assert!(git(root, &["add", "."]));
        assert!(git(
            root,
            &[
                "-c",
                "user.email=picker@zz.test",
                "-c",
                "user.name=picker",
                "-c",
                "commit.gpgsign=false",
                "commit",
                "--quiet",
                "-m",
                "seed",
            ]
        ));
    }

    #[test]
    fn git_marks_come_from_a_real_repository_subdirectory() {
        let scratch = tempfile::tempdir().expect("temp dir");
        let root = scratch.path();
        if !git(root, &["init", "--quiet"]) {
            return;
        }
        touch(root, "sub/tracked.txt");
        touch(root, "outside.txt");
        git_commit_all(root);
        fs::write(root.join("sub/tracked.txt"), b"changed").expect("edit");
        fs::write(root.join("outside.txt"), b"changed").expect("edit outside");
        touch(root, "sub/fresh/new.txt");

        let mut marks = git_marks(&root.join("sub"), &|| false);
        marks.sort_by(|left, right| left.0.cmp(&right.0));

        assert_eq!(
            marks,
            vec![
                ("fresh/".to_owned(), GitMark::Untracked),
                ("tracked.txt".to_owned(), GitMark::Modified),
            ]
        );
        assert!(git_marks(&std::env::temp_dir().join("zz-no-such-repo"), &|| false).is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn git_marks_never_run_repository_fsmonitor_or_filter_commands() {
        let scratch = tempfile::tempdir().expect("temp dir");
        let root = scratch.path().join("repo");
        fs::create_dir_all(&root).expect("repo dir");
        if !git(&root, &["init", "--quiet"]) {
            return;
        }
        fs::write(root.join(".gitattributes"), "*.txt filter=evil\n").expect("attributes");
        touch(&root, "tracked.txt");
        git_commit_all(&root);
        let sentinel = scratch.path().join("ran");
        let command = format!("touch '{}'", sentinel.display());
        for (key, value) in [
            ("core.fsmonitor", format!("{command}; false")),
            ("filter.evil.clean", command.clone()),
            ("filter.evil.process", command.clone()),
            ("filter.evil.required", "true".to_owned()),
        ] {
            assert!(git(&root, &["config", key, &value]));
        }
        fs::write(root.join("tracked.txt"), b"changed").expect("edit");
        touch(&root, "fresh.rs");

        let mut marks = git_marks(&root, &|| false);
        marks.sort_by(|left, right| left.0.cmp(&right.0));

        assert!(!sentinel.exists());
        assert_eq!(
            marks,
            vec![
                ("fresh.rs".to_owned(), GitMark::Untracked),
                ("tracked.txt".to_owned(), GitMark::Modified),
            ]
        );
    }

    #[test]
    fn git_filter_drivers_are_read_from_config_names() {
        assert_eq!(
            git_filter_drivers(b"filter.lfs.clean\0filter.lfs.process\0filter.a.b.clean\0"),
            Some(BTreeSet::from(["a.b", "lfs"]))
        );
        assert_eq!(git_filter_drivers(b""), Some(BTreeSet::new()));
        assert_eq!(git_filter_drivers(b"filter.a=b.clean\0"), None);
    }

    #[test]
    fn stream_sends_git_marks_for_sent_entries_before_done() {
        let scratch = tempfile::tempdir().expect("temp dir");
        touch(scratch.path(), "a.txt");
        touch(scratch.path(), "dir/b.txt");
        let mailbox = OutboundMailbox::new();
        let cancel = AtomicBool::new(false);
        let (sender, marks) = mpsc::sync_channel(1);
        sender
            .send(vec![
                ("a.txt".to_owned(), GitMark::Modified),
                ("dir/".to_owned(), GitMark::Untracked),
                ("elsewhere.txt".to_owned(), GitMark::Added),
            ])
            .expect("marks");

        assert!(
            PathListStream {
                outbound: &mailbox,
                cancel: &cancel,
                request_id: 4,
            }
            .walk(scratch.path(), false, WalkLimits::default(), Some(&marks))
        );
        let messages = decode_all(&mailbox);
        let git_index = messages
            .iter()
            .position(|message| matches!(message, ProtocolMessage::PathListGit { .. }))
            .expect("git marks sent");
        assert!(matches!(
            &messages[git_index],
            ProtocolMessage::PathListGit { marks, .. }
                if marks == &vec![
                    ("a.txt".to_owned(), GitMark::Modified),
                    ("dir/".to_owned(), GitMark::Untracked),
                ]
        ));
        assert!(matches!(
            messages.last(),
            Some(ProtocolMessage::PathListChunk {
                done: true,
                truncated: false,
                ..
            })
        ));
        let sent_before = messages[..git_index]
            .iter()
            .map(|message| match message {
                ProtocolMessage::PathListChunk { entries, done, .. } => {
                    assert!(!done);
                    entries.len()
                }
                other => panic!("unexpected {other:?}"),
            })
            .sum::<usize>();
        assert_eq!(sent_before, 3);
    }

    #[test]
    fn osc7_candidates_try_the_raw_path_then_a_tolerant_decode() {
        assert_eq!(
            osc7_candidates("file://host/tmp/a b"),
            vec![PathBuf::from("/tmp/a b")]
        );
        assert_eq!(
            osc7_candidates("file://host/tmp/a%20b"),
            vec![PathBuf::from("/tmp/a%20b"), PathBuf::from("/tmp/a b")]
        );
        assert_eq!(
            osc7_candidates("file:///tmp/100%zz"),
            vec![PathBuf::from("/tmp/100%zz")]
        );
        assert_eq!(
            osc7_candidates("file://MACHINE/C:/src"),
            vec![PathBuf::from("C:/src")]
        );
        assert_eq!(
            osc7_candidates("kitty-shell-cwd://host/x"),
            vec![PathBuf::from("/x")]
        );
        assert!(osc7_candidates("http://host/x").is_empty());
        assert!(osc7_candidates("file://host").is_empty());
    }

    #[test]
    fn osc7_directory_keeps_the_first_candidate_that_exists() {
        let scratch = tempfile::tempdir().expect("temp dir");
        let root = scratch.path().to_str().expect("utf-8 temp dir");
        fs::create_dir(scratch.path().join("100%41")).expect("percent dir");
        fs::create_dir(scratch.path().join("a b")).expect("space dir");

        assert_eq!(
            osc7_directory(&format!("file://elsewhere{root}/100%41")),
            Some(scratch.path().join("100%41"))
        );
        assert_eq!(
            osc7_directory(&format!("file://elsewhere{root}/a%20b")),
            Some(scratch.path().join("a b"))
        );
        assert_eq!(osc7_directory(&format!("file://h{root}/missing")), None);
    }

    #[test]
    fn requested_directories_expand_on_the_daemon() {
        let home = |user: &str| match user {
            "" => Some("/home/me".to_owned()),
            "bob" => Some("/home/bob".to_owned()),
            _ => None,
        };
        let base = Path::new("/home/me/dev/zz");
        for (dir, expected) in [
            ("~", Some("/home/me")),
            ("~/", Some("/home/me")),
            ("~/notes", Some("/home/me/notes")),
            ("~bob/src", Some("/home/bob/src")),
            ("~nobody/src", None),
            ("/etc/./x/..", Some("/etc")),
            ("../", Some("/home/me/dev")),
            ("src/../crates", Some("/home/me/dev/zz/crates")),
            ("../../../../..", Some("/")),
        ] {
            assert_eq!(
                expand_directory(dir, Some(base), home),
                expected.map(PathBuf::from),
                "{dir}"
            );
        }
        assert_eq!(expand_directory("src", None, home), None);
    }

    #[test]
    fn display_root_abbreviates_the_daemon_home() {
        assert_eq!(display_root(Path::new("/home/me"), Some("/home/me")), "~/");
        assert_eq!(
            display_root(Path::new("/home/me/dev/zz"), Some("/home/me")),
            "~/dev/zz/"
        );
        assert_eq!(
            display_root(Path::new("/home/meow"), Some("/home/me")),
            "/home/meow/"
        );
        assert_eq!(display_root(Path::new("/"), Some("/home/me")), "/");
        assert_eq!(display_root(Path::new("/srv"), Some("/")), "/srv/");
        assert_eq!(display_root(Path::new("/srv"), None), "/srv/");
    }

    #[test]
    fn shell_kind_follows_the_foreground_basename() {
        for (command, kind) in [
            ("zsh", ShellKind::Posix),
            ("-zsh", ShellKind::Posix),
            ("/opt/homebrew/bin/fish", ShellKind::Fish),
            ("FISH", ShellKind::Fish),
            ("pwsh", ShellKind::Pwsh),
            (
                "C:\\Program Files\\PowerShell\\7\\pwsh.exe",
                ShellKind::Pwsh,
            ),
            ("powershell.exe", ShellKind::Pwsh),
            ("nu", ShellKind::Nu),
            ("nu.exe", ShellKind::Nu),
            ("vim", ShellKind::Posix),
            ("", ShellKind::Posix),
        ] {
            assert_eq!(shell_kind(&foreground_basename(command)), kind, "{command}");
        }
        for command in ["ssh", "/usr/bin/mosh-client", "tmux", "zz_cli"] {
            assert!(REMOTE_FOREGROUND_COMMANDS.contains(&foreground_basename(command).as_str()));
        }
        assert_eq!(
            insert_style(PaneId(1), None, None, "fish"),
            InsertStyle::Shell(ShellKind::Fish)
        );
    }

    #[cfg(all(feature = "agent", unix))]
    #[test]
    fn claude_needs_its_own_record_in_the_foreground_group() {
        use crate::agent::claude_peers::PeerRecord;

        let me = std::process::id();
        let record = |zz: Option<serde_json::Value>| PeerRecord {
            pid: me,
            tmux: "/tmp/zz,1,0.%3".to_owned(),
            messaging_socket_path: PathBuf::from("/tmp/cc-socks/peer.sock"),
            updated_at: u64::try_from(
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .expect("clock")
                    .as_millis(),
            )
            .expect("millis"),
            zz,
            ..PeerRecord::default()
        };
        let pane = PaneId(3);
        let in_group = |_: u32| Some(77);

        assert!(claude_record_in_group(
            vec![record(None)],
            pane,
            Some(me),
            77,
            in_group
        ));
        assert!(!claude_record_in_group(
            vec![record(None)],
            pane,
            Some(me),
            78,
            in_group
        ));
        assert!(!claude_record_in_group(
            vec![record(Some(serde_json::json!({ "pane": "%3" })))],
            pane,
            Some(me),
            77,
            in_group
        ));
        assert!(!claude_record_in_group(
            vec![record(None)],
            PaneId(4),
            Some(me),
            77,
            in_group
        ));
    }

    #[cfg(all(feature = "agent", unix))]
    #[test]
    fn codex_counts_only_inside_the_foreground_group() {
        use std::os::unix::process::CommandExt as _;

        let Some(sleep) = ["/bin/sleep", "/usr/bin/sleep"]
            .into_iter()
            .map(Path::new)
            .find(|path| path.exists())
        else {
            return;
        };
        let scratch = tempfile::tempdir().expect("temp dir");
        let codex = scratch.path().join("codex");
        #[cfg(target_os = "macos")]
        std::os::unix::fs::symlink(sleep, &codex).expect("link sleep as codex");
        #[cfg(not(target_os = "macos"))]
        fs::copy(sleep, &codex).expect("copy sleep as codex");

        let mut leader = std::process::Command::new("sh")
            .arg("-c")
            .arg(format!("'{}' 30; true", codex.display()))
            .process_group(0)
            .spawn()
            .expect("node-like leader");
        let mut background = std::process::Command::new(&codex)
            .arg("30")
            .process_group(0)
            .spawn()
            .expect("background codex");
        let mut shell = std::process::Command::new("sh")
            .arg("-c")
            .arg("sleep 30; true")
            .process_group(0)
            .spawn()
            .expect("foreground shell");
        let leader_group = leader.id();
        let shell_group = shell.id();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !crate::agent::codex_queue::group_runs_codex(leader_group) {
            assert!(Instant::now() < deadline, "codex child never appeared");
            thread::sleep(Duration::from_millis(20));
        }

        let foreground_shell = crate::agent::codex_queue::group_runs_codex(shell_group);
        let codex_style = insert_style(PaneId(1), None, Some(leader_group), "node");
        let shell_style = insert_style(PaneId(1), None, Some(shell_group), "zsh");
        let prompt_style = insert_style(PaneId(1), None, Some(leader_group), "bash");

        for child in [&mut leader, &mut background, &mut shell] {
            let _ = rustix::process::kill_process_group(
                rustix::process::Pid::from_child(child),
                rustix::process::Signal::KILL,
            );
            let _ = child.wait();
        }
        assert!(!foreground_shell);
        assert_eq!(codex_style, InsertStyle::Codex);
        assert_eq!(shell_style, InsertStyle::Shell(ShellKind::Posix));
        assert_eq!(prompt_style, InsertStyle::Shell(ShellKind::Posix));
        assert_eq!(
            crate::process_info::process_group(std::process::id()),
            u32::try_from(rustix::process::getpgrp().as_raw_nonzero().get()).ok()
        );
    }

    fn listing(
        shared: &Arc<Shared>,
        client: ClientId,
        kind: ClientKind,
        request: (u64, PaneId, Option<String>),
    ) -> Vec<ProtocolMessage> {
        let mailbox = OutboundMailbox::new();
        shared.start_path_list(
            client,
            kind,
            request,
            &mailbox,
            (&Arc::new(AtomicBool::new(false)), &Arc::new(Mutex::new(()))),
        );
        collect_listing(&mailbox)
    }

    fn collect_listing(mailbox: &Arc<OutboundMailbox>) -> Vec<ProtocolMessage> {
        let (sender, messages) = mpsc::channel();
        let reader_mailbox = Arc::clone(mailbox);
        thread::spawn(move || {
            let mut collected = Vec::new();
            while let Some(frame) = reader_mailbox.recv() {
                let message = zz_protocol::decode_protocol_frame(&frame).expect("decode");
                let last = matches!(
                    message,
                    ProtocolMessage::PathListChunk { done: true, .. }
                        | ProtocolMessage::PathListBegin { result: Err(_), .. }
                );
                collected.push(message);
                if last {
                    break;
                }
            }
            let _ = sender.send(collected);
        });
        let collected = messages
            .recv_timeout(Duration::from_secs(20))
            .expect("the listing finished");
        mailbox.close();
        collected
    }

    #[test]
    fn path_list_requests_resolve_roots_on_the_daemon_and_refuse_bad_clients() {
        let shared = Arc::new(Shared::new(1));
        let mailbox = OutboundMailbox::new();
        let (client, _) = shared.register_subscribed(ClientKind::Interactive, None, None, mailbox);
        let mut context = ExecutionContext::default();
        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("new-session", ["-d", "-s", "listing"]),
            )
            .expect("session");
        let pane = context.pane.expect("pane id");
        let refused = listing(&shared, client, ClientKind::Interactive, (4, pane, None));
        assert!(
            matches!(
                refused.as_slice(),
                [ProtocolMessage::PathListBegin { result: Err(error), .. }]
                    if error == "path listing needs the zz desktop app"
            ),
            "{refused:?}"
        );
        shared.inner.lock().path_picker_clients.insert(client);
        let refused = listing(&shared, client, ClientKind::Interactive, (4, pane, None));
        assert!(
            matches!(
                refused.as_slice(),
                [ProtocolMessage::PathListBegin { result: Err(error), .. }]
                    if error.contains("not in an attached session")
            ),
            "{refused:?}"
        );
        shared
            .attach(client, context.session.expect("session id"))
            .expect("attach session");
        let scratch = tempfile::tempdir().expect("temp dir");
        let root = scratch.path().canonicalize().expect("canonical temp dir");
        touch(&root, "a.txt");
        touch(&root, "sub/b.txt");
        let root_text = root.to_str().expect("utf-8 temp dir").to_owned();

        let messages = listing(
            &shared,
            client,
            ClientKind::Interactive,
            (5, pane, Some(root_text.clone())),
        );
        let Some(ProtocolMessage::PathListBegin {
            request_id: 5,
            result: Ok(begin),
        }) = messages.first()
        else {
            panic!("expected a successful begin, got {messages:?}");
        };
        assert_eq!(begin.root, root_text);
        assert!(begin.display_root.ends_with('/'));
        assert!(matches!(begin.insert, InsertStyle::Shell(_)));
        let listed = messages
            .iter()
            .flat_map(|message| match message {
                ProtocolMessage::PathListChunk { entries, .. } => entries.clone(),
                _ => Vec::new(),
            })
            .map(|entry| entry.rel)
            .collect::<BTreeSet<_>>();
        assert_eq!(
            listed,
            BTreeSet::from(["a.txt".to_owned(), "sub".to_owned(), "sub/b.txt".to_owned()])
        );

        let messages = listing(
            &shared,
            client,
            ClientKind::Interactive,
            (6, pane, Some("sub".to_owned())),
        );
        assert!(matches!(
            messages.first(),
            Some(ProtocolMessage::PathListBegin { request_id: 6, result: Ok(begin) })
                if Path::new(&begin.root) == root.join("sub")
        ));
        let messages = listing(
            &shared,
            client,
            ClientKind::Interactive,
            (20, pane, Some("missing".to_owned())),
        );
        assert!(
            matches!(
                messages.as_slice(),
                [ProtocolMessage::PathListBegin { request_id: 20, result: Err(error) }]
                    if error == "missing is not a directory"
            ),
            "{messages:?}"
        );
        let messages = listing(
            &shared,
            client,
            ClientKind::Interactive,
            (21, pane, Some("..".to_owned())),
        );
        assert!(matches!(
            messages.first(),
            Some(ProtocolMessage::PathListBegin { request_id: 21, result: Ok(begin) })
                if begin.root == root_text
        ));

        let refusals = [
            (ClientKind::Command, pane, "interactive client"),
            (
                ClientKind::Interactive,
                PaneId(9_999),
                "not in an attached session",
            ),
        ];
        for (kind, target, expected) in refusals {
            let messages = listing(&shared, client, kind, (7, target, None));
            assert!(
                matches!(
                    messages.as_slice(),
                    [ProtocolMessage::PathListBegin { result: Err(error), .. }]
                        if error.contains(expected)
                ),
                "{messages:?}"
            );
        }
        shared.inner.lock().client_flags.insert(client);
        let messages = listing(&shared, client, ClientKind::Interactive, (8, pane, None));
        assert!(matches!(
            messages.as_slice(),
            [ProtocolMessage::PathListBegin { result: Err(error), .. }] if error == "client is read-only"
        ));
    }
    fn path_list_client() -> (Arc<Shared>, ClientId, PaneId) {
        let shared = Arc::new(Shared::new(1));
        let (client, _) =
            shared.register_subscribed(ClientKind::Interactive, None, None, OutboundMailbox::new());
        let mut context = ExecutionContext::default();
        shared
            .execute(
                client,
                ClientKind::Interactive,
                &mut context,
                &CommandInvocation::new("new-session", ["-d", "-s", "roots"]),
            )
            .expect("session");
        shared
            .attach(client, context.session.expect("session id"))
            .expect("attach session");
        shared.inner.lock().path_picker_clients.insert(client);
        (shared, client, context.pane.expect("pane id"))
    }

    #[test]
    fn relative_requests_join_the_latest_requested_root_before_any_walker_runs() {
        let (shared, client, _) = path_list_client();
        let root = Path::new("/r/a/b");
        assert_eq!(
            shared.record_requested_path_list_root(client, 1, root.to_str()),
            (Some(root.to_path_buf()), None)
        );
        assert_eq!(
            shared.record_requested_path_list_root(client, 2, Some("..")),
            (Some(PathBuf::from("/r/a")), Some(root.to_path_buf()))
        );
        assert_eq!(
            shared.record_requested_path_list_root(client, 3, Some("..")),
            (Some(PathBuf::from("/r")), Some(PathBuf::from("/r/a")))
        );

        let cancel = AtomicBool::new(false);
        shared.record_resolved_path_list_root(client, 2, Path::new("/stale"), &cancel);
        assert_eq!(
            shared.inner.lock().path_list_roots.get(&client),
            Some(&(3, Some(PathBuf::from("/r"))))
        );
        cancel.store(true, Ordering::Release);
        shared.record_resolved_path_list_root(client, 3, Path::new("/cancelled"), &cancel);
        assert_eq!(
            shared.inner.lock().path_list_roots.get(&client),
            Some(&(3, Some(PathBuf::from("/r"))))
        );
        cancel.store(false, Ordering::Release);
        shared.record_resolved_path_list_root(client, 3, Path::new("/home/me"), &cancel);
        assert_eq!(
            shared.inner.lock().path_list_roots.get(&client),
            Some(&(3, Some(PathBuf::from("/home/me"))))
        );

        shared.inner.lock().path_list_roots.remove(&client);
        shared.record_resolved_path_list_root(client, 3, Path::new("/gone"), &cancel);
        assert!(!shared.inner.lock().path_list_roots.contains_key(&client));
    }

    #[test]
    fn a_superseded_request_waiting_for_its_turn_sends_nothing() {
        let (shared, client, pane) = path_list_client();
        let scratch = tempfile::tempdir().expect("temp dir");
        touch(scratch.path(), "a.txt");
        let root = scratch
            .path()
            .canonicalize()
            .expect("canonical temp dir")
            .to_str()
            .expect("utf-8 temp dir")
            .to_owned();
        let mailbox = OutboundMailbox::new();
        let turn = Arc::new(Mutex::new(()));
        let held = turn.lock();
        let first = Arc::new(AtomicBool::new(false));
        shared.start_path_list(
            client,
            ClientKind::Interactive,
            (1, pane, Some(root.clone())),
            &mailbox,
            (&first, &turn),
        );
        first.store(true, Ordering::Release);
        shared.start_path_list(
            client,
            ClientKind::Interactive,
            (2, pane, Some(root)),
            &mailbox,
            (&Arc::new(AtomicBool::new(false)), &turn),
        );
        thread::sleep(PATH_LIST_TURN_POLL * 3);
        drop(held);

        let messages = collect_listing(&mailbox);
        assert!(matches!(
            messages.first(),
            Some(ProtocolMessage::PathListBegin {
                request_id: 2,
                result: Ok(_)
            })
        ));
        assert!(messages.iter().all(|message| matches!(
            message,
            ProtocolMessage::PathListBegin { request_id: 2, .. }
                | ProtocolMessage::PathListChunk { request_id: 2, .. }
                | ProtocolMessage::PathListGit { request_id: 2, .. }
        )));
    }
}
