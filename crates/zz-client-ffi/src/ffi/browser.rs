use std::{
    collections::{BTreeMap, VecDeque},
    ffi::{CStr, c_char, c_void},
    path::PathBuf,
    sync::Arc,
};

use async_channel::{Receiver, Sender};
use serde::Deserialize;
use serde_json::{Value, json};
use zz_browser::{
    BrowserBootstrap, BrowserEvent, BrowserProfilePaths, BrowserRuntime, BrowserSession,
    EditCommand, ElementPickerAppearance, Modifiers, OsrFrame, PointerButton, PointerEvent,
    PointerPhase, RuntimePhase, RuntimeSignal, SessionPhase, Viewport, WheelEvent,
};

use super::{ZzBytes, ZzClient, ZzJson, key_code, write_c_string};
use zz_chrome_import::recent_pages::{RecentPage, RecentPages};

pub struct ZzBrowserRuntime {
    runtime: BrowserRuntime,
    signals: Receiver<RuntimeSignal>,
    sessions: BTreeMap<u64, Session>,
    events: VecDeque<ZzBrowserEvent>,
    error: String,
    closing: bool,
    egress: Option<(String, u16)>,
    history: RecentPages,
    data_tx: Sender<BrowserData>,
    data_rx: Receiver<BrowserData>,
    pending_jobs: usize,
    next_request_id: u64,
    cookie_imports: Vec<PendingCookies>,
}

struct Session {
    browser: BrowserSession,
    events: Receiver<BrowserEvent>,
    data_clear: Option<Receiver<zz_browser::SiteDataClearResult>>,
    profile: String,
    url: String,
    omnibox: Option<(String, bool, bool)>,
}

impl Session {
    fn new(browser: BrowserSession, profile: String) -> Self {
        Self {
            events: browser.events(),
            browser,
            data_clear: None,
            profile,
            url: String::new(),
            omnibox: None,
        }
    }
}

pub struct ZzBrowserEvent {
    payload: Vec<u8>,
    image: Option<Arc<[u8]>>,
}

pub struct ZzBrowserFrame(OsrFrame);

impl ZzBrowserRuntime {
    fn enqueue(&mut self, value: Value, image: Option<Arc<[u8]>>) {
        self.events.push_back(ZzBrowserEvent {
            payload: value.to_string().into_bytes(),
            image,
        });
    }

