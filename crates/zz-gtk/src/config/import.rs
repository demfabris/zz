//! One-shot import of an existing Ghostty or tmux configuration, offered once
//! on first run.
//!
//! The desktop flattens its Ghostty donor through the appearance loader and
//! re-serializes 30 typed keys. This client copies the donor's own appearance
//! lines instead: the daemon already parses the Ghostty dialect — `theme = X`
//! included — so re-deriving the values here would only buy a second
//! serializer to keep in step. The cost is that a donor split across
//! `config-file` includes contributes only its root file, which is noted in the
//! prompt rather than silently papered over.
//!
//! Donors are read, never modified.

use std::{
    fs,
    io::{self, ErrorKind, Read as _},
    path::{Path, PathBuf},
};

use zz_terminal::{AppearanceConfigKey, discover_ghostty_config};

use crate::config::file;

/// Cap on the verbatim tmux copy, matching the desktop's import bound.
const MAX_MUX_CONFIG_BYTES: usize = 1024 * 1024;

const MARKER_FILE_NAME: &str = "import-prompted";

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Report {
    pub ghostty_keys: usize,
    pub config_path: Option<PathBuf>,
    pub mux_path: Option<PathBuf>,
}

impl Report {
    #[must_use]
    pub const fn imported_anything(&self) -> bool {
        self.config_path.is_some() || self.mux_path.is_some()
    }
}

fn nonempty_env(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

/// The tmux config the import copies, in tmux's own precedence order.
#[must_use]
pub fn discover_tmux_config() -> Option<PathBuf> {
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

#[must_use]
pub fn donors_present() -> bool {
    discover_ghostty_config().is_some() || discover_tmux_config().is_some()
}

/// Whether the one-time prompt is still owed. The marker is written whatever
/// the answer was, so declining is remembered as firmly as accepting.
#[must_use]
pub fn prompt_pending() -> bool {
    !marker_path().as_deref().is_some_and(Path::exists) && donors_present()
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

/// The marker lives beside the config rather than in a data directory of its
/// own: this client has exactly one state file's worth of state, and putting it
/// next to `zz/config` keeps the whole footprint in one place.
fn marker_path() -> Option<PathBuf> {
    file::candidates()
        .into_iter()
        .next()
        .and_then(|config| config.parent().map(|parent| parent.join(MARKER_FILE_NAME)))
}

pub fn run() -> io::Result<Report> {
    let mut report = Report::default();
    import_ghostty(&mut report)?;
    import_tmux(&mut report)?;
    Ok(report)
}

fn import_ghostty(report: &mut Report) -> io::Result<()> {
    let Some(donor) = discover_ghostty_config() else {
        return Ok(());
    };
    let groups = appearance_groups(&read_bounded_string(&donor, file::MAX_CONFIG_BYTES)?);
    if groups.is_empty() {
        return Ok(());
    }
    let target = file::path_for_write()?;
    let source = match file::read_source(&target) {
        Ok(source) => source,
        Err(error) if error.kind() == ErrorKind::NotFound => String::new(),
        Err(error) => return Err(error),
    };
    let mut edited = source;
    for (key, values) in &groups {
        edited = file::replace_key_group(&edited, key, values);
    }
    if edited.len() > file::MAX_CONFIG_BYTES {
        return Err(io::Error::new(
            ErrorKind::InvalidData,
            format!(
                "importing would push the configuration past its {}-byte limit",
                file::MAX_CONFIG_BYTES
            ),
        ));
    }
    file::atomic_write(&target, edited.as_bytes())?;
    report.ghostty_keys = groups.len();
    report.config_path = Some(target);
    Ok(())
}

/// The donor's appearance lines, grouped by key in first-appearance order. A
/// key the donor repeats keeps every occurrence, because `palette` and
/// `font-family` are cumulative.
fn appearance_groups(donor: &str) -> Vec<(String, Vec<String>)> {
    let mut groups: Vec<(String, Vec<String>)> = Vec::new();
    for line in donor.lines() {
        let Some(key) = file::key_for_line(line) else {
            continue;
        };
        if AppearanceConfigKey::from_config_key(key).is_none() {
            continue;
        }
        let Some((_, value)) = line.split_once('=') else {
            continue;
        };
        let value = file::value_without_comment(value).trim().to_owned();
        match groups.iter_mut().find(|(existing, _)| existing == key) {
            Some((_, values)) => values.push(value),
            None => groups.push((key.to_owned(), vec![value])),
        }
    }
    groups
}

fn import_tmux(report: &mut Report) -> io::Result<()> {
    let Some(donor) = discover_tmux_config() else {
        return Ok(());
    };
    let contents = read_bounded(&donor, MAX_MUX_CONFIG_BYTES)?;
    let target = zz_daemon::mux_config_write_path().ok_or_else(|| {
        io::Error::new(
            ErrorKind::NotFound,
            "cannot create zz/mux.conf because neither XDG_CONFIG_HOME nor HOME is available",
        )
    })?;
    file::atomic_write(&target, &contents)?;
    report.mux_path = Some(target);
    Ok(())
}

fn read_bounded_string(path: &Path, limit: usize) -> io::Result<String> {
    String::from_utf8(read_bounded(path, limit)?)
        .map_err(|error| io::Error::new(ErrorKind::InvalidData, error))
}

fn read_bounded(path: &Path, limit: usize) -> io::Result<Vec<u8>> {
    let file = fs::File::open(path)?;
    let byte_limit = u64::try_from(limit).unwrap_or(u64::MAX - 1);
    let mut contents = Vec::new();
    file.take(byte_limit + 1).read_to_end(&mut contents)?;
    if contents.len() > limit {
        return Err(io::Error::new(
            ErrorKind::InvalidData,
            format!("configuration exceeds the {limit}-byte import limit"),
        ));
    }
    Ok(contents)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tmux_candidates_prefer_the_home_dotfile_then_xdg() {
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
    fn tmux_candidates_dedupe_an_xdg_that_is_the_home_config() {
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
    fn only_appearance_keys_are_taken_from_a_ghostty_donor() {
        let groups = appearance_groups(
            "# ghostty\nfont-size = 15\nkeybind = ctrl+a=new_split\ntheme = catppuccin\n",
        );

        assert_eq!(
            groups,
            vec![
                ("font-size".to_owned(), vec!["15".to_owned()]),
                ("theme".to_owned(), vec!["catppuccin".to_owned()]),
            ]
        );
    }

    #[test]
    fn a_repeated_donor_key_keeps_every_occurrence() {
        let groups = appearance_groups("palette = 0=#000000\npalette = 1=#111111\n");

        assert_eq!(
            groups,
            vec![(
                "palette".to_owned(),
                vec!["0=#000000".to_owned(), "1=#111111".to_owned()]
            )]
        );
    }

    #[test]
    fn importing_replaces_the_whole_group_and_leaves_everything_else_alone() {
        let source = "# mine\npalette = 0=#FFFFFF\npane-margin = 8\npalette = 1=#EEEEEE\n";
        let groups = appearance_groups("palette = 0=#000000\n");

        let mut edited = source.to_owned();
        for (key, values) in &groups {
            edited = file::replace_key_group(&edited, key, values);
        }

        assert_eq!(edited, "# mine\npane-margin = 8\npalette = 0=#000000\n");
    }
}
