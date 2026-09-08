use std::net::Ipv4Addr;

use thiserror::Error;
use url::{Host, Url, form_urlencoded};

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum UrlInputError {
    #[error("enter a URL")]
    Empty,
    #[error("only http, https, file, and about:blank are supported")]
    UnsupportedScheme,
    #[error("the address is not a valid URL")]
    Invalid,
}

/// The engine an address-field entry that is not a URL is searched on.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum SearchProvider {
    #[default]
    Google,
    DuckDuckGo,
    Brave,
}

impl SearchProvider {
    pub const ALL: [Self; 3] = [Self::Google, Self::DuckDuckGo, Self::Brave];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Google => "google",
            Self::DuckDuckGo => "duckduckgo",
            Self::Brave => "brave",
        }
    }

    pub const fn title(self) -> &'static str {
        match self {
            Self::Google => "Google",
            Self::DuckDuckGo => "DuckDuckGo",
            Self::Brave => "Brave",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|provider| provider.as_str() == value.trim())
    }

    const fn endpoint(self) -> &'static str {
        match self {
            Self::Google => "https://www.google.com/search",
            Self::DuckDuckGo => "https://duckduckgo.com/",
            Self::Brave => "https://search.brave.com/search",
        }
    }

    pub fn search_url(self, query: &str) -> String {
        let mut url = format!("{}?q=", self.endpoint());
        url.extend(form_urlencoded::byte_serialize(query.trim().as_bytes()));
        url
    }
}

/// Turn an address-field entry into a navigation target the way an omnibox
/// does: a URL when the entry looks like one, a `provider` search otherwise.
/// An explicit scheme the runtime cannot open is an error, not a query.
pub fn resolve_address(input: &str, provider: SearchProvider) -> Result<String, UrlInputError> {
    let input = input.trim();
    if input.is_empty() {
        return Err(UrlInputError::Empty);
    }
    if has_explicit_scheme(input) {
        return normalize_url(input);
    }
    if looks_like_host(input)
        && let Ok(url) = normalize_url(input)
    {
        return Ok(url);
    }
    Ok(provider.search_url(input))
}

fn looks_like_host(input: &str) -> bool {
    if input.chars().any(char::is_whitespace) {
        return false;
    }
    let authority = input.split(['/', '?', '#']).next().unwrap_or_default();
    let authority = authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host);
    if let Some(literal) = authority.strip_prefix('[') {
        return literal.contains(']');
    }
    let (host, port) = authority.split_once(':').unwrap_or((authority, ""));
    if !port.is_empty() && port.bytes().all(|byte| byte.is_ascii_digit()) {
        return true;
    }
    if host.eq_ignore_ascii_case("localhost")
        || host.to_ascii_lowercase().ends_with(".localhost")
        || host.parse::<Ipv4Addr>().is_ok()
    {
        return true;
    }
    let Some((name, tld)) = host.rsplit_once('.') else {
        return false;
    };
    !name.is_empty() && tld.chars().count() >= 2 && tld.chars().all(char::is_alphabetic)
}

/// Normalize an address-field value into an allowed browser URL.
pub fn normalize_url(input: &str) -> Result<String, UrlInputError> {
    let input = input.trim();
    if input.is_empty() {
        return Err(UrlInputError::Empty);
    }
    if input.eq_ignore_ascii_case("about:blank") {
        return Ok("about:blank".to_owned());
    }

    let candidate = if has_explicit_scheme(input) {
        input.to_owned()
    } else {
        format!("https://{input}")
    };
    let mut parsed = Url::parse(&candidate).map_err(|_| UrlInputError::Invalid)?;
    if !matches!(parsed.scheme(), "http" | "https" | "file") {
        return Err(UrlInputError::UnsupportedScheme);
    }
    if parsed.scheme() != "file" && parsed.host_str().is_none() {
        return Err(UrlInputError::Invalid);
    }
    if !has_explicit_scheme(input) && defaults_to_http(&parsed) {
        parsed
            .set_scheme("http")
            .map_err(|()| UrlInputError::Invalid)?;
    }
    Ok(parsed.into())
}

fn has_explicit_scheme(input: &str) -> bool {
    if input.contains("://") {
        return true;
    }
    let Some((scheme, remainder)) = input.split_once(':') else {
        return false;
    };
    let mut characters = scheme.chars();
    if !characters
        .next()
        .is_some_and(|character| character.is_ascii_alphabetic())
        || !characters.all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '+' | '-' | '.')
        })
    {
        return false;
    }

    let port = remainder.split(['/', '?', '#']).next().unwrap_or_default();
    port.is_empty() || !port.bytes().all(|byte| byte.is_ascii_digit())
}

fn defaults_to_http(url: &Url) -> bool {
    match url.host() {
        Some(Host::Domain(host)) => {
            host.eq_ignore_ascii_case("localhost")
                || host.to_ascii_lowercase().ends_with(".localhost")
        }
        Some(Host::Ipv4(address)) => address.is_loopback() || address.is_unspecified(),
        Some(Host::Ipv6(address)) => address.is_loopback() || address.is_unspecified(),
        None => false,
    }
}

