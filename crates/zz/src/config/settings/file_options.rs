use super::*;
use zz_protocol::{CommandInvocation, MuxOptionKey, MuxOptions};
use zz_terminal::{AppearanceConfigKey, TerminalAppearance};

#[derive(Clone, Copy)]
enum FileKey {
    Mux(MuxOptionKey),
    Appearance(AppearanceConfigKey),
}

impl FileKey {
    fn name(self) -> &'static str {
        match self {
            Self::Mux(key) => key.as_str(),
            Self::Appearance(key) => key.as_str(),
        }
    }

    fn value(self, source: &str) -> Option<String> {
        match self {
            Self::Mux(key) => config::file_options::mux_option_value(source, key),
            Self::Appearance(key) => config::file_options::appearance_value(source, key),
        }
    }

    fn default_value(self) -> String {
        match self {
            Self::Mux(key) => MuxOptions::default().get(key).unwrap().value.clone(),
            Self::Appearance(AppearanceConfigKey::Theme) => String::new(),
            Self::Appearance(AppearanceConfigKey::WindowPaddingX) => {
                TerminalAppearance::default().padding_left.to_string()
            }
            Self::Appearance(AppearanceConfigKey::WindowPaddingY) => {
                TerminalAppearance::default().padding_top.to_string()
            }
            Self::Appearance(key) => {
                config::appearance_config_values(&TerminalAppearance::default(), key)
                    .ok()
                    .and_then(|values| values.into_iter().rev().find(|value| !value.is_empty()))
                    .unwrap_or_default()
            }
        }
    }
}

struct OptionRow {
    key: FileKey,
    title: &'static str,
    value: Option<String>,
    input: Entity<InputState>,
    select: Option<Entity<SelectState<Vec<SettingsSelectItem>>>>,
    numeric: bool,
}

pub(super) struct FileOptions {
    rows: Vec<OptionRow>,
    source: String,
    donor: Entity<InputState>,
    _subscriptions: Vec<Subscription>,
}

