#![cfg(all(unix, feature = "native-browser"))]

use std::{ffi::CString, os::unix::fs::PermissionsExt, path::PathBuf, process::Command};
use zz_client_ffi::{ZzConnectFailure, zz_client_connect_native, zz_client_free};
use zz_daemon::CommandClient;
use zz_protocol::CommandInvocation;

struct Fixture {
    root: PathBuf,
    socket: PathBuf,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if let Ok(mut client) = CommandClient::connect(&self.socket) {
            let _ = client.execute(CommandInvocation::new("kill-server", Vec::<String>::new()));
        }
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[test]
fn bootstrap_opt_in_helper_commands_and_custom_mux_reload() {
    let root = PathBuf::from(format!("/tmp/zz-native-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let fixture = Fixture {
        socket: root.join("daemon.sock"),
        root,
    };
    let helper = env!("CARGO_BIN_EXE_zz_native_helper");
    let wrapper = fixture.root.join("helper");
    std::fs::write(
        &wrapper,
        format!(
            "#!/bin/sh\nexport HOME='{}'\nexport XDG_CONFIG_HOME='{}'\nexec '{}' \"$@\"\n",
            fixture.root.display(),
            fixture.root.display(),
            helper
        ),
    )
    .unwrap();
    std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o700)).unwrap();
    let config = fixture.root.join("custom.conf");
    std::fs::write(
        &config,
        "set -g @native-reload first\nrun-shell 'tmux set-option -g @native-startup yes'\nnew-session -d -s native 'exec /bin/cat'\n",
    )
    .unwrap();
    let mut options = serde_json::json!({
        "endpoint": fixture.socket,
        "helper_path": wrapper,
        "start_if_missing": false,
        "restart_incompatible": false,
        "dark": false,
        "working_directory": fixture.root,
        "mux_config_path": config,
    });
    let mut failure = ZzConnectFailure::None;
    let mut error = [0; 1024];
    let mut dial = |options: &serde_json::Value| unsafe {
        let options = CString::new(options.to_string()).unwrap();
        zz_client_connect_native(
            options.as_ptr(),
            None,
            std::ptr::null_mut(),
            &mut failure,
            error.as_mut_ptr(),
            error.len(),
        )
    };
    assert!(dial(&options).is_null());
    assert!(!fixture.socket.exists());
    options["start_if_missing"] = true.into();
    let client = dial(&options);
    assert!(
        !client.is_null(),
        "{}",
        unsafe { std::ffi::CStr::from_ptr(error.as_ptr()) }.to_string_lossy()
    );
    unsafe {
        zz_client_free(client);
    }
    let run = |args: &[&str]| {
        Command::new(helper)
            .env("ZZ_SOCKET", &fixture.socket)
            .args(args)
            .output()
            .unwrap()
    };
    let value = run(&["show-options", "-gqv", "@native-reload"]);
    assert!(value.status.success(), "{:?}", value);
    assert_eq!(value.stdout, b"first\n");
    assert_eq!(
        run(&["show-options", "-gqv", "@native-startup"]).stdout,
        b"yes\n"
    );
    std::fs::write(&config, "set -g @native-reload second\n").unwrap();
    assert!(run(&["reload-config"]).status.success());
    assert_eq!(
        run(&["show-options", "-gqv", "@native-reload"]).stdout,
        b"second\n"
    );
    let chain = run(&[
        "display-message",
        "-p",
        "first",
        ";",
        "display-message",
        "-p",
        "second",
    ]);
    assert!(chain.status.success());
    assert_eq!(chain.stdout, b"first\nsecond\n");
    let missing = run(&["show-options", "-gv", "@missing-native-option"]);
    assert!(!missing.status.success());
    assert!(!missing.stderr.is_empty());
    assert!(
        run(&["run-shell", "tmux set-option -g @native-nested yes"])
            .status
            .success()
    );
    assert_eq!(
        run(&["show-options", "-gqv", "@native-nested"]).stdout,
        b"yes\n"
    );
}

#[test]
fn local_handshake_timeout_does_not_wait_for_a_silent_listener() {
    let path = PathBuf::from(format!("/tmp/zz-native-silent-{}.sock", std::process::id()));
    let listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
    let (release, wait) = std::sync::mpsc::channel();
    let server = std::thread::spawn(move || {
        let (_stream, _) = listener.accept().unwrap();
        wait.recv_timeout(std::time::Duration::from_secs(5)).ok();
    });
    let started = std::time::Instant::now();
    let result = zz_daemon::InteractiveClient::connect_terminal_surface_with_timeout(
        &path,
        zz_terminal::TerminalColorScheme::Dark,
        std::time::Duration::from_millis(100),
    );
    release.send(()).unwrap();
    server.join().unwrap();
    std::fs::remove_file(path).unwrap();
    assert!(result.is_err());
    assert!(started.elapsed() < std::time::Duration::from_secs(2));
}
