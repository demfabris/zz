use crate::{IosDisplay, IosWindow, nil, ns_string};
use anyhow::{Result, anyhow};
use futures::channel::oneshot;
use gpui::{
    Action, AnyWindowHandle, BackgroundExecutor, ClipboardItem, CursorStyle, DummyKeyboardMapper,
    ForegroundExecutor, Keymap, Menu, MenuItem, PathPromptOptions, Platform, PlatformDisplay,
    PlatformKeyboardLayout, PlatformKeyboardMapper, PlatformTextSystem, PlatformWindow, Task,
    ThermalState, WindowAppearance, WindowParams,
};
use objc::{
    class,
    declare::ClassDecl,
    msg_send,
    runtime::{BOOL, Object, Protocol, Sel, YES},
    sel, sel_impl,
};
use parking_lot::Mutex;
use std::{
    borrow::Cow,
    ffi::{c_char, c_int},
    path::{Path, PathBuf},
    rc::Rc,
    sync::{
        Arc, Once,
        atomic::{AtomicPtr, AtomicUsize, Ordering},
    },
};

use crate::id;

static PLATFORM: AtomicPtr<IosPlatform> = AtomicPtr::new(std::ptr::null_mut());
static REGISTER_DELEGATE: Once = Once::new();

static PENDING_SCENE: AtomicPtr<Object> = AtomicPtr::new(std::ptr::null_mut());
static IDLE_GUARDS: AtomicUsize = AtomicUsize::new(0);
static BACKGROUND_TASK: AtomicUsize = AtomicUsize::new(0);

pub(crate) fn take_window_scene() -> id {
    let pending = PENDING_SCENE.swap(std::ptr::null_mut(), Ordering::AcqRel);
    if !pending.is_null() {
        return pending;
    }
    unsafe {
        let application: id = msg_send![class!(UIApplication), sharedApplication];
        let scenes: id = msg_send![application, connectedScenes];
        let scenes: id = msg_send![scenes, allObjects];
        let count: usize = msg_send![scenes, count];
        (0..count)
            .map(|index| -> id { msg_send![scenes, objectAtIndex: index] })
            .find(|scene| {
                let is_window_scene: BOOL = msg_send![*scene, isKindOfClass: class!(UIWindowScene)];
                is_window_scene == YES
            })
            .unwrap_or(std::ptr::null_mut())
    }
}

unsafe extern "C" {
    fn UIApplicationMain(
        argc: c_int,
        argv: *mut *mut c_char,
        principal_class_name: id,
        delegate_class_name: id,
    ) -> c_int;
}

pub struct IosPlatform(Mutex<IosPlatformState>);

pub(crate) struct IosPlatformState {
    background_executor: BackgroundExecutor,
    foreground_executor: ForegroundExecutor,
    text_system: Arc<dyn PlatformTextSystem>,
    finish_launching: Option<Box<dyn FnOnce()>>,
    reopen: Option<Box<dyn FnMut()>>,
    touch_gestures: bool,
    appearance: Option<WindowAppearance>,
    menus: crate::menu::MenuState,
    menu_action: Option<Box<dyn FnMut(&dyn Action)>>,
    open_urls: Option<Box<dyn FnMut(Vec<String>)>>,
}

impl Default for IosPlatform {
    fn default() -> Self {
        Self::new()
    }
}

impl IosPlatform {
    pub fn new() -> Self {
        let dispatcher = Arc::new(crate::IosDispatcher::new());

        let text_system: Arc<dyn PlatformTextSystem> = Arc::new(
            gpui_wgpu::CosmicTextSystem::new_without_system_fonts("Lilex"),
        );
        text_system.add_fonts(system_fonts()).ok();

        Self(Mutex::new(IosPlatformState {
            background_executor: BackgroundExecutor::new(dispatcher.clone()),
            foreground_executor: ForegroundExecutor::new(dispatcher),
            text_system,
            finish_launching: None,
            reopen: None,
            touch_gestures: false,
            appearance: None,
            menus: Default::default(),
            menu_action: None,
            open_urls: None,
        }))
    }

    pub fn with_touch_gestures(self, enabled: bool) -> Self {
        self.0.lock().touch_gestures = enabled;
        self
    }
}

struct IosGestures;

impl gpui::PlatformGestures for IosGestures {
    fn tuning(&self) -> gpui::GestureTuning {
        gpui::GestureTuning {
            overscroll: gpui::Overscroll::Bounce,
            ..Default::default()
        }
    }
}

