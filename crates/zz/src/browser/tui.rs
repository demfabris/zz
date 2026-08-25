use std::{
    collections::HashMap,
    ffi::OsStr,
    thread,
    time::{Duration, Instant},
};

use async_channel::Receiver;
use zz_browser::{
    BrowserEvent, BrowserKey, BrowserProfilePaths, BrowserRuntime, BrowserSession,
    KeyAction as BrowserKeyAction, KeyInput as BrowserKeyInput, Modifiers as BrowserModifiers,
    OsrFrame, PointerButton, PointerEvent, PointerPhase, RuntimePhase, RuntimeSignal, SessionPhase,
    Viewport, WheelEvent, normalize_browser_profile_name,
};
use zz_protocol::{BrowserCommand, BrowserDescriptor, KeyToken, MAX_BROWSER_KEY_REPEAT, PaneId};
use zz_terminal::{
    KeyAction as TerminalKeyAction, KeyCode as TerminalKeyCode, KeyInput as TerminalKeyInput,
};
use zz_tui::browser::{
    BrowserFrameProvider, ProviderFrame, ProviderModifiers, ProviderPointerButton,
    ProviderPointerInput, ProviderPointerPhase, ProviderTick,
};

const MESSAGE_PUMP_INTERVAL: Duration = Duration::from_millis(4);
fn browser_frame_rate() -> i32 {
    std::env::var("ZZ_TUI_BROWSER_FPS")
        .ok()
        .and_then(|value| value.parse::<i32>().ok())
        .unwrap_or(60)
        .clamp(1, 60)
}

fn browser_frame_interval() -> Duration {
    Duration::from_millis(1000 / u64::try_from(browser_frame_rate()).unwrap_or(60))
}
const CLOSE_TIMEOUT: Duration = Duration::from_secs(2);
const TUI_CACHE_ROOT: &str = "root-tui";
const DEFAULT_PROFILE_DIRECTORY: &str = "zz-default";

pub(crate) fn tui_profile_paths(paths: &BrowserProfilePaths) -> BrowserProfilePaths {
    let profile_directory = paths
        .profile
        .file_name()
        .unwrap_or_else(|| OsStr::new(DEFAULT_PROFILE_DIRECTORY));
    let root = paths.root.with_file_name(TUI_CACHE_ROOT);
    let profile = root.join(profile_directory);
    BrowserProfilePaths { root, profile }
}

pub(crate) struct TuiBrowserProvider {
    runtime: BrowserRuntime,
    signals: Receiver<RuntimeSignal>,
    surfaces: HashMap<PaneId, BrowserSurface>,
    closing: Vec<LiveSession>,
    focused_pane: Option<PaneId>,
    external_begin_frames: bool,
    runtime_error: Option<String>,
}

struct BrowserSurface {
    descriptor: BrowserDescriptor,
    profile: Option<String>,
    viewport: Viewport,
    requested_url: Option<String>,
    session: Option<LiveSession>,
    failed: bool,
}

struct LiveSession {
    browser: BrowserSession,
    events: Receiver<BrowserEvent>,
    next_begin_frame: Instant,
}

impl LiveSession {
    fn new(browser: BrowserSession, now: Instant) -> Self {
        let events = browser.events();
        Self {
            browser,
            events,
            next_begin_frame: now,
        }
    }
}

impl TuiBrowserProvider {
    pub(crate) fn new(runtime: BrowserRuntime) -> Self {
        let signals = runtime.signals();
        let external_begin_frames = runtime.external_begin_frame_enabled();
        Self {
            runtime,
            signals,
            surfaces: HashMap::new(),
            closing: Vec::new(),
            focused_pane: None,
            external_begin_frames,
            runtime_error: None,
        }
    }

    fn ensure_runtime_started(&mut self) {
        if self.runtime_error.is_some() {
            return;
        }
        match self.runtime.phase() {
            RuntimePhase::Uninitialized => {
                if let Err(error) = self.runtime.start() {
                    self.fail_runtime(error.to_string());
                }
            }
            RuntimePhase::Initializing | RuntimePhase::Running => {}
            RuntimePhase::Closing | RuntimePhase::Closed | RuntimePhase::Failed => {
                self.fail_runtime("CEF browser runtime is unavailable".to_owned());
            }
        }
    }

