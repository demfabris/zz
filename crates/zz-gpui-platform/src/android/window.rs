use crate::android::{
    bridge::{self, Configuration},
    display::{self, AndroidDisplay, FrameLink},
    keyboard::Keyboard,
    platform, text_input,
};
use crate::ios::pinch::Pinch;
use crate::wgpu::{GpuContext, WgpuContext, WgpuRenderer, WgpuSurfaceConfig, wgpu};
use futures::channel::oneshot;
use ndk_sys::{
    ANativeWindow, ANativeWindow_getHeight, ANativeWindow_getWidth, ANativeWindow_release,
    ANativeWindow_setFrameRate,
};
use raw_window_handle as rwh;
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    ffi::c_void,
    ptr::NonNull,
    rc::{Rc, Weak},
    sync::Arc,
    time::{Duration, Instant},
};
use zz_gpui::{
    AnyWindowHandle, Bounds, Capslock, DevicePixels, DispatchEventResult, KeyDownEvent, KeyUpEvent,
    Keystroke, Modifiers, ModifiersChangedEvent, MouseButton, MouseDownEvent, MouseExitEvent,
    MouseMoveEvent, MouseUpEvent, PinchEvent, Pixels, PlatformAtlas, PlatformDisplay,
    PlatformInput, PlatformInputHandler, PlatformWindow, Point, PromptButton, PromptLevel,
    RequestFrameOptions, ScrollDelta, ScrollWheelEvent, Size, TextInputAction,
    TextInputConfiguration, TextInputMode, TextInputStateChange, TouchEvent, TouchId, TouchPhase,
    WindowAppearance, WindowBackgroundAppearance, WindowBounds, WindowParams, point, px, size,
};

const ACTION_DOWN: i32 = 0;
const ACTION_UP: i32 = 1;
const ACTION_MOVE: i32 = 2;
const ACTION_CANCEL: i32 = 3;
const ACTION_POINTER_DOWN: i32 = 5;
const ACTION_POINTER_UP: i32 = 6;
const ACTION_HOVER_MOVE: i32 = 7;
const ACTION_SCROLL: i32 = 8;
const ACTION_HOVER_ENTER: i32 = 9;
const ACTION_HOVER_EXIT: i32 = 10;
const TOOL_TYPE_MOUSE: i32 = 3;
const BUTTON_SECONDARY: i32 = 2;
const BUTTON_TERTIARY: i32 = 4;
const KEY_ACTION_DOWN: i32 = 0;
const KEY_ACTION_UP: i32 = 1;
const SCROLL_LINE: f32 = 3.0;
const DOUBLE_TAP: Duration = Duration::from_millis(300);

#[derive(Clone, Copy)]
enum RowKey {
    Key(&'static str),
    Control,
    Alt,
    Hide,
}

const KEY_ROW: [RowKey; 9] = [
    RowKey::Key("escape"),
    RowKey::Key("tab"),
    RowKey::Control,
    RowKey::Alt,
    RowKey::Key("left"),
    RowKey::Key("down"),
    RowKey::Key("up"),
    RowKey::Key("right"),
    RowKey::Hide,
];

thread_local! {
    static SURFACE: Cell<Option<NonNull<ANativeWindow>>> = const { Cell::new(None) };
    static WINDOW: RefCell<Option<Weak<RefCell<AndroidWindowState>>>> = const { RefCell::new(None) };
    static NEXT_KEY: Cell<usize> = const { Cell::new(1) };
    static INSETS: Cell<([i32; 4], i32, i32)> = const { Cell::new(([0; 4], 0, 0)) };
}

pub(crate) struct Pointer {
    pub id: i32,
    pub x: f32,
    pub y: f32,
    pub tool: i32,
}

pub(crate) struct AndroidWindowState {
    handle: AnyWindowHandle,
    key: usize,
    link: Rc<FrameLink>,
    renderer: WgpuRenderer,
    gpu_context: GpuContext,
    surface_size: Size<DevicePixels>,
    needs_presentation: bool,
    request_frame_callback: Option<Box<dyn FnMut(RequestFrameOptions)>>,
    event_callback: Option<Box<dyn FnMut(PlatformInput) -> DispatchEventResult>>,
    resize_callback: Option<Box<dyn FnMut(Size<Pixels>, f32)>>,
    appearance_callback: Option<Box<dyn FnMut()>>,
    insets_callback: Option<Box<dyn FnMut(zz_gpui::WindowInsets)>>,
    viewport_callback: Option<Box<dyn FnMut()>>,
    active_callback: Option<Box<dyn FnMut(bool)>>,
    close_callback: Option<Box<dyn FnOnce()>>,
    back_callback: Option<Box<dyn FnMut()>>,
    input_handler: Option<PlatformInputHandler>,
    accesskit: Option<accesskit_android::InjectingAdapter>,
    keyboard: Keyboard,
    safe_area: [i32; 4],
    ime: i32,
    ime_rest: i32,
    pub(crate) text_input: TextInputConfiguration,
    pub(crate) latched: Modifiers,
    compact_keyboard: bool,
    key_row: bool,
    keyboard_requested: bool,
    keyboard_sync_pending: bool,
    text_focused: bool,
    active: bool,
    touches: HashMap<i32, (TouchId, Point<Pixels>)>,
    pinch: Option<Pinch>,
    next_touch: u64,
    pointer_button: Option<MouseButton>,
    last_position: Point<Pixels>,
    last_click: Option<(Instant, Point<Pixels>, usize)>,
    selection_dirty: bool,
}

impl AndroidWindowState {
    fn scale(&self) -> f32 {
        bridge::configuration().density
    }

