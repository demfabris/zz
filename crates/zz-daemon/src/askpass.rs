//! ssh's password and host-key prompts, answered by the GUI through `SSH_ASKPASS`.

use std::{fmt, path::PathBuf, sync::Arc};

use zeroize::Zeroizing;

#[cfg(unix)]
use std::{
    fs,
    io::{self, Read as _, Write as _},
    os::unix::{
        fs::PermissionsExt as _,
        net::{UnixListener, UnixStream},
    },
    path::Path,
    process::ExitCode,
    sync::atomic::{AtomicU64, Ordering},
    thread,
    time::Duration,
};
#[cfg(windows)]
use std::{
    io::{self, Read as _, Write as _},
    path::Path,
    process::ExitCode,
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
    thread,
};

#[cfg(windows)]
use crate::transport::{LocalListener, LocalStream, LocalTransport, Transport as _};

/// Selects askpass mode and names the socket the helper dials.
pub const ASKPASS_SOCKET_ENV: &str = "ZZ_ASKPASS_SOCKET";
pub(crate) const SSH_ASKPASS_ENV: &str = "SSH_ASKPASS";
pub(crate) const SSH_ASKPASS_REQUIRE_ENV: &str = "SSH_ASKPASS_REQUIRE";
/// ssh sets this only for agent key-use confirmation and the FIDO touch notification.
const SSH_ASKPASS_PROMPT_ENV: &str = "SSH_ASKPASS_PROMPT";

/// The one stable fragment of ssh's host-key prompt; the rest of the body varies.
const HOST_KEY_QUESTION: &str = "(yes/no/[fingerprint])";
/// What ssh re-asks with after it rejects a host-key answer.
const HOST_KEY_RETRY: &str = "Please type 'yes', 'no' or the fingerprint";
const HOST_KEY_HEADER: &str = "The authenticity of host";

/// ssh reads at most 1024 bytes of an answer and keeps only what precedes the first CR or LF.
const MAX_REPLY_BYTES: usize = 1023;

/// Whether ssh named a prompt shape zz does not drive.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AskpassMode {
    /// `SSH_ASKPASS_PROMPT` unset: password, passphrase, keyboard-interactive answer, or host key.
    #[default]
    Answer,
    /// `confirm`: ssh-agent asking permission to use a key.
    AgentConfirm,
    /// `none`: a FIDO token touch. ssh ignores stdout and SIGTERMs the helper on the tap.
    Notification,
}

impl AskpassMode {
    fn from_env_value(value: Option<&str>) -> Self {
        match value {
            Some("confirm") => Self::AgentConfirm,
            Some("none") => Self::Notification,
            _ => Self::Answer,
        }
    }

    const fn wire(self) -> u8 {
        match self {
            Self::Answer => b'a',
            Self::AgentConfirm => b'c',
            Self::Notification => b'n',
        }
    }

    const fn from_wire(byte: u8) -> Option<Self> {
        match byte {
            b'a' => Some(Self::Answer),
            b'c' => Some(Self::AgentConfirm),
            b'n' => Some(Self::Notification),
            _ => None,
        }
    }
}

/// What the dialog has to be, and how a cancel has to be spelled: an empty answer with a zero exit
/// means an empty password, `no` for a host key, and consent for the agent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AskpassPromptKind {
    /// A password, passphrase or keyboard-interactive answer. Returned verbatim; cancelling prints
    /// nothing and exits non-zero.
    Secret,
    /// Host-key confirmation. Answered with exactly `yes` or `no`; cancelling is an explicit `no`.
    HostKey,
    /// ssh-agent key-use confirmation. Cancelling must exit non-zero; an empty answer reads as
    /// consent.
    AgentConfirm,
}

/// One question from ssh.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AskpassPrompt {
    mode: AskpassMode,
    text: String,
    echo: bool,
}

impl AskpassPrompt {
    #[must_use]
    pub fn new(mode: AskpassMode, text: impl Into<String>) -> Self {
        Self {
            mode,
            text: text.into(),
            echo: false,
        }
    }

    #[must_use]
    pub fn with_echo(mut self, echo: bool) -> Self {
        self.echo = echo;
        self
    }

    /// ssh's own words, shown to the user unedited.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    #[must_use]
    pub const fn mode(&self) -> AskpassMode {
        self.mode
    }

    #[must_use]
    pub const fn echo(&self) -> bool {
        self.echo
    }

