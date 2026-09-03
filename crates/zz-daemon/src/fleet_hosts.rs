//! Discovery and parsing for client-side fleet host entries in `zz/config`.

use std::{
    env,
    fs::{self, File, OpenOptions},
    io::{self, ErrorKind, Read as _, Write as _},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use crate::Endpoint;

const CONFIG_DIRECTORY_NAME: &str = "zz";
const CONFIG_FILE_NAME: &str = "config";
const MAX_CONFIG_BYTES: usize = 64 * 1024;
static CONFIG_TEMP_FILE_COUNTER: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HostEntry {
    pub name: String,
    pub endpoint: Endpoint,
}

/// A `host-<name>` line that could not become a [`HostEntry`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RejectedHost {
    pub name: String,
    pub value: String,
    pub reason: String,
}

/// Read every configured fleet host from the first discovered `zz/config`.
pub fn configured_fleet_hosts() -> io::Result<(Vec<HostEntry>, Vec<RejectedHost>)> {
    let candidates = config_candidates();
    let Some(path) = discover_config_path(&candidates) else {
        return Ok((Vec::new(), Vec::new()));
    };
    read_config_source(&path).map(|source| parse_fleet_hosts(&source))
}

/// Validate one `host-<name>` entry and write it to `zz/config` with an atomic rename.
pub fn write_fleet_host(name: &str, endpoint: &str) -> io::Result<()> {
    let path = config_path_for_write()?;
    write_fleet_host_at(&path, name, endpoint)
}

/// Apply one `host-<name>` entry, returning its diagnostic text when invalid
/// or when it replaces an earlier entry.
pub fn apply_fleet_host_entry(
    hosts: &mut Vec<HostEntry>,
    rejected: &mut Vec<RejectedHost>,
    key: &str,
    name: &str,
    value: &str,
) -> Option<String> {
    let message = if name.is_empty() {
        Some("host name must not be empty".to_owned())
    } else if name.chars().any(char::is_whitespace) {
        Some("host name must not contain whitespace".to_owned())
    } else if name == "local" {
        Some("host name `local` is reserved".to_owned())
    } else {
        None
    };
    if let Some(message) = message {
        rejected.push(RejectedHost {
            name: name.to_owned(),
            value: value.to_owned(),
            reason: message.clone(),
        });
        return Some(format!("invalid `{key}`: {message}"));
    }

    let endpoint = match Endpoint::parse(value) {
        Ok(endpoint) => endpoint,
        Err(error) => {
            rejected.push(RejectedHost {
                name: name.to_owned(),
                value: value.to_owned(),
                reason: error.to_string(),
            });
            return Some(format!("invalid `{key}`: {error}"));
        }
    };
    let entry = HostEntry {
        name: name.to_owned(),
        endpoint,
    };
    let Some(index) = hosts.iter().position(|host| host.name == name) else {
        hosts.push(entry);
        return None;
    };
    hosts.remove(index);
    hosts.push(entry);
    Some(format!("duplicate host `{name}`; last entry wins"))
}

pub fn validate_fleet_host(name: &str, endpoint: &str) -> Result<(), String> {
    let key = format!("host-{name}");
    apply_fleet_host_entry(&mut Vec::new(), &mut Vec::new(), &key, name, endpoint)
        .map_or(Ok(()), Err)
}

fn parse_fleet_hosts(source: &str) -> (Vec<HostEntry>, Vec<RejectedHost>) {
    let mut hosts = Vec::new();
    let mut rejected = Vec::new();
    for raw_line in source.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        let Some(name) = key.strip_prefix("host-") else {
            continue;
        };
        let value = config_value_without_comment(value).trim();
        let _ = apply_fleet_host_entry(&mut hosts, &mut rejected, key, name, value);
    }
    (hosts, rejected)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ConfigPlatform {
    Unix,
    Macos,
    Windows,
}

#[derive(Clone, Copy, Debug, Default)]
struct ConfigEnvironment<'a> {
    xdg_config_home: Option<&'a Path>,
    home: Option<&'a Path>,
    appdata: Option<&'a Path>,
    local_appdata: Option<&'a Path>,
    user_profile: Option<&'a Path>,
}

