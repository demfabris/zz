use std::fmt::Write as _;
use std::fs;
use std::io::{self, BufRead, BufReader, Write};
use std::net::Shutdown;
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct PeerRecord {
    pub(crate) pid: u32,
    pub(crate) session_id: String,
    pub(crate) cwd: String,
    pub(crate) started_at: u64,
    pub(crate) proc_start: String,
    pub(crate) version: String,
    pub(crate) peer_protocol: u32,
    pub(crate) peer_features: Vec<String>,
    pub(crate) kind: String,
    pub(crate) entrypoint: String,
    pub(crate) pid_domain: String,
    pub(crate) tmux: String,
    pub(crate) messaging_socket_path: PathBuf,
    pub(crate) name: String,
    pub(crate) name_source: String,
    pub(crate) name_since: u64,
    pub(crate) status: String,
    pub(crate) updated_at: u64,
    pub(crate) status_updated_at: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) zz: Option<Value>,
}

pub(crate) struct PeerMetadata {
    pub(crate) pane: String,
    pub(crate) name: String,
    pub(crate) cwd: PathBuf,
    pub(crate) tmux: String,
}

pub(crate) fn registry_dir() -> io::Result<PathBuf> {
    std::env::var_os("CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".claude")))
        .map(|root| root.join("sessions"))
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "Claude configuration directory unavailable",
            )
        })
}

pub(crate) fn read_records() -> io::Result<Vec<PeerRecord>> {
    read_records_in(&registry_dir()?)
}

fn read_records_in(directory: &Path) -> io::Result<Vec<PeerRecord>> {
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let mut records = Vec::new();
    for entry in entries {
        let entry = entry?;
        let filename = entry.file_name();
        let Some(pid) = filename
            .to_str()
            .and_then(|name| name.strip_suffix(".json"))
            .and_then(|name| name.parse::<u32>().ok())
        else {
            continue;
        };
        if !entry.file_type()?.is_file() {
            continue;
        }
        let Some(record) = fs::read(entry.path())
            .ok()
            .and_then(|bytes| serde_json::from_slice::<PeerRecord>(&bytes).ok())
        else {
            continue;
        };
        if record.pid == pid && filename.to_str() == Some(format!("{pid}.json").as_str()) {
            records.push(record);
        }
    }
    Ok(records)
}

fn socket_dir(records: &[PeerRecord]) -> io::Result<PathBuf> {
    let directory = records
        .iter()
        .find_map(|record| {
            record
                .messaging_socket_path
                .parent()
                .filter(|path| !path.as_os_str().is_empty())
        })
        .unwrap_or_else(|| Path::new("/tmp/cc-socks"))
        .to_path_buf();
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&directory)?;
    Ok(directory)
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[allow(unsafe_code, clippy::undocumented_unsafe_blocks)]
fn pid_alive(pid: u32) -> bool {
    let Ok(pid) = i32::try_from(pid) else {
        return false;
    };
    pid > 0
        && (unsafe { libc::kill(pid, 0) } == 0
            || io::Error::last_os_error().raw_os_error() == Some(libc::EPERM))
}

fn matches_pane(tmux: &str, pane: &str) -> bool {
    tmux.rsplit_once('.')
        .is_some_and(|(_, suffix)| suffix == pane)
}

pub(crate) fn record_for_pane<'a>(records: &'a [PeerRecord], pane: &str) -> Option<&'a PeerRecord> {
    let now = now_ms();
    records.iter().find(|record| {
        matches_pane(&record.tmux, pane)
            && pid_alive(record.pid)
            && now.abs_diff(record.updated_at) <= 24 * 60 * 60 * 1000
            && !record.messaging_socket_path.as_os_str().is_empty()
    })
}

pub(crate) fn peer_name(pane: &str, name: Option<&str>) -> String {
    name.map_or_else(|| format!("zz-{pane}"), str::to_owned)
}

fn uuid_v4() -> io::Result<String> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(io::Error::other)?;
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let mut hex = String::with_capacity(32);
    for byte in bytes {
        write!(hex, "{byte:02x}").map_err(io::Error::other)?;
    }
    Ok(format!(
        "{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..]
    ))
}