    fn fail_runtime(&mut self, message: String) {
        if self.runtime_error.is_none() {
            log::error!(target: "zz::browser::tui", "{message}");
            self.runtime_error = Some(message);
        }
    }

    fn pump_runtime(&mut self) {
        self.runtime.do_message_loop_work();
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
                self.fail_runtime(error.to_string());
            }
        }
    }

    fn try_create_sessions(&mut self, now: Instant) {
        if self.runtime_error.is_some() || self.runtime.phase() != RuntimePhase::Running {
            return;
        }
        let pending = self
            .surfaces
            .iter()
            .filter_map(|(pane, surface)| {
                (surface.session.is_none() && !surface.failed).then_some(*pane)
            })
            .collect::<Vec<_>>();

        for pane in pending {
            let Some((profile, url, viewport)) = self.surfaces.get(&pane).and_then(|surface| {
                Some((
                    surface.profile.clone()?,
                    requested_or_active_url(surface).to_owned(),
                    surface.viewport,
                ))
            }) else {
                continue;
            };

            match self.runtime.ensure_profile_context(&profile) {
                Ok(true) => {}
                Ok(false) => continue,
                Err(error) => {
                    self.fail_surface(pane, &error);
                    continue;
                }
            }

            match self.runtime.create_session(
                &profile,
                &url,
                viewport,
                1.0,
                Some(browser_frame_rate()),
                None,
                false,
            ) {
                Ok(session) => {
                    session.set_focus(self.focused_pane == Some(pane));
                    if let Some(surface) = self.surfaces.get_mut(&pane) {
                        surface.session = Some(LiveSession::new(session, now));
                    }
                }
                Err(error) => self.fail_surface(pane, &error),
            }
        }
    }

    fn fail_surface(&mut self, pane: PaneId, message: &impl std::fmt::Display) {
        log::error!(target: "zz::browser::tui", "browser pane {pane}: {message}");
        if let Some(surface) = self.surfaces.get_mut(&pane) {
            surface.failed = true;
        }
    }

    fn focus(&mut self, pane: PaneId) {
        if self.focused_pane == Some(pane) {
            return;
        }
        self.focused_pane = Some(pane);
        for (candidate, surface) in &self.surfaces {
            if let Some(session) = surface.session.as_ref() {
                session.browser.set_focus(*candidate == pane);
            }
        }
    }

    fn send_due_external_begin_frames(&mut self, now: Instant) {
        if !self.external_begin_frames {
            return;
        }
        for surface in self.surfaces.values_mut() {
            let Some(session) = surface.session.as_mut() else {
                continue;
            };
            if session.next_begin_frame <= now {
                session.browser.send_external_begin_frame();
                session.next_begin_frame = deadline(now, browser_frame_interval());
            }
        }
    }

    #[allow(
        clippy::match_wildcard_for_single_variants,
        reason = "every non-BGRA tier is equally unexpected, platform and future variants included"
    )]
    fn drain_surfaces(&mut self) -> ProviderTick {
        let panes = self.surfaces.keys().copied().collect::<Vec<_>>();
        let mut frames = Vec::new();
        let mut navigations = Vec::new();

        for pane in panes {
            let Some(surface) = self.surfaces.get_mut(&pane) else {
                continue;
            };
            let Some(session) = surface.session.as_mut() else {
                continue;
            };
            let mut closed = false;
            while let Ok(event) = session.events.try_recv() {
                if event.session() != session.browser.id() {
                    continue;
                }
                match event {
                    BrowserEvent::Created { .. } => session.browser.mark_ready(),
                    BrowserEvent::AddressChanged { url, .. } => {
                        surface.requested_url = None;
                        if update_active_url(&mut surface.descriptor, &url) {
                            record_navigation(pane, &surface.descriptor, &mut navigations);
                        }
                    }
                    BrowserEvent::PopupRequested {
                        url, foreground, ..
                    } => {
                        let url = browser_url(&url);
                        append_popup(&mut surface.descriptor, url.clone(), foreground);
                        if foreground {
                            session.browser.navigate(&url);
                        }
                        record_navigation(pane, &surface.descriptor, &mut navigations);
                    }
                    BrowserEvent::RenderProcessTerminated { .. } => {
                        session.browser.mark_crashed();
                    }
                    BrowserEvent::Closed { .. } => {
                        session.browser.mark_closed();
                        closed = true;
                    }
                    BrowserEvent::SharedTextureFailed { reason, .. } => {
                        log::error!(
                            target: "zz::browser::tui",
                            "readback-only browser pane {pane} reported a shared-texture failure: {reason}"
                        );
                    }
                    BrowserEvent::LoadFailed {
                        code, description, ..
                    } => {
                        log::debug!(
                            target: "zz::browser::tui",
                            "browser pane {pane} load failed ({code}): {description}"
                        );
                    }
                    BrowserEvent::TitleChanged { .. }
                    | BrowserEvent::LoadingChanged { .. }
                    | BrowserEvent::FrameReady { .. }
                    | BrowserEvent::CursorChanged { .. }
                    | BrowserEvent::ElementPicked { .. }
                    | BrowserEvent::ElementPickCancelled { .. }
                    | BrowserEvent::ElementPickFailed { .. }
                    | BrowserEvent::ContextMenuRequested { .. } => {}
                }
            }

            if !closed && let Some(frame) = session.browser.take_frame() {
                match frame {
                    OsrFrame::OwnedBgra(frame) => frames.push((
                        pane,
                        ProviderFrame {
                            width: frame.width,
                            height: frame.height,
                            premultiplied_bgra: frame.bgra,
                            damage: frame
                                .damage
                                .map(|damage| (damage.x, damage.y, damage.width, damage.height)),
                        },
                    )),
                    frame => log::error!(
                        target: "zz::browser::tui",
                        "discarding unexpected {:?} frame for readback-only browser pane {pane}",
                        frame.tier()
                    ),
                }
            }
            if closed {
                surface.session.take();
                surface.failed = false;
            }
        }

        ProviderTick {
            frames,
            navigations,
            next_due: Some(MESSAGE_PUMP_INTERVAL),
        }
    }

    fn retire(&mut self, mut session: LiveSession) {
        let mut viewport = session.browser.viewport();
        viewport.visible = false;
        session.browser.set_viewport(viewport);
        session.browser.close(true);
        self.closing.push(session);
    }

    fn drain_closing(&mut self) {
        self.closing.retain_mut(|session| {
            while let Ok(event) = session.events.try_recv() {
                if event.session() != session.browser.id() {
                    continue;
                }
                match event {
                    BrowserEvent::Created { .. } => session.browser.mark_ready(),
                    BrowserEvent::RenderProcessTerminated { .. } => {
                        session.browser.mark_crashed();
                    }
                    BrowserEvent::Closed { .. } => session.browser.mark_closed(),
                    BrowserEvent::AddressChanged { .. }
                    | BrowserEvent::TitleChanged { .. }
                    | BrowserEvent::LoadingChanged { .. }
                    | BrowserEvent::FrameReady { .. }
                    | BrowserEvent::SharedTextureFailed { .. }
                    | BrowserEvent::LoadFailed { .. }
                    | BrowserEvent::CursorChanged { .. }
                    | BrowserEvent::ElementPicked { .. }
                    | BrowserEvent::ElementPickCancelled { .. }
                    | BrowserEvent::ElementPickFailed { .. }
                    | BrowserEvent::ContextMenuRequested { .. }
                    | BrowserEvent::PopupRequested { .. } => {}
                }
            }
            if let Some(OsrFrame::OwnedBgra(frame)) = session.browser.take_frame() {
                session.browser.recycle_frame(frame.bgra);
            }
            session.browser.phase() != SessionPhase::Closed
        });
    }

    fn finish_closing(&mut self) {
        let timeout = deadline(Instant::now(), CLOSE_TIMEOUT);
        while !self.closing.is_empty() {
            self.pump_runtime();
            self.drain_closing();
            if self.closing.is_empty() {
                return;
            }
            if Instant::now() >= timeout {
                log::error!(
                    target: "zz::browser::tui",
                    "CEF browser sessions did not close before the TUI shutdown deadline"
                );
                return;
            }
            thread::sleep(MESSAGE_PUMP_INTERVAL);
        }
    }

    fn close_surfaces(&mut self) {
        let surfaces = std::mem::take(&mut self.surfaces);
        for mut surface in surfaces.into_values() {
            if let Some(session) = surface.session.take() {
                self.retire(session);
            }
        }
        self.focused_pane = None;
        self.finish_closing();
    }
}