fn current_config_platform() -> ConfigPlatform {
    if cfg!(target_os = "macos") {
        ConfigPlatform::Macos
    } else if cfg!(target_os = "windows") {
        ConfigPlatform::Windows
    } else {
        ConfigPlatform::Unix
    }
}

fn config_candidates() -> Vec<PathBuf> {
    let xdg_config_home = nonempty_env("XDG_CONFIG_HOME");
    let home = nonempty_env("HOME");
    let appdata = nonempty_env("APPDATA");
    let local_appdata = nonempty_env("LOCALAPPDATA");
    let user_profile = nonempty_env("USERPROFILE");
    config_candidates_for(
        current_config_platform(),
        ConfigEnvironment {
            xdg_config_home: xdg_config_home.as_deref(),
            home: home.as_deref(),
            appdata: appdata.as_deref(),
            local_appdata: local_appdata.as_deref(),
            user_profile: user_profile.as_deref(),
        },
    )
}

fn config_candidates_for(
    platform: ConfigPlatform,
    environment: ConfigEnvironment<'_>,
) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    push_config_candidate(&mut candidates, environment.xdg_config_home);
    match platform {
        ConfigPlatform::Unix => push_home_config_candidate(&mut candidates, environment.home),
        ConfigPlatform::Macos => {
            push_home_config_candidate(&mut candidates, environment.home);
            if let Some(home) = environment.home {
                push_config_candidate_path(
                    &mut candidates,
                    &home.join("Library/Application Support"),
                );
            }
        }
        ConfigPlatform::Windows => {
            push_config_candidate(&mut candidates, environment.appdata);
            push_config_candidate(&mut candidates, environment.local_appdata);
            push_home_config_candidate(&mut candidates, environment.user_profile);
            push_home_config_candidate(&mut candidates, environment.home);
        }
    }
    candidates
}

fn push_home_config_candidate(candidates: &mut Vec<PathBuf>, home: Option<&Path>) {
    if let Some(home) = home {
        push_config_candidate_path(candidates, &home.join(".config"));
    }
}

fn push_config_candidate(candidates: &mut Vec<PathBuf>, base: Option<&Path>) {
    if let Some(base) = base {
        push_config_candidate_path(candidates, base);
    }
}

fn push_config_candidate_path(candidates: &mut Vec<PathBuf>, base: &Path) {
    if !base.is_absolute() {
        return;
    }
    let candidate = base.join(CONFIG_DIRECTORY_NAME).join(CONFIG_FILE_NAME);
    if !candidates.contains(&candidate) {
        candidates.push(candidate);
    }
}

fn discover_config_path(candidates: &[PathBuf]) -> Option<PathBuf> {
    candidates
        .iter()
        .find(|candidate| candidate.is_file())
        .cloned()
}

fn preferred_config_creation_path(
    xdg_config_home: Option<&Path>,
    home: Option<&Path>,
) -> Option<PathBuf> {
    let mut candidates = Vec::new();
    push_config_candidate(&mut candidates, xdg_config_home);
    if candidates.is_empty() {
        push_home_config_candidate(&mut candidates, home);
    }
    candidates.into_iter().next()
}

fn config_path_for_write() -> io::Result<PathBuf> {
    let candidates = config_candidates();
    if let Some(path) = discover_config_path(&candidates) {
        return Ok(path);
    }

    let xdg_config_home = nonempty_env("XDG_CONFIG_HOME");
    let home = nonempty_env("HOME");
    preferred_config_creation_path(xdg_config_home.as_deref(), home.as_deref()).ok_or_else(|| {
        io::Error::new(
            ErrorKind::NotFound,
            "cannot create zz/config because neither XDG_CONFIG_HOME nor HOME is available",
        )
    })
}