    fn logical_size(&self) -> Size<Pixels> {
        let scale = self.scale();
        size(
            px(self.surface_size.width.0 as f32 / scale),
            px(self.surface_size.height.0 as f32 / scale),
        )
    }

    fn position(&self, x: f32, y: f32) -> Point<Pixels> {
        let scale = self.scale();
        point(px(x / scale), px(y / scale))
    }

    fn viewport_above(&self, keyboard: i32) -> Bounds<Pixels> {
        let mut size = self.logical_size();
        size.height = (size.height - px(keyboard as f32 / self.scale())).max(Pixels::ZERO);
        Bounds {
            origin: Point::default(),
            size,
        }
    }

    fn insets(&self) -> zz_gpui::WindowInsets {
        let scale = self.scale();
        let [top, left, bottom, right] = self.safe_area.map(|value| px(value as f32 / scale));
        zz_gpui::WindowInsets {
            safe_area: zz_gpui::Edges {
                top,
                right,
                bottom,
                left,
            },
            ime: zz_gpui::Edges {
                bottom: px(self.ime as f32 / scale),
                ..Default::default()
            },
        }
    }

    pub(crate) fn take_input_handler(&mut self) -> Option<PlatformInputHandler> {
        self.input_handler.take()
    }

    pub(crate) fn restore_input_handler(&mut self, handler: PlatformInputHandler) {
        if self.input_handler.is_none() {
            self.input_handler = Some(handler);
        }
    }
}

pub(crate) struct AndroidWindow(Rc<RefCell<AndroidWindowState>>, Rc<FrameLink>);

#[derive(Debug, Clone)]
struct AndroidRawWindow(NonNull<ANativeWindow>);

unsafe impl Send for AndroidRawWindow {}
unsafe impl Sync for AndroidRawWindow {}

impl rwh::HasWindowHandle for AndroidRawWindow {
    fn window_handle(&self) -> Result<rwh::WindowHandle<'_>, rwh::HandleError> {
        unsafe {
            Ok(rwh::WindowHandle::borrow_raw(
                rwh::RawWindowHandle::AndroidNdk(rwh::AndroidNdkWindowHandle::new(self.0.cast())),
            ))
        }
    }
}

impl rwh::HasDisplayHandle for AndroidRawWindow {
    fn display_handle(&self) -> Result<rwh::DisplayHandle<'_>, rwh::HandleError> {
        Ok(rwh::DisplayHandle::android())
    }
}

#[derive(Debug)]
struct AndroidDisplayHandle;

impl rwh::HasDisplayHandle for AndroidDisplayHandle {
    fn display_handle(&self) -> Result<rwh::DisplayHandle<'_>, rwh::HandleError> {
        Ok(rwh::DisplayHandle::android())
    }
}

fn surface_pixels(native: NonNull<ANativeWindow>) -> Size<DevicePixels> {
    unsafe {
        size(
            DevicePixels(ANativeWindow_getWidth(native.as_ptr()).max(1)),
            DevicePixels(ANativeWindow_getHeight(native.as_ptr()).max(1)),
        )
    }
}

fn prefer_refresh_rate(native: NonNull<ANativeWindow>) {
    let rate = bridge::configuration().refresh_rate.max(60.0);
    unsafe {
        ANativeWindow_setFrameRate(native.as_ptr(), rate, 0);
    }
}

fn surface_config(size: Size<DevicePixels>) -> WgpuSurfaceConfig {
    WgpuSurfaceConfig {
        size,
        transparent: false,
        preferred_present_mode: None,
    }
}

unsafe extern "C" fn frame(_time: i64, data: *mut c_void) {
    let Some(state) = current() else {
        return;
    };
    if state.borrow().key != data as usize {
        return;
    }
    step(&state, false);
}

fn current() -> Option<Rc<RefCell<AndroidWindowState>>> {
    WINDOW.with_borrow(|window| window.as_ref().and_then(Weak::upgrade))
}

pub(crate) fn with_state<R>(f: impl FnOnce(&Rc<RefCell<AndroidWindowState>>) -> R) -> Option<R> {
    current().as_ref().map(f)
}

pub(crate) fn has_window() -> bool {
    current().is_some()
}

pub(crate) fn surface() -> Option<NonNull<ANativeWindow>> {
    SURFACE.get()
}

pub(crate) fn active_window() -> Option<AnyWindowHandle> {
    current().map(|state| state.borrow().handle)
}

