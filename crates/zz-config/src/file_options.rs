use std::{io, ops::Range, path::Path};

use zz_protocol::{
    MuxOptionKey, TmuxOption, canonical_command, command_spec, parse_tmux_command_options,
};
use zz_terminal::AppearanceConfigKey;

fn mux_option_lines(source: &str, key: MuxOptionKey) -> Vec<(Range<usize>, String)> {
    let mut offset = 0;
    source
        .split_inclusive('\n')
        .filter_map(|line| {
            let range = offset..offset + line.len();
            offset += line.len();
            let parsed =
                zz_mux::MuxEngine::parse_config_without_variable_expansion("settings", line);
            let [command] = parsed.commands.as_slice() else {
                return None;
            };
            if !parsed.diagnostics.is_empty()
                || !matches!(
                    canonical_command(&command.name),
                    "set-option" | "set-window-option"
                )
            {
                return None;
            }
            let options = parse_tmux_command_options(command_spec(&command.name)?, command).ok()?;
            if !options
                .options
                .iter()
                .any(|option| matches!(option, TmuxOption::Flag("-g" | "-s")))
                || options
                    .options
                    .iter()
                    .any(|option| !matches!(option, TmuxOption::Flag("-g" | "-q" | "-s" | "-w")))
            {
                return None;
            }
            let [name, value] = options.positionals else {
                return None;
            };
            (name.as_str() == key.as_str()).then(|| (range, value.to_string()))
        })
        .collect()
}

pub fn mux_option_value(source: &str, key: MuxOptionKey) -> Option<String> {
    mux_option_lines(source, key).pop().map(|(_, value)| value)
}

pub fn edit_mux_option(source: &str, key: MuxOptionKey, value: Option<&str>) -> io::Result<String> {
    if let Some(value) = value {
        validate_value(value)?;
    }
    let last = mux_option_lines(source, key).pop();
    let mut edited = source.to_owned();
    match (last, value) {
        (Some((range, _)), Some(value)) => {
            let line = &source[range.clone()];
            let ending = if line.ends_with("\r\n") {
                "\r\n"
            } else if line.ends_with('\n') {
                "\n"
            } else {
                ""
            };
            let command = zz_mux::format_command(&zz_protocol::CommandInvocation::new(
                "set-option",
                ["-g", key.as_str(), value],
            ));
            edited.replace_range(range, &format!("{command}{ending}"));
        }
        (Some((range, _)), None) => {
            edited.replace_range(range, "");
        }
        (None, Some(value)) => {
            if !edited.is_empty() && !edited.ends_with('\n') {
                edited.push('\n');
            }
            edited.push_str(&zz_mux::format_command(
                &zz_protocol::CommandInvocation::new("set-option", ["-g", key.as_str(), value]),
            ));
            edited.push('\n');
        }
        (None, None) => {}
    }
    Ok(edited)
}

pub fn appearance_value(source: &str, key: AppearanceConfigKey) -> Option<String> {
    crate::parse_config(source, "")
        .daemon_entries
        .into_iter()
        .rev()
        .find(|entry| entry.0 == key.as_str())
        .map(|entry| entry.1)
}

fn appearance_option_edits(
    key: AppearanceConfigKey,
    value: Option<&str>,
) -> Vec<(AppearanceConfigKey, Vec<String>)> {
    let mut edits = Vec::new();
    if key == AppearanceConfigKey::Theme && value.is_some() {
        for color in [
            AppearanceConfigKey::Background,
            AppearanceConfigKey::Foreground,
            AppearanceConfigKey::CursorColor,
            AppearanceConfigKey::SelectionForeground,
            AppearanceConfigKey::SelectionBackground,
            AppearanceConfigKey::Palette,
        ] {
            edits.push((color, Vec::new()));
        }
    }
    edits.push((key, value.into_iter().map(str::to_owned).collect()));
    edits
}

pub fn edit_appearance_option(
    source: &str,
    key: AppearanceConfigKey,
    value: Option<&str>,
) -> io::Result<String> {
    if let Some(value) = value {
        validate_value(value)?;
    }
    if matches!(
        key,
        AppearanceConfigKey::FontFamily | AppearanceConfigKey::Theme
    ) {
        crate::apply_import_edits(source, &appearance_option_edits(key, value))
    } else {
        Ok(crate::edit_config_source(source, key.as_str(), value))
    }
}

pub fn write_appearance_option(
    path: &Path,
    key: AppearanceConfigKey,
    value: Option<&str>,
) -> io::Result<()> {
    if let Some(value) = value {
        validate_value(value)?;
    }
    if matches!(
        key,
        AppearanceConfigKey::FontFamily | AppearanceConfigKey::Theme
    ) {
        crate::import_appearance_values_at(path, &appearance_option_edits(key, value))
    } else {
        crate::write_config_edit_at(path, key.as_str(), value).map(drop)
    }
}

