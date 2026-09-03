//! One-shot import of Ghostty and tmux configuration into zz-owned files.

use std::{
    io::{self, ErrorKind},
    path::PathBuf,
};

use zz_terminal::{
    AppearanceConfigKey, AppearanceSource, TerminalColorScheme, discover_ghostty_config,
    load_ghostty_appearance_from_for,
};

use crate::config;

pub(crate) use zz_daemon::discover_tmux_config;

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
        let target = zz_daemon::mux_config_write_path().ok_or_else(|| {
            io::Error::new(
                ErrorKind::NotFound,
                "cannot create zz/mux.conf because neither XDG_CONFIG_HOME nor HOME is available",
            )
        })?;
        zz_daemon::copy_tmux_config_into(&tmux, &target)?;
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
