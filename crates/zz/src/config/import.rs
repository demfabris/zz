//! One-shot import of Ghostty and tmux configuration into zz-owned files.

use std::{
    io::{self, ErrorKind, Read as _},
    path::{Path, PathBuf},
};

use zz_terminal::{
    AppearanceConfigKey, AppearanceSource, TerminalColorScheme, discover_ghostty_config,
    load_ghostty_appearance_from_for,
};

use crate::config;

/// Cap on the verbatim tmux copy, matching the Ghostty loader's own read bound.
pub(crate) const MAX_MUX_CONFIG_BYTES: usize = 1024 * 1024;

#[derive(Debug, Default)]
pub(crate) struct ImportReport {
    /// Number of appearance keys copied from the Ghostty config into `zz/config`.
    pub ghostty_keys: usize,
    /// Where the appearance keys were written, when any were.
    pub config_path: Option<PathBuf>,
    /// Where the tmux configuration was copied, when a donor existed.
    pub mux_path: Option<PathBuf>,
}

impl ImportReport {
    #[cfg(not(target_os = "ios"))]
    pub(crate) fn imported_anything(&self) -> bool {
        self.config_path.is_some() || self.mux_path.is_some()
    }
}

/// Whether a Ghostty or tmux config exists to import.
#[cfg(not(target_os = "ios"))]
pub(crate) fn donors_present() -> bool {
    discover_ghostty_config().is_some() || discover_tmux_config().is_some()
}

/// The tmux config the import copies, in tmux's own precedence order.
pub(crate) fn discover_tmux_config() -> Option<PathBuf> {
    let home = nonempty_env("HOME");
    let xdg = nonempty_env("XDG_CONFIG_HOME");
    tmux_config_candidates(home.as_deref(), xdg.as_deref())
        .into_iter()
        .find(|path| path.is_file())
}

fn tmux_config_candidates(home: Option<&Path>, xdg: Option<&Path>) -> Vec<PathBuf> {
    let mut candidates = Vec::with_capacity(3);
    if let Some(home) = home {
        candidates.push(home.join(".tmux.conf"));
    }
    if let Some(xdg) = xdg {
        candidates.push(xdg.join("tmux/tmux.conf"));
    }
    if let Some(home) = home {
        let fallback = home.join(".config/tmux/tmux.conf");
        if !candidates.contains(&fallback) {
            candidates.push(fallback);
        }
    }
    candidates
}

