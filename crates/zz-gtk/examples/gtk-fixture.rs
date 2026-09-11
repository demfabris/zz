use std::{io, path::PathBuf, process::ExitCode};

use zz_daemon::{CommandClient, Daemon};
use zz_protocol::CommandInvocation;

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let socket = std::env::var_os("ZZ_SOCKET")
        .map(PathBuf::from)
        .ok_or_else(|| io::Error::other("set ZZ_SOCKET to the fixture socket path"))?;
    let mut args = std::env::args().skip(1);
    let command = args.next().ok_or_else(|| {
        io::Error::other("usage: gtk-fixture --daemon | <command> [arguments...]")
    })?;
    if command == "--daemon" {
        Daemon::new(&socket)
            .without_user_config()
            .run_foreground()?;
    } else {
        let output =
            CommandClient::connect(&socket)?.execute(CommandInvocation::new(command, args))?;
        print!("{output}");
    }
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("gtk-fixture: {error}");
            ExitCode::FAILURE
        }
    }
}