impl BrowserFrameProvider for TuiBrowserProvider {
    fn open(&mut self, pane: PaneId, descriptor: &BrowserDescriptor, px: (u32, u32), scale: f32) {
        self.ensure_runtime_started();
        let profile = match normalize_browser_profile_name(&descriptor.profile) {
            Ok(profile) => Some(profile),
            Err(error) => {
                log::error!(target: "zz::browser::tui", "browser pane {pane}: {error}");
                None
            }
        };
        let viewport = browser_viewport(px, scale);

        let Some(mut surface) = self.surfaces.remove(&pane) else {
            self.surfaces.insert(
                pane,
                BrowserSurface {
                    descriptor: descriptor.clone(),
                    failed: profile.is_none(),
                    profile,
                    viewport,
                    requested_url: None,
                    session: None,
                },
            );
            self.try_create_sessions(Instant::now());
            return;
        };

        let profile_changed = surface.profile != profile;
        let active_url_changed = surface.descriptor.url() != descriptor.url();
        if profile_changed && let Some(session) = surface.session.take() {
            self.retire(session);
        }
        surface.descriptor = descriptor.clone();
        surface.profile = profile;
        surface.viewport = viewport;
        surface.failed = surface.profile.is_none();
        if profile_changed || active_url_changed {
            surface.requested_url = None;
        }
        if !profile_changed && let Some(session) = surface.session.as_mut() {
            session.browser.set_viewport(viewport);
            if active_url_changed {
                session
                    .browser
                    .navigate(browser_url(descriptor.url()).as_str());
            }
        }
        self.surfaces.insert(pane, surface);
        self.try_create_sessions(Instant::now());
    }

