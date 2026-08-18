use std::{
    collections::{BTreeMap, HashSet},
    fs, io,
    path::{Path, PathBuf},
};

#[cfg(not(target_os = "windows"))]
use aes::Aes128;
#[cfg(not(target_os = "windows"))]
use cbc::cipher::{BlockDecryptMut as _, KeyIvInit as _, block_padding::Pkcs7};
#[cfg(not(target_os = "windows"))]
use pbkdf2::pbkdf2_hmac;
use rusqlite::{Connection, OpenFlags};
use serde_json::{Map, Value};
#[cfg(not(target_os = "windows"))]
use sha1::Sha1;
use sha2::{Digest as _, Sha256};
use tempfile::TempDir;
use thiserror::Error;
use url::Url;
#[cfg(target_os = "windows")]
use zeroize::Zeroize as _;
use zeroize::Zeroizing;
use zz_browser::{
    BrowserCookie, CookieImportBatch, CookieImportError, MAX_COOKIE_IMPORT_BYTES,
    parse_cookie_import,
};

#[cfg(target_os = "windows")]
use crate::profiles::{MAX_LOCAL_STATE_BYTES, chrome_local_state_path};
use crate::profiles::{chrome_storage_key, chrome_user_data_dir};

const CHROME_PROFILE_PREFIX: &str = "chrome:";
const DEFAULT_CHROME_PROFILE: &str = "Default";
const CHROMIUM_EPOCH_OFFSET_MICROS: i64 = 11_644_473_600_000_000;
const MAX_COOKIE_DATABASE_BYTES: u64 = 512 * 1024 * 1024;
const NORMALIZE_CHUNK_COUNT: usize = 512;
#[cfg(not(target_os = "windows"))]
const SALT: &[u8] = b"saltysalt";
#[cfg(not(target_os = "windows"))]
const IV: [u8; 16] = [b' '; 16];

#[cfg(not(target_os = "windows"))]
type Aes128CbcDecryptor = cbc::Decryptor<Aes128>;
#[cfg(not(target_os = "windows"))]
type DerivedKeys = Zeroizing<Vec<[u8; 16]>>;

#[derive(Debug, Error)]
pub enum ChromeCookieImportError {
    #[error("automatic Chrome cookie import is not supported on this platform")]
    UnsupportedPlatform,
    #[error("the selected Chrome profile is invalid")]
    InvalidProfile,
    #[error("Chrome's cookies database was not found")]
    DatabaseNotFound,
    #[error("Chrome's cookies database is unexpectedly large ({0} bytes)")]
    DatabaseTooLarge(u64),
    #[error("Chrome did not have cookies in the selected profile")]
    NoCookiesInProfile,
    #[error("Chrome's cookie encryption key could not be unlocked")]
    CredentialAccess,
    #[error(
        "Chrome 127 or newer sealed these cookies with app-bound (v20) encryption, which only \
         Chrome's own elevated app-bound service can unlock ({skipped} cookies skipped)"
    )]
    AppBoundEncryption { skipped: usize },
    #[error("Chrome had no cookies that can be imported ({skipped} skipped)")]
    NoUsableCookies { skipped: usize },
    #[error("could not read Chrome's cookies database: {0}")]
    Io(#[from] io::Error),
    #[error("could not query Chrome's cookies database: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("could not normalize Chrome cookies: {0}")]
    Normalize(#[from] CookieImportError),
    #[error("could not encode Chrome cookies: {0}")]
    Json(#[from] serde_json::Error),
}

impl ChromeCookieImportError {
    pub fn is_permission_denied(&self) -> bool {
        matches!(self, Self::Io(error) if error.kind() == io::ErrorKind::PermissionDenied)
    }
}

#[derive(Debug)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "these flags mirror independent columns in Chrome's cookie schema"
)]
struct RawChromeCookie {
    host_key: String,
    name: String,
    value: String,
    path: String,
    expires_utc: i64,
    encrypted_value: Vec<u8>,
    secure: bool,
    http_only: bool,
    has_expires: bool,
    same_site: i64,
    priority: i64,
    source_scheme: i64,
    source_port: i64,
    top_frame_site_key: String,
    partitioned: bool,
    same_party: bool,
}

struct ChromiumKeys {
    /// AES-128-CBC keys derived from the platform credential store.
    #[cfg(not(target_os = "windows"))]
    v10: DerivedKeys,
    #[cfg(not(target_os = "windows"))]
    v11: DerivedKeys,
    /// Windows keeps one AES-256-GCM master key in `Local State` instead.
    #[cfg(target_os = "windows")]
    master: Zeroizing<[u8; AES_GCM_KEY_BYTES]>,
    credential_unavailable: bool,
}

#[must_use]
pub const fn automatic_import_supported() -> bool {
    cfg!(any(
        target_os = "macos",
        target_os = "linux",
        target_os = "windows"
    ))
}

