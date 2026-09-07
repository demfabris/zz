use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

use async_channel::Sender;
use notify::{RecommendedWatcher, RecursiveMode, Watcher as _};

use super::host::HostEvent;

pub(super) struct Settings {
    candidates: Vec<PathBuf>,
    watched: BTreeSet<PathBuf>,
    watcher: RecommendedWatcher,
}

impl Settings {
    pub(super) fn new(events: Sender<HostEvent>) -> notify::Result<Self> {
        Self::with_candidates(crate::config::config_candidates(), events)
    }

    fn with_candidates(
        candidates: Vec<PathBuf>,
        events: Sender<HostEvent>,
    ) -> notify::Result<Self> {
        let paths = candidates.clone();
        let watcher =
            notify::recommended_watcher(move |event: notify::Result<notify::Event>| match event {
                Ok(event)
                    if !matches!(event.kind, notify::EventKind::Access(_))
                        && event.paths.iter().any(|changed| {
                            paths.iter().any(|candidate| {
                                candidate.starts_with(changed)
                                    || resolved_path(candidate)
                                        .is_some_and(|path| path.starts_with(changed))
                            })
                        }) =>
                {
                    let _ = events.try_send(HostEvent::ConfigChanged);
                }
                Err(error) => log::warn!(target: "zz::tray", "config watcher failed: {error}"),
                _ => {}
            })?;
        let mut settings = Self {
            candidates,
            watched: BTreeSet::new(),
            watcher,
        };
        settings.refresh();
        Ok(settings)
    }

    pub(super) fn enabled(&self) -> bool {
        crate::config::tray_enabled_from_files(&self.candidates)
    }

    pub(super) fn refresh(&mut self) {
        let paths: BTreeSet<_> = self
            .candidates
            .iter()
            .filter_map(|path| {
                path.ancestors()
                    .skip(1)
                    .find(|parent| parent.is_dir())
                    .map(PathBuf::from)
            })
            .collect();
        for path in paths.difference(&self.watched) {
            if let Err(error) = self.watcher.watch(path, RecursiveMode::NonRecursive) {
                log::warn!(target: "zz::tray", "could not watch {}: {error}", path.display());
            }
        }
        for path in self.watched.difference(&paths) {
            let _ = self.watcher.unwatch(path);
        }
        self.watched = paths;
    }
}

fn resolved_path(path: &Path) -> Option<PathBuf> {
    path.ancestors().find_map(|parent| {
        parent
            .canonicalize()
            .ok()
            .map(|resolved| resolved.join(path.strip_prefix(parent).expect("ancestor prefix")))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn watches_config_creation_replacement_and_removal_with_precedence() {
        let directory = tempfile::tempdir().expect("directory");
        let first = directory.path().join("first");
        let second = directory.path().join("second");
        std::fs::write(&second, "tray = false\n").expect("config");
        let (sender, receiver) = async_channel::unbounded();
        let mut settings =
            Settings::with_candidates(vec![first.clone(), second.clone()], sender).expect("watch");
        assert!(!settings.enabled());
        std::fs::write(&first, "tray = true\n").expect("preferred config");
        wait_for_change(&receiver);
        settings.refresh();
        assert!(settings.enabled());
        let replacement = directory.path().join("replacement");
        std::fs::write(&replacement, "tray = false\n").expect("replacement");
        std::fs::rename(replacement, &first).expect("replace");
        wait_for_change(&receiver);
        assert!(!settings.enabled());
        std::fs::remove_file(first).expect("remove preferred config");
        wait_for_change(&receiver);
        assert!(!settings.enabled());
        std::fs::remove_file(second).expect("remove fallback config");
        wait_for_change(&receiver);
        assert!(settings.enabled());
    }

    fn wait_for_change(receiver: &async_channel::Receiver<HostEvent>) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            if matches!(receiver.try_recv(), Ok(HostEvent::ConfigChanged)) {
                return;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "missing config notification"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }
}