    #[must_use]
    pub fn kind(&self) -> AskpassPromptKind {
        match self.mode {
            AskpassMode::AgentConfirm => AskpassPromptKind::AgentConfirm,
            AskpassMode::Answer | AskpassMode::Notification => {
                if is_host_key_prompt(&self.text) {
                    AskpassPromptKind::HostKey
                } else {
                    AskpassPromptKind::Secret
                }
            }
        }
    }
}

fn is_host_key_prompt(text: &str) -> bool {
    text.contains(HOST_KEY_QUESTION)
        || text.contains(HOST_KEY_RETRY)
        || text.contains(HOST_KEY_HEADER)
}

/// The answer for one prompt, or the user declining to give one.
pub enum AskpassReply {
    Answer(Zeroizing<String>),
    Cancel,
}

impl AskpassReply {
    #[must_use]
    pub fn answer(value: impl Into<String>) -> Self {
        Self::Answer(Zeroizing::new(value.into()))
    }
}

/// Never renders the answer: a secret must not reach a log line or a panic message.
impl fmt::Debug for AskpassReply {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Answer(_) => "AskpassReply::Answer(<redacted>)",
            Self::Cancel => "AskpassReply::Cancel",
        })
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct HelperOutcome {
    stdout: Option<Zeroizing<Vec<u8>>>,
    success: bool,
}

pub(crate) fn helper_outcome(kind: AskpassPromptKind, reply: &AskpassReply) -> HelperOutcome {
    match (kind, reply) {
        (_, AskpassReply::Answer(answer)) => HelperOutcome {
            stdout: Some(reply_line(answer)),
            success: true,
        },
        (AskpassPromptKind::HostKey, AskpassReply::Cancel) => HelperOutcome {
            stdout: Some(reply_line(&Zeroizing::new("no".to_owned()))),
            success: true,
        },
        (AskpassPromptKind::Secret | AskpassPromptKind::AgentConfirm, AskpassReply::Cancel) => {
            HelperOutcome {
                stdout: None,
                success: false,
            }
        }
    }
}

fn reply_line(answer: &Zeroizing<String>) -> Zeroizing<Vec<u8>> {
    let first_line = answer
        .split(['\r', '\n'])
        .next()
        .unwrap_or_default()
        .as_bytes();
    let mut kept = first_line.len().min(MAX_REPLY_BYTES);
    while kept > 0 && !answer.is_char_boundary(kept) {
        kept -= 1;
    }
    let mut line = Zeroizing::new(Vec::with_capacity(kept + 1));
    line.extend_from_slice(&first_line[..kept]);
    line.push(b'\n');
    line
}

#[cfg(unix)]
const REQUEST_READ_TIMEOUT: Duration = Duration::from_secs(10);
#[cfg(unix)]
const MAX_PROMPT_BYTES: u64 = 64 * 1024;
#[cfg(unix)]
static ASKPASS_COUNTER: AtomicU64 = AtomicU64::new(1);
#[cfg(windows)]
const MAX_PROMPT_BYTES: usize = 64 * 1024;
#[cfg(windows)]
static ASKPASS_COUNTER: AtomicU64 = AtomicU64::new(1);

/// How the GUI answers ssh, and the executable ssh runs to ask.
#[derive(Clone)]
pub struct SshPrompts {
    helper: PathBuf,
    responder: Arc<dyn Fn(&AskpassPrompt) -> AskpassReply + Send + Sync>,
}

impl SshPrompts {
    /// `helper` is the zz executable ssh runs; it re-enters askpass mode through
    /// [`ASKPASS_SOCKET_ENV`].
    pub fn new<R>(helper: PathBuf, responder: R) -> Self
    where
        R: Fn(&AskpassPrompt) -> AskpassReply + Send + Sync + 'static,
    {
        Self {
            helper,
            responder: Arc::new(responder),
        }
    }

    // Only the in-process ssh tier (iOS) asks directly; every other tier goes through the helper.
    #[cfg_attr(not(target_os = "ios"), allow(dead_code))]
    pub(crate) fn respond(&self, prompt: &AskpassPrompt) -> AskpassReply {
        (self.responder)(prompt)
    }
}

impl fmt::Debug for SshPrompts {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SshPrompts")
            .field("helper", &self.helper)
            .finish_non_exhaustive()
    }
}

#[cfg(unix)]
pub(crate) struct AskpassListener {
    helper: PathBuf,
    socket: PathBuf,
    stopped: Arc<std::sync::atomic::AtomicBool>,
}