    fn resize(&mut self, pane: PaneId, px: (u32, u32), scale: f32) {
        let Some(surface) = self.surfaces.get_mut(&pane) else {
            return;
        };
        surface.viewport = browser_viewport(px, scale);
        if let Some(session) = surface.session.as_mut() {
            session.browser.set_viewport(surface.viewport);
        }
    }

    fn close(&mut self, pane: PaneId) {
        if let Some(mut surface) = self.surfaces.remove(&pane)
            && let Some(session) = surface.session.take()
        {
            self.retire(session);
        }
        if self.focused_pane == Some(pane) {
            self.focused_pane = None;
        }
        if self.surfaces.is_empty() {
            self.finish_closing();
        }
    }

    fn close_all(&mut self) {
        self.close_surfaces();
    }

    fn pointer(&mut self, pane: PaneId, input: ProviderPointerInput) {
        if input.phase == ProviderPointerPhase::Down {
            self.focus(pane);
        }
        let Some(surface) = self.surfaces.get(&pane) else {
            return;
        };
        let Some(session) = surface.session.as_ref() else {
            return;
        };
        let input = scale_pointer_input(input, surface.viewport.scale_factor);
        match browser_pointer_input(input) {
            BrowserPointerInput::Pointer(event) => session.browser.send_pointer(event),
            BrowserPointerInput::Wheel(event) => session.browser.send_wheel(event),
        }
    }

