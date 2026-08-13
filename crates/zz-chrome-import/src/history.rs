use std::{
    collections::HashSet,
    fs, io,
    path::{Path, PathBuf},
};

use rusqlite::{Connection, OpenFlags};
use tempfile::TempDir;
use thiserror::Error;
use url::Url;

use crate::{cookie::source_storage_key, profiles::chrome_user_data_dir};

const CHROMIUM_EPOCH_OFFSET_MICROS: i64 = 11_644_473_600_000_000;
const MAX_HISTORY_DATABASE_BYTES: u64 = 512 * 1024 * 1024;
pub const MAX_HISTORY_IMPORT_COUNT: usize = 5_000;

/// One imported history row; the caller maps this onto its own store.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImportedPage {
    pub url: String,
    pub title: String,
    pub visited_at: u64,
    pub visit_count: u32,
    pub typed_count: u32,
}

/// Byte/entry caps the caller wants enforced during extraction.
#[derive(Clone, Copy, Debug)]
pub struct ImportLimits {
    pub max_count: usize,
    pub max_url_bytes: usize,
    pub max_title_bytes: usize,
}

#[derive(Debug, Error)]
pub enum ChromeHistoryImportError {
    #[error("the selected Chrome profile is invalid")]
    InvalidProfile,
    #[error("Chrome's history database was not found")]
    DatabaseNotFound,
    #[error("Chrome's history database is unexpectedly large ({0} bytes)")]
    DatabaseTooLarge(u64),
    #[error("Chrome did not have importable history in the selected profile")]
    NoHistory,
    #[error("could not read Chrome's history database: {0}")]
    Io(#[from] io::Error),
    #[error("could not query Chrome's history database: {0}")]
    Database(#[from] rusqlite::Error),
}

impl ChromeHistoryImportError {
    pub fn is_permission_denied(&self) -> bool {
        matches!(self, Self::Io(error) if error.kind() == io::ErrorKind::PermissionDenied)
    }
}

pub struct ChromeHistoryImport {
    pub pages: Vec<ImportedPage>,
    pub skipped: usize,
}

/// Read the selected installed Chrome profile's history through a private,
/// read-only `SQLite` snapshot. The source database is never opened for writes.
pub fn import_history(
    source_profile: &str,
    limits: ImportLimits,
) -> Result<ChromeHistoryImport, ChromeHistoryImportError> {
    let storage_key =
        source_storage_key(source_profile).map_err(|_| ChromeHistoryImportError::InvalidProfile)?;
    let database = chrome_history_database(storage_key)?;
    import_history_database(&database, limits)
}

fn chrome_history_database(storage_key: &str) -> Result<PathBuf, ChromeHistoryImportError> {
    let root = chrome_user_data_dir().ok_or(ChromeHistoryImportError::DatabaseNotFound)?;
    let database = root.join(storage_key).join("History");
    match database.try_exists() {
        Ok(true) => Ok(database),
        Ok(false) => Err(ChromeHistoryImportError::DatabaseNotFound),
        Err(error) => Err(error.into()),
    }
}

fn import_history_database(
    database: &Path,
    limits: ImportLimits,
) -> Result<ChromeHistoryImport, ChromeHistoryImportError> {
    let (_snapshot, snapshot_path) = snapshot_database(database)?;
    let connection = Connection::open_with_flags(
        snapshot_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    let max_count = limits.max_count;
    let sql = format!(
        "SELECT url, title, visit_count, typed_count, last_visit_time \
         FROM urls WHERE url LIKE 'http%' \
         ORDER BY last_visit_time DESC LIMIT {max_count}"
    );
    let mut statement = connection.prepare(&sql)?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;

    let mut pages = Vec::with_capacity(rows.len());
    let mut seen = HashSet::new();
    let mut skipped = 0usize;
    for (url, title, visit_count, typed_count, last_visit_time) in rows {
        let Some(visited_at) = chromium_time_to_unix_seconds(last_visit_time) else {
            skipped = skipped.saturating_add(1);
            continue;
        };
        if visit_count <= 0
            || url.len() > limits.max_url_bytes
            || !is_web_url(&url)
            || !seen.insert(url.clone())
        {
            skipped = skipped.saturating_add(1);
            continue;
        }
        pages.push(ImportedPage {
            url,
            title: truncate_utf8(&title, limits.max_title_bytes),
            visited_at,
            visit_count: u32::try_from(visit_count).unwrap_or(u32::MAX),
            typed_count: u32::try_from(typed_count.max(0)).unwrap_or(u32::MAX),
        });
    }
    if pages.is_empty() {
        return Err(ChromeHistoryImportError::NoHistory);
    }
    Ok(ChromeHistoryImport { pages, skipped })
}

fn is_web_url(value: &str) -> bool {
    Url::parse(value)
        .is_ok_and(|url| matches!(url.scheme(), "http" | "https") && url.host_str().is_some())
}

fn chromium_time_to_unix_seconds(value: i64) -> Option<u64> {
    let unix_micros = value.checked_sub(CHROMIUM_EPOCH_OFFSET_MICROS)?;
    u64::try_from(unix_micros.div_euclid(1_000_000)).ok()
}

fn snapshot_database(database: &Path) -> Result<(TempDir, PathBuf), ChromeHistoryImportError> {
    let database_bytes = fs::metadata(database)?.len();
    if database_bytes > MAX_HISTORY_DATABASE_BYTES {
        return Err(ChromeHistoryImportError::DatabaseTooLarge(database_bytes));
    }

    let directory = tempfile::Builder::new()
        .prefix("zz-chrome-history-")
        .tempdir()?;
    let snapshot = directory.path().join("History");
    let mut snapshot_bytes = fs::copy(database, &snapshot)?;
    if snapshot_bytes > MAX_HISTORY_DATABASE_BYTES {
        return Err(ChromeHistoryImportError::DatabaseTooLarge(snapshot_bytes));
    }
    for suffix in ["-wal", "-shm"] {
        let source = sidecar_path(database, suffix);
        let target = sidecar_path(&snapshot, suffix);
        match fs::metadata(&source) {
            Ok(metadata) => {
                let expected_bytes = snapshot_bytes.saturating_add(metadata.len());
                if expected_bytes > MAX_HISTORY_DATABASE_BYTES {
                    return Err(ChromeHistoryImportError::DatabaseTooLarge(expected_bytes));
                }
                match fs::copy(source, target) {
                    Ok(copied) => {
                        snapshot_bytes = snapshot_bytes.saturating_add(copied);
                        if snapshot_bytes > MAX_HISTORY_DATABASE_BYTES {
                            return Err(ChromeHistoryImportError::DatabaseTooLarge(snapshot_bytes));
                        }
                    }
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                    Err(error) => return Err(error.into()),
                }
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok((directory, snapshot))
}

fn sidecar_path(database: &Path, suffix: &str) -> PathBuf {
    let mut path = database.as_os_str().to_owned();
    path.push(suffix);
    PathBuf::from(path)
}

fn truncate_utf8(value: &str, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value.to_owned();
    }
    let mut end = max_bytes;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_owned()
}

#[cfg(test)]
mod tests {
    use rusqlite::params;
    use tempfile::tempdir;

    use super::*;

    const TEST_LIMITS: ImportLimits = ImportLimits {
        max_count: MAX_HISTORY_IMPORT_COUNT,
        max_url_bytes: 4 * 1024,
        max_title_bytes: 2 * 1024,
    };

    #[test]
    fn imports_recent_web_history_and_skips_invalid_rows() {
        let directory = tempdir().expect("temporary Chrome profile");
        let database = directory.path().join("History");
        let connection = Connection::open(&database).expect("create history database");
        connection
            .execute_batch(
                "CREATE TABLE urls ( \
                    url LONGVARCHAR, title LONGVARCHAR, visit_count INTEGER, \
                    typed_count INTEGER, last_visit_time INTEGER \
                 );",
            )
            .expect("create history schema");
        let earlier = CHROMIUM_EPOCH_OFFSET_MICROS + 10_000_000;
        let later = CHROMIUM_EPOCH_OFFSET_MICROS + 20_000_000;
        connection
            .execute(
                "INSERT INTO urls VALUES ('https://one.example', 'One', 1, 0, ?1)",
                params![earlier],
            )
            .expect("insert earlier page");
        connection
            .execute(
                "INSERT INTO urls VALUES ('https://two.example', 'Two', 2, 1, ?1)",
                params![later],
            )
            .expect("insert later page");
        connection
            .execute(
                "INSERT INTO urls VALUES ('https://two.example', 'Duplicate', 1, 0, ?1)",
                params![earlier],
            )
            .expect("insert duplicate page");
        connection
            .execute(
                "INSERT INTO urls VALUES ('http-not-a-url', 'Invalid', 1, 0, ?1)",
                params![later],
            )
            .expect("insert malformed page");
        connection
            .execute(
                "INSERT INTO urls VALUES ('file:///tmp/private', 'File', 1, 0, ?1)",
                params![later],
            )
            .expect("insert non-web page");
        drop(connection);

        let imported = import_history_database(&database, TEST_LIMITS).expect("import history");
        assert_eq!(imported.pages.len(), 2);
        assert_eq!(imported.skipped, 2);
        assert_eq!(imported.pages[0].url, "https://two.example");
        assert_eq!(imported.pages[0].visited_at, 20);
        assert_eq!(imported.pages[0].visit_count, 2);
        assert_eq!(imported.pages[0].typed_count, 1);
        assert_eq!(imported.pages[1].url, "https://one.example");
        assert_eq!(imported.pages[1].visited_at, 10);
    }

    #[test]
    fn snapshots_history_from_a_live_write_ahead_log() {
        let directory = tempdir().expect("temporary Chrome profile");
        let database = directory.path().join("History");
        let connection = Connection::open(&database).expect("create history database");
        let journal_mode = connection
            .query_row("PRAGMA journal_mode=WAL", [], |row| row.get::<_, String>(0))
            .expect("enable WAL mode");
        assert_eq!(journal_mode, "wal");
        connection
            .execute_batch(
                "PRAGMA wal_autocheckpoint=0; \
                 CREATE TABLE urls ( \
                    url LONGVARCHAR, title LONGVARCHAR, visit_count INTEGER, \
                    typed_count INTEGER, last_visit_time INTEGER \
                 );",
            )
            .expect("create history schema");
        connection
            .execute(
                "INSERT INTO urls VALUES ('https://live.example', 'Live', 1, 0, ?1)",
                params![CHROMIUM_EPOCH_OFFSET_MICROS + 30_000_000],
            )
            .expect("insert live page");

        let imported =
            import_history_database(&database, TEST_LIMITS).expect("snapshot active WAL");
        assert_eq!(imported.pages.len(), 1);
        assert_eq!(imported.pages[0].url, "https://live.example");
    }

    #[test]
    fn truncates_titles_on_utf8_boundaries() {
        let title = format!("{}é", "a".repeat(TEST_LIMITS.max_title_bytes - 1));
        let truncated = truncate_utf8(&title, TEST_LIMITS.max_title_bytes);
        assert_eq!(truncated.len(), TEST_LIMITS.max_title_bytes - 1);
        assert!(truncated.is_char_boundary(truncated.len()));
    }

    #[test]
    fn identifies_permission_denied_io_errors() {
        let denied = ChromeHistoryImportError::from(std::io::Error::from(
            std::io::ErrorKind::PermissionDenied,
        ));
        let missing =
            ChromeHistoryImportError::from(std::io::Error::from(std::io::ErrorKind::NotFound));

        assert!(denied.is_permission_denied());
        assert!(!missing.is_permission_denied());
    }
}
