use std::{
    collections::{HashMap, HashSet},
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use gpui::{App, Global};

use crate::{config::atomic_write, user_data::restrict_to_current_user};

const MAX_ENTRIES: usize = 5_000;
const MAX_SHORTCUTS: usize = 5_000;
const MAX_INPUT_BYTES: usize = 512;
const MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;
const SIGNIFICANT_RECENCY_SECS: u64 = 3 * 24 * 60 * 60;
const FORMAT_VERSION: &str = "v2";
pub(crate) const MAX_URL_BYTES: usize = 4 * 1024;
pub(crate) const MAX_TITLE_BYTES: usize = 2 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RecentPage {
    pub(crate) url: String,
    pub(crate) title: String,
    pub(crate) visited_at: u64,
    profile: String,
    visit_count: u32,
    typed_count: u32,
    last_typed_at: u64,
}

impl RecentPage {
    pub(crate) fn imported(
        profile: impl Into<String>,
        url: String,
        title: String,
        visited_at: u64,
        visit_count: u32,
        typed_count: u32,
    ) -> Self {
        Self {
            profile: profile.into(),
            url,
            title,
            visited_at,
            visit_count: visit_count.max(1),
            typed_count,
            last_typed_at: if typed_count > 0 { visited_at } else { 0 },
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct HistoryShortcut {
    profile: String,
    input: String,
    url: String,
    selected_at: u64,
    selected_count: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct HistorySuggestion {
    pub(crate) url: String,
    pub(crate) title: String,
    pub(crate) display_url: String,
    pub(crate) inline_completion: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct RecentPages {
    entries: Vec<RecentPage>,
    shortcuts: Vec<HistoryShortcut>,
    path: Option<PathBuf>,
}

impl Global for RecentPages {}

impl RecentPages {
    fn visit(&mut self, profile: &str, url: &str, timestamp: u64) {
        let mut entry = self
            .entries
            .iter()
            .position(|entry| entry.profile == profile && entry.url == url)
            .map_or_else(
                || RecentPage {
                    profile: profile.to_owned(),
                    url: url.to_owned(),
                    title: String::new(),
                    visited_at: timestamp,
                    visit_count: 0,
                    typed_count: 0,
                    last_typed_at: 0,
                },
                |index| self.entries.remove(index),
            );
        entry.visited_at = timestamp;
        entry.visit_count = entry.visit_count.saturating_add(1);
        self.entries.insert(0, entry);
        self.prune();
    }

    fn retitle(&mut self, profile: &str, url: &str, title: &str) -> bool {
        let Some(entry) = self
            .entries
            .iter_mut()
            .find(|entry| entry.profile == profile && entry.url == url)
        else {
            return false;
        };
        let title = single_line(title, MAX_TITLE_BYTES);
        if entry.title == title {
            return false;
        }
        entry.title = title;
        true
    }

    fn record_omnibox_use(
        &mut self,
        profile: &str,
        input: &str,
        url: &str,
        selected: bool,
        timestamp: u64,
    ) -> bool {
        let Some(entry) = self
            .entries
            .iter_mut()
            .find(|entry| entry.profile == profile && entry.url == url)
        else {
            return false;
        };
        entry.typed_count = entry.typed_count.saturating_add(1);
        entry.last_typed_at = timestamp;
        if !selected {
            return true;
        }
        let input = normalized_input(input);
        if input.is_empty() {
            return true;
        }
        let mut shortcut = self
            .shortcuts
            .iter()
            .position(|shortcut| {
                shortcut.profile == profile && shortcut.input == input && shortcut.url == url
            })
            .map_or_else(
                || HistoryShortcut {
                    profile: profile.to_owned(),
                    input: input.clone(),
                    url: url.to_owned(),
                    selected_at: timestamp,
                    selected_count: 0,
                },
                |index| self.shortcuts.remove(index),
            );
        shortcut.selected_at = timestamp;
        shortcut.selected_count = shortcut.selected_count.saturating_add(1);
        self.shortcuts.insert(0, shortcut);
        self.prune();
        true
    }

    fn remove(&mut self, profile: &str, url: &str) -> bool {
        let entries_before = self.entries.len();
        self.entries
            .retain(|entry| entry.profile != profile || entry.url != url);
        if self.entries.len() == entries_before {
            return false;
        }
        self.shortcuts
            .retain(|shortcut| shortcut.profile != profile || shortcut.url != url);
        true
    }

    fn merge_history(&mut self, imported: Vec<RecentPage>) -> usize {
        let mut by_url = HashMap::with_capacity(self.entries.len().saturating_add(imported.len()));
        for entry in self.entries.drain(..) {
            by_url.insert((entry.profile.clone(), entry.url.clone()), entry);
        }

        let mut changed_urls = HashSet::new();
        for mut candidate in imported {
            let Ok(profile) = zz_browser::normalize_browser_profile_name(&candidate.profile) else {
                continue;
            };
            if !recordable_url(&candidate.url) {
                continue;
            }
            candidate.profile = profile;
            candidate.title = single_line(&candidate.title, MAX_TITLE_BYTES);
            candidate.visit_count = candidate.visit_count.max(1);
            let key = (candidate.profile.clone(), candidate.url.clone());
            match by_url.entry(key.clone()) {
                std::collections::hash_map::Entry::Vacant(entry) => {
                    changed_urls.insert(key);
                    entry.insert(candidate);
                }
                std::collections::hash_map::Entry::Occupied(mut entry) => {
                    let current = entry.get_mut();
                    let before = current.clone();
                    if candidate.visited_at > current.visited_at {
                        current.visited_at = candidate.visited_at;
                        if !candidate.title.is_empty() {
                            current.title = candidate.title;
                        }
                    } else if current.title.is_empty() && !candidate.title.is_empty() {
                        current.title = candidate.title;
                    }
                    current.visit_count = current.visit_count.max(candidate.visit_count);
                    current.typed_count = current.typed_count.max(candidate.typed_count);
                    current.last_typed_at = current.last_typed_at.max(candidate.last_typed_at);
                    if *current != before {
                        changed_urls.insert(key);
                    }
                }
            }
        }

        self.entries = by_url.into_values().collect();
        self.entries.sort_unstable_by(|left, right| {
            right
                .visited_at
                .cmp(&left.visited_at)
                .then_with(|| left.profile.cmp(&right.profile))
                .then_with(|| left.url.cmp(&right.url))
        });
        self.prune();
        changed_urls
            .iter()
            .filter(|(profile, url)| {
                self.entries
                    .iter()
                    .any(|entry| &entry.profile == profile && &entry.url == url)
            })
            .count()
    }

    fn suggestions(
        &self,
        profile: &str,
        input: &str,
        timestamp: u64,
        limit: usize,
    ) -> Vec<HistorySuggestion> {
        let query = normalized_input(input);
        if query.is_empty() || limit == 0 {
            return Vec::new();
        }
        let shortcut_scores = self.shortcut_scores(profile, &query, timestamp);
        let mut ranked = self
            .entries
            .iter()
            .filter(|entry| entry.profile == profile && significant(entry, timestamp))
            .filter_map(|entry| {
                let quality = match_quality(entry, &query)?;
                let usage = usage_score(
                    entry,
                    shortcut_scores.get(&entry.url).copied().unwrap_or_default(),
                    timestamp,
                );
                Some((quality, usage, entry))
            })
            .collect::<Vec<_>>();
        ranked.sort_unstable_by(
            |(left_quality, left_usage, left), (right_quality, right_usage, right)| {
                left_quality
                    .cmp(right_quality)
                    .then_with(|| right_usage.cmp(left_usage))
                    .then_with(|| right.visited_at.cmp(&left.visited_at))
                    .then_with(|| left.url.cmp(&right.url))
            },
        );
        ranked
            .into_iter()
            .take(limit)
            .map(|(_, _, entry)| HistorySuggestion {
                url: entry.url.clone(),
                title: entry.title.clone(),
                display_url: address_text(&entry.url),
                inline_completion: inline_completion(entry, input),
            })
            .collect()
    }

    fn shortcut_scores(&self, profile: &str, query: &str, timestamp: u64) -> HashMap<String, u64> {
        let mut scores = HashMap::new();
        for shortcut in self
            .shortcuts
            .iter()
            .filter(|shortcut| shortcut.profile == profile && shortcut.input.starts_with(query))
        {
            let count = u64::from(shortcut.selected_count.saturating_add(1).ilog2());
            let score = count.saturating_mul(900).saturating_add(freshness(
                shortcut.selected_at,
                timestamp,
                1_200,
                14,
            ));
            scores
                .entry(shortcut.url.clone())
                .and_modify(|current: &mut u64| *current = (*current).max(score))
                .or_insert(score);
        }
        scores
    }

    fn parse(source: &str) -> (Vec<RecentPage>, Vec<HistoryShortcut>) {
        let mut entries = Vec::new();
        let mut shortcuts = Vec::new();
        let mut seen_entries = HashSet::new();
        let mut seen_shortcuts = HashSet::new();
        for line in source.lines() {
            if line == FORMAT_VERSION {
                continue;
            }
            if let Some(record) = line.strip_prefix("p\t") {
                let mut parts = record.splitn(7, '\t');
                let profile = parts.next().and_then(valid_profile);
                let visited_at = parts.next().and_then(|value| value.parse::<u64>().ok());
                let visit_count = parts.next().and_then(|value| value.parse::<u32>().ok());
                let typed_count = parts.next().and_then(|value| value.parse::<u32>().ok());
                let last_typed_at = parts.next().and_then(|value| value.parse::<u64>().ok());
                let url = parts.next();
                let title = parts.next().unwrap_or_default();
                let (
                    Some(profile),
                    Some(visited_at),
                    Some(visit_count),
                    Some(typed_count),
                    Some(last_typed_at),
                    Some(url),
                ) = (
                    profile,
                    visited_at,
                    visit_count,
                    typed_count,
                    last_typed_at,
                    url,
                )
                else {
                    continue;
                };
                let key = (profile.clone(), url.to_owned());
                if entries.len() >= MAX_ENTRIES || !recordable_url(url) || !seen_entries.insert(key)
                {
                    continue;
                }
                entries.push(RecentPage {
                    profile,
                    url: url.to_owned(),
                    title: single_line(title, MAX_TITLE_BYTES),
                    visited_at,
                    visit_count: visit_count.max(1),
                    typed_count,
                    last_typed_at,
                });
                continue;
            }
            if let Some(record) = line.strip_prefix("s\t") {
                let mut parts = record.splitn(5, '\t');
                let profile = parts.next().and_then(valid_profile);
                let selected_at = parts.next().and_then(|value| value.parse::<u64>().ok());
                let selected_count = parts.next().and_then(|value| value.parse::<u32>().ok());
                let input = parts.next().map(normalized_input);
                let url = parts.next();
                let (
                    Some(profile),
                    Some(selected_at),
                    Some(selected_count),
                    Some(input),
                    Some(url),
                ) = (profile, selected_at, selected_count, input, url)
                else {
                    continue;
                };
                let key = (profile.clone(), input.clone(), url.to_owned());
                if shortcuts.len() >= MAX_SHORTCUTS
                    || input.is_empty()
                    || !recordable_url(url)
                    || !seen_shortcuts.insert(key)
                {
                    continue;
                }
                shortcuts.push(HistoryShortcut {
                    profile,
                    input,
                    url: url.to_owned(),
                    selected_at,
                    selected_count: selected_count.max(1),
                });
                continue;
            }
            let mut parts = line.splitn(3, '\t');
            let Some(visited_at) = parts.next().and_then(|value| value.parse().ok()) else {
                continue;
            };
            let Some(url) = parts.next() else {
                continue;
            };
            let profile = zz_browser::DEFAULT_BROWSER_PROFILE.to_owned();
            let key = (profile.clone(), url.to_owned());
            if entries.len() >= MAX_ENTRIES || !recordable_url(url) || !seen_entries.insert(key) {
                continue;
            }
            entries.push(RecentPage {
                profile,
                url: url.to_owned(),
                title: single_line(parts.next().unwrap_or_default(), MAX_TITLE_BYTES),
                visited_at,
                visit_count: 1,
                typed_count: 0,
                last_typed_at: 0,
            });
        }
        let known_urls = entries
            .iter()
            .map(|entry| (entry.profile.as_str(), entry.url.as_str()))
            .collect::<HashSet<_>>();
        shortcuts.retain(|shortcut| {
            known_urls.contains(&(shortcut.profile.as_str(), shortcut.url.as_str()))
        });
        (entries, shortcuts)
    }

    fn serialize(&self) -> String {
        let mut contents = String::from(FORMAT_VERSION);
        contents.push('\n');
        for entry in &self.entries {
            contents.push_str("p\t");
            contents.push_str(&entry.profile);
            contents.push('\t');
            contents.push_str(&entry.visited_at.to_string());
            contents.push('\t');
            contents.push_str(&entry.visit_count.to_string());
            contents.push('\t');
            contents.push_str(&entry.typed_count.to_string());
            contents.push('\t');
            contents.push_str(&entry.last_typed_at.to_string());
            contents.push('\t');
            contents.push_str(&entry.url);
            contents.push('\t');
            contents.push_str(&single_line(&entry.title, MAX_TITLE_BYTES));
            contents.push('\n');
        }
        for shortcut in &self.shortcuts {
            contents.push_str("s\t");
            contents.push_str(&shortcut.profile);
            contents.push('\t');
            contents.push_str(&shortcut.selected_at.to_string());
            contents.push('\t');
            contents.push_str(&shortcut.selected_count.to_string());
            contents.push('\t');
            contents.push_str(&single_line(&shortcut.input, MAX_INPUT_BYTES));
            contents.push('\t');
            contents.push_str(&shortcut.url);
            contents.push('\n');
        }
        contents
    }

    fn prune(&mut self) {
        self.entries.truncate(MAX_ENTRIES);
        let known_urls = self
            .entries
            .iter()
            .map(|entry| (entry.profile.as_str(), entry.url.as_str()))
            .collect::<HashSet<_>>();
        self.shortcuts.retain(|shortcut| {
            known_urls.contains(&(shortcut.profile.as_str(), shortcut.url.as_str()))
        });
        self.shortcuts.truncate(MAX_SHORTCUTS);
    }

    fn save(&self) {
        let Some(path) = &self.path else {
            return;
        };
        if let Err(error) = atomic_write(path, self.serialize().as_bytes()) {
            log::warn!(
                target: "zz::recent_pages",
                "could not persist recent pages path={} error={error}",
                path.display(),
            );
            return;
        }
        if let Err(error) = restrict_to_current_user(path) {
            log::warn!(
                target: "zz::recent_pages",
                "could not restrict recent pages permissions path={} error={error}",
                path.display(),
            );
        }
    }
}

pub fn init(cx: &mut App) {
    let path = match zz_browser::recent_pages_path() {
        Ok(path) => Some(path),
        Err(error) => {
            log::warn!(target: "zz::recent_pages", "recent pages are not persisted: {error}");
            None
        }
    };
    let (entries, shortcuts) = path.as_deref().map_or_else(
        || (Vec::new(), Vec::new()),
        |path| {
            match fs::metadata(path) {
                Ok(metadata) if metadata.len() > MAX_FILE_BYTES => {
                    log::warn!(
                        target: "zz::recent_pages",
                        "ignoring oversized recent pages file path={} bytes={}",
                        path.display(),
                        metadata.len(),
                    );
                    return (Vec::new(), Vec::new());
                }
                _ => {}
            }
            match fs::read_to_string(path) {
                Ok(source) => RecentPages::parse(&source),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    (Vec::new(), Vec::new())
                }
                Err(error) => {
                    log::warn!(
                        target: "zz::recent_pages",
                        "could not load recent pages path={} error={error}",
                        path.display(),
                    );
                    (Vec::new(), Vec::new())
                }
            }
        },
    );
    cx.set_global(RecentPages {
        entries,
        shortcuts,
        path,
    });
}

/// The most recently visited pages, newest first, capped at `limit`.
pub(crate) fn recent(profile: &str, cx: &App, limit: usize) -> Vec<RecentPage> {
    cx.try_global::<RecentPages>()
        .map(|pages| {
            pages
                .entries
                .iter()
                .filter(|entry| entry.profile == profile)
                .take(limit)
                .cloned()
                .collect()
        })
        .unwrap_or_default()
}

pub(crate) fn suggestions(
    profile: &str,
    input: &str,
    limit: usize,
    cx: &App,
) -> Vec<HistorySuggestion> {
    cx.try_global::<RecentPages>()
        .map(|pages| pages.suggestions(profile, input, unix_now(), limit))
        .unwrap_or_default()
}

/// Move `url` to the front of the recent list and persist it.
pub(crate) fn record_visit(profile: &str, url: &str, cx: &mut App) {
    if !recordable_url(url) || !cx.has_global::<RecentPages>() {
        return;
    }
    let pages = cx.global_mut::<RecentPages>();
    pages.visit(profile, url, unix_now());
    pages.save();
    cx.refresh_windows();
}

/// Update the stored title for `url`, if it is in the recent list.
pub(crate) fn record_title(profile: &str, url: &str, title: &str, cx: &mut App) {
    if title.is_empty() || !cx.has_global::<RecentPages>() {
        return;
    }
    let pages = cx.global_mut::<RecentPages>();
    if pages.retitle(profile, url, title) {
        pages.save();
        cx.refresh_windows();
    }
}

pub(crate) fn record_omnibox_use(
    profile: &str,
    input: &str,
    url: &str,
    selected: bool,
    cx: &mut App,
) {
    if !cx.has_global::<RecentPages>() {
        return;
    }
    let pages = cx.global_mut::<RecentPages>();
    if pages.record_omnibox_use(profile, input, url, selected, unix_now()) {
        pages.save();
        cx.refresh_windows();
    }
}

pub(crate) fn remove(profile: &str, url: &str, cx: &mut App) -> bool {
    if !cx.has_global::<RecentPages>() {
        return false;
    }
    let pages = cx.global_mut::<RecentPages>();
    if !pages.remove(profile, url) {
        return false;
    }
    pages.save();
    cx.refresh_windows();
    true
}

/// Merge an imported browser history into the persisted list: URLs deduplicate,
/// newer visits win, existing titles survive. Returns the number of rows changed.
pub(crate) fn import_history(entries: Vec<RecentPage>, cx: &mut App) -> usize {
    if !cx.has_global::<RecentPages>() {
        return 0;
    }
    let pages = cx.global_mut::<RecentPages>();
    let changed = pages.merge_history(entries);
    if changed > 0 {
        pages.save();
        cx.refresh_windows();
    }
    changed
}

fn recordable_url(url: &str) -> bool {
    url.len() <= MAX_URL_BYTES
        && !url.chars().any(char::is_control)
        && (url.starts_with("http://") || url.starts_with("https://"))
}

fn valid_profile(profile: &str) -> Option<String> {
    zz_browser::normalize_browser_profile_name(profile).ok()
}

fn normalized_input(input: &str) -> String {
    let input = input.split_whitespace().collect::<Vec<_>>().join(" ");
    single_line(&input.to_lowercase(), MAX_INPUT_BYTES)
}

fn single_line(text: &str, max_bytes: usize) -> String {
    truncate_utf8(&text.replace(['\t', '\n', '\r'], " "), max_bytes)
}

pub(crate) fn truncate_utf8(value: &str, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value.to_owned();
    }
    let mut end = max_bytes;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_owned()
}

fn significant(entry: &RecentPage, timestamp: u64) -> bool {
    entry.typed_count > 0
        || entry.visit_count >= 4
        || timestamp.saturating_sub(entry.visited_at) <= SIGNIFICANT_RECENCY_SECS
}

fn match_quality(entry: &RecentPage, query: &str) -> Option<u8> {
    let url = entry.url.to_lowercase();
    let without_scheme = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .unwrap_or(&url);
    let address = address_text(&entry.url).to_lowercase();
    let host = address.split(['/', '?', '#']).next().unwrap_or_default();
    let raw_host = without_scheme
        .split(['/', '?', '#'])
        .next()
        .unwrap_or_default();
    let title = entry.title.to_lowercase();
    if !query
        .split_whitespace()
        .all(|term| url.contains(term) || title.contains(term))
    {
        return None;
    }
    let normalized_query = query.trim_end_matches('/');
    Some(
        if [url.as_str(), without_scheme, address.as_str()]
            .into_iter()
            .any(|candidate| candidate.trim_end_matches('/') == normalized_query)
        {
            0
        } else if (host.starts_with(query) || raw_host.starts_with(query))
            && !address.contains(['/', '?', '#'])
        {
            1
        } else if host.starts_with(query) || raw_host.starts_with(query) {
            2
        } else if address.starts_with(query)
            || without_scheme.starts_with(query)
            || url.starts_with(query)
        {
            3
        } else if contains_at_boundary(&url, query) {
            4
        } else if contains_at_boundary(&title, query) {
            5
        } else if url.contains(query) {
            6
        } else if title.contains(query) {
            7
        } else {
            8
        },
    )
}

fn contains_at_boundary(value: &str, query: &str) -> bool {
    value.match_indices(query).any(|(index, _)| {
        index == 0
            || value[..index]
                .chars()
                .next_back()
                .is_none_or(|character| !character.is_alphanumeric())
    })
}

fn usage_score(entry: &RecentPage, shortcut_score: u64, timestamp: u64) -> u64 {
    let visits = u64::from(entry.visit_count.saturating_add(1).ilog2()).saturating_mul(180);
    let typed = u64::from(entry.typed_count.saturating_add(1).ilog2()).saturating_mul(420);
    shortcut_score
        .saturating_add(visits)
        .saturating_add(typed)
        .saturating_add(freshness(entry.visited_at, timestamp, 1_400, 7))
        .saturating_add(freshness(entry.last_typed_at, timestamp, 700, 14))
}

fn freshness(used_at: u64, timestamp: u64, maximum: u64, half_life_days: u64) -> u64 {
    if used_at == 0 {
        return 0;
    }
    let age_days = timestamp.saturating_sub(used_at) / (24 * 60 * 60);
    maximum / (1 + age_days / half_life_days.max(1))
}

fn inline_completion(entry: &RecentPage, input: &str) -> Option<String> {
    if input.is_empty() || input != input.trim() || input.chars().any(char::is_whitespace) {
        return None;
    }
    let display = address_text(&entry.url);
    let deep = display.contains(['/', '?', '#']);
    if entry.typed_count < if deep { 2 } else { 1 } {
        return None;
    }
    let candidates = if input.contains("://") {
        vec![entry.url.as_str()]
    } else {
        let without_scheme = entry
            .url
            .strip_prefix("https://")
            .or_else(|| entry.url.strip_prefix("http://"))
            .unwrap_or(&entry.url);
        vec![
            without_scheme,
            without_scheme
                .strip_prefix("www.")
                .unwrap_or(without_scheme),
        ]
    };
    candidates.into_iter().find_map(|candidate| {
        if candidate.len() <= input.len()
            || !candidate
                .get(..input.len())
                .is_some_and(|prefix| prefix.eq_ignore_ascii_case(input))
        {
            return None;
        }
        Some(format!("{input}{}", candidate.get(input.len()..)?))
    })
}

fn address_text(url: &str) -> String {
    let without_scheme = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .unwrap_or(url);
    let address = without_scheme
        .strip_prefix("www.")
        .unwrap_or(without_scheme);
    if address.ends_with('/') && !address[..address.len() - 1].contains('/') {
        address[..address.len() - 1].to_owned()
    } else {
        address.to_owned()
    }
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEFAULT: &str = zz_browser::DEFAULT_BROWSER_PROFILE;

    fn page(
        profile: &str,
        url: &str,
        title: &str,
        visited_at: u64,
        visit_count: u32,
        typed_count: u32,
    ) -> RecentPage {
        RecentPage::imported(
            profile,
            url.to_owned(),
            title.to_owned(),
            visited_at,
            visit_count,
            typed_count,
        )
    }

    fn pages_with(entries: Vec<RecentPage>) -> RecentPages {
        RecentPages {
            entries,
            shortcuts: Vec::new(),
            path: None,
        }
    }

    #[test]
    fn visiting_moves_the_url_to_the_front_and_keeps_its_history() {
        let mut pages = pages_with(vec![
            page(DEFAULT, "https://zed.dev", "Zed", 10, 3, 1),
            page(DEFAULT, "https://ziglang.org", "Zig", 9, 2, 0),
        ]);
        pages.visit(DEFAULT, "https://ziglang.org", 11);
        assert_eq!(pages.entries[0].url, "https://ziglang.org");
        assert_eq!(pages.entries[0].title, "Zig");
        assert_eq!(pages.entries[0].visit_count, 3);
        assert_eq!(pages.entries[1].url, "https://zed.dev");
    }

    #[test]
    fn visiting_caps_the_list() {
        let mut pages = RecentPages::default();
        for index in 0..(MAX_ENTRIES + 5) {
            pages.visit(
                DEFAULT,
                &format!("https://example.com/{index}"),
                index as u64,
            );
        }
        assert_eq!(pages.entries.len(), MAX_ENTRIES);
        assert_eq!(
            pages.entries[0].url,
            format!("https://example.com/{}", MAX_ENTRIES + 4)
        );
    }

    #[test]
    fn retitle_is_scoped_to_the_profile() {
        let mut pages = pages_with(vec![
            page(DEFAULT, "https://zed.dev", "", 10, 1, 0),
            page("work", "https://zed.dev", "Work", 9, 1, 0),
        ]);
        assert!(pages.retitle(DEFAULT, "https://zed.dev", "Zed"));
        assert!(!pages.retitle(DEFAULT, "https://zed.dev", "Zed"));
        assert_eq!(pages.entries[0].title, "Zed");
        assert_eq!(pages.entries[1].title, "Work");
    }

    #[test]
    fn merging_history_keeps_newer_content_and_stronger_counts() {
        let mut pages = pages_with(vec![
            page(DEFAULT, "https://one.example", "Existing", 20, 2, 1),
            page(DEFAULT, "https://two.example", "Two", 10, 1, 0),
        ]);
        let changed = pages.merge_history(vec![
            page(DEFAULT, "https://one.example", "Older title", 15, 8, 4),
            page(DEFAULT, "https://two.example", "", 30, 3, 0),
            page(DEFAULT, "https://three.example", "Three", 25, 5, 2),
            page(DEFAULT, "file:///tmp/private", "Private", 40, 1, 0),
        ]);

        assert_eq!(changed, 3);
        assert_eq!(pages.entries[0].url, "https://two.example");
        assert_eq!(pages.entries[0].title, "Two");
        assert_eq!(pages.entries[1].url, "https://three.example");
        assert_eq!(pages.entries[2].title, "Existing");
        assert_eq!(pages.entries[2].visit_count, 8);
        assert_eq!(pages.entries[2].typed_count, 4);
    }

    #[test]
    fn serialization_round_trips_counts_shortcuts_and_flattened_text() {
        let mut pages = pages_with(vec![page(
            DEFAULT,
            "https://zed.dev",
            "Zed\tthe\neditor",
            10,
            4,
            2,
        )]);
        assert!(pages.record_omnibox_use(DEFAULT, " ZeD ", "https://zed.dev", true, 12));
        let (entries, shortcuts) = RecentPages::parse(&pages.serialize());
        assert_eq!(entries[0].title, "Zed the editor");
        assert_eq!(entries[0].visit_count, 4);
        assert_eq!(entries[0].typed_count, 3);
        assert_eq!(shortcuts[0].input, "zed");
        assert_eq!(shortcuts[0].selected_count, 1);
    }

    #[test]
    fn parsing_migrates_legacy_rows_into_the_default_profile() {
        let (entries, shortcuts) = RecentPages::parse(
            "nonsense\n\
             12\tabout:blank\tBlank\n\
             13\tfile:///etc/passwd\tNope\n\
             14\thttps://zed.dev\tZed\n\
             not-a-number\thttps://ziglang.org\tZig\n",
        );
        assert_eq!(
            entries,
            vec![page(DEFAULT, "https://zed.dev", "Zed", 14, 1, 0)]
        );
        assert!(shortcuts.is_empty());
    }

    #[test]
    fn suggestions_match_url_and_title_terms() {
        let now = 1_000_000;
        let pages = pages_with(vec![
            page(
                DEFAULT,
                "https://github.com/rust-lang/rust",
                "Rust compiler",
                now,
                3,
                1,
            ),
            page(
                DEFAULT,
                "https://docs.rs/tokio",
                "Tokio runtime",
                now - 1,
                2,
                1,
            ),
            page(DEFAULT, "https://example.com", "Unrelated", now, 5, 2),
        ]);
        let suggestions = pages.suggestions(DEFAULT, "rust compiler", now, 8);
        assert_eq!(suggestions.len(), 1);
        assert_eq!(suggestions[0].url, "https://github.com/rust-lang/rust");
        assert_eq!(suggestions[0].display_url, "github.com/rust-lang/rust");
    }

    #[test]
    fn host_prefix_outranks_a_newer_title_match() {
        let now = 1_000_000;
        let pages = pages_with(vec![
            page(DEFAULT, "https://other.example", "GitHub", now, 10, 5),
            page(DEFAULT, "https://github.com", "GitHub", now - 100, 1, 1),
        ]);
        let suggestions = pages.suggestions(DEFAULT, "git", now, 8);
        assert_eq!(suggestions[0].url, "https://github.com");
    }

    #[test]
    fn scheme_and_www_prefixes_keep_url_matches_in_the_strongest_classes() {
        let now = 1_000_000;
        let pages = pages_with(vec![
            page(
                DEFAULT,
                "https://www.example.com",
                "Example",
                now - 100,
                1,
                1,
            ),
            page(
                DEFAULT,
                "https://other.example",
                "www.example.com",
                now,
                20,
                10,
            ),
        ]);
        assert_eq!(
            pages.suggestions(DEFAULT, "www.exa", now, 8)[0].url,
            "https://www.example.com"
        );
        assert_eq!(
            pages.suggestions(DEFAULT, "https://www.example.com", now, 8)[0].url,
            "https://www.example.com"
        );
    }

    #[test]
    fn repeat_use_and_recency_rank_within_the_same_match_class() {
        let now = 30 * 24 * 60 * 60;
        let pages = pages_with(vec![
            page(
                DEFAULT,
                "https://docs.example/old",
                "Old",
                now - 20 * 24 * 60 * 60,
                40,
                8,
            ),
            page(DEFAULT, "https://docs.example/new", "New", now - 60, 1, 1),
        ]);
        let suggestions = pages.suggestions(DEFAULT, "docs.example/", now, 8);
        assert_eq!(suggestions[0].url, "https://docs.example/old");
        assert_eq!(suggestions[1].url, "https://docs.example/new");
    }

    #[test]
    fn selected_shortcuts_learn_a_prefix() {
        let now = 1_000_000;
        let mut pages = pages_with(vec![
            page(DEFAULT, "https://gitlab.com", "GitLab", now, 20, 4),
            page(DEFAULT, "https://github.com", "GitHub", now - 1, 2, 1),
        ]);
        for selected_at in [now - 2, now - 1, now] {
            assert!(pages.record_omnibox_use(
                DEFAULT,
                "gi",
                "https://github.com",
                true,
                selected_at,
            ));
        }
        let suggestions = pages.suggestions(DEFAULT, "g", now, 8);
        assert_eq!(suggestions[0].url, "https://github.com");
    }

    #[test]
    fn old_single_visits_do_not_qualify_until_reused() {
        let now = 10 * 24 * 60 * 60;
        let mut pages = pages_with(vec![page(
            DEFAULT,
            "https://forgotten.example",
            "Forgotten",
            1,
            1,
            0,
        )]);
        assert!(pages.suggestions(DEFAULT, "forgotten", now, 8).is_empty());
        pages.visit(DEFAULT, "https://forgotten.example", now);
        assert_eq!(pages.suggestions(DEFAULT, "forgotten", now, 8).len(), 1);
    }

    #[test]
    fn suggestions_do_not_cross_profile_boundaries() {
        let now = 1_000_000;
        let pages = pages_with(vec![
            page(DEFAULT, "https://personal.example", "Personal", now, 4, 1),
            page("work", "https://work.example", "Work", now, 4, 1),
        ]);
        let default_urls = pages
            .suggestions(DEFAULT, "example", now, 8)
            .into_iter()
            .map(|suggestion| suggestion.url)
            .collect::<Vec<_>>();
        let work_urls = pages
            .suggestions("work", "example", now, 8)
            .into_iter()
            .map(|suggestion| suggestion.url)
            .collect::<Vec<_>>();
        assert_eq!(default_urls, ["https://personal.example"]);
        assert_eq!(work_urls, ["https://work.example"]);
    }

    #[test]
    fn inline_completion_requires_typed_history() {
        let now = 1_000_000;
        let pages = pages_with(vec![
            page(DEFAULT, "https://www.example.com", "Example", now, 4, 1),
            page(DEFAULT, "https://example.com/deep", "Deep", now, 4, 1),
        ]);
        let root = pages.suggestions(DEFAULT, "exa", now, 8);
        assert_eq!(root[0].inline_completion.as_deref(), Some("example.com"));
        let deep = pages.suggestions(DEFAULT, "example.com/d", now, 8);
        assert_eq!(deep[0].inline_completion, None);
    }

    #[test]
    fn removing_a_page_removes_its_shortcuts_only_in_that_profile() {
        let mut pages = pages_with(vec![
            page(DEFAULT, "https://zed.dev", "Zed", 10, 1, 1),
            page("work", "https://zed.dev", "Work Zed", 9, 1, 1),
        ]);
        assert!(pages.record_omnibox_use(DEFAULT, "zed", "https://zed.dev", true, 11));
        assert!(pages.record_omnibox_use("work", "zed", "https://zed.dev", true, 11));
        assert!(pages.remove(DEFAULT, "https://zed.dev"));
        assert_eq!(pages.entries.len(), 1);
        assert_eq!(pages.entries[0].profile, "work");
        assert_eq!(pages.shortcuts.len(), 1);
        assert_eq!(pages.shortcuts[0].profile, "work");
    }
}
