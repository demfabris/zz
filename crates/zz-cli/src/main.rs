use std::process::ExitCode;

use zz_cli::{CommandLineOrigin, Startup, StartupOptions};

#[cfg(not(windows))]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
#[used]
#[unsafe(link_section = "__DATA,__mod_init_func")]
static TAG_ALLOCATOR_MEMORY: extern "C" fn() = zz_cli::tag_allocator_memory;

#[cfg(target_os = "linux")]
#[allow(unsafe_code)]
#[used]
#[unsafe(link_section = ".init_array.00100")]
static DISABLE_TRANSPARENT_HUGE_PAGES: extern "C" fn() =
    zz_terminal::disable_transparent_huge_pages;

fn main() -> ExitCode {
    #[cfg(windows)]
    zz_cli::attach_parent_console();
    if let Some(exit) = zz_terminal::run_pty_exec_mode() {
        return exit;
    }
    if let Some(exit) = zz_cli::run_askpass_mode() {
        return exit;
    }
    let socket = zz_daemon_client::default_socket_path();
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