#[cfg(unix)]
impl AskpassListener {
    pub(crate) fn start(prompts: SshPrompts) -> io::Result<Self> {
        let socket = crate::endpoint::ssh_runtime_dir()?.join(format!(
            "a{}",
            ASKPASS_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_file(&socket);
        let listener = UnixListener::bind(&socket)?;
        fs::set_permissions(&socket, fs::Permissions::from_mode(0o600))?;

        let stopped = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let thread_stopped = Arc::clone(&stopped);
        let responder = Arc::clone(&prompts.responder);
        thread::Builder::new()
            .name("zz-ssh-askpass".to_owned())
            .spawn(move || serve(&listener, &responder, &thread_stopped))?;
        Ok(Self {
            helper: prompts.helper,
            socket,
            stopped,
        })
    }

    pub(crate) fn helper(&self) -> &Path {
        &self.helper
    }

    pub(crate) fn socket(&self) -> &Path {
        &self.socket
    }
}

#[cfg(unix)]
impl Drop for AskpassListener {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::SeqCst);
        let _ = UnixStream::connect(&self.socket);
        let _ = fs::remove_file(&self.socket);
    }
}

#[cfg(unix)]
fn serve(
    listener: &UnixListener,
    responder: &Arc<dyn Fn(&AskpassPrompt) -> AskpassReply + Send + Sync>,
    stopped: &std::sync::atomic::AtomicBool,
) {
    let mut cancelled = false;
    let mut host_key_context: Option<String> = None;
    for stream in listener.incoming() {
        if stopped.load(Ordering::SeqCst) {
            return;
        }
        let Ok(stream) = stream else { continue };
        if let Err(error) = answer_one(
            stream,
            responder,
            &mut cancelled,
            &mut host_key_context,
            stopped,
        ) {
            log::debug!(target: "zz_daemon::askpass", "askpass connection failed: {error}");
        }
    }
}

#[cfg(unix)]
fn answer_one(
    mut stream: UnixStream,
    responder: &Arc<dyn Fn(&AskpassPrompt) -> AskpassReply + Send + Sync>,
    cancelled: &mut bool,
    host_key_context: &mut Option<String>,
    stopped: &std::sync::atomic::AtomicBool,
) -> io::Result<()> {
    stream.set_read_timeout(Some(REQUEST_READ_TIMEOUT))?;
    let mut raw = Vec::new();
    (&mut stream).take(MAX_PROMPT_BYTES).read_to_end(&mut raw)?;
    let Some(prompt) = decode_request(&raw) else {
        return Ok(());
    };
    if stopped.load(Ordering::SeqCst) {
        return Ok(());
    }

    let prompt = restore_host_key_context(prompt, host_key_context);
    let reply = if *cancelled {
        AskpassReply::Cancel
    } else {
        responder(&prompt)
    };
    if matches!(reply, AskpassReply::Cancel) {
        *cancelled = true;
    }
    stream.write_all(&encode_reply(&reply))?;
    stream.flush()
}

// A default-descriptor named pipe is readable by Everyone, so the random pipe name is the only
// boundary this listener has.
#[cfg(windows)]
pub(crate) struct AskpassListener {
    helper: PathBuf,
    socket: PathBuf,
    stopped: Arc<AtomicBool>,
}

#[cfg(windows)]
impl AskpassListener {
    pub(crate) fn start(prompts: SshPrompts) -> io::Result<Self> {
        let (socket, listener) = crate::endpoint::ssh_pipe(
            "zz-askpass",
            ASKPASS_COUNTER.fetch_add(1, Ordering::Relaxed),
        )?;

        let stopped = Arc::new(AtomicBool::new(false));
        let thread_stopped = Arc::clone(&stopped);
        let responder = Arc::clone(&prompts.responder);
        thread::Builder::new()
            .name("zz-ssh-askpass".to_owned())
            .spawn(move || serve(&listener, &responder, &thread_stopped))?;
        Ok(Self {
            helper: prompts.helper,
            socket,
            stopped,
        })
    }

    pub(crate) fn helper(&self) -> &Path {
        &self.helper
    }

    pub(crate) fn socket(&self) -> &Path {
        &self.socket
    }
}

#[cfg(windows)]
impl Drop for AskpassListener {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::SeqCst);
    }
}

