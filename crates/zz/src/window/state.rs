use std::{
    fs, io,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};

use gpui::{App, Bounds, Context, DisplayId, Pixels, Size, Window, WindowBounds, point, px, size};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use crate::{
    config::atomic_write,
    user_data::{platform_data_dir, restrict_directory_to_current_user, restrict_to_current_user},
};

const WINDOW_STATE_FILE_NAME: &str = "window-state.json";
const WINDOW_STATE_VERSION: u8 = 1;
const MAX_WINDOW_STATE_BYTES: u64 = 4 * 1024;
const MAX_COORDINATE: f32 = 1_000_000.0;
const MAX_DIMENSION: f32 = 100_000.0;
const SAVE_DELAY: Duration = Duration::from_millis(250);

#[derive(Clone, Copy, Debug)]
pub(crate) struct RestoredWindow {
    pub(crate) bounds: WindowBounds,
    pub(crate) display_id: Option<DisplayId>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct MainWindowState {
    inner: Arc<WindowStateInner>,
}

#[derive(Debug, Default)]
struct WindowStateInner {
    path: Option<PathBuf>,
    latest: Mutex<Option<StoredWindowState>>,
    revision: AtomicU64,
    save_queued: AtomicBool,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
struct StoredWindowState {
    version: u8,
    display_uuid: Option<String>,
    mode: WindowMode,
    bounds: StoredBounds,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum WindowMode {
    Windowed,
    Maximized,
    Fullscreen,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
struct StoredBounds {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
}

impl MainWindowState {
    pub(crate) fn load_persistent() -> Self {
        let path = match window_state_path() {
            Ok(path) => path,
            Err(error) => {
                log::warn!(target: "zz::window_state", "window state is not persisted: {error}");
                return Self::default();
            }
        };
        let latest = match load_at(&path) {
            Ok(stored) => Some(stored),
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => {
                log::warn!(
                    target: "zz::window_state",
                    "could not load window state path={} error={error}",
                    path.display(),
                );
                None
            }
        };
        Self {
            inner: Arc::new(WindowStateInner {
                path: Some(path),
                latest: Mutex::new(latest),
                revision: AtomicU64::new(0),
                save_queued: AtomicBool::new(false),
            }),
        }
    }

    pub(crate) fn restored_window(
        &self,
        cx: &App,
        default_size: Size<Pixels>,
        minimum_size: Size<Pixels>,
    ) -> RestoredWindow {
        let Some(stored) = self.inner.latest.lock().clone() else {
            return RestoredWindow {
                bounds: WindowBounds::Windowed(Bounds::centered(None, default_size, cx)),
                display_id: None,
            };
        };
        let saved_bounds = stored.bounds.to_pixels();
        let displays = cx.displays();
        if displays.is_empty() {
            return RestoredWindow {
                bounds: stored.mode.with_bounds(saved_bounds),
                display_id: None,
            };
        }

        let matching_display = stored.display_uuid.as_deref().and_then(|uuid| {
            displays
                .iter()
                .find(|display| {
                    display
                        .uuid()
                        .is_ok_and(|candidate| candidate.to_string() == uuid)
                })
                .cloned()
        });
        let intersecting_display = displays
            .iter()
            .find(|display| display.visible_bounds().intersects(&saved_bounds))
            .cloned();
        let keep_origin = matching_display.is_some() || intersecting_display.is_some();
        let display = matching_display
            .or(intersecting_display)
            .or_else(|| cx.primary_display())
            .or_else(|| displays.first().cloned());
        let Some(display) = display else {
            return RestoredWindow {
                bounds: stored.mode.with_bounds(saved_bounds),
                display_id: None,
            };
        };
        let visible_bounds = usable_display_bounds(display.visible_bounds(), display.bounds());
        let restored_bounds =
            fit_to_display(saved_bounds, visible_bounds, minimum_size, !keep_origin);
        RestoredWindow {
            bounds: stored.mode.with_bounds(restored_bounds),
            display_id: Some(display.id()),
        }
    }

    pub(crate) fn capture_and_flush(&self, window: &Window, cx: &App) {
        self.capture(window, cx, false);
        self.flush();
    }

    pub(crate) fn flush(&self) {
        self.inner.revision.fetch_add(1, Ordering::AcqRel);
        self.save_latest_with_warning();
    }

    fn capture(&self, window: &Window, cx: &App, debounce: bool) {
        let display_uuid = window
            .display(cx)
            .and_then(|display| display.uuid().ok())
            .map(|uuid| uuid.to_string());
        let stored = StoredWindowState::from_window(window.inner_window_bounds(), display_uuid);
        if !stored.is_valid() {
            log::warn!(target: "zz::window_state", "refusing to persist invalid window bounds");
            return;
        }
        *self.inner.latest.lock() = Some(stored);
        self.inner.revision.fetch_add(1, Ordering::AcqRel);
        if !debounce || self.inner.path.is_none() {
            return;
        }
        self.queue_debounced_save(cx);
    }

    fn queue_debounced_save(&self, cx: &App) {
        if self.inner.save_queued.swap(true, Ordering::AcqRel) {
            return;
        }
        let state = self.clone();
        let timer_executor = cx.background_executor().clone();
        cx.background_executor()
            .spawn(async move {
                loop {
                    let revision = state.inner.revision.load(Ordering::Acquire);
                    timer_executor.timer(SAVE_DELAY).await;
                    if state.inner.revision.load(Ordering::Acquire) != revision {
                        continue;
                    }
                    state.save_latest_with_warning();

                    state.inner.save_queued.store(false, Ordering::Release);
                    if state.inner.revision.load(Ordering::Acquire) == revision
                        || state.inner.save_queued.swap(true, Ordering::AcqRel)
                    {
                        break;
                    }
                }
            })
            .detach();
    }

    fn save_latest_with_warning(&self) {
        let Some(path) = &self.inner.path else {
            return;
        };
        let Some(stored) = self.inner.latest.lock().clone() else {
            return;
        };
        if let Err(error) = save_at(path, &stored) {
            log::warn!(
                target: "zz::window_state",
                "could not persist window state path={} error={error}",
                path.display(),
            );
        }
    }
}

pub(crate) fn observe<T: 'static>(
    state: MainWindowState,
    window: &mut Window,
    cx: &mut Context<T>,
) {
    state.capture(window, cx, true);
    cx.observe_window_bounds(window, move |_, window, cx| {
        state.capture(window, cx, true);
    })
    .detach();
}

impl StoredWindowState {
    fn from_window(bounds: WindowBounds, display_uuid: Option<String>) -> Self {
        let (mode, bounds) = match bounds {
            WindowBounds::Windowed(bounds) => (WindowMode::Windowed, bounds),
            WindowBounds::Maximized(bounds) => (WindowMode::Maximized, bounds),
            WindowBounds::Fullscreen(bounds) => (WindowMode::Fullscreen, bounds),
        };
        Self {
            version: WINDOW_STATE_VERSION,
            display_uuid,
            mode,
            bounds: StoredBounds::from_pixels(bounds),
        }
    }

