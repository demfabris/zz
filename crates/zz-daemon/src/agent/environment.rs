//! The environment an ACP child is spawned with.
//!
//! A daemon started from a launch agent, a login session, or `zz` itself never
//! runs the user's shell init, so its PATH misses everything the shell shapes:
//! nvm's shell function, fnm's per-shell multishells, mise shims, custom npm
//! prefixes. The agent CLIs live exactly there, so the child gets a repaired
//! PATH: the login shell's own PATH, then ours, then the Node version-manager
//! bin directories that exist on disk.

use std::{
    process::{Command, Stdio},
    thread,
};

use agent_client_protocol::{
    AcpAgent,
    schema::v1::{EnvVariable, McpServer},
};

pub(crate) fn with_platform_environment(agent: AcpAgent) -> AcpAgent {
    with_executable_path(agent, executable_path())
}

/// Pre-populate the npx cache for the configured adapter packages, off the
/// calling thread, so the first agent pane spawn doesn't pay the download —
/// and take the login-shell PATH snapshot there too, off the spawn path.
pub(crate) fn warm_adapter_cache(commands: &[String]) {
    let mut specs: Vec<String> = commands
        .iter()
        .filter_map(|command| npx_package_spec(command))
        .map(str::to_owned)
        .collect();
    specs.dedup();
    thread::spawn(move || {
        let path = executable_path();
        for spec in specs {
            let mut warm = Command::new("npx");
            warm.args(["--yes", "--package", &spec, "npm", "--version"])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null());
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
/// value the user configured through the adapter command wins.
pub(crate) fn with_workspace_environment(
    agent: AcpAgent,
    workspace: &AgentWorkspaceEnvironment,
) -> AcpAgent {
    let mut server = agent.into_server();
    if let McpServer::Stdio(stdio) = &mut server {
        for (name, value) in workspace.entries() {
            if stdio.env.iter().any(|variable| variable.name == name) {
                continue;
            }
            stdio.env.push(EnvVariable::new(name, value));
        }
    }
    AcpAgent::new(server)
}

fn with_executable_path(agent: AcpAgent, path: Option<&str>) -> AcpAgent {
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
        log::debug!(target: "zz::agent", "using the repaired PATH for the ACP process");
    }
    AcpAgent::new(server)
}

#[cfg(unix)]
use login_shell::executable_path;

/// Windows daemons inherit the user's PATH, so there is nothing to repair.
#[cfg(not(unix))]
fn executable_path() -> Option<&'static str> {
    None
}

#[cfg(unix)]
mod login_shell {
    use std::{
        collections::HashSet,
        env,
        ffi::OsStr,
        fs::{self, File},
        io::{Read as _, Seek as _, SeekFrom},
        path::{Path, PathBuf},
        process::{Command, Stdio},
        sync::OnceLock,
        thread,
        time::{Duration, Instant},
    };

    /// A shell whose init hangs must not stall an agent pane spawn: every
    /// attempt is killed at this deadline.
    const LOGIN_SHELL_TIMEOUT: Duration = Duration::from_secs(3);
    const POLL_INTERVAL: Duration = Duration::from_millis(10);
    /// Tried in order: rc files that hang or `exec` a multiplexer when
    /// interactive still answer a non-interactive login shell.
    const LOGIN_SHELL_FLAGS: [&[&str]; 2] = [&["-l", "-i", "-c"], &["-l", "-c"]];
    const DEFAULT_SHELLS: [&str; 3] = ["/bin/zsh", "/bin/bash", "/bin/sh"];
    const PATH_CAPTURE_COMMAND: &str = r#"printf '\036%s\037' "$PATH""#;
    /// fish keeps `$PATH` as a list, so the POSIX quoting joins it with spaces.
    const FISH_PATH_CAPTURE_COMMAND: &str = r"printf '\036%s\037' (string join : $PATH)";
    const PATH_MARKER_START: u8 = 0x1e;
    const PATH_MARKER_END: u8 = 0x1f;

    /// Resolved once per process, negative results included, so a broken shell
    /// is probed once rather than on every pane spawn.
    pub(super) fn executable_path() -> Option<&'static str> {
        static PATH: OnceLock<Option<String>> = OnceLock::new();

        PATH.get_or_init(resolve_executable_path).as_deref()
    }

    fn resolve_executable_path() -> Option<String> {
        let login = login_shell_enabled(env::var_os("ZZ_AGENT_LOGIN_SHELL").as_deref())
            .then(capture_login_shell_path)
            .flatten()
            .or_else(system_path);
        let managers = home()
            .map(|home| {
                node_version_manager_bins(
                    &home,
                    env::var_os("FNM_DIR").map(PathBuf::from).as_deref(),
                )
            })
            .unwrap_or_default();
        if login.is_none() && managers.is_empty() {
            return None;
        }
        compose_executable_path(
            login.as_deref().map(OsStr::new),
            env::var_os("PATH").as_deref(),
            &managers,
        )
    }

    fn login_shell_enabled(value: Option<&OsStr>) -> bool {
        value.is_none_or(|value| value != "0")
    }

    fn home() -> Option<PathBuf> {
        env::var_os("HOME")
            .map(PathBuf::from)
            .filter(|home| home.is_dir())
    }

    fn capture_login_shell_path() -> Option<String> {
        let shell = user_shell()?;
        LOGIN_SHELL_FLAGS
            .iter()
            .find_map(|flags| capture_path_from_shell(&shell, flags))
    }

    fn user_shell() -> Option<PathBuf> {
        env::var_os("SHELL")
            .map(PathBuf::from)
            .filter(|shell| shell.is_absolute() && shell.is_file())
            .or_else(|| {
                DEFAULT_SHELLS
                    .iter()
                    .map(PathBuf::from)
                    .find(|shell| shell.is_file())
            })
    }

    fn capture_path_from_shell(shell: &Path, flags: &[&str]) -> Option<String> {
        let mut capture = tempfile::tempfile().ok()?;
        let child_stdout = capture.try_clone().ok()?;
        let mut command = Command::new(shell);
        command
            .args(flags)
            .arg(path_capture_command(shell))
            .stdin(Stdio::null())
            .stdout(Stdio::from(child_stdout))
            .stderr(Stdio::null())
            .env("ZZ_RESOLVING_ENVIRONMENT", "1");
        if let Some(home) = home() {
            command.current_dir(home);
        }

        let mut child = command.spawn().ok()?;
        let deadline = Instant::now() + LOGIN_SHELL_TIMEOUT;
        loop {
            // Init that blocks after printing must not cost the whole timeout.
            if let Some(path) = read_captured_path(&mut capture) {
                let _ = child.kill();
                let _ = child.wait();
                return Some(path);
            }
            match child.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) if Instant::now() < deadline => thread::sleep(POLL_INTERVAL),
                Ok(None) | Err(_) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return None;
                }
            }
        }

        read_captured_path(&mut capture)
    }

    fn path_capture_command(shell: &Path) -> &'static str {
        if shell.file_name() == Some(OsStr::new("fish")) {
            FISH_PATH_CAPTURE_COMMAND
        } else {
            PATH_CAPTURE_COMMAND
        }
    }

    fn read_captured_path(capture: &mut File) -> Option<String> {
        capture.seek(SeekFrom::Start(0)).ok()?;
        let mut output = Vec::new();
        capture.read_to_end(&mut output).ok()?;
        parse_captured_path(&output)
    }

    fn system_path() -> Option<String> {
        #[cfg(target_os = "macos")]
        {
            capture_system_path()
        }

        #[cfg(not(target_os = "macos"))]
        {
            None
        }
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

    fn parse_captured_path(output: &[u8]) -> Option<String> {
        let start = output.iter().rposition(|byte| *byte == PATH_MARKER_START)?;
        let value = output.get(start + 1..)?;
        let end = value.iter().position(|byte| *byte == PATH_MARKER_END)?;
        String::from_utf8(value.get(..end)?.to_vec())
            .ok()
            .filter(|path| !path.is_empty())
    }

    /// Bin directories where npm-installed CLIs land under Node version
    /// managers. A GUI launch never sees them: fnm's multishell entries are
    /// per-shell and nvm is a shell function, so both live in shell init only.
    fn node_version_manager_bins(home: &Path, fnm_dir: Option<&Path>) -> Vec<PathBuf> {
        // fnm's active installation is reachable through the stable
        // `aliases/default` symlink; its PATH entries are ephemeral.
        let mut dirs: Vec<PathBuf> = fnm_dir
            .map(Path::to_path_buf)
            .into_iter()
            .chain([
                home.join(".local/share/fnm"),
                home.join("Library/Application Support/fnm"),
                home.join(".fnm"),
            ])
            .map(|root| root.join("aliases/default/bin"))
            .collect();
        dirs.extend([
            home.join(".volta/bin"),
            home.join(".bun/bin"),
            home.join("Library/pnpm"),
            home.join(".local/share/pnpm"),
            home.join(".local/share/mise/shims"),
        ]);

        let mut versions: Vec<PathBuf> = fs::read_dir(home.join(".nvm/versions/node"))
            .into_iter()
            .flatten()
            .flatten()
            .map(|entry| entry.path())
            .collect();
        versions.sort();
        versions.reverse();
        dirs.extend(versions.into_iter().map(|version| version.join("bin")));

        dirs.retain(|dir| dir.is_dir());
        dirs
    }

    /// The login shell's PATH keeps precedence, our own PATH follows, and the
    /// version-manager bins land last: they are a fallback for what the shell
    /// never told us about, never a shadow over what it did.
    fn compose_executable_path(
        login: Option<&OsStr>,
        inherited: Option<&OsStr>,
        managers: &[PathBuf],
    ) -> Option<String> {
        let mut paths = Vec::<PathBuf>::new();
        let mut seen = HashSet::<PathBuf>::new();
        let mut push = |path: PathBuf| {
            if !path.as_os_str().is_empty() && seen.insert(path.clone()) {
                paths.push(path);
            }
        };

        for source in [login, inherited].into_iter().flatten() {
            for path in env::split_paths(source) {
                push(path);
            }
        }
        for manager in managers {
            push(manager.clone());
        }

        env::join_paths(paths)
            .ok()?
            .into_string()
            .ok()
            .filter(|path| !path.is_empty())
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn bins(home: &Path, relative: &[&str]) -> Vec<PathBuf> {
            relative
                .iter()
                .map(|path| {
                    let path = home.join(path);
                    fs::create_dir_all(&path).expect("test directory");
                    path
                })
                .collect()
        }

        #[test]
        fn captured_path_ignores_noisy_shell_output() {
            assert_eq!(
                parse_captured_path(b"startup noise\x1e/first\x1fmore\x1e/custom:/usr/bin\x1f"),
                Some("/custom:/usr/bin".to_owned())
            );
        }

        #[test]
        fn a_partial_capture_is_not_parsed() {
            assert_eq!(parse_captured_path(b"noise\x1e/custom:/usr"), None);
        }

        #[test]
        fn login_path_keeps_precedence_and_appends_inherited_entries() {
            assert_eq!(
                compose_executable_path(
                    Some(OsStr::new("/custom:/usr/bin")),
                    Some(OsStr::new("/usr/bin:/opt/homebrew/bin")),
                    &[],
                ),
                Some("/custom:/usr/bin:/opt/homebrew/bin".to_owned())
            );
        }

        #[test]
        fn version_manager_bins_land_after_both_paths_and_empty_entries_are_dropped() {
            assert_eq!(
                compose_executable_path(
                    Some(OsStr::new("/custom::/usr/bin")),
                    Some(OsStr::new("/usr/bin")),
                    &[PathBuf::from("/home/u/.bun/bin"), PathBuf::from("/custom")],
                ),
                Some("/custom:/usr/bin:/home/u/.bun/bin".to_owned())
            );
        }

        #[test]
        fn a_failed_login_shell_still_yields_the_version_manager_bins() {
            assert_eq!(
                compose_executable_path(
                    None,
                    Some(OsStr::new("/usr/bin")),
                    &[PathBuf::from("/home/u/.volta/bin")],
                ),
                Some("/usr/bin:/home/u/.volta/bin".to_owned())
            );
        }

        #[test]
        fn nothing_to_compose_is_no_path_at_all() {
            assert_eq!(compose_executable_path(None, None, &[]), None);
        }

        #[test]
        fn version_manager_bins_are_existence_checked_and_newest_first() {
            let home = tempfile::tempdir().expect("temp home");
            let home = home.path();
            let expected = bins(
                home,
                &[
                    ".local/share/fnm/aliases/default/bin",
                    ".bun/bin",
                    ".local/share/mise/shims",
                    ".nvm/versions/node/v24.2.0/bin",
                    ".nvm/versions/node/v20.11.1/bin",
                ],
            );
            fs::create_dir_all(home.join(".nvm/versions/node/v18.0.0")).expect("empty version");

            assert_eq!(node_version_manager_bins(home, None), expected);
        }

        #[test]
        fn an_fnm_dir_override_is_probed_before_the_well_known_roots() {
            let home = tempfile::tempdir().expect("temp home");
            let home = home.path();
            let expected = bins(
                home,
                &[
                    "elsewhere/fnm/aliases/default/bin",
                    ".fnm/aliases/default/bin",
                ],
            );

            assert_eq!(
                node_version_manager_bins(home, Some(&home.join("elsewhere/fnm"))),
                expected
            );
        }

        #[test]
        fn the_capture_command_follows_the_shell_family() {
            assert_eq!(
                path_capture_command(Path::new("/bin/zsh")),
                PATH_CAPTURE_COMMAND
            );
            assert_eq!(
                path_capture_command(Path::new("/usr/local/bin/fish")),
                FISH_PATH_CAPTURE_COMMAND
            );
        }

        #[test]
        fn the_login_shell_probe_is_opt_out() {
            assert!(login_shell_enabled(None));
            assert!(login_shell_enabled(Some(OsStr::new("1"))));
            assert!(!login_shell_enabled(Some(OsStr::new("0"))));
        }
    }
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

    use super::{AgentWorkspaceEnvironment, with_executable_path, with_workspace_environment};

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

    #[test]
    fn the_repaired_path_never_replaces_an_explicit_one() {
        let agent = AcpAgent::from_str("npx -y example").expect("valid command");
        let agent = with_executable_path(agent, Some("/login/bin:/usr/bin"));
        assert!(
            environment(&agent).contains(&("PATH".to_owned(), "/login/bin:/usr/bin".to_owned()))
        );

        let agent = AcpAgent::from_str("PATH=/configured/bin npx -y example")
            .expect("valid configured command");
        let agent = with_executable_path(agent, Some("/login/bin:/usr/bin"));
        assert_eq!(
            environment(&agent)
                .into_iter()
                .filter(|(name, _)| name == "PATH")
                .map(|(_, value)| value)
                .collect::<Vec<_>>(),
            ["/configured/bin"]
        );
    }

    #[test]
    fn an_unresolved_path_leaves_the_agent_alone() {
        let agent = AcpAgent::from_str("npx -y example").expect("valid command");
        let agent = with_executable_path(agent, None);
        assert!(environment(&agent).is_empty());
    }
}
