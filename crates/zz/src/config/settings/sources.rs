use std::path::Path;

use super::*;

pub(super) struct MuxSources {
    tmux: Vec<PathBuf>,
    zz: Option<PathBuf>,
    copied: Option<(PathBuf, String)>,
}

impl SettingsView {
    pub(super) fn refresh_mux_sources(&mut self) {
        let tmux = zz_daemon::tmux_config_candidates()
            .into_iter()
            .filter(|path| path.is_file())
            .collect::<Vec<_>>();
        let copied = copied_root(&self.config_file_editor(ConfigFileKind::Mux).saved, &tmux);
        self.mux_sources = Some(MuxSources {
            tmux,
            zz: zz_daemon::default_mux_config(),
            copied,
        });
    }

    pub(super) fn reload_mux_configuration(&mut self, cx: &mut Context<Self>) {
        self.cancel_pending_mux_split();
        config::request_daemon_reload(cx);
        self.refresh_mux_sources();
        cx.notify();
    }

    pub(super) fn mux_sources_section(&self) -> AnyElement {
        let Some(sources) = &self.mux_sources else {
            return div().into_any_element();
        };
        let description = if sources.tmux.is_empty() {
            "No tmux configuration was found, so tmux defaults apply."
        } else {
            "Loaded in this order."
        };
        let mut stack = SettingsStack::titled("Configuration files").description(description);
        for path in &sources.tmux {
            stack = stack.child(SettingEntry::new(
                abbreviate_home(path),
                "Your tmux configuration, read as-is and never modified.",
            ));
        }
        if let Some(path) = &sources.zz {
            stack = stack.child(SettingEntry::new(
                abbreviate_home(path),
                "zz-only settings, loaded after tmux.",
            ));
        }
        stack.into_any_element()
    }

    pub(super) fn mux_copied_notice(&self, cx: &Context<Self>) -> Option<AnyElement> {
        let (root, contents) = self.mux_sources.as_ref()?.copied.as_ref()?;
        let editor = &self.config_file_editor(ConfigFileKind::Mux).editor;
        if !editor
            .read(cx)
            .value()
            .as_ref()
            .starts_with(contents.as_str())
        {
            return None;
        }
        Some(
            h_flex()
                .flex_none()
                .gap(px(10.0))
                .text_size(zz_ui::rems_from_px(11.0))
                .text_color(cx.theme().foreground.muted())
                .child(div().flex_1().min_w_0().child(format!(
                    "This file starts with a copy of {} from an earlier import. zz already reads \
                     the original, so only zz-specific lines need to stay here.",
                    abbreviate_home(root)
                )))
                .child(
                    Button::new("settings-trim-mux-copy")
                        .small()
                        .label("Remove copied lines")
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.trim_copied_mux_prefix(window, cx);
                        })),
                )
                .into_any_element(),
        )
    }

    fn trim_copied_mux_prefix(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((_, contents)) = self.mux_sources.as_ref().and_then(|s| s.copied.as_ref()) else {
            return;
        };
        let editor = self.config_file_editor(ConfigFileKind::Mux).editor.clone();
        let Some(remainder) = copy_remainder(editor.read(cx).value().as_ref(), contents) else {
            return;
        };
        editor.update(cx, |editor, cx| editor.set_value(&remainder, window, cx));
        cx.notify();
    }
}

fn copied_root(saved: &str, roots: &[PathBuf]) -> Option<(PathBuf, String)> {
    roots.iter().find_map(|root| {
        let contents =
            config::read_config_editor_source(root, ConfigFileKind::Mux.max_bytes()).ok()?;
        (!contents.trim().is_empty() && saved.starts_with(&contents))
            .then(|| (root.clone(), contents))
    })
}

fn copy_remainder(value: &str, contents: &str) -> Option<String> {
    value
        .strip_prefix(contents)
        .map(|rest| rest.trim_start_matches('\n').to_owned())
}

fn abbreviate_home(path: &Path) -> String {
    std::env::var_os("HOME")
        .and_then(|home| {
            path.strip_prefix(&home)
                .ok()
                .map(|rest| format!("~/{}", rest.display()))
        })
        .unwrap_or_else(|| path.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copied_root_flags_a_saved_file_that_begins_with_a_tmux_root() {
        let directory = tempfile::Builder::new()
            .prefix("zz-sources-")
            .tempdir_in("/tmp")
            .unwrap();
        let root = directory.path().join("tmux.conf");
        let empty = directory.path().join("empty.conf");
        std::fs::write(&root, "set -g prefix C-a\nbind a last-window\n").unwrap();
        std::fs::write(&empty, "\n\n").unwrap();
        let roots = vec![
            empty.clone(),
            root.clone(),
            directory.path().join("missing"),
        ];

        let saved = "set -g prefix C-a\nbind a last-window\n\nbind - { split-picker -v }\n";
        let copied = copied_root(saved, &roots).unwrap();
        assert_eq!(copied.0, root);
        assert_eq!(
            copy_remainder(saved, &copied.1).as_deref(),
            Some("bind - { split-picker -v }\n")
        );

        assert!(copied_root("set -g prefix C-b\n", &roots).is_none());
        assert!(copied_root("", &roots).is_none());
        assert_eq!(copy_remainder("unrelated", &copied.1), None);
    }
}
