use gpui::{App, Global, Task};
use std::{cell::OnceCell, future, path::PathBuf, sync::Arc, time::Duration};
use zz_chrome_import::recent_pages::RecentPages as StoredPages;

pub(crate) use zz_chrome_import::recent_pages::{
    HistorySuggestion, MAX_TITLE_BYTES, MAX_URL_BYTES, RecentPage,
};

#[derive(Default)]
pub(crate) struct RecentPages {
    path: Option<PathBuf>,
    pages: OnceCell<StoredPages>,
    revision: u64,
    saved_revision: u64,
    save_task: Option<Task<()>>,
    write_task: Option<Task<()>>,
    closing: bool,
}

const SAVE_INTERVAL: Duration = Duration::from_millis(250);

impl Global for RecentPages {}

impl RecentPages {
    fn pages(&self) -> &StoredPages {
        self.pages
            .get_or_init(|| StoredPages::load_deferred(self.path.clone()))
    }

    fn pages_mut(&mut self) -> &mut StoredPages {
        self.pages();
        self.pages.get_mut().expect("recent pages were initialized")
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
    init_at(path, cx);
}

fn init_at(path: Option<PathBuf>, cx: &mut App) {
    cx.set_global(RecentPages {
        path,
        ..RecentPages::default()
    });
    cx.on_app_quit(flush).detach();
}

fn schedule_save(cx: &mut App) {
    let pages = cx.global_mut::<RecentPages>();
    pages.revision = pages.revision.wrapping_add(1);
    if pages.closing || !pages.pages().is_persistent() || pages.save_task.is_some() {
        return;
    }
    let task = cx.spawn(async move |cx| {
        loop {
            cx.background_executor().timer(SAVE_INTERVAL).await;
            let write = cx.update(|cx| {
                let pages = cx.global_mut::<RecentPages>();
                if pages.closing {
                    return None;
                }
                let snapshot = pages.pages().clone();
                let revision = pages.revision;
                let (completed, receiver) = async_channel::bounded(1);
                let task = cx.background_executor().spawn(async move {
                    let saved = snapshot.save();
                    let _ = completed.try_send(saved);
                });
                cx.global_mut::<RecentPages>().write_task = Some(task);
                Some((receiver, revision))
            });
            let Some((completed, revision)) = write else {
                break;
            };
            let Ok(saved) = completed.recv().await else {
                break;
            };
            if let Err(error) = &saved {
                log::warn!(target: "zz::recent_pages", "could not persist recent pages: {error}");
            }
            let again = cx.update(|cx| {
                let pages = cx.global_mut::<RecentPages>();
                if let Some(task) = pages.write_task.take() {
                    task.detach();
                }
                if saved.is_ok() {
                    pages.saved_revision = revision;
                }
                let again = !pages.closing && pages.revision != revision;
                if !again && let Some(task) = pages.save_task.take() {
                    task.detach();
                }
                again
            });
            if !again {
                break;
            }
        }
    });
    cx.global_mut::<RecentPages>().save_task = Some(task);
}

fn flush(cx: &mut App) -> future::Ready<()> {
    let pages = cx.global_mut::<RecentPages>();
    pages.closing = true;
    drop(pages.save_task.take());
    let active_write = pages.write_task.take();
    if let Some(task) = active_write {
        cx.foreground_executor().block_on(task);
    }
    let pages = cx.global_mut::<RecentPages>();
    if pages.saved_revision != pages.revision
        && let Some(stored) = pages.pages.get()
        && stored.is_persistent()
    {
        match stored.save() {
            Ok(()) => pages.saved_revision = pages.revision,
            Err(error) => {
                log::warn!(target: "zz::recent_pages", "could not persist recent pages at shutdown: {error}");
            }
        }
    }
    future::ready(())
}

pub(crate) fn revision(cx: &App) -> u64 {
    cx.try_global::<RecentPages>()
        .map_or(0, |pages| pages.revision)
}

pub(crate) fn recent(profile: &str, cx: &App, limit: usize) -> Vec<RecentPage> {
    cx.try_global::<RecentPages>()
        .map(|pages| pages.pages().recent(profile, limit))
        .unwrap_or_default()
}

pub(crate) fn suggestions(
    profile: &str,
    input: &str,
    limit: usize,
    cx: &App,
) -> Vec<HistorySuggestion> {
    cx.try_global::<RecentPages>()
        .map(|pages| pages.pages().suggestions(profile, input, limit))
        .unwrap_or_default()
}

pub(crate) fn record_visit(profile: &str, url: &str, cx: &mut App) {
    if cx.has_global::<RecentPages>()
        && cx
            .global_mut::<RecentPages>()
            .pages_mut()
            .record_visit(profile, url)
    {
        schedule_save(cx);
    }
}

pub(crate) fn record_title(profile: &str, url: &str, title: &str, cx: &mut App) {
    if cx.has_global::<RecentPages>()
        && cx
            .global_mut::<RecentPages>()
            .pages_mut()
            .record_title(profile, url, title)
    {
        schedule_save(cx);
    }
}

pub(crate) fn favicon(profile: &str, url: &str, cx: &App) -> Option<Arc<[u8]>> {
    cx.try_global::<RecentPages>()
        .and_then(|pages| pages.pages().favicon(profile, url))
}

pub(crate) fn title(profile: &str, url: &str, cx: &App) -> Option<String> {
    cx.try_global::<RecentPages>()
        .and_then(|pages| pages.pages().title(profile, url))
        .map(str::to_owned)
}

pub(crate) fn record_favicon(profile: &str, url: &str, png: Arc<[u8]>, cx: &mut App) {
    if cx.has_global::<RecentPages>()
        && cx
            .global_mut::<RecentPages>()
            .pages_mut()
            .record_favicon(profile, url, png)
    {
        schedule_save(cx);
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
            .pages_mut()
            .record_omnibox_use(profile, input, url, selected)
    {
        schedule_save(cx);
    }
}

pub(crate) fn remove(profile: &str, url: &str, cx: &mut App) -> bool {
    if !cx.has_global::<RecentPages>()
        || !cx
            .global_mut::<RecentPages>()
            .pages_mut()
            .remove(profile, url)
    {
        return false;
    }
    schedule_save(cx);
    true
}

pub(crate) fn import_history(entries: Vec<RecentPage>, cx: &mut App) -> usize {
    if !cx.has_global::<RecentPages>() {
        return 0;
    }
    let changed = cx
        .global_mut::<RecentPages>()
        .pages_mut()
        .import_history(entries);
    if changed > 0 {
        schedule_save(cx);
    }
    changed
}

#[cfg(test)]
mod tests {
    use std::{
        cell::Cell,
        fs,
        rc::Rc,
        sync::atomic::{AtomicBool, Ordering},
        time::UNIX_EPOCH,
    };

    use gpui::{
        AppContext as _, Context, Entity, IntoElement, Render, StyleRefinement, Styled,
        TestAppContext, Window, div,
    };

    use super::*;

    const URL: &str = "https://example.com";

    #[gpui::test]
    fn startup_and_untouched_shutdown_do_not_load_or_rewrite_history(cx: &mut TestAppContext) {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("recent-pages");
        let mut stored = StoredPages::load_deferred(Some(path.clone()));
        stored.record_visit("default", URL);
        stored.record_title("default", URL, "Preserved");
        stored.save().unwrap();
        fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_times(fs::FileTimes::new().set_modified(UNIX_EPOCH + Duration::from_secs(1)))
            .unwrap();
        let contents = fs::read(&path).unwrap();
        let modified = fs::metadata(&path).unwrap().modified().unwrap();

        cx.update(|cx| {
            init_at(Some(path.clone()), cx);
            assert_eq!(revision(cx), 0);
            assert!(cx.global::<RecentPages>().pages.get().is_none());
            assert!(cx.global::<RecentPages>().save_task.is_none());
            cx.shutdown();
            assert!(cx.global::<RecentPages>().pages.get().is_none());
        });

        assert_eq!(fs::read(&path).unwrap(), contents);
        assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), modified);
    }