impl Platform for IosPlatform {
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
        REGISTER_DELEGATE.call_once(register_delegate_classes);
        self.0.lock().finish_launching = Some(on_finish_launching);
        PLATFORM.store(self as *const _ as *mut IosPlatform, Ordering::Release);
        unsafe {
            UIApplicationMain(0, std::ptr::null_mut(), nil, ns_string("ZZGPUIAppDelegate"));
        }
    }

    fn quit(&self) {}

    fn restart(&self, _binary_path: Option<PathBuf>, _arguments: Vec<std::ffi::OsString>) {}

    fn activate(&self, _ignoring_other_apps: bool) {}

    fn hide(&self) {}

    fn hide_other_apps(&self) {}

    fn unhide_other_apps(&self) {}

    fn displays(&self) -> Vec<Rc<dyn PlatformDisplay>> {
        vec![Rc::new(IosDisplay)]
    }

    fn primary_display(&self) -> Option<Rc<dyn PlatformDisplay>> {
        Some(Rc::new(IosDisplay))
    }

    fn active_window(&self) -> Option<AnyWindowHandle> {
        crate::window::active_window()
    }

    fn open_window(
        &self,
        handle: AnyWindowHandle,
        params: WindowParams,
    ) -> Result<Box<dyn PlatformWindow>> {
        let (touch_gestures, appearance) = {
            let state = self.0.lock();
            (state.touch_gestures, state.appearance)
        };
        Ok(Box::new(IosWindow::open(
            handle,
            params,
            touch_gestures,
            appearance,
        )?))
    }

    fn window_appearance(&self) -> WindowAppearance {
        self.0.lock().appearance.unwrap_or_else(screen_appearance)
    }

    fn set_window_appearance(&self, appearance: Option<WindowAppearance>) {
        self.0.lock().appearance = appearance;
        crate::window::set_window_appearance(appearance);
    }

    fn open_url(&self, url: &str) {
        let url = url.to_owned();
        self.foreground_executor()
            .spawn(async move {
                unsafe {
                    let ns_url: id = msg_send![class!(NSURL), URLWithString: ns_string(&url)];
                    if ns_url.is_null() {
                        log::warn!("could not create NSURL from {url}");
                        return;
                    }
                    let application: id = msg_send![class!(UIApplication), sharedApplication];
                    let options: id = msg_send![class!(NSDictionary), dictionary];
                    let _: () = msg_send![
                        application,
                        openURL: ns_url
                        options: options
                        completionHandler: nil
                    ];
                }
            })
            .detach();
    }

    fn on_open_urls(&self, callback: Box<dyn FnMut(Vec<String>)>) {
        self.0.lock().open_urls = Some(callback);
    }

    fn register_url_scheme(&self, _url: &str) -> Task<Result<()>> {
        Task::ready(Err(anyhow!("register_url_scheme unsupported on iOS")))
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

    fn prevent_idle_sleep(&self, _reason: &str) -> Task<Result<gpui::ActivityGuard>> {
        IDLE_GUARDS.fetch_add(1, Ordering::AcqRel);
        schedule_idle_timer_sync();
        Task::ready(Ok(gpui::ActivityGuard::new(|| {
            IDLE_GUARDS.fetch_sub(1, Ordering::AcqRel);
            schedule_idle_timer_sync();
        })))
    }

    fn on_system_wake(&self, _callback: Box<dyn FnMut()>) {}

    fn set_menus(&self, menus: Vec<Menu>, _keymap: &Keymap) {
        self.0.lock().menus = crate::menu::MenuState::new(menus);
        crate::menu::rebuild();
    }

    fn set_dock_menu(&self, _menu: Vec<MenuItem>, _keymap: &Keymap) {}

    fn on_app_menu_action(&self, callback: Box<dyn FnMut(&dyn Action)>) {
        self.0.lock().menu_action = Some(callback);
    }

    fn on_will_open_app_menu(&self, _callback: Box<dyn FnMut()>) {}

    fn on_validate_app_menu_command(&self, _callback: Box<dyn FnMut(&dyn Action) -> bool>) {}

    fn thermal_state(&self) -> ThermalState {
        unsafe {
            let info: id = msg_send![class!(NSProcessInfo), processInfo];
            let state: isize = msg_send![info, thermalState];
            let low_power: BOOL = msg_send![info, isLowPowerModeEnabled];
            match state {
                3 => ThermalState::Critical,
                2 => ThermalState::Serious,
                _ if low_power == YES => ThermalState::Serious,
                1 => ThermalState::Fair,
                _ => ThermalState::Nominal,
            }
        }
    }

    fn on_thermal_state_change(&self, _callback: Box<dyn FnMut()>) {}

    fn app_path(&self) -> Result<PathBuf> {
        Err(anyhow!("app_path unsupported on iOS"))
    }

    fn path_for_auxiliary_executable(&self, _name: &str) -> Result<PathBuf> {
        Err(anyhow!("auxiliary executables unsupported on iOS"))
    }

    fn set_cursor_style(&self, style: CursorStyle) {
        crate::window::set_cursor_style(style);
    }

    fn hide_cursor_until_mouse_moves(&self) {}

    fn is_cursor_visible(&self) -> bool {
        false
    }

    fn gestures(&self) -> Option<Rc<dyn gpui::PlatformGestures>> {
        Some(Rc::new(IosGestures))
    }

    fn should_auto_hide_scrollbars(&self) -> bool {
        true
    }

    fn read_from_clipboard(&self) -> Option<ClipboardItem> {
        unsafe {
            let pasteboard: id = msg_send![class!(UIPasteboard), generalPasteboard];
            if pasteboard.is_null() {
                return None;
            }
            let mut entries = Vec::new();
            for (kind, format) in [
                ("public.png", gpui::ImageFormat::Png),
                ("public.jpeg", gpui::ImageFormat::Jpeg),
            ] {
                let data: id = msg_send![pasteboard, dataForPasteboardType: ns_string(kind)];
                if data.is_null() {
                    continue;
                }
                let bytes: *const u8 = msg_send![data, bytes];
                let length: usize = msg_send![data, length];
                if !bytes.is_null() && length > 0 {
                    let bytes = std::slice::from_raw_parts(bytes, length).to_vec();
                    entries.push(gpui::ClipboardEntry::Image(gpui::Image::from_bytes(
                        format, bytes,
                    )));
                    break;
                }
            }
            let string: id = msg_send![pasteboard, string];
            if let Some(text) = crate::nsstring_to_string(string) {
                entries.push(gpui::ClipboardEntry::String(gpui::ClipboardString::new(
                    text,
                )));
            }
            (!entries.is_empty()).then_some(ClipboardItem { entries })
        }
    }

    fn write_to_clipboard(&self, item: ClipboardItem) {
        let Some(text) = item.text() else {
            return;
        };
        unsafe {
            let pasteboard: id = msg_send![class!(UIPasteboard), generalPasteboard];
            if pasteboard.is_null() {
                return;
            }
            let string = ns_string(&text);
            let _: () = msg_send![pasteboard, setString: string];
        }
    }

    fn write_credentials(&self, _url: &str, _username: &str, _password: &[u8]) -> Task<Result<()>> {
        Task::ready(Err(anyhow!("credentials unsupported on iOS")))
    }

    fn read_credentials(&self, _url: &str) -> Task<Result<Option<(String, Vec<u8>)>>> {
        Task::ready(Ok(None))
    }

    fn delete_credentials(&self, _url: &str) -> Task<Result<()>> {
        Task::ready(Err(anyhow!("credentials unsupported on iOS")))
    }

    fn keyboard_layout(&self) -> Box<dyn PlatformKeyboardLayout> {
        Box::new(IosKeyboardLayout)
    }

    fn keyboard_mapper(&self) -> Rc<dyn PlatformKeyboardMapper> {
        Rc::new(DummyKeyboardMapper)
    }

    fn on_keyboard_layout_change(&self, _callback: Box<dyn FnMut()>) {}
}