impl SettingsView {
    pub(super) fn synchronize_file_options(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let kind = match self.section {
            SettingsSection::Multiplexer => ConfigFileKind::Mux,
            SettingsSection::Terminal => ConfigFileKind::Terminal,
            _ => return,
        };
        let source = self.config_file_editor(kind).saved.clone();
        if let std::collections::btree_map::Entry::Vacant(vacant) = self.file_options.entry(kind) {
            let definitions = match kind {
                ConfigFileKind::Mux => vec![
                    (FileKey::Mux(MuxOptionKey::Prefix), "Prefix", false),
                    (FileKey::Mux(MuxOptionKey::ModeKeys), "Mode keys", false),
                    (FileKey::Mux(MuxOptionKey::Mouse), "Mouse", false),
                    (
                        FileKey::Mux(MuxOptionKey::HistoryLimit),
                        "History limit",
                        true,
                    ),
                    (FileKey::Mux(MuxOptionKey::SetClipboard), "Clipboard", false),
                    (
                        FileKey::Mux(MuxOptionKey::EscapeTime),
                        "Escape time (ms)",
                        true,
                    ),
                ],
                ConfigFileKind::Terminal => vec![
                    (
                        FileKey::Appearance(AppearanceConfigKey::FontFamily),
                        "Font family",
                        false,
                    ),
                    (
                        FileKey::Appearance(AppearanceConfigKey::FontSize),
                        "Font size",
                        true,
                    ),
                    (
                        FileKey::Appearance(AppearanceConfigKey::Theme),
                        "Theme",
                        false,
                    ),
                    (
                        FileKey::Appearance(AppearanceConfigKey::CursorStyle),
                        "Cursor style",
                        false,
                    ),
                    (
                        FileKey::Appearance(AppearanceConfigKey::CursorStyleBlink),
                        "Cursor blink",
                        false,
                    ),
                    (
                        FileKey::Appearance(AppearanceConfigKey::BackgroundOpacity),
                        "Background opacity (%)",
                        true,
                    ),
                    (
                        FileKey::Appearance(AppearanceConfigKey::WindowPaddingX),
                        "Padding X",
                        true,
                    ),
                    (
                        FileKey::Appearance(AppearanceConfigKey::WindowPaddingY),
                        "Padding Y",
                        true,
                    ),
                ],
            };
            let donor_path = match kind {
                ConfigFileKind::Mux => zz_daemon::discover_tmux_config(),
                ConfigFileKind::Terminal => discover_ghostty_config(),
            };
            let donor = text_value_input(
                &donor_path
                    .map(|path| path.display().to_string())
                    .unwrap_or_default(),
                window,
                cx,
            );
            let mut subscriptions = vec![cx.observe(&donor, |_, _, cx| cx.notify())];
            let mut rows = Vec::new();
            for (index, (key, title, numeric)) in definitions.into_iter().enumerate() {
                let value = key.value(&source);
                let displayed =
                    display_value(key, value.as_deref().unwrap_or(&key.default_value()));
                let input = cx.new(|cx| {
                    let input = InputState::new(window, cx).default_value(displayed);
                    let input = if numeric {
                        input.step(if key.name() == "background-opacity" {
                            5.0
                        } else {
                            1.0
                        })
                    } else {
                        input
                    };
                    if key.name() == "background-opacity" {
                        input.min(0.0).max(100.0)
                    } else if matches!(key.name(), "window-padding-x" | "window-padding-y") {
                        input.min(0.0)
                    } else {
                        input
                    }
                });
                subscriptions.push(cx.subscribe_in(
                    &input,
                    window,
                    move |this, input, event, window, cx| {
                        if matches!(event, InputEvent::Change)
                            && this.section == SettingsSection::Terminal
                        {
                            cx.notify();
                        }
                        if matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur) {
                            let mut value = input.read(cx).value().to_string();
                            if key.name() == "background-opacity" {
                                let Ok(percent) = value.parse::<f32>() else {
                                    return;
                                };
                                if !(0.0..=100.0).contains(&percent) {
                                    return;
                                }
                                value = (percent / 100.0).to_string();
                            }
                            this.commit_file_option(kind, index, Some(&value), window, cx);
                        }
                    },
                ));
                let choices = option_choices(key, import_color_scheme(cx));
                let select = if choices.is_empty() {
                    None
                } else {
                    let refs = choices
                        .iter()
                        .map(|(label, value)| (label.as_str(), value.as_str()))
                        .collect::<Vec<_>>();
                    let select = settings_select_state(
                        &refs,
                        &select_value(key, value.as_deref().unwrap_or(&key.default_value())),
                        window,
                        cx,
                    );
                    subscriptions.push(cx.subscribe_in(
                        &select,
                        window,
                        move |this, _, event, window, cx| {
                            if let SelectEvent::Confirm(Some(value)) = event {
                                this.commit_file_option(
                                    kind,
                                    index,
                                    (!value.is_empty()).then_some(value.as_str()),
                                    window,
                                    cx,
                                );
                            }
                        },
                    ));
                    Some(select)
                };
                rows.push(OptionRow {
                    key,
                    title,
                    value,
                    input,
                    select,
                    numeric,
                });
            }
            vacant.insert(FileOptions {
                rows,
                source: source.clone(),
                donor,
                _subscriptions: subscriptions,
            });
        }
        let controls = self.file_options.get_mut(&kind).unwrap();
        if controls.source != source {
            controls.source.clone_from(&source);
            for row in &mut controls.rows {
                row.value = row.key.value(&source);
                let value = row.value.clone().unwrap_or_else(|| row.key.default_value());
                row.input.update(cx, |input, cx| {
                    input.set_value(display_value(row.key, &value), window, cx);
                });
                if let Some(select) = &row.select {
                    select.update(cx, |select, cx| {
                        select.set_selected_value(&select_value(row.key, &value), window, cx);
                    });
                }
            }
        }
    }

    fn file_controls_disabled(&self, kind: ConfigFileKind, cx: &App) -> bool {
        let file = self.config_file_editor(kind);
        file.error.is_some()
            || file.editor.read(cx).value().as_ref() != file.saved
            || self.pending_file_command.is_some()
            || (kind == ConfigFileKind::Mux
                && (!self.mux.read(cx).is_connected()
                    || self.mux.read(cx).attached_host() != HostId::LOCAL))
    }

    pub(super) fn terminal_preview_source(&self, cx: &App) -> String {
        let file = self.config_file_editor(ConfigFileKind::Terminal);
        let mut source = file.editor.read(cx).value().to_string();
        if source != file.saved {
            return source;
        }
        let Some(controls) = self.file_options.get(&ConfigFileKind::Terminal) else {
            return source;
        };
        for row in &controls.rows {
            let FileKey::Appearance(key) = row.key else {
                continue;
            };
            if row.select.is_some() {
                continue;
            }
            let value = row.input.read(cx).value();
            let saved = display_value(
                row.key,
                row.value.as_deref().unwrap_or(&row.key.default_value()),
            );
            if value.as_ref() == saved {
                continue;
            }
            let value = if key == AppearanceConfigKey::BackgroundOpacity {
                let Ok(percent) = value.parse::<f32>() else {
                    continue;
                };
                if !(0.0..=100.0).contains(&percent) {
                    continue;
                }
                (percent / 100.0).to_string()
            } else {
                value.to_string()
            };
            if let Ok(edited) =
                config::file_options::edit_appearance_option(&source, key, Some(&value))
            {
                source = edited;
            }
        }
        source
    }

    pub(super) fn file_options_section(
        &self,
        kind: ConfigFileKind,
        cx: &Context<Self>,
    ) -> AnyElement {
        let Some(controls) = self.file_options.get(&kind) else {
            return div().into_any_element();
        };
        let disabled = self.file_controls_disabled(kind, cx);
        let mut stack = SettingsStack::titled(if kind == ConfigFileKind::Mux {
            "Options"
        } else {
            "Appearance"
        })
        .description(if disabled {
            "Save your editor changes and connect locally before editing multiplexer options."
        } else {
            "Edit a value and press Enter. These rows use the saved file below."
        });
        for (index, row) in controls.rows.iter().enumerate() {
            let provenance = if row.value.is_some() {
                ConfigProvenance::Override
            } else {
                ConfigProvenance::Default
            };
            let reset = settings_reset_button(
                format!("reset-file-{}", row.key.name()),
                if row.value.is_some() {
                    "Reset to the inherited or default value"
                } else {
                    "Already using the default value"
                },
                !disabled && row.value.is_some(),
            )
            .on_click(cx.listener(move |this, _, window, cx| {
                this.commit_file_option(kind, index, None, window, cx);
            }));
            let annotations = h_flex()
                .flex_none()
                .gap(px(8.0))
                .child(reset)
                .child(provenance_badge(provenance));
            let description = if row.key.name() == "theme" {
                "Choosing a theme replaces custom terminal colors."
            } else {
                row.key.name()
            };
            let entry = SettingEntry::new(row.title, description)
                .title_actions(annotations)
                .disabled(disabled);
            let control = if row.key.name() == "mouse" {
                let enabled = matches!(
                    row.value.as_deref().unwrap_or("on"),
                    "on" | "true" | "yes" | "1"
                );
                boolean_control(
                    row.key.name(),
                    enabled,
                    cx.listener(move |this, enabled: &bool, window, cx| {
                        this.commit_file_option(
                            kind,
                            index,
                            Some(if *enabled { "on" } else { "off" }),
                            window,
                            cx,
                        );
                    }),
                )
                .into_any_element()
            } else if let Some(select) = &row.select {
                select_control(select, cx)
                    .when(matches!(row.key.name(), "theme" | "cursor-style"), |this| {
                        this.w(px(240.0))
                    })
                    .into_any_element()
            } else if row.numeric {
                numeric_control(&row.input, cx).into_any_element()
            } else {
                div()
                    .w(px(200.0))
                    .child(Input::new(&row.input).small().bg(settings_control_fill(cx)))
                    .into_any_element()
            };
            stack = stack.child(entry.control(control));
        }
        div().flex_none().child(stack).into_any_element()
    }

    pub(super) fn file_import_section(
        &self,
        kind: ConfigFileKind,
        cx: &Context<Self>,
    ) -> AnyElement {
        if !crate::profile::profile(cx).has_config_import {
            return div().into_any_element();
        }
        let Some(controls) = self.file_options.get(&kind) else {
            return div().into_any_element();
        };
        let disabled = self.file_controls_disabled(kind, cx);
        SettingsStack::titled("Import")
            .child(
                SettingEntry::new(
                    "Source file",
                    "Choose a configuration file to copy into zz.",
                )
                .control(
                    h_flex()
                        .gap_2()
                        .child(
                            div()
                                .w(px(230.0))
                                .child(Input::new(&controls.donor).small()),
                        )
                        .child(
                            Button::new("choose-config-donor")
                                .small()
                                .label("Choose…")
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    let selected = cx.prompt_for_paths(gpui::PathPromptOptions {
                                        files: true,
                                        directories: false,
                                        multiple: false,
                                        prompt: Some("Choose configuration".into()),
                                    });
                                    let input = this.file_options[&kind].donor.clone();
                                    cx.spawn_in(window, async move |_, window| {
                                        let path =
                                            selected.await.ok()?.ok()??.into_iter().next()?;
                                        window
                                            .update(|window, cx| {
                                                input.update(cx, |input, cx| {
                                                    input.set_value(
                                                        path.display().to_string(),
                                                        window,
                                                        cx,
                                                    );
                                                });
                                            })
                                            .ok()
                                    })
                                    .detach();
                                })),
                        )
                        .child(
                            Button::new("import-config-donor")
                                .small()
                                .label("Import")
                                .disabled(
                                    disabled || controls.donor.read(cx).value().trim().is_empty(),
                                )
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.confirm_file_import(kind, window, cx);
                                })),
                        ),
                ),
            )
            .into_any_element()
    }

    fn commit_file_option(
        &mut self,
        kind: ConfigFileKind,
        index: usize,
        value: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.file_controls_disabled(kind, cx) {
            return;
        }
        let key = self.file_options[&kind].rows[index].key;
        let saved = self.file_options[&kind].rows[index].value.as_deref();
        if (saved == value
            || value.is_some_and(|value| {
                display_value(key, value)
                    == display_value(key, saved.unwrap_or(&key.default_value()))
            }))
            && !(matches!(key, FileKey::Appearance(AppearanceConfigKey::Theme)) && value.is_some())
        {
            return;
        }
        let result = (|| -> io::Result<()> {
            let path = config_file_path(kind)?;
            let source = config::read_config_editor_source(&path, kind.max_bytes())?;
            if kind.editor_view(source.clone()) != self.config_file_editor(kind).saved {
                return Err(io::Error::other(
                    "configuration changed on disk; reload before editing",
                ));
            }
            match key {
                FileKey::Mux(key) => {
                    let edited = config::file_options::edit_mux_option(&source, key, value)?;
                    config::write_config_editor_source(&path, &edited, kind.max_bytes())?;
                }
                FileKey::Appearance(key) => {
                    config::file_options::write_appearance_option(&path, key, value)?;
                }
            }
            Ok(())
        })();
        if let Err(error) = result {
            report_write_error("set", key.name(), &error, cx);
            return;
        }
        self.reload_config_editor(kind, window, cx);
        if let FileKey::Mux(option) = key {
            let expected = key
                .value(&self.config_file_editor(kind).saved)
                .unwrap_or_else(|| key.default_value());
            self.run_file_command(
                kind,
                CommandInvocation::new("reload-config", [] as [&str; 0]),
                Some((option, expected)),
                window,
                cx,
            );
        } else {
            config::request_daemon_reload(cx);
        }
    }

    fn confirm_file_import(
        &mut self,
        kind: ConfigFileKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.file_controls_disabled(kind, cx) {
            return;
        }
        let source = self.file_options[&kind].donor.read(cx).value().to_string();
        if source.trim().is_empty() {
            return;
        }
        let target = match config_file_path(kind) {
            Ok(path) => path,
            Err(error) => {
                report_write_error("import", kind.file_name(), &error, cx);
                return;
            }
        };
        let description = format!(
            "Import {} into {}? You can choose other paths in Settings.",
            source,
            target.display()
        );
        let settings = cx.entity();
        window.open_alert_dialog(cx, move |alert, _, _| {
            let settings = settings.clone();
            let source = source.clone();
            import_configuration_file_alert(
                alert,
                if kind == ConfigFileKind::Mux {
                    "Import from tmux?"
                } else {
                    "Import from Ghostty?"
                },
                description.clone(),
            )
            .on_ok(move |_, window, cx| {
                settings.update(cx, |this, cx| {
                    if this.file_controls_disabled(kind, cx) {
                        return;
                    }
                    if kind == ConfigFileKind::Mux {
                        this.run_file_command(
                            kind,
                            CommandInvocation::new(
                                "import-tmux-config",
                                ["--".to_owned(), source.clone()],
                            ),
                            None,
                            window,
                            cx,
                        );
                    } else {
                        match config::import::import_ghostty_config_from(
                            &PathBuf::from(&source),
                            import_color_scheme(cx),
                        ) {
                            Ok(_) => {
                                this.reload_config_editor(kind, window, cx);
                                config::request_daemon_reload(cx);
                            }
                            Err(error) => {
                                report_write_error("import", kind.file_name(), &error, cx);
                            }
                        }
                    }
                });
                true
            })
        });
    }

    fn run_file_command(
        &mut self,
        kind: ConfigFileKind,
        command: CommandInvocation,
        expected: Option<(MuxOptionKey, String)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.cancel_pending_mux_split();
        self.file_command_sequence += 1;
        let sequence = self.file_command_sequence;
        self.pending_file_command = Some((sequence, kind));
        #[cfg(not(target_os = "ios"))]
        let server_id = self.mux.read(cx).local_server_id();
        #[cfg(target_os = "ios")]
        let server_id = None;
        let job = cx
            .background_executor()
            .spawn(async move { execute_file_command(command, expected, server_id) });
        cx.spawn_in(window, async move |this, window| {
            let result = job.await;
            let _ = window.update(|window, cx| {
                let _ = this.update(cx, |this, cx| {
                    if this.pending_file_command != Some((sequence, kind)) {
                        return;
                    }
                    this.pending_file_command = None;
                    this.reload_config_editor_if_clean(kind.section(), window, cx);
                    match result {
                        Ok(output) if !output.trim().is_empty() => {
                            toast::push(Notification::success(output), cx);
                        }
                        Ok(_) => {}
                        Err(error) => toast::push(Notification::error(error), cx),
                    }
                    cx.notify();
                });
            });
        })
        .detach();
        cx.spawn_in(window, async move |this, window| {
            window.background_executor().timer(std::time::Duration::from_secs(5)).await;
            let _ = window.update(|window, cx| { let _ = this.update(cx, |this, cx| {
                if this.pending_file_command == Some((sequence, kind)) {
                    this.pending_file_command = None;
                    this.reload_config_editor_if_clean(kind.section(), window, cx);
                    toast::push(Notification::info("The daemon has not confirmed the configuration. Review the file and retry Reload."), cx);
                    cx.notify();
                }
            }); });
        }).detach();
        cx.notify();
    }
}

