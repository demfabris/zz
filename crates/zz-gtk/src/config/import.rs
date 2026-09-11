use std::{
    io::{self, ErrorKind},
    path::{Path, PathBuf},
};

use zz_terminal::{
    AppearanceConfigKey, AppearanceLoad, AppearanceSource, TerminalAppearance, TerminalColorScheme,
    discover_ghostty_config, load_ghostty_appearance_from_for,
};

use crate::config::{file, schema};

pub const MAX_MUX_CONFIG_BYTES: usize = 1024 * 1024;

const MARKER_FILE_NAME: &str = "import-prompted";

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Report {
    pub ghostty_keys: usize,
    pub config_path: Option<PathBuf>,
}

impl Report {
    #[must_use]
    pub const fn imported_anything(&self) -> bool {
        self.config_path.is_some()
    }
}

#[must_use]
pub fn donors_present() -> bool {
    discover_ghostty_config().is_some()
}

#[must_use]
pub fn prompt_pending() -> bool {
    if marker_path().as_deref().is_some_and(Path::exists)
        || legacy_marker_path().as_deref().is_some_and(Path::exists)
    {
        return false;
    }
    if donors_present() {
        return true;
    }
    mark_prompted();
    false
}

pub fn mark_prompted() {
    let Some(path) = marker_path() else {
        return;
    };
    if let Err(error) = file::atomic_write(&path, b"") {
        log::warn!(
            target: "zz_gtk::config",
            "could not persist the import prompt marker path={} error={error}",
            path.display(),
        );
    }
}

fn marker_path() -> Option<PathBuf> {
    zz_daemon::user_data::platform_data_dir().map(|data| data.join("zz").join(MARKER_FILE_NAME))
}

fn legacy_marker_path() -> Option<PathBuf> {
    file::candidates()
        .into_iter()
        .next()
        .and_then(|config| config.parent().map(|parent| parent.join(MARKER_FILE_NAME)))
}

pub fn run(scheme: TerminalColorScheme) -> io::Result<Report> {
    let Some(donor) = discover_ghostty_config() else {
        return Ok(Report::default());
    };
    let load = load_ghostty_appearance_from_for(&donor, scheme);
    let groups = appearance_groups(&load)?;
    if groups.is_empty() {
        return Ok(Report::default());
    }
    let target = file::path_for_write()?;
    write_import(&target, &groups)?;
    Ok(Report {
        ghostty_keys: groups.len(),
        config_path: Some(target),
    })
}

fn write_import(path: &Path, groups: &[(AppearanceConfigKey, Vec<String>)]) -> io::Result<()> {
    let source = file::read_editor_source(path, file::MAX_CONFIG_BYTES)?;
    let edited = apply_import(&source, groups)?;
    file::atomic_write(path, edited.as_bytes())
}

fn apply_import(source: &str, groups: &[(AppearanceConfigKey, Vec<String>)]) -> io::Result<String> {
    let mut edited = source.to_owned();
    for (key, values) in groups {
        edited = if !is_cumulative(*key)
            && let [value] = values.as_slice()
        {
            file::edit_source(&edited, key.as_str(), Some(value))
        } else {
            file::replace_key_group(&edited, key.as_str(), values)
        };
        if edited.len() > file::MAX_CONFIG_BYTES {
            return Err(io::Error::new(
                ErrorKind::InvalidData,
                format!(
                    "importing would push the configuration past its {}-byte limit",
                    file::MAX_CONFIG_BYTES
                ),
            ));
        }
    }
    Ok(edited)
}

fn appearance_groups(load: &AppearanceLoad) -> io::Result<Vec<(AppearanceConfigKey, Vec<String>)>> {
    let mut groups = Vec::new();
    for key in AppearanceConfigKey::ALL {
        if !matches!(
            load.provenance.source(key),
            AppearanceSource::Ghostty | AppearanceSource::ThemeFile
        ) {
            continue;
        }
        let values = appearance_values(&load.appearance, key)?;
        if !values.is_empty() || is_cumulative(key) {
            groups.push((key, values));
        }
    }
    Ok(groups)
}