impl AndroidWindow {
    pub(crate) fn open(handle: AnyWindowHandle, _params: WindowParams) -> anyhow::Result<Self> {
        let native = SURFACE
            .get()
            .ok_or_else(|| anyhow::anyhow!("the activity has no surface yet"))?;
        let surface_size = surface_pixels(native);
        display::set_screen_pixels(surface_size.width.0, surface_size.height.0);
        prefer_refresh_rate(native);
        // Debug labels stay off: the emulator's Vulkan driver crashes naming objects.
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::VULKAN | wgpu::Backends::GL,
            display: Some(Box::new(AndroidDisplayHandle)),
            flags: wgpu::InstanceFlags::empty().with_env(),
            backend_options: Default::default(),
            memory_budget_thresholds: Default::default(),
        });
        let raw = AndroidRawWindow(native);
        let surface = unsafe {
            instance.create_surface_unsafe(wgpu::SurfaceTargetUnsafe::RawHandle {
                raw_display_handle: Some(rwh::RawDisplayHandle::Android(
                    rwh::AndroidDisplayHandle::new(),
                )),
                raw_window_handle: rwh::RawWindowHandle::AndroidNdk(
                    rwh::AndroidNdkWindowHandle::new(native.cast()),
                ),
            })?
        };
        let context = WgpuContext::new(instance, &surface, None)?;
        drop(surface);
        let gpu_context: GpuContext = Rc::new(RefCell::new(Some(context)));
        let renderer = WgpuRenderer::new(
            gpu_context.clone(),
            &raw,
            surface_config(surface_size),
            None,
        )?;
        let key = NEXT_KEY.get();
        NEXT_KEY.set(key + 1);
        let (safe_area, ime, ime_rest) = INSETS.get();
        let link = Rc::new(FrameLink::new(key, frame));
        let state = Rc::new(RefCell::new(AndroidWindowState {
            handle,
            key,
            link: link.clone(),
            renderer,
            gpu_context,
            surface_size,
            needs_presentation: false,
            request_frame_callback: None,
            event_callback: None,
            resize_callback: None,
            appearance_callback: None,
            insets_callback: None,
            viewport_callback: None,
            active_callback: None,
            close_callback: None,
            back_callback: None,
            input_handler: None,
            accesskit: None,
            keyboard: Keyboard::default(),
            safe_area,
            ime,
            ime_rest,
            text_input: TextInputConfiguration::default(),
            latched: Modifiers::default(),
            compact_keyboard: false,
            key_row: false,
            keyboard_requested: false,
            keyboard_sync_pending: false,
            text_focused: false,
            active: true,
            touches: HashMap::new(),
            pinch: None,
            next_touch: 0,
            pointer_button: None,
            last_position: Point::default(),
            last_click: None,
            selection_dirty: false,
        }));
        WINDOW.set(Some(Rc::downgrade(&state)));
        log::info!(
            "window open: {}x{} px at density {}",
            surface_size.width.0,
            surface_size.height.0,
            bridge::configuration().density
        );
        link.wake();
        Ok(Self(state, link))
    }
}

impl Drop for AndroidWindow {
    fn drop(&mut self) {
        let close = {
            let mut state = self.0.borrow_mut();
            state.accesskit = None;
            state.renderer.destroy();
            state.close_callback.take()
        };
        WINDOW.with_borrow_mut(|window| {
            if window
                .as_ref()
                .is_some_and(|window| window.ptr_eq(&Rc::downgrade(&self.0)))
            {
                *window = None;
            }
        });
        if let Some(close) = close {
            close();
        }
    }
}

pub(crate) fn set_surface(native: Option<NonNull<ANativeWindow>>) {
    let previous = SURFACE.replace(native);
    if let Some(state) = current() {
        let mut guard = state.borrow_mut();
        let state = &mut *guard;
        match native {
            Some(native) => {
                prefer_refresh_rate(native);
                state.surface_size = surface_pixels(native);
                display::set_screen_pixels(state.surface_size.width.0, state.surface_size.height.0);
                let instance = state
                    .gpu_context
                    .borrow()
                    .as_ref()
                    .map(|context| context.instance.clone());
                if let Some(instance) = instance
                    && let Err(error) = state.renderer.replace_surface(
                        &AndroidRawWindow(native),
                        surface_config(state.surface_size),
                        &instance,
                    )
                {
                    log::error!("could not attach the new surface: {error:#}");
                }
                state.needs_presentation = true;
                state.link.wake();
            }
            None => state.renderer.unconfigure_surface(),
        }
    }
    if let Some(previous) = previous
        && Some(previous) != native
    {
        unsafe { ANativeWindow_release(previous.as_ptr()) };
    }
}

pub(crate) fn surface_changed(width: i32, height: i32) {
    display::set_screen_pixels(width, height);
    let Some(state) = current() else {
        return;
    };
    let (logical, scale) = {
        let mut state = state.borrow_mut();
        let new_size = size(DevicePixels(width.max(1)), DevicePixels(height.max(1)));
        if state.surface_size == new_size {
            return;
        }
        state.surface_size = new_size;
        state.renderer.update_drawable_size(new_size);
        (state.logical_size(), state.scale())
    };
    let callback = state.borrow_mut().resize_callback.take();
    if let Some(mut callback) = callback {
        callback(logical, scale);
        state.borrow_mut().resize_callback = Some(callback);
    }
    viewport_changed(&state);
    state.borrow().link.wake();
}

pub(crate) fn redraw_now() {
    if let Some(state) = current() {
        step(&state, true);
    }
}

fn step(state: &Rc<RefCell<AndroidWindowState>>, force: bool) {
    let link = state.borrow().link.clone();
    link.begin_frame();
    if std::mem::take(&mut state.borrow_mut().selection_dirty) {
        text_input::sync_selection(state);
    }
    let callback = state.borrow_mut().request_frame_callback.take();
    if let Some(mut callback) = callback {
        let options = {
            let mut state = state.borrow_mut();
            RequestFrameOptions {
                require_presentation: state.needs_presentation || force,
                force_render: state.renderer.needs_redraw() || force,
            }
        };
        callback(options);
        state.borrow_mut().request_frame_callback = Some(callback);
    }
    let busy = {
        let state = state.borrow();
        !state.touches.is_empty() || state.pinch.is_some()
    };
    link.end_frame(busy);
}