#[cfg(windows)]
fn serve(
    listener: &LocalListener,
    responder: &Arc<dyn Fn(&AskpassPrompt) -> AskpassReply + Send + Sync>,
    stopped: &AtomicBool,
) {
    let mut cancelled = false;
    let mut host_key_context: Option<String> = None;
    while let Some(stream) = crate::endpoint::accept_until_stopped(listener, stopped) {
        if let Err(error) = answer_one(
            stream,
            responder,
            &mut cancelled,
            &mut host_key_context,
            stopped,
        ) {
            log::debug!(target: "zz_daemon::askpass", "askpass connection failed: {error}");
        }
    }
}

#[cfg(windows)]
fn answer_one(
    mut stream: LocalStream,
    responder: &Arc<dyn Fn(&AskpassPrompt) -> AskpassReply + Send + Sync>,
    cancelled: &mut bool,
    host_key_context: &mut Option<String>,
    stopped: &AtomicBool,
) -> io::Result<()> {
    let Some(raw) = read_request(&mut stream)? else {
        return Ok(());
    };
    let Some(prompt) = decode_request(&raw) else {
        return Ok(());
    };
    if stopped.load(Ordering::SeqCst) {
        return Ok(());
    }

    let prompt = restore_host_key_context(prompt, host_key_context);
    let reply = if *cancelled {
        AskpassReply::Cancel
    } else {
        responder(&prompt)
    };
    if matches!(reply, AskpassReply::Cancel) {
        *cancelled = true;
    }
    stream.write_all(&encode_reply(&reply))?;
    stream.flush()
}

/// A Windows named pipe has no half-close, so the request carries its own length; the reply still
/// ends at end of stream, because the GUI closes once it has answered.
#[cfg(windows)]
fn read_request(stream: &mut LocalStream) -> io::Result<Option<Vec<u8>>> {
    let mut header = [0_u8; 4];
    if let Err(error) = stream.read_exact(&mut header) {
        return if error.kind() == io::ErrorKind::UnexpectedEof {
            Ok(None)
        } else {
            Err(error)
        };
    }
    let length = u32::from_le_bytes(header) as usize;
    if length == 0 || length > MAX_PROMPT_BYTES {
        return Ok(None);
    }
    let mut raw = vec![0_u8; length];
    stream.read_exact(&mut raw)?;
    Ok(Some(raw))
}

fn restore_host_key_context(
    prompt: AskpassPrompt,
    host_key_context: &mut Option<String>,
) -> AskpassPrompt {
    if prompt.text.contains(HOST_KEY_HEADER) {
        *host_key_context = Some(prompt.text.clone());
        return prompt;
    }
    if !prompt.text.contains(HOST_KEY_RETRY) {
        return prompt;
    }
    let Some(context) = host_key_context.as_deref() else {
        return prompt;
    };
    let text = format!("{context}\n{}", prompt.text);
    AskpassPrompt::new(prompt.mode, text)
}

fn encode_request(prompt: &AskpassPrompt) -> Vec<u8> {
    let mut request = Vec::with_capacity(prompt.text.len() + 1);
    request.push(prompt.mode.wire());
    request.extend_from_slice(prompt.text.as_bytes());
    request
}

fn decode_request(raw: &[u8]) -> Option<AskpassPrompt> {
    let (mode, text) = raw.split_first()?;
    let mode = AskpassMode::from_wire(*mode)?;
    Some(AskpassPrompt::new(
        mode,
        String::from_utf8_lossy(text).into_owned(),
    ))
}

const REPLY_ANSWER: u8 = b'A';
const REPLY_CANCEL: u8 = b'C';

fn encode_reply(reply: &AskpassReply) -> Zeroizing<Vec<u8>> {
    match reply {
        AskpassReply::Answer(answer) => {
            let mut encoded = Zeroizing::new(Vec::with_capacity(answer.len() + 1));
            encoded.push(REPLY_ANSWER);
            encoded.extend_from_slice(answer.as_bytes());
            encoded
        }
        AskpassReply::Cancel => Zeroizing::new(vec![REPLY_CANCEL]),
    }
}

fn decode_reply(raw: &[u8]) -> AskpassReply {
    match raw.split_first() {
        Some((&REPLY_ANSWER, answer)) => {
            AskpassReply::Answer(Zeroizing::new(String::from_utf8_lossy(answer).into_owned()))
        }
        _ => AskpassReply::Cancel,
    }
}

