use std::{path::PathBuf, process::ExitCode};

use zz_daemon::{Endpoint, default_socket_path};
use zz_gtk::ui::{self, Launch};

const USAGE: &str = "\
usage: zz-gtk [options] [session]

    session          the session to attach to; the daemon's default when omitted
    --socket <path>  the daemon socket to dial (default: $ZZ_SOCKET, or the
                     platform's runtime path)
    -h, --help       print this message

The daemon owns the sessions; zz-gtk attaches to one and renders it.";

fn main() -> ExitCode {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    match parse(std::env::args().skip(1)) {
        Ok(Some(launch)) => {
            log::info!("zz-gtk is dialing {}", launch.endpoint);
            if ui::run(launch) == gtk::glib::ExitCode::SUCCESS {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        Ok(None) => {
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("zz-gtk: {error}\n\n{USAGE}");
            ExitCode::FAILURE
        }
    }
}

/// `Ok(None)` means help was asked for and nothing should launch.
fn parse(arguments: impl Iterator<Item = String>) -> Result<Option<Launch>, String> {
    let mut socket: Option<PathBuf> = None;
    let mut session: Option<String> = None;
    let mut arguments = arguments;
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--socket" => {
                let path = arguments
                    .next()
                    .filter(|path| !path.is_empty())
                    .ok_or_else(|| "--socket needs a path".to_owned())?;
                socket = Some(PathBuf::from(path));
            }
            "--help" | "-h" => return Ok(None),
            other if other.starts_with('-') => return Err(format!("unknown option {other}")),
            other if session.is_some() => {
                return Err(format!(
                    "unexpected argument {other}: a client attaches to one session"
                ));
            }
            other => session = Some(other.to_owned()),
        }
    }
    Ok(Some(Launch {
        endpoint: Endpoint::Local(socket.unwrap_or_else(default_socket_path)),
        session: session.unwrap_or_default(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_arguments(arguments: &[&str]) -> Result<Option<Launch>, String> {
        parse(arguments.iter().map(|argument| (*argument).to_owned()))
    }

    fn launch(arguments: &[&str]) -> Launch {
        parse_arguments(arguments)
            .expect("a valid invocation")
            .expect("a launch rather than help")
    }

    #[test]
    fn a_bare_invocation_dials_the_default_socket_and_session() {
        let launch = launch(&[]);

        assert_eq!(launch.session, "");
        assert_eq!(launch.endpoint, Endpoint::Local(default_socket_path()));
    }

    #[test]
    fn the_socket_and_the_session_are_both_honoured_in_either_order() {
        let expected = Endpoint::Local(PathBuf::from("/tmp/zz-cli.sock"));

        for arguments in [
            ["--socket", "/tmp/zz-cli.sock", "work"],
            ["work", "--socket", "/tmp/zz-cli.sock"],
        ] {
            let launch = launch(&arguments);
            assert_eq!(launch.session, "work");
            assert_eq!(launch.endpoint, expected);
        }
    }

    #[test]
    fn help_launches_nothing() {
        for arguments in [["-h"], ["--help"]] {
            assert!(
                parse_arguments(&arguments)
                    .expect("help is not an error")
                    .is_none()
            );
        }
    }

    #[test]
    fn a_broken_invocation_says_what_is_wrong() {
        let cases: &[(&[&str], &str)] = &[
            (&["--socket"], "--socket needs a path"),
            (&["--socket", ""], "--socket needs a path"),
            (&["--sockets", "/tmp/zz.sock"], "unknown option --sockets"),
            (&["-x"], "unknown option -x"),
            (&["work", "play"], "unexpected argument play"),
        ];

        for (arguments, reason) in cases {
            let Err(error) = parse_arguments(arguments) else {
                panic!("{arguments:?} was accepted");
            };
            assert!(
                error.contains(reason),
                "{arguments:?} explained itself as {error:?}"
            );
        }
    }
}