fn nonempty_env(name: &str) -> Option<PathBuf> {
    env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn read_config_source(path: &Path) -> io::Result<String> {
    let file = File::open(path)?;
    let byte_limit = u64::try_from(MAX_CONFIG_BYTES).unwrap_or(u64::MAX - 1);
    let mut source = String::new();
    file.take(byte_limit + 1).read_to_string(&mut source)?;
    if source.len() > MAX_CONFIG_BYTES {
        return Err(io::Error::new(
            ErrorKind::InvalidData,
            format!("configuration exceeds the {MAX_CONFIG_BYTES}-byte limit"),
        ));
    }
    Ok(source)
}

fn write_fleet_host_at(path: &Path, name: &str, endpoint: &str) -> io::Result<()> {
    validate_fleet_host(name, endpoint)
        .map_err(|message| io::Error::new(ErrorKind::InvalidInput, message))?;
    let source = match read_config_source(path) {
        Ok(source) => source,
        Err(error) if error.kind() == ErrorKind::NotFound => String::new(),
        Err(error) => return Err(error),
    };
    let edited = edit_config_source(&source, &format!("host-{name}"), endpoint);
    if edited.len() > MAX_CONFIG_BYTES {
        return Err(io::Error::new(
            ErrorKind::InvalidData,
            format!("configuration edit exceeds the {MAX_CONFIG_BYTES}-byte limit"),
        ));
    }
    atomic_write(path, edited.as_bytes())
}

fn edit_config_source(source: &str, key: &str, value: &str) -> String {
    let Some(line) = last_key_line_range(source, key) else {
        return append_config_line(source, key, value);
    };
    let replacement = replace_line_value(&source[line.clone()], value);
    let mut edited =
        String::with_capacity(source.len() + replacement.len().saturating_sub(line.len()));
    edited.push_str(&source[..line.start]);
    edited.push_str(&replacement);
    edited.push_str(&source[line.end..]);
    edited
}

fn last_key_line_range(source: &str, key: &str) -> Option<std::ops::Range<usize>> {
    let mut offset = 0;
    let mut last = None;
    for line in source.split_inclusive('\n') {
        let end = offset + line.len();
        if config_key_for_line(line) == Some(key) {
            last = Some(offset..end);
        }
        offset = end;
    }
    last
}

fn config_key_for_line(line: &str) -> Option<&str> {
    let line = line.strip_suffix('\n').unwrap_or(line);
    let line = line.strip_suffix('\r').unwrap_or(line);
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    let (key, _) = line.split_once('=')?;
    Some(key.trim())
}

fn replace_line_value(line: &str, value: &str) -> String {
    let without_lf = line.strip_suffix('\n').unwrap_or(line);
    let body = without_lf.strip_suffix('\r').unwrap_or(without_lf);
    let equals = body
        .find('=')
        .expect("a matched configuration line has an equals sign");
    let value_area_start = equals + 1;
    let comment_start = config_comment_start(&body[value_area_start..])
        .map_or(body.len(), |index| value_area_start + index);
    let value_area = &body[value_area_start..comment_start];
    let (value_start, value_end) = if value_area.trim().is_empty() {
        (comment_start, comment_start)
    } else {
        (
            value_area_start + value_area.len() - value_area.trim_start().len(),
            value_area_start + value_area.trim_end().len(),
        )
    };

    let mut replacement = String::with_capacity(line.len() + value.len());
    replacement.push_str(&line[..value_start]);
    replacement.push_str(value);
    replacement.push_str(&line[value_end..]);
    replacement
}

fn append_config_line(source: &str, key: &str, value: &str) -> String {
    let newline = if source.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let mut edited = String::with_capacity(source.len() + key.len() + value.len() + 4);
    edited.push_str(source);
    if !source.is_empty() && !source.ends_with('\n') {
        edited.push_str(newline);
    }
    edited.push_str(key);
    edited.push_str(" = ");
    edited.push_str(value);
    edited.push_str(newline);
    edited
}

pub(crate) fn atomic_write(path: &Path, contents: &[u8]) -> io::Result<()> {
    let parent = path.parent().ok_or_else(|| {
        io::Error::new(
            ErrorKind::InvalidInput,
            "configuration path has no parent directory",
        )
    })?;
    fs::create_dir_all(parent)?;
    let (temporary_path, mut temporary_file) = create_config_temp_file(path, parent)?;
    let write_result = (|| {
        if let Ok(metadata) = fs::metadata(path) {
            temporary_file.set_permissions(metadata.permissions())?;
        }
        temporary_file.write_all(contents)?;
        temporary_file.sync_all()
    })();
    drop(temporary_file);

    if let Err(error) = write_result {
        let _ = fs::remove_file(&temporary_path);
        return Err(error);
    }
    if let Err(error) = fs::rename(&temporary_path, path) {
        let _ = fs::remove_file(&temporary_path);
        return Err(error);
    }
    Ok(())
}

fn create_config_temp_file(path: &Path, parent: &Path) -> io::Result<(PathBuf, File)> {
    let file_name = path
        .file_name()
        .unwrap_or_else(|| std::ffi::OsStr::new(CONFIG_FILE_NAME))
        .to_string_lossy();
    for _ in 0..128 {
        let nonce = CONFIG_TEMP_FILE_COUNTER.fetch_add(1, Ordering::Relaxed);
        let temporary_path =
            parent.join(format!(".{file_name}.tmp-{}-{nonce}", std::process::id()));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary_path)
        {
            Ok(file) => return Ok((temporary_path, file)),
            Err(error) if error.kind() == ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        ErrorKind::AlreadyExists,
        "could not allocate a unique temporary configuration file",
    ))
}

