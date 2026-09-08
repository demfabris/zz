use std::time::Duration;

use semver::Version;
use serde::Deserialize;

const RELEASES_API: &str = "https://api.github.com/repos/demfabris/zz/releases?per_page=10";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Channel {
    Stable,
    Beta,
}

impl Channel {
    pub fn of(version: &Version) -> Self {
        if version.pre.is_empty() {
            Self::Stable
        } else {
            Self::Beta
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Stable => "stable",
            Self::Beta => "beta",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Release {
    pub version: Version,
    pub url: String,
    pub assets: Vec<ReleaseAsset>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct ReleaseAsset {
    pub name: String,
    #[serde(rename = "browser_download_url")]
    pub url: String,
}

pub fn build_version() -> Result<Version, String> {
    Version::parse(env!("CARGO_PKG_VERSION")).map_err(|error| error.to_string())
}

pub fn checks_enabled() -> bool {
    match std::env::var("ZZ_UPDATE_CHECK") {
        Ok(value) => !matches!(value.as_str(), "" | "0" | "false" | "off"),
        Err(_) => !cfg!(debug_assertions),
    }
}

pub fn fetch_latest(channel: Channel) -> Result<Option<Release>, String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(REQUEST_TIMEOUT))
        .build()
        .into();
    let mut response = agent
        .get(RELEASES_API)
        .header("User-Agent", concat!("zz/", env!("CARGO_PKG_VERSION")))
        .header("Accept", "application/vnd.github+json")
        .call()
        .map_err(|error| error.to_string())?;
    let body = response
        .body_mut()
        .read_to_string()
        .map_err(|error| error.to_string())?;
    parse_releases(&body, channel)
}

#[derive(Deserialize)]
struct ReleaseEntry {
    tag_name: String,
    html_url: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    assets: Vec<ReleaseAsset>,
}

pub fn parse_releases(json: &str, channel: Channel) -> Result<Option<Release>, String> {
    let entries: Vec<ReleaseEntry> =
        serde_json::from_str(json).map_err(|error| format!("unexpected release list: {error}"))?;
    Ok(entries
        .into_iter()
        .filter(|entry| !entry.draft)
        .filter_map(|entry| {
            let version = Version::parse(entry.tag_name.strip_prefix('v')?).ok()?;
            let stable = !entry.prerelease && version.pre.is_empty();
            (channel == Channel::Beta || stable).then_some(Release {
                version,
                url: entry.html_url,
                assets: entry.assets,
            })
        })
        .max_by(|a, b| a.version.cmp(&b.version)))
}

pub fn native_macos_asset<'a>(
    release: &'a Release,
    architecture: &str,
) -> Option<&'a ReleaseAsset> {
    let architecture = match architecture {
        "aarch64" => "arm64",
        "x86_64" => "x64",
        _ => return None,
    };
    let name = format!("zz-native-{}-macos-{architecture}.dmg", release.version);
    let url = format!(
        "https://github.com/demfabris/zz/releases/download/v{}/{name}",
        release.version
    );
    release
        .assets
        .iter()
        .find(|asset| asset.name == name && asset.url == url)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn version(text: &str) -> Version {
        Version::parse(text).expect("valid version")
    }

    fn release(tag: &str, prerelease: bool, draft: bool) -> String {
        format!(
            r#"{{"tag_name":"{tag}","html_url":"https://github.com/demfabris/zz/releases/tag/{tag}","draft":{draft},"prerelease":{prerelease}}}"#
        )
    }

    fn releases(entries: &[String]) -> String {
        format!("[{}]", entries.join(","))
    }

    #[test]
    fn channel_follows_the_running_build() {
        assert_eq!(Channel::of(&version("0.3.1")), Channel::Stable);
        assert_eq!(Channel::of(&version("0.3.2-beta.1")), Channel::Beta);
    }

    #[test]
    fn stable_channel_skips_prereleases_and_drafts() {
        let json = releases(&[
            release("v0.4.0", false, true),
            release("v0.3.2-beta.1", true, false),
            release("v0.3.1", false, false),
            release("v0.3.0", false, false),
        ]);
        let newest = parse_releases(&json, Channel::Stable)
            .expect("parses")
            .expect("finds a release");
        assert_eq!(newest.version, version("0.3.1"));
        assert_eq!(
            newest.url,
            "https://github.com/demfabris/zz/releases/tag/v0.3.1"
        );
    }

    #[test]
    fn stable_channel_distrusts_an_unflagged_prerelease_tag() {
        let json = releases(&[
            release("v0.3.2-beta.1", false, false),
            release("v0.3.1", false, false),
        ]);
        let newest = parse_releases(&json, Channel::Stable).unwrap().unwrap();
        assert_eq!(newest.version, version("0.3.1"));
    }

    #[test]
    fn beta_channel_takes_the_newest_of_everything() {
        let json = releases(&[
            release("v0.3.1", false, false),
            release("v0.3.2-beta.1", true, false),
            release("v0.3.2-beta.2", true, false),
        ]);
        let newest = parse_releases(&json, Channel::Beta).unwrap().unwrap();
        assert_eq!(newest.version, version("0.3.2-beta.2"));

        let promoted = releases(&[
            release("v0.3.2", false, false),
            release("v0.3.2-beta.2", true, false),
        ]);
        let newest = parse_releases(&promoted, Channel::Beta).unwrap().unwrap();
        assert_eq!(newest.version, version("0.3.2"));
    }

    #[test]
    fn foreign_tags_and_bad_json_are_handled() {
        let json = releases(&[
            release("nightly", false, false),
            release("v0.3.1", false, false),
        ]);
        let newest = parse_releases(&json, Channel::Stable).unwrap().unwrap();
        assert_eq!(newest.version, version("0.3.1"));
        assert_eq!(parse_releases("[]", Channel::Stable), Ok(None));
        assert!(parse_releases(r#"{"message":"rate limited"}"#, Channel::Stable).is_err());
    }

    #[test]
    fn native_assets_require_the_native_product_architecture_and_release_url() {
        let mut release = Release {
            version: version("0.3.2"),
            url: "https://github.com/demfabris/zz/releases/tag/v0.3.2".to_owned(),
            assets: Vec::new(),
        };
        let asset = |name: &str| ReleaseAsset {
            name: name.to_owned(),
            url: format!("https://github.com/demfabris/zz/releases/download/v0.3.2/{name}"),
        };
        release.assets.push(asset("zz-0.3.2-macos-arm64.dmg"));
        assert_eq!(native_macos_asset(&release, "aarch64"), None);
        release.assets.push(asset("zz-native-0.3.2-macos-x64.dmg"));
        assert_eq!(native_macos_asset(&release, "aarch64"), None);
        let mut native = asset("zz-native-0.3.2-macos-arm64.dmg");
        native.url = "https://example.com/wrong-product.dmg".to_owned();
        release.assets.push(native);
        assert_eq!(native_macos_asset(&release, "aarch64"), None);
        release
            .assets
            .push(asset("zz-native-0.3.2-macos-arm64.dmg"));
        assert_eq!(
            native_macos_asset(&release, "aarch64"),
            release.assets.last()
        );
        assert_eq!(native_macos_asset(&release, "unknown"), None);
    }
}
