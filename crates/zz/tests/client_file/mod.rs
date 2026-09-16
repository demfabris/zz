//! A daemon whose working directory and home are deliberately not the
//! client's, so a buffer command that reads or writes on the wrong side of the
//! connection is visible.

#![allow(dead_code)]

use std::{
    path::{Path, PathBuf},
    process::{Child, Command, Output, Stdio},
    thread,
    time::{Duration, Instant},
};

pub struct Daemon {
    _directory: tempfile::TempDir,
    root: PathBuf,
    socket: PathBuf,
    config: PathBuf,
    process: Option<Child>,
}

impl Daemon {
    pub fn start(name: &str) -> Self {
        let directory = tempfile::Builder::new()
            .prefix(&format!("zz-{name}-"))
            .tempdir_in("/tmp")
            .expect("temporary client-file directory");
        let root = directory.path().to_path_buf();
        for leaf in ["daemon", "daemon-home", "client", "client-home"] {
            std::fs::create_dir_all(root.join(leaf)).expect("fixture directory");
        }
        let socket = root.join("d.sock");
        let config = root.join("empty.conf");
        std::fs::write(&config, b"").expect("empty mux config");

        let process = Command::new(env!("CARGO_BIN_EXE_zz"))
            .arg("-f")
            .arg(&config)
            .arg("-S")
            .arg(&socket)
            .arg("daemon")
            .current_dir(root.join("daemon"))
            .env("HOME", root.join("daemon-home"))
            .env("XDG_CONFIG_HOME", root.join("daemon-home"))
            .env_remove("TMUX")
            .env_remove("TMUX_PANE")
            .env_remove("ZZ_SOCKET")
            .env_remove("ZZ_SESSION")
            .env_remove("ZZ_PANE")
            .env_remove("PWD")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn the daemon");

        let daemon = Self {
            _directory: directory,
            root,
            socket,
            config,
            process: Some(process),
        };
        daemon.await_socket();
        daemon
    }

    fn await_socket(&self) {
        let deadline = Instant::now() + Duration::from_secs(30);
        while !self.socket.exists() {
            assert!(Instant::now() < deadline, "the daemon never bound a socket");
            thread::sleep(Duration::from_millis(20));
        }
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let listed = self.run_in_client_directory(&["list-sessions", "-F", "#{session_name}"]);
            if listed.status.code() == Some(0) || listed.status.code() == Some(1) {
                return;
            }
            assert!(Instant::now() < deadline, "the daemon never answered");
            thread::sleep(Duration::from_millis(20));
        }
    }

    pub fn daemon_directory(&self) -> PathBuf {
        self.root.join("daemon")
    }

    pub fn daemon_home(&self) -> PathBuf {
        self.root.join("daemon-home")
    }

    pub fn client_directory(&self) -> PathBuf {
        self.root.join("client")
    }

    pub fn client_home(&self) -> PathBuf {
        self.root.join("client-home")
    }

    pub fn run_in_client_directory(&self, arguments: &[&str]) -> Output {
        self.run_in(&self.client_directory(), arguments)
    }

    pub fn run_in(&self, directory: &Path, arguments: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_zz"))
            .arg("-f")
            .arg(&self.config)
            .arg("-S")
            .arg(&self.socket)
            .args(arguments)
            .current_dir(directory)
            .env("HOME", self.client_home())
            .env("XDG_CONFIG_HOME", self.client_home())
            .env("PWD", directory)
            .env_remove("TMUX")
            .env_remove("TMUX_PANE")
            .env_remove("ZZ_SOCKET")
            .env_remove("ZZ_SESSION")
            .env_remove("ZZ_PANE")
            .output()
            .expect("run a zz command")
    }

    pub fn buffer(&self, name: &str) -> String {
        let shown = self.run_in_client_directory(&["show-buffer", "-b", name]);
        assert!(shown.status.success(), "{shown:?}");
        String::from_utf8_lossy(&shown.stdout).into_owned()
    }

    pub fn set_buffer(&self, name: &str, value: &str) {
        let set = self.run_in_client_directory(&["set-buffer", "-b", name, value]);
        assert!(set.status.success(), "{set:?}");
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.run_in_client_directory(&["kill-server"]);
        if let Some(mut process) = self.process.take() {
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                match process.try_wait() {
                    Ok(Some(_)) => break,
                    Ok(None) if Instant::now() < deadline => {
                        thread::sleep(Duration::from_millis(20));
                    }
                    _ => {
                        let _ = process.kill();
                        let _ = process.wait();
                        break;
                    }
                }
            }
        }
    }
}
