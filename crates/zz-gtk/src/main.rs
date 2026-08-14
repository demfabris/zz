use std::{path::PathBuf, process::ExitCode};

use zz_daemon::{Endpoint, default_socket_path};
use zz_gtk::ui::{self, Launch};

const USAGE: &str = "usage: zz-gtk [--socket <path>] [session]";

fn main() -> ExitCode {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let launch = match parse(std::env::args().skip(1)) {
        Ok(launch) => launch,
        Err(error) => {
            eprintln!("zz-gtk: {error}\n{USAGE}");
            return ExitCode::FAILURE;
        }
    };
    if ui::run(launch) == gtk::glib::ExitCode::SUCCESS {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn parse(arguments: impl Iterator<Item = String>) -> Result<Launch, String> {
    let mut socket: Option<PathBuf> = None;
    let mut session = String::new();
    let mut arguments = arguments.peekable();
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--socket" => {
                socket = Some(PathBuf::from(
                    arguments
                        .next()
                        .ok_or_else(|| "--socket needs a path".to_owned())?,
                ));
            }
            "--help" | "-h" => {
                println!("{USAGE}");
                std::process::exit(0);
            }
            other if other.starts_with('-') => {
                return Err(format!("unknown option {other}"));
            }
            other => other.clone_into(&mut session),
        }
    }
    Ok(Launch {
        endpoint: Endpoint::Local(socket.unwrap_or_else(default_socket_path)),
        session,
    })
}
