use crate::android::{bridge, dispatcher::AndroidDispatcher, display::AndroidDisplay, window};
use anyhow::{Result, anyhow};
use futures::channel::oneshot;
use parking_lot::Mutex;
use std::{
    borrow::Cow,
    cell::{Cell, RefCell},
    path::{Path, PathBuf},
    ptr::NonNull,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};
use zz_gpui::{
    Action, AnyWindowHandle, BackgroundExecutor, ClipboardItem, CursorStyle, DummyKeyboardMapper,
    ForegroundExecutor, Keymap, Menu, MenuItem, PathPromptOptions, Platform, PlatformDisplay,
    PlatformKeyboardLayout, PlatformKeyboardMapper, PlatformTextSystem, PlatformWindow, Task,
    ThermalState, WindowAppearance, WindowParams,
};

thread_local! {
    static PLATFORM: Cell<*const AndroidPlatform> = const { Cell::new(std::ptr::null()) };
    static LAUNCHED: Cell<bool> = const { Cell::new(false) };
    static PENDING_URLS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

static IDLE_GUARDS: AtomicUsize = AtomicUsize::new(0);

pub struct AndroidPlatform(Mutex<AndroidPlatformState>);

struct AndroidPlatformState {
    background_executor: BackgroundExecutor,
    foreground_executor: ForegroundExecutor,
    text_system: Arc<dyn PlatformTextSystem>,
    finish_launching: Option<Box<dyn FnOnce()>>,
    reopen: Option<Box<dyn FnMut()>>,
    open_urls: Option<Box<dyn FnMut(Vec<String>)>>,
    touch_gestures: bool,
    appearance: Option<WindowAppearance>,
}

impl Default for AndroidPlatform {
    fn default() -> Self {
        Self::new()
    }
}

impl AndroidPlatform {
    /// Builds the platform; call it on the main thread, from the `main` that
    /// [`crate::android::on_load`] runs.
    pub fn new() -> Self {
        let dispatcher = Arc::new(AndroidDispatcher::new());
        let text_system: Arc<dyn PlatformTextSystem> = Arc::new(
            crate::wgpu::CosmicTextSystem::new_without_system_fonts("Lilex"),
        );
        let started = std::time::Instant::now();
        let fonts = system_fonts();
        let count = fonts.len();
        text_system.add_fonts(fonts).ok();
        log::info!("loaded {count} system fonts in {:?}", started.elapsed());
        Self(Mutex::new(AndroidPlatformState {
            background_executor: BackgroundExecutor::new(dispatcher.clone()),
            foreground_executor: ForegroundExecutor::new(dispatcher),
            text_system,
            finish_launching: None,
            reopen: None,
            open_urls: None,
            touch_gestures: false,
            appearance: None,
        }))
    }

    pub fn with_touch_gestures(self, enabled: bool) -> Self {
        self.0.lock().touch_gestures = enabled;
        self
    }
}

fn platform() -> Option<&'static AndroidPlatform> {
    let platform = PLATFORM.get();
    (!platform.is_null()).then(|| unsafe { &*platform })
}

pub(crate) fn appearance_is_dark() -> bool {
    let appearance = platform()
        .and_then(|platform| platform.0.lock().appearance)
        .unwrap_or_else(system_appearance);
    matches!(
        appearance,
        WindowAppearance::Dark | WindowAppearance::VibrantDark
    )
}

pub(crate) fn system_appearance() -> WindowAppearance {
    if bridge::configuration().night {
        WindowAppearance::Dark
    } else {
        WindowAppearance::Light
    }
}

pub(crate) fn touch_gestures() -> bool {
    platform().is_some_and(|platform| platform.0.lock().touch_gestures)
}

pub(crate) fn activity_created(main: Option<fn()>, url: Option<String>) {
    if let Some(url) = url {
        PENDING_URLS.with_borrow_mut(|urls| urls.push(url));
    }
    if platform().is_none()
        && let Some(main) = main
    {
        main();
    }
}

pub(crate) fn activity_destroyed() {
    window::close_all();
}

pub(crate) fn surface_created(native: NonNull<ndk_sys::ANativeWindow>) {
    window::set_surface(Some(native));
    if !LAUNCHED.get() {
        let callback = platform().and_then(|platform| platform.0.lock().finish_launching.take());
        if let Some(callback) = callback {
            LAUNCHED.set(true);
            callback();
        }
    } else if !window::has_window() {
        let callback = platform().and_then(|platform| platform.0.lock().reopen.take());
        if let Some(mut callback) = callback {
            callback();
            if let Some(platform) = platform() {
                platform.0.lock().reopen = Some(callback);
            }
        }
    }
    deliver_urls();
}

pub(crate) fn surface_destroyed() {
    window::set_surface(None);
}

pub(crate) fn open_urls(urls: Vec<String>) {
    PENDING_URLS.with_borrow_mut(|pending| pending.extend(urls));
    if LAUNCHED.get() {
        deliver_urls();
    }
}

