use std::{path::PathBuf, thread, time::Duration};
use zz_daemon::{CommandClient, Daemon};
use zz_protocol::CommandInvocation;

#[path = "native_fixture/agent.rs"]
mod agent;

fn main() {
    if std::env::args().nth(1).as_deref() == Some("--acp") {
        agent::run();
        return;
    }
    let with_agent = std::env::args().any(|arg| arg == "--agent");
    let socket = PathBuf::from(std::env::args_os().nth(1).expect("socket path"));
    let command_socket = socket.clone();
    thread::spawn(move || {
        for _ in 0..300 {
            if let Ok(mut client) = CommandClient::connect(&command_socket) {
                client
                    .execute(CommandInvocation::new(
                        "new-session",
                        [
                            "-d",
                            "-s",
                            "native-fixture",
                            "printf '\\033[32mzz-native-ready\\033[0m\\r\\nUnicode: café 界 👩‍💻\\r\\n'; exec /bin/cat",
                        ],
                    ))
                    .expect("create fixture session");
                if with_agent {
                    let executable = std::env::current_exe().unwrap();
                    let adapter = format!(
                        "'{}' --acp",
                        executable.display().to_string().replace('\'', "'\\''")
                    );
                    for command in [
                        CommandInvocation::new(
                            "set-option",
                            ["-g", "experimental-agent-pane", "on"],
                        ),
                        CommandInvocation::new("set-option", ["-g", "agent-auto-approve", "off"]),
                        CommandInvocation::new(
                            "set-option",
                            ["-g", "agent-command", adapter.as_str()],
                        ),
                        CommandInvocation::new("split-picker", ["-t", "native-fixture:", "-h"]),
                        CommandInvocation::new(
                            "select-pane-kind",
                            ["-t", "native-fixture:", "-c", "/tmp", "agent"],
                        ),
                    ] {
                        client.execute(command).expect("configure agent fixture");
                    }
                }
                std::fs::write(command_socket.with_extension("ready"), b"ready")
                    .expect("publish fixture readiness");
                return;
            }
            thread::sleep(Duration::from_millis(10));
        }
        panic!("fixture daemon did not start");
    });
    Daemon::new(&socket)
        .without_user_config()
        .run_foreground()
        .expect("run fixture daemon");
}