    fn is_valid(&self) -> bool {
        self.version == WINDOW_STATE_VERSION
            && self.bounds.is_valid()
            && self.display_uuid.as_ref().is_none_or(|uuid| {
                !uuid.is_empty()
                    && uuid.len() <= 64
                    && uuid
                        .bytes()
                        .all(|byte| byte.is_ascii_hexdigit() || byte == b'-')
            })
    }
}

impl WindowMode {
    fn with_bounds(self, bounds: Bounds<Pixels>) -> WindowBounds {
        match self {
            Self::Windowed => WindowBounds::Windowed(bounds),
            Self::Maximized => WindowBounds::Maximized(bounds),
            Self::Fullscreen => WindowBounds::Fullscreen(bounds),
        }
    }
}

impl StoredBounds {
    fn from_pixels(bounds: Bounds<Pixels>) -> Self {
        Self {
            x: bounds.origin.x.as_f32(),
            y: bounds.origin.y.as_f32(),
            width: bounds.size.width.as_f32(),
            height: bounds.size.height.as_f32(),
        }
    }

    fn to_pixels(self) -> Bounds<Pixels> {
        Bounds::new(
            point(px(self.x), px(self.y)),
            size(px(self.width), px(self.height)),
        )
    }

    fn is_valid(self) -> bool {
        [self.x, self.y, self.width, self.height]
            .into_iter()
            .all(f32::is_finite)
            && self.x.abs() <= MAX_COORDINATE
            && self.y.abs() <= MAX_COORDINATE
            && (1.0..=MAX_DIMENSION).contains(&self.width)
            && (1.0..=MAX_DIMENSION).contains(&self.height)
    }
}

fn usable_display_bounds(visible: Bounds<Pixels>, full: Bounds<Pixels>) -> Bounds<Pixels> {
    if visible.size.width.as_f32() > 0.0 && visible.size.height.as_f32() > 0.0 {
        visible
    } else {
        full
    }
}

fn fit_to_display(
    requested: Bounds<Pixels>,
    display: Bounds<Pixels>,
    minimum_size: Size<Pixels>,
    center: bool,
) -> Bounds<Pixels> {
    let display_width = display.size.width.as_f32().max(1.0);
    let display_height = display.size.height.as_f32().max(1.0);
    let minimum_width = minimum_size.width.as_f32().clamp(1.0, display_width);
    let minimum_height = minimum_size.height.as_f32().clamp(1.0, display_height);
    let width = requested
        .size
        .width
        .as_f32()
        .clamp(minimum_width, display_width);
    let height = requested
        .size
        .height
        .as_f32()
        .clamp(minimum_height, display_height);
    let display_x = display.origin.x.as_f32();
    let display_y = display.origin.y.as_f32();
    let x = if center {
        display_x + (display_width - width) / 2.0
    } else {
        requested
            .origin
            .x
            .as_f32()
            .clamp(display_x, display_x + display_width - width)
    };
    let y = if center {
        display_y + (display_height - height) / 2.0
    } else {
        requested
            .origin
            .y
            .as_f32()
            .clamp(display_y, display_y + display_height - height)
    };
    Bounds::new(point(px(x), px(y)), size(px(width), px(height)))
}

fn load_at(path: &Path) -> io::Result<StoredWindowState> {
    let metadata = fs::metadata(path)?;
    if metadata.len() > MAX_WINDOW_STATE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "window state file is too large",
        ));
    }
    let bytes = fs::read(path)?;
    let stored: StoredWindowState = serde_json::from_slice(&bytes)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    if !stored.is_valid() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "window state is invalid or unsupported",
        ));
    }
    Ok(stored)
}