fn system_fonts() -> Vec<Cow<'static, [u8]>> {
    let root = PathBuf::from(std::env::var_os("IPHONE_SIMULATOR_ROOT").unwrap_or_default())
        .join("System/Library/Fonts");
    ["Core", "CoreAddition", "LanguageSupport", "UnicodeSupport"]
        .into_iter()
        .filter_map(|directory| std::fs::read_dir(root.join(directory)).ok())
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_stem().is_some_and(|stem| stem != "LastResort")
                && path.extension().is_some_and(|extension| {
                    matches!(extension.to_str(), Some("ttf" | "ttc" | "otf"))
                })
        })
        .filter_map(|path| {
            let file = std::fs::File::open(path).ok()?;
            let map: &'static memmap2::Mmap =
                Box::leak(Box::new(unsafe { memmap2::Mmap::map(&file) }.ok()?));
            Some(Cow::Borrowed(&map[..]))
        })
        .collect()
}

struct IosKeyboardLayout;

impl PlatformKeyboardLayout for IosKeyboardLayout {
    fn id(&self) -> &str {
        "ios"
    }

    fn name(&self) -> &str {
        "iOS"
    }
}

fn register_delegate_classes() {
    let mut decl = ClassDecl::new("ZZGPUIAppDelegate", class!(UIResponder)).unwrap();
    unsafe {
        decl.add_method(
            sel!(buildMenuWithBuilder:),
            build_menu as extern "C" fn(&Object, Sel, id),
        );
        decl.add_method(
            sel!(zzMenuCommand:),
            menu_command as extern "C" fn(&Object, Sel, id),
        );
        decl.add_method(
            sel!(application:didFinishLaunchingWithOptions:),
            did_finish_launching as extern "C" fn(&Object, Sel, id, id) -> BOOL,
        );
    }
    decl.register();
    register_scene_delegate_class();
}