    fn command(&mut self, pane: PaneId, command: &BrowserCommand) {
        if matches!(
            command,
            BrowserCommand::SendKeys(_)
                | BrowserCommand::SendKeysRepeated { .. }
                | BrowserCommand::Key(_)
        ) {
            self.focus(pane);
        }
        let Some(surface) = self.surfaces.get_mut(&pane) else {
            return;
        };
        match command {
            BrowserCommand::Navigate(url) => {
                let url = browser_url(url);
                surface.requested_url = Some(url.clone());
                if let Some(session) = surface.session.as_ref() {
                    session.browser.navigate(&url);
                }
            }
            BrowserCommand::Reload => {
                if let Some(session) = surface.session.as_ref() {
                    session.browser.reload();
                }
            }
            BrowserCommand::Back => {
                if let Some(session) = surface.session.as_ref() {
                    session.browser.go_back();
                }
            }
            BrowserCommand::Forward => {
                if let Some(session) = surface.session.as_ref() {
                    session.browser.go_forward();
                }
            }
            BrowserCommand::SendKeys(tokens) => {
                let Some(session) = surface.session.as_ref() else {
                    return;
                };
                for token in tokens {
                    match token {
                        KeyToken::Literal(text) => session.browser.send_text(text),
                        KeyToken::Named(name) => {
                            if let Some(input) = browser_named_key(name) {
                                session.browser.send_key(input);
                            }
                        }
                    }
                }
            }
            BrowserCommand::SendKeysRepeated { keys, count } => {
                let Some(session) = surface.session.as_ref() else {
                    return;
                };
                for _ in 0..(*count).min(MAX_BROWSER_KEY_REPEAT) {
                    for token in keys {
                        match token {
                            KeyToken::Literal(text) => session.browser.send_text(text),
                            KeyToken::Named(name) => {
                                if let Some(input) = browser_named_key(name) {
                                    session.browser.send_key(input);
                                }
                            }
                        }
                    }
                }
            }
            BrowserCommand::Key(input) => {
                if let Some(session) = surface.session.as_ref() {
                    session.browser.send_key(browser_input_from_terminal(input));
                }
            }
            BrowserCommand::Screenshot { .. } => {}
        }
    }

    fn pump(&mut self) -> ProviderTick {
        let now = Instant::now();
        self.send_due_external_begin_frames(now);
        self.pump_runtime();
        self.drain_closing();
        self.try_create_sessions(now);
        self.send_due_external_begin_frames(now);
        self.drain_surfaces()
    }
}

impl Drop for TuiBrowserProvider {
    fn drop(&mut self) {
        self.close_surfaces();
        if let Err(error) = self.runtime.shutdown() {
            log::error!(target: "zz::browser::tui", "could not shut down CEF: {error}");
        }
    }
}

fn requested_or_active_url(surface: &BrowserSurface) -> &str {
    surface
        .requested_url
        .as_deref()
        .unwrap_or_else(|| surface.descriptor.url())
}

fn browser_url(url: &str) -> String {
    if url.trim().is_empty() {
        "about:blank".to_owned()
    } else {
        url.to_owned()
    }
}

fn browser_viewport(px: (u32, u32), scale: f32) -> Viewport {
    let scale = if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        1.0
    };
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "dimensions are small positive integers divided by a clamped scale"
    )]
    Viewport {
        width: ((px.0 as f32 / scale).round() as u32).max(1),
        height: ((px.1 as f32 / scale).round() as u32).max(1),
        scale_factor: scale,
        window_zoom: 1.0,
        screen_x: 0,
        screen_y: 0,
        visible: true,
    }
}

fn update_active_url(descriptor: &mut BrowserDescriptor, url: &str) -> bool {
    let url = browser_url(url);
    if descriptor.tabs.is_empty() {
        descriptor.tabs.push(url);
        descriptor.active_tab = 0;
        return true;
    }
    let mut changed = false;
    if descriptor.active_tab >= descriptor.tabs.len() {
        descriptor.active_tab = 0;
        changed = true;
    }
    let active = &mut descriptor.tabs[descriptor.active_tab];
    if *active != url {
        *active = url;
        changed = true;
    }
    changed
}

fn append_popup(descriptor: &mut BrowserDescriptor, url: String, foreground: bool) {
    descriptor.tabs.push(url);
    if foreground {
        descriptor.active_tab = descriptor.tabs.len().saturating_sub(1);
    } else if descriptor.active_tab >= descriptor.tabs.len() {
        descriptor.active_tab = 0;
    }
}