pub(crate) fn close_all() {
    if let Some(state) = current() {
        let close = state.borrow_mut().close_callback.take();
        if let Some(close) = close {
            close();
        }
    }
}

pub(crate) fn set_active(active: bool) {
    let Some(state) = current() else {
        return;
    };
    if !active {
        release_all_keys(&state);
        cancel_touches(&state);
    }
    let callback = {
        let mut state = state.borrow_mut();
        state.active = active;
        state.active_callback.take()
    };
    if let Some(mut callback) = callback {
        callback(active);
        state.borrow_mut().active_callback = Some(callback);
    }
    state.borrow().link.wake();
}

pub(crate) fn set_focused(focused: bool) {
    if !focused && let Some(state) = current() {
        release_all_keys(&state);
    }
}

pub(crate) fn appearance_changed() {
    let Some(state) = current() else {
        return;
    };
    let callback = state.borrow_mut().appearance_callback.take();
    if let Some(mut callback) = callback {
        callback();
        state.borrow_mut().appearance_callback = Some(callback);
    }
    bridge::set_status_bar_on_dark(None);
    state.borrow().link.wake();
}

pub(crate) fn configuration_changed(previous: Configuration, configuration: Configuration) {
    let Some(state) = current() else {
        return;
    };
    if previous.density != configuration.density {
        let (logical, scale) = {
            let state = state.borrow();
            (state.logical_size(), state.scale())
        };
        let callback = state.borrow_mut().resize_callback.take();
        if let Some(mut callback) = callback {
            callback(logical, scale);
            state.borrow_mut().resize_callback = Some(callback);
        }
    }
    if previous.night != configuration.night
        || previous.font_scale != configuration.font_scale
        || previous.reduce_motion != configuration.reduce_motion
        || previous.high_contrast != configuration.high_contrast
    {
        appearance_changed();
    }
    if previous.hardware_keyboard != configuration.hardware_keyboard {
        schedule_keyboard_sync(&state);
    }
    state.borrow().link.wake();
}

pub(crate) fn set_insets(safe_area: [i32; 4], ime: i32, ime_rest: i32) {
    INSETS.set((safe_area, ime, ime_rest));
    let Some(state) = current() else {
        return;
    };
    let insets = {
        let mut state = state.borrow_mut();
        if state.safe_area == safe_area && state.ime == ime && state.ime_rest == ime_rest {
            return;
        }
        if state.ime_rest > 0 && ime_rest == 0 {
            state.keyboard_requested = false;
        }
        state.safe_area = safe_area;
        state.ime = ime;
        state.ime_rest = ime_rest;
        state.insets()
    };
    let callback = state.borrow_mut().insets_callback.take();
    if let Some(mut callback) = callback {
        callback(insets);
        state.borrow_mut().insets_callback = Some(callback);
    }
    viewport_changed(&state);
    state.borrow().link.wake();
}

fn viewport_changed(state: &Rc<RefCell<AndroidWindowState>>) {
    let callback = state.borrow_mut().viewport_callback.take();
    if let Some(mut callback) = callback {
        callback();
        state.borrow_mut().viewport_callback = Some(callback);
    }
}

pub(crate) fn back() {
    let Some(state) = current() else {
        return;
    };
    let callback = state.borrow_mut().back_callback.take();
    if let Some(mut callback) = callback {
        callback();
        state.borrow_mut().back_callback = Some(callback);
    }
}

pub(crate) fn dispatch(
    state: &Rc<RefCell<AndroidWindowState>>,
    event: PlatformInput,
) -> DispatchEventResult {
    let callback = state.borrow_mut().event_callback.take();
    let Some(mut callback) = callback else {
        return DispatchEventResult::default();
    };
    let result = callback(event);
    state.borrow_mut().event_callback = Some(callback);
    state.borrow().link.wake();
    result
}

fn modifiers(state: &Rc<RefCell<AndroidWindowState>>) -> Modifiers {
    state.borrow().keyboard.modifiers
}

pub(crate) fn touch(action: i32, index: usize, pointers: &[Pointer], buttons: i32, meta: i32) {
    let Some(state) = current() else {
        return;
    };
    state.borrow().link.wake();
    if let Some(pointer) = pointers.get(index)
        && pointer.tool == TOOL_TYPE_MOUSE
    {
        mouse(&state, action, pointer, buttons, meta);
        return;
    }
    let phase_for = |action| match action {
        ACTION_DOWN | ACTION_POINTER_DOWN => Some(TouchPhase::Started),
        ACTION_UP | ACTION_POINTER_UP => Some(TouchPhase::Ended),
        ACTION_CANCEL => Some(TouchPhase::Cancelled),
        _ => None,
    };
    match action {
        ACTION_MOVE => {
            for pointer in pointers {
                finger(&state, pointer, TouchPhase::Moved);
            }
        }
        ACTION_CANCEL => {
            for pointer in pointers {
                finger(&state, pointer, TouchPhase::Cancelled);
            }
        }
        _ => {
            if let (Some(phase), Some(pointer)) = (phase_for(action), pointers.get(index)) {
                finger(&state, pointer, phase);
            }
        }
    }
}