fn save_at(path: &Path, stored: &StoredWindowState) -> io::Result<()> {
    if !stored.is_valid() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "window state is invalid",
        ));
    }
    let contents = serde_json::to_vec(stored)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    if u64::try_from(contents.len()).unwrap_or(u64::MAX) > MAX_WINDOW_STATE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "window state exceeds its size limit",
        ));
    }
    prepare_parent_directory(path)?;
    atomic_write(path, &contents)?;
    restrict_to_current_user(path)
}

fn prepare_parent_directory(path: &Path) -> io::Result<()> {
    let parent = path.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "window state path has no parent directory",
        )
    })?;
    fs::create_dir_all(parent)?;
    restrict_directory_to_current_user(parent)
}

fn window_state_path() -> io::Result<PathBuf> {
    let data = platform_data_dir().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "could not resolve the current user's application-data directory",
        )
    })?;
    Ok(data.join("zz").join(WINDOW_STATE_FILE_NAME))
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;

    fn bounds(x: f32, y: f32, width: f32, height: f32) -> Bounds<Pixels> {
        Bounds::new(point(px(x), px(y)), size(px(width), px(height)))
    }

    #[test]
    fn state_round_trips_all_window_modes() {
        let directory = tempdir().expect("temporary directory");
        let path = directory.path().join(WINDOW_STATE_FILE_NAME);
        for mode in [
            WindowMode::Windowed,
            WindowMode::Maximized,
            WindowMode::Fullscreen,
        ] {
            let stored = StoredWindowState {
                version: WINDOW_STATE_VERSION,
                display_uuid: Some("01234567-89ab-cdef-0123-456789abcdef".to_owned()),
                mode,
                bounds: StoredBounds {
                    x: 120.25,
                    y: 80.5,
                    width: 1080.75,
                    height: 720.25,
                },
            };
            save_at(&path, &stored).expect("save window state");
            assert_eq!(load_at(&path).expect("load window state"), stored);
        }

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            assert_eq!(
                fs::metadata(path).expect("metadata").permissions().mode() & 0o777,
                0o600
            );
            assert_eq!(
                fs::metadata(directory.path())
                    .expect("directory metadata")
                    .permissions()
                    .mode()
                    & 0o777,
                0o700
            );
        }
    }

    #[test]
    fn oversized_or_invalid_state_is_rejected() {
        let directory = tempdir().expect("temporary directory");
        let path = directory.path().join(WINDOW_STATE_FILE_NAME);
        fs::write(&path, vec![b' '; MAX_WINDOW_STATE_BYTES as usize + 1]).expect("oversized state");
        assert_eq!(
            load_at(&path).expect_err("reject oversized state").kind(),
            io::ErrorKind::InvalidData
        );

        let invalid = StoredWindowState {
            version: WINDOW_STATE_VERSION,
            display_uuid: None,
            mode: WindowMode::Windowed,
            bounds: StoredBounds {
                x: 0.0,
                y: 0.0,
                width: 0.0,
                height: 720.0,
            },
        };
        assert_eq!(
            save_at(&path, &invalid)
                .expect_err("reject invalid bounds")
                .kind(),
            io::ErrorKind::InvalidData
        );
    }

    #[test]
    fn bounds_are_clamped_to_the_usable_display() {
        let fitted = fit_to_display(
            bounds(-500.0, -100.0, 1600.0, 1000.0),
            bounds(0.0, 0.0, 1200.0, 800.0),
            size(px(480.0), px(320.0)),
            false,
        );
        assert_eq!(fitted, bounds(0.0, 0.0, 1200.0, 800.0));
    }

    #[test]
    fn offscreen_bounds_are_centered_on_the_fallback_display() {
        let fitted = fit_to_display(
            bounds(-2000.0, 300.0, 800.0, 600.0),
            bounds(1920.0, 0.0, 1920.0, 1080.0),
            size(px(480.0), px(320.0)),
            true,
        );
        assert_eq!(fitted, bounds(2480.0, 240.0, 800.0, 600.0));
    }
}