/// Remove URL components that should never appear in diagnostics.
#[must_use]
pub fn diagnostic_url(input: &str) -> String {
    let Ok(mut parsed) = Url::parse(input) else {
        return "<invalid URL>".to_owned();
    };
    if parsed.scheme() != "about" {
        let _ = parsed.set_username("");
        let _ = parsed.set_password(None);
    }
    parsed.set_query(None);
    parsed.set_fragment(None);
    parsed.into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_addresses() {
        assert_eq!(
            normalize_url(" example.com/path ").unwrap(),
            "https://example.com/path"
        );
        assert_eq!(normalize_url("about:blank").unwrap(), "about:blank");
        assert_eq!(normalize_url("file:///tmp/a").unwrap(), "file:///tmp/a");
        assert_eq!(normalize_url("   "), Err(UrlInputError::Empty));
    }

    #[test]
    fn defaults_local_development_addresses_to_http() {
        for (input, expected) in [
            ("localhost:3000", "http://localhost:3000/"),
            ("app.localhost:5173/path", "http://app.localhost:5173/path"),
            ("127.0.0.1:8080", "http://127.0.0.1:8080/"),
            ("[::1]:9000", "http://[::1]:9000/"),
            ("0.0.0.0:4000", "http://0.0.0.0:4000/"),
        ] {
            assert_eq!(normalize_url(input).as_deref(), Ok(expected));
        }
    }

    #[test]
    fn defaults_public_hosts_to_https_and_preserves_explicit_schemes() {
        assert_eq!(
            normalize_url("example.com:8443").as_deref(),
            Ok("https://example.com:8443/")
        );
        assert_eq!(
            normalize_url("https://localhost:3000").as_deref(),
            Ok("https://localhost:3000/")
        );
        assert_eq!(
            normalize_url("http://example.com").as_deref(),
            Ok("http://example.com/")
        );
        assert_eq!(
            normalize_url("javascript:alert(1)"),
            Err(UrlInputError::UnsupportedScheme)
        );
    }

    #[test]
    fn navigates_to_anything_that_looks_like_an_address() {
        for input in [
            "example.com",
            "example.com/path?q=1",
            "sub.example.co.uk",
            "localhost:3000",
            "nas:5000",
            "127.0.0.1",
            "[::1]:9000",
            "user@example.com/inbox",
            "https://example.com",
        ] {
            assert_eq!(
                resolve_address(input, SearchProvider::Google).ok(),
                normalize_url(input).ok(),
                "{input} should navigate"
            );
        }
    }

    #[test]
    fn searches_everything_that_does_not() {
        for input in ["rust lifetimes", "weather", "3.14", "why?", "example.c0m"] {
            assert_eq!(
                resolve_address(input, SearchProvider::Google).as_deref(),
                Ok(SearchProvider::Google.search_url(input).as_str()),
                "{input} should search"
            );
        }
    }

    #[test]
    fn searches_on_the_configured_provider() {
        assert_eq!(
            resolve_address("rust lifetimes", SearchProvider::DuckDuckGo).as_deref(),
            Ok("https://duckduckgo.com/?q=rust+lifetimes")
        );
        assert_eq!(
            resolve_address("c++ & rust", SearchProvider::Brave).as_deref(),
            Ok("https://search.brave.com/search?q=c%2B%2B+%26+rust")
        );
        assert_eq!(
            resolve_address("rust", SearchProvider::Google).as_deref(),
            Ok("https://www.google.com/search?q=rust")
        );
    }

    #[test]
    fn keeps_explicit_schemes_out_of_search() {
        assert_eq!(
            resolve_address("file:///tmp/a", SearchProvider::Google).as_deref(),
            Ok("file:///tmp/a")
        );
        assert_eq!(
            resolve_address("javascript:alert(1)", SearchProvider::Google),
            Err(UrlInputError::UnsupportedScheme)
        );
        assert_eq!(
            resolve_address("about:blank", SearchProvider::Google).as_deref(),
            Ok("about:blank")
        );
        assert_eq!(
            resolve_address("  ", SearchProvider::Google),
            Err(UrlInputError::Empty)
        );
    }

    #[test]
    fn round_trips_provider_names() {
        for provider in SearchProvider::ALL {
            assert_eq!(SearchProvider::parse(provider.as_str()), Some(provider));
        }
        assert_eq!(SearchProvider::parse("bing"), None);
    }

    #[test]
    fn redacts_diagnostic_urls() {
        assert_eq!(
            diagnostic_url("https://user:secret@example.com/path?q=token#section"),
            "https://example.com/path"
        );
        assert_eq!(diagnostic_url("not a url"), "<invalid URL>");
        assert_eq!(diagnostic_url("about:blank"), "about:blank");
    }
}
