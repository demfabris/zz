use crate::{CGPoint, CGRect, IosDisplay, id, nil, ns_array};
use futures::channel::oneshot;
use gpui::accesskit;
use gpui::{
    AnyWindowHandle, Bounds, Capslock, CursorStyle, DevicePixels, DispatchEventResult,
    KeyDownEvent, KeyUpEvent, Keystroke, Modifiers, ModifiersChangedEvent, MouseButton,
    MouseDownEvent, MouseExitEvent, MouseMoveEvent, MouseUpEvent, Pixels, PlatformAtlas,
    PlatformDisplay, PlatformInput, PlatformInputHandler, PlatformWindow, Point, PromptButton,
    PromptLevel, RequestFrameOptions, ScrollDelta, ScrollWheelEvent, Size, TextInputAction,
    TextInputConfiguration, TextInputStateChange, TouchEvent, TouchId, TouchPhase,
    WindowAppearance, WindowBackgroundAppearance, WindowBounds, WindowParams, point, px, size,
};
use objc::{
    class,
    declare::ClassDecl,
    msg_send,
    runtime::{BOOL, Class, Object, Protocol, Sel, YES},
    sel, sel_impl,
};
use raw_window_handle as rwh;
use std::cell::RefCell;
use std::{
    collections::HashMap,
    ffi::c_void,
    ptr::{self, NonNull},
    rc::Rc,
    sync::{Arc, Once},
    time::Instant,
};

const STATE_IVAR: &str = "zzWindowState";
static REGISTER_VIEW: Once = Once::new();
static mut VIEW_CLASS: *const Class = ptr::null();

pub(crate) struct IosWindowState {
    _handle: AnyWindowHandle,
    native_window: id,
    native_view: id,
    display_link: id,
    renderer: gpui_wgpu::WgpuRenderer,
    needs_presentation: bool,
    accesskit_adapter: Option<accesskit_ios::SubclassingAdapter>,
    request_frame_callback: Option<Box<dyn FnMut(RequestFrameOptions)>>,
    event_callback: Option<Box<dyn FnMut(PlatformInput) -> DispatchEventResult>>,
    resize_callback: Option<Box<dyn FnMut(Size<Pixels>, f32)>>,
    appearance_callback: Option<Box<dyn FnMut()>>,
    insets_callback: Option<Box<dyn FnMut(gpui::WindowInsets)>>,
    close_callback: Option<Box<dyn FnOnce()>>,
    input_handler: Option<PlatformInputHandler>,
    keyboard: crate::keyboard::Keyboard,
    input_view: id,
    accessory_view: id,
    keyboard_probe: id,
    keyboard_overlap: f64,
    keyboard_requested: bool,
    soft_keyboard: bool,
    keyboard_sync_pending: bool,
    reload_input_views: bool,
    text_input: TextInputConfiguration,
    latched: Modifiers,
    viewport_callback: Option<Box<dyn FnMut()>>,
    active: bool,
    active_callback: Option<Box<dyn FnMut(bool)>>,
    last_touch: Point<Pixels>,
    touch_gestures: bool,
    touches: HashMap<usize, (TouchId, Point<Pixels>)>,
    next_touch: u64,
    pointer_button: Option<MouseButton>,
    momentum: Option<crate::momentum::Momentum>,
    pointer_interaction: id,
    edit_menu: id,
}

impl IosWindowState {
    pub(crate) fn take_input_handler(&mut self) -> Option<PlatformInputHandler> {
        self.input_handler.take()
    }

    pub(crate) fn restore_input_handler(&mut self, handler: PlatformInputHandler) {
        if self.input_handler.is_none() {
            self.input_handler = Some(handler);
        }
    }
}

pub(crate) struct IosWindow(Rc<RefCell<IosWindowState>>);

impl IosWindow {
    pub(crate) fn open(
        handle: AnyWindowHandle,
        _params: WindowParams,
        touch_gestures: bool,
        appearance: Option<WindowAppearance>,
    ) -> anyhow::Result<Self> {
        REGISTER_VIEW.call_once(register_view_class);

        unsafe {
            let scene = crate::platform::take_window_scene();
            let (screen_bounds, scale): (CGRect, f64) = if scene.is_null() {
                let screen: id = msg_send![class!(UIScreen), mainScreen];
                (msg_send![screen, bounds], IosDisplay::scale_factor() as f64)
            } else {
                let space: id = msg_send![scene, coordinateSpace];
                let screen: id = msg_send![scene, screen];
                (msg_send![space, bounds], msg_send![screen, scale])
            };

            let native_window: id = msg_send![class!(UIWindow), alloc];
            let native_window: id = if scene.is_null() {
                msg_send![native_window, initWithFrame: screen_bounds]
            } else {
                msg_send![native_window, initWithWindowScene: scene]
            };
            apply_window_appearance(native_window, appearance);

            let controller: id = msg_send![class!(UIViewController), new];
            let native_view: id = msg_send![VIEW_CLASS, alloc];
            let native_view: id = msg_send![native_view, initWithFrame: screen_bounds];
            let _: () = msg_send![native_view, setContentScaleFactor: scale];
            let _: () = msg_send![native_view, setAutoresizingMask: 18usize];
            let _: () = msg_send![native_view, setMultipleTouchEnabled: touch_gestures as BOOL];
            let _: () = msg_send![controller, setView: native_view];
            let _: () = msg_send![native_window, setRootViewController: controller];
            let _: () = msg_send![controller, release];

            let instance = gpui_wgpu::wgpu::Instance::new(gpui_wgpu::wgpu::InstanceDescriptor {
                backends: gpui_wgpu::wgpu::Backends::METAL,
                display: Some(Box::new(IosDisplayHandle)),
                flags: Default::default(),
                backend_options: Default::default(),
                memory_budget_thresholds: Default::default(),
            });
            let surface = instance.create_surface_unsafe(
                gpui_wgpu::wgpu::SurfaceTargetUnsafe::RawHandle {
                    raw_display_handle: Some(rwh::RawDisplayHandle::UiKit(
                        rwh::UiKitDisplayHandle::new(),
                    )),
                    raw_window_handle: rwh::RawWindowHandle::UiKit(rwh::UiKitWindowHandle::new(
                        NonNull::new(native_view.cast()).unwrap(),
                    )),
                },
            )?;
            let context = gpui_wgpu::WgpuContext::new(instance, &surface, None)?;
            drop(surface);
            let gpu_context = Rc::new(std::cell::RefCell::new(Some(context)));
            let renderer = gpui_wgpu::WgpuRenderer::new(
                gpu_context,
                &IosRawWindow(native_view as usize),
                gpui_wgpu::WgpuSurfaceConfig {
                    size: size(
                        DevicePixels((screen_bounds.size.width * scale) as i32),
                        DevicePixels((screen_bounds.size.height * scale) as i32),
                    ),
                    transparent: false,
                    preferred_present_mode: None,
                },
                None,
            )?;

            let state = Rc::new(RefCell::new(IosWindowState {
                _handle: handle,
                native_window,
                native_view,
                display_link: nil,
                renderer,
                needs_presentation: false,
                accesskit_adapter: None,
                request_frame_callback: None,
                event_callback: None,
                resize_callback: None,
                appearance_callback: None,
                insets_callback: None,
                close_callback: None,
                input_handler: None,
                keyboard: Default::default(),
                input_view: msg_send![class!(UIView), new],
                accessory_view: accessory_view(native_view),
                keyboard_probe: install_keyboard_probe(native_view),
                keyboard_overlap: 0.0,
                keyboard_requested: false,
                soft_keyboard: false,
                keyboard_sync_pending: false,
                reload_input_views: false,
                text_input: TextInputConfiguration::default(),
                latched: Modifiers::default(),
                viewport_callback: None,
                active: true,
                active_callback: None,
                last_touch: Point::default(),
                touch_gestures,
                touches: HashMap::new(),
                next_touch: 0,
                pointer_button: None,
                momentum: None,
                pointer_interaction: nil,
                edit_menu: nil,
            }));

            state.borrow_mut().renderer.update_drawable_size(size(
                DevicePixels((screen_bounds.size.width * scale) as i32),
                DevicePixels((screen_bounds.size.height * scale) as i32),
            ));

            let state_ptr = Rc::into_raw(state.clone()) as *mut c_void;
            (*native_view).set_ivar(STATE_IVAR, state_ptr);
            let traits = ns_array(&[
                class!(UITraitUserInterfaceStyle) as *const Class as id,
                class!(UITraitPreferredContentSizeCategory) as *const Class as id,
                class!(UITraitAccessibilityContrast) as *const Class as id,
            ]);
            let _: id = msg_send![native_view, registerForTraitChanges: traits withAction: sel!(zzAppearanceChanged)];
            let center: id = msg_send![class!(NSNotificationCenter), defaultCenter];
            let _: () = msg_send![
                center,
                addObserver: native_view
                selector: sel!(zzAccessibilityChanged:)
                name: UIAccessibilityReduceMotionStatusDidChangeNotification
                object: nil
            ];

            state.borrow_mut().pointer_interaction = install_pointer_input(native_view);
            observe_hardware_keyboard(native_view);
            crate::drop::install(native_view);
            let edit_menu: id = msg_send![class!(UIEditMenuInteraction), alloc];
            let edit_menu: id = msg_send![edit_menu, initWithDelegate: native_view];
            let _: () = msg_send![native_view, addInteraction: edit_menu];
            let _: () = msg_send![edit_menu, release];
            state.borrow_mut().edit_menu = edit_menu;

            let _: () = msg_send![native_window, makeKeyAndVisible];
            let _: BOOL = msg_send![native_view, becomeFirstResponder];

            let display_link: id = msg_send![
                class!(CADisplayLink),
                displayLinkWithTarget: native_view
                selector: sel!(zzStep:)
            ];
            let run_loop: id = msg_send![class!(NSRunLoop), mainRunLoop];
            let _: () =
                msg_send![display_link, addToRunLoop: run_loop forMode: NSRunLoopCommonModes];
            state.borrow_mut().display_link = display_link;
            eprintln!(
                "[zz-ios] window open: bounds {}x{} scale {}",
                screen_bounds.size.width, screen_bounds.size.height, scale
            );

            VIEWS.with_borrow_mut(|views| views.push(native_view));
            ACTIVE_VIEW.set(native_view);
            Ok(Self(state))
        }
    }