    fn pump(&mut self) {
        self.runtime.do_message_loop_work();
        self.pump_data();
        while let Ok(signal) = self.signals.try_recv() {
            let result = match signal {
                RuntimeSignal::ContextInitialized => self.runtime.handle_context_initialized(),
                RuntimeSignal::RequestContextInitialized { profile } => self
                    .runtime
                    .handle_request_context_initialized(&profile)
                    .map(drop),
                RuntimeSignal::ScheduleMessagePump(_) => Ok(()),
            };
            if let Err(error) = result {
                self.error = error.to_string();
            }
        }
        let mut pending = Vec::new();
        let mut popups = Vec::new();
        for (id, session) in &mut self.sessions {
            if self.runtime.external_begin_frame_enabled()
                && session.browser.viewport().visible
                && session.browser.phase() == SessionPhase::Ready
            {
                session.browser.send_external_begin_frame();
            }
            while let Ok(event) = session.events.try_recv() {
                let (mut payload, image) = match event {
                    BrowserEvent::Created { .. } => {
                        session.browser.mark_ready();
                        (json!({"kind":"ready"}), None)
                    }
                    BrowserEvent::AddressChanged { url, .. } => {
                        session.url = url.to_string();
                        self.history.record_visit(&session.profile, &url);
                        (json!({"kind":"address", "url":url.as_ref()}), None)
                    }
                    BrowserEvent::TitleChanged { title, .. } => {
                        self.history
                            .record_title(&session.profile, &session.url, &title);
                        (json!({"kind":"title", "title":title.as_ref()}), None)
                    }
                    BrowserEvent::LoadingChanged {
                        loading,
                        can_go_back,
                        can_go_forward,
                        ..
                    } => {
                        if loading {
                            if let Some((_, _, started)) = session.omnibox.as_mut() {
                                *started = true;
                            }
                        } else if session
                            .omnibox
                            .as_ref()
                            .is_some_and(|(_, _, started)| *started)
                            && let Some((input, selected, _)) = session.omnibox.take()
                        {
                            self.history.record_omnibox_use(
                                &session.profile,
                                &input,
                                &session.url,
                                selected,
                            );
                        }
                        (
                            json!({"kind":"loading", "loading":loading,"back":can_go_back,"forward":can_go_forward}),
                            None,
                        )
                    }
                    BrowserEvent::FrameReady { .. } => continue,
                    BrowserEvent::SharedTextureFailed { reason, .. } => (
                        json!({"kind":"texture-error", "message":reason.as_ref()}),
                        None,
                    ),
                    BrowserEvent::LoadFailed {
                        code,
                        description,
                        url,
                        ..
                    } => {
                        session.omnibox = None;
                        (
                            json!({"kind":"load-error", "code":code,"message":description.as_ref(),"url":url.as_ref()}),
                            None,
                        )
                    }
                    BrowserEvent::CursorChanged { cursor, .. } => (
                        json!({"kind":"cursor", "cursor":format!("{cursor:?}")}),
                        None,
                    ),
                    BrowserEvent::ElementPicked {
                        text, screenshot, ..
                    } => (json!({"kind":"picked", "text":text.as_ref()}), screenshot),
                    BrowserEvent::ElementPickCancelled { .. } => {
                        (json!({"kind":"pick-cancelled"}), None)
                    }
                    BrowserEvent::ElementPickFailed { .. } => (json!({"kind":"pick-error"}), None),
                    BrowserEvent::ContextMenuRequested { request, .. } => (
                        json!({"kind":"context-menu", "x":request.x,"y":request.y,"url":request.link_url.as_deref(),"text":request.selection_text.as_deref(),"editable":request.editable,"cut":request.edit_flags.can_cut,"copy":request.edit_flags.can_copy,"paste":request.edit_flags.can_paste,"selectAll":request.edit_flags.can_select_all}),
                        None,
                    ),
                    BrowserEvent::PopupCreated {
                        popup,
                        url,
                        foreground,
                        ..
                    } => {
                        if let Some(child) = session.browser.take_popup(popup) {
                            popups.push((child, session.profile.clone()));
                        }
                        (
                            json!({"kind":"popup", "popup":popup.0,"url":url.as_ref(),"foreground":foreground}),
                            None,
                        )
                    }
                    BrowserEvent::PopupRequested {
                        url, foreground, ..
                    } => (
                        json!({"kind":"popup-request", "url":url.as_ref(),"foreground":foreground}),
                        None,
                    ),
                    BrowserEvent::RenderProcessTerminated {
                        status, error_code, ..
                    } => {
                        session.browser.mark_crashed();
                        (
                            json!({"kind":"crashed","message":status.as_ref(),"code":error_code}),
                            None,
                        )
                    }
                    BrowserEvent::Closed { .. } => {
                        session.browser.mark_closed();
                        (json!({"kind":"closed"}), None)
                    }
                };
                payload["session"] = json!(id);
                pending.push((payload, image));
            }
            if let Some(result) = session
                .data_clear
                .as_ref()
                .and_then(|receiver| receiver.try_recv().ok())
            {
                pending.push((
                    json!({"kind":"data-cleared", "session":id,"result":format!("{result:?}")}),
                    None,
                ));
                session.data_clear = None;
            }
        }
        for (mut popup, profile) in popups {
            if self.closing {
                popup.close(true);
            }
            self.sessions
                .insert(popup.id().0, Session::new(popup, profile));
        }
        self.sessions
            .retain(|_, session| session.browser.phase() != SessionPhase::Closed);
        for (payload, image) in pending {
            self.enqueue(payload, image);
        }
    }
}

