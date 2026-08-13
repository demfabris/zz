use crate::{
    CGPoint, CGRect, CGSize, IosDisplay, UIEdgeInsets, id, nil, ns_string, nsstring_to_string,
    renderer,
};
use block::ConcreteBlock;
use futures::channel::oneshot;
use gpui::accesskit;
use gpui::{
    AnyWindowHandle, Bounds, Capslock, CursorStyle, DevicePixels, DispatchEventResult,
    KeyDownEvent, KeyUpEvent, Keystroke, Modifiers, ModifiersChangedEvent, MouseButton,
    MouseDownEvent, MouseExitEvent, MouseMoveEvent, MouseUpEvent, Pixels, PlatformAtlas,
    PlatformDisplay, PlatformInput, PlatformInputHandler, PlatformWindow, Point, PromptButton,
    PromptLevel, RequestFrameOptions, ScrollDelta, ScrollWheelEvent, Size, TouchPhase,
    WindowAppearance, WindowBackgroundAppearance, WindowBounds, WindowParams, point, px, size,
};
use objc::{
    class,
    declare::ClassDecl,
    msg_send,
    runtime::{BOOL, Class, NO, Object, Protocol, Sel, YES},
    sel, sel_impl,
};
use parking_lot::Mutex;
use raw_window_handle as rwh;
use std::{
    collections::HashSet,
    ffi::c_void,
    ops::Range,
    ptr::{self, NonNull},
    rc::Rc,
    sync::{
        Arc, Once,
        atomic::{AtomicBool, AtomicPtr, AtomicU32, Ordering},
    },
};

const STATE_IVAR: &str = "zzWindowState";
static REGISTER_VIEW: Once = Once::new();
static mut VIEW_CLASS: *const Class = ptr::null();

/// Touch travel that still counts as a tap; matches UIKit's own pan threshold.
const TAP_SLOP: f32 = 10.0;

/// Size of the hidden scroll view's canvas, in screen-fulls per axis.
const SCROLL_CANVAS_SCREENS: f64 = 9.0;

/// `UIScrollViewContentInsetAdjustmentNever`.
const CONTENT_INSET_ADJUSTMENT_NEVER: i64 = 2;

/// `UIScrollTypeMaskDiscrete | UIScrollTypeMaskContinuous`.
const SCROLL_TYPE_MASK_ALL: i64 = 0b11;

// UIGestureRecognizerState.
const UI_GESTURE_STATE_BEGAN: i64 = 1;
const UI_GESTURE_STATE_CHANGED: i64 = 2;
const UI_GESTURE_STATE_ENDED: i64 = 3;

/// Seconds a finger must rest before a touch becomes a text selection.
const LONG_PRESS_DURATION: f64 = 0.45;

/// A long press opens as a double click, so lifting without a drag still selects a word.
const SELECTION_CLICK_COUNT: usize = 2;

// `UIAxis`, an option set: neither / horizontal / vertical.
const UI_AXIS_NEITHER: i64 = 0;
const UI_AXIS_VERTICAL: i64 = 1 << 1;

/// Length of the I-beam pointer shape, in points, near a default terminal line height.
const POINTER_BEAM_LENGTH: f64 = 20.0;

/// `UIDropOperationCopy`.
const UI_DROP_OPERATION_COPY: i64 = 2;

/// `UITouchTypeIndirectPointer`: a trackpad or mouse click, delivered as a touch.
const UI_TOUCH_TYPE_INDIRECT_POINTER: i64 = 3;
/// `UIEventButtonMaskSecondary`. `UIEvent.h` defines the mask as
/// `1 << (buttonNumber - 1)` over one-based buttons, so button 3 is the middle one.
const UI_EVENT_BUTTON_MASK_SECONDARY: i64 = 1 << 1;
const UI_EVENT_BUTTON_MASK_MIDDLE: i64 = 1 << 2;

// UIKeyModifierFlags. iOS exposes no flag for the fn/globe key.
const UI_KEY_MODIFIER_ALPHA_SHIFT: i64 = 1 << 16;
const UI_KEY_MODIFIER_SHIFT: i64 = 1 << 17;
const UI_KEY_MODIFIER_CONTROL: i64 = 1 << 18;
const UI_KEY_MODIFIER_ALTERNATE: i64 = 1 << 19;
const UI_KEY_MODIFIER_COMMAND: i64 = 1 << 20;

// UITextInputTraits enums (UITextInputTraits.h).
const UI_TEXT_AUTOCAPITALIZATION_NONE: i64 = 0;
const UI_TEXT_TRAIT_NO: i64 = 1;
const UI_KEYBOARD_APPEARANCE_DARK: i64 = 1;
const UI_KEYBOARD_APPEARANCE_LIGHT: i64 = 2;

// `UITextStorageDirection` and `UITextLayoutDirection` share one NSInteger
// space: storage forward/backward are 0/1, layout right/left/up/down are 2..=5.
const UI_TEXT_STORAGE_DIRECTION_FORWARD: i64 = 0;
const UI_TEXT_LAYOUT_DIRECTION_RIGHT: i64 = 2;
const UI_TEXT_LAYOUT_DIRECTION_DOWN: i64 = 5;
/// `NSWritingDirectionLeftToRight`.
const NS_WRITING_DIRECTION_LEFT_TO_RIGHT: i64 = 0;
/// `NSNotFound`, how UIKit spells "no range" in an `NSRange`.
const NS_NOT_FOUND: usize = i64::MAX as usize;

// UIKeyboardHIDUsage values (HID Usage Tables, keyboard/keypad page 0x07).
const HID_A: u16 = 4;
const HID_Z: u16 = 29;
const HID_1: u16 = 30;
const HID_9: u16 = 38;
const HID_0: u16 = 39;
const HID_RETURN: u16 = 40;
const HID_ESCAPE: u16 = 41;
const HID_BACKSPACE: u16 = 42;
const HID_TAB: u16 = 43;
const HID_SPACEBAR: u16 = 44;
const HID_CAPS_LOCK: u16 = 57;
const HID_F1: u16 = 58;
const HID_F12: u16 = 69;
const HID_INSERT: u16 = 73;
const HID_HOME: u16 = 74;
const HID_PAGE_UP: u16 = 75;
const HID_DELETE_FORWARD: u16 = 76;
const HID_END: u16 = 77;
const HID_PAGE_DOWN: u16 = 78;
const HID_RIGHT: u16 = 79;
const HID_LEFT: u16 = 80;
const HID_DOWN: u16 = 81;
const HID_UP: u16 = 82;
const HID_KEYPAD_ENTER: u16 = 88;
const HID_KEYPAD_1: u16 = 89;
const HID_KEYPAD_9: u16 = 97;
const HID_KEYPAD_0: u16 = 98;
const HID_F13: u16 = 104;
const HID_F24: u16 = 115;
const HID_LEFT_CONTROL: u16 = 224;
const HID_LEFT_SHIFT: u16 = 225;
const HID_LEFT_ALT: u16 = 226;
const HID_LEFT_GUI: u16 = 227;
const HID_RIGHT_CONTROL: u16 = 228;
const HID_RIGHT_SHIFT: u16 = 229;
const HID_RIGHT_ALT: u16 = 230;
const HID_RIGHT_GUI: u16 = 231;

#[link(name = "Foundation", kind = "framework")]
unsafe extern "C" {
    /// Foundation's own common-modes sentinel; pointer identity matters, see `IosWindow::open`.
    static NSRunLoopCommonModes: id;
}

#[link(name = "UIKit", kind = "framework")]
unsafe extern "C" {
    static UIKeyboardWillChangeFrameNotification: id;
    static UIKeyboardWillHideNotification: id;
    /// `NSValue` of the keyboard's post-animation frame, in screen space.
    static UIKeyboardFrameEndUserInfoKey: id;
}

pub(crate) struct IosWindowState {
    /// Unread; kept for parity with `MacWindowState`.
    #[allow(dead_code)]
    handle: AnyWindowHandle,
    #[allow(dead_code)]
    native_window: id,
    native_view: id,
    display_link: id,
    scroll_view: id,
    renderer: renderer::MetalRenderer,
    request_frame_callback: Option<Box<dyn FnMut(RequestFrameOptions)>>,
    event_callback: Option<Box<dyn FnMut(PlatformInput) -> DispatchEventResult>>,
    resize_callback: Option<Box<dyn FnMut(Size<Pixels>, f32)>>,
    close_callback: Option<Box<dyn FnOnce()>>,
    input_handler: Option<PlatformInputHandler>,
    last_touch: Point<Pixels>,
    tap_origin: Option<Point<Pixels>>,
    tap_button: MouseButton,
    tap_click_count: usize,
    modifiers: Modifiers,
    capslock: Capslock,
    pressed_keys: HashSet<u16>,
    scroll_offset: CGPoint,
    scroll_began: bool,
    selecting: bool,
    appearance_changed: Option<Box<dyn FnMut()>>,
    accesskit_adapter: Option<accesskit_ios::SubclassingAdapter>,
}

pub(crate) struct IosWindow(Arc<Mutex<IosWindowState>>);

impl IosWindow {
    pub(crate) fn open(
        handle: AnyWindowHandle,
        _params: WindowParams,
        renderer_context: renderer::Context,
    ) -> Self {
        REGISTER_VIEW.call_once(register_view_class);

        unsafe {
            let screen: id = msg_send![class!(UIScreen), mainScreen];
            let screen_bounds: CGRect = msg_send![screen, bounds];
            let scale = IosDisplay::scale_factor() as f64;

            let scene = crate::platform::current_window_scene();
            let native_window: id = msg_send![class!(UIWindow), alloc];
            let native_window: id = if scene.is_null() {
                msg_send![native_window, initWithFrame: screen_bounds]
            } else {
                msg_send![native_window, initWithWindowScene: scene]
            };
            let mut window_bounds: CGRect = msg_send![native_window, bounds];
            if window_bounds.size.width <= 0.0 || window_bounds.size.height <= 0.0 {
                let _: () = msg_send![native_window, setFrame: screen_bounds];
                window_bounds = screen_bounds;
            }

            let controller: id = msg_send![CONTROLLER_CLASS, new];
            let native_view: id = msg_send![VIEW_CLASS, alloc];
            let native_view: id = msg_send![native_view, initWithFrame: window_bounds];
            let _: () = msg_send![controller, setView: native_view];
            let _: () = msg_send![native_window, setRootViewController: controller];

            let renderer = renderer::new_renderer(
                renderer_context,
                native_window as *mut c_void,
                native_view as *mut c_void,
                gpui::size(
                    window_bounds.size.width as f32,
                    window_bounds.size.height as f32,
                ),
                false,
            );

            let layer = renderer.layer_ptr() as id;
            let _: () = msg_send![layer, setContentsScale: scale];
            let _: () = msg_send![layer, setFrame: window_bounds];
            let view_layer: id = msg_send![native_view, layer];
            let _: () = msg_send![view_layer, addSublayer: layer];

            let state = Arc::new(Mutex::new(IosWindowState {
                handle,
                native_window,
                native_view,
                display_link: nil,
                scroll_view: nil,
                renderer,
                request_frame_callback: None,
                event_callback: None,
                resize_callback: None,
                close_callback: None,
                input_handler: None,
                last_touch: Point::default(),
                tap_origin: None,
                tap_button: MouseButton::Left,
                tap_click_count: 1,
                modifiers: Modifiers::default(),
                capslock: Capslock::default(),
                pressed_keys: HashSet::new(),
                scroll_offset: CGPoint::default(),
                scroll_began: false,
                selecting: false,
                appearance_changed: None,
                accesskit_adapter: None,
            }));

            state.lock().renderer.update_drawable_size(size(
                DevicePixels((window_bounds.size.width * scale) as i32),
                DevicePixels((window_bounds.size.height * scale) as i32),
            ));

            let state_ptr = Arc::into_raw(state.clone()) as *mut c_void;
            (*native_view).set_ivar(STATE_IVAR, state_ptr);
            MAIN_VIEW.store(native_view, Ordering::Release);

            state.lock().scroll_offset = scroll_center(window_bounds);
            let scroll_view = if std::env::var_os("ZZ_IOS_NO_SCROLL").is_some() {
                nil
            } else {
                make_scroll_view(native_view, window_bounds)
            };
            state.lock().scroll_view = scroll_view;
            attach_pointer(native_view);
            attach_selection(native_view);
            attach_drop(native_view);
            attach_pinch(native_view);

            let _: () = msg_send![native_window, makeKeyAndVisible];
            let _: () = msg_send![native_view, becomeFirstResponder];
            set_safe_area_insets(msg_send![native_view, safeAreaInsets]);

            observe_keyboard(native_view);
            crate::system_traits::refresh();

            let display_link: id = msg_send![
                class!(CADisplayLink),
                displayLinkWithTarget: native_view
                selector: sel!(zzDisplayStep:)
            ];
            let display_link: id = msg_send![display_link, retain];
            let run_loop: id = msg_send![class!(NSRunLoop), mainRunLoop];
            // Must be Foundation's own constant, not an equal string built at
            // runtime: the first `addToRunLoop:forMode:` parks CoreAnimation's
            // process-wide source in that exact mode object, and a mode that is
            // only `isEqualToString:` silences every CADisplayLink in the app.
            let _: () = msg_send![
                display_link,
                addToRunLoop: run_loop
                forMode: NSRunLoopCommonModes
            ];
            state.lock().display_link = display_link;

            Self(state)
        }
    }