    fn bounds_impl(&self) -> Bounds<Pixels> {
        let view = self.0.borrow_mut().native_view;
        let rect: CGRect = unsafe { msg_send![view, bounds] };
        Bounds {
            origin: point(px(rect.origin.x as f32), px(rect.origin.y as f32)),
            size: size(px(rect.size.width as f32), px(rect.size.height as f32)),
        }
    }
}

impl Drop for IosWindow {
    fn drop(&mut self) {
        let (view, window, close) = {
            let mut state = self.0.borrow_mut();
            unsafe {
                let _: () = msg_send![state.display_link, invalidate];
                let _: () = msg_send![state.input_view, release];
                let _: () = msg_send![state.accessory_view, release];
                let _: () = msg_send![state.keyboard_probe, release];
                let center: id = msg_send![class!(NSNotificationCenter), defaultCenter];
                let _: () = msg_send![center, removeObserver: state.native_view];
            }
            state.accesskit_adapter = None;
            state.renderer.destroy();
            (
                state.native_view,
                state.native_window,
                state.close_callback.take(),
            )
        };
        VIEWS.with_borrow_mut(|views| views.retain(|registered| *registered != view));
        if ACTIVE_VIEW.get() == view {
            ACTIVE_VIEW.set(
                VIEWS
                    .with_borrow(|views| views.last().copied())
                    .unwrap_or(nil),
            );
        }
        unsafe {
            crate::text_input::release(view);
            let raw: *mut c_void = *(*view).get_ivar(STATE_IVAR);
            (*view).set_ivar(STATE_IVAR, ptr::null_mut::<c_void>());
            drop(Rc::from_raw(raw as *const RefCell<IosWindowState>));
            let _: () = msg_send![window, setHidden: true];
            let _: () = msg_send![window, setRootViewController: nil];
            let _: () = msg_send![view, release];
            let _: () = msg_send![window, release];
        }
        if let Some(close) = close {
            close();
        }
    }
}

impl PlatformWindow for IosWindow {
    fn insets(&self) -> gpui::WindowInsets {
        let state = self.0.borrow();
        view_insets(state.native_view, state.keyboard_overlap)
    }

    fn on_insets_changed(&self, callback: Box<dyn FnMut(gpui::WindowInsets)>) {
        self.0.borrow_mut().insets_callback = Some(callback);
    }

    fn visual_viewport_bounds(&self) -> Bounds<Pixels> {
        let mut bounds = self.bounds_impl();
        bounds.origin = Point::default();
        bounds.size.height =
            (bounds.size.height - px(self.0.borrow().keyboard_overlap as f32)).max(Pixels::ZERO);
        bounds
    }

    fn on_visual_viewport_changed(&self, callback: Box<dyn FnMut()>) {
        self.0.borrow_mut().viewport_callback = Some(callback);
    }

    fn show_soft_keyboard(&self) {
        let view = self.0.borrow().native_view;
        request_keyboard(view, true);
    }

    fn hide_soft_keyboard(&self) {
        let view = self.0.borrow().native_view;
        request_keyboard(view, false);
    }

    fn text_input_state_changed(&self, change: TextInputStateChange) {
        match change {
            TextInputStateChange::FocusGained => self.show_soft_keyboard(),
            TextInputStateChange::FocusLost => self.hide_soft_keyboard(),
            TextInputStateChange::SelectionChanged | TextInputStateChange::ContentChanged => {}
        }
    }

    fn set_text_input_configuration(&mut self, configuration: TextInputConfiguration) {
        let view = {
            let mut state = self.0.borrow_mut();
            state.text_input = configuration;
            state.reload_input_views = true;
            state.native_view
        };
        schedule_keyboard_sync(view);
    }

    fn bounds(&self) -> Bounds<Pixels> {
        self.bounds_impl()
    }

    fn visibility(&self) -> gpui::WindowVisibility {
        gpui::WindowVisibility::Visible
    }