fn config_value_without_comment(value: &str) -> &str {
    config_comment_start(value).map_or(value, |index| &value[..index])
}

fn config_comment_start(value: &str) -> Option<usize> {
    let mut quote = None;
    let mut escaped = false;
    let mut saw_value = false;
    let mut previous_was_whitespace = false;
    for (index, character) in value.char_indices() {
        if escaped {
            escaped = false;
            saw_value |= !character.is_whitespace();
            previous_was_whitespace = character.is_whitespace();
            continue;
        }
        match character {
            '\\' if quote.is_some() => escaped = true,
            '"' | '\'' if quote == Some(character) => quote = None,
            '"' | '\'' if quote.is_none() => quote = Some(character),
            '#' if quote.is_none() && saw_value && previous_was_whitespace => return Some(index),
            _ => {}
        }
        saw_value |= !character.is_whitespace();
        previous_was_whitespace = character.is_whitespace();
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SshEndpoint;

    fn absolute_test_root(name: &str) -> PathBuf {
        env::current_dir()
            .expect("current directory")
            .join("target")
            .join("fleet-host-path-tests")
            .join(name)
    }

    fn expected_config_path(base: &Path) -> PathBuf {
        base.join(CONFIG_DIRECTORY_NAME).join(CONFIG_FILE_NAME)
    }

    #[test]
    fn hosts_parse_in_config_order_and_ignore_other_entries() {
        let (hosts, rejected) = parse_fleet_hosts(
            "host-desktop = ssh://fabrico@desktop:2222\n\
             background = #101010\n\
             host-scratch = unix:///tmp/zz-scratch.sock\n\
             host-legacy = /tmp/zz-legacy.sock\n",
        );

        assert_eq!(
            hosts,
            [
                HostEntry {
                    name: "desktop".to_owned(),
                    endpoint: Endpoint::Ssh(SshEndpoint {
                        user: Some("fabrico".to_owned()),
                        host: "desktop".to_owned(),
                        port: Some(2222),
                        remote_socket: None,
                    }),
                },
                HostEntry {
                    name: "scratch".to_owned(),
                    endpoint: Endpoint::parse("unix:///tmp/zz-scratch.sock").unwrap(),
                },
                HostEntry {
                    name: "legacy".to_owned(),
                    endpoint: Endpoint::parse("/tmp/zz-legacy.sock").unwrap(),
                },
            ]
        );
        assert!(rejected.is_empty());
    }

    #[test]
    fn invalid_and_duplicate_hosts_keep_the_existing_error_contract() {
        let (hosts, rejected) = parse_fleet_hosts(
            "host-desktop = ssh://old\n\
             host-broken = quic://gpu:7777\n\
             host-desktop = ssh://new # effective\n\
             host-local = ssh://reserved\n",
        );

        assert_eq!(
            hosts,
            [HostEntry {
                name: "desktop".to_owned(),
                endpoint: Endpoint::parse("ssh://new").unwrap(),
            }]
        );
        assert_eq!(rejected.len(), 2);
        assert_eq!(rejected[0].name, "broken");
        assert_eq!(
            rejected[0].reason,
            "invalid endpoint URI `quic://gpu:7777`: quic endpoints were removed; use ssh://"
        );
        assert_eq!(rejected[1].name, "local");
        assert_eq!(rejected[1].reason, "host name `local` is reserved");
        assert_eq!(
            validate_fleet_host("bad name", "ssh://desktop"),
            Err("invalid `host-bad name`: host name must not contain whitespace".to_owned())
        );
    }

    #[test]
    fn candidate_order_matches_each_client_platform() {
        let root = absolute_test_root("candidate-order");
        let xdg = root.join("xdg");
        let home = root.join("home");
        let appdata = root.join("appdata").join("roaming");
        let local_appdata = root.join("appdata").join("local");
        let user_profile = root.join("user-profile");
        let environment = ConfigEnvironment {
            xdg_config_home: Some(&xdg),
            home: Some(&home),
            appdata: Some(&appdata),
            local_appdata: Some(&local_appdata),
            user_profile: Some(&user_profile),
        };

        assert_eq!(
            config_candidates_for(ConfigPlatform::Unix, environment),
            [
                expected_config_path(&xdg),
                expected_config_path(&home.join(".config")),
            ]
        );
        assert_eq!(
            config_candidates_for(ConfigPlatform::Macos, environment),
            [
                expected_config_path(&xdg),
                expected_config_path(&home.join(".config")),
                expected_config_path(&home.join("Library").join("Application Support")),
            ]
        );
        assert_eq!(
            config_candidates_for(ConfigPlatform::Windows, environment),
            [
                expected_config_path(&xdg),
                expected_config_path(&appdata),
                expected_config_path(&local_appdata),
                expected_config_path(&user_profile.join(".config")),
                expected_config_path(&home.join(".config")),
            ]
        );
    }

    #[test]
    fn bounded_reader_rejects_oversized_configuration() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join(CONFIG_FILE_NAME);
        std::fs::write(&path, vec![b' '; MAX_CONFIG_BYTES + 1]).unwrap();

        let error = read_config_source(&path).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::InvalidData);
    }

    #[test]
    fn writer_appends_to_a_new_config_file() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("zz/config");

        write_fleet_host_at(&path, "box", "ssh://user@box:2222").unwrap();

        assert_eq!(
            fs::read_to_string(path).unwrap(),
            "host-box = ssh://user@box:2222\n"
        );
    }

    #[test]
    fn writer_replaces_the_last_line_and_preserves_surrounding_bytes() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join(CONFIG_FILE_NAME);
        let source = "# keep this comment\r\n\
                      host-box = ssh://first\r\n\
                      show-fps = true\r\n\
                      host-box  = ssh://old  # keep this too\r\n";
        fs::write(&path, source).unwrap();

        write_fleet_host_at(&path, "box", "ssh://new:9922").unwrap();

        assert_eq!(
            fs::read_to_string(path).unwrap(),
            "# keep this comment\r\n\
             host-box = ssh://first\r\n\
             show-fps = true\r\n\
             host-box  = ssh://new:9922  # keep this too\r\n"
        );
    }

    #[test]
    fn writer_rejects_edits_over_the_config_limit() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join(CONFIG_FILE_NAME);
        fs::write(&path, vec![b' '; MAX_CONFIG_BYTES]).unwrap();

        let error = write_fleet_host_at(&path, "box", "ssh://box").unwrap_err();

        assert_eq!(error.kind(), ErrorKind::InvalidData);
        assert_eq!(
            error.to_string(),
            format!("configuration edit exceeds the {MAX_CONFIG_BYTES}-byte limit")
        );
    }

    #[test]
    fn writer_reuses_the_existing_validation_errors() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join(CONFIG_FILE_NAME);

        let name = write_fleet_host_at(&path, "local", "ssh://box").unwrap_err();
        assert_eq!(name.kind(), ErrorKind::InvalidInput);
        assert_eq!(
            name.to_string(),
            "invalid `host-local`: host name `local` is reserved"
        );

        let endpoint = write_fleet_host_at(&path, "box", "quic://box:7777").unwrap_err();
        assert_eq!(endpoint.kind(), ErrorKind::InvalidInput);
        assert_eq!(
            endpoint.to_string(),
            "invalid `host-box`: invalid endpoint URI `quic://box:7777`: quic endpoints were \
             removed; use ssh://"
        );
    }
}