fn is_cumulative(key: AppearanceConfigKey) -> bool {
    matches!(
        key,
        AppearanceConfigKey::Palette
            | AppearanceConfigKey::FontFamily
            | AppearanceConfigKey::FontFamilyBold
            | AppearanceConfigKey::FontFamilyItalic
            | AppearanceConfigKey::FontFamilyBoldItalic
            | AppearanceConfigKey::FontFeature
    )
}

fn appearance_values(
    appearance: &TerminalAppearance,
    key: AppearanceConfigKey,
) -> io::Result<Vec<String>> {
    let families = match key {
        AppearanceConfigKey::FontFamily => Some(&appearance.font_families),
        AppearanceConfigKey::FontFamilyBold => Some(&appearance.font_families_bold),
        AppearanceConfigKey::FontFamilyItalic => Some(&appearance.font_families_italic),
        AppearanceConfigKey::FontFamilyBoldItalic => Some(&appearance.font_families_bold_italic),
        _ => None,
    };
    if let Some(families) = families {
        return Ok(families
            .iter()
            .map(|family| format!("\"{family}\""))
            .collect());
    }
    Ok(match key {
        AppearanceConfigKey::Palette => {
            let defaults = TerminalAppearance::default();
            appearance
                .palette
                .as_array()
                .iter()
                .zip(defaults.palette.as_array())
                .enumerate()
                .filter(|(_, (color, default))| color != default)
                .map(|(index, (color, _))| {
                    format!("{index}=#{:02X}{:02X}{:02X}", color.r, color.g, color.b)
                })
                .collect()
        }
        AppearanceConfigKey::FontFeature => {
            let mut tags = Vec::new();
            let mut values = Vec::new();
            for feature in &appearance.font_features {
                if tags.contains(&feature.tag) {
                    return Err(io::Error::new(
                        ErrorKind::InvalidData,
                        format!(
                            "resolved appearance contains duplicate `{}` feature tags",
                            feature.tag_string()
                        ),
                    ));
                }
                tags.push(feature.tag);
                values.push(format!("{}={}", feature.tag_string(), feature.value));
            }
            values
        }
        AppearanceConfigKey::FontSyntheticStyle => {
            let styles = appearance.font_synthetic_style;
            vec![if styles.bold && styles.italic && styles.bold_italic {
                "true".to_owned()
            } else if !styles.bold && !styles.italic && !styles.bold_italic {
                "false".to_owned()
            } else {
                [
                    ("bold", styles.bold),
                    ("italic", styles.italic),
                    ("bold-italic", styles.bold_italic),
                ]
                .into_iter()
                .map(|(style, enabled)| {
                    if enabled {
                        style.to_owned()
                    } else {
                        format!("no-{style}")
                    }
                })
                .collect::<Vec<_>>()
                .join(",")
            }]
        }
        AppearanceConfigKey::FontThicken => vec![appearance.font_thicken.to_string()],
        AppearanceConfigKey::FontThickenStrength => {
            vec![appearance.font_thicken_strength.to_string()]
        }
        _ => schema::appearance_display(appearance, key)
            .into_iter()
            .collect(),
    })
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        sync::atomic::{AtomicU64, Ordering},
    };

    use super::*;

    static NEXT_SCRATCH: AtomicU64 = AtomicU64::new(0);

    struct Scratch(PathBuf);

    impl Scratch {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "zz-gtk-import-{}-{}",
                std::process::id(),
                NEXT_SCRATCH.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn write(&self, name: &str, contents: &str) -> PathBuf {
            let path = self.0.join(name);
            file::atomic_write(&path, contents.as_bytes()).unwrap();
            path
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }

    #[test]
    fn imports_resolved_theme_and_includes_without_touching_mux_or_donors() {
        let scratch = Scratch::new();
        let theme = scratch.write("theme", "background = #123456\nforeground = #abcdef\n");
        let child = scratch.write(
            "child",
            "font-family = First Mono\nfont-family = Second Mono\nfont-size = 17\n",
        );
        let donor_text = format!(
            "theme = {}\nconfig-file = child\nkeybind = ctrl+a=new_split\n",
            theme.display()
        );
        let donor = scratch.write("ghostty", &donor_text);
        let target = scratch.write(
            "zz/config",
            "# mine\npane-margin = 8\nfont-size = 12 # size\n",
        );
        let mux = scratch.write("zz/mux.conf", "set -g status off\n");
        let groups = appearance_groups(&load_ghostty_appearance_from_for(
            &donor,
            TerminalColorScheme::Dark,
        ))
        .unwrap();
        write_import(&target, &groups).unwrap();
        let imported = fs::read_to_string(&target).unwrap();
        assert!(imported.starts_with("# mine\npane-margin = 8\nfont-size = 17 # size\n"));
        assert!(imported.contains("background = #123456\n"));
        assert!(imported.contains("foreground = #ABCDEF\n"));
        assert!(imported.contains("font-family = \"First Mono\"\nfont-family = \"Second Mono\"\n"));
        assert!(!imported.contains("theme ="));
        assert!(!imported.contains("config-file ="));
        assert!(!imported.contains("keybind ="));
        assert_eq!(fs::read_to_string(&mux).unwrap(), "set -g status off\n");
        assert_eq!(fs::read_to_string(&donor).unwrap(), donor_text);
        assert!(
            fs::read_to_string(&child)
                .unwrap()
                .contains("font-size = 17")
        );
    }

    #[test]
    fn empty_cumulative_import_clears_existing_groups_and_keeps_other_bytes() {
        let scratch = Scratch::new();
        let donor = scratch.write("ghostty", "font-family-bold = \"\"\nfont-feature = \"\"\n");
        let groups = appearance_groups(&load_ghostty_appearance_from_for(
            &donor,
            TerminalColorScheme::Dark,
        ))
        .unwrap();
        let edited = apply_import(
            "# mine\r\nfont-family-bold = Old\r\nfont-family-bold = Older\r\nfont-feature = liga\r\npane-margin = 8\r\n",
            &groups,
        )
        .unwrap();
        assert_eq!(edited, "# mine\r\npane-margin = 8\r\n");
    }

    #[test]
    fn selected_color_scheme_is_resolved_before_import() {
        let scratch = Scratch::new();
        let light = scratch.write("light", "background = #ffffff\n");
        let dark = scratch.write("dark", "background = #000000\n");
        let donor = scratch.write(
            "ghostty",
            &format!(
                "theme = light:{},dark:{}\n",
                light.display(),
                dark.display()
            ),
        );
        for (scheme, expected) in [
            (TerminalColorScheme::Light, "#FFFFFF"),
            (TerminalColorScheme::Dark, "#000000"),
        ] {
            let groups =
                appearance_groups(&load_ghostty_appearance_from_for(&donor, scheme)).unwrap();
            assert!(groups.contains(&(AppearanceConfigKey::Background, vec![expected.to_owned()])));
        }
    }

    #[test]
    fn oversized_import_leaves_existing_file_unchanged() {
        let scratch = Scratch::new();
        let source = format!("#{}\n", "x".repeat(file::MAX_CONFIG_BYTES - 2));
        let target = scratch.write("config", &source);
        let error = write_import(
            &target,
            &[(AppearanceConfigKey::FontSize, vec!["17".to_owned()])],
        )
        .unwrap_err();
        assert_eq!(error.kind(), ErrorKind::InvalidData);
        assert_eq!(fs::read_to_string(target).unwrap(), source);
    }
}