#[cfg(not(target_os = "ios"))]
fn execute_file_command(
    command: CommandInvocation,
    expected: Option<(MuxOptionKey, String)>,
    server_id: Option<u64>,
) -> Result<String, String> {
    let mut client = config::local_command_client()?;
    if Some(client.server_hello().server_id) != server_id {
        return Err("The local daemon changed. Reconnect before editing.".to_owned());
    }
    let output = client.execute(command).map_err(|error| error.to_string())?;
    if let Some((key, expected)) = expected {
        let actual = client
            .execute(CommandInvocation::new(
                "show-options",
                ["-gqv", key.as_str()],
            ))
            .map_err(|error| error.to_string())?;
        if !mux_confirmation_matches(key, &expected, actual.trim()) {
            return Err(format!(
                "Saved {} = {}, but the daemon reports {}. Review the configuration and retry Reload.",
                key.as_str(),
                expected,
                actual.trim()
            ));
        }
    }
    Ok(output)
}

fn mux_confirmation_matches(key: MuxOptionKey, expected: &str, actual: &str) -> bool {
    if expected == actual {
        return true;
    }
    match key {
        MuxOptionKey::HistoryLimit | MuxOptionKey::EscapeTime => expected
            .parse::<u64>()
            .ok()
            .zip(actual.parse::<u64>().ok())
            .is_some_and(|(expected, actual)| expected == actual),
        MuxOptionKey::Prefix => zz_mux::parse_tmux_key(expected)
            .zip(zz_mux::parse_tmux_key(actual))
            .is_some_and(|(expected, actual)| expected == actual),
        MuxOptionKey::Mouse => {
            matches!(expected, "true" | "yes" | "1") && actual == "on"
                || matches!(expected, "false" | "no" | "0") && actual == "off"
        }
        _ => false,
    }
}