fn deliver_urls() {
    let urls = PENDING_URLS.with_borrow_mut(std::mem::take);
    if urls.is_empty() {
        return;
    }
    let Some(platform) = platform() else {
        return;
    };
    let callback = platform.0.lock().open_urls.take();
    match callback {
        Some(mut callback) => {
            callback(urls);
            platform.0.lock().open_urls = Some(callback);
        }
        None => PENDING_URLS.with_borrow_mut(|pending| pending.extend(urls)),
    }
}

impl Platform for AndroidPlatform {
    fn background_executor(&self) -> BackgroundExecutor {
        self.0.lock().background_executor.clone()
    }

    fn foreground_executor(&self) -> ForegroundExecutor {
        self.0.lock().foreground_executor.clone()
    }

    fn text_system(&self) -> Arc<dyn PlatformTextSystem> {
        self.0.lock().text_system.clone()
    }

    fn run(&self, on_finish_launching: Box<dyn 'static + FnOnce()>) {
        PLATFORM.set(self as *const _);
        self.0.lock().finish_launching = Some(on_finish_launching);
        if window::surface().is_some() {
            LAUNCHED.set(true);
            let callback = self.0.lock().finish_launching.take();
            if let Some(callback) = callback {
                callback();
            }
            deliver_urls();
        }
    }

    fn quit(&self) {}

    fn restart(&self, _binary_path: Option<PathBuf>, _arguments: Vec<std::ffi::OsString>) {}

    fn activate(&self, _ignoring_other_apps: bool) {}

    fn hide(&self) {}

    fn hide_other_apps(&self) {}

    fn unhide_other_apps(&self) {}

    fn displays(&self) -> Vec<Rc<dyn PlatformDisplay>> {
        vec![Rc::new(AndroidDisplay)]
    }

    fn primary_display(&self) -> Option<Rc<dyn PlatformDisplay>> {
        Some(Rc::new(AndroidDisplay))
    }

    fn active_window(&self) -> Option<AnyWindowHandle> {
        window::active_window()
    }

    fn open_window(
        &self,
        handle: AnyWindowHandle,
        params: WindowParams,
    ) -> Result<Box<dyn PlatformWindow>> {
        Ok(Box::new(window::AndroidWindow::open(handle, params)?))
    }

    fn window_appearance(&self) -> WindowAppearance {
        self.0.lock().appearance.unwrap_or_else(system_appearance)
    }

    fn set_window_appearance(&self, appearance: Option<WindowAppearance>) {
        let dark = appearance_is_dark();
        self.0.lock().appearance = appearance;
        if appearance_is_dark() != dark {
            crate::android::dispatcher::defer(window::appearance_changed);
        }
    }

    fn open_url(&self, url: &str) {
        let url = url.to_owned();
        crate::android::dispatcher::defer(move || bridge::open_url(&url));
    }

    fn on_open_urls(&self, callback: Box<dyn FnMut(Vec<String>)>) {
        self.0.lock().open_urls = Some(callback);
    }

    fn register_url_scheme(&self, _url: &str) -> Task<Result<()>> {
        Task::ready(Err(anyhow!("register_url_scheme unsupported on Android")))
    }

    fn prompt_for_paths(
        &self,
        _options: PathPromptOptions,
    ) -> oneshot::Receiver<Result<Option<Vec<PathBuf>>>> {
        let (tx, rx) = oneshot::channel();
        tx.send(Ok(None)).ok();
        rx
    }

    fn prompt_for_new_path(
        &self,
        _directory: &Path,
        _suggested_name: Option<&str>,
    ) -> oneshot::Receiver<Result<Option<PathBuf>>> {
        let (tx, rx) = oneshot::channel();
        tx.send(Ok(None)).ok();
        rx
    }

    fn can_select_mixed_files_and_dirs(&self) -> bool {
        false
    }

    fn reveal_path(&self, _path: &Path) {}

    fn open_with_system(&self, _path: &Path) {}

    fn on_quit(&self, _callback: Box<dyn FnMut() -> bool>) {}

    fn on_reopen(&self, callback: Box<dyn FnMut()>) {
        self.0.lock().reopen = Some(callback);
    }

    fn on_system_sleep(&self, _callback: Box<dyn FnMut()>) {}

    fn prevent_idle_sleep(&self, _reason: &str) -> Task<Result<zz_gpui::ActivityGuard>> {
        if IDLE_GUARDS.fetch_add(1, Ordering::AcqRel) == 0 {
            crate::android::dispatcher::defer(sync_idle_timer);
        }
        Task::ready(Ok(zz_gpui::ActivityGuard::new(|| {
            if IDLE_GUARDS.fetch_sub(1, Ordering::AcqRel) == 1 {
                crate::android::dispatcher::defer(sync_idle_timer);
            }
        })))
    }

    fn on_system_wake(&self, _callback: Box<dyn FnMut()>) {}

    fn set_menus(&self, _menus: Vec<Menu>, _keymap: &Keymap) {}

    fn set_dock_menu(&self, _menu: Vec<MenuItem>, _keymap: &Keymap) {}

