use std::{
    process::{Command, Stdio},
    thread,
};

use agent_client_protocol::{
    AcpAgent,
    schema::v1::{EnvVariable as WorkspaceEnvVariable, McpServer as WorkspaceMcpServer},
};

use crate::config::AgentConfig;

pub(crate) fn with_platform_environment(agent: AcpAgent) -> AcpAgent {
    #[cfg(target_os = "macos")]
    {
        with_macos_executable_path(agent, macos_executable_path())
    }

    #[cfg(not(target_os = "macos"))]
    {
        agent
    }
}

/// Pre-populate the npx cache for the pinned adapter packages, off the main
/// thread, so the first agent pane spawn doesn't pay the download.
pub fn warm_agent_adapter_cache(config: &AgentConfig) {
    let mut specs: Vec<String> = [config.command.as_str(), config.claude_code_command.as_str()]
        .into_iter()
        .filter_map(npx_package_spec)
        .map(str::to_owned)
        .collect();
    specs.dedup();
    if specs.is_empty() {
        return;
    }
    thread::spawn(move || {
        #[cfg(target_os = "macos")]
        let path = macos_executable_path();
        for spec in specs {
            let mut warm = Command::new("npx");
            warm.args(["--yes", "--package", &spec, "npm", "--version"])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            #[cfg(target_os = "macos")]
            if let Some(path) = path {
                warm.env("PATH", path);
            }
            match warm.status() {
                Ok(status) if status.success() => {
                    log::debug!(target: "zz::agent", "warmed adapter cache for {spec}");
                }
                Ok(status) => {
                    log::debug!(target: "zz::agent", "adapter cache warm exited {status}: {spec}");
                }
                Err(error) => {
                    log::debug!(target: "zz::agent", "adapter cache warm failed to spawn: {error}");
                }
            }
        }
    });
}

fn npx_package_spec(command: &str) -> Option<&str> {
    let mut tokens = command
        .split_whitespace()
        .skip_while(|token| is_environment_assignment(token));
    if tokens.next() != Some("npx") {
        return None;
    }
    tokens.find(|token| !token.starts_with('-'))
}

fn is_environment_assignment(token: &str) -> bool {
    token.split_once('=').is_some_and(|(name, _)| {
        !name.is_empty()
            && name
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || character == '_')
    })
}

/// Which zz pane, session, and daemon an ACP child belongs to.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct AgentWorkspaceEnvironment {
    pub(crate) pane: Option<String>,
    pub(crate) session: Option<String>,
    pub(crate) socket: Option<String>,
}

impl AgentWorkspaceEnvironment {
    fn entries(&self) -> impl Iterator<Item = (&'static str, &str)> {
        [
            ("ZZ_PANE", self.pane.as_deref()),
            ("ZZ_SESSION", self.session.as_deref()),
            ("ZZ_SOCKET", self.socket.as_deref()),
        ]
        .into_iter()
        .filter_map(|(name, value)| value.map(|value| (name, value)))
    }
}

/// Add the workspace identity to an ACP child's environment, additively: a
/// value the user configured through `agent-command` wins.
pub(crate) fn with_workspace_environment(
    agent: AcpAgent,
    workspace: &AgentWorkspaceEnvironment,
) -> AcpAgent {
    let mut server = agent.into_server();
    if let WorkspaceMcpServer::Stdio(stdio) = &mut server {
        for (name, value) in workspace.entries() {
            if stdio.env.iter().any(|variable| variable.name == name) {
                continue;
            }
            stdio.env.push(WorkspaceEnvVariable::new(name, value));
        }
    }
    AcpAgent::new(server)
}

#[cfg(target_os = "macos")]
use std::{
    collections::HashSet,
    env,
    ffi::OsStr,
    io::{Read as _, Seek as _, SeekFrom},
    path::{Path, PathBuf},
    sync::OnceLock,
    time::{Duration, Instant},
};

#[cfg(target_os = "macos")]
use agent_client_protocol::schema::v1::{EnvVariable, McpServer};

#[cfg(target_os = "macos")]
const LOGIN_SHELL_TIMEOUT: Duration = Duration::from_secs(3);
#[cfg(target_os = "macos")]
const PATH_CAPTURE_COMMAND: &str = r#"printf '\036%s\037' "$PATH""#;
#[cfg(target_os = "macos")]
const PATH_MARKER_START: u8 = 0x1e;
#[cfg(target_os = "macos")]
const PATH_MARKER_END: u8 = 0x1f;