    fn bounds_impl(&self) -> Bounds<Pixels> {
        let view = self.0.lock().native_view;
        let rect: CGRect = unsafe { msg_send![view, bounds] };
        Bounds {
            origin: point(px(rect.origin.x as f32), px(rect.origin.y as f32)),
            size: size(px(rect.size.width as f32), px(rect.size.height as f32)),
        }
    }
}

impl PlatformWindow for IosWindow {
    fn bounds(&self) -> Bounds<Pixels> {
        self.bounds_impl()
    }

    fn is_maximized(&self) -> bool {
        false
    }

    fn window_bounds(&self) -> WindowBounds {
        WindowBounds::Fullscreen(self.bounds_impl())
    }

    fn content_size(&self) -> Size<Pixels> {
        self.bounds_impl().size
    }

    fn resize(&mut self, _size: Size<Pixels>) {}

    fn scale_factor(&self) -> f32 {
        IosDisplay::scale_factor()
    }

    fn appearance(&self) -> WindowAppearance {
        crate::platform::screen_appearance()
    }

    fn display(&self) -> Option<Rc<dyn PlatformDisplay>> {
        Some(Rc::new(IosDisplay))
    }

    fn mouse_position(&self) -> Point<Pixels> {
        self.0.lock().last_touch
    }

    fn modifiers(&self) -> Modifiers {
        self.0.lock().modifiers
    }

    fn capslock(&self) -> Capslock {
        self.0.lock().capslock
    }

    fn set_input_handler(&mut self, input_handler: PlatformInputHandler) {
        self.0.lock().input_handler = Some(input_handler);
    }

    fn take_input_handler(&mut self) -> Option<PlatformInputHandler> {
        self.0.lock().input_handler.take()
    }

    fn prompt(
        &self,
        _level: PromptLevel,
        _msg: &str,
        _detail: Option<&str>,
        _answers: &[PromptButton],
    ) -> Option<oneshot::Receiver<usize>> {
        None
    }

    fn activate(&self) {}

    fn is_active(&self) -> bool {
        true
    }

    fn is_hovered(&self) -> bool {
        false
    }

    fn background_appearance(&self) -> WindowBackgroundAppearance {
        WindowBackgroundAppearance::Opaque
    }

    fn set_title(&mut self, _title: &str) {}

    fn set_background_appearance(&self, _background_appearance: WindowBackgroundAppearance) {}

    fn minimize(&self) {}

    fn zoom(&self) {}

    fn toggle_fullscreen(&self) {}

    fn is_fullscreen(&self) -> bool {
        true
    }

    fn on_request_frame(&self, callback: Box<dyn FnMut(RequestFrameOptions)>) {
        self.0.lock().request_frame_callback = Some(callback);
    }

    fn on_input(&self, callback: Box<dyn FnMut(PlatformInput) -> DispatchEventResult>) {
        self.0.lock().event_callback = Some(callback);
    }

    fn on_active_status_change(&self, _callback: Box<dyn FnMut(bool)>) {}

    fn on_hover_status_change(&self, _callback: Box<dyn FnMut(bool)>) {}

    fn on_resize(&self, callback: Box<dyn FnMut(Size<Pixels>, f32)>) {
        self.0.lock().resize_callback = Some(callback);
    }

    fn on_moved(&self, _callback: Box<dyn FnMut()>) {}

    fn on_should_close(&self, _callback: Box<dyn FnMut() -> bool>) {}

    fn on_hit_test_window_control(
        &self,
        _callback: Box<dyn FnMut() -> Option<gpui::WindowControlArea>>,
    ) {
    }

    fn on_close(&self, callback: Box<dyn FnOnce()>) {
        self.0.lock().close_callback = Some(callback);
    }

    fn on_appearance_changed(&self, callback: Box<dyn FnMut()>) {
        self.0.lock().appearance_changed = Some(callback);
    }

    fn draw(&self, scene: &gpui::Scene) {
        self.0.lock().renderer.draw(scene);
    }

    fn sprite_atlas(&self) -> Arc<dyn PlatformAtlas> {
        self.0.lock().renderer.sprite_atlas().clone()
    }

    fn is_subpixel_rendering_supported(&self) -> bool {
        false
    }

    fn update_ime_position(&self, _bounds: Bounds<Pixels>) {}

    fn gpu_specs(&self) -> Option<gpui::GpuSpecs> {
        None
    }

    fn a11y_init(&self, callbacks: gpui::A11yCallbacks) {
        let view = self.0.lock().native_view;
        let adapter = unsafe {
            accesskit_ios::SubclassingAdapter::new(
                view as *mut c_void,
                A11yActivationHandler(callbacks.activation),
                A11yActionHandler(callbacks.action),
                A11yDeactivationHandler(callbacks.deactivation),
            )
        };
        self.0.lock().accesskit_adapter = Some(adapter);
    }

    fn a11y_tree_update(&self, tree_update: accesskit::TreeUpdate) {
        let events = {
            let lock = self.0.lock();
            lock.accesskit_adapter
                .as_ref()
                .and_then(|adapter| adapter.update_if_active(|| tree_update))
        };
        if let Some(events) = events {
            events.raise();
        }
    }

    fn a11y_update_window_bounds(&self) {}
}