unsafe fn string<'a>(value: *const c_char) -> Option<&'a str> {
    if value.is_null() {
        return None;
    }
    unsafe { CStr::from_ptr(value) }.to_str().ok()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_browser_runtime_new(
    cache_root: *const c_char,
    error: *mut c_char,
    capacity: usize,
) -> *mut ZzBrowserRuntime {
    let prepared = match unsafe { string(cache_root) }.filter(|root| !root.is_empty()) {
        Some(root) => {
            let root = PathBuf::from(root);
            zz_browser::bootstrap_with_profile_paths(BrowserProfilePaths {
                profile: root.join("zz-default"),
                root,
            })
        }
        None => zz_browser::resolve_profile_paths()
            .map_err(|error| zz_browser::BrowserError::Profile(error.into()))
            .and_then(|paths| {
                let root = paths.root.with_file_name("root-native");
                zz_browser::bootstrap_with_profile_paths(BrowserProfilePaths {
                    profile: root.join("zz-default"),
                    root,
                })
            }),
    };
    let result = prepared.and_then(|bootstrap| match bootstrap {
        BrowserBootstrap::Runtime(mut runtime) => {
            runtime.set_background_color(0xffff_ffff);
            runtime.start()?;
            let history =
                RecentPages::load(Some(runtime.profile_paths().root.join("recent-pages")));
            let (data_tx, data_rx) = async_channel::unbounded();
            Ok(ZzBrowserRuntime {
                history,
                data_tx,
                data_rx,
                egress: None,
                pending_jobs: 0,
                next_request_id: 1,
                cookie_imports: Vec::new(),
                signals: runtime.signals(),
                runtime,
                sessions: BTreeMap::new(),
                events: VecDeque::new(),
                error: String::new(),
                closing: false,
            })
        }
        BrowserBootstrap::SubprocessExit(_) => Err(zz_browser::BrowserError::NotReady),
    });
    match result {
        Ok(runtime) => {
            unsafe {
                write_c_string("", error, capacity);
            }
            Box::into_raw(Box::new(runtime))
        }
        Err(value) => {
            unsafe {
                write_c_string(&value.to_string(), error, capacity);
            }
            std::ptr::null_mut()
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_browser_runtime_pump(runtime: *mut ZzBrowserRuntime) -> bool {
    let Some(runtime) = (unsafe { runtime.as_mut() }) else {
        return false;
    };
    runtime.pump();
    runtime.runtime.phase() == RuntimePhase::Running && !runtime.closing
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_browser_runtime_error(runtime: *const ZzBrowserRuntime) -> ZzBytes {
    unsafe { runtime.as_ref() }.map_or(ZzBytes::EMPTY, |runtime| ZzBytes::new(&runtime.error))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_browser_runtime_free(runtime: *mut ZzBrowserRuntime) -> bool {
    let Some(value) = (unsafe { runtime.as_mut() }) else {
        return true;
    };
    if !value.closing {
        value.closing = true;
        for session in value.sessions.values_mut() {
            session.browser.close(true);
        }
    }
    value.pump();
    if value.runtime.active_session_count() != 0
        || value.runtime.active_data_operation_count() != 0
        || value.pending_jobs != 0
        || !value.cookie_imports.is_empty()
    {
        return false;
    }
    match value.runtime.shutdown() {
        Ok(()) => {
            drop(unsafe { Box::from_raw(runtime) });
            true
        }
        Err(error) => {
            value.error = error.to_string();
            false
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_browser_session_new(
    runtime: *mut ZzBrowserRuntime,
    profile: *const c_char,
    url: *const c_char,
    width: u32,
    height: u32,
    scale: f32,
) -> u64 {
    let (Some(runtime), Some(profile), Some(url)) =
        (unsafe { (runtime.as_mut(), string(profile), string(url)) })
    else {
        return 0;
    };
    if runtime.closing || runtime.runtime.phase() != RuntimePhase::Running {
        return 0;
    }
    let profile = match zz_browser::normalize_browser_profile_name(profile) {
        Ok(profile) => profile,
        Err(error) => {
            runtime.error = error.to_string();
            return 0;
        }
    };
    let request_profile = match runtime.request_profile(&profile) {
        Ok(profile) => profile,
        Err(error) => {
            runtime.error = error;
            return 0;
        }
    };
    let ready = if runtime.egress.is_some() {
        runtime
            .runtime
            .ensure_egress_profile_context(&request_profile)
    } else {
        runtime.runtime.ensure_profile_context(&request_profile)
    };
    match ready {
        Ok(true) => {}
        Ok(false) => return 0,
        Err(error) => {
            runtime.error = error.to_string();
            return 0;
        }
    }
    if let Some((_, port)) = &runtime.egress
        && let Err(error) = runtime.runtime.set_profile_proxy(&request_profile, *port)
    {
        runtime.error = error.to_string();
        return 0;
    }
    let viewport = Viewport {
        width: width.max(1),
        height: height.max(1),
        scale_factor: scale,
        visible: true,
        ..Viewport::default()
    }
    .sanitized();
    match runtime
        .runtime
        .create_session(&request_profile, url, viewport, 1.0, None, None, true)
    {
        Ok(browser) => {
            let id = browser.id().0;
            runtime.sessions.insert(id, Session::new(browser, profile));
            id
        }
        Err(error) => {
            runtime.error = error.to_string();
            0
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_browser_session_close(runtime: *mut ZzBrowserRuntime, session: u64) {
    if let Some(session) = unsafe { runtime.as_mut() }.and_then(|r| r.sessions.get_mut(&session)) {
        session.browser.close(true);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_browser_session_viewport(
    runtime: *mut ZzBrowserRuntime,
    session: u64,
    width: u32,
    height: u32,
    scale: f32,
    zoom: f32,
    screen_x: i32,
    screen_y: i32,
    visible: bool,
    focused: bool,
) {
    if let Some(session) = unsafe { runtime.as_mut() }.and_then(|r| r.sessions.get_mut(&session)) {
        session.browser.set_viewport(
            Viewport {
                width: width.max(1),
                height: height.max(1),
                scale_factor: scale,
                window_zoom: zoom,
                screen_x,
                screen_y,
                visible,
            }
            .sanitized(),
        );
        session.browser.set_focus(focused);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_browser_event_next(
    runtime: *mut ZzBrowserRuntime,
) -> *mut ZzBrowserEvent {
    unsafe { runtime.as_mut() }
        .and_then(|r| r.events.pop_front())
        .map_or(std::ptr::null_mut(), |event| Box::into_raw(Box::new(event)))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_browser_event_json(event: *const ZzBrowserEvent) -> ZzBytes {
    unsafe { event.as_ref() }.map_or(ZzBytes::EMPTY, |event| ZzBytes::from_bytes(&event.payload))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_browser_event_image(event: *const ZzBrowserEvent) -> ZzBytes {
    unsafe { event.as_ref() }
        .and_then(|event| event.image.as_deref())
        .map_or(ZzBytes::EMPTY, ZzBytes::from_bytes)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_browser_event_free(event: *mut ZzBrowserEvent) {
    if !event.is_null() {
        drop(unsafe { Box::from_raw(event) });
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_browser_frame_take(
    runtime: *mut ZzBrowserRuntime,
    session: u64,
) -> *mut ZzBrowserFrame {
    unsafe { runtime.as_mut() }
        .and_then(|r| r.sessions.get_mut(&session))
        .and_then(|session| session.browser.take_frame())
        .map_or(std::ptr::null_mut(), |frame| {
            Box::into_raw(Box::new(ZzBrowserFrame(frame)))
        })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_browser_frame_free(frame: *mut ZzBrowserFrame) {
    if !frame.is_null() {
        drop(unsafe { Box::from_raw(frame) });
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_browser_frame_width(frame: *const ZzBrowserFrame) -> u32 {
    unsafe { frame.as_ref() }.map_or(0, |frame| frame.0.device_width())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_browser_frame_height(frame: *const ZzBrowserFrame) -> u32 {
    unsafe { frame.as_ref() }.map_or(0, |frame| frame.0.device_height())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_browser_frame_surface(frame: *const ZzBrowserFrame) -> *mut c_void {
    match unsafe { frame.as_ref() }.map(|frame| &frame.0) {
        Some(OsrFrame::MacGpu(frame)) => frame.io_surface.as_ptr(),
        _ => std::ptr::null_mut(),
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_browser_frame_bgra(frame: *const ZzBrowserFrame) -> ZzBytes {
    match unsafe { frame.as_ref() }.map(|frame| &frame.0) {
        Some(OsrFrame::OwnedBgra(frame)) => ZzBytes::from_bytes(&frame.bgra),
        _ => ZzBytes::EMPTY,
    }
}

#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "kebab-case")]
enum Action {
    Navigate {
        url: String,
    },
    SubmitAddress {
        input: String,
        provider: String,
        selected: bool,
        url: Option<String>,
    },
    Back,
    Forward,
    Reload,
    Stop,
    Devtools,
    CancelPick,
    ZoomIn,
    ZoomOut,
    ZoomReset,
    Edit {
        command: EditCommand,
    },
    Text {
        text: String,
    },
    Composition {
        text: String,
        start: usize,
        end: usize,
    },
    Commit {
        text: String,
    },
    CancelComposition,
    Inspect {
        x: i32,
        y: i32,
    },
    ClearSiteData,
    Pick {
        appearance: PickerAppearance,
    },
}

#[derive(Deserialize)]
struct PickerAppearance {
    outline: String,
    fill: String,
    contrast: String,
    background: String,
    foreground: String,
    border: String,
    shadow: Option<String>,
    radius: f32,
    font: String,
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_browser_session_action(
    runtime: *mut ZzBrowserRuntime,
    id: u64,
    json: *const c_char,
) -> bool {
    let (Some(runtime), Some(json)) = (unsafe { (runtime.as_mut(), string(json)) }) else {
        return false;
    };
    let Some(session) = runtime.sessions.get_mut(&id) else {
        return false;
    };
    let Ok(action) = serde_json::from_str::<Action>(json) else {
        return false;
    };
    match action {
        Action::Navigate { url } => {
            session.omnibox = None;
            session.browser.navigate(&url);
        }
        Action::SubmitAddress {
            input,
            provider,
            selected,
            url,
        } => {
            let Some(provider) = zz_browser::SearchProvider::parse(&provider) else {
                return false;
            };
            let Ok(url) = zz_browser::resolve_address(url.as_deref().unwrap_or(&input), provider)
            else {
                return false;
            };
            session.omnibox = Some((input, selected, false));
            session.browser.navigate(&url);
        }
        Action::Back => {
            session.omnibox = None;
            session.browser.go_back();
        }
        Action::Forward => {
            session.omnibox = None;
            session.browser.go_forward();
        }
        Action::Reload => {
            session.omnibox = None;
            session.browser.reload();
        }
        Action::Stop => {
            session.omnibox = None;
            session.browser.command_sink().stop_loading();
        }
        Action::CancelPick => {
            let _ = session.browser.cancel_element_pick();
        }
        Action::Devtools => session.browser.toggle_dev_tools(),
        Action::ZoomIn => {
            let _ = session.browser.zoom_in();
        }
        Action::ZoomOut => {
            let _ = session.browser.zoom_out();
        }
        Action::ZoomReset => {
            let _ = session.browser.reset_zoom();
        }
        Action::Edit { command } => session.browser.edit(command),
        Action::Text { text } => session.browser.send_text(&text),
        Action::Composition { text, start, end } => {
            session.browser.set_composition(&text, start..end)
        }
        Action::Commit { text } => session.browser.commit_composition(&text),
        Action::CancelComposition => session.browser.cancel_composition(),
        Action::Inspect { x, y } => session.browser.inspect_element_at(x, y),
        Action::ClearSiteData => match session.browser.clear_site_data() {
            Ok(receiver) => session.data_clear = Some(receiver),
            Err(error) => {
                runtime.error = error.to_string();
                return false;
            }
        },
        Action::Pick { appearance } => {
            return session
                .browser
                .start_element_pick(&ElementPickerAppearance {
                    highlight_outline: appearance.outline,
                    highlight_fill: appearance.fill,
                    highlight_contrast: appearance.contrast,
                    preview_background: appearance.background,
                    preview_foreground: appearance.foreground,
                    preview_border: appearance.border,
                    shadow: appearance.shadow,
                    radius: appearance.radius,
                    font_family: appearance.font,
                    page_zoom: session.browser.page_zoom_factor(),
                });
        }
    }
    true
}

fn modifiers(bits: u8) -> Modifiers {
    let button = if bits & 16 != 0 {
        Some(PointerButton::Left)
    } else if bits & 32 != 0 {
        Some(PointerButton::Middle)
    } else if bits & 64 != 0 {
        Some(PointerButton::Right)
    } else {
        None
    };
    Modifiers::new(bits & 1 != 0, bits & 2 != 0, bits & 4 != 0, bits & 8 != 0)
        .with_pointer_button(button)
        .with_repeat(bits & 128 != 0)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_browser_session_pointer(
    runtime: *mut ZzBrowserRuntime,
    id: u64,
    x: i32,
    y: i32,
    phase: u32,
    button: u32,
    clicks: i32,
    flags: u8,
) {
    let Some(session) = unsafe { runtime.as_mut() }.and_then(|r| r.sessions.get_mut(&id)) else {
        return;
    };
    let phase = match phase {
        0 => PointerPhase::Move,
        1 => PointerPhase::Leave,
        2 => PointerPhase::Down,
        3 => PointerPhase::Up,
        _ => return,
    };
    let button = match button {
        1 => Some(PointerButton::Left),
        2 => Some(PointerButton::Middle),
        3 => Some(PointerButton::Right),
        _ => None,
    };
    session.browser.send_pointer(PointerEvent {
        x,
        y,
        phase,
        button,
        click_count: clicks,
        modifiers: modifiers(flags),
    });
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_browser_session_wheel(
    runtime: *mut ZzBrowserRuntime,
    id: u64,
    x: i32,
    y: i32,
    dx: i32,
    dy: i32,
    precise: bool,
    flags: u8,
) {
    let Some(session) = unsafe { runtime.as_mut() }.and_then(|r| r.sessions.get_mut(&id)) else {
        return;
    };
    session.browser.send_wheel(WheelEvent {
        x,
        y,
        delta_x: dx,
        delta_y: dy,
        precise,
        modifiers: modifiers(flags),
    });
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_browser_session_key(
    runtime: *mut ZzBrowserRuntime,
    id: u64,
    code: u32,
    scalar: u32,
    function: u8,
    action: u32,
    flags: u8,
) {
    let Some(session) = unsafe { runtime.as_mut() }.and_then(|r| r.sessions.get_mut(&id)) else {
        return;
    };
    let (Some(key), Some(action), Some(modifiers)) = (
        key_code(code, scalar, function),
        super::key_action(action),
        zz_terminal::Modifiers::from_bits(flags & 15),
    ) else {
        return;
    };
    session
        .browser
        .send_key(zz_browser::terminal_key_input(&zz_terminal::KeyInput {
            key,
            action,
            modifiers,
            text: None,
            unshifted_codepoint: None,
        }));
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_browser_resolve_address(
    input: *const c_char,
    provider: *const c_char,
    output: *mut c_char,
    capacity: usize,
) -> usize {
    let (Some(input), Some(provider)) = (unsafe { (string(input), string(provider)) }) else {
        return 0;
    };
    let Some(provider) = zz_browser::SearchProvider::parse(provider) else {
        return 0;
    };
    match zz_browser::resolve_address(input, provider) {
        Ok(url) => unsafe { write_c_string(&url, output, capacity) },
        Err(_) => 0,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_browser_session_dispatch(
    runtime: *mut ZzBrowserRuntime,
    id: u64,
    json: *const c_char,
) -> bool {
    let (Some(runtime), Some(json)) = (unsafe { (runtime.as_mut(), string(json)) }) else {
        return false;
    };
    let Some(session) = runtime.sessions.get_mut(&id) else {
        return false;
    };
    let Ok(command) = serde_json::from_str::<zz_protocol::BrowserCommand>(json) else {
        return false;
    };
    use zz_protocol::{BrowserCommand as Command, KeyToken};
    let send = |keys: &[KeyToken]| {
        for key in keys {
            match key {
                KeyToken::Literal(text) => session.browser.send_text(text),
                KeyToken::Named(name) => {
                    if let Some(key) = zz_browser::named_key_input(name) {
                        session.browser.send_key(key);
                    }
                }
                KeyToken::Raw(_) => {}
            }
        }
    };
    match command {
        Command::Navigate(url) => {
            session.omnibox = None;
            session.browser.navigate(&url);
        }
        Command::Back => {
            session.omnibox = None;
            session.browser.go_back();
        }
        Command::Forward => {
            session.omnibox = None;
            session.browser.go_forward();
        }
        Command::Reload => {
            session.omnibox = None;
            session.browser.reload();
        }
        Command::SendKeys(keys) => send(&keys),
        Command::SendKeysRepeated { keys, count } => {
            for _ in 0..count.min(zz_protocol::MAX_BROWSER_KEY_REPEAT) {
                send(&keys);
            }
        }
        Command::Key(key) => session
            .browser
            .send_key(zz_browser::terminal_key_input(&key)),
        Command::Screenshot { .. } => return false,
    }
    true
}

struct ImportFailure {
    message: String,
    permission_denied: bool,
}

enum BrowserData {
    Profiles {
        request_id: u64,
        result: Result<Vec<zz_chrome_import::profiles::DetectedChromeProfile>, String>,
    },
    Import {
        request_id: u64,
        profile: String,
        request_profile: String,
        history: Result<zz_chrome_import::history::ChromeHistoryImport, ImportFailure>,
        cookies: Result<zz_browser::CookieImportBatch, ImportFailure>,
    },
}

struct PendingCookies {
    request_profile: String,
    result: Value,
    receiver: Receiver<zz_browser::CookieImportResult>,
}

impl ZzBrowserRuntime {
    fn request_profile(&self, profile: &str) -> Result<String, String> {
        self.egress.as_ref().map_or_else(
            || Ok(profile.to_owned()),
            |(host, _)| {
                BrowserProfilePaths::egress_profile_name(profile, host)
                    .map_err(|error| error.to_string())
            },
        )
    }

    fn next_job(&mut self) -> Option<u64> {
        if self.closing || self.pending_jobs + self.cookie_imports.len() >= 8 {
            return None;
        }
        let request_id = self.next_request_id;
        self.next_request_id = request_id.checked_add(1)?;
        self.pending_jobs += 1;
        Some(request_id)
    }

    fn pump_data(&mut self) {
        while let Ok(data) = self.data_rx.try_recv() {
            self.pending_jobs = self.pending_jobs.saturating_sub(1);
            match data {
                BrowserData::Profiles { request_id, result } => {
                    let payload = match result {
                        Ok(profiles) => {
                            json!({"kind":"chrome-profiles", "request_id":request_id, "profiles":profiles, "error":null})
                        }
                        Err(error) => {
                            json!({"kind":"chrome-profiles", "request_id":request_id, "profiles":[], "error":error})
                        }
                    };
                    self.enqueue(payload, None);
                }
                BrowserData::Import {
                    request_id,
                    profile,
                    request_profile,
                    history,
                    cookies,
                } => {
                    let mut result = json!({
                        "kind":"chrome-import", "request_id":request_id, "profile":profile,
                        "history_imported":0, "history_skipped":0, "cookies_imported":0,
                        "cookies_skipped":0, "cookies_rejected":0, "persisted":false,
                        "errors":[], "permission_denied":false
                    });
                    match history {
                        Ok(history) => {
                            result["history_skipped"] = json!(history.skipped);
                            let pages = history
                                .pages
                                .into_iter()
                                .map(|page| {
                                    RecentPage::imported(
                                        profile.clone(),
                                        page.url,
                                        page.title,
                                        page.visited_at,
                                        page.visit_count,
                                        page.typed_count,
                                    )
                                })
                                .collect();
                            result["history_imported"] = json!(self.history.import_history(pages));
                        }
                        Err(error) => import_error(&mut result, error),
                    }
                    match cookies {
                        Ok(batch) => match self.runtime.import_cookies(&request_profile, batch) {
                            Ok(receiver) => {
                                self.cookie_imports.push(PendingCookies {
                                    request_profile,
                                    result,
                                    receiver,
                                });
                                continue;
                            }
                            Err(error) => import_error(
                                &mut result,
                                ImportFailure {
                                    message: error.to_string(),
                                    permission_denied: false,
                                },
                            ),
                        },
                        Err(error) => import_error(&mut result, error),
                    }
                    self.enqueue(result, None);
                }
            }
        }
        let mut finished = Vec::new();
        let mut reload_profiles = Vec::new();
        self.cookie_imports.retain_mut(|pending| {
            match pending.receiver.try_recv() {
                Ok(cookies) => {
                    if cookies.imported > 0 {
                        reload_profiles.push(pending.request_profile.clone());
                    }
                    pending.result["cookies_imported"] = json!(cookies.imported);
                    pending.result["cookies_skipped"] = json!(cookies.skipped);
                    pending.result["cookies_rejected"] = json!(cookies.rejected);
                    pending.result["persisted"] = json!(cookies.persisted);
                    if !cookies.persisted {
                        import_error(
                            &mut pending.result,
                            ImportFailure {
                                message: "Chrome cookies were imported but could not be saved"
                                    .to_owned(),
                                permission_denied: false,
                            },
                        );
                    }
                }
                Err(async_channel::TryRecvError::Empty) => return true,
                Err(async_channel::TryRecvError::Closed) => import_error(
                    &mut pending.result,
                    ImportFailure {
                        message: "Chrome cookie import was interrupted".to_owned(),
                        permission_denied: false,
                    },
                ),
            }
            finished.push(pending.result.take());
            false
        });
        for session in self.sessions.values_mut() {
            if reload_profiles
                .iter()
                .any(|profile| profile == session.browser.profile())
            {
                session.omnibox = None;
                session.browser.reload();
            }
        }
        for result in finished {
            self.enqueue(result, None);
        }
    }
}

fn import_error(result: &mut Value, error: ImportFailure) {
    if error.permission_denied {
        result["permission_denied"] = json!(true);
    }
    if let Some(errors) = result["errors"].as_array_mut() {
        errors.push(json!(error.message));
    }
}

fn egress_route(
    endpoint: &str,
    enabled: bool,
    socks_port: Option<u16>,
) -> Result<Option<(String, u16)>, String> {
    if !enabled {
        return Ok(None);
    }
    match zz_daemon::Endpoint::parse(endpoint).map_err(|error| error.to_string())? {
        zz_daemon::Endpoint::Local(_) => Ok(None),
        zz_daemon::Endpoint::Ssh(mut endpoint) => {
            endpoint.remote_socket = None;
            Ok(socks_port.map(|port| {
                (
                    endpoint.to_string().trim_start_matches("ssh://").to_owned(),
                    port,
                )
            }))
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_browser_runtime_set_egress(
    runtime: *mut ZzBrowserRuntime,
    client: *const ZzClient,
    endpoint: *const c_char,
    enabled: bool,
) -> bool {
    let Some(runtime) = (unsafe { runtime.as_mut() }) else {
        return false;
    };
    let endpoint = unsafe { string(endpoint) }.unwrap_or_default();
    let socks_port = unsafe { client.as_ref() }.and_then(|client| client.client.socks_port());
    let route = match egress_route(endpoint, enabled, socks_port) {
        Ok(route) => route,
        Err(error) => {
            runtime.error = error;
            return false;
        }
    };
    runtime.error.clear();
    if runtime.egress == route {
        return false;
    }
    runtime.egress = route;
    true
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_browser_history_suggestions(
    runtime: *const ZzBrowserRuntime,
    profile: *const c_char,
    input: *const c_char,
    limit: usize,
) -> *mut ZzJson {
    let (Some(runtime), Some(profile), Some(input)) =
        (unsafe { (runtime.as_ref(), string(profile), string(input)) })
    else {
        return std::ptr::null_mut();
    };
    let Ok(profile) = zz_browser::normalize_browser_profile_name(profile) else {
        return std::ptr::null_mut();
    };
    let values: Vec<Value> = runtime.history.suggestions(&profile, input, limit.min(50)).into_iter().map(|suggestion| json!({
        "url":suggestion.url,"title":suggestion.title,"display_url":suggestion.display_url,"inline_completion":suggestion.inline_completion,
    })).collect();
    Box::into_raw(Box::new(ZzJson::new(json!(values))))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_browser_history_recent(
    runtime: *const ZzBrowserRuntime,
    profile: *const c_char,
    limit: usize,
) -> *mut ZzJson {
    let (Some(runtime), Some(profile)) = (unsafe { (runtime.as_ref(), string(profile)) }) else {
        return std::ptr::null_mut();
    };
    let Ok(profile) = zz_browser::normalize_browser_profile_name(profile) else {
        return std::ptr::null_mut();
    };
    let values: Vec<Value> = runtime.history.recent(&profile, limit.min(500)).into_iter().map(|entry| json!({
        "url":entry.url,"title":entry.title,"display_url":entry.url,"inline_completion":null,
    })).collect();
    Box::into_raw(Box::new(ZzJson::new(json!(values))))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_browser_history_remove(
    runtime: *mut ZzBrowserRuntime,
    profile: *const c_char,
    url: *const c_char,
) -> bool {
    let (Some(runtime), Some(profile), Some(url)) =
        (unsafe { (runtime.as_mut(), string(profile), string(url)) })
    else {
        return false;
    };
    let Ok(profile) = zz_browser::normalize_browser_profile_name(profile) else {
        return false;
    };
    runtime.history.remove(&profile, url)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_browser_chrome_profiles(runtime: *mut ZzBrowserRuntime) -> u64 {
    let Some(runtime) = (unsafe { runtime.as_mut() }) else {
        return 0;
    };
    let Some(request_id) = runtime.next_job() else {
        return 0;
    };
    let sender = runtime.data_tx.clone();
    if let Err(error) = std::thread::Builder::new()
        .name("zz-native-chrome-profiles".to_owned())
        .spawn(move || {
            let result =
                zz_chrome_import::profiles::discover_profiles().map_err(|error| error.to_string());
            let _ = sender.send_blocking(BrowserData::Profiles { request_id, result });
        })
    {
        runtime.pending_jobs -= 1;
        runtime.error = error.to_string();
        return 0;
    }
    request_id
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_browser_import_chrome(
    runtime: *mut ZzBrowserRuntime,
    session: u64,
    source_profile: *const c_char,
) -> u64 {
    let (Some(runtime), Some(source_profile)) =
        (unsafe { (runtime.as_mut(), string(source_profile)) })
    else {
        return 0;
    };
    let Ok(source_profile) = zz_browser::normalize_browser_profile_name(source_profile) else {
        return 0;
    };
    let Some(session) = runtime.sessions.get(&session) else {
        return 0;
    };
    let profile = session.profile.clone();
    let request_profile = session.browser.profile().to_owned();
    let Some(request_id) = runtime.next_job() else {
        return 0;
    };
    let sender = runtime.data_tx.clone();
    if let Err(error) =
        std::thread::Builder::new()
            .name("zz-native-chrome-import".to_owned())
            .spawn(move || {
                let history = zz_chrome_import::history::import_history(
                    &source_profile,
                    zz_chrome_import::history::ImportLimits {
                        max_count: zz_chrome_import::history::MAX_HISTORY_IMPORT_COUNT,
                        max_url_bytes: zz_chrome_import::recent_pages::MAX_URL_BYTES,
                        max_title_bytes: zz_chrome_import::recent_pages::MAX_TITLE_BYTES,
                    },
                )
                .map_err(|error| ImportFailure {
                    permission_denied: error.is_permission_denied(),
                    message: error.to_string(),
                });
                let cookies = zz_chrome_import::cookie::import_all_cookies(&source_profile)
                    .map_err(|error| ImportFailure {
                        permission_denied: error.is_permission_denied(),
                        message: error.to_string(),
                    });
                let _ = sender.send_blocking(BrowserData::Import {
                    request_id,
                    profile,
                    request_profile,
                    history,
                    cookies,
                });
            })
    {
        runtime.pending_jobs -= 1;
        runtime.error = error.to_string();
        return 0;
    }
    request_id
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn egress_uses_the_ssh_destination_and_current_forward_port() {
        let route = egress_route("ssh://alice@[::1]:2222/tmp/zz.sock", true, Some(1234)).unwrap();
        assert_eq!(route, Some(("alice@[::1]:2222".to_owned(), 1234)));
        assert_ne!(
            route,
            egress_route("ssh://alice@[::1]:2222/tmp/zz.sock", true, Some(5678)).unwrap()
        );
        assert_eq!(
            egress_route("ssh://alice@host", false, Some(1234)).unwrap(),
            None
        );
        assert_eq!(egress_route("ssh://alice@host", true, None).unwrap(), None);
        assert_eq!(
            egress_route("/tmp/zz.sock", true, Some(1234)).unwrap(),
            None
        );
        assert!(egress_route("ssh://", true, Some(1234)).is_err());
    }
}