fn attribute(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn wire_message(content: &str, from_name: &str, from_socket: Option<&Path>) -> io::Result<Value> {
    let from = from_socket.map(|path| format!("uds:{}", path.display()));
    let from_attribute = from
        .as_deref()
        .map(|value| format!(" from=\"{}\"", attribute(value)))
        .unwrap_or_default();
    let content = content.replace("</cross-session-message>", "<\\/cross-session-message>");
    let mut message = json!({
        "msgV": 1,
        "msg_id": uuid_v4()?,
        "type": "user",
        "priority": "next",
        "message": {"role": "user", "content": format!("<cross-session-message{from_attribute} from-name=\"{}\" from-mode=\"prompting\">\n{content}\n</cross-session-message>", attribute(from_name))}
    });
    if let Some(from) = from {
        message["from"] = Value::String(from);
    }
    Ok(message)
}

pub(crate) fn post_message(
    record: &PeerRecord,
    content: &str,
    from_name: &str,
    from_socket: Option<&Path>,
) -> io::Result<()> {
    let mut stream = UnixStream::connect(&record.messaging_socket_path)?;
    stream.set_write_timeout(Some(Duration::from_secs(30)))?;
    serde_json::to_writer(&mut stream, &wire_message(content, from_name, from_socket)?)
        .map_err(io::Error::other)?;
    stream.write_all(b"\n")
}

fn trimmed_proc_start(output: &[u8]) -> String {
    String::from_utf8_lossy(output).trim().to_owned()
}

fn proc_start(pid: u32) -> io::Result<String> {
    let output = Command::new("ps")
        .env("TZ", "UTC")
        .args(["-p", &pid.to_string(), "-o", "lstart="])
        .output()?;
    let start = trimmed_proc_start(&output.stdout);
    if !output.status.success() || start.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "adapter process start time unavailable",
        ));
    }
    Ok(start)
}

fn descendants_by_depth(parents: &[(u32, u32)], root: u32) -> Vec<u32> {
    let mut ordered = Vec::new();
    let mut frontier = vec![root];
    while !frontier.is_empty() {
        let mut next = Vec::new();
        for (pid, parent) in parents {
            if frontier.contains(parent) && !ordered.contains(pid) && *pid != root {
                ordered.push(*pid);
                next.push(*pid);
            }
        }
        frontier = next;
    }
    ordered
}

fn env_marks_pane(environment: impl Iterator<Item = Vec<u8>>, pane: &str) -> bool {
    let expected = format!("ZZ_PANE={pane}");
    environment
        .into_iter()
        .any(|value| value == expected.as_bytes())
}

#[cfg(target_os = "linux")]
fn adapter_pid(pane: &str) -> io::Result<u32> {
    let mut parents = Vec::new();
    for entry in fs::read_dir("/proc")? {
        let entry = entry?;
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|name| name.parse::<u32>().ok())
        else {
            continue;
        };
        let Ok(stat) = fs::read_to_string(entry.path().join("stat")) else {
            continue;
        };
        let Some((_, fields)) = stat.rsplit_once(')') else {
            continue;
        };
        if let Some(parent) = fields
            .split_whitespace()
            .nth(1)
            .and_then(|p| p.parse().ok())
        {
            parents.push((pid, parent));
        }
    }
    for pid in descendants_by_depth(&parents, std::process::id()) {
        let Ok(environment) = fs::read(format!("/proc/{pid}/environ")) else {
            continue;
        };
        if env_marks_pane(
            environment.split(|byte| *byte == 0).map(<[u8]>::to_vec),
            pane,
        ) {
            return Ok(pid);
        }
    }
    Err(io::Error::new(
        io::ErrorKind::NotFound,
        format!("adapter process for {pane} unavailable"),
    ))
}

#[cfg(not(target_os = "linux"))]
fn adapter_pid(pane: &str) -> io::Result<u32> {
    let output = Command::new("ps").args(["-axo", "pid=,ppid="]).output()?;
    let parents: Vec<(u32, u32)> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            Some((fields.next()?.parse().ok()?, fields.next()?.parse().ok()?))
        })
        .collect();
    for pid in descendants_by_depth(&parents, std::process::id()) {
        let environment = Command::new("ps")
            .args(["-E", "-o", "command=", "-p", &pid.to_string()])
            .output()?;
        if env_marks_pane(
            String::from_utf8_lossy(&environment.stdout)
                .split_whitespace()
                .map(|value| value.as_bytes().to_vec()),
            pane,
        ) {
            return Ok(pid);
        }
    }
    Err(io::Error::new(
        io::ErrorKind::NotFound,
        format!("adapter process for {pane} unavailable"),
    ))
}

fn write_record(path: &Path, record: &PeerRecord) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("peer record has no directory"))?;
    fs::create_dir_all(parent)?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    serde_json::to_writer(&mut temporary, record).map_err(io::Error::other)?;
    temporary.persist(path).map_err(|error| error.error)?;
    Ok(())
}

