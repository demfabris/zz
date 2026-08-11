use crate::{IosDisplay, IosWindow, nil, ns_string, renderer};
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
    ffi::{c_char, c_int},
    path::{Path, PathBuf},
    rc::Rc,
    sync::{Arc, Once, atomic::AtomicPtr, atomic::Ordering},
};

use crate::id;

// `UIApplicationMain` never returns, so the delegate reaches the platform here.
static PLATFORM: AtomicPtr<IosPlatform> = AtomicPtr::new(std::ptr::null_mut());
static REGISTER_DELEGATE: Once = Once::new();

/// The `UIWindowScene` this process is attached to, null on the legacy path.
/// Not retained: UIKit owns the scene, and this is read while it is connected.
static WINDOW_SCENE: AtomicPtr<Object> = AtomicPtr::new(std::ptr::null_mut());

/// The scene the window belongs to, or `nil` under the legacy launch path.
pub(crate) fn current_window_scene() -> id {
    WINDOW_SCENE.load(Ordering::Acquire)
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
    renderer_context: renderer::Context,
    finish_launching: Option<Box<dyn FnOnce()>>,
    reopen: Option<Box<dyn FnMut()>>,
    activated_once: bool,
}

impl Default for IosPlatform {
    fn default() -> Self {
        Self::new()
    }
}

impl IosPlatform {
    pub fn new() -> Self {
        let dispatcher = Arc::new(crate::MacDispatcher::new());

        #[cfg(feature = "font-kit")]
        let text_system: Arc<dyn PlatformTextSystem> = Arc::new(crate::MacTextSystem::new());
        #[cfg(not(feature = "font-kit"))]
        let text_system: Arc<dyn PlatformTextSystem> = Arc::new(gpui::NoopTextSystem::new());

        Self(Mutex::new(IosPlatformState {
            background_executor: BackgroundExecutor::new(dispatcher.clone()),
            foreground_executor: ForegroundExecutor::new(dispatcher),
            text_system,
            renderer_context: renderer::Context::default(),
            finish_launching: None,
            reopen: None,
            activated_once: false,
        }))
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

    fn restart(&self, _binary_path: Option<PathBuf>) {}

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
        None
    }

    fn open_window(
        &self,
        handle: AnyWindowHandle,
        params: WindowParams,
    ) -> Result<Box<dyn PlatformWindow>> {
        let renderer_context = self.0.lock().renderer_context.clone();
        Ok(Box::new(IosWindow::open(handle, params, renderer_context)))
    }

    fn window_appearance(&self) -> WindowAppearance {
        screen_appearance()
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

    fn on_open_urls(&self, _callback: Box<dyn FnMut(Vec<String>)>) {}

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

    fn on_quit(&self, _callback: Box<dyn FnMut()>) {}

    fn on_reopen(&self, callback: Box<dyn FnMut()>) {
        self.0.lock().reopen = Some(callback);
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

    fn should_auto_hide_scrollbars(&self) -> bool {
        true
    }

    fn read_from_clipboard(&self) -> Option<ClipboardItem> {
        unsafe {
            let pasteboard: id = msg_send![class!(UIPasteboard), generalPasteboard];
            if pasteboard.is_null() {
                return None;
            }
            let string: id = msg_send![pasteboard, string];
            crate::nsstring_to_string(string).map(ClipboardItem::new_string)
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
    let mut decl = ClassDecl::new("ZZGPUIAppDelegate", class!(NSObject)).unwrap();
    unsafe {
        decl.add_method(
            sel!(application:didFinishLaunchingWithOptions:),
            did_finish_launching as extern "C" fn(&Object, Sel, id, id) -> BOOL,
        );
        decl.add_method(
            sel!(applicationDidBecomeActive:),
            did_become_active as extern "C" fn(&Object, Sel, id),
        );
    }
    decl.register();
    register_scene_delegate_class();
}

/// The scene delegate named by `UIApplicationSceneManifest`, which UIKit
/// resolves by name when the window scene connects.
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
            sel!(sceneDidBecomeActive:),
            scene_did_become_active as extern "C" fn(&Object, Sel, id),
        );
        // Both spellings of "the scene changed shape": iOS 16 through 25 call
        // the four-part one, iOS 26 the geometry one. The handler is idempotent.
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

/// Whether the bundle opts into scenes. Without the key launch stays on the
/// legacy path, so a bundle that lost its manifest still boots.
fn scene_manifest_present() -> bool {
    unsafe {
        let bundle: id = msg_send![class!(NSBundle), mainBundle];
        let key = ns_string("UIApplicationSceneManifest");
        let manifest: id = msg_send![bundle, objectForInfoDictionaryKey: key];
        !manifest.is_null()
    }
}

/// Runs gpui's launch closure, which opens the window. The closure is taken,
/// so whichever launch path arrives first is the only one that runs it.
fn finish_launching() {
    let platform = PLATFORM.load(Ordering::Acquire);
    assert!(!platform.is_null(), "IosPlatform not registered before run");
    let callback = unsafe { (*platform).0.lock().finish_launching.take() };
    if let Some(callback) = callback {
        callback();
    }
}

extern "C" fn did_finish_launching(_this: &Object, _: Sel, _app: id, _opts: id) -> BOOL {
    // With a manifest, `scene_will_connect` is where the app is built instead.
    if !scene_manifest_present() {
        finish_launching();
    }
    YES
}

/// The scene-based launch: the first moment there is a scene for the window to
/// belong to, one callback after `didFinishLaunchingWithOptions:`.
extern "C" fn scene_will_connect(_this: &Object, _: Sel, scene: id, _session: id, _options: id) {
    unsafe {
        let is_window_scene: BOOL = msg_send![scene, isKindOfClass: class!(UIWindowScene)];
        if is_window_scene == YES {
            WINDOW_SCENE.store(scene, Ordering::Release);
        }
    }
    finish_launching();
}

extern "C" fn scene_did_become_active(_this: &Object, _: Sel, _scene: id) {
    became_active();
}

extern "C" fn scene_did_update_geometry(
    _this: &Object,
    _: Sel,
    _scene: id,
    _previous_coordinate_space: id,
    _previous_orientation: i64,
    _previous_traits: id,
) {
    crate::window::scene_geometry_changed();
}

/// iOS 26's replacement for the callback above, same meaning.
extern "C" fn scene_did_update_effective_geometry(
    _this: &Object,
    _: Sel,
    _scene: id,
    _previous_geometry: id,
) {
    crate::window::scene_geometry_changed();
}

extern "C" fn did_become_active(_this: &Object, _: Sel, _app: id) {
    became_active();
}

/// Fires gpui's reopen handler on every return to foreground. The first
/// activation is launch itself, which the handler must not see.
fn became_active() {
    let platform = PLATFORM.load(Ordering::Acquire);
    if platform.is_null() {
        return;
    }
    let callback = {
        let mut state = unsafe { (*platform).0.lock() };
        if !state.activated_once {
            state.activated_once = true;
            return;
        }
        state.reopen.take()
    };
    if let Some(mut callback) = callback {
        callback();
        unsafe { (*platform).0.lock().reopen = Some(callback) };
    }
}

/// `UIUserInterfaceStyle`: 1 = light, 2 = dark, 0 = unspecified.
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