fn record_navigation(
    pane: PaneId,
    descriptor: &BrowserDescriptor,
    navigations: &mut Vec<(PaneId, Vec<String>, usize)>,
) {
    let navigation = (pane, descriptor.tabs.clone(), descriptor.active_tab);
    if let Some(existing) = navigations
        .iter_mut()
        .find(|(candidate, _, _)| *candidate == pane)
    {
        *existing = navigation;
    } else {
        navigations.push(navigation);
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum BrowserPointerInput {
    Pointer(PointerEvent),
    Wheel(WheelEvent),
}

fn scale_pointer_input(input: ProviderPointerInput, scale: f32) -> ProviderPointerInput {
    if !scale.is_finite() || scale <= 0.0 || (scale - 1.0).abs() < f32::EPSILON {
        return input;
    }
    #[allow(
        clippy::cast_possible_truncation,
        reason = "pane-local pointer offsets divided by a small positive scale"
    )]
    ProviderPointerInput {
        x: (input.x as f32 / scale).round() as i32,
        y: (input.y as f32 / scale).round() as i32,
        ..input
    }
}

fn browser_pointer_input(input: ProviderPointerInput) -> BrowserPointerInput {
    let button = input.button.map(browser_pointer_button);
    let pressed = match input.phase {
        ProviderPointerPhase::Move | ProviderPointerPhase::Down => button,
        ProviderPointerPhase::Up | ProviderPointerPhase::Wheel => None,
    };
    let modifiers = browser_modifiers(input.modifiers, pressed, false);
    if input.phase == ProviderPointerPhase::Wheel {
        return BrowserPointerInput::Wheel(WheelEvent {
            x: input.x,
            y: input.y,
            delta_x: input.wheel_delta_x,
            delta_y: input.wheel_delta_y,
            precise: false,
            modifiers,
        });
    }
    BrowserPointerInput::Pointer(PointerEvent {
        x: input.x,
        y: input.y,
        phase: match input.phase {
            ProviderPointerPhase::Move => PointerPhase::Move,
            ProviderPointerPhase::Down => PointerPhase::Down,
            ProviderPointerPhase::Up => PointerPhase::Up,
            ProviderPointerPhase::Wheel => unreachable!("wheel returned above"),
        },
        button,
        click_count: input.click_count,
        modifiers,
    })
}

const fn browser_pointer_button(button: ProviderPointerButton) -> PointerButton {
    match button {
        ProviderPointerButton::Left => PointerButton::Left,
        ProviderPointerButton::Middle => PointerButton::Middle,
        ProviderPointerButton::Right => PointerButton::Right,
    }
}

fn browser_input_from_terminal(input: &TerminalKeyInput) -> BrowserKeyInput {
    BrowserKeyInput {
        action: match input.action {
            TerminalKeyAction::Press | TerminalKeyAction::Repeat => BrowserKeyAction::Press,
            TerminalKeyAction::Release => BrowserKeyAction::Release,
        },
        key: match input.key {
            TerminalKeyCode::Character(character) => BrowserKey::Character(character),
            TerminalKeyCode::Backspace => BrowserKey::Backspace,
            TerminalKeyCode::Enter => BrowserKey::Enter,
            TerminalKeyCode::Tab => BrowserKey::Tab,
            TerminalKeyCode::Escape => BrowserKey::Escape,
            TerminalKeyCode::Delete => BrowserKey::Delete,
            TerminalKeyCode::Insert => BrowserKey::Insert,
            TerminalKeyCode::Home => BrowserKey::Home,
            TerminalKeyCode::End => BrowserKey::End,
            TerminalKeyCode::PageUp => BrowserKey::PageUp,
            TerminalKeyCode::PageDown => BrowserKey::PageDown,
            TerminalKeyCode::ArrowUp => BrowserKey::ArrowUp,
            TerminalKeyCode::ArrowDown => BrowserKey::ArrowDown,
            TerminalKeyCode::ArrowLeft => BrowserKey::ArrowLeft,
            TerminalKeyCode::ArrowRight => BrowserKey::ArrowRight,
            TerminalKeyCode::Function(number) => BrowserKey::Function(number),
            TerminalKeyCode::Unidentified => BrowserKey::Unidentified,
        },
        modifiers: browser_modifiers(
            ProviderModifiers::new(
                input.modifiers.shift(),
                input.modifiers.control(),
                input.modifiers.alt(),
                input.modifiers.platform(),
            ),
            None,
            input.action == TerminalKeyAction::Repeat,
        ),
    }
}