struct A11yActivationHandler(Box<dyn Fn() -> Option<accesskit::TreeUpdate> + Send + 'static>);

impl accesskit::ActivationHandler for A11yActivationHandler {
    fn request_initial_tree(&mut self) -> Option<accesskit::TreeUpdate> {
        (self.0)()
    }
}

struct A11yActionHandler(Box<dyn Fn(accesskit::ActionRequest) + Send + 'static>);

impl accesskit::ActionHandler for A11yActionHandler {
    fn do_action(&mut self, request: accesskit::ActionRequest) {
        (self.0)(request);
    }
}

struct A11yDeactivationHandler(Box<dyn Fn() + Send + 'static>);

impl accesskit::DeactivationHandler for A11yDeactivationHandler {
    fn deactivate_accessibility(&mut self) {
        (self.0)();
    }
}

impl rwh::HasWindowHandle for IosWindow {
    fn window_handle(&self) -> Result<rwh::WindowHandle<'_>, rwh::HandleError> {
        unsafe {
            let view = NonNull::new_unchecked(self.0.lock().native_view as *mut c_void);
            Ok(rwh::WindowHandle::borrow_raw(rwh::RawWindowHandle::UiKit(
                rwh::UiKitWindowHandle::new(view),
            )))
        }
    }
}

impl rwh::HasDisplayHandle for IosWindow {
    fn display_handle(&self) -> Result<rwh::DisplayHandle<'_>, rwh::HandleError> {
        Ok(rwh::DisplayHandle::uikit())
    }
}

unsafe fn get_window_state(object: &Object) -> Arc<Mutex<IosWindowState>> {
    let raw: *mut c_void = *object.get_ivar(STATE_IVAR);
    let state = Arc::from_raw(raw as *const Mutex<IosWindowState>);
    let clone = state.clone();
    std::mem::forget(state);
    clone
}

unsafe fn make_scroll_view(native_view: id, bounds: CGRect) -> id {
    let scroll_view: id = msg_send![class!(UIScrollView), alloc];
    let scroll_view: id = msg_send![scroll_view, initWithFrame: bounds];
    let _: () = msg_send![scroll_view, setDelegate: native_view];
    let _: () = msg_send![scroll_view, setShowsVerticalScrollIndicator: NO];
    let _: () = msg_send![scroll_view, setShowsHorizontalScrollIndicator: NO];
    let _: () = msg_send![
        scroll_view,
        setContentInsetAdjustmentBehavior: CONTENT_INSET_ADJUSTMENT_NEVER
    ];
    let _: () = msg_send![scroll_view, setAlwaysBounceVertical: NO];
    let _: () = msg_send![scroll_view, setAlwaysBounceHorizontal: NO];
    let _: () = msg_send![scroll_view, setDirectionalLockEnabled: YES];
    let _: () = msg_send![scroll_view, setDelaysContentTouches: NO];
    let _: () = msg_send![scroll_view, setUserInteractionEnabled: NO];
    let _: () = msg_send![scroll_view, setOpaque: NO];
    let _: () = msg_send![scroll_view, setBackgroundColor: nil];
    let _: () = msg_send![native_view, insertSubview: scroll_view atIndex: 0usize];

    let pan: id = msg_send![scroll_view, panGestureRecognizer];
    let _: () = msg_send![pan, setAllowedScrollTypesMask: SCROLL_TYPE_MASK_ALL];
    let _: () = msg_send![native_view, addGestureRecognizer: pan];

    resize_scroll_view(scroll_view, bounds);
    scroll_view
}

unsafe fn resize_scroll_view(scroll_view: id, bounds: CGRect) {
    let _: () = msg_send![scroll_view, setFrame: bounds];
    let content = CGSize {
        width: bounds.size.width * SCROLL_CANVAS_SCREENS,
        height: bounds.size.height * SCROLL_CANVAS_SCREENS,
    };
    let _: () = msg_send![scroll_view, setContentSize: content];
    let center = scroll_center(bounds);
    let _: () = msg_send![scroll_view, setContentOffset: center animated: NO];
}

fn scroll_center(bounds: CGRect) -> CGPoint {
    CGPoint {
        x: bounds.size.width * (SCROLL_CANVAS_SCREENS - 1.0) / 2.0,
        y: bounds.size.height * (SCROLL_CANVAS_SCREENS - 1.0) / 2.0,
    }
}

fn register_view_class() {
    register_text_classes();
    register_controller_class();
    let mut decl = ClassDecl::new("ZZGPUIView", class!(UIView)).unwrap();
    decl.add_ivar::<*mut c_void>(STATE_IVAR);
    unsafe {
        for name in [
            "UIKeyInput",
            "UITextInput",
            "UIScrollViewDelegate",
            "UIPointerInteractionDelegate",
            "UIDropInteractionDelegate",
        ] {
            if let Some(protocol) = Protocol::get(name) {
                decl.add_protocol(protocol);
            } else {
                log::error!("protocol {name} not found; conformance skipped");
            }
        }
        decl.add_method(
            sel!(zzDisplayStep:),
            display_step as extern "C" fn(&Object, Sel, id),
        );
        decl.add_method(
            sel!(layoutSubviews),
            layout_subviews as extern "C" fn(&Object, Sel),
        );
        decl.add_method(
            sel!(safeAreaInsetsDidChange),
            safe_area_insets_did_change as extern "C" fn(&Object, Sel),
        );
        decl.add_method(
            sel!(touchesBegan:withEvent:),
            touches_began as extern "C" fn(&Object, Sel, id, id),
        );
        decl.add_method(
            sel!(touchesMoved:withEvent:),
            touches_moved as extern "C" fn(&Object, Sel, id, id),
        );
        decl.add_method(
            sel!(touchesEnded:withEvent:),
            touches_ended as extern "C" fn(&Object, Sel, id, id),
        );
        decl.add_method(
            sel!(touchesCancelled:withEvent:),
            touches_cancelled as extern "C" fn(&Object, Sel, id, id),
        );
        decl.add_method(
            sel!(zzPointerHover:),
            pointer_hover as extern "C" fn(&Object, Sel, id),
        );
        decl.add_method(
            sel!(pointerInteraction:styleForRegion:),
            pointer_style_for_region as extern "C" fn(&Object, Sel, id, id) -> id,
        );
        decl.add_method(
            sel!(zzLongPress:),
            long_press as extern "C" fn(&Object, Sel, id),
        );
        decl.add_method(
            sel!(canPerformAction:withSender:),
            can_perform_action as extern "C" fn(&Object, Sel, Sel, id) -> BOOL,
        );
        decl.add_method(sel!(copy:), edit_copy as extern "C" fn(&Object, Sel, id));
        decl.add_method(sel!(paste:), edit_paste as extern "C" fn(&Object, Sel, id));
        decl.add_method(
            sel!(selectAll:),
            edit_select_all as extern "C" fn(&Object, Sel, id),
        );
        decl.add_method(
            sel!(keyCommands),
            key_commands as extern "C" fn(&Object, Sel) -> id,
        );
        decl.add_method(sel!(zzPinch:), pinch as extern "C" fn(&Object, Sel, id));
        decl.add_method(
            sel!(dropInteraction:canHandleSession:),
            drop_can_handle_session as extern "C" fn(&Object, Sel, id, id) -> BOOL,
        );
        decl.add_method(
            sel!(dropInteraction:sessionDidUpdate:),
            drop_session_did_update as extern "C" fn(&Object, Sel, id, id) -> id,
        );
        decl.add_method(
            sel!(dropInteraction:performDrop:),
            drop_perform as extern "C" fn(&Object, Sel, id, id),
        );
        decl.add_method(
            sel!(canBecomeFirstResponder),
            can_become_first_responder as extern "C" fn(&Object, Sel) -> BOOL,
        );
        decl.add_method(
            sel!(traitCollectionDidChange:),
            trait_collection_did_change as extern "C" fn(&Object, Sel, id),
        );
        decl.add_method(
            sel!(pressesBegan:withEvent:),
            presses_began as extern "C" fn(&Object, Sel, id, id),
        );
        decl.add_method(
            sel!(pressesEnded:withEvent:),
            presses_ended as extern "C" fn(&Object, Sel, id, id),
        );
        decl.add_method(
            sel!(pressesCancelled:withEvent:),
            presses_cancelled as extern "C" fn(&Object, Sel, id, id),
        );
        decl.add_method(
            sel!(hasText),
            has_text as extern "C" fn(&Object, Sel) -> BOOL,
        );
        decl.add_method(
            sel!(insertText:),
            insert_text as extern "C" fn(&Object, Sel, id),
        );
        decl.add_method(
            sel!(deleteBackward),
            delete_backward as extern "C" fn(&Object, Sel),
        );
        decl.add_method(
            sel!(autocapitalizationType),
            autocapitalization_type as extern "C" fn(&Object, Sel) -> i64,
        );
        for trait_selector in [
            sel!(autocorrectionType),
            sel!(spellCheckingType),
            sel!(smartQuotesType),
            sel!(smartDashesType),
            sel!(smartInsertDeleteType),
        ] {
            decl.add_method(
                trait_selector,
                text_trait_no as extern "C" fn(&Object, Sel) -> i64,
            );
        }
        decl.add_method(
            sel!(keyboardAppearance),
            keyboard_appearance as extern "C" fn(&Object, Sel) -> i64,
        );
        decl.add_method(
            sel!(textInRange:),
            text_in_range as extern "C" fn(&Object, Sel, id) -> id,
        );
        decl.add_method(
            sel!(replaceRange:withText:),
            replace_range_with_text as extern "C" fn(&Object, Sel, id, id),
        );
        decl.add_method(
            sel!(selectedTextRange),
            selected_text_range as extern "C" fn(&Object, Sel) -> id,
        );
        decl.add_method(
            sel!(setSelectedTextRange:),
            set_selected_text_range as extern "C" fn(&Object, Sel, id),
        );
        decl.add_method(
            sel!(markedTextRange),
            marked_text_range as extern "C" fn(&Object, Sel) -> id,
        );
        decl.add_method(
            sel!(markedTextStyle),
            marked_text_style as extern "C" fn(&Object, Sel) -> id,
        );
        decl.add_method(
            sel!(setMarkedTextStyle:),
            set_marked_text_style as extern "C" fn(&Object, Sel, id),
        );
        decl.add_method(
            sel!(setMarkedText:selectedRange:),
            set_marked_text as extern "C" fn(&Object, Sel, id, NSRange),
        );
        decl.add_method(sel!(unmarkText), unmark_text as extern "C" fn(&Object, Sel));
        decl.add_method(
            sel!(beginningOfDocument),
            beginning_of_document as extern "C" fn(&Object, Sel) -> id,
        );
        decl.add_method(
            sel!(endOfDocument),
            end_of_document as extern "C" fn(&Object, Sel) -> id,
        );
        decl.add_method(
            sel!(textRangeFromPosition:toPosition:),
            text_range_from_position as extern "C" fn(&Object, Sel, id, id) -> id,
        );
        decl.add_method(
            sel!(positionFromPosition:offset:),
            position_from_position as extern "C" fn(&Object, Sel, id, i64) -> id,
        );
        decl.add_method(
            sel!(positionFromPosition:inDirection:offset:),
            position_from_position_in_direction as extern "C" fn(&Object, Sel, id, i64, i64) -> id,
        );
        decl.add_method(
            sel!(comparePosition:toPosition:),
            compare_position as extern "C" fn(&Object, Sel, id, id) -> i64,
        );
        decl.add_method(
            sel!(offsetFromPosition:toPosition:),
            offset_from_position as extern "C" fn(&Object, Sel, id, id) -> i64,
        );
        decl.add_method(
            sel!(inputDelegate),
            input_delegate as extern "C" fn(&Object, Sel) -> id,
        );
        decl.add_method(
            sel!(setInputDelegate:),
            set_input_delegate as extern "C" fn(&Object, Sel, id),
        );
        decl.add_method(
            sel!(tokenizer),
            tokenizer as extern "C" fn(&Object, Sel) -> id,
        );
        decl.add_method(
            sel!(positionWithinRange:farthestInDirection:),
            position_within_range as extern "C" fn(&Object, Sel, id, i64) -> id,
        );
        decl.add_method(
            sel!(characterRangeByExtendingPosition:inDirection:),
            character_range_by_extending as extern "C" fn(&Object, Sel, id, i64) -> id,
        );
        decl.add_method(
            sel!(baseWritingDirectionForPosition:inDirection:),
            base_writing_direction as extern "C" fn(&Object, Sel, id, i64) -> i64,
        );
        decl.add_method(
            sel!(setBaseWritingDirection:forRange:),
            set_base_writing_direction as extern "C" fn(&Object, Sel, i64, id),
        );
        decl.add_method(
            sel!(firstRectForRange:),
            first_rect_for_range as extern "C" fn(&Object, Sel, id) -> CGRect,
        );
        decl.add_method(
            sel!(caretRectForPosition:),
            caret_rect_for_position as extern "C" fn(&Object, Sel, id) -> CGRect,
        );
        decl.add_method(
            sel!(selectionRectsForRange:),
            selection_rects_for_range as extern "C" fn(&Object, Sel, id) -> id,
        );
        decl.add_method(
            sel!(closestPositionToPoint:),
            closest_position_to_point as extern "C" fn(&Object, Sel, CGPoint) -> id,
        );
        decl.add_method(
            sel!(closestPositionToPoint:withinRange:),
            closest_position_within_range as extern "C" fn(&Object, Sel, CGPoint, id) -> id,
        );
        decl.add_method(
            sel!(characterRangeAtPoint:),
            character_range_at_point as extern "C" fn(&Object, Sel, CGPoint) -> id,
        );
        decl.add_method(
            sel!(beginFloatingCursorAtPoint:),
            begin_floating_cursor as extern "C" fn(&Object, Sel, CGPoint),
        );
        decl.add_method(
            sel!(updateFloatingCursorAtPoint:),
            update_floating_cursor as extern "C" fn(&Object, Sel, CGPoint),
        );
        decl.add_method(
            sel!(endFloatingCursor),
            end_floating_cursor as extern "C" fn(&Object, Sel),
        );
        decl.add_method(
            sel!(zzKeyboardFrameChanged:),
            keyboard_frame_changed as extern "C" fn(&Object, Sel, id),
        );
        decl.add_method(
            sel!(scrollViewDidScroll:),
            scroll_view_did_scroll as extern "C" fn(&Object, Sel, id),
        );
        decl.add_method(
            sel!(scrollViewWillBeginDragging:),
            scroll_view_will_begin_dragging as extern "C" fn(&Object, Sel, id),
        );
        decl.add_method(
            sel!(scrollViewDidEndDragging:willDecelerate:),
            scroll_view_did_end_dragging as extern "C" fn(&Object, Sel, id, BOOL),
        );
        decl.add_method(
            sel!(scrollViewDidEndDecelerating:),
            scroll_view_did_end_decelerating as extern "C" fn(&Object, Sel, id),
        );
        VIEW_CLASS = decl.register();
    }
}

static mut CONTROLLER_CLASS: *const Class = ptr::null();

fn register_controller_class() {
    let mut decl = ClassDecl::new("ZZViewController", class!(UIViewController)).unwrap();
    unsafe {
        decl.add_method(
            sel!(prefersHomeIndicatorAutoHidden),
            prefers_home_indicator_auto_hidden as extern "C" fn(&Object, Sel) -> BOOL,
        );
        CONTROLLER_CLASS = decl.register();
    }
}

extern "C" fn prefers_home_indicator_auto_hidden(_this: &Object, _: Sel) -> BOOL {
    YES
}

fn pump_frame(this: &Object) {
    insert_dropped_text(this);
    let state = unsafe { get_window_state(this) };
    let mut lock = state.lock();
    if let Some(mut callback) = lock.request_frame_callback.take() {
        drop(lock);
        callback(RequestFrameOptions {
            require_presentation: false,
            // `|` rather than `||` so every flag is cleared, whichever one is set.
            force_render: KEYBOARD_INSET_CHANGED.swap(false, Ordering::Relaxed)
                | SAFE_AREA_INSETS_CHANGED.swap(false, Ordering::Relaxed)
                | PINCH_CHANGED.swap(false, Ordering::Relaxed)
                | crate::system_traits::take_changed(),
        });
        state.lock().request_frame_callback = Some(callback);
    }
}

extern "C" fn display_step(this: &Object, _: Sel, _link: id) {
    pump_frame(this);
}

static MAIN_VIEW: AtomicPtr<Object> = AtomicPtr::new(ptr::null_mut());

/// Asks the drawing view for a layout pass after the scene changed shape.
pub(crate) fn scene_geometry_changed() {
    let view = MAIN_VIEW.load(Ordering::Acquire);
    if view.is_null() {
        return;
    }
    unsafe {
        let _: () = msg_send![view, setNeedsLayout];
    }
}

extern "C" fn layout_subviews(this: &Object, _: Sel) {
    unsafe {
        let _: () = msg_send![super(this, class!(UIView)), layoutSubviews];
    }
    let state = unsafe { get_window_state(this) };
    let mut lock = state.lock();
    unsafe {
        let bounds: CGRect = msg_send![lock.native_view, bounds];
        let scale = IosDisplay::scale_factor() as f64;
        let layer = lock.renderer.layer_ptr() as id;
        let _: () = msg_send![layer, setFrame: bounds];
        lock.renderer.update_drawable_size(size(
            DevicePixels((bounds.size.width * scale) as i32),
            DevicePixels((bounds.size.height * scale) as i32),
        ));
        if lock.scroll_view != nil {
            let scroll_view = lock.scroll_view;
            // Recentring re-enters `scrollViewDidScroll:`: publish the expected
            // offset first, and hold no lock across the call.
            lock.scroll_offset = scroll_center(bounds);
            drop(lock);
            resize_scroll_view(scroll_view, bounds);
            lock = state.lock();
        }
        let logical = size(px(bounds.size.width as f32), px(bounds.size.height as f32));
        if let Some(mut callback) = lock.resize_callback.take() {
            drop(lock);
            callback(logical, scale as f32);
            state.lock().resize_callback = Some(callback);
        }
    }
}

extern "C" fn safe_area_insets_did_change(this: &Object, _: Sel) {
    unsafe {
        let _: () = msg_send![super(this, class!(UIView)), safeAreaInsetsDidChange];
        set_safe_area_insets(msg_send![this, safeAreaInsets]);
    }
}

fn touch_position(this: &Object, touches: id) -> Point<Pixels> {
    unsafe {
        let touch: id = msg_send![touches, anyObject];
        let location: CGPoint = msg_send![touch, locationInView: this as *const Object as id];
        point(px(location.x as f32), px(location.y as f32))
    }
}

unsafe fn touch_click(touches: id, event: id) -> (MouseButton, usize) {
    if event.is_null() {
        return (MouseButton::Left, 1);
    }
    let touch: id = msg_send![touches, anyObject];
    if touch.is_null() {
        return (MouseButton::Left, 1);
    }
    let touch_type: i64 = msg_send![touch, type];
    if touch_type != UI_TOUCH_TYPE_INDIRECT_POINTER {
        return (MouseButton::Left, 1);
    }
    let mask: i64 = msg_send![event, buttonMask];
    let button = if mask & UI_EVENT_BUTTON_MASK_SECONDARY != 0 {
        MouseButton::Right
    } else if mask & UI_EVENT_BUTTON_MASK_MIDDLE != 0 {
        MouseButton::Middle
    } else {
        MouseButton::Left
    };
    let taps: usize = msg_send![touch, tapCount];
    (button, taps.max(1))
}

fn dispatch_event(this: &Object, event: PlatformInput) -> DispatchEventResult {
    let state = unsafe { get_window_state(this) };
    let mut lock = state.lock();
    let Some(mut callback) = lock.event_callback.take() else {
        return DispatchEventResult::default();
    };
    drop(lock);
    let result = callback(event);
    state.lock().event_callback = Some(callback);
    result
}

/// Runs `f` against the focused element's input handler. Taking the handler for
/// the call is the re-entrancy guard: a nested UIKit query finds nothing.
fn with_input_handler<R>(
    this: &Object,
    f: impl FnOnce(&mut PlatformInputHandler) -> R,
) -> Option<R> {
    let state = unsafe { get_window_state(this) };
    let mut handler = state.lock().input_handler.take()?;
    let result = f(&mut handler);
    state.lock().input_handler = Some(handler);
    Some(result)
}

extern "C" fn touches_began(this: &Object, _: Sel, touches: id, event: id) {
    let position = touch_position(this, touches);
    let (button, click_count) = unsafe { touch_click(touches, event) };
    {
        let state = unsafe { get_window_state(this) };
        let mut lock = state.lock();
        lock.last_touch = position;
        lock.tap_origin = Some(position);
        lock.tap_button = button;
        lock.tap_click_count = click_count;
    }
    unsafe {
        let responder: BOOL = msg_send![this, isFirstResponder];
        if responder == NO {
            let _: () = msg_send![this, becomeFirstResponder];
        }
    }
}

extern "C" fn touches_moved(this: &Object, _: Sel, touches: id, _event: id) {
    let position = touch_position(this, touches);
    let state = unsafe { get_window_state(this) };
    let mut lock = state.lock();
    lock.last_touch = position;
    if let Some(origin) = lock.tap_origin
        && (f32::from(position.x - origin.x).abs() > TAP_SLOP
            || f32::from(position.y - origin.y).abs() > TAP_SLOP)
    {
        lock.tap_origin = None;
    }
}

extern "C" fn touches_ended(this: &Object, _: Sel, touches: id, _event: id) {
    let position = touch_position(this, touches);
    let tapped = {
        let state = unsafe { get_window_state(this) };
        let mut lock = state.lock();
        lock.last_touch = position;
        let click = (lock.tap_button, lock.tap_click_count, lock.modifiers);
        lock.tap_origin
            .take()
            .filter(|origin| {
                f32::from(position.x - origin.x).abs() <= TAP_SLOP
                    && f32::from(position.y - origin.y).abs() <= TAP_SLOP
            })
            .map(|_| click)
    };
    let Some((button, click_count, modifiers)) = tapped else {
        return;
    };
    dispatch_event(
        this,
        PlatformInput::MouseDown(MouseDownEvent {
            button,
            position,
            modifiers,
            click_count,
            first_mouse: false,
        }),
    );
    dispatch_event(
        this,
        PlatformInput::MouseUp(MouseUpEvent {
            button,
            position,
            modifiers,
            click_count,
        }),
    );
}

extern "C" fn touches_cancelled(this: &Object, _: Sel, _touches: id, _event: id) {
    let state = unsafe { get_window_state(this) };
    state.lock().tap_origin = None;
}

extern "C" fn can_become_first_responder(_this: &Object, _: Sel) -> BOOL {
    YES
}

extern "C" fn trait_collection_did_change(this: &Object, _: Sel, previous: id) {
    unsafe {
        let _: () = msg_send![
            super(this, class!(UIView)),
            traitCollectionDidChange: previous
        ];
    }
    crate::system_traits::refresh();
    let state = unsafe { get_window_state(this) };
    let mut lock = state.lock();
    if let Some(mut callback) = lock.appearance_changed.take() {
        drop(lock);
        callback();
        state.lock().appearance_changed = Some(callback);
    }
}

static CURSOR_STYLE: Mutex<CursorStyle> = Mutex::new(CursorStyle::Arrow);
static POINTER_INTERACTION: AtomicPtr<Object> = AtomicPtr::new(ptr::null_mut());

/// Records the style gpui wants and asks UIKit to re-query the pointer shape.
pub(crate) fn set_cursor_style(style: CursorStyle) {
    let changed = {
        let mut current = CURSOR_STYLE.lock();
        let changed = *current != style;
        *current = style;
        changed
    };
    if !changed {
        return;
    }
    let interaction = POINTER_INTERACTION.load(Ordering::Acquire);
    if interaction.is_null() {
        return;
    }
    unsafe {
        let _: () = msg_send![interaction, invalidate];
    }
}

unsafe fn attach_pointer(native_view: id) {
    let hover: id = msg_send![class!(UIHoverGestureRecognizer), alloc];
    let hover: id = msg_send![hover, initWithTarget: native_view action: sel!(zzPointerHover:)];
    let _: () = msg_send![native_view, addGestureRecognizer: hover];

    let interaction: id = msg_send![class!(UIPointerInteraction), alloc];
    let interaction: id = msg_send![interaction, initWithDelegate: native_view];
    let _: () = msg_send![native_view, addInteraction: interaction];
    POINTER_INTERACTION.store(interaction, Ordering::Release);
}

extern "C" fn pointer_hover(this: &Object, _: Sel, recognizer: id) {
    let (gesture_state, position) = unsafe {
        let gesture_state: i64 = msg_send![recognizer, state];
        let location: CGPoint = msg_send![recognizer, locationInView: this as *const Object as id];
        (
            gesture_state,
            point(px(location.x as f32), px(location.y as f32)),
        )
    };
    let modifiers = {
        let state = unsafe { get_window_state(this) };
        let mut lock = state.lock();
        lock.last_touch = position;
        lock.modifiers
    };
    let event =
        if gesture_state == UI_GESTURE_STATE_BEGAN || gesture_state == UI_GESTURE_STATE_CHANGED {
            PlatformInput::MouseMove(MouseMoveEvent {
                position,
                pressed_button: None,
                modifiers,
            })
        } else {
            PlatformInput::MouseExited(MouseExitEvent {
                position,
                pressed_button: None,
                modifiers,
            })
        };
    dispatch_event(this, event);
}

extern "C" fn pointer_style_for_region(
    _this: &Object,
    _: Sel,
    _interaction: id,
    _region: id,
) -> id {
    if *CURSOR_STYLE.lock() != CursorStyle::IBeam {
        return nil;
    }
    unsafe {
        let shape: id = msg_send![
            class!(UIPointerShape),
            beamWithPreferredLength: POINTER_BEAM_LENGTH
            axis: UI_AXIS_VERTICAL
        ];
        let style: id = msg_send![
            class!(UIPointerStyle),
            styleWithShape: shape
            constrainedAxes: UI_AXIS_NEITHER
        ];
        style
    }
}

unsafe fn press_keys(presses: id) -> Vec<id> {
    let array: id = msg_send![presses, allObjects];
    let count: usize = msg_send![array, count];
    (0..count)
        .map(|index| {
            let press: id = msg_send![array, objectAtIndex: index];
            let key: id = msg_send![press, key];
            key
        })
        .filter(|key| !key.is_null())
        .collect()
}

const fn modifier_bit(hid: u16) -> Option<u8> {
    match hid {
        HID_LEFT_CONTROL | HID_RIGHT_CONTROL => Some(1 << 0),
        HID_LEFT_SHIFT | HID_RIGHT_SHIFT => Some(1 << 1),
        HID_LEFT_ALT | HID_RIGHT_ALT => Some(1 << 2),
        HID_LEFT_GUI | HID_RIGHT_GUI => Some(1 << 3),
        _ => None,
    }
}

fn modifiers_from_flags(flags: i64) -> Modifiers {
    Modifiers {
        control: flags & UI_KEY_MODIFIER_CONTROL != 0,
        alt: flags & UI_KEY_MODIFIER_ALTERNATE != 0,
        shift: flags & UI_KEY_MODIFIER_SHIFT != 0,
        platform: flags & UI_KEY_MODIFIER_COMMAND != 0,
        function: false,
    }
}

/// Rebuilt from the keys we know are held: `UIKey.modifierFlags` still reports
/// a modifier as down on its own release.
fn modifiers_from_pressed(pressed: &HashSet<u16>) -> Modifiers {
    let held = |bit: u8| pressed.iter().any(|hid| modifier_bit(*hid) == Some(bit));
    Modifiers {
        control: held(1 << 0),
        shift: held(1 << 1),
        alt: held(1 << 2),
        platform: held(1 << 3),
        function: false,
    }
}

/// Unshifted US-layout name for a HID usage, in gpui's vocabulary.
fn hid_key_name(hid: u16) -> Option<String> {
    let named = match hid {
        HID_RETURN | HID_KEYPAD_ENTER => "enter",
        HID_ESCAPE => "escape",
        HID_BACKSPACE => "backspace",
        HID_TAB => "tab",
        HID_SPACEBAR => "space",
        HID_INSERT => "insert",
        HID_HOME => "home",
        HID_PAGE_UP => "pageup",
        HID_DELETE_FORWARD => "delete",
        HID_END => "end",
        HID_PAGE_DOWN => "pagedown",
        HID_RIGHT => "right",
        HID_LEFT => "left",
        HID_DOWN => "down",
        HID_UP => "up",
        _ => "",
    };
    if !named.is_empty() {
        return Some(named.to_owned());
    }
    if (HID_F1..=HID_F12).contains(&hid) {
        return Some(format!("f{}", hid - HID_F1 + 1));
    }
    if (HID_F13..=HID_F24).contains(&hid) {
        return Some(format!("f{}", hid - HID_F13 + 13));
    }
    hid_character(hid).map(|character| character.to_string())
}

/// The character a HID usage produces unshifted on a US layout.
fn hid_character(hid: u16) -> Option<char> {
    if (HID_A..=HID_Z).contains(&hid) {
        return char::from_u32(u32::from(b'a') + u32::from(hid - HID_A));
    }
    if (HID_1..=HID_9).contains(&hid) {
        return char::from_u32(u32::from(b'1') + u32::from(hid - HID_1));
    }
    if (HID_KEYPAD_1..=HID_KEYPAD_9).contains(&hid) {
        return char::from_u32(u32::from(b'1') + u32::from(hid - HID_KEYPAD_1));
    }
    match hid {
        HID_0 | HID_KEYPAD_0 => Some('0'),
        45 => Some('-'),
        46 => Some('='),
        47 => Some('['),
        48 => Some(']'),
        49 => Some('\\'),
        51 => Some(';'),
        52 => Some('\''),
        53 => Some('`'),
        54 => Some(','),
        55 => Some('.'),
        56 | 84 => Some('/'),
        85 => Some('*'),
        86 => Some('-'),
        87 => Some('+'),
        99 => Some('.'),
        _ => None,
    }
}

const fn us_shifted(character: char) -> Option<char> {
    Some(match character {
        '1' => '!',
        '2' => '@',
        '3' => '#',
        '4' => '$',
        '5' => '%',
        '6' => '^',
        '7' => '&',
        '8' => '*',
        '9' => '(',
        '0' => ')',
        '-' => '_',
        '=' => '+',
        '[' => '{',
        ']' => '}',
        '\\' => '|',
        ';' => ':',
        '\'' => '"',
        '`' => '~',
        ',' => '<',
        '.' => '>',
        '/' => '?',
        _ => return None,
    })
}

/// UIKit reports arrows and friends as private-use codepoints and special keys
/// as names ("UIKeyInputEscape"); neither belongs in `key_char`.
fn printable(text: &str) -> bool {
    !text.is_empty()
        && !text.starts_with("UIKeyInput")
        && !text.chars().any(|character| {
            character.is_control() || ('\u{f700}'..='\u{f8ff}').contains(&character)
        })
}

fn single_char(text: &str) -> Option<char> {
    let mut characters = text.chars();
    let character = characters.next()?;
    characters.next().is_none().then_some(character)
}

/// Builds a gpui keystroke from a `UIKey`, following gpui's macOS convention:
/// letters stay lowercase with `shift` set, everything else takes the shifted
/// glyph with `shift` cleared, so `shift-=` is `+`.
fn keystroke_for(hid: u16, characters: &str, ignoring: &str, flags: i64) -> Keystroke {
    let mut modifiers = modifiers_from_flags(flags);
    let key_char = (!modifiers.control && !modifiers.platform && printable(characters))
        .then(|| characters.to_owned());

    let named = hid_key_name(hid);
    if named.is_some() && hid_character(hid).is_none() {
        return Keystroke {
            modifiers,
            key: named.unwrap_or_default(),
            key_char: None,
        };
    }

    let candidate = single_char(ignoring)
        .filter(|_| printable(ignoring))
        .or_else(|| hid_character(hid));
    let Some(candidate) = candidate else {
        return Keystroke {
            modifiers,
            key: named.unwrap_or_else(|| ignoring.to_lowercase()),
            key_char,
        };
    };

    let key = if !modifiers.shift {
        candidate.to_string()
    } else if candidate.is_alphabetic() {
        candidate.to_lowercase().to_string()
    } else {
        modifiers.shift = false;
        us_shifted(candidate).unwrap_or(candidate).to_string()
    };
    Keystroke {
        modifiers,
        key,
        key_char,
    }
}

unsafe fn read_key(key: id) -> (u16, String, String, i64) {
    let hid: i64 = msg_send![key, keyCode];
    let flags: i64 = msg_send![key, modifierFlags];
    let characters: id = msg_send![key, characters];
    let ignoring: id = msg_send![key, charactersIgnoringModifiers];
    (
        u16::try_from(hid).unwrap_or_default(),
        nsstring_to_string(characters).unwrap_or_default(),
        nsstring_to_string(ignoring).unwrap_or_default(),
        flags,
    )
}

fn update_capslock(this: &Object, flags: i64) {
    let state = unsafe { get_window_state(this) };
    state.lock().capslock = Capslock {
        on: flags & UI_KEY_MODIFIER_ALPHA_SHIFT != 0,
    };
}

// Every press is dispatched as a KeyDown first; only a press nothing consumed
// and that would insert text is handed back to UIKit (by calling super) to
// become an `insertText:`. So a key reaches zz exactly once, either way.
extern "C" fn presses_began(this: &Object, _: Sel, presses: id, event: id) {
    let mut unhandled = false;
    for key in unsafe { press_keys(presses) } {
        let (hid, characters, ignoring, flags) = unsafe { read_key(key) };
        let is_held = {
            let state = unsafe { get_window_state(this) };
            let mut lock = state.lock();
            !lock.pressed_keys.insert(hid)
        };
        if modifier_bit(hid).is_some() || hid == HID_CAPS_LOCK {
            dispatch_modifiers(this, flags);
            continue;
        }
        update_capslock(this, flags);
        let keystroke = keystroke_for(hid, &characters, &ignoring, flags);
        let inserts_text = keystroke.key_char.as_deref().is_some_and(printable);
        {
            let state = unsafe { get_window_state(this) };
            state.lock().modifiers = keystroke.modifiers;
        }
        let result = dispatch_event(
            this,
            PlatformInput::KeyDown(KeyDownEvent {
                keystroke,
                is_held,
                prefer_character_input: false,
            }),
        );
        if result.propagate && inserts_text {
            unhandled = true;
        }
    }
    if unhandled {
        unsafe {
            let _: () =
                msg_send![super(this, class!(UIView)), pressesBegan: presses withEvent: event];
        }
    }
}

extern "C" fn presses_ended(this: &Object, _: Sel, presses: id, event: id) {
    release_presses(this, presses);
    unsafe {
        let _: () = msg_send![super(this, class!(UIView)), pressesEnded: presses withEvent: event];
    }
}

extern "C" fn presses_cancelled(this: &Object, _: Sel, presses: id, event: id) {
    release_presses(this, presses);
    unsafe {
        let _: () =
            msg_send![super(this, class!(UIView)), pressesCancelled: presses withEvent: event];
    }
}

fn release_presses(this: &Object, presses: id) {
    for key in unsafe { press_keys(presses) } {
        let (hid, characters, ignoring, flags) = unsafe { read_key(key) };
        {
            let state = unsafe { get_window_state(this) };
            state.lock().pressed_keys.remove(&hid);
        }
        if modifier_bit(hid).is_some() || hid == HID_CAPS_LOCK {
            dispatch_modifiers(this, flags);
            continue;
        }
        let keystroke = keystroke_for(hid, &characters, &ignoring, flags);
        dispatch_event(this, PlatformInput::KeyUp(KeyUpEvent { keystroke }));
    }
}

fn dispatch_modifiers(this: &Object, flags: i64) {
    let (modifiers, capslock) = {
        let state = unsafe { get_window_state(this) };
        let mut lock = state.lock();
        let modifiers = modifiers_from_pressed(&lock.pressed_keys);
        let capslock = Capslock {
            on: flags & UI_KEY_MODIFIER_ALPHA_SHIFT != 0,
        };
        lock.modifiers = modifiers;
        lock.capslock = capslock;
        (modifiers, capslock)
    };
    dispatch_event(
        this,
        PlatformInput::ModifiersChanged(ModifiersChangedEvent {
            modifiers,
            capslock,
        }),
    );
}

extern "C" fn has_text(_this: &Object, _: Sel) -> BOOL {
    YES
}

extern "C" fn insert_text(this: &Object, _: Sel, text: id) {
    let Some(text) = (unsafe { nsstring_to_string(text) }) else {
        return;
    };
    if text.is_empty() {
        return;
    }
    if text == "\n" || text == "\r" {
        synthesize_key(this, "enter");
        return;
    }
    if text == "\t" {
        synthesize_key(this, "tab");
        return;
    }
    with_input_handler(this, |handler| handler.replace_text_in_range(None, &text));
}

extern "C" fn delete_backward(this: &Object, _: Sel) {
    synthesize_key(this, "backspace");
}

fn synthesize_key(this: &Object, key: &str) {
    let state = unsafe { get_window_state(this) };
    let modifiers = state.lock().modifiers;
    synthesize_chord(this, key, modifiers);
}

fn synthesize_chord(this: &Object, key: &str, modifiers: Modifiers) {
    let keystroke = Keystroke {
        modifiers,
        key: key.to_owned(),
        key_char: None,
    };
    dispatch_event(
        this,
        PlatformInput::KeyDown(KeyDownEvent {
            keystroke: keystroke.clone(),
            is_held: false,
            prefer_character_input: false,
        }),
    );
    dispatch_event(this, PlatformInput::KeyUp(KeyUpEvent { keystroke }));
}

extern "C" fn autocapitalization_type(_this: &Object, _: Sel) -> i64 {
    UI_TEXT_AUTOCAPITALIZATION_NONE
}

extern "C" fn text_trait_no(_this: &Object, _: Sel) -> i64 {
    UI_TEXT_TRAIT_NO
}

extern "C" fn keyboard_appearance(_this: &Object, _: Sel) -> i64 {
    match crate::platform::screen_appearance() {
        WindowAppearance::Light | WindowAppearance::VibrantLight => UI_KEYBOARD_APPEARANCE_LIGHT,
        WindowAppearance::Dark | WindowAppearance::VibrantDark => UI_KEYBOARD_APPEARANCE_DARK,
    }
}

const POSITION_OFFSET_IVAR: &str = "zzOffset";
const RANGE_START_IVAR: &str = "zzRangeStart";
const RANGE_END_IVAR: &str = "zzRangeEnd";

static mut TEXT_POSITION_CLASS: *const Class = ptr::null();
static mut TEXT_RANGE_CLASS: *const Class = ptr::null();

static INPUT_DELEGATE: AtomicPtr<Object> = AtomicPtr::new(ptr::null_mut());
static TOKENIZER: AtomicPtr<Object> = AtomicPtr::new(ptr::null_mut());

#[repr(C)]
#[derive(Clone, Copy)]
struct NSRange {
    location: usize,
    length: usize,
}

unsafe impl objc::Encode for NSRange {
    fn encode() -> objc::Encoding {
        unsafe { objc::Encoding::from_str("{_NSRange=QQ}") }
    }
}

impl NSRange {
    fn to_range(self) -> Option<Range<usize>> {
        if self.location == NS_NOT_FOUND {
            return None;
        }
        Some(self.location..self.location.checked_add(self.length)?)
    }
}

fn register_text_classes() {
    let mut position = ClassDecl::new("ZZTextPosition", class!(UITextPosition)).unwrap();
    position.add_ivar::<i64>(POSITION_OFFSET_IVAR);
    let mut range = ClassDecl::new("ZZTextRange", class!(UITextRange)).unwrap();
    range.add_ivar::<i64>(RANGE_START_IVAR);
    range.add_ivar::<i64>(RANGE_END_IVAR);
    unsafe {
        position.add_method(
            sel!(isEqual:),
            position_is_equal as extern "C" fn(&Object, Sel, id) -> BOOL,
        );
        position.add_method(
            sel!(hash),
            position_hash as extern "C" fn(&Object, Sel) -> usize,
        );
        TEXT_POSITION_CLASS = position.register();

        range.add_method(
            sel!(start),
            range_start as extern "C" fn(&Object, Sel) -> id,
        );
        range.add_method(sel!(end), range_end as extern "C" fn(&Object, Sel) -> id);
        range.add_method(
            sel!(isEmpty),
            range_is_empty as extern "C" fn(&Object, Sel) -> BOOL,
        );
        range.add_method(
            sel!(isEqual:),
            range_is_equal as extern "C" fn(&Object, Sel, id) -> BOOL,
        );
        range.add_method(
            sel!(hash),
            range_hash as extern "C" fn(&Object, Sel) -> usize,
        );
        TEXT_RANGE_CLASS = range.register();
    }
}

unsafe fn make_position(offset: usize) -> id {
    let position: id = msg_send![TEXT_POSITION_CLASS, alloc];
    let position: id = msg_send![position, init];
    (*position).set_ivar(POSITION_OFFSET_IVAR, clamp_offset(offset));
    let position: id = msg_send![position, autorelease];
    position
}

unsafe fn make_range(range: Range<usize>) -> id {
    let object: id = msg_send![TEXT_RANGE_CLASS, alloc];
    let object: id = msg_send![object, init];
    (*object).set_ivar(RANGE_START_IVAR, clamp_offset(range.start.min(range.end)));
    (*object).set_ivar(RANGE_END_IVAR, clamp_offset(range.start.max(range.end)));
    let object: id = msg_send![object, autorelease];
    object
}

const fn clamp_offset(offset: usize) -> i64 {
    if offset > i64::MAX as usize {
        i64::MAX
    } else {
        offset as i64
    }
}

unsafe fn position_offset(position: id) -> Option<usize> {
    if position.is_null() || !ptr::eq((*position).class(), TEXT_POSITION_CLASS) {
        return None;
    }
    let offset: i64 = *(*position).get_ivar(POSITION_OFFSET_IVAR);
    usize::try_from(offset).ok()
}

unsafe fn range_offsets(range: id) -> Option<Range<usize>> {
    if range.is_null() || !ptr::eq((*range).class(), TEXT_RANGE_CLASS) {
        return None;
    }
    let start: i64 = *(*range).get_ivar(RANGE_START_IVAR);
    let end: i64 = *(*range).get_ivar(RANGE_END_IVAR);
    let start = usize::try_from(start).ok()?;
    let end = usize::try_from(end).ok()?;
    Some(start.min(end)..start.max(end))
}

extern "C" fn position_is_equal(this: &Object, _: Sel, other: id) -> BOOL {
    let this = unsafe { position_offset(this as *const Object as id) };
    let other = unsafe { position_offset(other) };
    if this.is_some() && this == other {
        YES
    } else {
        NO
    }
}

extern "C" fn position_hash(this: &Object, _: Sel) -> usize {
    unsafe { position_offset(this as *const Object as id) }.unwrap_or_default()
}

extern "C" fn range_start(this: &Object, _: Sel) -> id {
    let Some(range) = (unsafe { range_offsets(this as *const Object as id) }) else {
        return nil;
    };
    unsafe { make_position(range.start) }
}

extern "C" fn range_end(this: &Object, _: Sel) -> id {
    let Some(range) = (unsafe { range_offsets(this as *const Object as id) }) else {
        return nil;
    };
    unsafe { make_position(range.end) }
}

extern "C" fn range_is_empty(this: &Object, _: Sel) -> BOOL {
    let empty = unsafe { range_offsets(this as *const Object as id) }
        .is_none_or(|range| range.start == range.end);
    if empty { YES } else { NO }
}

extern "C" fn range_is_equal(this: &Object, _: Sel, other: id) -> BOOL {
    let this = unsafe { range_offsets(this as *const Object as id) };
    let other = unsafe { range_offsets(other) };
    if this.is_some() && this == other {
        YES
    } else {
        NO
    }
}

extern "C" fn range_hash(this: &Object, _: Sel) -> usize {
    unsafe { range_offsets(this as *const Object as id) }
        .map_or(0, |range| range.start ^ (range.end << 1))
}

/// Where the document ends, in UTF-16 offsets. An element that does not know
/// its own length (the terminal) ends at the composition or the caret.
fn document_length(this: &Object) -> usize {
    with_input_handler(this, |handler| {
        if let Some(length) = handler.text_length_utf16() {
            return length;
        }
        let marked = handler.marked_text_range().map_or(0, |range| range.end);
        let selected = handler
            .selected_text_range(false)
            .map_or(0, |selection| selection.range.end);
        marked.max(selected)
    })
    .unwrap_or_default()
}

extern "C" fn beginning_of_document(_this: &Object, _: Sel) -> id {
    unsafe { make_position(0) }
}

extern "C" fn end_of_document(this: &Object, _: Sel) -> id {
    unsafe { make_position(document_length(this)) }
}

extern "C" fn text_in_range(this: &Object, _: Sel, range: id) -> id {
    let Some(range) = (unsafe { range_offsets(range) }) else {
        return nil;
    };
    if range.is_empty() {
        return unsafe { ns_string("") };
    }
    let text = with_input_handler(this, |handler| {
        let mut adjusted = None;
        handler.text_for_range(range, &mut adjusted)
    })
    .flatten();
    let Some(text) = text else {
        return nil;
    };
    // `ns_string` goes through a C string, so an interior NUL would panic.
    unsafe { ns_string(&text.replace('\0', "")) }
}

extern "C" fn replace_range_with_text(this: &Object, _: Sel, range: id, text: id) {
    let Some(range) = (unsafe { range_offsets(range) }) else {
        return;
    };
    let text = unsafe { nsstring_to_string(text) }.unwrap_or_default();
    if text.is_empty() && range.is_empty() {
        return;
    }
    with_input_handler(this, |handler| {
        handler.replace_text_in_range(Some(range), &text);
    });
}

/// Never `nil`: UIKit will not type into a responder that reports no selection,
/// so an unfocused window answers with an empty one at the end of the document.
extern "C" fn selected_text_range(this: &Object, _: Sel) -> id {
    let selection =
        with_input_handler(this, |handler| handler.selected_text_range(false)).flatten();
    match selection {
        Some(selection) => unsafe { make_range(selection.range) },
        None => {
            let end = document_length(this);
            unsafe { make_range(end..end) }
        }
    }
}

extern "C" fn set_selected_text_range(this: &Object, _: Sel, range: id) {
    let Some(range) = (unsafe { range_offsets(range) }) else {
        return;
    };
    with_input_handler(this, |handler| handler.set_selected_text_range(range));
}

extern "C" fn marked_text_range(this: &Object, _: Sel) -> id {
    let range = with_input_handler(this, |handler| handler.marked_text_range()).flatten();
    range.map_or(nil, |range| unsafe { make_range(range) })
}

extern "C" fn marked_text_style(_this: &Object, _: Sel) -> id {
    nil
}

extern "C" fn set_marked_text_style(_this: &Object, _: Sel, _style: id) {}

extern "C" fn set_marked_text(this: &Object, _: Sel, text: id, selected_range: NSRange) {
    let text = unsafe { nsstring_to_string(text) }.unwrap_or_default();
    let selected_range = selected_range.to_range();
    with_input_handler(this, |handler| {
        handler.replace_and_mark_text_in_range(None, &text, selected_range);
    });
}

extern "C" fn unmark_text(this: &Object, _: Sel) {
    with_input_handler(this, |handler| handler.unmark_text());
}

extern "C" fn text_range_from_position(_this: &Object, _: Sel, from: id, to: id) -> id {
    let (Some(from), Some(to)) = (unsafe { position_offset(from) }, unsafe {
        position_offset(to)
    }) else {
        return nil;
    };
    unsafe { make_range(from..to) }
}

fn position_at(this: &Object, position: id, delta: i64) -> id {
    let Some(base) = (unsafe { position_offset(position) }) else {
        return nil;
    };
    let Some(target) = clamp_offset(base).checked_add(delta) else {
        return nil;
    };
    let Ok(target) = usize::try_from(target) else {
        return nil;
    };
    if target > document_length(this) {
        return nil;
    }
    unsafe { make_position(target) }
}

extern "C" fn position_from_position(this: &Object, _: Sel, position: id, offset: i64) -> id {
    position_at(this, position, offset)
}

extern "C" fn position_from_position_in_direction(
    this: &Object,
    _: Sel,
    position: id,
    direction: i64,
    offset: i64,
) -> id {
    let delta = if direction_is_forward(direction) {
        offset
    } else {
        offset.saturating_neg()
    };
    position_at(this, position, delta)
}

const fn direction_is_forward(direction: i64) -> bool {
    matches!(
        direction,
        UI_TEXT_STORAGE_DIRECTION_FORWARD
            | UI_TEXT_LAYOUT_DIRECTION_RIGHT
            | UI_TEXT_LAYOUT_DIRECTION_DOWN
    )
}

extern "C" fn compare_position(_this: &Object, _: Sel, position: id, other: id) -> i64 {
    let (Some(position), Some(other)) = (unsafe { position_offset(position) }, unsafe {
        position_offset(other)
    }) else {
        return 0;
    };
    // NSComparisonResult is -1/0/1, the same as std's Ordering discriminant.
    position.cmp(&other) as i64
}

extern "C" fn offset_from_position(_this: &Object, _: Sel, from: id, to: id) -> i64 {
    let (Some(from), Some(to)) = (unsafe { position_offset(from) }, unsafe {
        position_offset(to)
    }) else {
        return 0;
    };
    clamp_offset(to) - clamp_offset(from)
}

extern "C" fn input_delegate(_this: &Object, _: Sel) -> id {
    INPUT_DELEGATE.load(Ordering::Acquire)
}

extern "C" fn set_input_delegate(_this: &Object, _: Sel, delegate: id) {
    INPUT_DELEGATE.store(delegate, Ordering::Release);
}

extern "C" fn tokenizer(this: &Object, _: Sel) -> id {
    let existing = TOKENIZER.load(Ordering::Acquire);
    if !existing.is_null() {
        return existing;
    }
    unsafe {
        let tokenizer: id = msg_send![class!(UITextInputStringTokenizer), alloc];
        let tokenizer: id = msg_send![tokenizer, initWithTextInput: this as *const Object as id];
        TOKENIZER.store(tokenizer, Ordering::Release);
        tokenizer
    }
}

extern "C" fn position_within_range(_this: &Object, _: Sel, range: id, direction: i64) -> id {
    let Some(range) = (unsafe { range_offsets(range) }) else {
        return nil;
    };
    let offset = if direction_is_forward(direction) {
        range.end
    } else {
        range.start
    };
    unsafe { make_position(offset) }
}

extern "C" fn character_range_by_extending(
    this: &Object,
    _: Sel,
    position: id,
    direction: i64,
) -> id {
    let Some(base) = (unsafe { position_offset(position) }) else {
        return nil;
    };
    let range = if direction_is_forward(direction) {
        base..base.saturating_add(1).min(document_length(this))
    } else {
        base.saturating_sub(1)..base
    };
    unsafe { make_range(range) }
}

extern "C" fn base_writing_direction(
    _this: &Object,
    _: Sel,
    _position: id,
    _direction: i64,
) -> i64 {
    NS_WRITING_DIRECTION_LEFT_TO_RIGHT
}

extern "C" fn set_base_writing_direction(_this: &Object, _: Sel, _direction: i64, _range: id) {}

fn rect_for_range(this: &Object, range: Range<usize>) -> CGRect {
    let bounds = with_input_handler(this, |handler| handler.bounds_for_range(range)).flatten();
    bounds.map_or_else(CGRect::default, |bounds| CGRect {
        origin: CGPoint {
            x: f64::from(f32::from(bounds.origin.x)),
            y: f64::from(f32::from(bounds.origin.y)),
        },
        size: CGSize {
            width: f64::from(f32::from(bounds.size.width)),
            height: f64::from(f32::from(bounds.size.height)),
        },
    })
}

extern "C" fn first_rect_for_range(this: &Object, _: Sel, range: id) -> CGRect {
    let Some(range) = (unsafe { range_offsets(range) }) else {
        return CGRect::default();
    };
    rect_for_range(this, range)
}

extern "C" fn caret_rect_for_position(this: &Object, _: Sel, position: id) -> CGRect {
    let Some(offset) = (unsafe { position_offset(position) }) else {
        return CGRect::default();
    };
    rect_for_range(this, offset..offset)
}

extern "C" fn selection_rects_for_range(_this: &Object, _: Sel, _range: id) -> id {
    unsafe { msg_send![class!(NSArray), array] }
}

fn position_for_point(this: &Object, location: CGPoint) -> Option<usize> {
    let location = point(px(location.x as f32), px(location.y as f32));
    with_input_handler(this, |handler| handler.character_index_for_point(location)).flatten()
}

extern "C" fn closest_position_to_point(this: &Object, _: Sel, location: CGPoint) -> id {
    let offset = position_for_point(this, location).unwrap_or_else(|| document_length(this));
    unsafe { make_position(offset) }
}

extern "C" fn closest_position_within_range(
    this: &Object,
    _: Sel,
    location: CGPoint,
    range: id,
) -> id {
    let Some(range) = (unsafe { range_offsets(range) }) else {
        return nil;
    };
    let offset = position_for_point(this, location)
        .unwrap_or(range.start)
        .clamp(range.start, range.end);
    unsafe { make_position(offset) }
}

extern "C" fn character_range_at_point(this: &Object, _: Sel, location: CGPoint) -> id {
    let Some(offset) = position_for_point(this, location) else {
        return nil;
    };
    unsafe { make_range(offset..offset.saturating_add(1).min(document_length(this))) }
}

extern "C" fn begin_floating_cursor(_this: &Object, _: Sel, _location: CGPoint) {}

extern "C" fn update_floating_cursor(_this: &Object, _: Sel, _location: CGPoint) {}

extern "C" fn end_floating_cursor(_this: &Object, _: Sel) {}

static KEYBOARD_INSET: AtomicU32 = AtomicU32::new(0);
static KEYBOARD_INSET_CHANGED: AtomicBool = AtomicBool::new(false);

static SAFE_AREA_TOP: AtomicU32 = AtomicU32::new(0);
static SAFE_AREA_RIGHT: AtomicU32 = AtomicU32::new(0);
static SAFE_AREA_BOTTOM: AtomicU32 = AtomicU32::new(0);
static SAFE_AREA_LEFT: AtomicU32 = AtomicU32::new(0);
static SAFE_AREA_INSETS_CHANGED: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SafeAreaInsets {
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
    pub left: f32,
}

pub fn safe_area_insets() -> SafeAreaInsets {
    SafeAreaInsets {
        top: f32::from_bits(SAFE_AREA_TOP.load(Ordering::Relaxed)),
        right: f32::from_bits(SAFE_AREA_RIGHT.load(Ordering::Relaxed)),
        bottom: f32::from_bits(SAFE_AREA_BOTTOM.load(Ordering::Relaxed)),
        left: f32::from_bits(SAFE_AREA_LEFT.load(Ordering::Relaxed)),
    }
}

fn set_safe_area_insets(insets: UIEdgeInsets) {
    let mut changed = false;
    for (value, slot) in [
        (insets.top, &SAFE_AREA_TOP),
        (insets.right, &SAFE_AREA_RIGHT),
        (insets.bottom, &SAFE_AREA_BOTTOM),
        (insets.left, &SAFE_AREA_LEFT),
    ] {
        let value = value as f32;
        let value = if value.is_finite() {
            value.max(0.0)
        } else {
            0.0
        };
        changed |= slot.swap(value.to_bits(), Ordering::Relaxed) != value.to_bits();
    }
    if changed {
        SAFE_AREA_INSETS_CHANGED.store(true, Ordering::Relaxed);
    }
}

/// Points at the bottom of the window the software keyboard covers, zero when
/// it is down. A change forces the next frame, so there is nothing to subscribe to.
pub fn keyboard_inset() -> f32 {
    f32::from_bits(KEYBOARD_INSET.load(Ordering::Relaxed))
}

fn set_keyboard_inset(inset: f32) {
    if !inset.is_finite() {
        return;
    }
    let bits = inset.to_bits();
    if KEYBOARD_INSET.swap(bits, Ordering::Relaxed) != bits {
        KEYBOARD_INSET_CHANGED.store(true, Ordering::Relaxed);
    }
}

unsafe fn observe_keyboard(native_view: id) {
    let center: id = msg_send![class!(NSNotificationCenter), defaultCenter];
    for name in [
        UIKeyboardWillChangeFrameNotification,
        UIKeyboardWillHideNotification,
    ] {
        let _: () = msg_send![
            center,
            addObserver: native_view
            selector: sel!(zzKeyboardFrameChanged:)
            name: name
            object: nil
        ];
    }
}

extern "C" fn keyboard_frame_changed(this: &Object, _: Sel, notification: id) {
    set_keyboard_inset(unsafe { keyboard_overlap(this, notification) });
}

unsafe fn keyboard_overlap(this: &Object, notification: id) -> f32 {
    let view = this as *const Object as id;
    let name: id = msg_send![notification, name];
    let hiding: BOOL = msg_send![name, isEqualToString: UIKeyboardWillHideNotification];
    if hiding == YES {
        return 0.0;
    }
    let window: id = msg_send![view, window];
    let info: id = msg_send![notification, userInfo];
    if window.is_null() || info.is_null() {
        return 0.0;
    }
    let value: id = msg_send![info, objectForKey: UIKeyboardFrameEndUserInfoKey];
    if value.is_null() {
        return 0.0;
    }
    let frame: CGRect = msg_send![value, CGRectValue];
    let in_window: CGRect = msg_send![window, convertRect: frame fromWindow: nil];
    let in_view: CGRect = msg_send![view, convertRect: in_window fromView: nil];
    let bounds: CGRect = msg_send![view, bounds];
    bottom_occlusion(bounds, in_view)
}

fn bottom_occlusion(bounds: CGRect, occluder: CGRect) -> f32 {
    let values = [
        bounds.origin.x,
        bounds.origin.y,
        bounds.size.width,
        bounds.size.height,
        occluder.origin.x,
        occluder.origin.y,
        occluder.size.width,
        occluder.size.height,
    ];
    if values.iter().any(|value| !value.is_finite())
        || bounds.size.width <= 0.0
        || bounds.size.height <= 0.0
        || occluder.size.width <= 0.0
        || occluder.size.height <= 0.0
    {
        return 0.0;
    }

    let bounds_right = bounds.origin.x + bounds.size.width;
    let bounds_bottom = bounds.origin.y + bounds.size.height;
    let occluder_right = occluder.origin.x + occluder.size.width;
    let occluder_bottom = occluder.origin.y + occluder.size.height;
    let edge_slop = 1.0;
    if occluder.origin.x > bounds.origin.x + edge_slop
        || occluder_right < bounds_right - edge_slop
        || occluder_bottom < bounds_bottom - edge_slop
    {
        return 0.0;
    }

    (bounds_bottom - occluder.origin.y).clamp(0.0, bounds.size.height) as f32
}

extern "C" fn scroll_view_will_begin_dragging(this: &Object, _: Sel, scroll_view: id) {
    let offset: CGPoint = unsafe { msg_send![scroll_view, contentOffset] };
    let state = unsafe { get_window_state(this) };
    let mut lock = state.lock();
    lock.scroll_offset = offset;
    lock.scroll_began = true;
}

extern "C" fn scroll_view_did_scroll(this: &Object, _: Sel, scroll_view: id) {
    let state = unsafe { get_window_state(this) };
    let dispatch = {
        let mut lock = state.lock();
        let offset: CGPoint = unsafe { msg_send![scroll_view, contentOffset] };
        let delta = point(
            px((lock.scroll_offset.x - offset.x) as f32),
            px((lock.scroll_offset.y - offset.y) as f32),
        );
        lock.scroll_offset = offset;
        if delta.x == px(0.0) && delta.y == px(0.0) {
            None
        } else {
            let tracking: BOOL = unsafe { msg_send![scroll_view, isTracking] };
            if tracking == YES {
                let native_view = lock.native_view;
                let pan: id = unsafe { msg_send![scroll_view, panGestureRecognizer] };
                let location: CGPoint = unsafe { msg_send![pan, locationInView: native_view] };
                lock.last_touch = point(px(location.x as f32), px(location.y as f32));
            }
            let touch_phase = if std::mem::take(&mut lock.scroll_began) {
                TouchPhase::Started
            } else {
                TouchPhase::Moved
            };
            Some(ScrollWheelEvent {
                position: lock.last_touch,
                delta: ScrollDelta::Pixels(delta),
                modifiers: lock.modifiers,
                touch_phase,
            })
        }
    };
    if let Some(event) = dispatch {
        dispatch_event(this, PlatformInput::ScrollWheel(event));
    }
}

extern "C" fn scroll_view_did_end_dragging(
    this: &Object,
    _: Sel,
    scroll_view: id,
    will_decelerate: BOOL,
) {
    if will_decelerate == NO {
        end_scroll(this, scroll_view);
    }
}

extern "C" fn scroll_view_did_end_decelerating(this: &Object, _: Sel, scroll_view: id) {
    end_scroll(this, scroll_view);
}

/// Closes the gesture and recentres the offset in the virtual canvas, so the
/// next flick has a full canvas of runway either way.
fn end_scroll(this: &Object, scroll_view: id) {
    let (position, modifiers, bounds) = {
        let state = unsafe { get_window_state(this) };
        let lock = state.lock();
        let bounds: CGRect = unsafe { msg_send![lock.native_view, bounds] };
        (lock.last_touch, lock.modifiers, bounds)
    };
    dispatch_event(
        this,
        PlatformInput::ScrollWheel(ScrollWheelEvent {
            position,
            delta: ScrollDelta::Pixels(Point::default()),
            modifiers,
            touch_phase: TouchPhase::Ended,
        }),
    );
    let center = scroll_center(bounds);
    {
        let state = unsafe { get_window_state(this) };
        let mut lock = state.lock();
        lock.scroll_offset = center;
        lock.scroll_began = false;
    }
    unsafe {
        let _: () = msg_send![scroll_view, setContentOffset: center animated: NO];
    }
}

static EDIT_MENU_INTERACTION: AtomicPtr<Object> = AtomicPtr::new(ptr::null_mut());

unsafe fn attach_selection(native_view: id) {
    let press: id = msg_send![class!(UILongPressGestureRecognizer), alloc];
    let press: id = msg_send![press, initWithTarget: native_view action: sel!(zzLongPress:)];
    let _: () = msg_send![press, setMinimumPressDuration: LONG_PRESS_DURATION];
    let _: () = msg_send![native_view, addGestureRecognizer: press];

    // Looked up by name: `class!` panics on a runtime that lacks the class.
    let Some(menu_class) = Class::get("UIEditMenuInteraction") else {
        log::warn!("UIEditMenuInteraction unavailable; selection has no copy/paste menu");
        return;
    };
    let interaction: id = msg_send![menu_class, alloc];
    let interaction: id = msg_send![interaction, initWithDelegate: nil];
    let _: () = msg_send![native_view, addInteraction: interaction];
    EDIT_MENU_INTERACTION.store(interaction, Ordering::Release);
}

/// Turns a long press into a held left button: the mouse selection the app already has.
extern "C" fn long_press(this: &Object, _: Sel, recognizer: id) {
    let (gesture_state, location) = unsafe {
        let gesture_state: i64 = msg_send![recognizer, state];
        let location: CGPoint = msg_send![recognizer, locationInView: this as *const Object as id];
        (gesture_state, location)
    };
    let position = point(px(location.x as f32), px(location.y as f32));
    let (modifiers, scroll_view, selecting) = {
        let state = unsafe { get_window_state(this) };
        let mut lock = state.lock();
        lock.last_touch = position;
        (lock.modifiers, lock.scroll_view, lock.selecting)
    };

    if gesture_state == UI_GESTURE_STATE_BEGAN {
        {
            let state = unsafe { get_window_state(this) };
            let mut lock = state.lock();
            lock.selecting = true;
            lock.tap_origin = None;
        }
        // The pan sees the same finger; both running scrolls the scene away.
        set_pan_enabled(scroll_view, false);
        dispatch_event(
            this,
            PlatformInput::MouseDown(MouseDownEvent {
                button: MouseButton::Left,
                position,
                modifiers,
                click_count: SELECTION_CLICK_COUNT,
                first_mouse: false,
            }),
        );
        return;
    }

    if !selecting {
        return;
    }

    if gesture_state == UI_GESTURE_STATE_CHANGED {
        dispatch_event(
            this,
            PlatformInput::MouseMove(MouseMoveEvent {
                position,
                pressed_button: Some(MouseButton::Left),
                modifiers,
            }),
        );
        return;
    }

    {
        let state = unsafe { get_window_state(this) };
        state.lock().selecting = false;
    }
    set_pan_enabled(scroll_view, true);
    dispatch_event(
        this,
        PlatformInput::MouseUp(MouseUpEvent {
            button: MouseButton::Left,
            position,
            modifiers,
            click_count: 1,
        }),
    );
    if gesture_state == UI_GESTURE_STATE_ENDED {
        unsafe { present_edit_menu(this, location) };
    }
}

fn set_pan_enabled(scroll_view: id, enabled: bool) {
    if scroll_view.is_null() {
        return;
    }
    unsafe {
        let pan: id = msg_send![scroll_view, panGestureRecognizer];
        let _: () = msg_send![pan, setEnabled: if enabled { YES } else { NO }];
    }
}

unsafe fn present_edit_menu(this: &Object, location: CGPoint) {
    let interaction = EDIT_MENU_INTERACTION.load(Ordering::Acquire);
    if interaction.is_null() {
        return;
    }
    let Some(configuration_class) = Class::get("UIEditMenuConfiguration") else {
        return;
    };
    let responder: BOOL = msg_send![this, isFirstResponder];
    if responder == NO {
        let _: () = msg_send![this, becomeFirstResponder];
    }
    let configuration: id = msg_send![
        configuration_class,
        configurationWithIdentifier: nil
        sourcePoint: location
    ];
    if configuration.is_null() {
        return;
    }
    let _: () = msg_send![interaction, presentEditMenuWithConfiguration: configuration];
}

/// Which `UIResponderStandardEditActions` this view can do, which is also the
/// whole edit menu: UIKit builds it from these answers.
extern "C" fn can_perform_action(_this: &Object, _: Sel, action: Sel, _sender: id) -> BOOL {
    if action == sel!(copy:) || action == sel!(selectAll:) {
        return YES;
    }
    if action == sel!(paste:) {
        return unsafe {
            let pasteboard: id = msg_send![class!(UIPasteboard), generalPasteboard];
            if pasteboard.is_null() {
                NO
            } else {
                msg_send![pasteboard, hasStrings]
            }
        };
    }
    NO
}

extern "C" fn edit_copy(this: &Object, _: Sel, _sender: id) {
    synthesize_platform_chord(this, "c");
}

extern "C" fn edit_paste(this: &Object, _: Sel, _sender: id) {
    synthesize_platform_chord(this, "v");
}

extern "C" fn edit_select_all(this: &Object, _: Sel, _sender: id) {
    synthesize_platform_chord(this, "a");
}

/// A `platform-<key>` press/release, which is how the app spells copy, paste
/// and select all. The window's own modifier state is left alone.
fn synthesize_platform_chord(this: &Object, key: &str) {
    synthesize_chord(
        this,
        key,
        Modifiers {
            platform: true,
            ..Modifiers::default()
        },
    );
}

// Entries for the Cmd-hold shortcut HUD. Every one carries Command, which is
// what keeps them exclusive with `pressesBegan:`: a Command chord has no
// `key_char`, so the press path never hands it back to UIKit.
static KEY_COMMANDS: AtomicPtr<Object> = AtomicPtr::new(ptr::null_mut());

extern "C" fn key_commands(_this: &Object, _: Sel) -> id {
    let cached = KEY_COMMANDS.load(Ordering::Acquire);
    if !cached.is_null() {
        return cached;
    }
    unsafe {
        let commands: id = msg_send![class!(NSMutableArray), array];
        for (input, action, title) in [
            ("c", sel!(copy:), "Copy"),
            ("v", sel!(paste:), "Paste"),
            ("a", sel!(selectAll:), "Select All"),
        ] {
            let command: id = msg_send![
                class!(UIKeyCommand),
                keyCommandWithInput: ns_string(input)
                modifierFlags: UI_KEY_MODIFIER_COMMAND
                action: action
            ];
            if command.is_null() {
                continue;
            }
            // Without a title the command works but stays out of the HUD.
            let _: () = msg_send![command, setDiscoverabilityTitle: ns_string(title)];
            let _: () = msg_send![commands, addObject: command];
        }
        let commands: id = msg_send![commands, retain];
        KEY_COMMANDS.store(commands, Ordering::Release);
        commands
    }
}

/// Text a drop delivered, waiting for the next frame to insert it. UIKit says
/// nothing about which queue answers the load, so the frame pump drains it.
static PENDING_DROP: Mutex<Option<String>> = Mutex::new(None);

unsafe fn attach_drop(native_view: id) {
    let interaction: id = msg_send![class!(UIDropInteraction), alloc];
    let interaction: id = msg_send![interaction, initWithDelegate: native_view];
    let _: () = msg_send![native_view, addInteraction: interaction];
}

unsafe fn drop_object_class(session: id) -> id {
    for class in [class!(NSURL), class!(NSString)] {
        let loadable: BOOL = msg_send![session, canLoadObjectsOfClass: class];
        if loadable == YES {
            return class as *const Class as id;
        }
    }
    nil
}

extern "C" fn drop_can_handle_session(
    _this: &Object,
    _: Sel,
    _interaction: id,
    session: id,
) -> BOOL {
    if unsafe { drop_object_class(session) }.is_null() {
        NO
    } else {
        YES
    }
}

extern "C" fn drop_session_did_update(
    _this: &Object,
    _: Sel,
    _interaction: id,
    _session: id,
) -> id {
    unsafe {
        let proposal: id = msg_send![class!(UIDropProposal), alloc];
        let proposal: id = msg_send![proposal, initWithDropOperation: UI_DROP_OPERATION_COPY];
        let proposal: id = msg_send![proposal, autorelease];
        proposal
    }
}

extern "C" fn drop_perform(_this: &Object, _: Sel, _interaction: id, session: id) {
    let object_class = unsafe { drop_object_class(session) };
    if object_class.is_null() {
        return;
    }
    let block = ConcreteBlock::new(move |objects: id| {
        if let Some(text) = unsafe { dropped_text(objects) } {
            *PENDING_DROP.lock() = Some(text);
        }
    });
    let block = block.copy();
    unsafe {
        let _: id = msg_send![session, loadObjectsOfClass: object_class completion: &*block];
    }
}

unsafe fn dropped_text(objects: id) -> Option<String> {
    if objects.is_null() {
        return None;
    }
    let count: usize = msg_send![objects, count];
    let mut parts = Vec::with_capacity(count);
    for index in 0..count {
        let object: id = msg_send![objects, objectAtIndex: index];
        if object.is_null() {
            continue;
        }
        let is_url: BOOL = msg_send![object, isKindOfClass: class!(NSURL)];
        let string: id = if is_url == YES {
            let file: BOOL = msg_send![object, isFileURL];
            if file == YES {
                msg_send![object, path]
            } else {
                msg_send![object, absoluteString]
            }
        } else {
            object
        };
        if let Some(text) = nsstring_to_string(string).filter(|text| !text.is_empty()) {
            parts.push(text);
        }
    }
    (!parts.is_empty()).then(|| parts.join(" "))
}

/// Hands a drop's text to whatever gpui has focused, not to the pane under the
/// finger: aiming it there would mean synthesising a click first.
fn insert_dropped_text(this: &Object) {
    let dropped = PENDING_DROP.lock().take();
    let Some(text) = dropped else {
        return;
    };
    with_input_handler(this, |handler| {
        handler.replace_text_in_range(None, &text);
    });
}

/// `1.0f32`'s bit pattern: nothing pinched since the app last looked.
const PINCH_IDENTITY: u32 = 0x3f80_0000;

static PINCH_SCALE: AtomicU32 = AtomicU32::new(PINCH_IDENTITY);
static PINCH_CHANGED: AtomicBool = AtomicBool::new(false);

/// Largest zoom change one gesture callback may apply.
const PINCH_DELTA_CLAMP: f64 = 2.0;

/// Zoom factor pinches accumulated since the last call, `None` if none. Taking
/// it resets it, so the caller must apply what it took.
pub fn take_pinch_scale() -> Option<f32> {
    let bits = PINCH_SCALE.swap(PINCH_IDENTITY, Ordering::Relaxed);
    if bits == PINCH_IDENTITY {
        return None;
    }
    let scale = f32::from_bits(bits);
    (scale.is_finite() && scale > 0.0).then_some(scale)
}

fn accumulate_pinch(delta: f64) {
    if !delta.is_finite() || delta <= 0.0 {
        return;
    }
    let delta = delta.clamp(1.0 / PINCH_DELTA_CLAMP, PINCH_DELTA_CLAMP) as f32;
    let scale = f32::from_bits(PINCH_SCALE.load(Ordering::Relaxed)) * delta;
    PINCH_SCALE.store(scale.to_bits(), Ordering::Relaxed);
    PINCH_CHANGED.store(true, Ordering::Relaxed);
}

unsafe fn attach_pinch(native_view: id) {
    let pinch: id = msg_send![class!(UIPinchGestureRecognizer), alloc];
    let pinch: id = msg_send![pinch, initWithTarget: native_view action: sel!(zzPinch:)];
    let _: () = msg_send![native_view, addGestureRecognizer: pinch];
}

/// Reading and resetting the recogniser's scale on every callback turns its
/// absolute scale into the per-callback delta [`PINCH_SCALE`] accumulates.
extern "C" fn pinch(this: &Object, _: Sel, recognizer: id) {
    let gesture_state: i64 = unsafe { msg_send![recognizer, state] };
    if gesture_state == UI_GESTURE_STATE_CHANGED || gesture_state == UI_GESTURE_STATE_ENDED {
        let delta = unsafe {
            let scale: f64 = msg_send![recognizer, scale];
            let _: () = msg_send![recognizer, setScale: 1.0f64];
            scale
        };
        accumulate_pinch(delta);
    }
    if gesture_state == UI_GESTURE_STATE_CHANGED {
        return;
    }
    let (scroll_view, selecting) = {
        let state = unsafe { get_window_state(this) };
        let lock = state.lock();
        (lock.scroll_view, lock.selecting)
    };
    if selecting {
        // A long press already holds the pan off and owns putting it back.
        return;
    }
    set_pan_enabled(scroll_view, gesture_state != UI_GESTURE_STATE_BEGAN);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: f64, y: f64, width: f64, height: f64) -> CGRect {
        CGRect {
            origin: CGPoint { x, y },
            size: CGSize { width, height },
        }
    }

    #[test]
    fn docked_keyboard_returns_bottom_overlap() {
        assert_eq!(
            bottom_occlusion(
                rect(0.0, 0.0, 1024.0, 768.0),
                rect(0.0, 468.0, 1024.0, 300.0)
            ),
            300.0
        );
        assert_eq!(
            bottom_occlusion(
                rect(100.0, 50.0, 824.0, 600.0),
                rect(0.0, 500.0, 1024.0, 300.0)
            ),
            150.0
        );
    }

    #[test]
    fn floating_keyboard_does_not_inset_the_whole_window() {
        let bounds = rect(0.0, 0.0, 1024.0, 768.0);
        assert_eq!(
            bottom_occlusion(bounds, rect(620.0, 480.0, 360.0, 260.0)),
            0.0
        );
        assert_eq!(
            bottom_occlusion(bounds, rect(0.0, 300.0, 1024.0, 300.0)),
            0.0
        );
    }

    #[test]
    fn invalid_keyboard_geometry_is_ignored() {
        let bounds = rect(0.0, 0.0, 1024.0, 768.0);
        assert_eq!(
            bottom_occlusion(bounds, rect(0.0, f64::NAN, 1024.0, 300.0)),
            0.0
        );
        assert_eq!(bottom_occlusion(bounds, rect(0.0, 768.0, 1024.0, 0.0)), 0.0);
    }
}
