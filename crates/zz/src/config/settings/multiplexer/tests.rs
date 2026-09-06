#![cfg(unix)]

use std::{
    cell::RefCell,
    net::Shutdown,
    os::unix::net::{UnixListener, UnixStream},
    rc::Rc,
    sync::mpsc,
    thread,
};

use zz_daemon::InteractiveClient;
use zz_protocol::{
    ClientId, ClientInstanceId, Event, EventPayload, KeyTableSnapshot, MuxOptions,
    PROTOCOL_VERSION, ProtocolMessage, ServerHello, StatusLine, read_protocol_message,
    write_protocol_message,
};
use zz_terminal::{AppearanceProvenance, TerminalAppearance};

use super::*;

struct TestServer {
    stream: UnixStream,
    thread: Option<thread::JoinHandle<()>>,
    directory: tempfile::TempDir,
}

impl Drop for TestServer {
    fn drop(&mut self) {
        let _ = self.stream.shutdown(Shutdown::Both);
        if let Some(thread) = self.thread.take() {
            thread.join().expect("test protocol server");
        }
    }
}

fn key_tables(source: &str) -> Vec<KeyTableSnapshot> {
    let mut engine = zz_mux::MuxEngine::default();
    let mut context = zz_mux::ExecutionContext::default();
    let parsed = zz_mux::MuxEngine::parse_config_without_variable_expansion("mux.conf", source);
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    for command in parsed.commands {
        engine.execute(&mut context, &command).unwrap();
    }
    engine.keys.snapshot()
}

fn test_server(tables: Vec<KeyTableSnapshot>) -> (InteractiveClient, TestServer) {
    let directory = tempfile::Builder::new()
        .prefix("zz-split-")
        .tempdir_in("/tmp")
        .unwrap();
    let socket = directory.path().join("s");
    let listener = UnixListener::bind(&socket).unwrap();
    let (sender, receiver) = mpsc::channel();
    let thread = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        assert!(matches!(
            read_protocol_message(&mut stream).unwrap(),
            ProtocolMessage::ClientHello(_)
        ));
        sender.send(stream.try_clone().unwrap()).unwrap();
        write_protocol_message(
            &mut stream,
            &ProtocolMessage::ServerHello(ServerHello {
                protocol_version: PROTOCOL_VERSION,
                server_id: 1,
                client_id: ClientId(1),
                client_instance_id: ClientInstanceId(1),
                capabilities: Vec::new(),
                appearance: TerminalAppearance::default(),
                appearance_provenance: AppearanceProvenance::default(),
                mux_options: MuxOptions::default(),
                status: StatusLine::default(),
                key_tables: tables,
            }),
        )
        .unwrap();
        while read_protocol_message(&mut stream).is_ok() {}
    });
    let client = InteractiveClient::connect(&socket).unwrap();
    let stream = receiver.recv().unwrap();
    (
        client,
        TestServer {
            stream,
            thread: Some(thread),
            directory,
        },
    )
}

