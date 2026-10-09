//! Persistent mux daemon: session state, PTY workers, and client connections.
#![cfg_attr(
    test,
    allow(
        clippy::disallowed_methods,
        reason = "tests spawn helper processes from threads with an empty signal mask"
    )
)]

use std::{
    ffi::{OsStr, OsString},
    path::Path,
    process::Command,
};

use zz_daemon_client::STARTUP_REENTRY_ENVIRONMENT_VARIABLE;
use zz_protocol::RawText;

pub(crate) const PARENT_CLAUDE_SESSION_ENVIRONMENT: &[&str] = &[
    "CLAUDECODE",
    "CLAUDE_CODE_CHILD_SESSION",
    "CLAUDE_CODE_SESSION_ID",
    "CLAUDE_CODE_SESSION_ATTENDED",
    "CLAUDE_CODE_ENTRYPOINT",
    "CLAUDE_CODE_EXECPATH",
    "CLAUDE_CODE_MESSAGING_SOCKET",
    "CLAUDE_CODE_MESSAGING_TOKEN",
    "CLAUDE_PID",
    "CLAUDE_EFFORT",
];
const TMUX_SHIM_EXECUTABLE_ENVIRONMENT_VARIABLE: &str = "ZZ_TMUX_EXECUTABLE";
const TMUX_SHIM_DIRECTORY_ENVIRONMENT_VARIABLE: &str = "ZZ_TMUX_SHIM_DIR";

#[cfg(feature = "agent")]
mod agent;
#[cfg_attr(not(feature = "agent"), allow(dead_code))]
mod bounded_command;
mod daemon;
mod keys;
mod status;
mod wake;

#[cfg(feature = "agent")]
pub use agent::stream::{
    AgentAuthMethod, AgentImage as AgentStreamImage, AgentPrompt, AgentPromptOutcome,
    AgentSessionCapabilities, AgentSessionSummary, AgentStreamItem, AgentStreamPayload,
};
pub use daemon::path_listing::path_walk_enters;
pub use daemon::{
    CommandStdinSink, Daemon, agent_send_reads_stdin, append_stdin_payload, command_stdin_sink,
    exec_resume_kind, load_buffer_reads_stdin, send_text_reads_stdin,
};

fn tmux_shim_environment(
    tmux_shim: Option<&Path>,
    zz_executable: Option<&Path>,
    path: Option<&OsStr>,
) -> Vec<(OsString, OsString)> {
    let (Some(tmux_shim), Some(zz_executable)) = (tmux_shim, zz_executable) else {
        return Vec::new();
    };
    let paths = std::iter::once(tmux_shim.to_path_buf())
        .chain(path.into_iter().flat_map(std::env::split_paths));
    let mut environment = Vec::with_capacity(3);
    if let Ok(path) = std::env::join_paths(paths) {
        environment.push(("PATH".into(), path));
    }
    environment.push((
        TMUX_SHIM_EXECUTABLE_ENVIRONMENT_VARIABLE.into(),
        zz_executable.as_os_str().to_owned(),
    ));
    environment.push((
        TMUX_SHIM_DIRECTORY_ENVIRONMENT_VARIABLE.into(),
        tmux_shim.as_os_str().to_owned(),
    ));
    environment
}

#[cfg(unix)]
fn configure_pane_tmux_environment(
    environment: &mut Vec<(OsString, Option<OsString>)>,
    tmux_shim: &Path,
    zz_executable: &Path,
) {
    let path = environment
        .iter()
        .rev()
        .find_map(|(name, value)| (name == "PATH").then_some(value.as_deref()))
        .flatten();
    let shim = tmux_shim_environment(Some(tmux_shim), Some(zz_executable), path);
    environment.extend(shim.into_iter().map(|(name, value)| (name, Some(value))));
}

fn configure_shell_job_environment(
    process: &mut Command,
    environment: &[(RawText, Option<RawText>)],
    default_terminal: &str,
    startup: bool,
    tmux: &str,
    zz_socket: &OsStr,
    tmux_shim: Option<&Path>,
    zz_executable: Option<&Path>,
) {
    let path = environment
        .iter()
        .rev()
        .find_map(|(name, value)| (name == "PATH").then_some(value.as_ref()))
        .flatten()
        .map(RawText::to_os_string);
    process.env_clear();
    for (name, value) in environment {
        if matches!(
            name.as_str(),
            STARTUP_REENTRY_ENVIRONMENT_VARIABLE
                | TMUX_SHIM_EXECUTABLE_ENVIRONMENT_VARIABLE
                | TMUX_SHIM_DIRECTORY_ENVIRONMENT_VARIABLE
        ) {
            continue;
        }
        match value {
            Some(value) => {
                process.env(name.to_os_string(), value.to_os_string());
            }
            None => {
                process.env_remove(name.to_os_string());
            }
        }
    }
    if !startup {
        let version = zz_protocol::CommandSpec::TMUX_VERSION_OUTPUT
            .strip_prefix("tmux ")
            .expect("tmux version output prefix");
        process
            .env("TERM", default_terminal)
            .env("TERM_PROGRAM", "tmux")
            .env("TERM_PROGRAM_VERSION", version)
            .env("COLORTERM", "truecolor");
    }
    process.env("TMUX", tmux).env("ZZ_SOCKET", zz_socket);
    process.envs(tmux_shim_environment(
        tmux_shim,
        zz_executable,
        path.as_deref(),
    ));
}

#[cfg(not(windows))]
fn shell_process(command: &str) -> Command {
    let mut process = Command::new("/bin/sh");
    process.arg("-c").arg(command);
    process
}

#[cfg(windows)]
fn shell_process(command: &str) -> Command {
    use std::os::windows::process::CommandExt as _;
    let mut process = Command::new("cmd");
    process
        .args(["/D", "/S", "/C"])
        .raw_arg(format!("\"{command}\""));
    process
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tmux_shim_environment_exports_directory_and_shell_jobs_skip_stale_values() {
        let directory = Path::new("private-tmux");
        let executable = Path::new("zz");
        let environment = tmux_shim_environment(Some(directory), Some(executable), None);
        assert!(environment.contains(&(
            TMUX_SHIM_DIRECTORY_ENVIRONMENT_VARIABLE.into(),
            directory.as_os_str().to_owned(),
        )));
        assert!(environment.contains(&(
            TMUX_SHIM_EXECUTABLE_ENVIRONMENT_VARIABLE.into(),
            executable.as_os_str().to_owned(),
        )));

        let inherited = [
            (
                TMUX_SHIM_DIRECTORY_ENVIRONMENT_VARIABLE.into(),
                Some("stale-directory".into()),
            ),
            (
                TMUX_SHIM_EXECUTABLE_ENVIRONMENT_VARIABLE.into(),
                Some("stale-executable".into()),
            ),
        ];
        for startup in [false, true] {
            let mut process = Command::new("unused");
            configure_shell_job_environment(
                &mut process,
                &inherited,
                "tmux-256color",
                startup,
                "socket,1,0",
                OsStr::new("socket"),
                None,
                None,
            );
            for name in [
                TMUX_SHIM_DIRECTORY_ENVIRONMENT_VARIABLE,
                TMUX_SHIM_EXECUTABLE_ENVIRONMENT_VARIABLE,
            ] {
                assert!(process.get_envs().all(|(key, _)| key != name));
            }
        }
    }
}
