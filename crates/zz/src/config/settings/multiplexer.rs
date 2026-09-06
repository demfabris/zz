use super::*;
use crate::config::mux_bindings::{
    SplitDirection, SplitPaneKind, preferred_split_binding, split_binding_kind,
    update_split_binding,
};
use zz_protocol::KeyBindingSnapshot;

#[cfg(test)]
mod tests;

const DIRECTIONS: [SplitDirection; 2] = [SplitDirection::Vertical, SplitDirection::Horizontal];
const PANE_KINDS: [(SplitPaneKind, &str); 3] = [
    (SplitPaneKind::Picker, "Pane picker"),
    (SplitPaneKind::Terminal, "Terminal"),
    (SplitPaneKind::Browser, "Browser"),
];

struct SplitControl {
    binding: Option<KeyBindingSnapshot>,
    preferred_key: Option<String>,
    key: Entity<InputState>,
}

pub(super) struct SplitControls {
    rows: [SplitControl; 2],
    pending: Option<PendingSplit>,
    timeout: Option<gpui::Task<()>>,
    _subscriptions: Vec<Subscription>,
}

struct PendingSplit {
    key: String,
    kind: SplitPaneKind,
    removed_key: Option<String>,
    timed_out: bool,
}

impl SettingsView {
    pub(super) fn synchronize_mux_splits(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.section != SettingsSection::Multiplexer {
            return;
        }
        if self.mux_split_controls.is_none() {
            let rows = DIRECTIONS.map(|direction| SplitControl {
                binding: None,
                preferred_key: None,
                key: text_value_input(default_key(direction), window, cx),
            });
            let subscriptions = rows
                .iter()
                .enumerate()
                .map(|(index, row)| {
                    cx.subscribe_in(&row.key, window, move |this, _, event, window, cx| {
                        if matches!(event, InputEvent::PressEnter { .. }) {
                            let kind = this.mux_split_controls.as_ref().and_then(|controls| {
                                controls.rows[index]
                                    .binding
                                    .as_ref()
                                    .map_or(Some(SplitPaneKind::Picker), split_binding_kind)
                            });
                            if let Some(kind) = kind {
                                this.commit_mux_split(index, kind, window, cx);
                            }
                        }
                    })
                })
                .collect();
            self.mux_split_controls = Some(SplitControls {
                rows,
                pending: None,
                timeout: None,
                _subscriptions: subscriptions,
            });
        }

        let bindings = self.mux.read(cx).prefix_bindings().to_vec();
        let controls = self.mux_split_controls.as_mut().unwrap();
        if controls.pending.as_ref().is_some_and(|pending| {
            bindings.iter().any(|binding| {
                binding.key == pending.key && split_binding_kind(binding) == Some(pending.kind)
            }) && pending
                .removed_key
                .as_ref()
                .is_none_or(|key| !bindings.iter().any(|binding| &binding.key == key))
        }) || !self.mux.read(cx).is_connected()
            || self.mux.read(cx).attached_host() != HostId::LOCAL
        {
            controls.pending = None;
            controls.timeout = None;
        }
        if controls.pending.is_some() {
            return;
        }
        for (row, direction) in controls.rows.iter_mut().zip(DIRECTIONS) {
            let binding = row
                .preferred_key
                .as_ref()
                .and_then(|key| bindings.iter().find(|binding| &binding.key == key))
                .filter(|binding| {
                    preferred_split_binding(std::slice::from_ref(*binding), direction).is_some()
                })
                .or_else(|| preferred_split_binding(&bindings, direction))
                .or_else(|| {
                    bindings
                        .iter()
                        .find(|binding| binding.key == default_key(direction))
                })
                .cloned();
            if row.binding != binding {
                let key = binding
                    .as_ref()
                    .map_or(default_key(direction), |binding| &binding.key);
                row.key
                    .update(cx, |input, cx| input.set_value(key, window, cx));
                row.binding = binding;
            }
        }
    }