pub fn validate_value(value: &str) -> io::Result<()> {
    if value.trim().is_empty() || value.contains(['\r', '\n']) {
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "enter a non-empty value on one line",
        ))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mux_options_read_each_global_spelling_and_ignore_append() {
        for spelling in [
            "set -g",
            "set -s",
            "set -gq",
            "set -q -g",
            "set-option -g",
            "set-window-option -g",
        ] {
            let source = format!("{spelling} prefix C-a\nset -ag prefix C-x\nset prefix C-y\n");
            assert_eq!(
                mux_option_value(&source, MuxOptionKey::Prefix).as_deref(),
                Some("C-a")
            );
        }
    }

    #[test]
    fn mux_options_replace_last_append_and_reset() {
        let source = "# user\r\nset -g prefix C-a\r\nset-option -g prefix C-x\r\n# tail\r\n";
        let edited = edit_mux_option(source, MuxOptionKey::Prefix, Some("C-z")).unwrap();
        assert_eq!(edited, source.replace("prefix C-x", "prefix C-z"));
        let reset = edit_mux_option(source, MuxOptionKey::Prefix, None).unwrap();
        assert_eq!(reset, "# user\r\nset -g prefix C-a\r\n# tail\r\n");
        assert_eq!(
            mux_option_value(&reset, MuxOptionKey::Prefix).as_deref(),
            Some("C-a")
        );
        assert_eq!(
            edit_mux_option("# tail", MuxOptionKey::Mouse, Some("off")).unwrap(),
            "# tail\nset-option -g mouse off\n"
        );
    }

    #[test]
    fn appearance_options_replace_last_and_reset_preserving_other_text() {
        let source = "font-size = 10\nfont-size = 12 # mine\n# tail\n";
        assert_eq!(
            appearance_value(source, AppearanceConfigKey::FontSize).as_deref(),
            Some("12")
        );
        assert_eq!(
            edit_appearance_option(source, AppearanceConfigKey::FontSize, Some("14")).unwrap(),
            source.replace("12 #", "14 #")
        );
        assert_eq!(
            edit_appearance_option(source, AppearanceConfigKey::FontSize, None).unwrap(),
            "font-size = 10\n# tail\n"
        );
    }

    #[test]
    fn appearance_font_family_replaces_the_whole_stack_with_one_line() {
        let source = "font-family = First\n# retained\nfont-family = Second\n";
        assert_eq!(
            edit_appearance_option(source, AppearanceConfigKey::FontFamily, Some("New Font"))
                .unwrap(),
            "# retained\nfont-family = New Font\n"
        );
        assert_eq!(
            edit_appearance_option(source, AppearanceConfigKey::FontFamily, None).unwrap(),
            "# retained\n"
        );
    }

    #[test]
    fn selecting_a_theme_replaces_imported_colors_and_preserves_other_settings() {
        use zz_terminal::{AppearanceLoad, TerminalColorScheme, apply_appearance_overrides};

        let directory = tempfile::tempdir().unwrap();
        let theme_path = directory.path().join("preview-theme");
        std::fs::write(
            &theme_path,
            "background = #102030\nforeground = #d0e0f0\ncursor-color = #aabbcc\n\
             selection-background = #405060\nselection-foreground = #708090\npalette = 1=#cc1122\n",
        )
        .unwrap();
        let theme = theme_path.to_str().unwrap();
        let preserved = "# keep this\nfont-family = Menlo\nfont-size = 18\nwindow-padding-x = 8\n\
                         background-opacity = 0.7\ncursor-style = bar\npane-margin = 10\n";
        let source = format!(
            "{preserved}theme = {theme}\nbackground = #010101\nbackground = #020202\n\
             foreground = #030303\ncursor-color = #040404\nselection-background = #050505\n\
             selection-foreground = #060606\npalette = 1=#070707\npalette = 1=#080808\n"
        );
        let path = directory.path().join("config");
        std::fs::write(&path, &source).unwrap();
        write_appearance_option(&path, AppearanceConfigKey::Theme, Some(theme)).unwrap();
        let edited = std::fs::read_to_string(&path).unwrap();
        assert_eq!(edited, format!("{preserved}theme = {theme}\n"));
        assert_eq!(
            edited,
            edit_appearance_option(&source, AppearanceConfigKey::Theme, Some(theme)).unwrap()
        );
        let parsed = crate::parse_config(&edited, "");
        let actual = apply_appearance_overrides(
            AppearanceLoad::defaults_for(TerminalColorScheme::Dark),
            &parsed.daemon_entries,
        );
        let expected = apply_appearance_overrides(
            AppearanceLoad::defaults_for(TerminalColorScheme::Dark),
            &[("theme".to_owned(), theme.to_owned())],
        );
        assert!(!actual.fatal);
        assert_eq!(actual.invalid, 0);
        assert_eq!(actual.appearance.background, expected.appearance.background);
        assert_eq!(actual.appearance.foreground, expected.appearance.foreground);
        assert_eq!(actual.appearance.palette, expected.appearance.palette);
        assert_eq!(
            actual.appearance.cursor_color,
            expected.appearance.cursor_color
        );
        assert_eq!(
            actual.appearance.selection_background,
            expected.appearance.selection_background
        );
        assert_eq!(
            actual.appearance.selection_foreground,
            expected.appearance.selection_foreground
        );
        assert_eq!(actual.appearance.font_size_points, 18.0);
        assert_eq!(actual.appearance.padding_left, 8.0);
        assert_eq!(actual.appearance.background_opacity, 0.7);
    }

    #[test]
    fn resetting_theme_keeps_custom_colors() {
        let source = "theme = Old\nbackground = #123456\npalette = 1=#abcdef\n";
        assert_eq!(
            edit_appearance_option(source, AppearanceConfigKey::Theme, None).unwrap(),
            "background = #123456\npalette = 1=#abcdef\n"
        );
    }
}