    fn on_app_menu_action(&self, _callback: Box<dyn FnMut(&dyn Action)>) {}

    fn on_will_open_app_menu(&self, _callback: Box<dyn FnMut()>) {}

    fn on_validate_app_menu_command(&self, _callback: Box<dyn FnMut(&dyn Action) -> bool>) {}

    fn thermal_state(&self) -> ThermalState {
        ThermalState::Nominal
    }

    fn on_thermal_state_change(&self, _callback: Box<dyn FnMut()>) {}

    fn app_path(&self) -> Result<PathBuf> {
        Err(anyhow!("app_path unsupported on Android"))
    }

    fn path_for_auxiliary_executable(&self, _name: &str) -> Result<PathBuf> {
        Err(anyhow!("auxiliary executables unsupported on Android"))
    }

    fn set_cursor_style(&self, _style: CursorStyle) {}

    fn hide_cursor_until_mouse_moves(&self) {}

    fn is_cursor_visible(&self) -> bool {
        false
    }

    fn gestures(&self) -> Option<Rc<dyn zz_gpui::PlatformGestures>> {
        Some(Rc::new(AndroidGestures))
    }

    fn should_auto_hide_scrollbars(&self) -> bool {
        true
    }

    fn read_from_clipboard(&self) -> Option<ClipboardItem> {
        read_clipboard()
    }

    fn write_to_clipboard(&self, item: ClipboardItem) {
        let text = item.text();
        let image = item.entries().iter().find_map(|entry| match entry {
            zz_gpui::ClipboardEntry::Image(image) => match image.format() {
                zz_gpui::ImageFormat::Png => Some((image.bytes().to_vec(), "image/png")),
                zz_gpui::ImageFormat::Jpeg => Some((image.bytes().to_vec(), "image/jpeg")),
                _ => None,
            },
            _ => None,
        });
        bridge::set_clipboard(
            text.as_deref(),
            image
                .as_ref()
                .map(|(bytes, kind)| (bytes.as_slice(), *kind)),
        );
    }

    fn write_credentials(&self, _url: &str, _username: &str, _password: &[u8]) -> Task<Result<()>> {
        Task::ready(Err(anyhow!("credentials unsupported on Android")))
    }

    fn read_credentials(&self, _url: &str) -> Task<Result<Option<(String, Vec<u8>)>>> {
        Task::ready(Ok(None))
    }

    fn delete_credentials(&self, _url: &str) -> Task<Result<()>> {
        Task::ready(Err(anyhow!("credentials unsupported on Android")))
    }

    fn keyboard_layout(&self) -> Box<dyn PlatformKeyboardLayout> {
        Box::new(AndroidKeyboardLayout)
    }

    fn keyboard_mapper(&self) -> Rc<dyn PlatformKeyboardMapper> {
        Rc::new(DummyKeyboardMapper)
    }

    fn on_keyboard_layout_change(&self, _callback: Box<dyn FnMut()>) {}
}

struct AndroidGestures;

impl zz_gpui::PlatformGestures for AndroidGestures {
    fn tuning(&self) -> zz_gpui::GestureTuning {
        zz_gpui::GestureTuning {
            overscroll: zz_gpui::Overscroll::Bounce,
            ..Default::default()
        }
    }
}

struct AndroidKeyboardLayout;

impl PlatformKeyboardLayout for AndroidKeyboardLayout {
    fn id(&self) -> &str {
        "android"
    }

    fn name(&self) -> &str {
        "Android"
    }
}

fn sync_idle_timer() {
    bridge::set_keep_screen_on(IDLE_GUARDS.load(Ordering::Acquire) > 0);
}

pub(crate) fn read_clipboard() -> Option<ClipboardItem> {
    let mut entries = Vec::new();
    if let Some((kind, bytes)) = bridge::clipboard_image() {
        let format = if kind == "image/png" {
            zz_gpui::ImageFormat::Png
        } else {
            zz_gpui::ImageFormat::Jpeg
        };
        entries.push(zz_gpui::ClipboardEntry::Image(zz_gpui::Image::from_bytes(
            format, bytes,
        )));
    }
    if let Some(text) = bridge::clipboard_text() {
        entries.push(zz_gpui::ClipboardEntry::String(
            zz_gpui::ClipboardString::new(text),
        ));
    }
    (!entries.is_empty()).then_some(ClipboardItem { entries })
}

fn system_fonts() -> Vec<Cow<'static, [u8]>> {
    let Ok(entries) = std::fs::read_dir("/system/fonts") else {
        return Vec::new();
    };
    entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| matches!(extension.to_str(), Some("ttf" | "ttc" | "otf")))
        })
        .filter_map(|path| {
            let file = std::fs::File::open(path).ok()?;
            let map: &'static memmap2::Mmap =
                Box::leak(Box::new(unsafe { memmap2::Mmap::map(&file) }.ok()?));
            Some(Cow::Borrowed(&map[..]))
        })
        .collect()
}
