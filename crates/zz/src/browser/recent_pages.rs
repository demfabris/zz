use gpui::{App, Global};

pub(crate) use zz_chrome_import::recent_pages::{
    HistorySuggestion, MAX_TITLE_BYTES, MAX_URL_BYTES, RecentPage,
};

#[derive(Default)]
pub(crate) struct RecentPages(zz_chrome_import::recent_pages::RecentPages);

impl Global for RecentPages {}

pub fn init(cx: &mut App) {
    let path = match zz_browser::recent_pages_path() {
        Ok(path) => Some(path),
        Err(error) => {
            log::warn!(target: "zz::recent_pages", "recent pages are not persisted: {error}");
            None
        }
    };
    cx.set_global(RecentPages(
        zz_chrome_import::recent_pages::RecentPages::load(path),
    ));
}

pub(crate) fn recent(profile: &str, cx: &App, limit: usize) -> Vec<RecentPage> {
    cx.try_global::<RecentPages>()
        .map(|pages| pages.0.recent(profile, limit))
        .unwrap_or_default()
}

pub(crate) fn suggestions(
    profile: &str,
    input: &str,
    limit: usize,
    cx: &App,
) -> Vec<HistorySuggestion> {
    cx.try_global::<RecentPages>()
        .map(|pages| pages.0.suggestions(profile, input, limit))
        .unwrap_or_default()
}

pub(crate) fn record_visit(profile: &str, url: &str, cx: &mut App) {
    if cx.has_global::<RecentPages>() && cx.global_mut::<RecentPages>().0.record_visit(profile, url)
    {
        cx.refresh_windows();
    }
}

pub(crate) fn record_title(profile: &str, url: &str, title: &str, cx: &mut App) {
    if cx.has_global::<RecentPages>()
        && cx
            .global_mut::<RecentPages>()
            .0
            .record_title(profile, url, title)
    {
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
    if cx.has_global::<RecentPages>()
        && cx
            .global_mut::<RecentPages>()
            .0
            .record_omnibox_use(profile, input, url, selected)
    {
        cx.refresh_windows();
    }
}

pub(crate) fn remove(profile: &str, url: &str, cx: &mut App) -> bool {
    if !cx.has_global::<RecentPages>() || !cx.global_mut::<RecentPages>().0.remove(profile, url) {
        return false;
    }
    cx.refresh_windows();
    true
}

pub(crate) fn import_history(entries: Vec<RecentPage>, cx: &mut App) -> usize {
    if !cx.has_global::<RecentPages>() {
        return 0;
    }
    let changed = cx.global_mut::<RecentPages>().0.import_history(entries);
    if changed > 0 {
        cx.refresh_windows();
    }
    changed
}