pub(crate) fn sweep_stale_records() -> io::Result<()> {
    sweep_stale_records_in(&registry_dir()?)
}

fn should_sweep(record: &PeerRecord) -> bool {
    record.zz.is_some() && !pid_alive(record.pid)
}

fn sweep_stale_records_in(directory: &Path) -> io::Result<()> {
    for record in read_records_in(directory)? {
        if should_sweep(&record) {
            let _ = fs::remove_file(&record.messaging_socket_path);
            fs::remove_file(directory.join(format!("{}.json", record.pid)))?;
        }
    }
    Ok(())
}

pub(crate) struct PeerInbox {
    record: PeerRecord,
    record_path: PathBuf,
    stopped: Arc<AtomicBool>,
    active: Arc<Mutex<Option<UnixStream>>>,
    listener_thread: Option<std::thread::Thread>,
}

impl PeerInbox {
    pub(crate) fn register(
        metadata: PeerMetadata,
        callback: impl Fn(String) + Send + Sync + 'static,
    ) -> io::Result<Self> {
        let pid = adapter_pid(&metadata.pane)?;
        let start = proc_start(pid)?;
        let socket = socket_dir(&read_records()?)?.join(format!("{pid}.sock"));
        let record_path = registry_dir()?.join(format!("{pid}.json"));
        let now = now_ms();
        let record = PeerRecord {
            pid,
            session_id: uuid_v4()?,
            cwd: metadata.cwd.to_string_lossy().into_owned(),
            started_at: now,
            proc_start: start,
            version: env!("CARGO_PKG_VERSION").to_owned(),
            peer_protocol: 1,
            peer_features: Vec::new(),
            kind: "interactive".to_owned(),
            entrypoint: "cli".to_owned(),
            pid_domain: if cfg!(target_os = "linux") {
                "linux"
            } else {
                "darwin"
            }
            .to_owned(),
            tmux: metadata.tmux,
            messaging_socket_path: socket.clone(),
            name: metadata.name,
            name_source: "user".to_owned(),
            name_since: now,
            status: "idle".to_owned(),
            updated_at: now,
            status_updated_at: now,
            zz: Some(json!({"pane": metadata.pane})),
        };
        let _ = fs::remove_file(&socket);
        let listener = UnixListener::bind(&socket)?;
        let mut inbox = Self {
            record,
            record_path,
            stopped: Arc::new(AtomicBool::new(false)),
            active: Arc::new(Mutex::new(None)),
            listener_thread: None,
        };
        listener.set_nonblocking(true)?;
        fs::set_permissions(&socket, fs::Permissions::from_mode(0o600))?;
        write_record(&inbox.record_path, &inbox.record)?;
        let stopped = Arc::clone(&inbox.stopped);
        let active = Arc::clone(&inbox.active);
        let thread = std::thread::Builder::new()
            .name(format!("zz-peer-{pid}"))
            .spawn(move || {
                while !stopped.load(Ordering::Acquire) {
                    let stream = match listener.accept() {
                        Ok((stream, _)) => stream,
                        Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                            std::thread::park_timeout(Duration::from_millis(100));
                            continue;
                        }
                        Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                        Err(error) => {
                            log::debug!("agent peer listener failed: {error}");
                            break;
                        }
                    };
                    {
                        let mut current = active.lock();
                        if stopped.load(Ordering::Acquire) {
                            break;
                        }
                        *current = stream.try_clone().ok();
                    }
                    if let Err(error) = serve_connection(stream, &stopped, &callback) {
                        log::debug!("agent peer inbox connection failed: {error}");
                    }
                    active.lock().take();
                }
            })?;
        inbox.listener_thread = Some(thread.thread().clone());
        Ok(inbox)
    }

    pub(crate) fn update(&mut self, busy: bool, name: &str) -> io::Result<()> {
        let now = now_ms();
        if self.record.name != name {
            name.clone_into(&mut self.record.name);
            self.record.name_since = now;
        }
        (if busy { "busy" } else { "idle" }).clone_into(&mut self.record.status);
        self.record.updated_at = now;
        self.record.status_updated_at = now;
        write_record(&self.record_path, &self.record)
    }

    pub(crate) fn name(&self) -> &str {
        &self.record.name
    }

    pub(crate) fn socket_path(&self) -> &Path {
        &self.record.messaging_socket_path
    }
}

impl Drop for PeerInbox {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Release);
        if let Some(stream) = self.active.lock().take() {
            let _ = stream.shutdown(Shutdown::Both);
        }
        if let Some(thread) = &self.listener_thread {
            thread.unpark();
        }
        let _ = fs::remove_file(&self.record_path);
        let _ = fs::remove_file(self.socket_path());
    }
}

