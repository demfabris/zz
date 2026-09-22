#![cfg(unix)]

use std::{
    ffi::CString,
    path::{Path, PathBuf},
    ptr, thread,
    time::{Duration, Instant},
};

use serde_json::{Value, json};
use zz_client_ffi::*;
use zz_daemon::{CommandClient, Daemon};
use zz_protocol::CommandInvocation;

struct Fixture {
    root: PathBuf,
    socket: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = PathBuf::from(format!("/tmp/zz-mobile-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let socket = root.join("d.sock");
        let daemon = Daemon::new(&socket).without_user_config();
        thread::spawn(move || daemon.run_foreground());
        eventually("daemon starts", || CommandClient::connect(&socket).is_ok());
        Self { root, socket }
    }

    fn execute(&self, name: &str, args: &[&str]) {
        CommandClient::connect(&self.socket)
            .unwrap()
            .execute(CommandInvocation::new(name, args.iter().copied()))
            .unwrap();
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if let Ok(mut commands) = CommandClient::connect(&self.socket) {
            let _ = commands.execute(CommandInvocation::new("kill-server", [] as [&str; 0]));
        }
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

struct Client(*mut ZzClient);

impl Client {
    fn new(socket: &Path) -> Self {
        let socket = CString::new(socket.to_str().unwrap()).unwrap();
        let client = unsafe { zz_client_connect(socket.as_ptr()) };
        assert!(!client.is_null());
        assert!(unsafe { zz_client_attach(client, c"mobile".as_ptr()) });
        let client = Self(client);
        eventually("client receives terminal", || client.pane().is_some());
        client
    }

    fn pane(&self) -> Option<u64> {
        let mut pane = 0;
        (unsafe { zz_client_terminal_panes(self.0, &mut pane, 1) } > 0).then_some(pane)
    }

    fn state(&self) -> Value {
        read_json(unsafe { zz_client_tmux_state_json(self.0) })
    }

    fn action(&self, value: Value) {
        let json = CString::new(value.to_string()).unwrap();
        assert!(unsafe { zz_client_tmux_action_json(self.0, json.as_ptr()) });
    }

    fn key(&self, pane: u64, character: char, modifiers: u8) {
        let text = CString::new(character.to_string()).unwrap();
        assert!(unsafe {
            zz_client_send_key(
                self.0,
                pane,
                0,
                character.into(),
                0,
                0,
                0,
                modifiers,
                text.as_ptr(),
                false,
            )
        });
        assert!(unsafe {
            zz_client_send_key(
                self.0,
                pane,
                0,
                character.into(),
                0,
                0,
                2,
                modifiers,
                ptr::null(),
                false,
            )
        });
    }

    fn background(&self, pane: u64) -> Option<u32> {
        let viewport = unsafe { zz_client_viewport_acquire(self.0, pane) };
        if viewport.is_null() {
            return None;
        }
        let color = unsafe { zz_viewport_background(viewport) };
        unsafe { zz_viewport_release(viewport) };
        Some(color)
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        unsafe { zz_client_free(self.0) };
    }
}

struct Settings(*mut ZzSettingsModel);

impl Drop for Settings {
    fn drop(&mut self) {
        unsafe { zz_settings_model_free(self.0) };
    }
}

fn read_json(handle: *mut ZzJson) -> Value {
    assert!(!handle.is_null());
    let bytes = unsafe { zz_json_bytes(handle) };
    let result =
        serde_json::from_slice(unsafe { std::slice::from_raw_parts(bytes.ptr, bytes.len) })
            .unwrap();
    unsafe { zz_json_free(handle) };
    result
}

fn eventually(description: &str, mut predicate: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        if predicate() {
            return;
        }
        assert!(Instant::now() < deadline, "timed out: {description}");
        thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn mobile_settings_and_tmux_interaction_share_daemon_contracts() {
    let fixture = Fixture::new();
    fixture.execute(
        "new-session",
        &[
            "-d",
            "-s",
            "mobile",
            "printf 'mobile-ready\\r\\n'; exec /bin/cat",
        ],
    );
    fixture.execute("set-option", &["-g", "prefix", "C-b"]);
    let mobile = Client::new(&fixture.socket);
    let observer = Client::new(&fixture.socket);
    let pane = mobile.pane().unwrap();
    eventually("both clients receive a viewport", || {
        mobile.background(pane).is_some() && observer.background(pane).is_some()
    });
    eventually("fixture text is available", || {
        let viewport = unsafe { zz_client_viewport_acquire(mobile.0, pane) };
        if viewport.is_null() {
            return false;
        }
        let mut text = vec![0; 512];
        let found = (0..unsafe { zz_viewport_rows(viewport) }).any(|row| {
            let length =
                unsafe { zz_viewport_row_text(viewport, row, text.as_mut_ptr(), text.len()) };
            String::from_utf8_lossy(unsafe {
                std::slice::from_raw_parts(text.as_ptr().cast::<u8>(), length)
            })
            .contains("mobile-ready")
        });
        unsafe { zz_viewport_release(viewport) };
        found
    });
    let original_background = observer.background(pane).unwrap();
    let original_appearance = read_json(unsafe { zz_client_appearance_json(observer.0) });

    let theme_dir = fixture.root.join("themes");
    std::fs::create_dir_all(&theme_dir).unwrap();
    std::fs::write(
        theme_dir.join("Mobile Test"),
        "background = #123456\nforeground = #abcdef\npalette = 1=#654321\n",
    )
    .unwrap();
    let config = fixture.root.join("config");
    let mux = fixture.root.join("mux.conf");
    std::fs::write(&config, "theme = Mobile Test\nfont-size = 19\n").unwrap();
    std::fs::write(
        &mux,
        "set-option -g prefix C-a\nset-window-option -g mode-keys vi\nbind-key -T prefix z choose-tree\n",
    )
    .unwrap();
    let config = CString::new(config.to_str().unwrap()).unwrap();
    let mux = CString::new(mux.to_str().unwrap()).unwrap();
    let directory = CString::new(theme_dir.to_str().unwrap()).unwrap();
    let settings = Settings(unsafe {
        zz_settings_model_new(c"monospace".as_ptr(), config.as_ptr(), mux.as_ptr())
    });
    assert!(!settings.0.is_null());
    let themes =
        read_json(unsafe { zz_settings_model_themes_json(settings.0, directory.as_ptr(), true) });
    assert_eq!(themes[0]["name"], "Mobile Test");
    assert_eq!(themes[0]["background"], "#123456");
    assert!(unsafe { zz_settings_model_mobile_apply(settings.0, mobile.0) });
    eventually("mobile mux prefix is applied", || unsafe {
        zz_client_claims_prefix_key(mobile.0, 0, 'a'.into(), 0, 2)
    });
    eventually("custom binding is published", || {
        let tables = read_json(unsafe { zz_client_key_tables_json(mobile.0) });
        tables.as_array().unwrap().iter().any(|table| {
            table["name"] == "prefix"
                && table["bindings"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|binding| binding["key"] == "z" && binding["summary"] == "choose-tree")
        })
    });

    let local = unsafe {
        zz_settings_model_viewport_acquire(settings.0, mobile.0, pane, directory.as_ptr(), true)
    };
    assert!(!local.is_null());
    assert_eq!(unsafe { zz_viewport_background(local) }, 0x123456);
    unsafe { zz_viewport_release(local) };
    assert_eq!(observer.background(pane), Some(original_background));
    assert_eq!(mobile.background(pane), Some(original_background));
    assert_eq!(
        read_json(unsafe { zz_client_appearance_json(observer.0) }),
        original_appearance
    );

    mobile.key(pane, 'a', 2);
    mobile.key(pane, 'z', 0);
    eventually("custom prefix binding opens chooser", || {
        mobile.state()["tree"].is_object()
    });
    assert!(
        !mobile.state()["tree"]["items"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    mobile.action(json!({"ChooseTree":{"action":"Close"}}));
    eventually("chooser closes", || mobile.state()["tree"].is_null());
    mobile.action(json!({"TerminalView":{"pane":pane,"action":"EnterCopyMode"}}));
    eventually("copy mode reports pane", || {
        mobile.state()["copies"]
            .as_array()
            .unwrap()
            .iter()
            .any(|copy| copy["pane"] == pane)
    });
    mobile.action(json!({"TerminalView":{"pane":pane,"action":"SelectAll"}}));
    eventually("selection spans reach native viewport", || {
        let viewport = unsafe { zz_client_viewport_acquire(mobile.0, pane) };
        if viewport.is_null() {
            return false;
        }
        let count = unsafe { zz_viewport_overlay_count(viewport) };
        let has_selection = if count == 0 {
            false
        } else {
            unsafe { std::slice::from_raw_parts(zz_viewport_overlays(viewport), count) }
                .iter()
                .any(|span| span.kind() == zz_terminal::OverlayKind::Selection)
        };
        unsafe { zz_viewport_release(viewport) };
        has_selection
    });
    assert!(unsafe { zz_client_copy_selection(mobile.0, pane, 42) });
    eventually("selection produces clipboard text", || {
        let clipboard = unsafe { zz_client_clipboard_next(mobile.0) };
        if clipboard.is_null() {
            return false;
        }
        let bytes = unsafe { zz_clipboard_text(clipboard) };
        let text =
            String::from_utf8_lossy(unsafe { std::slice::from_raw_parts(bytes.ptr, bytes.len) });
        let found = text.contains("mobile-ready");
        unsafe { zz_clipboard_release(clipboard) };
        found
    });
    mobile.action(json!({"TerminalView":{"pane":pane,"action":{"SearchBegin":{"text":"mobile-ready","mode":"Literal","case":"Smart","direction":"Forward"}}}}));
    eventually("search reports a match", || {
        mobile.state()["copies"]
            .as_array()
            .unwrap()
            .iter()
            .any(|copy| copy["matches"].as_u64().unwrap_or(0) > 0)
    });
    mobile.action(json!({"TerminalView":{"pane":pane,"action":{"CopyMode":"Cancel"}}}));
    eventually("copy mode exits", || {
        mobile.state()["copies"].as_array().unwrap().is_empty()
    });
    drop(mobile);
    fixture.execute("set-option", &["-g", "prefix", "C-b"]);
    let mobile = Client::new(&fixture.socket);
    assert!(unsafe { zz_settings_model_mobile_apply(settings.0, mobile.0) });
    eventually("reconnected client reapplies its saved prefix", || unsafe {
        zz_client_claims_prefix_key(mobile.0, 0, 'a'.into(), 0, 2)
    });
    assert!(unsafe {
        zz_settings_model_mobile_action(
            settings.0,
            mobile.0,
            c"{\"action\":\"set-mux\",\"key\":\"prefix\",\"value\":null}".as_ptr(),
        )
    });
    assert!(unsafe { zz_settings_model_mobile_apply(settings.0, mobile.0) });
    eventually("reset restores the host prefix", || {
        CommandClient::connect(&fixture.socket)
            .unwrap()
            .execute(CommandInvocation::new(
                "show-options",
                ["-g", "-v", "prefix"],
            ))
            .is_ok_and(|value| value.trim() == "C-b")
    });
}