#[cfg(target_os = "macos")]
fn with_macos_executable_path(agent: AcpAgent, path: Option<&str>) -> AcpAgent {
    let Some(path) = path else {
        return agent;
    };

    let mut server = agent.into_server();
    let mut injected = false;
    if let McpServer::Stdio(stdio) = &mut server
        && !stdio.env.iter().any(|variable| variable.name == "PATH")
    {
        stdio.env.push(EnvVariable::new("PATH", path));
        injected = true;
    }
    if injected {
        log::debug!(target: "zz::agent", "using login-shell PATH for ACP process");
    }
    AcpAgent::new(server)
}

#[cfg(target_os = "macos")]
fn macos_executable_path() -> Option<&'static str> {
    static PATH: OnceLock<Option<String>> = OnceLock::new();

    PATH.get_or_init(resolve_macos_executable_path).as_deref()
}

#[cfg(target_os = "macos")]
fn resolve_macos_executable_path() -> Option<String> {
    let preferred = capture_login_shell_path().or_else(capture_system_path)?;
    let inherited = env::var_os("PATH");
    merge_executable_paths(&preferred, inherited.as_deref()).or(Some(preferred))
}

#[cfg(target_os = "macos")]
fn capture_login_shell_path() -> Option<String> {
    let shell = env::var_os("SHELL")
        .filter(|shell| {
            let shell = Path::new(shell);
            shell.is_absolute() && shell.is_file()
        })
        .unwrap_or_else(|| "/bin/zsh".into());
    let mut capture = tempfile::tempfile().ok()?;
    let child_stdout = capture.try_clone().ok()?;
    let mut command = Command::new(shell);
    command
        .args(["-l", "-i", "-c", PATH_CAPTURE_COMMAND])
        .stdin(Stdio::null())
        .stdout(Stdio::from(child_stdout))
        .stderr(Stdio::null());
    if let Some(home) = env::var_os("HOME").filter(|home| Path::new(home).is_dir()) {
        command.current_dir(home);
    }

    let mut child = command.spawn().ok()?;
    let deadline = Instant::now() + LOGIN_SHELL_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
            Ok(None) | Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }

    capture.seek(SeekFrom::Start(0)).ok()?;
    let mut output = Vec::new();
    capture.read_to_end(&mut output).ok()?;
    parse_captured_path(&output)
}

#[cfg(target_os = "macos")]
fn capture_system_path() -> Option<String> {
    let output = Command::new("/bin/sh")
        .args([
            "-c",
            r#"PATH=; eval "$(/usr/libexec/path_helper -s)"; printf '\036%s\037' "$PATH""#,
        ])
        .env_clear()
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| parse_captured_path(&output.stdout))
        .flatten()
}

#[cfg(target_os = "macos")]
fn parse_captured_path(output: &[u8]) -> Option<String> {
    let start = output.iter().rposition(|byte| *byte == PATH_MARKER_START)?;
    let value = output.get(start + 1..)?;
    let end = value.iter().position(|byte| *byte == PATH_MARKER_END)?;
    String::from_utf8(value.get(..end)?.to_vec())
        .ok()
        .filter(|path| !path.is_empty())
}

#[cfg(target_os = "macos")]
fn merge_executable_paths(preferred: &str, inherited: Option<&OsStr>) -> Option<String> {
    let mut paths = Vec::<PathBuf>::new();
    let mut seen = HashSet::<PathBuf>::new();
    let mut push = |path: PathBuf| {
        if seen.insert(path.clone()) {
            paths.push(path);
        }
    };

    for path in env::split_paths(OsStr::new(preferred)) {
        push(path);
    }
    if let Some(inherited) = inherited {
        for path in env::split_paths(inherited) {
            push(path);
        }
    }

    env::join_paths(paths).ok()?.into_string().ok()
}

#[cfg(test)]
mod warm_tests {
    use super::npx_package_spec;

    #[test]
    fn extracts_the_spec_from_a_default_command() {
        assert_eq!(
            npx_package_spec("npx -y @agentclientprotocol/codex-acp@1.1.7"),
            Some("@agentclientprotocol/codex-acp@1.1.7")
        );
    }

    #[test]
    fn skips_environment_assignments_and_flags() {
        assert_eq!(
            npx_package_spec("ZZ_SOCKET=/custom/zz.sock npx --yes @scope/pkg@2.0.0"),
            Some("@scope/pkg@2.0.0")
        );
    }

    #[test]
    fn a_custom_binary_is_not_warmed() {
        assert_eq!(npx_package_spec("my-agent --stdio"), None);
    }

