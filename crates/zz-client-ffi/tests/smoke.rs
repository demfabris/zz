//! Compile the C smoke client against `include/zz-client.h`, link it with the
//! staticlib, and run it against a real in-process daemon. Passing proves the
//! header matches the exports and that a from-scratch C client can attach,
//! read viewport content, and type through the ABI alone.

#![cfg(unix)]

use std::{
    path::{Path, PathBuf},
    process::Command,
    thread,
    time::{Duration, Instant},
};

use zz_daemon::{CommandClient, Daemon};
use zz_protocol::CommandInvocation;

fn static_library() -> PathBuf {
    let executable = std::env::current_exe().expect("test executable path");
    let deps = executable.parent().expect("deps directory");
    let debug = deps.parent().expect("target profile directory");
    let uplifted = debug.join("libzz_client_ffi.a");
    let mut candidates: Vec<PathBuf> = std::fs::read_dir(deps)
        .expect("read deps directory")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("libzz_client_ffi") && name.ends_with(".a"))
        })
        .collect();
    candidates.sort();
    candidates
        .pop()
        .or_else(|| uplifted.exists().then_some(uplifted))
        .expect("libzz_client_ffi.a was built")
}

fn compile_smoke_client(scratch: &Path) -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let binary = scratch.join("zz-smoke-client");
    let mut compiler = Command::new("cc");
    compiler
        .arg(manifest.join("tests/smoke.c"))
        .arg("-I")
        .arg(manifest.join("include"))
        .arg("-o")
        .arg(&binary)
        .arg(static_library());
    #[cfg(target_os = "macos")]
    compiler.args([
        "-framework",
        "CoreFoundation",
        "-lobjc",
        "-framework",
        "IOKit",
        "-liconv",
        "-lSystem",
        "-lc",
        "-lm",
    ]);
    #[cfg(not(target_os = "macos"))]
    compiler.args(["-lpthread", "-ldl", "-lm"]);
    let output = compiler.output().expect("run the C compiler");
    assert!(
        output.status.success(),
        "compiling the smoke client failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    binary
}

fn connect_commands(socket: &Path) -> CommandClient {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        match CommandClient::connect(socket) {
            Ok(client) => return client,
            Err(error) => {
                assert!(Instant::now() < deadline, "daemon did not start: {error}");
                thread::sleep(Duration::from_millis(10));
            }
        }
    }
}

#[test]
fn a_c_client_attaches_reads_and_types_through_the_abi() {
    let scratch = std::env::temp_dir().join(format!("zz-smoke-{}", std::process::id()));
    std::fs::create_dir_all(&scratch).expect("create scratch directory");
    let binary = compile_smoke_client(&scratch);

    let socket = scratch.join("smoke.sock");
    let _ = std::fs::remove_file(&socket);
    let daemon = Daemon::new(&socket).without_user_config();
    thread::Builder::new()
        .name("zz-smoke-daemon".to_owned())
        .spawn(move || {
            let _ = daemon.run_foreground();
        })
        .expect("spawn smoke daemon");
    let mut commands = connect_commands(&socket);
    commands
        .execute(CommandInvocation::new(
            "new-session",
            [
                "-d",
                "-s",
                "smoke",
                "printf 'zz-smoke-ready\\r\\n'; exec /bin/cat",
            ],
        ))
        .expect("create the smoke session");

    let run = Command::new(&binary)
        .arg(&socket)
        .output()
        .expect("run the smoke client");
    let _ = commands.execute(CommandInvocation::new("kill-server", [] as [&str; 0]));
    let _ = std::fs::remove_dir_all(&scratch);
    assert!(
        run.status.success(),
        "the C smoke client failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
}