/// Imports every usable cookie from one installed Chrome profile, read-only.
/// The caller picks the destination zz profile when it hands the batch to CEF.
pub fn import_all_cookies(
    source_profile: &str,
) -> Result<CookieImportBatch, ChromeCookieImportError> {
    if !automatic_import_supported() {
        return Err(ChromeCookieImportError::UnsupportedPlatform);
    }

    let storage_key = source_storage_key(source_profile)?;
    let database = chrome_cookie_database(storage_key)?;
    import_cookie_database(&database, load_chromium_keys)
}

pub fn source_storage_key(zz_profile: &str) -> Result<&str, ChromeCookieImportError> {
    if zz_profile == zz_browser::DEFAULT_BROWSER_PROFILE {
        return Ok(DEFAULT_CHROME_PROFILE);
    }
    if zz_profile.starts_with(CHROME_PROFILE_PREFIX) {
        return chrome_storage_key(zz_profile).ok_or(ChromeCookieImportError::InvalidProfile);
    }
    Ok(DEFAULT_CHROME_PROFILE)
}

fn chrome_cookie_database(storage_key: &str) -> Result<PathBuf, ChromeCookieImportError> {
    let root = chrome_user_data_dir().ok_or(ChromeCookieImportError::DatabaseNotFound)?;
    let profile = root.join(storage_key);
    for relative in ["Network/Cookies", "Cookies"] {
        let candidate = profile.join(relative);
        match candidate.try_exists() {
            Ok(true) => return Ok(candidate),
            Ok(false) => {}
            Err(error) => return Err(error.into()),
        }
    }
    Err(ChromeCookieImportError::DatabaseNotFound)
}