    fn on_visibility_change(&self, _callback: Box<dyn FnMut(gpui::WindowVisibility)>) {}

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
        display_scale(self.0.borrow().native_view) as f32
    }

    fn appearance(&self) -> WindowAppearance {
        let view = self.0.borrow().native_view;
        let style: i64 = unsafe {
            let traits: id = msg_send![view, traitCollection];
            msg_send![traits, userInterfaceStyle]
        };
        if style == 1 {
            WindowAppearance::Light
        } else {
            WindowAppearance::Dark
        }
    }

    fn display(&self) -> Option<Rc<dyn PlatformDisplay>> {
        Some(Rc::new(IosDisplay))
    }

    fn mouse_position(&self) -> Point<Pixels> {
        self.0.borrow_mut().last_touch
    }

    fn modifiers(&self) -> Modifiers {
        self.0.borrow().keyboard.modifiers
    }

    fn capslock(&self) -> Capslock {
        self.0.borrow().keyboard.capslock
    }

    fn set_input_handler(&mut self, input_handler: PlatformInputHandler) {
        self.0.borrow_mut().input_handler = Some(input_handler);
    }

    fn take_input_handler(&mut self) -> Option<PlatformInputHandler> {
        self.0.borrow_mut().input_handler.take()
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
        self.0.borrow().active
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
        self.0.borrow_mut().request_frame_callback = Some(callback);
    }

    fn on_input(&self, callback: Box<dyn FnMut(PlatformInput) -> DispatchEventResult>) {
        self.0.borrow_mut().event_callback = Some(callback);
    }

    fn on_active_status_change(&self, callback: Box<dyn FnMut(bool)>) {
        self.0.borrow_mut().active_callback = Some(callback);
    }

    fn on_hover_status_change(&self, _callback: Box<dyn FnMut(bool)>) {}

    fn on_resize(&self, callback: Box<dyn FnMut(Size<Pixels>, f32)>) {
        self.0.borrow_mut().resize_callback = Some(callback);
    }

    fn on_moved(&self, _callback: Box<dyn FnMut()>) {}

    fn on_should_close(&self, _callback: Box<dyn FnMut() -> bool>) {}

    fn on_hit_test_window_control(
        &self,
        _callback: Box<dyn FnMut() -> Option<gpui::WindowControlArea>>,
    ) {
    }

    fn on_close(&self, callback: Box<dyn FnOnce()>) {
        self.0.borrow_mut().close_callback = Some(callback);
    }

    fn on_appearance_changed(&self, callback: Box<dyn FnMut()>) {
        self.0.borrow_mut().appearance_callback = Some(callback);
    }

    fn draw(&self, scene: &gpui::Scene) {
        let mut state = self.0.borrow_mut();
        state.needs_presentation = !state.renderer.draw(scene);
    }

    fn sprite_atlas(&self) -> Arc<dyn PlatformAtlas> {
        self.0.borrow_mut().renderer.sprite_atlas().clone()
    }

    fn is_subpixel_rendering_supported(&self) -> bool {
        false
    }

    fn update_ime_position(&self, _bounds: Bounds<Pixels>) {}

    fn gpu_specs(&self) -> Option<gpui::GpuSpecs> {
        Some(self.0.borrow_mut().renderer.gpu_specs())
    }
    fn a11y_init(&self, callbacks: gpui::A11yCallbacks) {
        let view = self.0.borrow_mut().native_view;
        let adapter = unsafe {
            accesskit_ios::SubclassingAdapter::new(
                view as *mut c_void,
                A11yActivationHandler(callbacks.activation),
                A11yActionHandler(callbacks.action),
                A11yDeactivationHandler(callbacks.deactivation),
            )
        };
        self.0.borrow_mut().accesskit_adapter = Some(adapter);
    }

    fn a11y_tree_update(&self, tree_update: accesskit::TreeUpdate) {
        let events = {
            let lock = self.0.borrow_mut();
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
            let view = NonNull::new_unchecked(self.0.borrow_mut().native_view as *mut c_void);
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

pub(crate) unsafe fn try_window_state(object: &Object) -> Option<Rc<RefCell<IosWindowState>>> {
    let raw: *mut c_void = *object.get_ivar(STATE_IVAR);
    (!raw.is_null()).then(|| get_window_state(object))
}

unsafe fn get_window_state(object: &Object) -> Rc<RefCell<IosWindowState>> {
    let raw: *mut c_void = *object.get_ivar(STATE_IVAR);
    let state = Rc::from_raw(raw as *const RefCell<IosWindowState>);
    let clone = state.clone();
    std::mem::forget(state);
    clone
}

fn register_view_class() {
    let mut decl = ClassDecl::new("ZZGPUIView", class!(UIView)).unwrap();
    decl.add_ivar::<*mut c_void>(STATE_IVAR);
    unsafe {
        decl.add_protocol(Protocol::get("UIKeyInput").unwrap());
        crate::text_input::register(&mut decl);
        crate::drop::register(&mut decl);
        if let Some(protocol) = Protocol::get("UIPointerInteractionDelegate") {
            decl.add_protocol(protocol);
        }
        decl.add_method(sel!(zzHover:), hover as extern "C" fn(&Object, Sel, id));
        decl.add_method(sel!(zzScroll:), scroll as extern "C" fn(&Object, Sel, id));
        decl.add_method(sel!(zzWheel:), wheel as extern "C" fn(&Object, Sel, id));
        decl.add_method(
            sel!(pointerInteraction:styleForRegion:),
            pointer_style as extern "C" fn(&Object, Sel, id, id) -> id,
        );
        decl.add_method(
            sel!(zzAppearanceChanged),
            appearance_changed as extern "C" fn(&Object, Sel),
        );
        decl.add_method(
            sel!(zzAccessibilityChanged:),
            accessibility_changed as extern "C" fn(&Object, Sel, id),
        );
        decl.add_method(
            sel!(canBecomeFirstResponder),
            can_become_first_responder as extern "C" fn(&Object, Sel) -> BOOL,
        );
        decl.add_method(
            sel!(resignFirstResponder),
            resign_first_responder as extern "C" fn(&Object, Sel) -> BOOL,
        );
        decl.add_method(
            sel!(inputView),
            input_view as extern "C" fn(&Object, Sel) -> id,
        );
        decl.add_method(
            sel!(inputAccessoryView),
            input_accessory_view as extern "C" fn(&Object, Sel) -> id,
        );
        decl.add_method(
            sel!(zzAccessoryKey:),
            accessory_key as extern "C" fn(&Object, Sel, id),
        );
        decl.add_method(
            sel!(zzHardwareKeyboardChanged:),
            hardware_keyboard_changed as extern "C" fn(&Object, Sel, id),
        );
        let traits: [(Sel, extern "C" fn(&Object, Sel) -> isize); 8] = [
            (sel!(autocorrectionType), autocorrection_type),
            (sel!(autocapitalizationType), autocapitalization_type),
            (sel!(spellCheckingType), spell_checking_type),
            (sel!(smartQuotesType), smart_punctuation_type),
            (sel!(smartDashesType), smart_punctuation_type),
            (sel!(smartInsertDeleteType), smart_punctuation_type),
            (sel!(inlinePredictionType), inline_prediction_type),
            (sel!(returnKeyType), return_key_type),
        ];
        for (selector, getter) in traits {
            decl.add_method(selector, getter);
        }
        decl.add_method(
            sel!(hasText),
            can_become_first_responder as extern "C" fn(&Object, Sel) -> BOOL,
        );
        decl.add_method(
            sel!(insertText:),
            insert_text as extern "C" fn(&Object, Sel, id),
        );
        decl.add_method(
            sel!(deleteBackward),
            delete_backward as extern "C" fn(&Object, Sel),
        );
        decl.add_method(sel!(paste:), paste as extern "C" fn(&Object, Sel, id));
        decl.add_method(sel!(copy:), copy as extern "C" fn(&Object, Sel, id));
        decl.add_method(
            sel!(selectAll:),
            select_all as extern "C" fn(&Object, Sel, id),
        );
        if let Some(protocol) = Protocol::get("UIEditMenuInteractionDelegate") {
            decl.add_protocol(protocol);
        }
        decl.add_method(
            sel!(editMenuInteraction:menuForConfiguration:suggestedActions:),
            edit_menu as extern "C" fn(&Object, Sel, id, id, id) -> id,
        );
        decl.add_method(
            sel!(canPerformAction:withSender:),
            can_perform_action as extern "C" fn(&Object, Sel, Sel, id) -> BOOL,
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
        decl.add_class_method(
            sel!(layerClass),
            layer_class as extern "C" fn(&Class, Sel) -> *const Class,
        );
        decl.add_method(sel!(zzStep:), step as extern "C" fn(&Object, Sel, id));
        decl.add_method(
            sel!(layoutSubviews),
            layout_subviews as extern "C" fn(&Object, Sel),
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
        VIEW_CLASS = decl.register();
    }
}

extern "C" fn step(this: &Object, _: Sel, _link: id) {
    advance_momentum(this);
    let state = unsafe { get_window_state(this) };
    let repeat = state.borrow_mut().keyboard.repeat(Instant::now());
    if let Some(event) = repeat {
        dispatch_event(this, PlatformInput::KeyDown(event));
    }
    let mut lock = state.borrow_mut();
    if let Some(mut callback) = lock.request_frame_callback.take() {
        let options = RequestFrameOptions {
            require_presentation: lock.needs_presentation,
            force_render: lock.renderer.needs_redraw(),
        };
        drop(lock);
        callback(options);
        state.borrow_mut().request_frame_callback = Some(callback);
    }
}

extern "C" fn appearance_changed(this: &Object, _: Sel) {
    let raw: *mut c_void = unsafe { *this.get_ivar(STATE_IVAR) };
    if raw.is_null() {
        return;
    }
    let state = unsafe { get_window_state(this) };
    let callback = state.borrow_mut().appearance_callback.take();
    if let Some(mut callback) = callback {
        callback();
        state.borrow_mut().appearance_callback = Some(callback);
    }
}

extern "C" fn accessibility_changed(this: &Object, sel: Sel, _: id) {
    appearance_changed(this, sel);
}

pub(crate) fn set_window_appearance(appearance: Option<WindowAppearance>) {
    for view in VIEWS.with_borrow(Clone::clone) {
        unsafe {
            let window: id = msg_send![view, window];
            if !window.is_null() {
                apply_window_appearance(window, appearance);
            }
        }
    }
}

fn display_scale(view: id) -> f64 {
    let scale: f64 = unsafe {
        let traits: id = msg_send![view, traitCollection];
        msg_send![traits, displayScale]
    };
    if scale > 0.0 {
        scale
    } else {
        IosDisplay::scale_factor() as f64
    }
}

fn view_for_scene(scene: id) -> Option<id> {
    VIEWS.with_borrow(|views| {
        views.iter().copied().find(|view| unsafe {
            let window: id = msg_send![*view, window];
            !window.is_null() && {
                let window_scene: id = msg_send![window, windowScene];
                window_scene == scene
            }
        })
    })
}

pub(crate) fn scene_disconnected(scene: id) {
    let Some(state) = view_for_scene(scene).and_then(|view| unsafe { try_window_state(&*view) })
    else {
        return;
    };
    let close = state.borrow_mut().close_callback.take();
    if let Some(close) = close {
        close();
    }
}

unsafe fn apply_window_appearance(window: id, appearance: Option<WindowAppearance>) {
    let style = match appearance {
        Some(WindowAppearance::Light | WindowAppearance::VibrantLight) => 1i64,
        Some(WindowAppearance::Dark | WindowAppearance::VibrantDark) => 2i64,
        None => 0i64,
    };
    let current: i64 = msg_send![window, overrideUserInterfaceStyle];
    if current == style {
        return;
    }
    let _: () = msg_send![window, setOverrideUserInterfaceStyle: style];
    let controller: id = msg_send![window, rootViewController];
    if !controller.is_null() {
        let _: () = msg_send![controller, setNeedsStatusBarAppearanceUpdate];
    }
}

extern "C" fn layout_subviews(this: &Object, _: Sel) {
    unsafe {
        let _: () = msg_send![super(this, class!(UIView)), layoutSubviews];
    }
    let Some(state) = (unsafe { try_window_state(this) }) else {
        return;
    };
    let viewport_changed = {
        let mut state = state.borrow_mut();
        let overlap = unsafe { keyboard_overlap(state.native_view, state.keyboard_probe) };
        let changed = overlap != state.keyboard_overlap;
        state.keyboard_overlap = overlap;
        changed
    };
    let callback = state.borrow_mut().insets_callback.take();
    if let Some(mut callback) = callback {
        let overlap = state.borrow().keyboard_overlap;
        callback(view_insets(this as *const Object as id, overlap));
        state.borrow_mut().insets_callback = Some(callback);
    }
    let mut lock = state.borrow_mut();
    unsafe {
        let bounds: CGRect = msg_send![lock.native_view, bounds];
        let scale = display_scale(lock.native_view);
        lock.renderer.update_drawable_size(size(
            DevicePixels((bounds.size.width * scale) as i32),
            DevicePixels((bounds.size.height * scale) as i32),
        ));
        let logical = size(px(bounds.size.width as f32), px(bounds.size.height as f32));
        if let Some(mut callback) = lock.resize_callback.take() {
            drop(lock);
            callback(logical, scale as f32);
            state.borrow_mut().resize_callback = Some(callback);
        } else {
            drop(lock);
        }
    }
    if viewport_changed {
        let callback = state.borrow_mut().viewport_callback.take();
        if let Some(mut callback) = callback {
            callback();
            state.borrow_mut().viewport_callback = Some(callback);
        }
    }
}

fn view_insets(view: id, keyboard_overlap: f64) -> gpui::WindowInsets {
    let insets: crate::UIEdgeInsets = unsafe { msg_send![view, safeAreaInsets] };
    gpui::WindowInsets {
        safe_area: gpui::Edges {
            top: px(insets.top as f32),
            right: px(insets.right as f32),
            bottom: px(insets.bottom as f32),
            left: px(insets.left as f32),
        },
        ime: gpui::Edges {
            bottom: px(keyboard_overlap as f32),
            ..Default::default()
        },
    }
}

fn touch_position(this: &Object, touches: id) -> Point<Pixels> {
    unsafe {
        let touch: id = msg_send![touches, anyObject];
        let location: CGPoint = msg_send![touch, locationInView: this as *const Object as id];
        point(px(location.x as f32), px(location.y as f32))
    }
}

fn dispatch_event(this: &Object, event: PlatformInput) -> DispatchEventResult {
    let state = unsafe { get_window_state(this) };
    let mut lock = state.borrow_mut();
    if let Some(mut callback) = lock.event_callback.take() {
        drop(lock);
        let result = callback(event);
        state.borrow_mut().event_callback = Some(callback);
        return result;
    }
    DispatchEventResult::default()
}

fn touch_count(touches: id) -> usize {
    unsafe {
        let touch: id = msg_send![touches, anyObject];
        let count: usize = msg_send![touch, tapCount];
        count.max(1)
    }
}

const UI_TOUCH_TYPE_INDIRECT_POINTER: isize = 3;

fn pointer_touch(touches: id) -> bool {
    unsafe {
        let touch: id = msg_send![touches, anyObject];
        let kind: isize = msg_send![touch, type];
        kind == UI_TOUCH_TYPE_INDIRECT_POINTER
    }
}

fn event_modifiers(event: id) -> Modifiers {
    if event.is_null() {
        return Modifiers::default();
    }
    let flags: isize = unsafe { msg_send![event, modifierFlags] };
    crate::keyboard::modifiers(flags as u64)
}

fn event_button(event: id) -> MouseButton {
    let mask: isize = if event.is_null() {
        0
    } else {
        unsafe { msg_send![event, buttonMask] }
    };
    match mask {
        mask if mask & 1 != 0 => MouseButton::Left,
        mask if mask & 2 != 0 => MouseButton::Right,
        0 => MouseButton::Left,
        _ => MouseButton::Middle,
    }
}

fn pointer_input(this: &Object, touches: id, event: id, phase: TouchPhase) {
    let position = touch_position(this, touches);
    let modifiers = event_modifiers(event);
    let state = unsafe { get_window_state(this) };
    state.borrow_mut().last_touch = position;
    let input = match phase {
        TouchPhase::Started => {
            let button = event_button(event);
            state.borrow_mut().pointer_button = Some(button);
            PlatformInput::MouseDown(MouseDownEvent {
                button,
                position,
                modifiers,
                click_count: touch_count(touches),
                first_mouse: false,
            })
        }
        TouchPhase::Moved => PlatformInput::MouseMove(MouseMoveEvent {
            position,
            pressed_button: state.borrow().pointer_button,
            modifiers,
        }),
        TouchPhase::Ended | TouchPhase::Cancelled => {
            let button = state
                .borrow_mut()
                .pointer_button
                .take()
                .unwrap_or(MouseButton::Left);
            PlatformInput::MouseUp(MouseUpEvent {
                button,
                position,
                modifiers,
                click_count: touch_count(touches),
            })
        }
    };
    dispatch_event(this, input);
}

extern "C" fn touches_began(this: &Object, _: Sel, touches: id, event: id) {
    unsafe {
        let _: BOOL = msg_send![this, becomeFirstResponder];
    }
    stop_momentum(this);
    if pointer_touch(touches) {
        pointer_input(this, touches, event, TouchPhase::Started);
        return;
    }
    if dispatch_touches(this, touches, TouchPhase::Started) {
        return;
    }
    let position = touch_position(this, touches);
    {
        let state = unsafe { get_window_state(this) };
        state.borrow_mut().last_touch = position;
    }
    let modifiers = unsafe { get_window_state(this) }
        .borrow()
        .keyboard
        .modifiers;
    dispatch_event(
        this,
        PlatformInput::MouseDown(MouseDownEvent {
            button: MouseButton::Left,
            position,
            modifiers,
            click_count: touch_count(touches),
            first_mouse: false,
        }),
    );
}

extern "C" fn touches_moved(this: &Object, _: Sel, touches: id, event: id) {
    if pointer_touch(touches) {
        pointer_input(this, touches, event, TouchPhase::Moved);
        return;
    }
    if dispatch_touches(this, touches, TouchPhase::Moved) {
        return;
    }
    let position = touch_position(this, touches);
    {
        let state = unsafe { get_window_state(this) };
        state.borrow_mut().last_touch = position;
    }
    let modifiers = unsafe { get_window_state(this) }
        .borrow()
        .keyboard
        .modifiers;
    dispatch_event(
        this,
        PlatformInput::MouseMove(MouseMoveEvent {
            position,
            pressed_button: Some(MouseButton::Left),
            modifiers,
        }),
    );
}

extern "C" fn touches_ended(this: &Object, _: Sel, touches: id, event: id) {
    if pointer_touch(touches) {
        pointer_input(this, touches, event, TouchPhase::Ended);
        return;
    }
    if dispatch_touches(this, touches, TouchPhase::Ended) {
        return;
    }
    let position = touch_position(this, touches);
    let modifiers = unsafe { get_window_state(this) }
        .borrow()
        .keyboard
        .modifiers;
    dispatch_event(
        this,
        PlatformInput::MouseUp(MouseUpEvent {
            button: MouseButton::Left,
            position,
            modifiers,
            click_count: touch_count(touches),
        }),
    );
}

extern "C" fn touches_cancelled(this: &Object, sel: Sel, touches: id, event: id) {
    if pointer_touch(touches) {
        pointer_input(this, touches, event, TouchPhase::Cancelled);
        return;
    }
    if !dispatch_touches(this, touches, TouchPhase::Cancelled) {
        touches_ended(this, sel, touches, event);
    }
}

fn dispatch_touches(this: &Object, touches: id, phase: TouchPhase) -> bool {
    let state = unsafe { get_window_state(this) };
    if !state.borrow().touch_gestures {
        return false;
    }
    for touch in press_objects(touches) {
        let location: CGPoint =
            unsafe { msg_send![touch, locationInView: this as *const Object as id] };
        let position = point(px(location.x as f32), px(location.y as f32));
        let id = {
            let mut state = state.borrow_mut();
            state.last_touch = position;
            if phase == TouchPhase::Started {
                state.next_touch += 1;
                let id = TouchId(state.next_touch);
                state.touches.insert(touch as usize, (id, position));
                id
            } else if matches!(phase, TouchPhase::Ended | TouchPhase::Cancelled) {
                let Some((id, _)) = state.touches.remove(&(touch as usize)) else {
                    continue;
                };
                id
            } else {
                let Some((id, last)) = state.touches.get_mut(&(touch as usize)) else {
                    continue;
                };
                *last = position;
                *id
            }
        };
        dispatch_event(
            this,
            PlatformInput::Touch(TouchEvent {
                id,
                phase,
                position,
                predicted_position: None,
                force: None,
            }),
        );
    }
    true
}

extern "C" fn can_become_first_responder(_: &Object, _: Sel) -> BOOL {
    YES
}

extern "C" fn input_view(this: &Object, _: Sel) -> id {
    let state = unsafe { get_window_state(this) };
    let state = state.borrow();
    if state.soft_keyboard {
        nil
    } else {
        state.input_view
    }
}

extern "C" fn input_accessory_view(this: &Object, _: Sel) -> id {
    let state = unsafe { get_window_state(this) };
    let state = state.borrow();
    if state.soft_keyboard {
        state.accessory_view
    } else {
        nil
    }
}

extern "C" fn resign_first_responder(this: &Object, _: Sel) -> BOOL {
    release_all_keys(this);
    if let Some(state) = unsafe { try_window_state(this) } {
        let mut state = state.borrow_mut();
        state.keyboard_requested = false;
        state.soft_keyboard = false;
    }
    unsafe { msg_send![super(this, class!(UIView)), resignFirstResponder] }
}

fn text_input(this: &Object) -> TextInputConfiguration {
    unsafe { try_window_state(this) }
        .map(|state| state.borrow().text_input.clone())
        .unwrap_or_default()
}

extern "C" fn autocorrection_type(this: &Object, _: Sel) -> isize {
    if text_input(this).autocorrect { 2 } else { 1 }
}

extern "C" fn autocapitalization_type(this: &Object, _: Sel) -> isize {
    match text_input(this).autocapitalize {
        gpui::Autocapitalize::None => 0,
        gpui::Autocapitalize::Words => 1,
        gpui::Autocapitalize::Sentences => 2,
        gpui::Autocapitalize::Characters => 3,
    }
}

extern "C" fn spell_checking_type(this: &Object, _: Sel) -> isize {
    if text_input(this).suggestions { 2 } else { 1 }
}

extern "C" fn smart_punctuation_type(this: &Object, _: Sel) -> isize {
    if text_input(this).autocorrect { 0 } else { 1 }
}

extern "C" fn inline_prediction_type(this: &Object, _: Sel) -> isize {
    if text_input(this).suggestions { 0 } else { 1 }
}

extern "C" fn return_key_type(this: &Object, _: Sel) -> isize {
    match text_input(this).input_action {
        TextInputAction::Go => 1,
        TextInputAction::Next => 4,
        TextInputAction::Search => 6,
        TextInputAction::Send => 7,
        TextInputAction::Done => 9,
        TextInputAction::Unspecified | TextInputAction::Enter | TextInputAction::Previous => 0,
    }
}

fn press_objects(presses: id) -> Vec<id> {
    unsafe {
        let array: id = msg_send![presses, allObjects];
        let count: usize = msg_send![array, count];
        (0..count)
            .map(|index| msg_send![array, objectAtIndex: index])
            .collect()
    }
}

extern "C" fn presses_began(this: &Object, _: Sel, presses: id, event: id) {
    if crate::text_input::composing(this) {
        unsafe {
            let _: () =
                msg_send![super(this, class!(UIView)), pressesBegan: presses withEvent: event];
        }
        return;
    }
    let state = unsafe { get_window_state(this) };
    unsafe {
        let unhandled: id = msg_send![class!(NSMutableSet), set];
        for press in press_objects(presses) {
            let key: id = msg_send![press, key];
            if key.is_null() {
                let _: () = msg_send![unhandled, addObject: press];
                continue;
            }
            let hid: usize = msg_send![key, keyCode];
            let flags: u64 = msg_send![key, modifierFlags];
            let characters: id = msg_send![key, characters];
            let ignoring: id = msg_send![key, charactersIgnoringModifiers];
            let characters = crate::nsstring_to_string(characters).unwrap_or_default();
            let ignoring = crate::nsstring_to_string(ignoring).unwrap_or_default();
            let input =
                state
                    .borrow_mut()
                    .keyboard
                    .press(hid as u16, &characters, &ignoring, flags);
            dispatch_modifiers(this);
            if let Some(input) = input {
                let is_held = input.is_held;
                let result = dispatch_event(this, PlatformInput::KeyDown(input));
                if !result.propagate || result.default_prevented {
                    state
                        .borrow_mut()
                        .keyboard
                        .consumed(hid as u16, is_held, Instant::now());
                    continue;
                }
            } else if crate::keyboard::is_modifier(hid as u16) {
                continue;
            }
            let _: () = msg_send![unhandled, addObject: press];
        }
        let count: usize = msg_send![unhandled, count];
        if count > 0 {
            let _: () =
                msg_send![super(this, class!(UIView)), pressesBegan: unhandled withEvent: event];
        }
    }
}

fn release_presses(this: &Object, presses: id) {
    let state = unsafe { get_window_state(this) };
    for press in press_objects(presses) {
        unsafe {
            let key: id = msg_send![press, key];
            if key.is_null() {
                continue;
            }
            let hid: usize = msg_send![key, keyCode];
            let flags: u64 = msg_send![key, modifierFlags];
            let keystroke = state.borrow_mut().keyboard.release(hid as u16, flags);
            if let Some(keystroke) = keystroke {
                dispatch_event(this, PlatformInput::KeyUp(KeyUpEvent { keystroke }));
            }
        }
    }
    dispatch_modifiers(this);
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

fn release_all_keys(this: &Object) {
    let raw: *mut c_void = unsafe { *this.get_ivar(STATE_IVAR) };
    if raw.is_null() {
        return;
    }
    let keys = unsafe { get_window_state(this) }
        .borrow_mut()
        .keyboard
        .clear();
    for keystroke in keys {
        dispatch_event(this, PlatformInput::KeyUp(KeyUpEvent { keystroke }));
    }
    dispatch_modifiers(this);
}

fn dispatch_modifiers(this: &Object) {
    let state = unsafe { get_window_state(this) };
    let event = {
        let state = state.borrow();
        ModifiersChangedEvent {
            modifiers: state.keyboard.modifiers,
            capslock: state.keyboard.capslock,
        }
    };
    dispatch_event(this, PlatformInput::ModifiersChanged(event));
}

extern "C" fn insert_text(this: &Object, _: Sel, text: id) {
    let Some(text) = (unsafe { crate::nsstring_to_string(text) }) else {
        return;
    };
    let latched = unsafe { get_window_state(this) }.borrow().latched;
    let mut chars = text.chars();
    if latched != Modifiers::default()
        && let (Some(character), None) = (chars.next(), chars.next())
    {
        match character {
            '\n' | '\r' => synthesize_key(this, "enter"),
            '\t' => synthesize_key(this, "tab"),
            ' ' => synthesize_key(this, "space"),
            character => {
                let extra = Modifiers {
                    shift: character.is_uppercase(),
                    ..Default::default()
                };
                synthesize_keystroke(this, character.to_lowercase().to_string(), extra);
            }
        }
        return;
    }
    match text.as_str() {
        "\n" | "\r" => synthesize_key(this, "enter"),
        "\t" => synthesize_key(this, "tab"),
        _ => {
            let state = unsafe { get_window_state(this) };
            let handler = state.borrow_mut().input_handler.take();
            if let Some(mut handler) = handler {
                handler.replace_text_in_range(None, &text);
                state.borrow_mut().input_handler = Some(handler);
            }
        }
    }
}

extern "C" fn delete_backward(this: &Object, _: Sel) {
    synthesize_key(this, "backspace");
}

extern "C" fn can_perform_action(this: &Object, _: Sel, action: Sel, sender: id) -> BOOL {
    unsafe {
        if action == sel!(paste:) {
            let pasteboard: id = msg_send![class!(UIPasteboard), generalPasteboard];
            msg_send![pasteboard, hasStrings]
        } else if action == sel!(copy:) || action == sel!(selectAll:) {
            YES
        } else {
            msg_send![super(this, class!(UIView)), canPerformAction: action withSender: sender]
        }
    }
}

extern "C" fn paste(this: &Object, _: Sel, _: id) {
    let text = unsafe {
        let pasteboard: id = msg_send![class!(UIPasteboard), generalPasteboard];
        let string: id = msg_send![pasteboard, string];
        crate::nsstring_to_string(string)
    };
    if let Some(text) = text {
        let state = unsafe { get_window_state(this) };
        let handler = state.borrow_mut().input_handler.take();
        if let Some(mut handler) = handler {
            handler.paste(gpui::ClipboardItem::new_string(text));
            state.borrow_mut().input_handler = Some(handler);
        }
    }
}

extern "C" fn copy(this: &Object, _: Sel, _: id) {
    synthesize_keystroke(this, "c".into(), Modifiers::command());
}

extern "C" fn select_all(this: &Object, _: Sel, _: id) {
    synthesize_keystroke(this, "a".into(), Modifiers::command());
}

extern "C" fn edit_menu(_: &Object, _: Sel, _: id, _: id, suggested: id) -> id {
    unsafe {
        let elements = menu_elements(suggested)
            .into_iter()
            .filter(|element| {
                menu_identifier(*element).as_deref() == Some("com.apple.menu.standard-edit")
            })
            .map(|group| {
                let children: id = msg_send![group, children];
                let children: Vec<id> = menu_elements(children)
                    .into_iter()
                    .filter(|child| {
                        menu_identifier(*child).as_deref() != Some("com.apple.menu.autofill")
                    })
                    .collect();
                msg_send![group, menuByReplacingChildren: ns_array(&children)]
            })
            .collect::<Vec<id>>();
        msg_send![class!(UIMenu), menuWithChildren: ns_array(&elements)]
    }
}

unsafe fn menu_elements(array: id) -> Vec<id> {
    let count: usize = msg_send![array, count];
    (0..count)
        .map(|index| msg_send![array, objectAtIndex: index])
        .collect()
}

unsafe fn menu_identifier(element: id) -> Option<String> {
    let is_menu: BOOL = msg_send![element, isKindOfClass: class!(UIMenu)];
    if is_menu != YES {
        return None;
    }
    let identifier: id = msg_send![element, identifier];
    crate::nsstring_to_string(identifier)
}

pub fn show_edit_menu(x: f32, y: f32) {
    let view = ACTIVE_VIEW.get();
    let Some(state) = (!view.is_null())
        .then(|| unsafe { try_window_state(&*view) })
        .flatten()
    else {
        return;
    };
    let edit_menu = state.borrow().edit_menu;
    unsafe {
        let point = CGPoint {
            x: x as f64,
            y: y as f64,
        };
        let configuration: id = msg_send![
            class!(UIEditMenuConfiguration),
            configurationWithIdentifier: nil
            sourcePoint: point
        ];
        let _: () = msg_send![edit_menu, presentEditMenuWithConfiguration: configuration];
    }
}

pub fn request_paste() {
    unsafe {
        dispatch2::DispatchQueue::main().exec_async_f(ptr::null_mut(), deferred_paste);
    }
}

extern "C" fn deferred_paste(_: *mut c_void) {
    let view = ACTIVE_VIEW.get();
    if !view.is_null() {
        unsafe {
            let app: id = msg_send![class!(UIApplication), sharedApplication];
            let _: BOOL =
                msg_send![app, sendAction: sel!(paste:) to: view from: view forEvent: nil];
        }
    }
}

fn synthesize_key(this: &Object, key: &str) {
    synthesize_keystroke(this, key.to_owned(), Modifiers::default());
}

fn synthesize_keystroke(this: &Object, key: String, extra: Modifiers) {
    let latched = take_latch(this);
    let held = unsafe { get_window_state(this) }
        .borrow()
        .keyboard
        .modifiers;
    let keystroke = Keystroke {
        modifiers: Modifiers {
            control: held.control || latched.control || extra.control,
            alt: held.alt || latched.alt || extra.alt,
            shift: held.shift || latched.shift || extra.shift,
            platform: held.platform || latched.platform || extra.platform,
            function: held.function || latched.function || extra.function,
        },
        key,
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

pub(crate) fn scene_active_changed(scene: id, active: bool) {
    let Some(view) = view_for_scene(scene) else {
        return;
    };
    if active {
        ACTIVE_VIEW.set(view);
    }
    unsafe {
        if !active {
            release_all_keys(&*view);
            let touches = std::mem::take(&mut get_window_state(&*view).borrow_mut().touches);
            for (_, (id, position)) in touches {
                dispatch_event(
                    &*view,
                    PlatformInput::Touch(TouchEvent {
                        id,
                        phase: TouchPhase::Cancelled,
                        position,
                        predicted_position: None,
                        force: None,
                    }),
                );
            }
        }
        let state = get_window_state(&*view);
        let callback = {
            let mut state = state.borrow_mut();
            state.active = active;
            state.active_callback.take()
        };
        if let Some(mut callback) = callback {
            callback(active);
            state.borrow_mut().active_callback = Some(callback);
        }
        if active {
            let _: BOOL = msg_send![view, becomeFirstResponder];
        }
    }
}

#[derive(Debug)]
struct IosDisplayHandle;

impl rwh::HasDisplayHandle for IosDisplayHandle {
    fn display_handle(&self) -> Result<rwh::DisplayHandle<'_>, rwh::HandleError> {
        Ok(rwh::DisplayHandle::uikit())
    }
}

#[link(name = "Foundation", kind = "framework")]
unsafe extern "C" {
    static NSRunLoopCommonModes: id;
}

#[link(name = "UIKit", kind = "framework")]
unsafe extern "C" {
    static UIAccessibilityReduceMotionStatusDidChangeNotification: id;
    fn UIAccessibilityIsReduceMotionEnabled() -> BOOL;
    fn UIAccessibilityDarkerSystemColorsEnabled() -> BOOL;
}

#[derive(Clone, Copy, Debug)]
pub struct Accessibility {
    pub reduce_motion: bool,
    pub increase_contrast: bool,
    pub text_scale: f32,
}

pub fn accessibility() -> Accessibility {
    unsafe {
        let application: id = msg_send![class!(UIApplication), sharedApplication];
        let category: id = msg_send![application, preferredContentSizeCategory];
        let category = crate::nsstring_to_string(category).unwrap_or_default();
        Accessibility {
            reduce_motion: UIAccessibilityIsReduceMotionEnabled() == YES,
            increase_contrast: UIAccessibilityDarkerSystemColorsEnabled() == YES,
            text_scale: match category.trim_start_matches("UICTContentSizeCategory") {
                "XS" => 0.82,
                "S" => 0.88,
                "M" => 0.94,
                "XL" => 1.12,
                "XXL" => 1.24,
                "XXXL" => 1.35,
                "AccessibilityM" => 1.5,
                "AccessibilityL" => 1.6,
                "AccessibilityXL" => 1.7,
                "AccessibilityXXL" => 1.8,
                "AccessibilityXXXL" => 1.9,
                _ => 1.0,
            },
        }
    }
}

thread_local! {
    static VIEWS: RefCell<Vec<id>> = const { RefCell::new(Vec::new()) };
    static ACTIVE_VIEW: std::cell::Cell<id> = const { std::cell::Cell::new(ptr::null_mut()) };
}

pub(crate) fn is_live_view(view: id) -> bool {
    VIEWS.with_borrow(|views| views.contains(&view))
}

pub(crate) fn active_window() -> Option<AnyWindowHandle> {
    let view = ACTIVE_VIEW.get();
    if view.is_null() {
        return None;
    }
    unsafe { try_window_state(&*view) }.map(|state| state.borrow()._handle)
}

static CURSOR_STYLE: std::sync::Mutex<CursorStyle> = std::sync::Mutex::new(CursorStyle::Arrow);

unsafe fn install_pointer_input(view: id) -> id {
    unsafe {
        let hover: id = msg_send![class!(UIHoverGestureRecognizer), alloc];
        let hover: id = msg_send![hover, initWithTarget: view action: sel!(zzHover:)];
        let _: () = msg_send![hover, setCancelsTouchesInView: false as BOOL];
        let _: () = msg_send![view, addGestureRecognizer: hover];
        let _: () = msg_send![hover, release];

        for (action, mask) in [
            (sel!(zzWheel:), UI_SCROLL_TYPE_MASK_DISCRETE),
            (sel!(zzScroll:), UI_SCROLL_TYPE_MASK_CONTINUOUS),
        ] {
            let scroll: id = msg_send![class!(UIPanGestureRecognizer), alloc];
            let scroll: id = msg_send![scroll, initWithTarget: view action: action];
            let _: () = msg_send![scroll, setAllowedScrollTypesMask: mask];
            let no_touches: id = msg_send![class!(NSArray), array];
            let _: () = msg_send![scroll, setAllowedTouchTypes: no_touches];
            let _: () = msg_send![scroll, setCancelsTouchesInView: false as BOOL];
            let _: () = msg_send![view, addGestureRecognizer: scroll];
            let _: () = msg_send![scroll, release];
        }

        let interaction: id = msg_send![class!(UIPointerInteraction), alloc];
        let interaction: id = msg_send![interaction, initWithDelegate: view];
        let _: () = msg_send![view, addInteraction: interaction];
        let _: () = msg_send![interaction, release];
        interaction
    }
}

pub(crate) fn set_cursor_style(style: CursorStyle) {
    let mut current = CURSOR_STYLE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if *current == style {
        return;
    }
    *current = style;
    drop(current);
    for view in VIEWS.with_borrow(Clone::clone) {
        if let Some(state) = unsafe { try_window_state(&*view) } {
            let interaction = state.borrow().pointer_interaction;
            if !interaction.is_null() {
                unsafe {
                    let _: () = msg_send![interaction, invalidate];
                }
            }
        }
    }
}

extern "C" fn pointer_style(_: &Object, _: Sel, _interaction: id, _region: id) -> id {
    let style = *CURSOR_STYLE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    unsafe {
        let beam = |axis: usize| -> id {
            let shape: id =
                msg_send![class!(UIPointerShape), beamWithPreferredLength: 20.0f64 axis: axis];
            msg_send![class!(UIPointerStyle), styleWithShape: shape constrainedAxes: 0usize]
        };
        match style {
            CursorStyle::IBeam => beam(2),
            CursorStyle::IBeamCursorForVerticalLayout => beam(1),
            _ => nil,
        }
    }
}

fn recognizer_point(this: &Object, recognizer: id) -> Point<Pixels> {
    let location: CGPoint =
        unsafe { msg_send![recognizer, locationInView: this as *const Object as id] };
    point(px(location.x as f32), px(location.y as f32))
}

extern "C" fn hover(this: &Object, _: Sel, recognizer: id) {
    let phase: isize = unsafe { msg_send![recognizer, state] };
    let position = recognizer_point(this, recognizer);
    let modifiers = event_modifiers(recognizer);
    unsafe { get_window_state(this) }.borrow_mut().last_touch = position;
    let input = match phase {
        1 | 2 => PlatformInput::MouseMove(MouseMoveEvent {
            position,
            pressed_button: None,
            modifiers,
        }),
        3..=5 => PlatformInput::MouseExited(MouseExitEvent {
            position,
            pressed_button: None,
            modifiers,
        }),
        _ => return,
    };
    dispatch_event(this, input);
}

const UI_SCROLL_TYPE_MASK_DISCRETE: isize = 1;
const UI_SCROLL_TYPE_MASK_CONTINUOUS: isize = 2;

extern "C" fn scroll(this: &Object, _: Sel, recognizer: id) {
    pan_scroll(this, recognizer, true);
}

extern "C" fn wheel(this: &Object, _: Sel, recognizer: id) {
    pan_scroll(this, recognizer, false);
}

fn pan_scroll(this: &Object, recognizer: id, coasts: bool) {
    let phase: isize = unsafe { msg_send![recognizer, state] };
    let touch_phase = match phase {
        1 => TouchPhase::Started,
        2 => TouchPhase::Moved,
        3 | 4 => TouchPhase::Ended,
        _ => return,
    };
    if touch_phase == TouchPhase::Started {
        stop_momentum(this);
    }
    let view = this as *const Object as id;
    let translation: CGPoint = unsafe { msg_send![recognizer, translationInView: view] };
    unsafe {
        let _: () = msg_send![recognizer, setTranslation: CGPoint { x: 0.0, y: 0.0 } inView: view];
    }
    let touch_phase = if coasts {
        touch_phase
    } else if translation.x == 0.0 && translation.y == 0.0 {
        return;
    } else {
        TouchPhase::Moved
    };
    let position = recognizer_point(this, recognizer);
    unsafe { get_window_state(this) }.borrow_mut().last_touch = position;
    dispatch_event(
        this,
        PlatformInput::ScrollWheel(ScrollWheelEvent {
            position,
            delta: ScrollDelta::Pixels(point(px(translation.x as f32), px(translation.y as f32))),
            modifiers: event_modifiers(recognizer),
            touch_phase,
        }),
    );
    if coasts && phase == 3 {
        let velocity: CGPoint = unsafe { msg_send![recognizer, velocityInView: view] };
        let momentum = crate::momentum::Momentum::fling(
            position,
            point(velocity.x, velocity.y),
            Instant::now(),
        );
        unsafe { get_window_state(this) }.borrow_mut().momentum = momentum;
    }
}

fn advance_momentum(this: &Object) {
    let state = unsafe { get_window_state(this) };
    let (position, delta, done, modifiers) = {
        let mut state = state.borrow_mut();
        let modifiers = state.keyboard.modifiers;
        let Some(momentum) = state.momentum.as_mut() else {
            return;
        };
        let (delta, done) = momentum.step(Instant::now());
        let position = momentum.position;
        if done {
            state.momentum = None;
        }
        (position, delta, done, modifiers)
    };
    dispatch_event(
        this,
        PlatformInput::ScrollWheel(ScrollWheelEvent {
            position,
            delta: ScrollDelta::Pixels(delta),
            modifiers,
            touch_phase: if done {
                TouchPhase::Ended
            } else {
                TouchPhase::Moved
            },
        }),
    );
}

fn stop_momentum(this: &Object) {
    let Some(momentum) = unsafe { get_window_state(this) }
        .borrow_mut()
        .momentum
        .take()
    else {
        return;
    };
    dispatch_event(
        this,
        PlatformInput::ScrollWheel(ScrollWheelEvent {
            position: momentum.position,
            delta: ScrollDelta::Pixels(Point::default()),
            modifiers: Modifiers::default(),
            touch_phase: TouchPhase::Ended,
        }),
    );
}

extern "C" fn layer_class(_: &Class, _: Sel) -> *const Class {
    class!(CAMetalLayer)
}

pub(crate) fn scene_geometry_changed(scene: id) {
    if let Some(view) = view_for_scene(scene) {
        unsafe {
            let _: () = msg_send![view, setNeedsLayout];
        }
    }
}

#[derive(Debug, Clone)]
struct IosRawWindow(usize);

impl rwh::HasWindowHandle for IosRawWindow {
    fn window_handle(&self) -> Result<rwh::WindowHandle<'_>, rwh::HandleError> {
        let view = NonNull::new(self.0 as *mut c_void).ok_or(rwh::HandleError::Unavailable)?;
        unsafe {
            Ok(rwh::WindowHandle::borrow_raw(rwh::RawWindowHandle::UiKit(
                rwh::UiKitWindowHandle::new(view),
            )))
        }
    }
}

impl rwh::HasDisplayHandle for IosRawWindow {
    fn display_handle(&self) -> Result<rwh::DisplayHandle<'_>, rwh::HandleError> {
        Ok(rwh::DisplayHandle::uikit())
    }
}

#[link(name = "GameController", kind = "framework")]
unsafe extern "C" {
    static GCKeyboardDidConnectNotification: id;
    static GCKeyboardDidDisconnectNotification: id;
}

#[derive(Clone, Copy)]
enum AccessoryKey {
    Key(&'static str),
    Control,
    Alt,
    Hide,
}

const ACCESSORY_TAG: isize = 1000;

const ACCESSORY_KEYS: [(&str, bool, &str, AccessoryKey); 9] = [
    ("esc", false, "Escape", AccessoryKey::Key("escape")),
    ("tab", false, "Tab", AccessoryKey::Key("tab")),
    ("ctrl", false, "Control", AccessoryKey::Control),
    ("alt", false, "Option", AccessoryKey::Alt),
    ("arrow.left", true, "Left", AccessoryKey::Key("left")),
    ("arrow.down", true, "Down", AccessoryKey::Key("down")),
    ("arrow.up", true, "Up", AccessoryKey::Key("up")),
    ("arrow.right", true, "Right", AccessoryKey::Key("right")),
    (
        "keyboard.chevron.compact.down",
        true,
        "Hide keyboard",
        AccessoryKey::Hide,
    ),
];

unsafe fn install_keyboard_probe(view: id) -> id {
    let probe: id = msg_send![class!(UIView), new];
    let _: () = msg_send![probe, setHidden: YES];
    let _: () = msg_send![probe, setUserInteractionEnabled: false as BOOL];
    let _: () = msg_send![probe, setTranslatesAutoresizingMaskIntoConstraints: false as BOOL];
    let _: () = msg_send![view, addSubview: probe];
    let guide: id = msg_send![view, keyboardLayoutGuide];
    let _: () = msg_send![guide, setUsesBottomSafeArea: false as BOOL];
    let probe_top: id = msg_send![probe, topAnchor];
    let probe_bottom: id = msg_send![probe, bottomAnchor];
    let probe_leading: id = msg_send![probe, leadingAnchor];
    let probe_width: id = msg_send![probe, widthAnchor];
    let guide_top: id = msg_send![guide, topAnchor];
    let view_bottom: id = msg_send![view, bottomAnchor];
    let view_leading: id = msg_send![view, leadingAnchor];
    let constraints: [id; 4] = [
        msg_send![probe_top, constraintEqualToAnchor: guide_top],
        msg_send![probe_bottom, constraintEqualToAnchor: view_bottom],
        msg_send![probe_leading, constraintEqualToAnchor: view_leading],
        msg_send![probe_width, constraintEqualToConstant: 0.0f64],
    ];
    let _: () = msg_send![class!(NSLayoutConstraint), activateConstraints: ns_array(&constraints)];
    probe
}

unsafe fn keyboard_overlap(view: id, probe: id) -> f64 {
    let bounds: CGRect = msg_send![view, bounds];
    let frame: CGRect = msg_send![probe, frame];
    if (frame.origin.y + frame.size.height - bounds.size.height).abs() < 1.0 {
        frame.size.height.clamp(0.0, bounds.size.height)
    } else {
        0.0
    }
}

unsafe fn accessory_view(view: id) -> id {
    let frame = CGRect {
        origin: CGPoint::default(),
        size: crate::CGSize {
            width: 0.0,
            height: 52.0,
        },
    };
    let container: id = msg_send![class!(UIInputView), alloc];
    let container: id = msg_send![container, initWithFrame: frame inputViewStyle: 1isize];
    let _: () = msg_send![container, setAutoresizingMask: 2usize];
    let glass: BOOL = msg_send![
        class!(UIButtonConfiguration),
        respondsToSelector: sel!(glassButtonConfiguration)
    ];
    let mut arranged = Vec::new();
    for (index, (label, symbol, name, key)) in ACCESSORY_KEYS.into_iter().enumerate() {
        let configuration: id = if glass == YES {
            msg_send![class!(UIButtonConfiguration), glassButtonConfiguration]
        } else {
            msg_send![class!(UIButtonConfiguration), grayButtonConfiguration]
        };
        if symbol {
            let image: id = msg_send![class!(UIImage), systemImageNamed: crate::ns_string(label)];
            let _: () = msg_send![configuration, setImage: image];
        } else {
            let _: () = msg_send![configuration, setTitle: crate::ns_string(label)];
        }
        let button: id = msg_send![
            class!(UIButton),
            buttonWithConfiguration: configuration
            primaryAction: nil
        ];
        let _: () = msg_send![button, setTag: ACCESSORY_TAG + index as isize];
        let _: () = msg_send![button, setAccessibilityLabel: crate::ns_string(name)];
        if matches!(key, AccessoryKey::Control | AccessoryKey::Alt) {
            let _: () = msg_send![button, setChangesSelectionAsPrimaryAction: YES];
        }
        let _: () = msg_send![
            button,
            addTarget: view
            action: sel!(zzAccessoryKey:)
            forControlEvents: 1usize << 6
        ];
        if matches!(key, AccessoryKey::Hide) {
            let spacer: id = msg_send![class!(UIView), new];
            let _: () = msg_send![spacer, setContentHuggingPriority: 1.0f32 forAxis: 0isize];
            arranged.push(spacer);
        }
        arranged.push(button);
    }
    let stack: id = msg_send![class!(UIStackView), alloc];
    let stack: id = msg_send![stack, initWithArrangedSubviews: ns_array(&arranged)];
    let _: () = msg_send![stack, setAxis: 0isize];
    let _: () = msg_send![stack, setSpacing: 8.0f64];
    let _: () = msg_send![stack, setAlignment: 3isize];
    let _: () = msg_send![stack, setTranslatesAutoresizingMaskIntoConstraints: false as BOOL];
    let _: () = msg_send![container, addSubview: stack];
    let margins: id = msg_send![container, layoutMarginsGuide];
    let stack_leading: id = msg_send![stack, leadingAnchor];
    let stack_trailing: id = msg_send![stack, trailingAnchor];
    let stack_center: id = msg_send![stack, centerYAnchor];
    let margins_leading: id = msg_send![margins, leadingAnchor];
    let margins_trailing: id = msg_send![margins, trailingAnchor];
    let container_center: id = msg_send![container, centerYAnchor];
    let constraints: [id; 3] = [
        msg_send![stack_leading, constraintEqualToAnchor: margins_leading],
        msg_send![stack_trailing, constraintEqualToAnchor: margins_trailing],
        msg_send![stack_center, constraintEqualToAnchor: container_center],
    ];
    let _: () = msg_send![class!(NSLayoutConstraint), activateConstraints: ns_array(&constraints)];
    for view in arranged.iter().filter(|view| {
        let tag: isize = msg_send![**view, tag];
        tag < ACCESSORY_TAG
    }) {
        let _: () = msg_send![*view, release];
    }
    let _: () = msg_send![stack, release];
    container
}

extern "C" fn accessory_key(this: &Object, _: Sel, sender: id) {
    let tag: isize = unsafe { msg_send![sender, tag] };
    let Some((.., key)) = ACCESSORY_KEYS.get((tag - ACCESSORY_TAG) as usize) else {
        return;
    };
    match *key {
        AccessoryKey::Key(key) => synthesize_key(this, key),
        AccessoryKey::Control | AccessoryKey::Alt => {
            let selected: BOOL = unsafe { msg_send![sender, isSelected] };
            let state = unsafe { get_window_state(this) };
            let mut state = state.borrow_mut();
            if matches!(key, AccessoryKey::Control) {
                state.latched.control = selected == YES;
            } else {
                state.latched.alt = selected == YES;
            }
        }
        AccessoryKey::Hide => request_keyboard(this as *const Object as id, false),
    }
}

fn take_latch(this: &Object) -> Modifiers {
    let state = unsafe { get_window_state(this) };
    let (latched, accessory) = {
        let mut state = state.borrow_mut();
        (std::mem::take(&mut state.latched), state.accessory_view)
    };
    if latched != Modifiers::default() {
        for (index, (.., key)) in ACCESSORY_KEYS.iter().enumerate() {
            if matches!(key, AccessoryKey::Control | AccessoryKey::Alt) {
                unsafe {
                    let button: id =
                        msg_send![accessory, viewWithTag: ACCESSORY_TAG + index as isize];
                    if !button.is_null() {
                        let _: () = msg_send![button, setSelected: false as BOOL];
                    }
                }
            }
        }
    }
    latched
}

fn hardware_keyboard() -> bool {
    unsafe {
        let keyboard: id = msg_send![class!(GCKeyboard), coalescedKeyboard];
        !keyboard.is_null()
    }
}

unsafe fn observe_hardware_keyboard(view: id) {
    let center: id = msg_send![class!(NSNotificationCenter), defaultCenter];
    for name in [
        GCKeyboardDidConnectNotification,
        GCKeyboardDidDisconnectNotification,
    ] {
        let _: () = msg_send![
            center,
            addObserver: view
            selector: sel!(zzHardwareKeyboardChanged:)
            name: name
            object: nil
        ];
    }
}

extern "C" fn hardware_keyboard_changed(this: &Object, _: Sel, _: id) {
    schedule_keyboard_sync(this as *const Object as id);
}

fn request_keyboard(view: id, visible: bool) {
    if let Some(state) = unsafe { try_window_state(&*view) } {
        state.borrow_mut().keyboard_requested = visible;
        schedule_keyboard_sync(view);
    }
}

fn schedule_keyboard_sync(view: id) {
    let Some(state) = (unsafe { try_window_state(&*view) }) else {
        return;
    };
    if std::mem::replace(&mut state.borrow_mut().keyboard_sync_pending, true) {
        return;
    }
    unsafe {
        let _: id = msg_send![view, retain];
        dispatch2::DispatchQueue::main().exec_async_f(view.cast(), sync_keyboard);
    }
}

extern "C" fn sync_keyboard(context: *mut c_void) {
    let view = context as id;
    unsafe {
        if let Some(state) = try_window_state(&*view) {
            let (visible, reload) = {
                let mut state = state.borrow_mut();
                state.keyboard_sync_pending = false;
                let visible = state.keyboard_requested && !hardware_keyboard();
                let reload = state.soft_keyboard != visible
                    || std::mem::take(&mut state.reload_input_views)
                    || (visible && state.keyboard_overlap == 0.0);
                state.soft_keyboard = visible;
                (visible, reload)
            };
            if reload {
                let first_responder: BOOL = msg_send![view, isFirstResponder];
                if first_responder == YES {
                    let _: () = msg_send![view, reloadInputViews];
                } else if visible {
                    let _: BOOL = msg_send![view, becomeFirstResponder];
                }
            }
        }
        let _: () = msg_send![view, release];
    }
}