    #[test]
    fn a_raw_stdio_json_configuration_is_not_warmed() {
        assert_eq!(
            npx_package_spec(r#"{"command":"my-agent","args":[]}"#),
            None
        );
    }

    #[test]
    fn an_npx_command_without_a_package_is_not_warmed() {
        assert_eq!(npx_package_spec("npx --yes"), None);
    }
}

#[cfg(test)]
mod workspace_tests {
    use std::str::FromStr as _;

    use agent_client_protocol::{AcpAgent, schema::v1::McpServer};

    use super::{AgentWorkspaceEnvironment, with_workspace_environment};

    fn workspace() -> AgentWorkspaceEnvironment {
        AgentWorkspaceEnvironment {
            pane: Some("%4".to_owned()),
            session: Some("work".to_owned()),
            socket: Some("/tmp/zz/default.sock".to_owned()),
        }
    }

    fn environment(agent: &AcpAgent) -> Vec<(String, String)> {
        let McpServer::Stdio(stdio) = agent.server() else {
            panic!("expected stdio agent");
        };
        stdio
            .env
            .iter()
            .map(|variable| (variable.name.clone(), variable.value.clone()))
            .collect()
    }

    #[test]
    fn workspace_identity_is_injected_for_the_pane() {
        let agent = AcpAgent::from_str("npx -y example").expect("valid command");
        let agent = with_workspace_environment(agent, &workspace());
        let environment = environment(&agent);
        assert!(environment.contains(&("ZZ_PANE".to_owned(), "%4".to_owned())));
        assert!(environment.contains(&("ZZ_SESSION".to_owned(), "work".to_owned())));
        assert!(environment.contains(&("ZZ_SOCKET".to_owned(), "/tmp/zz/default.sock".to_owned())));
    }

    #[test]
    fn a_user_configured_value_is_never_clobbered() {
        let agent = AcpAgent::from_str("ZZ_SOCKET=/custom/zz.sock npx -y example")
            .expect("valid configured command");
        let agent = with_workspace_environment(agent, &workspace());
        assert_eq!(
            environment(&agent)
                .into_iter()
                .filter(|(name, _)| name == "ZZ_SOCKET")
                .map(|(_, value)| value)
                .collect::<Vec<_>>(),
            ["/custom/zz.sock"]
        );
    }

    #[test]
    fn missing_identity_pieces_are_simply_left_out() {
        let agent = AcpAgent::from_str("npx -y example").expect("valid command");
        let agent = with_workspace_environment(
            agent,
            &AgentWorkspaceEnvironment {
                pane: Some("%1".to_owned()),
                ..AgentWorkspaceEnvironment::default()
            },
        );
        let environment = environment(&agent);
        assert_eq!(environment.len(), 1);
        assert_eq!(environment[0], ("ZZ_PANE".to_owned(), "%1".to_owned()));
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use std::str::FromStr as _;

    use super::*;

    #[test]
    fn captured_path_ignores_noisy_shell_output() {
        assert_eq!(
            parse_captured_path(b"startup noise\x1e/first\x1fmore\x1e/custom:/usr/bin\x1f"),
            Some("/custom:/usr/bin".to_owned())
        );
    }

    #[test]
    fn login_path_keeps_precedence_and_appends_inherited_entries() {
        assert_eq!(
            merge_executable_paths(
                "/custom:/usr/bin",
                Some(OsStr::new("/usr/bin:/opt/homebrew/bin")),
            ),
            Some("/custom:/usr/bin:/opt/homebrew/bin".to_owned())
        );
    }

    #[test]
    fn agent_path_is_injected_without_replacing_an_explicit_path() {
        let agent = AcpAgent::from_str("npx -y example").expect("valid command");
        let agent = with_macos_executable_path(agent, Some("/login/bin:/usr/bin"));
        let McpServer::Stdio(stdio) = agent.server() else {
            panic!("expected stdio agent");
        };
        assert!(
            stdio
                .env
                .iter()
                .any(|variable| variable.name == "PATH" && variable.value == "/login/bin:/usr/bin")
        );

        let agent = AcpAgent::from_str("PATH=/configured/bin npx -y example")
            .expect("valid configured command");
        let agent = with_macos_executable_path(agent, Some("/login/bin:/usr/bin"));
        let McpServer::Stdio(stdio) = agent.server() else {
            panic!("expected stdio agent");
        };
        assert_eq!(
            stdio
                .env
                .iter()
                .filter(|variable| variable.name == "PATH")
                .map(|variable| variable.value.as_str())
                .collect::<Vec<_>>(),
            ["/configured/bin"]
        );
    }
}