#[cfg(target_os = "ios")]
fn execute_file_command(
    _: CommandInvocation,
    _: Option<(MuxOptionKey, String)>,
    _: Option<u64>,
) -> Result<String, String> {
    Err("Connect with the desktop client to import local multiplexer configuration.".to_owned())
}

fn select_value(key: FileKey, value: &str) -> String {
    if key.name() == "cursor-style-blink" {
        match value {
            "true" | "yes" | "1" => "on".to_owned(),
            "false" | "no" | "0" => "off".to_owned(),
            _ => value.to_owned(),
        }
    } else {
        value.to_owned()
    }
}

fn display_value(key: FileKey, value: &str) -> String {
    if key.name() == "font-family"
        && let Some(unquoted) = value
            .strip_prefix('"')
            .and_then(|value| value.strip_suffix('"'))
    {
        return unquoted.to_owned();
    }
    if matches!(key.name(), "window-padding-x" | "window-padding-y")
        && let Some((first, second)) = value.split_once(',')
        && first.trim() == second.trim()
    {
        return first.trim().to_owned();
    }
    if key.name() == "background-opacity" {
        value
            .parse::<f32>()
            .map_or_else(|_| value.to_owned(), |value| (value * 100.0).to_string())
    } else {
        value.to_owned()
    }
}

