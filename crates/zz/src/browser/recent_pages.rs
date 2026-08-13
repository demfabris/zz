//! Recently visited pages shown in the browser pane empty state.

use std::{
    collections::{HashMap, HashSet},
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use gpui::{App, Global};

use crate::{config::atomic_write, user_data::restrict_to_current_user};

const MAX_ENTRIES: usize = 5_000;
const MAX_FILE_BYTES: u64 = 32 * 1024 * 1024;
pub(crate) const MAX_URL_BYTES: usize = 4 * 1024;
pub(crate) const MAX_TITLE_BYTES: usize = 2 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RecentPage {
    pub(crate) url: String,
    pub(crate) title: String,
    pub(crate) visited_at: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct RecentPages {
    entries: Vec<RecentPage>,
    path: Option<PathBuf>,
}

impl Global for RecentPages {}

impl RecentPages {
    fn visit(&mut self, url: &str, timestamp: u64) {
        let title = self
            .entries
            .iter()
            .position(|entry| entry.url == url)
            .map(|index| self.entries.remove(index).title)
            .unwrap_or_default();
        self.entries.insert(
            0,
            RecentPage {
                url: url.to_owned(),
                title,
                visited_at: timestamp,
            },
        );
        self.entries.truncate(MAX_ENTRIES);
    }

    fn retitle(&mut self, url: &str, title: &str) -> bool {
        let Some(entry) = self.entries.iter_mut().find(|entry| entry.url == url) else {
            return false;
        };
        let title = single_line(title);
        if entry.title == title {
            return false;
        }
        entry.title = title;
        true
    }

    fn merge_history(&mut self, imported: Vec<RecentPage>) -> usize {
        let mut by_url = HashMap::with_capacity(self.entries.len().saturating_add(imported.len()));
        for entry in self.entries.drain(..) {
            by_url.insert(entry.url.clone(), entry);
        }

        let mut changed_urls = HashSet::new();
        for mut candidate in imported {
            if !recordable_url(&candidate.url) {
                continue;
            }
            candidate.title = single_line(&candidate.title);
            match by_url.entry(candidate.url.clone()) {
                std::collections::hash_map::Entry::Vacant(entry) => {
                    changed_urls.insert(candidate.url.clone());
                    entry.insert(candidate);
                }
                std::collections::hash_map::Entry::Occupied(mut entry) => {
                    let current = entry.get_mut();
                    if candidate.visited_at > current.visited_at {
                        if candidate.title.is_empty() {
                            current.title.clone_into(&mut candidate.title);
                        }
                        changed_urls.insert(candidate.url.clone());
                        *current = candidate;
                    } else if current.title.is_empty() && !candidate.title.is_empty() {
                        candidate.title.clone_into(&mut current.title);
                        changed_urls.insert(current.url.clone());
                    }
                }
            }
        }

        self.entries = by_url.into_values().collect();
        self.entries.sort_unstable_by(|left, right| {
            right
                .visited_at
                .cmp(&left.visited_at)
                .then_with(|| left.url.cmp(&right.url))
        });
        self.entries.truncate(MAX_ENTRIES);
        changed_urls
            .iter()
            .filter(|url| self.entries.iter().any(|entry| &entry.url == *url))
            .count()
    }

    fn parse(source: &str) -> Vec<RecentPage> {
        let mut seen = HashSet::new();
        source
            .lines()
            .filter_map(|line| {
                let mut parts = line.splitn(3, '\t');
                let visited_at = parts.next()?.parse().ok()?;
                let url = parts.next()?;
                (recordable_url(url) && seen.insert(url.to_owned())).then(|| RecentPage {
                    url: url.to_owned(),
                    title: single_line(parts.next().unwrap_or_default()),
                    visited_at,
                })
            })
            .take(MAX_ENTRIES)
            .collect()
    }

    fn serialize(&self) -> String {
        let mut contents = String::new();
        for entry in &self.entries {
            contents.push_str(&entry.visited_at.to_string());
            contents.push('\t');
            contents.push_str(&entry.url);
            contents.push('\t');
            contents.push_str(&single_line(&entry.title));
            contents.push('\n');
        }
        contents
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
    let entries = path.as_deref().map_or_else(Vec::new, |path| {
        match fs::metadata(path) {
            Ok(metadata) if metadata.len() > MAX_FILE_BYTES => {
                log::warn!(
                    target: "zz::recent_pages",
                    "ignoring oversized recent pages file path={} bytes={}",
                    path.display(),
                    metadata.len(),
                );
                return Vec::new();
            }
            _ => {}
        }
        match fs::read_to_string(path) {
            Ok(source) => RecentPages::parse(&source),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(error) => {
                log::warn!(
                    target: "zz::recent_pages",
                    "could not load recent pages path={} error={error}",
                    path.display(),
                );
                Vec::new()
            }
        }
    });
    cx.set_global(RecentPages { entries, path });
}

/// The most recently visited pages, newest first, capped at `limit`.
pub(crate) fn recent(cx: &App, limit: usize) -> Vec<RecentPage> {
    cx.try_global::<RecentPages>()
        .map(|pages| pages.entries.iter().take(limit).cloned().collect())
        .unwrap_or_default()
}

/// Move `url` to the front of the recent list and persist it.
pub(crate) fn record_visit(url: &str, cx: &mut App) {
    if !recordable_url(url) || !cx.has_global::<RecentPages>() {
        return;
    }
    let pages = cx.global_mut::<RecentPages>();
    pages.visit(url, unix_now());
    pages.save();
    cx.refresh_windows();
}

/// Update the stored title for `url`, if it is in the recent list.
pub(crate) fn record_title(url: &str, title: &str, cx: &mut App) {
    if title.is_empty() || !cx.has_global::<RecentPages>() {
        return;
    }
    let pages = cx.global_mut::<RecentPages>();
    if pages.retitle(url, title) {
        pages.save();
        cx.refresh_windows();
    }
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
    url.len() <= MAX_URL_BYTES && (url.starts_with("http://") || url.starts_with("https://"))
}

fn single_line(text: &str) -> String {
    truncate_utf8(&text.replace(['\t', '\n', '\r'], " "), MAX_TITLE_BYTES)
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

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pages_with(entries: &[(&str, &str, u64)]) -> RecentPages {
        RecentPages {
            entries: entries
                .iter()
                .map(|(url, title, visited_at)| RecentPage {
                    url: (*url).to_owned(),
                    title: (*title).to_owned(),
                    visited_at: *visited_at,
                })
                .collect(),
            path: None,
        }
    }

    #[test]
    fn visiting_moves_the_url_to_the_front_and_keeps_its_title() {
        let mut pages = pages_with(&[
            ("https://zed.dev", "Zed", 10),
            ("https://ziglang.org", "Zig", 9),
        ]);
        pages.visit("https://ziglang.org", 11);
        assert_eq!(
            pages,
            pages_with(&[
                ("https://ziglang.org", "Zig", 11),
                ("https://zed.dev", "Zed", 10),
            ])
        );
    }

    #[test]
    fn visiting_caps_the_list() {
        let mut pages = RecentPages::default();
        for index in 0..(MAX_ENTRIES + 5) {
            pages.visit(&format!("https://example.com/{index}"), index as u64);
        }
        assert_eq!(pages.entries.len(), MAX_ENTRIES);
        assert_eq!(
            pages.entries[0].url,
            format!("https://example.com/{}", MAX_ENTRIES + 4)
        );
    }

    #[test]
    fn retitle_updates_only_matching_entries() {
        let mut pages = pages_with(&[("https://zed.dev", "", 10)]);
        assert!(pages.retitle("https://zed.dev", "Zed"));
        assert!(!pages.retitle("https://zed.dev", "Zed"));
        assert!(!pages.retitle("https://unknown.dev", "Nope"));
        assert_eq!(pages.entries[0].title, "Zed");
    }

    #[test]
    fn merging_history_deduplicates_urls_and_keeps_the_newest_visit() {
        let mut pages = pages_with(&[
            ("https://one.example", "Existing", 20),
            ("https://two.example", "Two", 10),
        ]);
        let changed = pages.merge_history(vec![
            RecentPage {
                url: "https://one.example".to_owned(),
                title: "Older title".to_owned(),
                visited_at: 15,
            },
            RecentPage {
                url: "https://two.example".to_owned(),
                title: String::new(),
                visited_at: 30,
            },
            RecentPage {
                url: "https://three.example".to_owned(),
                title: "Three".to_owned(),
                visited_at: 25,
            },
            RecentPage {
                url: "file:///tmp/private".to_owned(),
                title: "Private".to_owned(),
                visited_at: 40,
            },
        ]);

        assert_eq!(changed, 2);
        assert_eq!(
            pages,
            pages_with(&[
                ("https://two.example", "Two", 30),
                ("https://three.example", "Three", 25),
                ("https://one.example", "Existing", 20),
            ])
        );
    }

    #[test]
    fn merging_history_can_fill_a_missing_title_without_moving_the_visit() {
        let mut pages = pages_with(&[("https://one.example", "", 20)]);
        let changed = pages.merge_history(vec![RecentPage {
            url: "https://one.example".to_owned(),
            title: "Imported title".to_owned(),
            visited_at: 10,
        }]);

        assert_eq!(changed, 1);
        assert_eq!(pages.entries[0].visited_at, 20);
        assert_eq!(pages.entries[0].title, "Imported title");
    }

    #[test]
    fn serialization_round_trips_and_flattens_titles() {
        let pages = pages_with(&[
            ("https://zed.dev", "Zed\tthe\neditor", 10),
            ("https://ziglang.org", "Zig", 9),
        ]);
        let parsed = RecentPages::parse(&pages.serialize());
        assert_eq!(parsed[0].title, "Zed the editor");
        assert_eq!(parsed[1], pages.entries[1]);
    }

    #[test]
    fn parsing_skips_malformed_and_non_web_lines() {
        let parsed = RecentPages::parse(
            "nonsense\n\
             12\tabout:blank\tBlank\n\
             13\tfile:///etc/passwd\tNope\n\
             14\thttps://zed.dev\tZed\n\
             not-a-number\thttps://ziglang.org\tZig\n",
        );
        assert_eq!(
            parsed,
            vec![RecentPage {
                url: "https://zed.dev".to_owned(),
                title: "Zed".to_owned(),
                visited_at: 14,
            }]
        );
    }
}