    #[gpui::test]
    fn first_read_loads_persisted_history_without_scheduling_a_save(cx: &mut TestAppContext) {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("recent-pages");
        cx.update(|cx| init_at(Some(path.clone()), cx));
        let mut stored = StoredPages::load_deferred(Some(path));
        stored.record_visit("default", URL);
        stored.record_title("default", URL, "Loaded on demand");
        stored.save().unwrap();

        cx.update(|cx| {
            assert!(cx.global::<RecentPages>().pages.get().is_none());
            assert_eq!(
                title("default", URL, cx).as_deref(),
                Some("Loaded on demand")
            );
            assert_eq!(revision(cx), 0);
            assert!(cx.global::<RecentPages>().pages.get().is_some());
            assert!(cx.global::<RecentPages>().save_task.is_none());
        });
    }

    #[gpui::test]
    fn first_mutation_preserves_previously_persisted_entries(cx: &mut TestAppContext) {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("recent-pages");
        let mut stored = StoredPages::load_deferred(Some(path.clone()));
        stored.record_visit("default", URL);
        stored.record_title("default", URL, "Existing page");
        stored.save().unwrap();
        let new_url = "https://example.com/new";

        cx.update(|cx| {
            init_at(Some(path.clone()), cx);
            assert!(cx.global::<RecentPages>().pages.get().is_none());
            record_visit("default", new_url, cx);
            record_title("default", new_url, "New page", cx);
            cx.shutdown();
        });

        let saved = StoredPages::load(Some(path));
        assert_eq!(saved.title("default", URL), Some("Existing page"));
        assert_eq!(saved.title("default", new_url), Some("New page"));
    }