fn option_choices(key: FileKey, scheme: TerminalColorScheme) -> Vec<(String, String)> {
    let choices = match key.name() {
        "mode-keys" => vec!["vi", "emacs"],
        "set-clipboard" => vec!["on", "external", "off"],
        "cursor-style" => vec!["block", "bar", "underline", "block_hollow"],
        "cursor-style-blink" => vec!["on", "off", "terminal"],
        "theme" => {
            let mut choices = vec![("None".to_owned(), String::new())];
            choices.extend(
                zz_terminal::enumerate_ghostty_themes_for(scheme)
                    .into_iter()
                    .map(|theme| (theme.name.clone(), theme.name)),
            );
            return choices;
        }
        _ => Vec::new(),
    };
    choices
        .into_iter()
        .map(|value| (value.to_owned(), value.to_owned()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[gpui::test]
    fn terminal_preview_uses_drafts_without_changing_saved_settings(cx: &mut gpui::TestAppContext) {
        use gpui::EntityInputHandler as _;
        use std::{cell::RefCell, rc::Rc};

        cx.update(zz_ui::init);
        let captured = Rc::new(RefCell::new(None));
        let captured_for_window = Rc::clone(&captured);
        let (_, cx) = cx.add_window_view(move |window, cx| {
            let mux = cx.new(|cx| {
                MuxClient::new(
                    Err(zz_daemon::DaemonError::Thread("test client".to_owned())),
                    zz_daemon::default_socket_path(),
                    cx,
                )
            });
            captured_for_window.replace(Some(cx.entity()));
            SettingsView::new(mux, window, cx)
        });
        let settings = captured.borrow().clone().unwrap();
        let preview = cx.update(|window, cx| {
            settings.update(cx, |settings, cx| {
                settings.set_section(SettingsSection::Terminal, window, cx);
                let saved = "font-size = 12\nbackground-opacity = 1\n";
                let file = settings.config_file_editor_mut(ConfigFileKind::Terminal);
                file.saved = saved.to_owned();
                file.editor
                    .update(cx, |editor, cx| editor.set_value(saved, window, cx));
                settings.synchronize_file_options(window, cx);
                let rows = &settings.file_options[&ConfigFileKind::Terminal].rows;
                for (key, draft) in [
                    ("font-family", "JetBrains Mono"),
                    ("font-size", "18"),
                    ("background-opacity", "65"),
                ] {
                    rows.iter()
                        .find(|row| row.key.name() == key)
                        .unwrap()
                        .input
                        .update(cx, |input, cx| {
                            input.set_value("", window, cx);
                            input.replace_text_in_range(None, draft, window, cx);
                            assert_eq!(input.value().as_ref(), draft);
                        });
                }
                let source = settings.terminal_preview_source(cx);
                assert_eq!(
                    config::file_options::appearance_value(
                        &source,
                        AppearanceConfigKey::FontFamily
                    )
                    .as_deref(),
                    Some("JetBrains Mono")
                );
                assert_eq!(
                    config::file_options::appearance_value(&source, AppearanceConfigKey::FontSize)
                        .as_deref(),
                    Some("18")
                );
                assert_eq!(
                    config::file_options::appearance_value(
                        &source,
                        AppearanceConfigKey::BackgroundOpacity,
                    )
                    .as_deref(),
                    Some("0.65")
                );
                assert_eq!(
                    settings.config_file_editor(ConfigFileKind::Terminal).saved,
                    saved
                );
                let editor_draft = "font-size = 22\n";
                settings
                    .config_file_editor(ConfigFileKind::Terminal)
                    .editor
                    .update(cx, |editor, cx| editor.set_value(editor_draft, window, cx));
                assert_eq!(settings.terminal_preview_source(cx), editor_draft);
                settings.synchronize_terminal_preview(cx);
                let preview = settings.terminal_preview.as_ref().unwrap().downgrade();
                settings.set_section(SettingsSection::Appearance, window, cx);
                assert!(settings.terminal_preview.is_none());
                preview
            })
        });
        assert!(preview.upgrade().is_none());
    }

    #[test]
    fn mux_confirmation_accepts_daemon_normalized_values() {
        assert!(mux_confirmation_matches(
            MuxOptionKey::EscapeTime,
            "0010",
            "10"
        ));
        assert!(mux_confirmation_matches(MuxOptionKey::Mouse, "true", "on"));
        assert!(!mux_confirmation_matches(
            MuxOptionKey::HistoryLimit,
            "10",
            "20"
        ));
        assert!(!mux_confirmation_matches(
            MuxOptionKey::EscapeTime,
            "invalid",
            "10"
        ));
    }
}