fn browser_modifiers(
    modifiers: ProviderModifiers,
    pressed: Option<PointerButton>,
    repeat: bool,
) -> BrowserModifiers {
    BrowserModifiers::new(
        modifiers.shift(),
        modifiers.control(),
        modifiers.alt(),
        modifiers.platform(),
    )
    .with_pointer_button(pressed)
    .with_repeat(repeat)
}

fn browser_named_key(name: &str) -> Option<BrowserKeyInput> {
    let mut modifiers = BrowserModifiers::default();
    let mut name = name;
    loop {
        if let Some(rest) = name.strip_prefix("C-") {
            modifiers.set_control(true);
            name = rest;
        } else if let Some(rest) = name.strip_prefix("M-") {
            modifiers.set_alt(true);
            name = rest;
        } else {
            break;
        }
    }
    let key = match name {
        "Enter" => BrowserKey::Enter,
        "Escape" => BrowserKey::Escape,
        "Space" => BrowserKey::Space,
        "Tab" => BrowserKey::Tab,
        "BSpace" => BrowserKey::Backspace,
        "Up" => BrowserKey::ArrowUp,
        "Down" => BrowserKey::ArrowDown,
        "Left" => BrowserKey::ArrowLeft,
        "Right" => BrowserKey::ArrowRight,
        "Home" => BrowserKey::Home,
        "End" => BrowserKey::End,
        "PPage" => BrowserKey::PageUp,
        "NPage" => BrowserKey::PageDown,
        "DC" => BrowserKey::Delete,
        "IC" => BrowserKey::Insert,
        value if value.chars().count() == 1 => BrowserKey::Character(value.chars().next()?),
        value => BrowserKey::Function(value.strip_prefix('F')?.parse().ok()?),
    };
    Some(BrowserKeyInput {
        action: BrowserKeyAction::Press,
        key,
        modifiers,
    })
}

