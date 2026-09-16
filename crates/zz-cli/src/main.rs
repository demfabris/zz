use std::process::ExitCode;

use zz_cli::{CommandLineOrigin, Startup, StartupOptions};

#[cfg(not(windows))]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

fn main() -> ExitCode {
    #[cfg(windows)]
    zz_cli::attach_parent_console();
    if let Some(exit) = zz_cli::run_askpass_mode() {
        return exit;
    }
    let socket = zz_daemon::default_socket_path();
    match zz_cli::run_startup(
        &socket,
        StartupOptions {
            origin: CommandLineOrigin::Launcher,
            browser_provider: None,
        },
    ) {
        Startup::Exit(code) => code,
        Startup::Application(socket_path) => zz_cli::launch_application(&socket_path),
    }
}