#[gpui::test]
fn split_controls_follow_wire_bindings_preserve_drafts_and_keep_renamed_keys(
    cx: &mut gpui::TestAppContext,
) {
    cx.executor().allow_parking();
    cx.update(zz_ui::init);
    cx.update(|cx| config::set_fleet_hosts_for_test(Vec::new(), cx));
    let source =
        "# fixture\nbind - split-window -v -c '#{pane_current_path}'\nbind | split-picker -h\n";
    let (client, server) = test_server(key_tables(source));
    let path = server.directory.path().join("mux.conf");
    std::fs::write(&path, source).unwrap();
    let socket = server.directory.path().join("s");
    let captured = Rc::new(RefCell::new(None));
    let captured_for_window = Rc::clone(&captured);
    let (_, cx) = cx.add_window_view(move |window, cx| {
        let mux = cx.new(|cx| MuxClient::new(Ok(client), socket, cx));
        captured_for_window.replace(Some(cx.entity()));
        SettingsView::new(mux, window, cx)
    });
    let settings = captured.borrow().clone().unwrap();

    cx.update(|window, cx| {
        settings.update(cx, |settings, cx| {
            settings.section = SettingsSection::Multiplexer;
            settings.ensure_section_state(SettingsSection::Multiplexer, window, cx);
            let file = settings.config_file_editor_mut(ConfigFileKind::Mux);
            file.path = Some(path.clone());
            file.saved = source.to_owned();
            file.error = None;
            file.editor
                .update(cx, |editor, cx| editor.set_value(source, window, cx));
            settings.synchronize_mux_splits(window, cx);
            assert_eq!(settings.mux_split_disabled_reason(cx), None);
            let rows = &settings.mux_split_controls.as_ref().unwrap().rows;
            assert_eq!(rows[0].key.read(cx).value().as_ref(), "-");
            assert_eq!(rows[1].key.read(cx).value().as_ref(), "|");
            assert_eq!(
                split_binding_kind(rows[0].binding.as_ref().unwrap()),
                Some(SplitPaneKind::Terminal)
            );
        });
    });

    let draft = format!("{source}# unsaved edit\n");
    cx.update(|window, cx| {
        settings.update(cx, |settings, cx| {
            let editor = settings
                .config_file_editor(ConfigFileKind::Mux)
                .editor
                .clone();
            editor.update(cx, |editor, cx| editor.set_value(&draft, window, cx));
            assert!(
                settings
                    .mux_split_disabled_reason(cx)
                    .unwrap()
                    .contains("Save")
            );
            settings.commit_mux_split(0, SplitPaneKind::Picker, window, cx);
            assert_eq!(editor.read(cx).value().as_ref(), draft);
            assert_eq!(
                settings.config_file_editor(ConfigFileKind::Mux).saved,
                source
            );
        });
    });
    assert_eq!(std::fs::read_to_string(&path).unwrap(), source);

    cx.update(|window, cx| {
        settings.update(cx, |settings, cx| {
            settings
                .config_file_editor(ConfigFileKind::Mux)
                .editor
                .update(cx, |editor, cx| editor.set_value(source, window, cx));
            let key = settings.mux_split_controls.as_ref().unwrap().rows[0]
                .key
                .clone();
            key.update(cx, |input, cx| input.set_value("v", window, cx));
            settings.synchronize_mux_splits(window, cx);
            assert_eq!(key.read(cx).value().as_ref(), "v");
            settings.commit_mux_split(0, SplitPaneKind::Terminal, window, cx);
            assert!(settings.mux_split_disabled_reason(cx).is_some());
            let pending_source = settings
                .config_file_editor(ConfigFileKind::Mux)
                .saved
                .clone();
            settings.commit_mux_split(0, SplitPaneKind::Browser, window, cx);
            assert_eq!(
                settings.config_file_editor(ConfigFileKind::Mux).saved,
                pending_source
            );
        });
    });
    let saved = std::fs::read_to_string(&path).unwrap();
    let tables = key_tables(&saved);
    let prefix = &tables
        .iter()
        .find(|table| table.name == "prefix")
        .unwrap()
        .bindings;
    assert!(prefix.iter().all(|binding| binding.key != "-"));
    assert!(prefix.iter().any(|binding| binding.key == "v"));
    cx.update(|window, cx| {
        settings.update(cx, |settings, cx| {
            settings.mux.update(cx, |mux, cx| {
                mux.handle_message_for_test(
                    ProtocolMessage::Event(Event {
                        sequence: 1,
                        payload: EventPayload::KeyTablesChanged { tables },
                    }),
                    cx,
                );
            });
            settings.synchronize_mux_splits(window, cx);
            let row = &settings.mux_split_controls.as_ref().unwrap().rows[0];
            assert_eq!(row.key.read(cx).value().as_ref(), "v");
            assert_eq!(row.binding.as_ref().unwrap().key, "v");
            assert_eq!(settings.mux_split_disabled_reason(cx), None);
        });
    });

    let externally_changed = key_tables(&format!("{saved}bind v split-window -v -c /tmp\n"));
    cx.update(|window, cx| {
        settings.update(cx, |settings, cx| {
            settings.mux.update(cx, |mux, cx| {
                mux.handle_message_for_test(
                    ProtocolMessage::Event(Event {
                        sequence: 2,
                        payload: EventPayload::KeyTablesChanged {
                            tables: externally_changed,
                        },
                    }),
                    cx,
                );
            });
            settings.commit_mux_split(0, SplitPaneKind::Browser, window, cx);
            assert_eq!(
                settings.config_file_editor(ConfigFileKind::Mux).saved,
                saved
            );
            let row = &settings.mux_split_controls.as_ref().unwrap().rows[0];
            assert_eq!(split_binding_kind(row.binding.as_ref().unwrap()), None);
        });
    });
    assert_eq!(std::fs::read_to_string(&path).unwrap(), saved);
}