fn import_cookie_database(
    database: &Path,
    mut load_keys: impl FnMut() -> Result<ChromiumKeys, ChromeCookieImportError>,
) -> Result<CookieImportBatch, ChromeCookieImportError> {
    let (_snapshot, snapshot_path) = snapshot_database(database)?;
    let connection = Connection::open_with_flags(
        snapshot_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    let meta_version = chromium_meta_version(&connection);
    let rows = read_cookie_rows(&connection)?;
    if rows.is_empty() {
        return Err(ChromeCookieImportError::NoCookiesInProfile);
    }
    let mut skipped = 0usize;
    let rows: Vec<_> = rows
        .into_iter()
        .filter(|row| {
            let supported =
                row.top_frame_site_key.is_empty() && !row.partitioned && !row.same_party;
            if !supported {
                skipped = skipped.saturating_add(1);
            }
            supported
        })
        .collect();
    let mut keys = None;
    let mut key_error = false;
    let mut app_bound = 0usize;
    let mut normalized_candidates = 0usize;
    let mut records = Vec::with_capacity(rows.len().min(NORMALIZE_CHUNK_COUNT));
    let mut batch = CookieImportBatch {
        cookies: Vec::new(),
        skipped,
    };
    for row in rows {
        let value = if !row.value.is_empty() || row.encrypted_value.is_empty() {
            row.value.clone()
        } else if is_app_bound_value(&row.encrypted_value) {
            // Skipped before the credential store opens: no key here decrypts it.
            app_bound = app_bound.saturating_add(1);
            batch.skipped = batch.skipped.saturating_add(1);
            continue;
        } else {
            let Ok(key_set) = keys.get_or_insert_with(&mut load_keys) else {
                key_error = true;
                batch.skipped = batch.skipped.saturating_add(1);
                continue;
            };
            let Some(value) = decrypt_cookie_value(
                &row.encrypted_value,
                &row.host_key,
                meta_version >= 24,
                key_set,
            ) else {
                key_error |= key_set.credential_unavailable;
                batch.skipped = batch.skipped.saturating_add(1);
                continue;
            };
            value
        };

        records.push(cookie_json(&row, value));
        normalized_candidates = normalized_candidates.saturating_add(1);
        if records.len() == NORMALIZE_CHUNK_COUNT {
            normalize_cookie_record_chunk(&records, &mut batch)?;
            records.clear();
        }
    }
    normalize_cookie_record_chunk(&records, &mut batch)?;
    let (cookies, duplicates) = deduplicate_normalized_cookies(batch.cookies);
    batch.cookies = cookies;
    batch.skipped = batch.skipped.saturating_add(duplicates);
    if batch.cookies.is_empty() {
        if app_bound > 0 {
            return Err(ChromeCookieImportError::AppBoundEncryption { skipped: app_bound });
        }
        return if key_error {
            if normalized_candidates == 0 {
                Err(ChromeCookieImportError::CredentialAccess)
            } else {
                Err(ChromeCookieImportError::NoUsableCookies {
                    skipped: batch.skipped,
                })
            }
        } else {
            Err(ChromeCookieImportError::NoUsableCookies {
                skipped: batch.skipped,
            })
        };
    }
    Ok(batch)
}

fn snapshot_database(database: &Path) -> Result<(TempDir, PathBuf), ChromeCookieImportError> {
    let database_bytes = fs::metadata(database)?.len();
    if database_bytes > MAX_COOKIE_DATABASE_BYTES {
        return Err(ChromeCookieImportError::DatabaseTooLarge(database_bytes));
    }

    let directory = tempfile::Builder::new()
        .prefix("zz-chrome-cookies-")
        .tempdir()?;
    let snapshot = directory.path().join("Cookies");
    let mut snapshot_bytes = fs::copy(database, &snapshot)?;
    if snapshot_bytes > MAX_COOKIE_DATABASE_BYTES {
        return Err(ChromeCookieImportError::DatabaseTooLarge(snapshot_bytes));
    }
    for suffix in ["-wal", "-shm"] {
        let source = sidecar_path(database, suffix);
        let target = sidecar_path(&snapshot, suffix);
        match fs::metadata(&source) {
            Ok(metadata) => {
                let expected_bytes = snapshot_bytes.saturating_add(metadata.len());
                if expected_bytes > MAX_COOKIE_DATABASE_BYTES {
                    return Err(ChromeCookieImportError::DatabaseTooLarge(expected_bytes));
                }
                match fs::copy(source, target) {
                    Ok(copied) => {
                        snapshot_bytes = snapshot_bytes.saturating_add(copied);
                        if snapshot_bytes > MAX_COOKIE_DATABASE_BYTES {
                            return Err(ChromeCookieImportError::DatabaseTooLarge(snapshot_bytes));
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

fn chromium_meta_version(connection: &Connection) -> i64 {
    connection
        .query_row("SELECT value FROM meta WHERE key = 'version'", [], |row| {
            row.get::<_, String>(0)
        })
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or_default()
}

fn read_cookie_rows(
    connection: &Connection,
) -> Result<Vec<RawChromeCookie>, ChromeCookieImportError> {
    let columns = cookie_columns(connection)?;
    let optional = |name: &str, fallback: &str| {
        if columns.contains(name) {
            name.to_owned()
        } else {
            fallback.to_owned()
        }
    };
    let sql = format!(
        "SELECT host_key, name, value, path, expires_utc, encrypted_value, \
         is_secure, is_httponly, {}, {}, {}, {}, {}, {}, {}, {} \
         FROM cookies",
        optional("has_expires", "CASE WHEN expires_utc = 0 THEN 0 ELSE 1 END"),
        optional("samesite", "-1"),
        optional("priority", "1"),
        optional("source_scheme", "0"),
        optional("source_port", "-1"),
        optional("top_frame_site_key", "''"),
        optional("is_partitioned", "0"),
        optional("is_same_party", "0"),
    );

    let mut statement = connection.prepare(&sql)?;
    statement
        .query_map([], raw_chrome_cookie)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(ChromeCookieImportError::from)
}

fn raw_chrome_cookie(row: &rusqlite::Row<'_>) -> rusqlite::Result<RawChromeCookie> {
    Ok(RawChromeCookie {
        host_key: row.get(0)?,
        name: row.get(1)?,
        value: row.get(2)?,
        path: row.get(3)?,
        expires_utc: row.get(4)?,
        encrypted_value: row.get(5)?,
        secure: row.get::<_, i64>(6)? != 0,
        http_only: row.get::<_, i64>(7)? != 0,
        has_expires: row.get::<_, i64>(8)? != 0,
        same_site: row.get(9)?,
        priority: row.get(10)?,
        source_scheme: row.get(11)?,
        source_port: row.get(12)?,
        top_frame_site_key: row.get(13)?,
        partitioned: row.get::<_, i64>(14)? != 0,
        same_party: row.get::<_, i64>(15)? != 0,
    })
}

/// Collapses cookies onto CEF's narrower name/domain/path identity, keeping the
/// later expiry. Chrome's own key also covers the source scheme and port.
fn deduplicate_normalized_cookies(cookies: Vec<BrowserCookie>) -> (Vec<BrowserCookie>, usize) {
    let original_len = cookies.len();
    let mut unique = BTreeMap::new();
    for cookie in cookies {
        let identity_domain = if cookie.domain.is_empty() {
            Url::parse(&cookie.source_url)
                .ok()
                .and_then(|url| url.host_str().map(str::to_owned))
                .unwrap_or_default()
        } else {
            cookie.domain.clone()
        };
        let identity = (cookie.name.clone(), identity_domain, cookie.path.clone());
        match unique.entry(identity) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(cookie);
            }
            std::collections::btree_map::Entry::Occupied(mut entry)
                if cookie.expires_unix_micros.unwrap_or(i64::MIN)
                    > entry.get().expires_unix_micros.unwrap_or(i64::MIN) =>
            {
                entry.insert(cookie);
            }
            std::collections::btree_map::Entry::Occupied(_) => {}
        }
    }
    let duplicates = original_len.saturating_sub(unique.len());
    (unique.into_values().collect(), duplicates)
}

/// Normalizes in chunks under the public parser's 10,000-row / 8 MiB cap, so a
/// large profile reaches CEF as one batch instead of being truncated.
#[cfg(test)]
fn normalize_cookie_records(
    records: &[Value],
    skipped: usize,
) -> Result<CookieImportBatch, ChromeCookieImportError> {
    let mut batch = CookieImportBatch {
        cookies: Vec::new(),
        skipped,
    };
    for chunk in records.chunks(NORMALIZE_CHUNK_COUNT) {
        normalize_cookie_record_chunk(chunk, &mut batch)?;
    }
    if batch.cookies.is_empty() {
        return Err(ChromeCookieImportError::NoUsableCookies {
            skipped: batch.skipped,
        });
    }
    Ok(batch)
}

fn normalize_cookie_record_chunk(
    records: &[Value],
    output: &mut CookieImportBatch,
) -> Result<(), ChromeCookieImportError> {
    if records.is_empty() {
        return Ok(());
    }
    let json = serde_json::to_string(records)?;
    if json.len() > MAX_COOKIE_IMPORT_BYTES {
        if records.len() == 1 {
            output.skipped = output.skipped.saturating_add(1);
            return Ok(());
        }
        let middle = records.len() / 2;
        normalize_cookie_record_chunk(&records[..middle], output)?;
        return normalize_cookie_record_chunk(&records[middle..], output);
    }
    match parse_cookie_import(&json) {
        Ok(batch) => {
            output.cookies.extend(batch.cookies);
            output.skipped = output.skipped.saturating_add(batch.skipped);
        }
        Err(CookieImportError::NoUsableCookies { skipped }) => {
            output.skipped = output.skipped.saturating_add(skipped);
        }
        Err(error) => return Err(error.into()),
    }
    Ok(())
}

fn cookie_columns(connection: &Connection) -> Result<HashSet<String>, rusqlite::Error> {
    let mut statement = connection.prepare("PRAGMA table_info(cookies)")?;
    statement
        .query_map([], |row| row.get(1))?
        .collect::<Result<HashSet<_>, _>>()
}

fn cookie_json(row: &RawChromeCookie, value: String) -> Value {
    let mut record = Map::new();
    record.insert("name".to_owned(), Value::String(row.name.clone()));
    record.insert("value".to_owned(), Value::String(value));
    record.insert("domain".to_owned(), Value::String(row.host_key.clone()));
    record.insert(
        "hostOnly".to_owned(),
        Value::Bool(!row.host_key.starts_with('.')),
    );
    record.insert(
        "path".to_owned(),
        Value::String(if row.path.is_empty() {
            "/".to_owned()
        } else {
            row.path.clone()
        }),
    );
    record.insert("secure".to_owned(), Value::Bool(row.secure));
    record.insert("httpOnly".to_owned(), Value::Bool(row.http_only));
    if row.has_expires && row.expires_utc > 0 {
        let unix_micros = row.expires_utc.saturating_sub(CHROMIUM_EPOCH_OFFSET_MICROS);
        let unix_seconds = unix_micros.div_euclid(1_000_000);
        record.insert(
            "expirationDate".to_owned(),
            Value::Number(unix_seconds.into()),
        );
    } else {
        record.insert("session".to_owned(), Value::Bool(true));
    }
    record.insert(
        "sameSite".to_owned(),
        Value::String(
            match row.same_site {
                -1 => "unspecified",
                0 => "no_restriction",
                1 => "lax",
                2 => "strict",
                _ => "unsupported",
            }
            .to_owned(),
        ),
    );
    record.insert(
        "priority".to_owned(),
        Value::String(
            match row.priority {
                0 => "low",
                1 => "medium",
                2 => "high",
                _ => "unsupported",
            }
            .to_owned(),
        ),
    );
    match row.source_scheme {
        1 => {
            record.insert(
                "sourceScheme".to_owned(),
                Value::String("nonsecure".to_owned()),
            );
        }
        2 => {
            record.insert(
                "sourceScheme".to_owned(),
                Value::String("secure".to_owned()),
            );
        }
        0 => {}
        _ => {
            record.insert(
                "sourceScheme".to_owned(),
                Value::String("unsupported".to_owned()),
            );
        }
    }
    if let Ok(port) = i32::try_from(row.source_port)
        && port > 0
    {
        record.insert("sourcePort".to_owned(), Value::Number(port.into()));
    }
    Value::Object(record)
}

/// Chrome 127 and newer re-seal Windows cookies with app-bound encryption, and
/// unwrapping a `v20` value needs Chrome's own elevated service. Skip them.
#[cfg(target_os = "windows")]
fn is_app_bound_value(encrypted: &[u8]) -> bool {
    encrypted.starts_with(APP_BOUND_PREFIX)
}

#[cfg(not(target_os = "windows"))]
const fn is_app_bound_value(_encrypted: &[u8]) -> bool {
    false
}

#[cfg(not(target_os = "windows"))]
fn decrypt_cookie_value(
    encrypted: &[u8],
    host_key: &str,
    has_host_digest: bool,
    keys: &ChromiumKeys,
) -> Option<String> {
    let (prefix, ciphertext) = encrypted.split_at_checked(3)?;
    let candidates = match prefix {
        b"v10" => keys.v10.as_slice(),
        b"v11" => keys.v11.as_slice(),
        _ => return None,
    };
    for key in candidates {
        let mut buffer = ciphertext.to_vec();
        let Ok(plaintext) = Aes128CbcDecryptor::new(key.into(), &IV.into())
            .decrypt_padded_mut::<Pkcs7>(&mut buffer)
        else {
            continue;
        };
        let value = if has_host_digest {
            let digest = Sha256::digest(host_key.as_bytes());
            let Some(value) = plaintext.strip_prefix(digest.as_slice()) else {
                continue;
            };
            value
        } else {
            plaintext
        };
        if let Ok(value) = String::from_utf8(value.to_vec()) {
            return Some(value);
        }
    }
    None
}

#[cfg(not(target_os = "windows"))]
fn derive_key(password: &[u8], iterations: u32) -> [u8; 16] {
    let mut key = [0u8; 16];
    pbkdf2_hmac::<Sha1>(password, SALT, iterations, &mut key);
    key
}

#[cfg(target_os = "macos")]
fn load_chromium_keys() -> Result<ChromiumKeys, ChromeCookieImportError> {
    use security_framework::passwords::get_generic_password;

    let secret = get_generic_password("Chrome Safe Storage", "Chrome")
        .map(Zeroizing::new)
        .map_err(|_| ChromeCookieImportError::CredentialAccess)?;
    if secret.is_empty() {
        return Err(ChromeCookieImportError::CredentialAccess);
    }
    let key = derive_key(&secret, 1003);
    Ok(ChromiumKeys {
        v10: Zeroizing::new(vec![key]),
        v11: Zeroizing::new(vec![key]),
        credential_unavailable: false,
    })
}

#[cfg(target_os = "linux")]
#[allow(
    clippy::unnecessary_wraps,
    reason = "platform key loaders share one fallible callback signature"
)]
fn load_chromium_keys() -> Result<ChromiumKeys, ChromeCookieImportError> {
    let mut v10 = vec![derive_key(b"peanuts", 1), derive_key(b"", 1)];
    let mut v11 = vec![derive_key(b"", 1)];
    let secret = linux_safe_storage_secret();
    if let Some(secret) = secret.as_deref() {
        let key = derive_key(secret, 1);
        v10.push(key);
        v11.insert(0, key);
    }
    Ok(ChromiumKeys {
        v10: Zeroizing::new(v10),
        v11: Zeroizing::new(v11),
        credential_unavailable: secret.is_none(),
    })
}

#[cfg(target_os = "linux")]
fn linux_safe_storage_secret() -> Option<Zeroizing<Vec<u8>>> {
    smol::block_on(async {
        let service = oo7::dbus::Service::new().await.ok()?;
        let collection = service.default_collection().await.ok()?;
        if collection.is_locked().await.ok()? {
            collection.unlock(None).await.ok()?;
        }
        for attributes in [
            vec![("application", "chrome")],
            vec![("service", "Chrome Safe Storage"), ("account", "Chrome")],
        ] {
            let items = collection.search_items(&attributes).await.ok()?;
            for item in items {
                if item.is_locked().await.ok()? {
                    item.unlock(None).await.ok()?;
                }
                let secret = item.secret().await.ok()?;
                if !secret.as_bytes().is_empty() {
                    return Some(Zeroizing::new(secret.as_bytes().to_vec()));
                }
            }
        }
        None
    })
}

// Chrome tags the `Local State` master key with the API protecting it.
#[cfg(target_os = "windows")]
const DPAPI_KEY_TAG: &[u8] = b"DPAPI";
#[cfg(target_os = "windows")]
const APP_BOUND_PREFIX: &[u8] = b"v20";
#[cfg(target_os = "windows")]
const AES_GCM_KEY_BYTES: usize = 32;
#[cfg(target_os = "windows")]
const AES_GCM_NONCE_BYTES: usize = 12;

#[cfg(target_os = "windows")]
fn load_chromium_keys() -> Result<ChromiumKeys, ChromeCookieImportError> {
    Ok(ChromiumKeys {
        master: local_state_master_key().ok_or(ChromeCookieImportError::CredentialAccess)?,
        credential_unavailable: false,
    })
}

/// `Local State` carries `OSCrypt`'s master key as base64 of `"DPAPI" || blob`,
/// which only the logged-in user can unwrap.
#[cfg(target_os = "windows")]
fn local_state_master_key() -> Option<Zeroizing<[u8; AES_GCM_KEY_BYTES]>> {
    use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};

    let path = chrome_local_state_path()?;
    if fs::metadata(&path).ok()?.len() > MAX_LOCAL_STATE_BYTES {
        return None;
    }
    let state: Value = serde_json::from_slice(&fs::read(path).ok()?).ok()?;
    let encoded = state.get("os_crypt")?.get("encrypted_key")?.as_str()?;
    let tagged = Zeroizing::new(BASE64.decode(encoded).ok()?);
    let key = dpapi_unprotect(tagged.strip_prefix(DPAPI_KEY_TAG)?)?;
    Some(Zeroizing::new(
        <[u8; AES_GCM_KEY_BYTES]>::try_from(key.as_slice()).ok()?,
    ))
}

/// Chrome's Windows cookie layout: `"v10" || 12-byte nonce || ciphertext || tag`.
#[cfg(target_os = "windows")]
fn decrypt_cookie_value(
    encrypted: &[u8],
    host_key: &str,
    has_host_digest: bool,
    keys: &ChromiumKeys,
) -> Option<String> {
    let (plaintext, host_bound) = if let Some(sealed) = encrypted.strip_prefix(b"v10") {
        (decrypt_aes_gcm(sealed, &keys.master)?, has_host_digest)
    } else if is_app_bound_value(encrypted) {
        return None;
    } else {
        // Values older than the `v10` prefix are raw DPAPI, and never host-bound.
        (dpapi_unprotect(encrypted)?, false)
    };
    let value = if host_bound {
        plaintext.strip_prefix(Sha256::digest(host_key.as_bytes()).as_slice())?
    } else {
        plaintext.as_slice()
    };
    String::from_utf8(value.to_vec()).ok()
}

#[cfg(target_os = "windows")]
fn decrypt_aes_gcm(sealed: &[u8], key: &[u8; AES_GCM_KEY_BYTES]) -> Option<Zeroizing<Vec<u8>>> {
    use aes_gcm::{Aes256Gcm, KeyInit as _, aead::Aead as _};

    let (nonce, ciphertext) = sealed.split_at_checked(AES_GCM_NONCE_BYTES)?;
    let nonce: &[u8; AES_GCM_NONCE_BYTES] = nonce.try_into().ok()?;
    Aes256Gcm::new(key.into())
        .decrypt(nonce.into(), ciphertext)
        .ok()
        .map(Zeroizing::new)
}

/// DPAPI decryption for the logged-in user: the master key and pre-`v10` blobs.
#[cfg(target_os = "windows")]
#[allow(
    unsafe_code,
    reason = "CryptUnprotectData is a raw Win32 entry point with no safe wrapper"
)]
fn dpapi_unprotect(protected: &[u8]) -> Option<Zeroizing<Vec<u8>>> {
    use windows::Win32::{
        Foundation::{HLOCAL, LocalFree},
        Security::Cryptography::{CRYPT_INTEGER_BLOB, CryptUnprotectData},
    };

    let input = CRYPT_INTEGER_BLOB {
        cbData: u32::try_from(protected.len()).ok()?,
        pbData: protected.as_ptr().cast_mut(),
    };
    let mut output = CRYPT_INTEGER_BLOB::default();
    // SAFETY: `input` describes a buffer that outlives the call and that
    // CryptUnprotectData only reads, and `output` is a live local. Success
    // means it wrote a LocalAlloc'd plaintext buffer into `output`.
    unsafe { CryptUnprotectData(&raw const input, None, None, None, None, 0, &raw mut output) }
        .ok()?;
    let length = usize::try_from(output.cbData).unwrap_or_default();
    let plaintext = (!output.pbData.is_null() && length > 0).then(|| {
        // SAFETY: on success the API reports exactly `cbData` readable bytes at
        // `pbData`, and the copy finishes before the buffer is released.
        Zeroizing::new(unsafe { std::slice::from_raw_parts(output.pbData, length) }.to_vec())
    });
    if !output.pbData.is_null() && length > 0 {
        unsafe { std::slice::from_raw_parts_mut(output.pbData, length) }.zeroize();
    }
    // SAFETY: `pbData` is the LocalAlloc'd buffer CryptUnprotectData returned,
    // and nothing references it once the copy above is done.
    let _ = unsafe { LocalFree(Some(HLOCAL(output.pbData.cast()))) };
    plaintext
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
fn load_chromium_keys() -> Result<ChromiumKeys, ChromeCookieImportError> {
    Err(ChromeCookieImportError::UnsupportedPlatform)
}

#[cfg(test)]
mod tests {
    #[cfg(not(target_os = "windows"))]
    use cbc::cipher::{BlockEncryptMut as _, block_padding::Pkcs7};
    use rusqlite::params;
    use tempfile::tempdir;
    use zz_browser::MAX_COOKIE_IMPORT_COUNT;

    use super::*;

    #[cfg(not(target_os = "windows"))]
    type Aes128CbcEncryptor = cbc::Encryptor<Aes128>;

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn decrypts_chromium_values_with_the_host_digest() {
        let host = ".example.com";
        let key = derive_key(b"test password", 1003);
        let mut plaintext = Sha256::digest(host.as_bytes()).to_vec();
        plaintext.extend_from_slice(b"session-value");
        let ciphertext = Aes128CbcEncryptor::new(&key.into(), &IV.into())
            .encrypt_padded_vec_mut::<Pkcs7>(&plaintext);
        let mut encrypted = b"v10".to_vec();
        encrypted.extend(ciphertext);
        let keys = ChromiumKeys {
            v10: Zeroizing::new(vec![key]),
            v11: Zeroizing::new(Vec::new()),
            credential_unavailable: false,
        };

        assert_eq!(
            decrypt_cookie_value(&encrypted, host, true, &keys).as_deref(),
            Some("session-value")
        );
        assert_eq!(
            decrypt_cookie_value(&encrypted, "other.example", true, &keys),
            None
        );
    }

    #[test]
    fn imports_plaintext_cookies_and_skips_unsupported_attributes() {
        let directory = tempdir().expect("temporary Chrome profile");
        let database = directory.path().join("Cookies");
        let connection = Connection::open(&database).expect("create cookie database");
        connection
            .execute_batch(
                "CREATE TABLE meta (key LONGVARCHAR NOT NULL UNIQUE PRIMARY KEY, value LONGVARCHAR); \
                 INSERT INTO meta (key, value) VALUES ('version', '24'); \
                 CREATE TABLE cookies ( \
                    host_key TEXT NOT NULL, name TEXT NOT NULL, value TEXT NOT NULL, \
                    path TEXT NOT NULL, expires_utc INTEGER NOT NULL, encrypted_value BLOB NOT NULL, \
                    is_secure INTEGER NOT NULL, is_httponly INTEGER NOT NULL, has_expires INTEGER NOT NULL, \
                    samesite INTEGER NOT NULL, priority INTEGER NOT NULL, source_scheme INTEGER NOT NULL, \
                    source_port INTEGER NOT NULL, top_frame_site_key TEXT NOT NULL, \
                    is_same_party INTEGER NOT NULL \
                 );",
            )
            .expect("create cookie schema");
        let future_expiry = CHROMIUM_EPOCH_OFFSET_MICROS + 4_102_444_800_000_000;
        connection
            .execute(
                "INSERT INTO cookies VALUES (?1, ?2, ?3, '/', ?4, x'', 1, 1, 1, 1, 2, 2, 443, '', 0)",
                params![".example.com", "session", "secret", future_expiry],
            )
            .expect("insert domain cookie");
        connection
            .execute(
                "INSERT INTO cookies VALUES ('app.example.com', 'exact-host-only', 'secret', '/', ?1, x'', 0, 0, 1, -1, 1, 1, 80, '', 0)",
                params![future_expiry],
            )
            .expect("insert exact host-only cookie");
        connection
            .execute(
                "INSERT INTO cookies VALUES ('.example.com', 'same-party', 'secret', '/', ?1, x'', 1, 1, 1, 1, 1, 2, 443, '', 1)",
                params![future_expiry],
            )
            .expect("insert unsupported SameParty cookie");
        drop(connection);

        let batch = import_cookie_database(&database, || {
            panic!("plaintext cookies must not open the credential store")
        })
        .expect("import plaintext cookies");
        assert_eq!(batch.cookies.len(), 2);
        assert_eq!(batch.skipped, 1);
        let cookie = batch
            .cookies
            .iter()
            .find(|cookie| cookie.name == "session")
            .expect("domain cookie");
        assert_eq!(cookie.domain, ".example.com");
        assert!(cookie.secure);
        assert!(cookie.http_only);
        let host_only = batch
            .cookies
            .iter()
            .find(|cookie| cookie.name == "exact-host-only")
            .expect("exact host-only cookie");
        assert_eq!(host_only.domain, "");
    }

    #[test]
    fn imports_all_hosts_and_deduplicates_by_cef_cookie_identity() {
        let directory = tempdir().expect("temporary Chrome profile");
        let database = directory.path().join("Cookies");
        let connection = Connection::open(&database).expect("create cookie database");
        connection
            .execute_batch(
                "CREATE TABLE meta (key LONGVARCHAR NOT NULL UNIQUE PRIMARY KEY, value LONGVARCHAR); \
                 INSERT INTO meta (key, value) VALUES ('version', '24'); \
                 CREATE TABLE cookies ( \
                    host_key TEXT NOT NULL, name TEXT NOT NULL, value TEXT NOT NULL, \
                    path TEXT NOT NULL, expires_utc INTEGER NOT NULL, encrypted_value BLOB NOT NULL, \
                    is_secure INTEGER NOT NULL, is_httponly INTEGER NOT NULL, \
                    top_frame_site_key TEXT NOT NULL, is_partitioned INTEGER NOT NULL \
                 );",
            )
            .expect("create cookie schema");
        let earlier_expiry = CHROMIUM_EPOCH_OFFSET_MICROS + 4_102_444_800_000_000;
        let later_expiry = earlier_expiry + 1_000_000;
        connection
            .execute(
                "INSERT INTO cookies VALUES ('one.example', 'session', 'older', '/', ?1, x'', 1, 1, '', 0)",
                params![earlier_expiry],
            )
            .expect("insert older duplicate");
        connection
            .execute(
                "INSERT INTO cookies VALUES ('one.example', 'session', 'newer', '/', ?1, x'', 1, 1, '', 0)",
                params![later_expiry],
            )
            .expect("insert newer duplicate");
        connection
            .execute(
                "INSERT INTO cookies VALUES ('two.example', 'other', 'value', '/', ?1, x'', 0, 0, '', 0)",
                params![later_expiry],
            )
            .expect("insert other host");
        connection
            .execute(
                "INSERT INTO cookies VALUES ('three.example', 'other', 'another', '/', ?1, x'', 0, 0, '', 0)",
                params![later_expiry],
            )
            .expect("insert same identity fields on another host");
        connection
            .execute(
                "INSERT INTO cookies VALUES ('one.example', 'session', 'partitioned', '/', ?1, x'', 1, 1, 'https://top.example', 1)",
                params![later_expiry + 1_000_000],
            )
            .expect("insert newer partitioned duplicate");
        connection
            .execute(
                "INSERT INTO cookies VALUES ('one.example', 'session-only', 'active', '/', 0, x'', 1, 1, '', 0)",
                [],
            )
            .expect("insert active session cookie");
        connection
            .execute(
                "INSERT INTO cookies VALUES ('one.example', 'session-only', 'expired', '/', ?1, x'', 1, 1, '', 0)",
                params![CHROMIUM_EPOCH_OFFSET_MICROS + 1_000_000],
            )
            .expect("insert expired duplicate");
        drop(connection);

        let batch = import_cookie_database(&database, || {
            panic!("plaintext cookies must not open the credential store")
        })
        .expect("import all cookies");

        assert_eq!(batch.cookies.len(), 4);
        assert_eq!(batch.skipped, 3);
        assert_eq!(
            batch
                .cookies
                .iter()
                .find(|cookie| cookie.name == "session")
                .expect("deduplicated cookie")
                .value,
            "newer"
        );
        assert_eq!(
            batch
                .cookies
                .iter()
                .find(|cookie| cookie.name == "session-only")
                .expect("active session cookie")
                .value,
            "active"
        );
        assert!(batch.cookies.iter().any(|cookie| {
            cookie.domain.is_empty() && cookie.source_url.contains("two.example")
        }));
        assert!(batch.cookies.iter().any(|cookie| {
            cookie.domain.is_empty() && cookie.source_url.contains("three.example")
        }));
    }

    #[test]
    fn normalizes_installed_profile_batches_larger_than_the_file_import_limit() {
        let records = (0..=MAX_COOKIE_IMPORT_COUNT)
            .map(|index| {
                serde_json::json!({
                    "name": format!("cookie-{index}"),
                    "value": "value",
                    "domain": "example.com",
                    "path": "/"
                })
            })
            .collect::<Vec<_>>();

        let batch = normalize_cookie_records(&records, 0).expect("normalize all records");
        assert_eq!(batch.cookies.len(), MAX_COOKIE_IMPORT_COUNT + 1);
        assert_eq!(batch.skipped, 0);
    }

    #[test]
    fn snapshots_a_live_write_ahead_log() {
        let directory = tempdir().expect("temporary Chrome profile");
        let database = directory.path().join("Cookies");
        let connection = Connection::open(&database).expect("create cookie database");
        let journal_mode = connection
            .query_row("PRAGMA journal_mode=WAL", [], |row| row.get::<_, String>(0))
            .expect("enable WAL mode");
        assert_eq!(journal_mode, "wal");
        connection
            .execute_batch(
                "PRAGMA wal_autocheckpoint=0; \
                 CREATE TABLE meta (key LONGVARCHAR NOT NULL UNIQUE PRIMARY KEY, value LONGVARCHAR); \
                 INSERT INTO meta (key, value) VALUES ('version', '24'); \
                 CREATE TABLE cookies ( \
                    host_key TEXT NOT NULL, name TEXT NOT NULL, value TEXT NOT NULL, \
                    path TEXT NOT NULL, expires_utc INTEGER NOT NULL, encrypted_value BLOB NOT NULL, \
                    is_secure INTEGER NOT NULL, is_httponly INTEGER NOT NULL \
                 ); \
                 INSERT INTO cookies VALUES ('example.com', 'live', 'value', '/', 0, x'', 1, 1);",
            )
            .expect("populate active WAL");

        let batch = import_cookie_database(&database, || {
            panic!("plaintext cookies must not open the credential store")
        })
        .expect("snapshot active WAL");
        assert_eq!(batch.cookies.len(), 1);
        assert_eq!(batch.cookies[0].name, "live");
        assert_eq!(batch.cookies[0].value, "value");
    }

    #[test]
    fn maps_zz_profiles_to_safe_chrome_storage_keys() {
        assert_eq!(
            source_storage_key(zz_browser::DEFAULT_BROWSER_PROFILE).expect("default source"),
            DEFAULT_CHROME_PROFILE
        );
        assert_eq!(
            source_storage_key("chrome:Profile 3").expect("named source"),
            "Profile 3"
        );
        assert!(source_storage_key("chrome:../Default").is_err());
        assert_eq!(
            source_storage_key("My zz profile").expect("custom source"),
            DEFAULT_CHROME_PROFILE
        );
    }

    #[test]
    fn identifies_permission_denied_io_errors() {
        let denied = ChromeCookieImportError::from(std::io::Error::from(
            std::io::ErrorKind::PermissionDenied,
        ));
        let missing =
            ChromeCookieImportError::from(std::io::Error::from(std::io::ErrorKind::NotFound));

        assert!(denied.is_permission_denied());
        assert!(!missing.is_permission_denied());
    }
}
