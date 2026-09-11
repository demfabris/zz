use std::{
    io::{self, ErrorKind},
    path::{Path, PathBuf},
};

use zz_terminal::{
    AppearanceConfigKey, AppearanceSource, TerminalColorScheme, discover_ghostty_config,
    load_ghostty_appearance_from_for,
};

use crate as config;

#[derive(Debug, Default)]
pub struct ImportReport {
    /// Number of appearance keys copied from the Ghostty config into `zz/config`.
    pub ghostty_keys: usize,
    /// Where the appearance keys were written, when any were.
    pub config_path: Option<PathBuf>,
}

impl ImportReport {
    #[cfg(not(target_os = "ios"))]
    pub fn imported_anything(&self) -> bool {
        self.config_path.is_some()
    }
}

/// Import only the Ghostty appearance into `zz/config`, leaving `zz/mux.conf`
/// alone. Re-running overwrites the keys a previous import wrote.
pub fn import_ghostty_config(scheme: TerminalColorScheme) -> io::Result<ImportReport> {
    discover_ghostty_config().map_or_else(
        || Ok(ImportReport::default()),
        |path| import_ghostty_config_from(&path, scheme),
    )
}

pub fn import_ghostty_config_from(
    path: &Path,
    scheme: TerminalColorScheme,
) -> io::Result<ImportReport> {
    std::fs::File::open(path)
        .map_err(|error| io::Error::new(error.kind(), format!("{}: {error}", path.display())))?;
    if !path.is_file() {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            format!("{}: expected a configuration file", path.display()),
        ));
    }
    let load = load_ghostty_appearance_from_for(path, scheme);
    if load.fatal {
        return Err(io::Error::new(
            ErrorKind::InvalidData,
            format!("could not load {}: {:?}", path.display(), load.diagnostics),
        ));
    }
    let values = ghostty_import_values(&load)?;
    let mut report = ImportReport::default();
    if !values.is_empty() {
        let target = config::import_target_path()?;
        config::import_appearance_values_at(&target, &values)?;
        report.ghostty_keys = values.len();
        report.config_path = Some(target);
    }
    Ok(report)
}

/// The `zz/config` lines a loaded Ghostty appearance imports as: every key the
/// donor set, directly or through its `theme` directive, as concrete values.
pub fn ghostty_import_values(
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ghostty_import_missing_donor_names_the_path() {
        let directory = tempfile::tempdir().unwrap();
        let missing = directory.path().join("missing-ghostty");
        let error = import_ghostty_config_from(&missing, TerminalColorScheme::Dark).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::NotFound);
        assert!(error.to_string().contains(missing.to_str().unwrap()));
    }
}
