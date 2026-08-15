//! Where the daemon keeps the state it owns. The GUI's data directory holds
//! what a user edits; this one holds what the daemon writes, so the two never
//! collide when both run on the same machine.

use std::{io, path::PathBuf};

use crate::user_data::platform_data_dir;

const DAEMON_DIRECTORY_NAME: &str = "daemon";
const JOURNAL_DIRECTORY_NAME: &str = "agent-journal";

/// `<data>/zz/daemon`, the root of everything the daemon persists.
pub(crate) fn daemon_data_dir() -> io::Result<PathBuf> {
    let data = platform_data_dir().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "could not resolve the current user's application-data directory",
        )
    })?;
    Ok(data.join("zz").join(DAEMON_DIRECTORY_NAME))
}

pub(crate) fn journal_directory() -> io::Result<PathBuf> {
    Ok(daemon_data_dir()?.join(JOURNAL_DIRECTORY_NAME))
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;

    use super::{
        DAEMON_DIRECTORY_NAME, JOURNAL_DIRECTORY_NAME, daemon_data_dir, journal_directory,
    };
    use crate::user_data::platform_data_dir;

    #[test]
    fn the_journal_lives_under_the_daemons_own_data_directory() {
        let Some(data) = platform_data_dir() else {
            return;
        };
        let root = daemon_data_dir().expect("daemon data directory");
        let journal = journal_directory().expect("journal directory");

        assert_eq!(root, data.join("zz").join(DAEMON_DIRECTORY_NAME));
        assert_eq!(journal.parent(), Some(root.as_path()));
        assert_eq!(
            journal.file_name().and_then(OsStr::to_str),
            Some(JOURNAL_DIRECTORY_NAME)
        );
    }
}