fn register_scene_delegate_class() {
    let mut decl = ClassDecl::new("ZZGPUISceneDelegate", class!(NSObject)).unwrap();
    unsafe {
        for name in ["UISceneDelegate", "UIWindowSceneDelegate"] {
            if let Some(protocol) = Protocol::get(name) {
                decl.add_protocol(protocol);
            } else {
                log::error!("protocol {name} not found; conformance skipped");
            }
        }
        decl.add_method(
            sel!(scene:willConnectToSession:options:),
            scene_will_connect as extern "C" fn(&Object, Sel, id, id, id),
        );
        decl.add_method(
            sel!(scene:openURLContexts:),
            scene_open_urls as extern "C" fn(&Object, Sel, id, id),
        );
        decl.add_method(
            sel!(sceneDidDisconnect:),
            scene_did_disconnect as extern "C" fn(&Object, Sel, id),
        );
        decl.add_method(
            sel!(sceneDidBecomeActive:),
            scene_did_become_active as extern "C" fn(&Object, Sel, id),
        );
        decl.add_method(
            sel!(sceneWillResignActive:),
            scene_will_resign_active as extern "C" fn(&Object, Sel, id),
        );
        decl.add_method(
            sel!(sceneDidEnterBackground:),
            scene_did_enter_background as extern "C" fn(&Object, Sel, id),
        );
        decl.add_method(
            sel!(sceneWillEnterForeground:),
            scene_will_enter_foreground as extern "C" fn(&Object, Sel, id),
        );
        decl.add_method(
            sel!(windowScene:didUpdateCoordinateSpace:interfaceOrientation:traitCollection:),
            scene_did_update_geometry as extern "C" fn(&Object, Sel, id, id, i64, id),
        );
        decl.add_method(
            sel!(windowScene:didUpdateEffectiveGeometry:),
            scene_did_update_effective_geometry as extern "C" fn(&Object, Sel, id, id),
        );
    }
    decl.register();
}

fn scene_manifest_present() -> bool {
    unsafe {
        let bundle: id = msg_send![class!(NSBundle), mainBundle];
        let key = ns_string("UIApplicationSceneManifest");
        let manifest: id = msg_send![bundle, objectForInfoDictionaryKey: key];
        !manifest.is_null()
    }
}

extern "C" fn build_menu(_: &Object, _: Sel, builder: id) {
    let platform = PLATFORM.load(Ordering::Acquire);
    if !platform.is_null() {
        unsafe { (*platform).0.lock().menus.build(builder) };
    }
}

extern "C" fn menu_command(_: &Object, _: Sel, command: id) {
    let platform = PLATFORM.load(Ordering::Acquire);
    let Some(index) = crate::menu::command_index(command) else {
        return;
    };
    if platform.is_null() {
        return;
    }
    let (action, callback) = {
        let mut state = unsafe { (*platform).0.lock() };
        (state.menus.action(index), state.menu_action.take())
    };
    if let (Some(action), Some(mut callback)) = (action, callback) {
        callback(action.as_ref());
        unsafe { (*platform).0.lock().menu_action = Some(callback) };
    }
}

fn finish_launching() {
    let platform = PLATFORM.load(Ordering::Acquire);
    assert!(!platform.is_null(), "IosPlatform not registered before run");
    let callback = unsafe { (*platform).0.lock().finish_launching.take() };
    if let Some(callback) = callback {
        callback();
    }
}

extern "C" fn did_finish_launching(_this: &Object, _: Sel, _app: id, _opts: id) -> BOOL {
    if !scene_manifest_present() {
        finish_launching();
    }
    YES
}