fn deadline(now: Instant, after: Duration) -> Instant {
    now.checked_add(after).unwrap_or(now)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use zz_terminal::Modifiers as TerminalModifiers;

    fn descriptor() -> BrowserDescriptor {
        BrowserDescriptor {
            tabs: vec!["https://one.test/".to_owned()],
            active_tab: 0,
            profile: "default".to_owned(),
        }
    }

    #[test]
    fn tui_cache_root_keeps_the_default_profile_as_an_immediate_child() {
        let paths = tui_profile_paths(&BrowserProfilePaths {
            root: PathBuf::from("app-data/zz/browser/root"),
            profile: PathBuf::from("app-data/zz/browser/root/zz-default"),
        });
        assert_eq!(paths.root, PathBuf::from("app-data/zz/browser/root-tui"));
        assert_eq!(
            paths.profile,
            PathBuf::from("app-data/zz/browser/root-tui/zz-default")
        );
        assert_eq!(paths.profile.parent(), Some(paths.root.as_path()));
    }

    #[test]
    fn viewport_uses_the_outer_terminals_exact_pixel_size() {
        assert_eq!(browser_viewport((960, 640), 0.0).scale_factor, 1.0);
        assert_eq!(browser_viewport((960, 640), 2.5).scale_factor, 2.5);
        assert_eq!(
            browser_viewport((960, 640), 1.0),
            Viewport {
                width: 960,
                height: 640,
                scale_factor: 1.0,
                window_zoom: 1.0,
                screen_x: 0,
                screen_y: 0,
                visible: true,
            }
        );
    }

    #[test]
    fn address_and_popup_events_reduce_to_full_tab_descriptors() {
        let mut descriptor = descriptor();
        assert!(update_active_url(&mut descriptor, "https://two.test/"));
        assert!(!update_active_url(&mut descriptor, "https://two.test/"));
        append_popup(
            &mut descriptor,
            "https://background.test/".to_owned(),
            false,
        );
        assert_eq!(descriptor.active_tab, 0);
        append_popup(&mut descriptor, "https://foreground.test/".to_owned(), true);
        assert_eq!(descriptor.active_tab, 2);
        assert_eq!(
            descriptor.tabs,
            [
                "https://two.test/",
                "https://background.test/",
                "https://foreground.test/",
            ]
        );

        let mut navigations = Vec::new();
        record_navigation(PaneId(7), &descriptor, &mut navigations);
        descriptor.tabs[2] = "https://settled.test/".to_owned();
        record_navigation(PaneId(7), &descriptor, &mut navigations);
        assert_eq!(navigations.len(), 1);
        assert_eq!(navigations[0].1[2], "https://settled.test/");
    }

    #[test]
    fn terminal_keys_preserve_actions_modifiers_and_repeat() {
        let input = TerminalKeyInput {
            action: TerminalKeyAction::Repeat,
            key: TerminalKeyCode::ArrowLeft,
            modifiers: TerminalModifiers::new(true, true, false, true),
            text: None,
            unshifted_codepoint: None,
        };
        let mapped = browser_input_from_terminal(&input);
        assert_eq!(mapped.action, BrowserKeyAction::Press);
        assert_eq!(mapped.key, BrowserKey::ArrowLeft);
        assert!(mapped.modifiers.shift());
        assert!(mapped.modifiers.control());
        assert!(mapped.modifiers.platform());
        assert!(mapped.modifiers.is_repeat());

        let release = browser_input_from_terminal(&TerminalKeyInput {
            action: TerminalKeyAction::Release,
            ..input
        });
        assert_eq!(release.action, BrowserKeyAction::Release);
        assert!(!release.modifiers.is_repeat());

        let named = browser_named_key("C-M-F12").expect("supported named key");
        assert_eq!(named.key, BrowserKey::Function(12));
        assert!(named.modifiers.control());
        assert!(named.modifiers.alt());
    }

    #[test]
    fn pointer_coordinates_scale_to_logical_but_wheel_notches_do_not() {
        let input = ProviderPointerInput {
            x: 300,
            y: 90,
            phase: ProviderPointerPhase::Down,
            button: Some(ProviderPointerButton::Left),
            click_count: 1,
            wheel_delta_x: 0,
            wheel_delta_y: -120,
            modifiers: ProviderModifiers::default(),
        };
        let scaled = scale_pointer_input(input, 3.0);
        assert_eq!((scaled.x, scaled.y), (100, 30));
        assert_eq!(scaled.wheel_delta_y, -120, "notches are not offsets");
        let unscaled = scale_pointer_input(input, 1.0);
        assert_eq!((unscaled.x, unscaled.y), (300, 90));
        let garbage = scale_pointer_input(input, f32::NAN);
        assert_eq!((garbage.x, garbage.y), (300, 90));
    }

    #[test]
    fn pointer_up_clears_the_pressed_button_and_wheel_stays_discrete() {
        let input = ProviderPointerInput {
            x: 12,
            y: 20,
            phase: ProviderPointerPhase::Down,
            button: Some(ProviderPointerButton::Left),
            click_count: 1,
            wheel_delta_x: 0,
            wheel_delta_y: 0,
            modifiers: ProviderModifiers::new(true, false, false, false),
        };
        let BrowserPointerInput::Pointer(down) = browser_pointer_input(input) else {
            panic!("down must map to a pointer event");
        };
        assert!(down.modifiers.left_mouse());
        assert!(down.modifiers.shift());

        let BrowserPointerInput::Pointer(up) = browser_pointer_input(ProviderPointerInput {
            phase: ProviderPointerPhase::Up,
            ..input
        }) else {
            panic!("up must map to a pointer event");
        };
        assert_eq!(up.button, Some(PointerButton::Left));
        assert!(!up.modifiers.left_mouse());

        let BrowserPointerInput::Wheel(wheel) = browser_pointer_input(ProviderPointerInput {
            phase: ProviderPointerPhase::Wheel,
            button: None,
            wheel_delta_y: -120,
            ..input
        }) else {
            panic!("wheel must map to a wheel event");
        };
        assert_eq!(wheel.delta_y, -120);
        assert!(!wheel.precise);
    }
}