fn finger(state: &Rc<RefCell<AndroidWindowState>>, pointer: &Pointer, phase: TouchPhase) {
    let position = state.borrow().position(pointer.x, pointer.y);
    if platform::touch_gestures() {
        dispatch_touch(state, pointer.id, phase, position);
        if matches!(phase, TouchPhase::Ended | TouchPhase::Cancelled) {
            lift_finger(state);
        }
        return;
    }
    let modifiers = modifiers(state);
    state.borrow_mut().last_position = position;
    let event = match phase {
        TouchPhase::Started => {
            let click_count = click_count(state, position);
            PlatformInput::MouseDown(MouseDownEvent {
                button: MouseButton::Left,
                position,
                modifiers,
                click_count,
                first_mouse: false,
            })
        }
        TouchPhase::Moved => PlatformInput::MouseMove(MouseMoveEvent {
            position,
            pressed_button: Some(MouseButton::Left),
            modifiers,
        }),
        TouchPhase::Ended | TouchPhase::Cancelled => PlatformInput::MouseUp(MouseUpEvent {
            button: MouseButton::Left,
            position,
            modifiers,
            click_count: state.borrow().last_click.map_or(1, |(_, _, count)| count),
        }),
    };
    dispatch(state, event);
    if matches!(phase, TouchPhase::Ended | TouchPhase::Cancelled) {
        lift_finger(state);
    }
}

fn click_count(state: &Rc<RefCell<AndroidWindowState>>, position: Point<Pixels>) -> usize {
    let now = Instant::now();
    let mut state = state.borrow_mut();
    let count = match state.last_click {
        Some((at, last, count))
            if now.duration_since(at) < DOUBLE_TAP
                && (f32::from(last.x - position.x).abs()
                    + f32::from(last.y - position.y).abs())
                    < 24.0 =>
        {
            count + 1
        }
        _ => 1,
    };
    state.last_click = Some((now, position, count));
    count
}

fn dispatch_touch(
    state: &Rc<RefCell<AndroidWindowState>>,
    pointer: i32,
    phase: TouchPhase,
    position: Point<Pixels>,
) {
    if pinch_touch(state, pointer, phase, position) {
        return;
    }
    let id = {
        let mut state = state.borrow_mut();
        state.last_position = position;
        match phase {
            TouchPhase::Started => {
                state.next_touch += 1;
                let id = TouchId(state.next_touch);
                state.touches.insert(pointer, (id, position));
                id
            }
            TouchPhase::Ended | TouchPhase::Cancelled => {
                let Some((id, _)) = state.touches.remove(&pointer) else {
                    return;
                };
                id
            }
            TouchPhase::Moved => {
                let Some((id, last)) = state.touches.get_mut(&pointer) else {
                    return;
                };
                if *last == position {
                    return;
                }
                *last = position;
                *id
            }
        }
    };
    dispatch(
        state,
        PlatformInput::Touch(TouchEvent {
            id,
            phase,
            position,
            predicted_position: None,
            force: None,
        }),
    );
}

fn pinch_touch(
    state: &Rc<RefCell<AndroidWindowState>>,
    pointer: i32,
    phase: TouchPhase,
    position: Point<Pixels>,
) -> bool {
    let key = pointer as usize;
    let (step, cancelled, modifiers) = {
        let mut state = state.borrow_mut();
        let modifiers = state.keyboard.modifiers;
        if let Some(pinch) = state.pinch.as_mut() {
            if pinch.owns(key) {
                let step = pinch.touch(key, phase, position);
                if pinch.finished() {
                    state.pinch = None;
                }
                (step, None, modifiers)
            } else {
                return phase == TouchPhase::Started;
            }
        } else if phase == TouchPhase::Started && state.touches.len() == 1 {
            let Some((first, (id, at))) = state.touches.drain().next() else {
                return false;
            };
            state.pinch = Some(Pinch::new([first as usize, key], [at, position]));
            (None, Some((id, at)), modifiers)
        } else {
            return false;
        }
    };
    if let Some((id, position)) = cancelled {
        dispatch(
            state,
            PlatformInput::Touch(TouchEvent {
                id,
                phase: TouchPhase::Cancelled,
                position,
                predicted_position: None,
                force: None,
            }),
        );
    }
    if let Some(step) = step {
        dispatch(
            state,
            PlatformInput::Pinch(PinchEvent {
                position: step.position,
                delta: step.delta,
                modifiers,
                phase: step.phase,
            }),
        );
    }
    true
}

fn lift_finger(state: &Rc<RefCell<AndroidWindowState>>) {
    let modifiers = modifiers(state);
    dispatch(
        state,
        PlatformInput::MouseExited(MouseExitEvent {
            position: point(px(-1.0), px(-1.0)),
            pressed_button: None,
            modifiers,
        }),
    );
}