/// Answer one ssh prompt over `socket` and exit, writing the answer to stdout for ssh to read.
///
/// Never fork here: ssh leaks the write end of the answer pipe, so a surviving child blocks ssh in
/// `read` forever.
#[cfg(any(unix, windows))]
#[must_use]
pub fn run_helper(socket: &Path, prompt: &str) -> ExitCode {
    let mode = AskpassMode::from_env_value(std::env::var(SSH_ASKPASS_PROMPT_ENV).ok().as_deref());
    if mode == AskpassMode::Notification {
        return ExitCode::SUCCESS;
    }

    let prompt = AskpassPrompt::new(mode, prompt);
    let reply = match exchange(socket, &prompt) {
        Ok(reply) => reply,
        Err(error) => {
            eprintln!("zz: could not reach the zz window to ask for a password: {error}");
            AskpassReply::Cancel
        }
    };
    let outcome = helper_outcome(prompt.kind(), &reply);
    if let Some(bytes) = &outcome.stdout {
        let mut stdout = io::stdout().lock();
        if stdout
            .write_all(bytes)
            .and_then(|()| stdout.flush())
            .is_err()
        {
            return ExitCode::FAILURE;
        }
    }
    if outcome.success {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

#[cfg(unix)]
fn exchange(socket: &Path, prompt: &AskpassPrompt) -> io::Result<AskpassReply> {
    let mut stream = UnixStream::connect(socket)?;
    stream.write_all(&encode_request(prompt))?;
    stream.shutdown(std::net::Shutdown::Write)?;
    let mut raw = Zeroizing::new(Vec::new());
    stream.read_to_end(&mut raw)?;
    Ok(decode_reply(&raw))
}

#[cfg(windows)]
fn exchange(socket: &Path, prompt: &AskpassPrompt) -> io::Result<AskpassReply> {
    let mut stream = LocalTransport::connect(socket)?;
    let request = encode_request(prompt);
    let length = u32::try_from(request.len()).map_err(io::Error::other)?;
    stream.write_all(&length.to_le_bytes())?;
    stream.write_all(&request)?;
    stream.flush()?;
    let mut raw = Zeroizing::new(Vec::new());
    stream.read_to_end(&mut raw)?;
    Ok(decode_reply(&raw))
}

#[cfg(test)]
mod tests {
    use super::*;

    const PASSWORD_PROMPT: &str = "demfabris@xps's password: ";
    const HOST_KEY_PROMPT: &str = "The authenticity of host '[localhost]:2222 ([::1]:2222)' can't \
                                   be established.\nED25519 key fingerprint is \
                                   SHA256:abcdefghijklmnopqrstuvwxyz0123456789ABCDEFG.\nThis key \
                                   is not known by any other names.\nAre you sure you want to \
                                   continue connecting (yes/no/[fingerprint])? ";

    fn kind(text: &str) -> AskpassPromptKind {
        AskpassPrompt::new(AskpassMode::Answer, text).kind()
    }

    #[test]
    fn host_key_questions_classify_as_confirmations_however_they_are_dressed() {
        assert_eq!(kind(HOST_KEY_PROMPT), AskpassPromptKind::HostKey);
        assert_eq!(
            kind(
                "The authenticity of host 'gpu (10.0.0.2)' can't be established.\n+---[ED25519 \
                 256]---+\n|      .o+o       |\n+----[SHA256]-----+\nBut keys of different type \
                 are already known for this host.\nAre you sure you want to continue connecting \
                 (yes/no/[fingerprint])? "
            ),
            AskpassPromptKind::HostKey
        );
        assert_eq!(
            kind("Please type 'yes', 'no' or the fingerprint: "),
            AskpassPromptKind::HostKey
        );
    }

    #[test]
    fn everything_else_is_a_secret() {
        assert_eq!(kind(PASSWORD_PROMPT), AskpassPromptKind::Secret);
        assert_eq!(
            kind("Enter passphrase for \"/Users/fabrico/.ssh/id_ed25519\": "),
            AskpassPromptKind::Secret
        );
        assert_eq!(
            kind("(fabrico@xps) Verification code: "),
            AskpassPromptKind::Secret
        );
        assert_eq!(kind(""), AskpassPromptKind::Secret);
        assert_eq!(kind("   \n\t "), AskpassPromptKind::Secret);
    }

    #[test]
    fn the_confirmation_marker_wins_over_password_wording() {
        assert_eq!(
            kind(
                "Password authentication is disabled.\nAre you sure you want to continue \
                 connecting (yes/no/[fingerprint])? "
            ),
            AskpassPromptKind::HostKey
        );
    }

    #[test]
    fn the_prompt_env_var_selects_the_two_shapes_zz_does_not_drive() {
        assert_eq!(AskpassMode::from_env_value(None), AskpassMode::Answer);
        assert_eq!(
            AskpassMode::from_env_value(Some("confirm")),
            AskpassMode::AgentConfirm
        );
        assert_eq!(
            AskpassMode::from_env_value(Some("none")),
            AskpassMode::Notification
        );
        assert_eq!(
            AskpassPrompt::new(AskpassMode::AgentConfirm, "Allow use of key?").kind(),
            AskpassPromptKind::AgentConfirm
        );
    }

    fn outcome(kind: AskpassPromptKind, reply: &AskpassReply) -> (Option<String>, bool) {
        let outcome = helper_outcome(kind, reply);
        (
            outcome
                .stdout
                .map(|bytes| String::from_utf8(bytes.to_vec()).expect("utf-8")),
            outcome.success,
        )
    }

    #[test]
    fn a_secret_is_returned_verbatim_and_a_cancel_says_nothing_at_all() {
        assert_eq!(
            outcome(
                AskpassPromptKind::Secret,
                &AskpassReply::answer("  hunter2  ")
            ),
            (Some("  hunter2  \n".to_owned()), true),
            "ssh does not trim a password, so neither may zz",
        );
        assert_eq!(
            outcome(AskpassPromptKind::Secret, &AskpassReply::Cancel),
            (None, false),
        );
    }

    #[test]
    fn cancelling_is_spelled_differently_for_each_prompt_kind() {
        assert_eq!(
            outcome(AskpassPromptKind::HostKey, &AskpassReply::Cancel),
            (Some("no\n".to_owned()), true),
        );
        assert_eq!(
            outcome(AskpassPromptKind::AgentConfirm, &AskpassReply::Cancel),
            (None, false),
        );
        assert_eq!(
            outcome(AskpassPromptKind::HostKey, &AskpassReply::answer("yes")),
            (Some("yes\n".to_owned()), true),
        );
    }

    #[test]
    fn replies_are_cut_where_ssh_would_cut_them() {
        assert_eq!(
            outcome(
                AskpassPromptKind::Secret,
                &AskpassReply::answer("first\nsecond")
            ),
            (Some("first\n".to_owned()), true),
        );
        assert_eq!(
            outcome(
                AskpassPromptKind::Secret,
                &AskpassReply::answer("first\r\nsecond")
            ),
            (Some("first\n".to_owned()), true),
        );
        let long = "é".repeat(1024);
        let (stdout, success) = outcome(AskpassPromptKind::Secret, &AskpassReply::answer(long));
        assert!(success);
        let stdout = stdout.expect("a truncated answer still prints");
        assert!(stdout.len() <= MAX_REPLY_BYTES + 1, "{}", stdout.len());
        assert!(
            stdout.trim_end_matches('\n').chars().all(|c| c == 'é'),
            "the cut must land on a character boundary",
        );
    }

    #[test]
    fn the_wire_format_round_trips_prompts_and_replies() {
        for mode in [
            AskpassMode::Answer,
            AskpassMode::AgentConfirm,
            AskpassMode::Notification,
        ] {
            let prompt = AskpassPrompt::new(mode, HOST_KEY_PROMPT);
            assert_eq!(decode_request(&encode_request(&prompt)), Some(prompt));
        }
        assert_eq!(decode_request(&[]), None);
        assert_eq!(decode_request(b"?nonsense"), None);

        let decoded = decode_reply(&encode_reply(&AskpassReply::answer("hunter2")));
        assert!(matches!(decoded, AskpassReply::Answer(answer) if *answer == "hunter2"));
        assert!(matches!(
            decode_reply(&encode_reply(&AskpassReply::Cancel)),
            AskpassReply::Cancel
        ));
        assert!(matches!(decode_reply(&[]), AskpassReply::Cancel));
    }

    #[test]
    fn a_rejected_host_key_answer_is_re_asked_with_its_original_context() {
        let mut context = None;
        let first = restore_host_key_context(
            AskpassPrompt::new(AskpassMode::Answer, HOST_KEY_PROMPT),
            &mut context,
        );
        assert_eq!(first.text(), HOST_KEY_PROMPT);

        let retry = restore_host_key_context(
            AskpassPrompt::new(
                AskpassMode::Answer,
                "Please type 'yes', 'no' or the fingerprint: ",
            ),
            &mut context,
        );
        assert!(retry.text().contains("The authenticity of host"));
        assert!(retry.text().contains("Please type 'yes'"));
        assert_eq!(retry.kind(), AskpassPromptKind::HostKey);

        let secret = restore_host_key_context(
            AskpassPrompt::new(AskpassMode::Answer, PASSWORD_PROMPT),
            &mut context,
        );
        assert_eq!(secret.text(), PASSWORD_PROMPT);
    }

    #[test]
    fn a_retry_without_context_is_still_a_confirmation() {
        let mut context = None;
        let retry = restore_host_key_context(
            AskpassPrompt::new(
                AskpassMode::Answer,
                "Please type 'yes', 'no' or the fingerprint: ",
            ),
            &mut context,
        );
        assert_eq!(retry.kind(), AskpassPromptKind::HostKey);
    }

    #[test]
    fn a_reply_never_renders_its_answer() {
        assert_eq!(
            format!("{:?}", AskpassReply::answer("hunter2")),
            "AskpassReply::Answer(<redacted>)"
        );
    }

    #[cfg(unix)]
    fn ask(socket: &Path, text: &str) -> AskpassReply {
        exchange(socket, &AskpassPrompt::new(AskpassMode::Answer, text)).expect("askpass exchange")
    }

    #[cfg(unix)]
    #[test]
    fn the_socket_round_trips_an_answer_and_latches_after_a_cancel() {
        use std::sync::atomic::AtomicUsize;

        let asked = Arc::new(AtomicUsize::new(0));
        let responder_asked = Arc::clone(&asked);
        let listener = AskpassListener::start(SshPrompts::new(
            PathBuf::from("/nonexistent/zz"),
            move |_: &AskpassPrompt| {
                if responder_asked.fetch_add(1, Ordering::SeqCst) == 0 {
                    AskpassReply::answer("hunter2")
                } else {
                    AskpassReply::Cancel
                }
            },
        ))
        .expect("askpass listener");

        let socket = listener.socket().to_path_buf();
        assert!(
            matches!(ask(&socket, PASSWORD_PROMPT), AskpassReply::Answer(answer) if *answer == "hunter2")
        );
        assert!(matches!(
            ask(&socket, PASSWORD_PROMPT),
            AskpassReply::Cancel
        ));
        assert!(matches!(
            ask(&socket, PASSWORD_PROMPT),
            AskpassReply::Cancel
        ));
        assert_eq!(
            asked.load(Ordering::SeqCst),
            2,
            "a cancelled attempt must not ask again",
        );

        drop(listener);
        assert!(!socket.exists(), "the socket outlived its listener");
    }

    #[cfg(unix)]
    #[test]
    fn the_socket_is_private_to_this_user() {
        let listener = AskpassListener::start(SshPrompts::new(
            PathBuf::from("/nonexistent/zz"),
            |_: &AskpassPrompt| AskpassReply::Cancel,
        ))
        .expect("askpass listener");
        let socket = fs::metadata(listener.socket()).expect("socket metadata");
        assert_eq!(socket.permissions().mode() & 0o777, 0o600);

        let directory = fs::metadata(
            listener
                .socket()
                .parent()
                .expect("the socket lives in a directory"),
        )
        .expect("directory metadata");
        assert_eq!(directory.permissions().mode() & 0o777, 0o700);
    }

    #[cfg(unix)]
    #[test]
    fn a_host_key_answer_crosses_the_socket_verbatim() {
        let listener = AskpassListener::start(SshPrompts::new(
            PathBuf::from("/nonexistent/zz"),
            |prompt: &AskpassPrompt| match prompt.kind() {
                AskpassPromptKind::HostKey => AskpassReply::answer("yes"),
                AskpassPromptKind::Secret | AskpassPromptKind::AgentConfirm => AskpassReply::Cancel,
            },
        ))
        .expect("askpass listener");

        let reply = ask(listener.socket(), HOST_KEY_PROMPT);
        assert!(matches!(&reply, AskpassReply::Answer(answer) if **answer == *"yes"));
        assert_eq!(
            helper_outcome(AskpassPromptKind::HostKey, &reply)
                .stdout
                .map(|bytes| String::from_utf8(bytes.to_vec()).expect("utf-8")),
            Some("yes\n".to_owned()),
        );
    }
}