    struct RenderProbe(Rc<Cell<usize>>);

    impl Render for RenderProbe {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            self.0.set(self.0.get() + 1);
            div()
        }
    }

    struct CachedProbe(Entity<RenderProbe>);

    impl Render for CachedProbe {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            self.0
                .clone()
                .cached(StyleRefinement::default().size_full())
        }
    }

    #[gpui::test]
    fn title_changes_do_not_redraw_unrelated_views(cx: &mut TestAppContext) {
        cx.update(|cx| init_at(None, cx));
        let renders = Rc::new(Cell::new(0));
        let probe = Rc::clone(&renders);
        let (_, cx) = cx.add_window_view(move |_, cx| CachedProbe(cx.new(|_| RenderProbe(probe))));
        cx.update(|window, cx| {
            window.draw(cx).clear(cx);
        });
        let before = renders.get();
        assert!(before > 0);
        cx.update(|_, cx| {
            record_visit("default", URL, cx);
            record_title("default", URL, "Changed", cx);
        });
        cx.update(|window, cx| {
            window.draw(cx).clear(cx);
        });
        assert_eq!(renders.get(), before);
        cx.update(|_, cx| cx.refresh_windows());
        cx.update(|window, cx| window.draw(cx).clear(cx));
        assert!(renders.get() > before);
    }

    #[gpui::test]
    fn history_changes_share_one_delayed_save(cx: &mut TestAppContext) {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("recent-pages");
        cx.update(|cx| {
            init_at(Some(path.clone()), cx);
            record_visit("default", URL, cx);
            for index in 0..100 {
                record_title("default", URL, &format!("Title {index}"), cx);
            }
            assert!(cx.global::<RecentPages>().save_task.is_some());
            assert_eq!(cx.global::<RecentPages>().saved_revision, 0);
        });
        while cx.executor().tick() {}
        assert!(!path.exists());
        cx.executor().advance_clock(SAVE_INTERVAL);
        cx.run_until_parked();
        assert_eq!(
            StoredPages::load(Some(path)).title("default", URL),
            Some("Title 99")
        );
        cx.update(|cx| {
            let pages = cx.global::<RecentPages>();
            assert_eq!(pages.saved_revision, pages.revision);
            assert!(pages.save_task.is_none());
        });
    }

    #[gpui::test]
    fn shutdown_flushes_without_waiting_for_the_save_timer(cx: &mut TestAppContext) {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("recent-pages");
        cx.update(|cx| {
            init_at(Some(path.clone()), cx);
            record_visit("default", URL, cx);
            record_title("default", URL, "Final", cx);
        });
        while cx.executor().tick() {}
        cx.update(App::shutdown);
        assert_eq!(
            StoredPages::load(Some(path)).title("default", URL),
            Some("Final")
        );
    }

    #[gpui::test]
    fn shutdown_waits_for_the_active_writer_before_saving_newer_history(cx: &mut TestAppContext) {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("recent-pages");
        let wrote_first = Arc::new(AtomicBool::new(false));
        let completed = Arc::clone(&wrote_first);
        let delay = Duration::from_secs(1);
        let started = cx.executor().now();
        cx.update(|cx| {
            init_at(Some(path.clone()), cx);
            record_visit("default", URL, cx);
            record_title("default", URL, "First", cx);
            let snapshot = cx.global::<RecentPages>().pages().clone();
            let timer = cx.background_executor().timer(delay);
            let task = cx.background_executor().spawn(async move {
                timer.await;
                snapshot.save().unwrap();
                completed.store(true, Ordering::SeqCst);
            });
            cx.global_mut::<RecentPages>().write_task = Some(task);
        });
        while cx.executor().tick() {}
        assert!(!wrote_first.load(Ordering::SeqCst));
        cx.update(|cx| {
            record_title("default", URL, "Final", cx);
            cx.shutdown();
        });
        assert!(cx.executor().now().duration_since(started) >= delay);
        assert!(wrote_first.load(Ordering::SeqCst));
        assert_eq!(
            StoredPages::load(Some(path)).title("default", URL),
            Some("Final")
        );
        cx.update(|cx| {
            let pages = cx.global::<RecentPages>();
            assert!(pages.write_task.is_none());
            assert_eq!(pages.saved_revision, pages.revision);
            assert!(pages.save_task.is_none());
        });
    }

    #[gpui::test]
    fn memory_only_history_never_schedules_persistence(cx: &mut TestAppContext) {
        cx.update(|cx| {
            init_at(None, cx);
            record_visit("default", URL, cx);
            record_title("default", URL, "Memory only", cx);
            assert!(cx.global::<RecentPages>().save_task.is_none());
            assert_eq!(title("default", URL, cx).as_deref(), Some("Memory only"));
        });
    }
}