fn cancel_touches(state: &Rc<RefCell<AndroidWindowState>>) {
    let touches = std::mem::take(&mut state.borrow_mut().touches);
    state.borrow_mut().pinch = None;
    for (_, (id, position)) in touches {
        dispatch(
            state,
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

fn button(buttons: i32) -> MouseButton {
    if buttons & BUTTON_SECONDARY != 0 {
        MouseButton::Right
    } else if buttons & BUTTON_TERTIARY != 0 {
        MouseButton::Middle
    } else {
        MouseButton::Left
    }
}

fn mouse(
    state: &Rc<RefCell<AndroidWindowState>>,
    action: i32,
    pointer: &Pointer,
    buttons: i32,
    meta: i32,
) {
    let position = state.borrow().position(pointer.x, pointer.y);
    let modifiers = crate::android::keyboard::modifiers(meta);
    state.borrow_mut().last_position = position;
    let event = match action {
        ACTION_DOWN | ACTION_POINTER_DOWN => {
            let button = button(buttons);
            state.borrow_mut().pointer_button = Some(button);
            let click_count = click_count(state, position);
            PlatformInput::MouseDown(MouseDownEvent {
                button,
                position,
                modifiers,
                click_count,
                first_mouse: false,
            })
        }
        ACTION_MOVE => PlatformInput::MouseMove(MouseMoveEvent {
            position,
            pressed_button: state.borrow().pointer_button,
            modifiers,
        }),
        ACTION_UP | ACTION_POINTER_UP | ACTION_CANCEL => {
            let button = state
                .borrow_mut()
                .pointer_button
                .take()
                .unwrap_or(MouseButton::Left);
            PlatformInput::MouseUp(MouseUpEvent {
                button,
                position,
                modifiers,
                click_count: state.borrow().last_click.map_or(1, |(_, _, count)| count),
            })
        }
        _ => return,
    };
    dispatch(state, event);
}

pub(crate) fn pointer(
    action: i32,
    x: f32,
    y: f32,
    scroll_x: f32,
    scroll_y: f32,
    _buttons: i32,
    meta: i32,
) -> bool {
    let Some(state) = current() else {
        return false;
    };
    let position = state.borrow().position(x, y);
    let modifiers = crate::android::keyboard::modifiers(meta);
    state.borrow_mut().last_position = position;
    let event = match action {
        ACTION_HOVER_ENTER | ACTION_HOVER_MOVE => PlatformInput::MouseMove(MouseMoveEvent {
            position,
            pressed_button: None,
            modifiers,
        }),
        ACTION_HOVER_EXIT => PlatformInput::MouseExited(MouseExitEvent {
            position,
            pressed_button: None,
            modifiers,
        }),
        ACTION_SCROLL => PlatformInput::ScrollWheel(ScrollWheelEvent {
            position,
            delta: ScrollDelta::Lines(point(-scroll_x * SCROLL_LINE, scroll_y * SCROLL_LINE)),
            modifiers,
            touch_phase: TouchPhase::Moved,
        }),
        _ => return false,
    };
    dispatch(&state, event);
    true
}

pub(crate) fn key(
    action: i32,
    code: i32,
    meta: i32,
    character: u32,
    base: u32,
    repeat: i32,
) -> bool {
    let Some(state) = current() else {
        return false;
    };
    match action {
        KEY_ACTION_DOWN => {
            let event = state
                .borrow_mut()
                .keyboard
                .press(code, meta, character, base, repeat);
            dispatch_modifiers(&state);
            let Some(mut event) = event else {
                return crate::android::keyboard::is_modifier(code);
            };
            event.keystroke.modifiers |= take_latch(&state);
            let keystroke = event.keystroke.clone();
            let result = dispatch(&state, PlatformInput::KeyDown(event));
            if result.propagate
                && !result.default_prevented
                && keystroke.modifiers.is_subset_of(&Modifiers::shift())
                && let Some(key_char) = keystroke.key_char.as_deref()
            {
                text_input::insert_unhandled(&state, key_char);
            }
            true
        }
        KEY_ACTION_UP => {
            let keystroke = state.borrow_mut().keyboard.release(code, meta);
            if let Some(keystroke) = keystroke {
                dispatch(&state, PlatformInput::KeyUp(KeyUpEvent { keystroke }));
            }
            dispatch_modifiers(&state);
            true
        }
        _ => false,
    }
}

fn dispatch_modifiers(state: &Rc<RefCell<AndroidWindowState>>) {
    let event = {
        let state = state.borrow();
        ModifiersChangedEvent {
            modifiers: state.keyboard.modifiers,
            capslock: state.keyboard.capslock,
        }
    };
    dispatch(state, PlatformInput::ModifiersChanged(event));
}

fn release_all_keys(state: &Rc<RefCell<AndroidWindowState>>) {
    let keys = state.borrow_mut().keyboard.clear();
    for keystroke in keys {
        dispatch(state, PlatformInput::KeyUp(KeyUpEvent { keystroke }));
    }
    dispatch_modifiers(state);
}

pub(crate) fn synthesize_keystroke(
    state: &Rc<RefCell<AndroidWindowState>>,
    key: &str,
    extra: Modifiers,
) {
    let latched = take_latch(state);
    let held = state.borrow().keyboard.modifiers;
    let keystroke = Keystroke {
        modifiers: held | latched | extra,
        key: key.to_owned(),
        key_char: None,
    };
    dispatch(
        state,
        PlatformInput::KeyDown(KeyDownEvent {
            keystroke: keystroke.clone(),
            is_held: false,
            prefer_character_input: false,
        }),
    );
    dispatch(state, PlatformInput::KeyUp(KeyUpEvent { keystroke }));
}

fn take_latch(state: &Rc<RefCell<AndroidWindowState>>) -> Modifiers {
    let latched = std::mem::take(&mut state.borrow_mut().latched);
    if latched != Modifiers::default() {
        bridge::clear_key_row_latch();
    }
    latched
}

/// A tap on the key row above the soft keyboard, which tablets get in place
/// of the phone shell's own row, as on iPad. Control and Alt latch onto the
/// next key.
pub(crate) fn key_row_key(index: i32, selected: bool) {
    let Some(state) = current() else {
        return;
    };
    let Some(key) = usize::try_from(index)
        .ok()
        .and_then(|index| KEY_ROW.get(index))
    else {
        return;
    };
    match *key {
        RowKey::Key(key) => synthesize_keystroke(&state, key, Modifiers::default()),
        RowKey::Control => state.borrow_mut().latched.control = selected,
        RowKey::Alt => state.borrow_mut().latched.alt = selected,
        RowKey::Hide => request_keyboard(&state, false),
    }
}

fn request_keyboard(state: &Rc<RefCell<AndroidWindowState>>, visible: bool) {
    state.borrow_mut().keyboard_requested = visible;
    schedule_keyboard_sync(state);
}

/// Shows or hides the soft keyboard on the next turn of the main looper, once
/// GPUI has applied the focused input's configuration. Android itself keeps it
/// down while a hardware keyboard is attached, unless the user asked otherwise.
fn schedule_keyboard_sync(state: &Rc<RefCell<AndroidWindowState>>) {
    if std::mem::replace(&mut state.borrow_mut().keyboard_sync_pending, true) {
        return;
    }
    crate::android::dispatcher::defer(|| {
        let Some(state) = current() else {
            return;
        };
        let (visible, key_row) = {
            let mut state = state.borrow_mut();
            state.keyboard_sync_pending = false;
            let typing = state.text_input.input_mode != TextInputMode::None;
            let key_row = typing
                && state.text_focused
                && !state.compact_keyboard
                && state.text_input.input_action == TextInputAction::Enter;
            let changed = std::mem::replace(&mut state.key_row, key_row) != key_row;
            (
                state.keyboard_requested && typing,
                changed.then_some(key_row),
            )
        };
        bridge::set_keyboard_visible(visible);
        if let Some(key_row) = key_row {
            bridge::set_key_row(key_row);
        }
    });
}

/// Stops the soft keyboard rising whenever a text input takes focus: the phone
/// shell raises it only from its keyboard button or a tap on the terminal, and
/// draws its own key row.
pub fn set_compact_keyboard(enabled: bool) {
    with_state(|state| {
        if std::mem::replace(&mut state.borrow_mut().compact_keyboard, enabled) != enabled {
            take_latch(state);
            schedule_keyboard_sync(state);
        }
    });
}

/// Pastes the clipboard into the focused input, outside the current update.
pub fn request_paste() {
    crate::android::dispatcher::defer(|| {
        let Some(item) = platform::read_clipboard() else {
            return;
        };
        with_state(|state| {
            let handler = state.borrow_mut().input_handler.take();
            if let Some(mut handler) = handler {
                handler.paste(item);
                state.borrow_mut().restore_input_handler(handler);
            }
        });
    });
}

/// Shows the copy, paste and select-all menu at a point in window coordinates.
pub fn show_edit_menu(x: f32, y: f32) {
    let scale = bridge::configuration().density;
    crate::android::dispatcher::defer(move || bridge::show_text_menu(x * scale, y * scale));
}

impl PlatformWindow for AndroidWindow {
    fn bounds(&self) -> Bounds<Pixels> {
        Bounds {
            origin: Point::default(),
            size: self.0.borrow().logical_size(),
        }
    }

    fn is_maximized(&self) -> bool {
        false
    }

    fn window_bounds(&self) -> WindowBounds {
        WindowBounds::Fullscreen(self.bounds())
    }

    fn content_size(&self) -> Size<Pixels> {
        self.0.borrow().logical_size()
    }

    fn visual_viewport_bounds(&self) -> Bounds<Pixels> {
        let state = self.0.borrow();
        state.viewport_above(state.ime)
    }

    fn on_visual_viewport_changed(&self, callback: Box<dyn FnMut()>) {
        self.0.borrow_mut().viewport_callback = Some(callback);
    }

    fn target_visual_viewport_bounds(&self) -> Bounds<Pixels> {
        let state = self.0.borrow();
        state.viewport_above(state.ime_rest)
    }

    fn resize(&mut self, _size: Size<Pixels>) {}

    fn scale_factor(&self) -> f32 {
        self.0.borrow().scale()
    }

    fn appearance(&self) -> WindowAppearance {
        if platform::appearance_is_dark() {
            WindowAppearance::Dark
        } else {
            WindowAppearance::Light
        }
    }

    fn display(&self) -> Option<Rc<dyn PlatformDisplay>> {
        Some(Rc::new(AndroidDisplay))
    }

    fn mouse_position(&self) -> Point<Pixels> {
        self.0.borrow().last_position
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

    fn set_text_input_configuration(&mut self, configuration: TextInputConfiguration) {
        let changed = {
            let mut state = self.0.borrow_mut();
            let changed = state.text_input != configuration;
            state.text_input = configuration;
            changed
        };
        if changed {
            crate::android::dispatcher::defer(bridge::restart_input);
            schedule_keyboard_sync(&self.0);
        }
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

    fn visibility(&self) -> zz_gpui::WindowVisibility {
        zz_gpui::WindowVisibility::Visible
    }

    fn on_visibility_change(&self, _callback: Box<dyn FnMut(zz_gpui::WindowVisibility)>) {}

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

    fn frame_waker(&self) -> Option<Rc<dyn Fn()>> {
        let link = self.1.clone();
        Some(Rc::new(move || link.wake()))
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
        _callback: Box<dyn FnMut() -> Option<zz_gpui::WindowControlArea>>,
    ) {
    }

    fn on_close(&self, callback: Box<dyn FnOnce()>) {
        self.0.borrow_mut().close_callback = Some(callback);
    }

    fn on_appearance_changed(&self, callback: Box<dyn FnMut()>) {
        self.0.borrow_mut().appearance_callback = Some(callback);
    }

    fn draw(&self, scene: &zz_gpui::Scene) {
        let mut state = self.0.borrow_mut();
        state.needs_presentation = !state.renderer.draw(scene);
    }

    fn schedule_frame(&self) {
        self.1.wake();
    }

    fn keeps_presenting_after_input(&self) -> bool {
        false
    }

    fn sprite_atlas(&self) -> Arc<dyn PlatformAtlas> {
        self.0.borrow().renderer.sprite_atlas().clone()
    }

    fn is_subpixel_rendering_supported(&self) -> bool {
        false
    }

    fn supports_backdrop_sampling(&self) -> bool {
        self.0.borrow().renderer.supports_backdrop_sampling()
    }

    fn gpu_specs(&self) -> Option<zz_gpui::GpuSpecs> {
        self.0.borrow().renderer.gpu_specs()
    }

    fn update_ime_position(&self, _bounds: Bounds<Pixels>) {}

    fn a11y_init(&self, callbacks: zz_gpui::A11yCallbacks) {
        let adapter = bridge::with_view(|env, view| {
            accesskit_android::InjectingAdapter::new(
                env,
                view,
                A11yActivationHandler(callbacks.activation),
                A11yActionHandler(callbacks.action),
            )
        });
        self.0.borrow_mut().accesskit = adapter;
    }

    fn a11y_tree_update(&self, tree_update: zz_gpui::accesskit::TreeUpdate) {
        if let Some(adapter) = self.0.borrow_mut().accesskit.as_mut() {
            adapter.update_if_active(|| tree_update);
        }
    }

    fn insets(&self) -> zz_gpui::WindowInsets {
        self.0.borrow().insets()
    }

    fn on_insets_changed(&self, callback: Box<dyn FnMut(zz_gpui::WindowInsets)>) {
        self.0.borrow_mut().insets_callback = Some(callback);
    }

    fn set_back_handler(&self, callback: Box<dyn FnMut()>) {
        self.0.borrow_mut().back_callback = Some(callback);
    }

    fn set_back_enabled(&self, enabled: bool) {
        crate::android::dispatcher::defer(move || bridge::set_back_enabled(enabled));
    }

    fn show_soft_keyboard(&self) {
        request_keyboard(&self.0, true);
    }

    fn hide_soft_keyboard(&self) {
        request_keyboard(&self.0, false);
    }

    fn text_input_state_changed(&self, change: TextInputStateChange) {
        match change {
            TextInputStateChange::FocusGained => {
                let compact = {
                    let mut state = self.0.borrow_mut();
                    state.text_focused = true;
                    state.compact_keyboard
                };
                crate::android::dispatcher::defer(bridge::restart_input);
                if !compact {
                    request_keyboard(&self.0, true);
                }
            }
            TextInputStateChange::FocusLost => {
                self.0.borrow_mut().text_focused = false;
                request_keyboard(&self.0, false);
            }
            TextInputStateChange::SelectionChanged | TextInputStateChange::ContentChanged => {
                self.0.borrow_mut().selection_dirty = true;
                self.1.wake();
            }
        }
    }
}

impl rwh::HasWindowHandle for AndroidWindow {
    fn window_handle(&self) -> Result<rwh::WindowHandle<'_>, rwh::HandleError> {
        let native = SURFACE.get().ok_or(rwh::HandleError::Unavailable)?;
        unsafe {
            Ok(rwh::WindowHandle::borrow_raw(
                rwh::RawWindowHandle::AndroidNdk(rwh::AndroidNdkWindowHandle::new(native.cast())),
            ))
        }
    }
}

impl rwh::HasDisplayHandle for AndroidWindow {
    fn display_handle(&self) -> Result<rwh::DisplayHandle<'_>, rwh::HandleError> {
        Ok(rwh::DisplayHandle::android())
    }
}

struct A11yActivationHandler(
    Box<dyn Fn() -> Option<zz_gpui::accesskit::TreeUpdate> + Send + 'static>,
);

impl zz_gpui::accesskit::ActivationHandler for A11yActivationHandler {
    fn request_initial_tree(&mut self) -> Option<zz_gpui::accesskit::TreeUpdate> {
        (self.0)()
    }
}

struct A11yActionHandler(Box<dyn Fn(zz_gpui::accesskit::ActionRequest) + Send + 'static>);

impl zz_gpui::accesskit::ActionHandler for A11yActionHandler {
    fn do_action(&mut self, request: zz_gpui::accesskit::ActionRequest) {
        (self.0)(request);
    }
}