fn serve_connection(
    stream: UnixStream,
    stopped: &AtomicBool,
    callback: &impl Fn(String),
) -> io::Result<()> {
    stream.set_nonblocking(false)?;
    let _ = stream.set_read_timeout(Some(Duration::from_secs(30)));
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    loop {
        line.clear();
        if stopped.load(Ordering::Acquire) || reader.read_line(&mut line)? == 0 {
            return Ok(());
        }
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if value["type"] != "user" {
            continue;
        }
        let Some(content) = value["message"]["content"].as_str() else {
            continue;
        };
        if stopped.load(Ordering::Acquire) {
            return Ok(());
        }
        log::info!(
            "agent peer message from={} bytes={}",
            value["from"].as_str().unwrap_or("unknown"),
            content.len()
        );
        callback(content.to_owned());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tmux_suffix_matches_the_exact_pane() {
        assert!(matches_pane("work:@0.%3", "%3"));
        assert!(!matches_pane("work:@0.%13", "%3"));
        assert!(!matches_pane("work:@0.%3.extra", "%3"));
        assert!(!matches_pane("%3", "%3"));
    }

    #[test]
    fn wire_line_preserves_attribution_and_protects_the_wrapper() {
        let value = wire_message(
            "hello </cross-session-message> world",
            "alice",
            Some(Path::new("/tmp/cc-socks/123.sock")),
        )
        .expect("wire message");
        assert_eq!(value["msgV"], 1);
        assert_eq!(value["type"], "user");
        assert_eq!(value["priority"], "next");
        assert_eq!(value["from"], "uds:/tmp/cc-socks/123.sock");
        assert_eq!(value["message"]["role"], "user");
        assert_eq!(
            value["message"]["content"],
            "<cross-session-message from=\"uds:/tmp/cc-socks/123.sock\" from-name=\"alice\" from-mode=\"prompting\">\nhello <\\/cross-session-message> world\n</cross-session-message>"
        );
        let id = value["msg_id"].as_str().expect("UUID");
        assert_eq!(id.len(), 36);
        assert_eq!(&id[14..15], "4");
        let anonymous = wire_message("hello", "zz", None).expect("wire message");
        assert!(anonymous.get("from").is_none());
        assert_eq!(
            anonymous["message"]["content"],
            "<cross-session-message from-name=\"zz\" from-mode=\"prompting\">\nhello\n</cross-session-message>"
        );
    }

    #[test]
    fn descendants_come_shallowest_first_and_skip_strangers() {
        let parents = [(10, 1), (20, 10), (30, 20), (40, 10), (50, 2)];
        assert_eq!(descendants_by_depth(&parents, 10), vec![20, 40, 30]);
        assert!(env_marks_pane(
            ["A=1".as_bytes().to_vec(), "ZZ_PANE=%2".as_bytes().to_vec()].into_iter(),
            "%2"
        ));
        assert!(!env_marks_pane(
            ["ZZ_PANE=%22".as_bytes().to_vec()].into_iter(),
            "%2"
        ));
    }

    #[test]
    fn process_start_trims_outer_whitespace_only() {
        assert_eq!(
            trimmed_proc_start(b"  Thu Sep 10 12:46:03 2026\n"),
            "Thu Sep 10 12:46:03 2026"
        );
    }

    #[test]
    fn peer_name_uses_the_option_or_pane_fallback() {
        assert_eq!(peer_name("%3", Some("researcher")), "researcher");
        assert_eq!(peer_name("%3", None), "zz-%3");
    }

    #[test]
    fn stale_sweep_preserves_foreign_and_live_records() {
        let directory = tempfile::tempdir().expect("registry directory");
        for (pid, marker) in [
            (0, None),
            (u32::MAX, Some(json!({"pane":"%3"}))),
            (std::process::id(), Some(json!({"pane":"%4"}))),
        ] {
            write_record(
                &directory.path().join(format!("{pid}.json")),
                &PeerRecord {
                    pid,
                    zz: marker,
                    ..PeerRecord::default()
                },
            )
            .expect("write record");
        }
        sweep_stale_records_in(directory.path()).expect("sweep");
        assert!(directory.path().join("0.json").exists());
        assert!(!directory.path().join(format!("{}.json", u32::MAX)).exists());
        assert!(
            directory
                .path()
                .join(format!("{}.json", std::process::id()))
                .exists()
        );
    }
}