fn nonempty_env(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

/// Run the import: Ghostty appearance into `zz/config`, tmux config into
/// `zz/mux.conf`. Donors are read, never modified, and re-running lets donor
/// values win. A Ghostty `theme = …` flattens to concrete colors for `scheme`.
#[cfg(not(target_os = "ios"))]
pub(crate) fn import_external_config(scheme: TerminalColorScheme) -> io::Result<ImportReport> {
    let mut report = ImportReport::default();
    import_ghostty_config_into(scheme, &mut report)?;
    import_tmux_config_into(&mut report)?;
    Ok(report)
}

/// Import only the Ghostty appearance into `zz/config`, leaving `zz/mux.conf`
/// alone. Re-running overwrites the keys a previous import wrote.
pub(crate) fn import_ghostty_config(scheme: TerminalColorScheme) -> io::Result<ImportReport> {
    let mut report = ImportReport::default();
    import_ghostty_config_into(scheme, &mut report)?;
    Ok(report)
}

fn import_ghostty_config_into(
    scheme: TerminalColorScheme,
    report: &mut ImportReport,
) -> io::Result<()> {
    if let Some(ghostty) = discover_ghostty_config() {
        let load = load_ghostty_appearance_from_for(&ghostty, scheme);
        let values = ghostty_import_values(&load)?;
        if !values.is_empty() {
            let target = config::import_target_path()?;
            config::import_appearance_values_at(&target, &values)?;
            report.ghostty_keys = values.len();
            report.config_path = Some(target);
        }
    }
    Ok(())
}

/// Import only the tmux donor into `zz/mux.conf`.
pub(crate) fn import_tmux_config() -> io::Result<ImportReport> {
    let mut report = ImportReport::default();
    import_tmux_config_into(&mut report)?;
    Ok(report)
}

fn import_tmux_config_into(report: &mut ImportReport) -> io::Result<()> {
    if let Some(tmux) = discover_tmux_config() {
        let contents = read_bounded(&tmux, MAX_MUX_CONFIG_BYTES)?;
        let target = zz_daemon::mux_config_write_path().ok_or_else(|| {
            io::Error::new(
                ErrorKind::NotFound,
                "cannot create zz/mux.conf because neither XDG_CONFIG_HOME nor HOME is available",
            )
        })?;
        config::atomic_write(&target, &contents)?;
        report.mux_path = Some(target);
    }
    Ok(())
}

/// The `zz/config` lines a loaded Ghostty appearance imports as: every key the
/// donor set, directly or through its `theme` directive, as concrete values.
pub(crate) fn ghostty_import_values(
    load: &zz_terminal::AppearanceLoad,
) -> io::Result<Vec<(AppearanceConfigKey, Vec<String>)>> {
    let mut values = Vec::new();
    for key in AppearanceConfigKey::ALL {
        if !matches!(
            load.provenance.source(key),
            AppearanceSource::Ghostty | AppearanceSource::ThemeFile
        ) {
            continue;
        }
        let group = config::appearance_config_values(&load.appearance, key)
            .map_err(|error| io::Error::new(ErrorKind::InvalidData, error.to_string()))?;
        if group.is_empty() && !config::is_cumulative_appearance_key(key) {
            continue;
        }
        values.push((key, group));
    }
    Ok(values)
}

fn read_bounded(path: &Path, limit: usize) -> io::Result<Vec<u8>> {
    let file = std::fs::File::open(path)?;
    let byte_limit = u64::try_from(limit).unwrap_or(u64::MAX - 1);
    let mut contents = Vec::new();
    file.take(byte_limit + 1).read_to_end(&mut contents)?;
    if contents.len() > limit {
        return Err(io::Error::new(
            ErrorKind::InvalidData,
            format!("tmux configuration exceeds the {limit}-byte import limit"),
        ));
    }
    Ok(contents)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tmux_candidates_prefer_home_dotfile_then_xdg() {
        let home = PathBuf::from("/home/u");
        let xdg = PathBuf::from("/home/u/xdg");
        assert_eq!(
            tmux_config_candidates(Some(&home), Some(&xdg)),
            vec![
                PathBuf::from("/home/u/.tmux.conf"),
                PathBuf::from("/home/u/xdg/tmux/tmux.conf"),
                PathBuf::from("/home/u/.config/tmux/tmux.conf"),
            ],
        );
    }

    #[test]
    fn tmux_candidates_dedupe_xdg_matching_home_config() {
        let home = PathBuf::from("/home/u");
        let xdg = PathBuf::from("/home/u/.config");
        assert_eq!(
            tmux_config_candidates(Some(&home), Some(&xdg)),
            vec![
                PathBuf::from("/home/u/.tmux.conf"),
                PathBuf::from("/home/u/.config/tmux/tmux.conf"),
            ],
        );
    }

    #[test]
    fn tmux_copy_is_verbatim_and_overwrites_on_reimport() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let donor = directory.path().join(".tmux.conf");
        let target = directory.path().join("zz/mux.conf");
        let first = b"set -g prefix C-a\nbind-key F2 new-window # bytes kept exactly\n";
        std::fs::write(&donor, first).expect("write donor");

        let contents = read_bounded(&donor, MAX_MUX_CONFIG_BYTES).expect("read donor");
        config::atomic_write(&target, &contents).expect("copy donor");
        assert_eq!(std::fs::read(&target).expect("read copy"), first);

        let second = b"set -g prefix C-b\n";
        std::fs::write(&donor, second).expect("update donor");
        let contents = read_bounded(&donor, MAX_MUX_CONFIG_BYTES).expect("re-read donor");
        config::atomic_write(&target, &contents).expect("overwrite copy");
        assert_eq!(
            std::fs::read(&target).expect("read overwritten copy"),
            second
        );
    }

    #[test]
    fn oversized_tmux_donor_is_rejected() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let donor = directory.path().join(".tmux.conf");
        std::fs::write(&donor, vec![b'#'; 32]).expect("write donor");

        let error = read_bounded(&donor, 16).expect_err("donor past the bound must fail");
        assert_eq!(error.kind(), ErrorKind::InvalidData);
    }
}
