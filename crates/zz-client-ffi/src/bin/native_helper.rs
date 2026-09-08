use std::{
    ffi::OsString,
    io::{self, Write},
    path::PathBuf,
    process::ExitCode,
};
use zz_daemon::{CommandClient, Daemon, DaemonError};
use zz_protocol::{CommandInvocation, PreparedCommandResult, RawText, StdoutClaim};

fn main() -> ExitCode {
    if let Some(socket) = std::env::var_os(zz_daemon::ASKPASS_SOCKET_ENV) {
        return zz_daemon::run_helper(
            &PathBuf::from(socket),
            &std::env::args().nth(1).unwrap_or_default(),
        );
    }
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    if args
        .iter()
        .any(|arg| arg.to_string_lossy().starts_with("--type="))
    {
        #[cfg(target_os = "macos")]
        return ExitCode::from(
            u8::try_from(zz_browser::run_subprocess().clamp(0, 255)).unwrap_or(1),
        );
        #[cfg(not(target_os = "macos"))]
        return ExitCode::FAILURE;
    }
    match run(args) {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            eprintln!("zz: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(mut args: Vec<OsString>) -> Result<u8, Box<dyn std::error::Error>> {
    if args.first().is_some_and(|arg| arg == "daemon") {
        return run_daemon(&args[1..]);
    }
    if args.len() == 1 && args[0] == "-V" {
        println!("{}", zz_protocol::CommandSpec::TMUX_VERSION_OUTPUT);
        return Ok(0);
    }
    if args.len() == 1 && args[0] == "protocol-version" {
        println!("{}", zz_protocol::PROTOCOL_VERSION);
        return Ok(0);
    }
    let mut socket = std::env::var_os("ZZ_SOCKET")
        .map(PathBuf::from)
        .unwrap_or_else(zz_daemon::default_socket_path);
    if args
        .first()
        .is_some_and(|arg| arg == "-S" || arg == "--socket")
    {
        if args.len() < 2 {
            return Err("socket path is required".into());
        }
        socket = PathBuf::from(args.remove(1));
        args.remove(0);
    }
    let commands =
        zz_protocol::split_command_words(args.iter().map(|arg| RawText::from_os_str(arg)))
            .into_iter()
            .filter_map(|words| {
                let mut words = words.into_iter();
                Some(CommandInvocation::new(words.next()?, words))
            })
            .collect::<Vec<_>>();
    if commands.is_empty() {
        return Err("a command is required".into());
    }
    let mut client = CommandClient::connect(&socket)?;
    let commands = client.prepare_commands(commands)?;
    let mut code = 0;
    let mut stdout_owner = None;
    for command in commands {
        if let PreparedCommandResult::Error(error) = command.result {
            return Err(error.into());
        }
        let outcome = match client.execute_prepared_streams(command.invocation) {
            Ok(outcome) => outcome,
            Err(DaemonError::CommandFailed { output, error }) => {
                write_output(&output, StdoutClaim::Print, &mut stdout_owner)?;
                return Err(error.into());
            }
            Err(error) => return Err(error.into()),
        };
        if outcome.exit_code != 0 {
            code = outcome.exit_code;
        }
        if write_output(&outcome.stdout, outcome.stdout_claim, &mut stdout_owner).is_err() {
            code = 1;
        }
        if !outcome.stderr.is_empty() {
            let mut stderr = io::stderr().lock();
            stderr.write_all(outcome.stderr.as_bytes())?;
            if !outcome.stderr.as_bytes().ends_with(b"\n") {
                stderr.write_all(b"\n")?;
            }
        }
    }
    Ok(code)
}

fn write_output(output: &RawText, claim: StdoutClaim, owner: &mut Option<bool>) -> io::Result<()> {
    if output.is_empty() {
        return Ok(());
    }
    let raw = claim == StdoutClaim::Raw;
    if raw && owner.is_some() {
        return Err(io::Error::other("stdout is already in use"));
    }
    if *owner == Some(true) {
        return Ok(());
    }
    *owner = Some(raw);
    let mut stdout = io::stdout().lock();
    stdout.write_all(output.as_bytes())?;
    if !raw && !output.as_bytes().ends_with(b"\n") {
        stdout.write_all(b"\n")?;
    }
    stdout.flush()
}

fn run_daemon(args: &[OsString]) -> Result<u8, Box<dyn std::error::Error>> {
    let mut socket = None;
    let mut cwd = None;
    let mut mux = None;
    let mut no_config = false;
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        if arg == "--no-user-config" {
            no_config = true;
            continue;
        }
        let value = args.next().ok_or("daemon option requires a value")?;
        match arg.to_str() {
            Some("--socket") => socket = Some(PathBuf::from(value)),
            Some("--client-cwd") => cwd = Some(PathBuf::from(value)),
            Some("--mux-config") => mux = Some(PathBuf::from(value)),
            _ => return Err("unknown daemon option".into()),
        }
    }
    let mut daemon = Daemon::new(socket.ok_or("daemon socket is required")?);
    if let Some(cwd) = cwd {
        daemon = daemon.with_initial_client_working_directory(cwd);
    }
    if let Some(mux) = mux {
        daemon = daemon.with_zz_mux_config_path(mux);
    }
    if no_config {
        daemon = daemon.without_user_config();
    }
    match daemon.run_foreground() {
        Ok(()) | Err(DaemonError::AlreadyRunning(_)) => Ok(0),
        Err(error) => Err(error.into()),
    }
}