    fn mux_split_disabled_reason(&self, cx: &App) -> Option<&'static str> {
        let mux = self.mux.read(cx);
        if !mux.is_connected() || mux.attached_host() != HostId::LOCAL {
            return Some("Connect to a local session to edit split shortcuts.");
        }
        if let Some(pending) = self
            .mux_split_controls
            .as_ref()
            .and_then(|controls| controls.pending.as_ref())
        {
            return Some(if pending.timed_out {
                "Saved. Reload configuration or reconnect to apply the shortcut."
            } else {
                "Applying split shortcut…"
            });
        }
        let file = self.config_file_editor(ConfigFileKind::Mux);
        if file.error.is_some() {
            return Some("Resolve the configuration error below to edit split shortcuts.");
        }
        if file.editor.read(cx).value().as_ref() != file.saved {
            return Some("Save your configuration edits below to edit split shortcuts.");
        }
        None
    }

    pub(super) fn mux_splits_section(&self, cx: &Context<Self>) -> AnyElement {
        let Some(controls) = &self.mux_split_controls else {
            return div().into_any_element();
        };
        let disabled_reason = self.mux_split_disabled_reason(cx);
        let mut stack =
            SettingsStack::titled("Split panes").description(disabled_reason.unwrap_or(
                "Choose what a split opens. Edit a shortcut and press Enter to apply it.",
            ));
        for (index, row) in controls.rows.iter().enumerate() {
            let kind = row.binding.as_ref().and_then(split_binding_kind);
            let custom = row.binding.is_some() && kind.is_none();
            let disabled = disabled_reason.is_some() || custom;
            let label = kind.map_or(if custom { "Custom" } else { "Unbound" }, |kind| {
                PANE_KINDS
                    .iter()
                    .find(|(value, _)| *value == kind)
                    .unwrap()
                    .1
            });
            let title = if index == 0 {
                "Split below"
            } else {
                "Split right"
            };
            let description = if custom {
                "This shortcut has a custom command. Edit it in the configuration below."
            } else if index == 0 {
                "Open a pane below the current pane."
            } else {
                "Open a pane to the right of the current pane."
            };
            let settings = cx.entity().downgrade();
            stack = stack.child(
                SettingEntry::new(title, description).control(
                    h_flex()
                        .gap(px(8.0))
                        .flex_none()
                        .child(
                            div()
                                .text_size(zz_ui::rems_from_px(11.0))
                                .text_color(cx.theme().foreground.muted())
                                .child("Prefix +"),
                        )
                        .child(
                            div().w(px(65.0)).child(
                                Input::new(&row.key)
                                    .small()
                                    .disabled(disabled)
                                    .bg(settings_control_fill(cx)),
                            ),
                        )
                        .child(
                            Button::new(("settings-split-kind", index))
                                .small()
                                .w(px(120.0))
                                .label(label)
                                .dropdown_caret(true)
                                .disabled(disabled)
                                .bg(settings_control_fill(cx))
                                .dropdown_menu_with_anchor(
                                    gpui::Anchor::TopRight,
                                    move |menu, _, _| {
                                        PANE_KINDS.into_iter().fold(
                                            menu,
                                            |menu, (choice, label)| {
                                                let settings = settings.clone();
                                                menu.item(
                                                    PopupMenuItem::new(label)
                                                        .checked(kind == Some(choice))
                                                        .on_click(move |_, window, cx| {
                                                            let _ =
                                                                settings.update(cx, |this, cx| {
                                                                    this.commit_mux_split(
                                                                        index, choice, window, cx,
                                                                    );
                                                                });
                                                        }),
                                                )
                                            },
                                        )
                                    },
                                ),
                        ),
                ),
            );
        }
        if controls
            .pending
            .as_ref()
            .is_some_and(|pending| pending.timed_out)
        {
            stack = stack.child(
                SettingEntry::new(
                    "Apply saved shortcuts",
                    "Retry loading the saved configuration.",
                )
                .control(
                    Button::new("settings-retry-split-reload")
                        .small()
                        .label("Reload")
                        .on_click(cx.listener(|this, _, _, cx| {
                            config::request_daemon_reload(cx);
                            this.wait_for_mux_split(cx);
                        })),
                ),
            );
        }
        div().flex_none().child(stack).into_any_element()
    }

    fn commit_mux_split(
        &mut self,
        index: usize,
        kind: SplitPaneKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(reason) = self.mux_split_disabled_reason(cx) {
            toast::push(Notification::info(reason), cx);
            return;
        }
        let controls = self.mux_split_controls.as_ref().unwrap();
        let row = &controls.rows[index];
        if row.binding.as_ref().is_some_and(|original| {
            self.mux
                .read(cx)
                .prefix_bindings()
                .iter()
                .find(|binding| binding.key == original.key)
                != Some(original)
        }) {
            self.synchronize_mux_splits(window, cx);
            toast::push(
                Notification::info("The shortcut changed. Review it and try again."),
                cx,
            );
            return;
        }
        let key = row.key.read(cx).value();
        let previous_key = row.binding.as_ref().map(|binding| binding.key.clone());
        if row.binding.as_ref().is_some_and(|binding| {
            split_binding_kind(binding) == Some(kind)
                && zz_mux::parse_tmux_key(key.trim()).as_deref() == Some(binding.key.as_str())
        }) {
            return;
        }
        let file = self.config_file_editor(ConfigFileKind::Mux);
        let source = match update_split_binding(
            &file.saved,
            self.mux.read(cx).prefix_bindings(),
            row.binding.as_ref(),
            DIRECTIONS[index],
            &key,
            kind,
        ) {
            Ok(source) => source,
            Err(error) => {
                toast::push(Notification::error(error), cx);
                return;
            }
        };
        if source == file.saved {
            return;
        }
        let path = file
            .path
            .clone()
            .map_or_else(|| config_file_path(ConfigFileKind::Mux), Ok);
        let current = path.and_then(|path| {
            config::read_config_editor_source(&path, ConfigFileKind::Mux.max_bytes())
        });
        match current {
            Ok(current) if current == file.saved => {}
            Ok(_) => {
                self.reload_config_editor(ConfigFileKind::Mux, window, cx);
                toast::push(
                    Notification::info("Configuration changed on disk. Review it and try again."),
                    cx,
                );
                return;
            }
            Err(error) => {
                toast::push(
                    Notification::error(format!(
                        "Could not read multiplexer configuration: {error}"
                    )),
                    cx,
                );
                return;
            }
        }
        let editor = file.editor.clone();
        editor.update(cx, |editor, cx| editor.set_value(&source, window, cx));
        self.save_config_editor(ConfigFileKind::Mux, cx);
        if self.config_file_editor(ConfigFileKind::Mux).saved == source {
            let key = zz_mux::parse_tmux_key(key.trim()).unwrap();
            let controls = self.mux_split_controls.as_mut().unwrap();
            controls.rows[index].preferred_key = Some(key.clone());
            controls.pending = Some(PendingSplit {
                removed_key: previous_key.filter(|previous| previous != &key),
                key,
                kind,
                timed_out: false,
            });
            self.wait_for_mux_split(cx);
        }
    }

    fn wait_for_mux_split(&mut self, cx: &mut Context<Self>) {
        let controls = self.mux_split_controls.as_mut().unwrap();
        if let Some(pending) = &mut controls.pending {
            pending.timed_out = false;
        }
        controls.timeout = Some(cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_secs(5))
                .await;
            let _ = this.update(cx, |this, cx| {
                if let Some(pending) = this
                    .mux_split_controls
                    .as_mut()
                    .and_then(|controls| controls.pending.as_mut())
                {
                    pending.timed_out = true;
                    cx.notify();
                }
            });
        }));
        cx.notify();
    }
}

fn default_key(direction: SplitDirection) -> &'static str {
    match direction {
        SplitDirection::Vertical => "\"",
        SplitDirection::Horizontal => "%",
    }
}