extern "C" fn scene_will_connect(_this: &Object, _: Sel, scene: id, _session: id, options: id) {
    unsafe {
        let is_window_scene: BOOL = msg_send![scene, isKindOfClass: class!(UIWindowScene)];
        if is_window_scene != YES {
            return;
        }
    }
    PENDING_SCENE.store(scene, Ordering::Release);
    let platform = PLATFORM.load(Ordering::Acquire);
    let launching =
        !platform.is_null() && unsafe { (*platform).0.lock().finish_launching.is_some() };
    if launching {
        finish_launching();
    } else {
        reopen();
    }
    if !options.is_null() {
        open_urls(unsafe { msg_send![options, URLContexts] });
    }
}

extern "C" fn scene_open_urls(_: &Object, _: Sel, _: id, contexts: id) {
    open_urls(contexts);
}

fn open_urls(contexts: id) {
    let urls: Vec<String> = unsafe {
        if contexts.is_null() {
            return;
        }
        let contexts: id = msg_send![contexts, allObjects];
        let count: usize = msg_send![contexts, count];
        (0..count)
            .filter_map(|index| {
                let context: id = msg_send![contexts, objectAtIndex: index];
                let url: id = msg_send![context, URL];
                let string: id = msg_send![url, absoluteString];
                crate::nsstring_to_string(string)
            })
            .collect()
    };
    let platform = PLATFORM.load(Ordering::Acquire);
    if urls.is_empty() || platform.is_null() {
        return;
    }
    let callback = unsafe { (*platform).0.lock().open_urls.take() };
    if let Some(mut callback) = callback {
        callback(urls);
        unsafe { (*platform).0.lock().open_urls = Some(callback) };
    }
}

extern "C" fn scene_did_disconnect(_: &Object, _: Sel, scene: id) {
    crate::window::scene_disconnected(scene);
}

extern "C" fn scene_did_become_active(_this: &Object, _: Sel, scene: id) {
    crate::window::scene_active_changed(scene, true);
}

extern "C" fn scene_will_resign_active(_: &Object, _: Sel, scene: id) {
    crate::window::scene_active_changed(scene, false);
}

extern "C" fn scene_did_enter_background(_: &Object, _: Sel, _: id) {
    end_background_task();
    unsafe {
        let application: id = msg_send![class!(UIApplication), sharedApplication];
        let expired = block2::RcBlock::new(end_background_task);
        let task: usize = msg_send![
            application,
            beginBackgroundTaskWithName: ns_string("zz connection")
            expirationHandler: &*expired as *const block2::Block<dyn Fn()> as id
        ];
        BACKGROUND_TASK.store(task, Ordering::Release);
    }
}

extern "C" fn scene_will_enter_foreground(_: &Object, _: Sel, _: id) {
    end_background_task();
}

fn end_background_task() {
    let task = BACKGROUND_TASK.swap(0, Ordering::AcqRel);
    if task != 0 {
        unsafe {
            let application: id = msg_send![class!(UIApplication), sharedApplication];
            let _: () = msg_send![application, endBackgroundTask: task];
        }
    }
}

fn schedule_idle_timer_sync() {
    unsafe {
        dispatch2::DispatchQueue::main().exec_async_f(std::ptr::null_mut(), sync_idle_timer);
    }
}

extern "C" fn sync_idle_timer(_: *mut std::ffi::c_void) {
    unsafe {
        let application: id = msg_send![class!(UIApplication), sharedApplication];
        let disabled = IDLE_GUARDS.load(Ordering::Acquire) > 0;
        let _: () = msg_send![application, setIdleTimerDisabled: disabled as BOOL];
    }
}

extern "C" fn scene_did_update_geometry(
    _this: &Object,
    _: Sel,
    scene: id,
    _previous_coordinate_space: id,
    _previous_orientation: i64,
    _previous_traits: id,
) {
    crate::window::scene_geometry_changed(scene);
}

extern "C" fn scene_did_update_effective_geometry(
    _this: &Object,
    _: Sel,
    scene: id,
    _previous_geometry: id,
) {
    crate::window::scene_geometry_changed(scene);
}

fn reopen() {
    let platform = PLATFORM.load(Ordering::Acquire);
    if platform.is_null() {
        return;
    }
    let callback = unsafe { (*platform).0.lock().reopen.take() };
    if let Some(mut callback) = callback {
        callback();
        unsafe { (*platform).0.lock().reopen = Some(callback) };
    }
}

pub(crate) fn screen_appearance() -> WindowAppearance {
    unsafe {
        let screen: id = msg_send![class!(UIScreen), mainScreen];
        if screen.is_null() {
            return WindowAppearance::Dark;
        }
        let traits: id = msg_send![screen, traitCollection];
        let style: i64 = msg_send![traits, userInterfaceStyle];
        if style == 1 {
            WindowAppearance::Light
        } else {
            WindowAppearance::Dark
        }
    }
}
