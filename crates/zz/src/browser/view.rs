use std::{
    collections::{HashMap, HashSet},
    fs::File,
    io::Read as _,
    ops::Range,
    path::Path,
    sync::Arc,
    time::Instant,
};

#[cfg(target_os = "macos")]
use core_video::pixel_buffer::CVPixelBuffer;
#[cfg(target_os = "macos")]
use gpui::PromptLevel;
use gpui::{
    Anchor, AnyElement, AnyView, App, Bounds, ClipboardEntry, ClipboardItem, ClipboardString,
    Context, Corners, CursorStyle, DismissEvent, Entity, EntityInputHandler, FocusHandle,
    Focusable, Image, ImageFormat, IntoElement, KeyBinding, KeyDownEvent, KeyUpEvent, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, NoAction, PathPromptOptions, Pixels, Point,
    Render, RenderImage, ScrollWheelEvent, SharedString, StyleRefinement, Subscription,
    UTF16Selection, WeakEntity, Window, anchored, deferred, div, point, prelude::*, px,
};
#[cfg(target_os = "windows")]
use gpui::{ObjectFit, external_texture};
#[cfg(any(target_os = "linux", target_os = "freebsd"))]
use gpui::{ObjectFit, external_texture, wgpu};
#[cfg(target_os = "windows")]
use zz_browser::WinGpuTexture;
use zz_browser::{
    BrowserCursor, BrowserEvent, BrowserGpuContext, BrowserKey, ContextMenuRequest,
    CookieImportBatch, EditCommand, ElementPickerAppearance, KeyAction, KeyInput,
    MAX_COOKIE_IMPORT_BYTES, Modifiers, PointerButton, PointerEvent, PointerPhase, RuntimePhase,
    SessionId, Viewport, WheelEvent, diagnostic_url, normalize_browser_profile_name, normalize_url,
    parse_cookie_import, resolve_address,
};
use zz_client::{BROWSER_TABLE, ChromeAction};
use zz_protocol::{
    BrowserCommand, BrowserDescriptor, ClientMessageKind, CommandInvocation, GuiResponse,
    InputMessage, KeyToken, PaneId,
};
use zz_terminal::{
    KeyAction as TerminalKeyAction, KeyCode as TerminalKeyCode, KeyInput as TerminalKeyInput,
};
use zz_ui::browser::{
    BrowserActionMenuState, BrowserEmptyHint, BrowserErrorPanel, BrowserMenuActions,
    BrowserMenuProfile, BrowserPickStatus, BrowserProfileDiscoveryState, BrowserTabInfo,
    BrowserTabStrip, BrowserToolbar, browser_action_menu, browser_omnibox_panel,
    browser_omnibox_row, browser_recent_row, browser_toolbar_button,
};
use zz_ui::feedback::browser_clear_site_data_alert;
use zz_ui::pane::frame_rate_badge;
use zz_ui::{
    ActiveTheme as _, Colorize as _, IconName, Sizable as _, WindowExt as _,
    button::{Button, ButtonVariants as _},
    input::{InputEvent, InputState},
    menu::{DropdownMenu as _, PopupMenu, PopupMenuItem},
    to_hex,
};

#[cfg(target_os = "macos")]
use crate::browser::macos_surface::MacBrowserSurfaceCache;
use zz_chrome_import as chrome_import;

use super::recent_pages::{self, HistorySuggestion, RecentPage};
use crate::{
    browser::controller::{
        BrowserController, BrowserPaneFrameContent, BrowserSessionRequest, ControllerEvent,
        EgressSpec, TabId,
    },
    browser::element::BrowserElement,
    browser::screenshot::Screenshot,
    config::{pane_content_radii, resolved_config},
    diagnostics,
    diagnostics::fps::{FPS_SAMPLE_INTERVAL, FrameRateSampler},
    keymap::ChromeChord,
    mux::{client::MuxClient, prefix::terminal_key_input},
    window::corners::{WindowCorners, round_div_radii},
    workspace::ClosePane,
};

const DEFAULT_URL: &str = "about:blank";
const TAB_ID: TabId = TabId(0);
const EMPTY_STATE_RECENT_LIMIT: usize = 8;
const OMNIBOX_SUGGESTION_LIMIT: usize = 8;
const BROWSER_KEY_CONTEXT: &str = "Browser";
const OMNIBOX_INPUT_KEY_CONTEXT: &str = "BrowserOmnibox > ZzInput";
#[cfg(target_os = "macos")]
const FULL_DISK_ACCESS_SETTINGS_URL: &str =
    "x-apple.systempreferences:com.apple.settings.PrivacySecurity.extension?Privacy_AllFiles";

gpui::actions!(
    browser,
    [
        ZoomIn,
        ZoomOut,
        ResetZoom,
        ToggleDevTools,
        NewTab,
        NextTab,
        PreviousTab,
        SelectLastTab,
        GoBack,
        GoForward,
        Reload,
        FocusAddress,
        OmniboxNext,
        OmniboxPrevious,
        OmniboxDelete
    ]
);

#[allow(clippy::unsafe_derive_deserialize)]
#[derive(Clone, Debug, gpui::Action, PartialEq, Eq, serde::Deserialize)]
#[action(namespace = browser, no_json)]
struct BrowserEdit {
    command: EditCommand,
}

#[allow(clippy::unsafe_derive_deserialize)]
#[derive(Clone, Debug, gpui::Action, PartialEq, Eq, serde::Deserialize)]
#[action(namespace = browser, no_json)]
struct SelectTab {
    index: usize,
}

#[allow(clippy::unsafe_derive_deserialize)]
#[derive(Clone, Debug, gpui::Action, PartialEq, Eq, serde::Deserialize)]
#[action(namespace = browser, no_json)]
struct ConfiguredElementSelector {
    chord: String,
}

pub fn init(cx: &mut App) {
    cx.bind_keys(raw_key_bindings());
    crate::keymap::bind(cx, BROWSER_TABLE, browser_chrome_bindings);
}

#[cfg(target_os = "macos")]
fn prompt_for_chrome_data_access(window: &mut Window, cx: &mut App) {
    let response = window.prompt(
        PromptLevel::Warning,
        "Allow access to Chrome data",
        Some(
            "macOS blocked zz from reading Chrome's protected data. Open Full Disk Access, enable zz, then quit and reopen zz before retrying the import.",
        ),
        &["Open Full Disk Access", "Cancel"],
        cx,
    );
    cx.spawn(async move |cx| {
        if response.await == Ok(0) {
            cx.update(|cx| cx.open_url(FULL_DISK_ACCESS_SETTINGS_URL));
        }
    })
    .detach();
}

/// Bindings a browser pane owns outright: the tabs it swallows so focus never
/// leaves the page, and the omnibox list keys, which belong to an input widget
/// rather than to chrome.
fn raw_key_bindings() -> [KeyBinding; 5] {
    [
        KeyBinding::new("tab", NoAction, Some(BROWSER_KEY_CONTEXT)),
        KeyBinding::new("shift-tab", NoAction, Some(BROWSER_KEY_CONTEXT)),
        KeyBinding::new("down", OmniboxNext, Some(OMNIBOX_INPUT_KEY_CONTEXT)),
        KeyBinding::new("up", OmniboxPrevious, Some(OMNIBOX_INPUT_KEY_CONTEXT)),
        KeyBinding::new(
            "shift-delete",
            OmniboxDelete,
            Some(OMNIBOX_INPUT_KEY_CONTEXT),
        ),
    ]
}

fn browser_chrome_bindings(chords: &[ChromeChord]) -> Vec<KeyBinding> {
    let context = Some(BROWSER_KEY_CONTEXT);
    chords
        .iter()
        .filter_map(|chord| {
            let edit = |command| BrowserEdit { command };
            Some(match chord.action() {
                ChromeAction::BrowserZoomIn => chord.binding(ZoomIn, context),
                ChromeAction::BrowserZoomOut => chord.binding(ZoomOut, context),
                ChromeAction::BrowserZoomReset => chord.binding(ResetZoom, context),
                ChromeAction::BrowserDevTools => chord.binding(ToggleDevTools, context),
                ChromeAction::BrowserNewTab => chord.binding(NewTab, context),
                ChromeAction::BrowserNextTab => chord.binding(NextTab, context),
                ChromeAction::BrowserPreviousTab => chord.binding(PreviousTab, context),
                ChromeAction::BrowserSelectTab(index) => chord.binding(
                    SelectTab {
                        index: usize::from(index),
                    },
                    context,
                ),
                ChromeAction::BrowserSelectLastTab => chord.binding(SelectLastTab, context),
                ChromeAction::BrowserBack => chord.binding(GoBack, context),
                ChromeAction::BrowserForward => chord.binding(GoForward, context),
                ChromeAction::BrowserReload => chord.binding(Reload, context),
                ChromeAction::BrowserFocusAddress => chord.binding(FocusAddress, context),
                ChromeAction::BrowserElementSelector => chord.binding(
                    ConfiguredElementSelector {
                        chord: chord.key().to_owned(),
                    },
                    context,
                ),
                ChromeAction::BrowserUndo => chord.binding(edit(EditCommand::Undo), context),
                ChromeAction::BrowserRedo => chord.binding(edit(EditCommand::Redo), context),
                ChromeAction::BrowserCut => chord.binding(edit(EditCommand::Cut), context),
                ChromeAction::BrowserCopy => chord.binding(edit(EditCommand::Copy), context),
                ChromeAction::BrowserPaste => chord.binding(edit(EditCommand::Paste), context),
                ChromeAction::BrowserPasteAndMatchStyle => {
                    chord.binding(edit(EditCommand::PasteAndMatchStyle), context)
                }
                ChromeAction::BrowserSelectAll => {
                    chord.binding(edit(EditCommand::SelectAll), context)
                }
                ChromeAction::ClosePane => chord.binding(ClosePane, context),
                _ => return None,
            })
        })
        .collect()
}

fn is_blank_url(url: &str) -> bool {
    url == "about:blank"
}

fn differs_by_one_character_edit(previous: &str, next: &str) -> bool {
    let previous = previous.chars().collect::<Vec<_>>();
    let next = next.chars().collect::<Vec<_>>();
    match previous.len().cmp(&next.len()) {
        std::cmp::Ordering::Equal => {
            previous
                .iter()
                .zip(&next)
                .filter(|(left, right)| left != right)
                .count()
                == 1
        }
        std::cmp::Ordering::Less if previous.len() + 1 == next.len() => {
            one_character_inserted(&previous, &next)
        }
        std::cmp::Ordering::Greater if next.len() + 1 == previous.len() => {
            one_character_inserted(&next, &previous)
        }
        std::cmp::Ordering::Less | std::cmp::Ordering::Greater => false,
    }
}

fn one_character_inserted(shorter: &[char], longer: &[char]) -> bool {
    let mismatch = shorter
        .iter()
        .zip(longer)
        .position(|(left, right)| left != right)
        .unwrap_or(shorter.len());
    shorter[mismatch..] == longer[mismatch + 1..]
}

fn tab_host_label(url: &str) -> SharedString {
    if url.is_empty() || is_blank_url(url) {
        return "New Tab".into();
    }
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    let host = rest.split(['/', '?', '#']).next().unwrap_or(rest);
    let host = host.trim_start_matches("www.");
    if host.is_empty() {
        "New Tab".into()
    } else {
        host.to_owned().into()
    }
}

#[derive(Clone)]
struct PendingHistoryUse {
    profile: String,
    input: String,
    selected: bool,
    started: bool,
}

#[derive(Default)]
struct OmniboxState {
    query: String,
    suggestions: Vec<HistorySuggestion>,
    selected: Option<usize>,
    previous_value: String,
    incremental_input: bool,
}

impl OmniboxState {
    fn begin(&mut self, value: &str) {
        self.reset();
        value.clone_into(&mut self.previous_value);
        self.incremental_input = true;
    }

    fn reset(&mut self) {
        self.query.clear();
        self.suggestions.clear();
        self.selected = None;
        self.previous_value.clear();
        self.incremental_input = false;
    }
}

#[allow(
    clippy::struct_excessive_bools,
    reason = "independent loading, history-direction, and session-lifecycle states"
)]
struct BrowserTab {
    id: TabId,
    url: String,
    title: String,
    loading: bool,
    can_go_back: bool,
    can_go_forward: bool,
    page_zoom_factor: f64,
    page_zoom_percent: u16,
    error: Option<Arc<str>>,
    started: bool,
}

impl BrowserTab {
    fn new(id: TabId, url: String) -> Self {
        Self {
            id,
            url,
            title: String::new(),
            loading: false,
            can_go_back: false,
            can_go_forward: false,
            page_zoom_factor: 1.0,
            page_zoom_percent: 100,
            error: None,
            started: true,
        }
    }

    fn restored(id: TabId, url: String) -> Self {
        Self {
            started: false,
            ..Self::new(id, url)
        }
    }
}

const DIAGNOSTIC_TARGET: &str = "zz::diagnostics::browser_render";

#[derive(Clone, Copy, PartialEq, Eq)]
enum ChromeProfileDiscovery {
    NotStarted,
    Loading,
    Loaded,
    Failed,
}

#[derive(Clone, PartialEq)]
struct ChromeState {
    can_go_back: bool,
    can_go_forward: bool,
    element_pick_active: bool,
    current_url: String,
    profile: String,
    chrome_profiles: Vec<chrome_import::profiles::DetectedChromeProfile>,
    chrome_profile_discovery: ChromeProfileDiscovery,
    page_zoom_percent: u16,
    tabs: Vec<BrowserTabInfo>,
    active_tab_index: usize,
}

struct BrowserChromeView {
    browser: WeakEntity<BrowserView>,
    address: Entity<InputState>,
    state: ChromeState,
}

impl BrowserChromeView {
    fn new(
        browser: WeakEntity<BrowserView>,
        address: Entity<InputState>,
        state: ChromeState,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.observe(&address, |_, _, cx| cx.notify()).detach();
        Self {
            browser,
            address,
            state,
        }
    }
}

#[allow(
    clippy::struct_excessive_bools,
    reason = "independent toolbar, visibility, loading, and recovery states"
)]
pub(crate) struct BrowserView {
    pane: PaneId,
    mux: Entity<MuxClient>,
    controller: Entity<BrowserController>,
    address: Entity<InputState>,
    chrome: Entity<BrowserChromeView>,
    address_editing: bool,
    omnibox: OmniboxState,
    focus_handle: FocusHandle,
    window_corners: WindowCorners,
    content_bounds: Option<Bounds<Pixels>>,
    page_buttons_down: u8,
    viewport: Viewport,
    visible: bool,
    image: Option<Arc<RenderImage>>,
    retired_images: Vec<Arc<RenderImage>>,
    #[cfg(target_os = "macos")]
    mac_surface: Option<CVPixelBuffer>,
    #[cfg(target_os = "macos")]
    mac_surface_cache: MacBrowserSurfaceCache,
    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    gpu_texture: Option<wgpu::Texture>,
    #[cfg(target_os = "windows")]
    win_gpu_texture: Option<WinGpuTexture>,
    gpu_context: Option<BrowserGpuContext>,
    frame_session: Option<SessionId>,
    image_generation: u64,
    browser_fps: FrameRateSampler,
    current_url: String,
    mux_tabs: Vec<String>,
    mux_active: usize,
    applying_mux: bool,
    profile: String,
    page_zoom_factor: f64,
    page_zoom_percent: u16,
    title: String,
    loading: bool,
    can_go_back: bool,
    can_go_forward: bool,
    tabs: Vec<BrowserTab>,
    active_tab: TabId,
    next_tab_id: u64,
    pending_history_uses: HashMap<TabId, PendingHistoryUse>,
    error: Option<Arc<str>>,
    recoverable: bool,
    chrome_profiles: Vec<chrome_import::profiles::DetectedChromeProfile>,
    chrome_profile_discovery: ChromeProfileDiscovery,
    element_pick_active: bool,
    context_menu: Option<PageContextMenu>,
    cursor: CursorStyle,
    marked_text: Option<String>,
    prompt_composition: bool,
    _subscriptions: Vec<Subscription>,
}

#[derive(Clone, Copy)]
enum ZoomOp {
    In,
    Out,
    Reset,
}

fn element_picker_appearance(theme: &zz_ui::Theme, page_zoom: f64) -> ElementPickerAppearance {
    ElementPickerAppearance {
        highlight_outline: to_hex(theme.foreground),
        highlight_fill: to_hex(theme.foreground.fill()),
        highlight_contrast: to_hex(theme.background.opaque()),
        preview_background: to_hex(theme.background.raised(1).opaque()),
        preview_foreground: to_hex(theme.foreground),
        preview_border: to_hex(theme.border),
        shadow: theme.shadow.then(|| to_hex(theme.scrim)),
        radius: f32::from(theme.radius),
        font_family: theme.mono_font_family.to_string(),
        page_zoom,
    }
}

fn browser_session_request(
    pane: PaneId,
    url: String,
    profile: String,
    viewport: Viewport,
    page_zoom_factor: f64,
    gpu_context: Option<BrowserGpuContext>,
    mux: &Entity<MuxClient>,
    cx: &App,
) -> BrowserSessionRequest {
    let egress = browser_egress_spec(mux, pane, &profile, cx);
    let request_profile = egress
        .as_ref()
        .map_or_else(|| profile, |egress| egress.composite_profile.clone());
    BrowserSessionRequest::new(
        url,
        request_profile,
        viewport,
        page_zoom_factor,
        gpu_context,
    )
    .with_egress(egress)
}

fn browser_egress_spec(
    mux: &Entity<MuxClient>,
    pane: PaneId,
    profile: &str,
    cx: &App,
) -> Option<EgressSpec> {
    if !crate::config::browser_egress_enabled(cx) {
        return None;
    }
    browser_egress_route_spec(mux, pane, profile, cx)
}

fn browser_egress_route_spec(
    mux: &Entity<MuxClient>,
    pane: PaneId,
    profile: &str,
    cx: &App,
) -> Option<EgressSpec> {
    if cfg!(target_os = "windows") {
        return None;
    }
    let (egress_host, socks_port) = mux.read(cx).attached_ssh_egress()?;
    let composite_profile =
        match zz_browser::BrowserProfilePaths::egress_profile_name(profile, &egress_host) {
            Ok(profile) => profile,
            Err(error) => {
                log::warn!(
                    target: "zz::browser::egress",
                    "could not derive browser egress profile for pane {pane}: {error}"
                );
                return None;
            }
        };
    Some(EgressSpec {
        composite_profile,
        egress_host,
        socks_port,
    })
}

struct PageContextMenu {
    menu: Entity<PopupMenu>,
    position: Point<Pixels>,
    _subscription: Subscription,
}

impl BrowserView {
    pub(crate) fn new(
        pane: PaneId,
        descriptor: &BrowserDescriptor,
        controller: Entity<BrowserController>,
        mux: Entity<MuxClient>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mux_tabs = descriptor.tabs.clone();
        let mux_active = descriptor.active_tab;
        let initial_url = normalize_url(descriptor.url()).unwrap_or_else(|error| {
            log::warn!("browser pane {pane} initial url rejected ({error}); opening blank");
            DEFAULT_URL.to_owned()
        });
        let profile = normalize_browser_profile_name(&descriptor.profile)
            .unwrap_or_else(|_| zz_browser::DEFAULT_BROWSER_PROFILE.to_owned());
        let active_index = descriptor
            .active_tab
            .min(descriptor.tabs.len().saturating_sub(1));
        let mut next_tab_id = TAB_ID.0;
        let mut tabs = Vec::with_capacity(descriptor.tabs.len().max(1));
        for (index, url) in descriptor.tabs.iter().enumerate() {
            let id = TabId(next_tab_id);
            next_tab_id += 1;
            if index == active_index {
                tabs.push(BrowserTab::new(id, initial_url.clone()));
            } else {
                let url = normalize_url(url).unwrap_or_else(|_| DEFAULT_URL.to_owned());
                tabs.push(BrowserTab::restored(id, url));
            }
        }
        if tabs.is_empty() {
            tabs.push(BrowserTab::new(TAB_ID, initial_url.clone()));
            next_tab_id = TAB_ID.0 + 1;
        }
        let active_tab = tabs[active_index.min(tabs.len() - 1)].id;
        let focus_handle = cx.focus_handle();
        let address_value = if is_blank_url(&initial_url) {
            String::new()
        } else {
            initial_url.clone()
        };
        let address = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Search or enter a URL")
                .default_value(address_value)
                .context_menu(true)
        });
        let chrome = cx.new({
            let browser = cx.entity().downgrade();
            let address = address.clone();
            let state = ChromeState {
                can_go_back: false,
                can_go_forward: false,
                element_pick_active: false,
                current_url: initial_url.clone(),
                profile: profile.clone(),
                chrome_profiles: Vec::new(),
                chrome_profile_discovery: ChromeProfileDiscovery::NotStarted,
                page_zoom_percent: 100,
                tabs: Vec::new(),
                active_tab_index: 0,
            };
            move |cx| BrowserChromeView::new(browser, address, state, cx)
        });

        let address_subscription = cx.subscribe_in(
            &address,
            window,
            |view, address, event: &InputEvent, window, cx| match event {
                InputEvent::PressEnter { .. } => {
                    view.accept_omnibox(window, cx);
                }
                InputEvent::Focus => {
                    view.select_pane(cx);
                    view.omnibox.begin(&address.read(cx).value());
                    view.set_address_editing(true, cx);
                    let address = address.clone();
                    cx.defer_in(window, move |_, _, cx| {
                        let end = address.read(cx).value().len();
                        address.update(cx, |address, cx| {
                            address.set_selected_range(0..end, cx);
                        });
                    });
                }
                InputEvent::Blur => view.set_address_editing(false, cx),
                InputEvent::Change => {
                    let value = address.read(cx).value().to_string();
                    if let Some(completion) = view.update_omnibox(&value, cx) {
                        let query = view.omnibox.query.clone();
                        let address = address.clone();
                        cx.defer_in(window, move |view, window, cx| {
                            if !view.address_editing
                                || view.omnibox.query != query
                                || view.omnibox.selected != Some(0)
                            {
                                return;
                            }
                            address.update(cx, |address, cx| {
                                address.set_value(completion.clone(), window, cx);
                                address.set_selected_range(query.len()..completion.len(), cx);
                            });
                        });
                    }
                }
                InputEvent::PasteImages(_) => {}
            },
        );
        let controller_subscription = cx.subscribe_in(
            &controller,
            window,
            |view, controller, event: &ControllerEvent, window, cx| {
                view.handle_controller_event(controller, event, window, cx);
            },
        );
        let focus_controller = controller.clone();
        let focus_pane = pane;
        let focus_in = window.on_focus_in(&focus_handle, cx, move |_, cx| {
            focus_controller.update(cx, |controller, _| {
                controller.set_focus(focus_pane, true);
            });
        });
        let blur_controller = controller.clone();
        let blur_pane = pane;
        let focus_out = window.on_focus_out(&focus_handle, cx, move |_, _, cx| {
            blur_controller.update(cx, |controller, _| {
                controller.set_focus(blur_pane, false);
            });
        });

        let error = controller.read(cx).startup_error();
        let recoverable = controller.read(cx).runtime_phase() == Some(RuntimePhase::Running);
        let viewport = Viewport {
            width: 800,
            height: 500,
            scale_factor: window.scale_factor(),
            window_zoom: window.zoom(),
            visible: false,
            ..Viewport::default()
        };
        #[cfg(any(target_os = "linux", target_os = "freebsd"))]
        let gpu_context = window
            .wgpu_device_context()
            .map(|gpu| BrowserGpuContext::new(gpu.device, gpu.queue));
        #[cfg(target_os = "windows")]
        let gpu_context = window
            .directx_device_context()
            .map(BrowserGpuContext::from_directx);
        #[cfg(not(any(target_os = "linux", target_os = "freebsd", target_os = "windows")))]
        let gpu_context = None;
        let request = browser_session_request(
            pane,
            initial_url.clone(),
            profile.clone(),
            viewport,
            1.0,
            gpu_context.clone(),
            &mux,
            cx,
        );
        controller.update(cx, |controller, cx| {
            controller.set_active_tab(pane, active_tab, cx);
            controller.request_browser(pane, active_tab, request, cx);
        });
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(FPS_SAMPLE_INTERVAL).await;
                if this
                    .update(cx, |view, cx| {
                        if view.browser_fps.sample(Instant::now()) {
                            cx.notify();
                        }
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();

        Self {
            pane,
            mux,
            controller,
            address,
            chrome,
            address_editing: false,
            omnibox: OmniboxState::default(),
            focus_handle,
            window_corners: WindowCorners::NONE,
            content_bounds: None,
            page_buttons_down: 0,
            viewport,
            visible: false,
            image: None,
            retired_images: Vec::new(),
            #[cfg(target_os = "macos")]
            mac_surface: None,
            #[cfg(target_os = "macos")]
            mac_surface_cache: MacBrowserSurfaceCache::default(),
            #[cfg(any(target_os = "linux", target_os = "freebsd"))]
            gpu_texture: None,
            #[cfg(target_os = "windows")]
            win_gpu_texture: None,
            gpu_context,
            frame_session: None,
            image_generation: 0,
            browser_fps: FrameRateSampler::new(),
            current_url: initial_url,
            mux_tabs,
            mux_active,
            applying_mux: false,
            profile,
            page_zoom_factor: 1.0,
            page_zoom_percent: 100,
            title: "Browser".to_owned(),
            loading: false,
            can_go_back: false,
            can_go_forward: false,
            tabs,
            active_tab,
            next_tab_id,
            pending_history_uses: HashMap::new(),
            error,
            recoverable,
            chrome_profiles: Vec::new(),
            chrome_profile_discovery: ChromeProfileDiscovery::NotStarted,
            element_pick_active: false,
            context_menu: None,
            cursor: CursorStyle::Arrow,
            marked_text: None,
            prompt_composition: false,
            _subscriptions: vec![
                address_subscription,
                controller_subscription,
                focus_in,
                focus_out,
            ],
        }
    }

    pub(crate) fn focus_handle(&self) -> FocusHandle {
        self.focus_handle.clone()
    }

    /// The handle pane chrome should focus: the address bar while it is being
    /// edited or the pane is blank, the page otherwise.
    pub(crate) fn pane_focus_handle(&self, cx: &App) -> FocusHandle {
        if self.address_editing || self.shows_empty_state() {
            self.address.read(cx).focus_handle(cx)
        } else {
            self.focus_handle.clone()
        }
    }

    fn shows_empty_state(&self) -> bool {
        self.error.is_none() && is_blank_url(&self.current_url)
    }

    fn set_address_editing(&mut self, editing: bool, cx: &mut Context<Self>) {
        let mut changed = false;
        if self.address_editing != editing {
            self.address_editing = editing;
            changed = true;
        }
        if !editing
            && (!self.omnibox.query.is_empty()
                || !self.omnibox.suggestions.is_empty()
                || self.omnibox.selected.is_some())
        {
            self.omnibox.reset();
            changed = true;
        }
        if changed {
            cx.notify();
        }
    }

    pub(crate) fn set_window_corners(&mut self, corners: WindowCorners, cx: &mut Context<Self>) {
        if self.window_corners != corners {
            self.window_corners = corners;
            cx.notify();
        }
    }

    pub(crate) fn set_visible(
        &mut self,
        visible: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.visible == visible {
            return;
        }
        if !visible {
            self.cancel_element_pick(cx);
        }
        self.visible = visible;
        self.viewport.visible = visible;
        self.controller.update(cx, |controller, cx| {
            controller.set_viewport(self.pane, self.viewport, cx);
            controller.set_focus(self.pane, visible);
        });
        if visible {
            let controller = self.controller.clone();
            let _ = self.consume_frame(&controller, cx);
            self.focus_handle.focus(window, cx);
        }
        cx.notify();
    }

    pub(crate) fn apply_command(&mut self, command: BrowserCommand, cx: &mut Context<Self>) {
        match command {
            BrowserCommand::Navigate(url) => self.submit_address(&url, cx),
            BrowserCommand::Reload => {
                self.cancel_element_pick(cx);
                self.pending_history_uses.remove(&self.active_tab);
                self.controller.update(cx, |controller, cx| {
                    controller.reload(self.pane, self.active_tab, cx);
                });
            }
            BrowserCommand::Back => {
                self.cancel_element_pick(cx);
                self.pending_history_uses.remove(&self.active_tab);
                self.controller.update(cx, |controller, cx| {
                    controller.go_back(self.pane, self.active_tab, cx);
                });
            }
            BrowserCommand::Forward => {
                self.cancel_element_pick(cx);
                self.pending_history_uses.remove(&self.active_tab);
                self.controller.update(cx, |controller, cx| {
                    controller.go_forward(self.pane, self.active_tab, cx);
                });
            }
            BrowserCommand::SendKeys(tokens) => {
                for token in tokens {
                    match token {
                        KeyToken::Literal(text) => self.controller.update(cx, |controller, cx| {
                            controller.send_text(self.pane, self.active_tab, &text, cx);
                        }),
                        KeyToken::Named(name) => {
                            if let Some(input) = browser_named_key(&name) {
                                self.controller.update(cx, |controller, cx| {
                                    controller.send_key(self.pane, self.active_tab, input, cx);
                                });
                            }
                        }
                    }
                }
            }
            BrowserCommand::Key(input) => {
                let input = browser_input_from_terminal(&input);
                self.controller.update(cx, |controller, cx| {
                    controller.send_key(self.pane, self.active_tab, input, cx);
                });
            }
            BrowserCommand::Screenshot { request_id, path } => {
                self.screenshot(request_id, path, cx);
            }
        }
    }

    /// Answer one `capture-browser` request, whether or not the capture worked:
    /// a CLI client is parked on the reply.
    pub(crate) fn screenshot(&mut self, request_id: u64, path: String, cx: &mut Context<Self>) {
        let response = match self.write_screenshot(Path::new(&path), cx) {
            Ok(()) => GuiResponse::Success {
                request_id,
                output: path,
            },
            Err(message) => GuiResponse::Error {
                request_id,
                message,
            },
        };
        self.mux.read(cx).respond_to_request(response);
    }

    fn write_screenshot(&self, path: &Path, cx: &mut Context<Self>) -> Result<(), String> {
        let _ = cx;
        if let Some(image) = &self.image {
            let size = image.size(0);
            let bytes = image
                .as_bytes(0)
                .ok_or_else(|| "the browser frame has no pixel data".to_owned())?;
            let width =
                u32::try_from(size.width.0).map_err(|_| "invalid frame width".to_owned())?;
            let height =
                u32::try_from(size.height.0).map_err(|_| "invalid frame height".to_owned())?;
            return Screenshot::from_bgra(width, height, bytes)?.write_png(path);
        }
        #[cfg(target_os = "macos")]
        if let Some(surface) = &self.mac_surface {
            return crate::browser::macos_surface::read_pixel_buffer(surface)?.write_png(path);
        }
        Err(
            "no CPU-readable frame: either this pane has not rendered yet, or it renders \
             through a GPU texture with no readback path (restart zz with \
             ZZ_BROWSER_SHARED_TEXTURE=0 to capture it)"
                .to_owned(),
        )
    }

    pub(crate) fn update_content_bounds(
        &mut self,
        bounds: Bounds<Pixels>,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        let started = diagnostics::timer(DIAGNOSTIC_TARGET);
        let previous = self.viewport;
        self.content_bounds = Some(bounds);
        let window_bounds = window.bounds();
        let next = Viewport {
            width: rounded_dimension(bounds.size.width),
            height: rounded_dimension(bounds.size.height),
            scale_factor: window.scale_factor(),
            window_zoom: window.zoom(),
            screen_x: rounded_coordinate(window_bounds.origin.x + bounds.origin.x * window.zoom()),
            screen_y: rounded_coordinate(window_bounds.origin.y + bounds.origin.y * window.zoom()),
            visible: self.visible,
        }
        .sanitized();
        if next != self.viewport {
            self.viewport = next;
            self.controller.update(cx, |controller, cx| {
                controller.set_viewport(self.pane, next, cx);
            });
        }
        log::trace!(
            target: "zz::diagnostics::browser_render",
            "content_bounds pane={} bounds={bounds:?} previous_viewport={previous:?} next_viewport={next:?} changed={} elapsed_us={}",
            self.pane,
            previous != next,
            diagnostics::elapsed_us(started),
        );
    }

    fn update_omnibox(&mut self, value: &str, cx: &mut Context<Self>) -> Option<String> {
        if !self.address_editing {
            let previous_value = if is_blank_url(&self.current_url) {
                ""
            } else {
                &self.current_url
            };
            self.omnibox.begin(previous_value);
            self.address_editing = true;
        }
        let previous = self.omnibox.query.clone();
        let typed_one = value.starts_with(&previous)
            && value.chars().count() == previous.chars().count().saturating_add(1);
        self.omnibox.incremental_input &=
            typed_one || differs_by_one_character_edit(&self.omnibox.previous_value, value);
        value.clone_into(&mut self.omnibox.previous_value);
        value.clone_into(&mut self.omnibox.query);
        self.omnibox.suggestions =
            recent_pages::suggestions(&self.profile, value, OMNIBOX_SUGGESTION_LIMIT, cx);
        self.omnibox.selected = None;
        let completion = typed_one
            .then(|| {
                self.omnibox
                    .suggestions
                    .first()
                    .and_then(|suggestion| suggestion.inline_completion.clone())
            })
            .flatten();
        if completion.is_some() {
            self.omnibox.selected = Some(0);
        }
        cx.notify();
        completion
    }

    fn accept_omnibox(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let selected = self
            .omnibox
            .selected
            .and_then(|index| self.omnibox.suggestions.get(index))
            .cloned();
        let value = selected.as_ref().map_or_else(
            || self.address.read(cx).value().to_string(),
            |suggestion| suggestion.url.clone(),
        );
        let input = if self.omnibox.query.is_empty() {
            value.clone()
        } else {
            self.omnibox.query.clone()
        };
        let selected = selected.is_some();
        let history_use = (selected || self.omnibox.incremental_input).then(|| PendingHistoryUse {
            profile: self.profile.clone(),
            input,
            selected,
            started: false,
        });
        self.navigate_address(&value, history_use, cx);
        self.focus_page(window, cx);
    }

    fn submit_address(&mut self, value: &str, cx: &mut Context<Self>) {
        self.navigate_address(value, None, cx);
    }

    fn navigate_address(
        &mut self,
        value: &str,
        history_use: Option<PendingHistoryUse>,
        cx: &mut Context<Self>,
    ) {
        if value.trim().is_empty() {
            return;
        }
        self.pending_history_uses.remove(&self.active_tab);
        match resolve_address(value, crate::config::browser_search_provider(cx)) {
            Ok(url) => {
                self.cancel_element_pick(cx);
                self.error = None;
                self.current_url.clone_from(&url);
                if let Some(history_use) = history_use {
                    self.pending_history_uses
                        .insert(self.active_tab, history_use);
                }
                self.controller.update(cx, |controller, cx| {
                    controller.navigate(self.pane, self.active_tab, &url, cx);
                });
            }
            Err(error) => self.error = Some(Arc::from(error.to_string())),
        }
        cx.notify();
    }

    fn complete_history_use(
        &mut self,
        tab: TabId,
        url: &str,
        succeeded: bool,
        cx: &mut Context<Self>,
    ) {
        if !self
            .pending_history_uses
            .get(&tab)
            .is_some_and(|history_use| history_use.started)
        {
            return;
        }
        let Some(history_use) = self.pending_history_uses.remove(&tab) else {
            return;
        };
        if succeeded {
            recent_pages::record_omnibox_use(
                &history_use.profile,
                &history_use.input,
                url,
                history_use.selected,
                cx,
            );
        }
    }

    fn mark_history_use_started(&mut self, tab: TabId) {
        if let Some(history_use) = self.pending_history_uses.get_mut(&tab) {
            history_use.started = true;
        }
    }

    fn focus_page(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.set_address_editing(false, cx);
        self.select_pane(cx);
        self.focus_handle.focus(window, cx);
        self.controller.update(cx, |controller, _| {
            controller.set_focus(self.pane, true);
        });
    }

    fn select_pane(&self, cx: &App) {
        self.mux
            .read(cx)
            .execute(zz_protocol::CommandInvocation::new(
                "select-pane",
                ["-t", &self.pane.to_string()],
            ));
    }

    fn handle_controller_event(
        &mut self,
        controller: &Entity<BrowserController>,
        event: &ControllerEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            ControllerEvent::RuntimeReady => {
                self.recoverable = true;
                self.error = None;
            }
            ControllerEvent::Failed(error) => {
                self.element_pick_active = false;
                self.pending_history_uses.clear();
                self.error = Some(error.clone());
                self.recoverable =
                    controller.read(cx).runtime_phase() == Some(RuntimePhase::Running);
            }
            ControllerEvent::Browser { pane, tab, event }
                if *pane == self.pane && *tab != self.active_tab =>
            {
                if !self.handle_inactive_tab_event(*tab, event, window, cx) {
                    return;
                }
            }
            ControllerEvent::Browser { pane, event, .. } if *pane == self.pane => match event {
                BrowserEvent::Created { .. } => {
                    self.error = None;
                    self.recoverable = true;
                }
                BrowserEvent::AddressChanged { url, .. } => {
                    log::debug!("browser main-frame address changed to {}", display_url(url));
                    self.current_url = url.to_string();
                    if !self.address_editing {
                        let display = if is_blank_url(url) {
                            String::new()
                        } else {
                            url.to_string()
                        };
                        self.address.update(cx, |address, cx| {
                            address.set_value(display, window, cx);
                        });
                    }
                    recent_pages::record_visit(&self.profile, url, cx);
                    self.publish_tabs(cx);
                }
                BrowserEvent::TitleChanged { title, .. } => {
                    self.title = title.to_string();
                    recent_pages::record_title(&self.profile, &self.current_url, title, cx);
                }
                BrowserEvent::LoadingChanged {
                    loading,
                    can_go_back,
                    can_go_forward,
                    ..
                } => {
                    self.loading = *loading;
                    self.can_go_back = *can_go_back;
                    self.can_go_forward = *can_go_forward;
                    self.address.update(cx, |address, cx| {
                        address.set_loading(*loading, window, cx);
                    });
                    if *loading {
                        self.mark_history_use_started(self.active_tab);
                        self.error = None;
                    } else {
                        let url = self.current_url.clone();
                        self.complete_history_use(self.active_tab, &url, self.error.is_none(), cx);
                    }
                }
                BrowserEvent::FrameReady { .. } if self.visible => {
                    if !self.consume_frame(controller, cx) {
                        return;
                    }
                }
                BrowserEvent::SharedTextureFailed { .. } | BrowserEvent::FrameReady { .. } => {
                    return;
                }
                BrowserEvent::LoadFailed {
                    description, url, ..
                } => {
                    self.error = Some(Arc::from(format!(
                        "Could not load {}: {}",
                        display_url(url),
                        description
                    )));
                    self.recoverable = true;
                }
                BrowserEvent::CursorChanged { cursor, .. } => {
                    self.cursor = cursor_style(*cursor);
                }
                BrowserEvent::RenderProcessTerminated { status, .. } => {
                    self.pending_history_uses.remove(&self.active_tab);
                    self.element_pick_active = false;
                    self.error = Some(Arc::from(format!("Renderer stopped: {status}")));
                    self.recoverable = true;
                }
                BrowserEvent::ElementPicked {
                    text, screenshot, ..
                } => {
                    self.element_pick_active = false;
                    let (item, message) = match screenshot {
                        Some(png) => (
                            ClipboardItem {
                                entries: vec![
                                    ClipboardEntry::String(ClipboardString::new(text.to_string())),
                                    ClipboardEntry::Image(Image::from_bytes(
                                        ImageFormat::Png,
                                        png.to_vec(),
                                    )),
                                ],
                            },
                            "Element context + screenshot copied",
                        ),
                        None => (
                            ClipboardItem::new_string(text.to_string()),
                            "Element context copied",
                        ),
                    };
                    cx.write_to_clipboard(item);
                    self.emit_notification(message, ClientMessageKind::Success, cx);
                }
                BrowserEvent::ElementPickFailed { .. } => {
                    self.element_pick_active = false;
                    self.emit_notification(
                        "Could not inspect that element",
                        ClientMessageKind::Error,
                        cx,
                    );
                }
                BrowserEvent::ContextMenuRequested { request, .. } => {
                    self.open_context_menu(request, window, cx);
                }
                BrowserEvent::PopupRequested {
                    url, foreground, ..
                } => {
                    self.open_tab(Some(url), *foreground, window, cx);
                }
                BrowserEvent::Closed { .. } => {
                    self.pending_history_uses.remove(&self.active_tab);
                    self.element_pick_active = false;
                }
                BrowserEvent::ElementPickCancelled { .. } => {
                    self.element_pick_active = false;
                }
            },
            ControllerEvent::CookiesImported { pane, result } if *pane == self.pane => {
                self.handle_cookie_import_result(controller, *result, cx);
            }
            ControllerEvent::SiteDataCleared { pane, result } if *pane == self.pane => {
                self.handle_site_data_clear_result(controller, *result, cx);
            }
            ControllerEvent::BrowserDataFailed { pane, message } if *pane == self.pane => {
                self.emit_notification(message.to_string(), ClientMessageKind::Error, cx);
            }
            ControllerEvent::BrowserFailed { pane, message } if *pane == self.pane => {
                self.pending_history_uses.clear();
                self.element_pick_active = false;
                self.error = Some(message.clone());
                self.recoverable = true;
            }
            ControllerEvent::Browser { .. }
            | ControllerEvent::CookiesImported { .. }
            | ControllerEvent::SiteDataCleared { .. }
            | ControllerEvent::BrowserDataFailed { .. }
            | ControllerEvent::BrowserFailed { .. } => return,
        }
        cx.notify();
    }

    fn handle_inactive_tab_event(
        &mut self,
        tab: TabId,
        event: &BrowserEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if let BrowserEvent::PopupRequested {
            url, foreground, ..
        } = event
        {
            self.open_tab(Some(url), *foreground, window, cx);
            return true;
        }
        if matches!(event, BrowserEvent::Closed { .. }) {
            self.pending_history_uses.remove(&tab);
            return true;
        }
        let Some(entry) = self.tabs.iter_mut().find(|entry| entry.id == tab) else {
            return false;
        };
        let mut completed_load = None;
        let mut started_load = false;
        match event {
            BrowserEvent::AddressChanged { url, .. } => {
                entry.url = url.to_string();
                recent_pages::record_visit(&self.profile, url, cx);
            }
            BrowserEvent::TitleChanged { title, .. } => {
                recent_pages::record_title(&self.profile, &entry.url, title, cx);
                entry.title = title.to_string();
            }
            BrowserEvent::LoadingChanged {
                loading,
                can_go_back,
                can_go_forward,
                ..
            } => {
                entry.loading = *loading;
                entry.can_go_back = *can_go_back;
                entry.can_go_forward = *can_go_forward;
                if *loading {
                    started_load = true;
                    entry.error = None;
                } else {
                    completed_load = Some((entry.url.clone(), entry.error.is_none()));
                }
            }
            BrowserEvent::LoadFailed {
                description, url, ..
            } => {
                entry.error = Some(Arc::from(format!(
                    "Could not load {}: {}",
                    display_url(url),
                    description
                )));
            }
            _ => return false,
        }
        if matches!(event, BrowserEvent::AddressChanged { .. }) {
            self.publish_tabs(cx);
        }
        if started_load {
            self.mark_history_use_started(tab);
        }
        if let Some((url, succeeded)) = completed_load {
            self.complete_history_use(tab, &url, succeeded, cx);
        }
        true
    }

    fn handle_cookie_import_result(
        &mut self,
        controller: &Entity<BrowserController>,
        result: zz_browser::CookieImportResult,
        cx: &mut Context<Self>,
    ) {
        let ignored = result.skipped + result.rejected;
        if result.imported > 0 {
            self.pending_history_uses.remove(&self.active_tab);
            controller.update(cx, |controller, cx| {
                controller.reload(self.pane, self.active_tab, cx);
            });
        }
        let (message, kind) = if result.imported == 0 {
            (
                format!("Chromium rejected all {ignored} cookies"),
                ClientMessageKind::Error,
            )
        } else if !result.persisted {
            (
                format!(
                    "Imported {} cookies, but could not flush the profile{}",
                    result.imported,
                    ignored_suffix(ignored)
                ),
                ClientMessageKind::Warning,
            )
        } else {
            (
                format!(
                    "Imported {} cookies{}",
                    result.imported,
                    ignored_suffix(ignored)
                ),
                ClientMessageKind::Success,
            )
        };
        self.emit_notification(message, kind, cx);
    }

    fn handle_site_data_clear_result(
        &mut self,
        controller: &Entity<BrowserController>,
        result: zz_browser::SiteDataClearResult,
        cx: &mut Context<Self>,
    ) {
        if result.success {
            self.pending_history_uses.remove(&self.active_tab);
            controller.update(cx, |controller, cx| {
                controller.reload(self.pane, self.active_tab, cx);
            });
            self.emit_notification("Site data cleared", ClientMessageKind::Success, cx);
        } else {
            self.emit_notification(
                "Chromium could not clear this site's data",
                ClientMessageKind::Error,
                cx,
            );
        }
    }

    fn consume_frame(
        &mut self,
        controller: &Entity<BrowserController>,
        cx: &mut Context<Self>,
    ) -> bool {
        let started = diagnostics::timer(DIAGNOSTIC_TARGET);
        let Some(frame) = controller.read(cx).latest_frame(self.pane, self.active_tab) else {
            log::trace!(
                target: "zz::diagnostics::browser_render",
                "consume_frame pane={} skipped=no_frame elapsed_us={}",
                self.pane,
                diagnostics::elapsed_us(started),
            );
            return false;
        };
        if self.frame_session == Some(frame.session) && frame.generation <= self.image_generation {
            log::trace!(
                target: "zz::diagnostics::browser_render",
                "consume_frame pane={} skipped=stale frame_generation={} image_generation={} elapsed_us={}",
                self.pane,
                frame.generation,
                self.image_generation,
                diagnostics::elapsed_us(started),
            );
            return false;
        }
        let (tier, image_strong_count) = match &frame.content {
            BrowserPaneFrameContent::OwnedBgra(image) => {
                if let Some(previous) = self.image.replace(image.clone()) {
                    self.retired_images.push(previous);
                }
                #[cfg(target_os = "macos")]
                {
                    self.mac_surface = None;
                    self.mac_surface_cache.clear();
                }
                #[cfg(any(target_os = "linux", target_os = "freebsd"))]
                {
                    self.gpu_texture = None;
                }
                #[cfg(target_os = "windows")]
                {
                    self.win_gpu_texture = None;
                }
                ("owned_bgra", Some(Arc::strong_count(image)))
            }
            BrowserPaneFrameContent::Gpu(texture) => {
                if let Some(previous) = self.image.take() {
                    self.retired_images.push(previous);
                }
                #[cfg(any(target_os = "linux", target_os = "freebsd"))]
                {
                    self.gpu_texture = Some(texture.clone());
                    ("gpu", None)
                }
                #[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
                {
                    let _ = texture;
                    log::warn!(
                        "received a GPU browser frame without GPUI external-texture support"
                    );
                    return true;
                }
            }
            #[cfg(target_os = "macos")]
            BrowserPaneFrameContent::MacGpu(io_surface) => {
                if let Some(previous) = self.image.take() {
                    self.retired_images.push(previous);
                }
                let Some(pool_generation) = frame.pool_generation else {
                    log::error!("macOS GPU browser frame is missing its pool generation");
                    return true;
                };
                match self.mac_surface_cache.pixel_buffer(
                    frame.session,
                    pool_generation,
                    io_surface,
                ) {
                    Ok(surface) => {
                        self.mac_surface = Some(surface);
                        ("mac_gpu", None)
                    }
                    Err(error) => {
                        log::warn!(
                            target: "zz_browser::accelerated_paint",
                            "could not wrap browser IOSurface for GPUI: {error}"
                        );
                        let pane = self.pane;
                        let tab = self.active_tab;
                        let reason = error.to_string();
                        controller.update(cx, |controller, _| {
                            controller.force_readback(pane, tab, &reason);
                        });
                        return true;
                    }
                }
            }
            #[cfg(target_os = "windows")]
            BrowserPaneFrameContent::WinGpu(texture) => {
                if let Some(previous) = self.image.take() {
                    self.retired_images.push(previous);
                }
                self.win_gpu_texture = Some(texture.clone());
                ("win_gpu", None)
            }
        };
        self.frame_session = Some(frame.session);
        self.image_generation = frame.generation;
        self.browser_fps.record_frame();
        log::trace!(
            target: "zz::diagnostics::browser_render",
            "consume_frame pane={} session={} generation={} delivery_generation={} tier={tier} logical={}x{} device={}x{} pool_generation={:?} sequence={:?} image_strong_count={image_strong_count:?} retired_images_len={} retired_images_capacity={} total_elapsed_us={}",
            self.pane,
            frame.session.0,
            frame.generation,
            frame.delivery_generation,
            frame.logical_width,
            frame.logical_height,
            frame.width,
            frame.height,
            frame.pool_generation,
            frame.sequence,
            self.retired_images.len(),
            self.retired_images.capacity(),
            diagnostics::elapsed_us(started),
        );
        true
    }

    fn on_back(&mut self, cx: &mut Context<Self>) {
        if self.can_go_back {
            self.cancel_element_pick(cx);
            self.pending_history_uses.remove(&self.active_tab);
            self.controller.update(cx, |controller, cx| {
                controller.go_back(self.pane, self.active_tab, cx);
            });
        }
        cx.stop_propagation();
    }

    fn on_forward(&mut self, cx: &mut Context<Self>) {
        if self.can_go_forward {
            self.cancel_element_pick(cx);
            self.pending_history_uses.remove(&self.active_tab);
            self.controller.update(cx, |controller, cx| {
                controller.go_forward(self.pane, self.active_tab, cx);
            });
        }
        cx.stop_propagation();
    }

    fn on_reload(&mut self, cx: &mut Context<Self>) {
        self.cancel_element_pick(cx);
        self.pending_history_uses.remove(&self.active_tab);
        self.error = None;
        self.controller.update(cx, |controller, cx| {
            controller.reload(self.pane, self.active_tab, cx);
        });
        cx.stop_propagation();
        cx.notify();
    }

    fn on_retry(&mut self, cx: &mut Context<Self>) {
        if self.recoverable {
            self.recreate_session(cx);
        }
        cx.stop_propagation();
    }

    fn reset_frame_state(&mut self) {
        if let Some(image) = self.image.take() {
            self.retired_images.push(image);
        }
        #[cfg(any(target_os = "linux", target_os = "freebsd"))]
        {
            self.gpu_texture = None;
        }
        #[cfg(target_os = "windows")]
        {
            self.win_gpu_texture = None;
        }
        #[cfg(target_os = "macos")]
        {
            self.mac_surface = None;
            self.mac_surface_cache.clear();
        }
        self.frame_session = None;
        self.image_generation = 0;
    }

    fn active_tab_index(&self) -> usize {
        self.tabs
            .iter()
            .position(|tab| tab.id == self.active_tab)
            .unwrap_or(0)
    }

    fn save_active_tab_state(&mut self) {
        let index = self.active_tab_index();
        let Some(tab) = self.tabs.get_mut(index) else {
            return;
        };
        tab.url.clone_from(&self.current_url);
        tab.title.clone_from(&self.title);
        tab.loading = self.loading;
        tab.can_go_back = self.can_go_back;
        tab.can_go_forward = self.can_go_forward;
        tab.page_zoom_factor = self.page_zoom_factor;
        tab.page_zoom_percent = self.page_zoom_percent;
        tab.error.clone_from(&self.error);
    }

    fn activate_tab(&mut self, tab: TabId, window: &mut Window, cx: &mut Context<Self>) {
        if tab == self.active_tab {
            return;
        }
        let Some(index) = self.tabs.iter().position(|entry| entry.id == tab) else {
            return;
        };
        self.cancel_element_pick(cx);
        self.context_menu = None;
        self.omnibox.reset();
        self.save_active_tab_state();
        self.active_tab = tab;
        let entry = &self.tabs[index];
        self.current_url = entry.url.clone();
        self.title = entry.title.clone();
        self.loading = entry.loading;
        self.can_go_back = entry.can_go_back;
        self.can_go_forward = entry.can_go_forward;
        self.page_zoom_factor = entry.page_zoom_factor;
        self.page_zoom_percent = entry.page_zoom_percent;
        self.error = entry.error.clone();
        self.reset_frame_state();
        let display = if is_blank_url(&self.current_url) {
            String::new()
        } else {
            self.current_url.clone()
        };
        let loading = self.loading;
        self.address.update(cx, |address, cx| {
            address.set_value(display, window, cx);
            address.set_loading(loading, window, cx);
        });
        self.controller.update(cx, |controller, cx| {
            controller.set_active_tab(self.pane, tab, cx);
        });
        if let Some(entry) = self.tabs.iter_mut().find(|entry| entry.id == tab)
            && !entry.started
        {
            entry.started = true;
            let request = browser_session_request(
                self.pane,
                self.current_url.clone(),
                self.profile.clone(),
                self.viewport,
                self.page_zoom_factor,
                self.gpu_context.clone(),
                &self.mux,
                cx,
            );
            self.controller.update(cx, |controller, cx| {
                controller.request_browser(self.pane, tab, request, cx);
            });
        }
        self.publish_tabs(cx);
        cx.notify();
    }

    fn open_tab(
        &mut self,
        url: Option<&str>,
        foreground: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let url = url.map_or_else(
            || DEFAULT_URL.to_owned(),
            |url| normalize_url(url).unwrap_or_else(|_| DEFAULT_URL.to_owned()),
        );
        let id = TabId(self.next_tab_id);
        self.next_tab_id += 1;
        self.tabs.push(BrowserTab::new(id, url.clone()));
        let request = browser_session_request(
            self.pane,
            url,
            self.profile.clone(),
            self.viewport,
            1.0,
            self.gpu_context.clone(),
            &self.mux,
            cx,
        );
        if foreground {
            self.activate_tab(id, window, cx);
        }
        self.controller.update(cx, |controller, cx| {
            controller.request_browser(self.pane, id, request, cx);
        });
        if foreground && self.shows_empty_state() {
            self.address.read(cx).focus_handle(cx).focus(window, cx);
        }
        self.publish_tabs(cx);
        cx.notify();
    }

    fn close_tab_by_id(&mut self, tab: TabId, window: &mut Window, cx: &mut Context<Self>) {
        if self.tabs.len() <= 1 {
            return;
        }
        let Some(index) = self.tabs.iter().position(|entry| entry.id == tab) else {
            return;
        };
        if tab == self.active_tab {
            let neighbor = self
                .tabs
                .get(index + 1)
                .or_else(|| index.checked_sub(1).and_then(|index| self.tabs.get(index)))
                .map(|entry| entry.id);
            if let Some(neighbor) = neighbor {
                self.activate_tab(neighbor, window, cx);
            }
        }
        self.tabs.retain(|entry| entry.id != tab);
        self.pending_history_uses.remove(&tab);
        self.controller.update(cx, |controller, _| {
            controller.close_tab(self.pane, tab);
        });
        self.publish_tabs(cx);
        cx.notify();
    }

    fn cycle_tab(&mut self, offset: i64, window: &mut Window, cx: &mut Context<Self>) {
        if self.tabs.len() <= 1 {
            return;
        }
        let len = i64::try_from(self.tabs.len()).unwrap_or(i64::MAX);
        let index = i64::try_from(self.active_tab_index()).unwrap_or_default();
        let next = usize::try_from((index + offset).rem_euclid(len)).unwrap_or_default();
        let tab = self.tabs[next].id;
        self.activate_tab(tab, window, cx);
    }

    fn tab_strip_state(&self) -> (Vec<BrowserTabInfo>, usize) {
        let tabs = self
            .tabs
            .iter()
            .map(|tab| {
                let (url, title) = if tab.id == self.active_tab {
                    (self.current_url.as_str(), self.title.as_str())
                } else {
                    (tab.url.as_str(), tab.title.as_str())
                };
                let detail = if title.is_empty() { url } else { title };
                BrowserTabInfo::new(tab.id.0, tab_host_label(url), detail.to_owned())
            })
            .collect();
        (tabs, self.active_tab_index())
    }

    fn on_new_tab(&mut self, _: &NewTab, window: &mut Window, cx: &mut Context<Self>) {
        self.open_tab(None, true, window, cx);
        cx.stop_propagation();
    }

    fn on_close_pane(&mut self, _: &ClosePane, window: &mut Window, cx: &mut Context<Self>) {
        if self.tabs.len() > 1 {
            self.close_tab_by_id(self.active_tab, window, cx);
        } else {
            cx.propagate();
        }
    }

    fn on_next_tab(&mut self, _: &NextTab, window: &mut Window, cx: &mut Context<Self>) {
        self.cycle_tab(1, window, cx);
        cx.stop_propagation();
    }

    fn on_previous_tab(&mut self, _: &PreviousTab, window: &mut Window, cx: &mut Context<Self>) {
        self.cycle_tab(-1, window, cx);
        cx.stop_propagation();
    }

    fn on_select_tab(&mut self, action: &SelectTab, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(tab) = self.tabs.get(action.index).map(|tab| tab.id) {
            self.activate_tab(tab, window, cx);
        }
        cx.stop_propagation();
    }

    fn on_select_last_tab(
        &mut self,
        _: &SelectLastTab,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(tab) = self.tabs.last().map(|tab| tab.id) {
            self.activate_tab(tab, window, cx);
        }
        cx.stop_propagation();
    }

    fn on_go_back(&mut self, _: &GoBack, _: &mut Window, cx: &mut Context<Self>) {
        self.on_back(cx);
    }

    fn on_go_forward(&mut self, _: &GoForward, _: &mut Window, cx: &mut Context<Self>) {
        self.on_forward(cx);
    }

    fn on_reload_action(&mut self, _: &Reload, _: &mut Window, cx: &mut Context<Self>) {
        self.on_reload(cx);
    }

    fn on_focus_address(&mut self, _: &FocusAddress, window: &mut Window, cx: &mut Context<Self>) {
        self.address.read(cx).focus_handle(cx).focus(window, cx);
        cx.stop_propagation();
    }

    fn on_omnibox_next(&mut self, _: &OmniboxNext, window: &mut Window, cx: &mut Context<Self>) {
        self.select_omnibox_suggestion(1, window, cx);
    }

    fn on_omnibox_previous(
        &mut self,
        _: &OmniboxPrevious,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_omnibox_suggestion(-1, window, cx);
    }

    fn select_omnibox_suggestion(
        &mut self,
        direction: i64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.address_editing || self.omnibox.suggestions.is_empty() {
            cx.propagate();
            return;
        }
        let count = i64::try_from(self.omnibox.suggestions.len()).unwrap_or(i64::MAX);
        let next = self.omnibox.selected.map_or_else(
            || if direction < 0 { count - 1 } else { 0 },
            |selected| (i64::try_from(selected).unwrap_or_default() + direction).rem_euclid(count),
        );
        self.omnibox.selected = usize::try_from(next).ok();
        let preview = self
            .omnibox
            .selected
            .and_then(|selected| self.omnibox.suggestions.get(selected))
            .map_or_else(
                || self.omnibox.query.clone(),
                |suggestion| suggestion.url.clone(),
            );
        self.address.update(cx, |address, cx| {
            if address.value().as_ref() != preview {
                address.set_value(preview.clone(), window, cx);
            }
            address.set_selected_range(preview.len()..preview.len(), cx);
        });
        cx.stop_propagation();
        cx.notify();
    }

    fn on_omnibox_delete(
        &mut self,
        _: &OmniboxDelete,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(index) = self.omnibox.selected else {
            cx.propagate();
            return;
        };
        let Some(url) = self
            .omnibox
            .suggestions
            .get(index)
            .map(|suggestion| suggestion.url.clone())
        else {
            cx.propagate();
            return;
        };
        recent_pages::remove(&self.profile, &url, cx);
        let query = self.omnibox.query.clone();
        self.omnibox.suggestions =
            recent_pages::suggestions(&self.profile, &query, OMNIBOX_SUGGESTION_LIMIT, cx);
        self.omnibox.selected = None;
        self.address.update(cx, |address, cx| {
            address.set_value(query.clone(), window, cx);
            address.set_selected_range(query.len()..query.len(), cx);
        });
        cx.stop_propagation();
        cx.notify();
    }

    fn accept_omnibox_suggestion(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if index >= self.omnibox.suggestions.len() {
            return;
        }
        self.omnibox.selected = Some(index);
        self.accept_omnibox(window, cx);
    }

    fn on_browser_edit(&mut self, action: &BrowserEdit, _: &mut Window, cx: &mut Context<Self>) {
        self.edit(action.command, cx);
        cx.stop_propagation();
    }

    fn recreate_session(&mut self, cx: &mut Context<Self>) {
        self.cancel_element_pick(cx);
        self.pending_history_uses.remove(&self.active_tab);
        self.error = None;
        self.reset_frame_state();
        let url = self.current_url.clone();
        let request = browser_session_request(
            self.pane,
            url,
            self.profile.clone(),
            self.viewport,
            self.page_zoom_factor,
            self.gpu_context.clone(),
            &self.mux,
            cx,
        );
        let tab = self.active_tab;
        self.controller.update(cx, |controller, cx| {
            controller.retry(self.pane, tab, request, cx);
        });
        cx.notify();
    }

    /// Apply a descriptor tab-list change from mux. Only a changed descriptor
    /// reconciles, so a stale echo cannot yank the pane back mid-browse. Tabs
    /// match up by index because tab ids are client-local.
    pub(crate) fn synchronize_tabs(
        &mut self,
        tabs: &[String],
        active_tab: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if tabs.is_empty() || (self.mux_tabs == tabs && self.mux_active == active_tab) {
            return;
        }
        self.mux_tabs = tabs.to_vec();
        self.mux_active = active_tab;
        self.applying_mux = true;
        while self.tabs.len() > tabs.len() {
            let id = self.tabs[self.tabs.len() - 1].id;
            self.close_tab_by_id(id, window, cx);
        }
        for (index, url) in tabs.iter().enumerate() {
            if index < self.tabs.len() {
                self.navigate_tab_at(index, url, cx);
            } else {
                let id = TabId(self.next_tab_id);
                self.next_tab_id += 1;
                let url = normalize_url(url).unwrap_or_else(|_| DEFAULT_URL.to_owned());
                self.tabs.push(BrowserTab::restored(id, url));
            }
        }
        let active = self.tabs[active_tab.min(self.tabs.len() - 1)].id;
        self.activate_tab(active, window, cx);
        self.applying_mux = false;
        cx.notify();
    }

    fn navigate_tab_at(&mut self, index: usize, url: &str, cx: &mut Context<Self>) {
        let url = match normalize_url(url) {
            Ok(url) => url,
            Err(error) => {
                log::warn!("ignoring mux url for browser pane {}: {error}", self.pane);
                return;
            }
        };
        let Some(entry) = self.tabs.get_mut(index) else {
            return;
        };
        let tab = entry.id;
        self.pending_history_uses.remove(&tab);
        if tab == self.active_tab {
            if url == self.current_url {
                return;
            }
            self.cancel_element_pick(cx);
            self.error = None;
            self.current_url.clone_from(&url);
        } else {
            if entry.url == url {
                return;
            }
            entry.url.clone_from(&url);
            if !entry.started {
                return;
            }
        }
        self.controller.update(cx, |controller, cx| {
            controller.navigate(self.pane, tab, &url, cx);
        });
    }

    fn publish_tabs(&mut self, cx: &mut Context<Self>) {
        if self.applying_mux {
            return;
        }
        let tabs: Vec<String> = self
            .tabs
            .iter()
            .map(|tab| {
                if tab.id == self.active_tab {
                    self.current_url.clone()
                } else {
                    tab.url.clone()
                }
            })
            .collect();
        let active = self.active_tab_index();
        if self.mux_tabs == tabs && self.mux_active == active {
            return;
        }
        self.mux_tabs.clone_from(&tabs);
        self.mux_active = active;
        let mut args = vec![
            "-t".to_owned(),
            self.pane.to_string(),
            "-a".to_owned(),
            active.to_string(),
            "--".to_owned(),
        ];
        args.extend(tabs);
        self.mux
            .read(cx)
            .execute(CommandInvocation::new("set-browser-tabs", args));
    }

    pub(crate) fn synchronize_profile(&mut self, profile: &str, cx: &mut Context<Self>) {
        let Ok(profile) = normalize_browser_profile_name(profile) else {
            return;
        };
        let egress = browser_egress_route_spec(&self.mux, self.pane, &profile, cx);
        self.controller.update(cx, |controller, _| {
            controller.refresh_egress(self.pane, egress);
        });
        if self.profile == profile {
            return;
        }
        self.pending_history_uses.clear();
        self.omnibox.reset();
        self.profile = profile;
        let removed: Vec<TabId> = self
            .tabs
            .iter()
            .map(|tab| tab.id)
            .filter(|id| *id != self.active_tab)
            .collect();
        if !removed.is_empty() {
            self.tabs.retain(|tab| tab.id == self.active_tab);
            self.controller.update(cx, |controller, _| {
                for tab in removed {
                    controller.close_tab(self.pane, tab);
                }
            });
        }
        self.recreate_session(cx);
    }

    fn on_zoom_in(&mut self, _: &ZoomIn, _: &mut Window, cx: &mut Context<Self>) {
        self.apply_zoom(ZoomOp::In, cx);
        cx.stop_propagation();
    }

    fn on_zoom_out(&mut self, _: &ZoomOut, _: &mut Window, cx: &mut Context<Self>) {
        self.apply_zoom(ZoomOp::Out, cx);
        cx.stop_propagation();
    }

    fn on_reset_zoom(&mut self, _: &ResetZoom, _: &mut Window, cx: &mut Context<Self>) {
        self.apply_zoom(ZoomOp::Reset, cx);
        cx.stop_propagation();
    }

    fn on_toggle_dev_tools(&mut self, _: &ToggleDevTools, _: &mut Window, cx: &mut Context<Self>) {
        self.toggle_dev_tools(cx);
        cx.stop_propagation();
    }

    fn on_configured_element_selector(
        &mut self,
        action: &ConfiguredElementSelector,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if crate::keymap::action_for(cx, BROWSER_TABLE, &action.chord)
            != Some(ChromeAction::BrowserElementSelector)
        {
            cx.propagate();
            return;
        }
        self.on_element_pick(window, cx);
    }

    fn toggle_dev_tools(&mut self, cx: &mut Context<Self>) {
        self.controller.update(cx, |controller, _| {
            controller.toggle_dev_tools(self.pane, self.active_tab);
        });
    }

    fn apply_zoom(&mut self, op: ZoomOp, cx: &mut Context<Self>) {
        let result = self.controller.update(cx, |controller, _| match op {
            ZoomOp::In => controller.zoom_in(self.pane, self.active_tab),
            ZoomOp::Out => controller.zoom_out(self.pane, self.active_tab),
            ZoomOp::Reset => controller.reset_zoom(self.pane, self.active_tab),
        });
        if let Some((factor, percent)) = result {
            self.page_zoom_factor = factor;
            self.page_zoom_percent = percent;
            if self.element_pick_active && !self.start_element_pick(cx) {
                self.element_pick_active = false;
                self.emit_notification(
                    "Element picker is unavailable",
                    ClientMessageKind::Error,
                    cx,
                );
            }
            cx.notify();
        }
    }

    fn start_element_pick(&self, cx: &mut Context<Self>) -> bool {
        let appearance = element_picker_appearance(cx.theme(), self.page_zoom_factor);
        self.controller.update(cx, |controller, _| {
            controller.start_element_pick(self.pane, self.active_tab, &appearance)
        })
    }

    fn on_element_pick(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.element_pick_active {
            self.cancel_element_pick(cx);
        } else if self.start_element_pick(cx) {
            self.element_pick_active = true;
            self.focus_handle.focus(window, cx);
            self.controller.update(cx, |controller, _| {
                controller.set_focus(self.pane, true);
            });
        } else {
            self.emit_notification(
                "Element picker is unavailable",
                ClientMessageKind::Error,
                cx,
            );
        }
        cx.stop_propagation();
        cx.notify();
    }

    fn on_root_key_down(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if event.keystroke.key != "escape" {
            return;
        }
        if self.element_pick_active {
            self.cancel_element_pick(cx);
            cx.stop_propagation();
            cx.notify();
            return;
        }
        if !self.address_editing {
            return;
        }
        if self.omnibox.selected.take().is_some() {
            let query = self.omnibox.query.clone();
            self.address.update(cx, |address, cx| {
                address.set_value(query.clone(), window, cx);
                address.set_selected_range(query.len()..query.len(), cx);
            });
            cx.stop_propagation();
            cx.notify();
            return;
        }
        if !self.omnibox.suggestions.is_empty() {
            self.omnibox.suggestions.clear();
            cx.stop_propagation();
            cx.notify();
            return;
        }
        let current_url = if is_blank_url(&self.current_url) {
            String::new()
        } else {
            self.current_url.clone()
        };
        self.address.update(cx, |address, cx| {
            address.set_value(current_url, window, cx);
        });
        self.focus_page(window, cx);
        cx.stop_propagation();
    }

    fn cancel_element_pick(&mut self, cx: &mut Context<Self>) {
        if !self.element_pick_active {
            return;
        }
        self.controller.update(cx, |controller, _| {
            let _ = controller.cancel_element_pick(self.pane, self.active_tab);
        });
        self.element_pick_active = false;
    }

    fn open_context_menu(
        &mut self,
        request: &ContextMenuRequest,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.element_pick_active {
            return;
        }
        let Some(bounds) = self.content_bounds else {
            return;
        };
        let position = bounds.origin + pane_offset(request.x, request.y);
        let browser = cx.entity().downgrade();
        let action_context = self.focus_handle.clone();
        let link = request.link_url.clone();
        let has_selection = request.selection_text.is_some();
        let editable = request.editable;
        let flags = request.edit_flags;
        let can_go_back = self.can_go_back;
        let can_go_forward = self.can_go_forward;
        let (inspect_x, inspect_y) = (request.x, request.y);

        let menu = PopupMenu::build(window, cx, move |mut menu, _, _| {
            menu = menu.action_context(action_context);
            if let Some(link) = link {
                let address = link.clone();
                menu = menu
                    .item(browser_menu_item("Open link", true, &browser, {
                        move |view, cx| view.submit_address(&link, cx)
                    }))
                    .item(
                        PopupMenuItem::new("Copy link address").on_click(move |_, _, cx| {
                            cx.write_to_clipboard(ClipboardItem::new_string(address.to_string()));
                        }),
                    )
                    .separator();
            }
            if editable {
                menu = menu
                    .item(browser_menu_item(
                        "Cut",
                        flags.can_cut,
                        &browser,
                        |view, cx| {
                            view.edit(EditCommand::Cut, cx);
                        },
                    ))
                    .item(browser_menu_item(
                        "Copy",
                        flags.can_copy,
                        &browser,
                        |view, cx| view.edit(EditCommand::Copy, cx),
                    ))
                    .item(browser_menu_item(
                        "Paste",
                        flags.can_paste,
                        &browser,
                        |view, cx| view.edit(EditCommand::Paste, cx),
                    ))
                    .item(browser_menu_item(
                        "Select all",
                        flags.can_select_all,
                        &browser,
                        |view, cx| view.edit(EditCommand::SelectAll, cx),
                    ))
                    .separator();
            } else if has_selection {
                menu = menu
                    .item(browser_menu_item(
                        "Copy",
                        flags.can_copy,
                        &browser,
                        |view, cx| view.edit(EditCommand::Copy, cx),
                    ))
                    .separator();
            }
            menu.item(browser_menu_item(
                "Back",
                can_go_back,
                &browser,
                BrowserView::on_back,
            ))
            .item(browser_menu_item(
                "Forward",
                can_go_forward,
                &browser,
                BrowserView::on_forward,
            ))
            .item(browser_menu_item(
                "Reload",
                true,
                &browser,
                BrowserView::on_reload,
            ))
            .separator()
            .item(browser_menu_item(
                "Inspect element",
                true,
                &browser,
                move |view, cx| view.inspect_element_at(inspect_x, inspect_y, cx),
            ))
        });

        let subscription = cx.subscribe(&menu, |view, _, _: &DismissEvent, cx| {
            view.context_menu = None;
            cx.notify();
        });
        menu.focus_handle(cx).focus(window, cx);
        self.context_menu = Some(PageContextMenu {
            menu,
            position,
            _subscription: subscription,
        });
    }

    fn edit(&mut self, command: EditCommand, cx: &mut Context<Self>) {
        self.controller.update(cx, |controller, cx| {
            controller.edit(self.pane, self.active_tab, command, cx);
        });
    }

    fn inspect_element_at(&mut self, x: i32, y: i32, cx: &mut Context<Self>) {
        self.controller.update(cx, |controller, _| {
            controller.inspect_element_at(self.pane, self.active_tab, x, y);
        });
    }

    fn emit_notification(
        &self,
        message: impl Into<String>,
        kind: ClientMessageKind,
        cx: &mut Context<Self>,
    ) {
        self.mux.update(cx, |_, cx| {
            MuxClient::emit_notification(kind, message, cx);
        });
    }

    fn prompt_cookie_import(window: &mut Window, cx: &mut Context<Self>) {
        let selected = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Choose a Cookie-Editor JSON or cookies.txt file".into()),
        });
        let background = cx.background_executor().clone();
        let view = cx.entity();
        cx.spawn_in(window, async move |_, window| {
            let path = selected.await.ok()?.ok()??.into_iter().next()?;
            let result = background
                .spawn(async move { read_cookie_import(&path) })
                .await;

            window
                .update(|_, cx| {
                    view.update(cx, |view, cx| match result {
                        Ok(batch) => {
                            let pane = view.pane;
                            let tab = view.active_tab;
                            view.controller.update(cx, |controller, cx| {
                                controller.import_cookies(pane, tab, batch, cx);
                            });
                        }
                        Err(error) => {
                            view.emit_notification(error, ClientMessageKind::Error, cx);
                        }
                    });
                })
                .ok()
        })
        .detach();
    }

    fn import_chrome_data(
        &self,
        source_profile: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let background = cx.background_executor().clone();
        let view = cx.entity();
        let destination_profile = self.profile.clone();
        self.emit_notification(
            "Reading cookies and history from Chrome…",
            ClientMessageKind::Info,
            cx,
        );
        cx.spawn_in(window, async move |_, window| {
            let (history_result, cookie_result) = background
                .spawn(async move {
                    let limits = chrome_import::history::ImportLimits {
                        max_count: chrome_import::history::MAX_HISTORY_IMPORT_COUNT,
                        max_url_bytes: recent_pages::MAX_URL_BYTES,
                        max_title_bytes: recent_pages::MAX_TITLE_BYTES,
                    };
                    (
                        chrome_import::history::import_history(&source_profile, limits),
                        chrome_import::cookie::import_all_cookies(&source_profile),
                    )
                })
                .await;
            window
                .update(|window, cx| {
                    // `window` only feeds the macOS Chrome-permission prompt.
                    #[cfg(not(target_os = "macos"))]
                    let _ = &window;
                    let permission_denied = cfg!(target_os = "macos")
                        && (history_result.as_ref().is_err_and(
                            chrome_import::history::ChromeHistoryImportError::is_permission_denied,
                        ) || cookie_result.as_ref().is_err_and(
                            chrome_import::cookie::ChromeCookieImportError::is_permission_denied,
                        ));
                    view.update(cx, |view, cx| {
                        match history_result {
                            Ok(imported) => {
                                let pages = imported
                                    .pages
                                    .into_iter()
                                    .map(|page| {
                                        RecentPage::imported(
                                            destination_profile.clone(),
                                            page.url,
                                            page.title,
                                            page.visited_at,
                                            page.visit_count,
                                            page.typed_count,
                                        )
                                    })
                                    .collect();
                                let changed = recent_pages::import_history(pages, cx);
                                let message = if changed == 0 {
                                    format!(
                                        "Chrome history is already up to date{}",
                                        ignored_suffix(imported.skipped)
                                    )
                                } else {
                                    format!(
                                        "Imported {changed} history entries{}",
                                        ignored_suffix(imported.skipped)
                                    )
                                };
                                view.emit_notification(message, ClientMessageKind::Success, cx);
                            }
                            Err(error) if permission_denied && error.is_permission_denied() => {}
                            Err(error) => view.emit_notification(
                                format!("Could not import Chrome history: {error}"),
                                ClientMessageKind::Error,
                                cx,
                            ),
                        }
                        match cookie_result {
                            Ok(batch) => {
                                let pane = view.pane;
                                let tab = view.active_tab;
                                view.controller.update(cx, |controller, cx| {
                                    controller.import_cookies(pane, tab, batch, cx);
                                });
                            }
                            Err(error) if permission_denied && error.is_permission_denied() => {}
                            Err(error) => view.emit_notification(
                                format!("Could not import Chrome cookies: {error}"),
                                ClientMessageKind::Error,
                                cx,
                            ),
                        }
                    });
                    #[cfg(target_os = "macos")]
                    if permission_denied {
                        prompt_for_chrome_data_access(window, cx);
                    }
                })
                .ok()
        })
        .detach();
    }

    fn refresh_chrome_profiles(
        &mut self,
        notify_completion: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.chrome_profile_discovery == ChromeProfileDiscovery::Loading {
            return;
        }
        self.chrome_profile_discovery = ChromeProfileDiscovery::Loading;
        let background = cx.background_executor().clone();
        let view = cx.entity();
        cx.spawn_in(window, async move |_, window| {
            let result = background
                .spawn(async move {
                    match chrome_import::profiles::discover_profiles() {
                        Ok(profiles) => {
                            if let Err(error) =
                                chrome_import::profiles::save_cached_profiles(&profiles)
                            {
                                log::warn!("could not cache Chrome profile labels: {error}");
                            }
                            (profiles, None)
                        }
                        Err(error) => {
                            log::warn!("could not discover Chrome profiles: {error}");
                            let profiles = chrome_import::profiles::load_cached_profiles()
                                .unwrap_or_else(|cache_error| {
                                    log::warn!(
                                        "could not load cached Chrome profile labels: {cache_error}"
                                    );
                                    Vec::new()
                                });
                            (profiles, Some(error.to_string()))
                        }
                    }
                })
                .await;
            window
                .update(|_, cx| {
                    view.update(cx, |view, cx| {
                        let (profiles, error) = result;
                        view.chrome_profiles = profiles;
                        view.chrome_profile_discovery = if error.is_some() {
                            ChromeProfileDiscovery::Failed
                        } else {
                            ChromeProfileDiscovery::Loaded
                        };
                        if notify_completion {
                            if let Some(error) = error {
                                view.emit_notification(error, ClientMessageKind::Error, cx);
                            } else {
                                let message = match view.chrome_profiles.len() {
                                    0 => "No Chrome profiles found".to_owned(),
                                    1 => "Loaded 1 Chrome profile".to_owned(),
                                    count => format!("Loaded {count} Chrome profiles"),
                                };
                                view.emit_notification(message, ClientMessageKind::Success, cx);
                            }
                        }
                        cx.notify();
                    });
                })
                .ok()
        })
        .detach();
    }

    fn switch_profile(&mut self, profile: String, cx: &mut Context<Self>) {
        if profile != self.profile {
            self.mux.read(cx).execute(CommandInvocation::new(
                "set-browser-profile",
                ["-t".to_owned(), self.pane.to_string(), profile],
            ));
        }
    }

    fn confirm_clear_site_data(window: &mut Window, cx: &mut Context<Self>) {
        let view = cx.entity();
        window.open_alert_dialog(cx, move |alert, _, cx| {
            let view = view.clone();
            browser_clear_site_data_alert(alert, cx).on_ok(move |_, _, cx| {
                view.update(cx, |view, cx| {
                    let pane = view.pane;
                    let tab = view.active_tab;
                    view.controller.update(cx, |controller, cx| {
                        controller.clear_site_data(pane, tab, cx);
                    });
                });
                true
            })
        });
    }

    fn on_page_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.focus_page(window, cx);
        self.page_buttons_down |= pointer_button_bit(event.button);
        self.controller.update(cx, |controller, _| {
            if let Some(event) = self.pointer_event(
                event.position,
                PointerPhase::Down,
                Some(event.button),
                event.click_count,
                event.modifiers,
                Some(event.button),
            ) {
                controller.send_pointer(self.pane, self.active_tab, event);
            }
        });
        cx.stop_propagation();
    }

    fn on_page_mouse_up(&mut self, event: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.release_page_button(event, cx);
        cx.stop_propagation();
    }

    fn on_page_mouse_up_out(
        &mut self,
        event: &MouseUpEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.page_buttons_down != 0 {
            self.release_page_button(event, cx);
        }
    }

    fn release_page_button(&mut self, event: &MouseUpEvent, cx: &mut Context<Self>) {
        self.page_buttons_down &= !pointer_button_bit(event.button);
        if let Some(event) = self.pointer_event(
            event.position,
            PointerPhase::Up,
            Some(event.button),
            event.click_count,
            event.modifiers,
            None,
        ) {
            self.controller.update(cx, |controller, _| {
                controller.send_pointer(self.pane, self.active_tab, event);
            });
        }
    }

    fn on_page_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !owns_pressed_button(self.page_buttons_down, event.pressed_button) {
            return;
        }
        if let Some(event) = self.pointer_event(
            event.position,
            PointerPhase::Move,
            None,
            0,
            event.modifiers,
            event.pressed_button,
        ) {
            self.controller.update(cx, |controller, _| {
                controller.send_pointer(self.pane, self.active_tab, event);
            });
        }
    }

    #[allow(
        clippy::trivially_copy_pass_by_ref,
        reason = "GPUI's hover callback supplies &bool"
    )]
    fn on_page_hover(&mut self, hovered: &bool, window: &mut Window, cx: &mut Context<Self>) {
        if !hovered
            && let Some(event) = self.pointer_event(
                window.mouse_position(),
                PointerPhase::Leave,
                None,
                0,
                gpui::Modifiers::default(),
                None,
            )
        {
            self.controller.update(cx, |controller, _| {
                controller.send_pointer(self.pane, self.active_tab, event);
            });
        }
    }

    fn on_page_scroll(&mut self, event: &ScrollWheelEvent, _: &mut Window, cx: &mut Context<Self>) {
        let Some((x, y)) = self.local_point(event.position) else {
            return;
        };
        let delta = event.delta.pixel_delta(px(20.0));
        let wheel = WheelEvent {
            x,
            y,
            delta_x: rounded_coordinate(delta.x),
            delta_y: rounded_coordinate(delta.y),
            precise: event.delta.precise(),
            modifiers: browser_modifiers(event.modifiers, None, false),
        };
        self.controller.update(cx, |controller, cx| {
            controller.send_wheel(self.pane, self.active_tab, wheel, cx);
        });
        cx.stop_propagation();
    }

    fn on_page_key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let key = browser_key(&event.keystroke.key);
        let allow_text_input = allows_text_input(key, event.keystroke.modifiers);
        self.mux
            .read(cx)
            .send_input(InputMessage::BrowserSurfaceKey {
                pane: self.pane,
                input: terminal_key_input(
                    &event.keystroke,
                    if event.is_held {
                        TerminalKeyAction::Repeat
                    } else {
                        TerminalKeyAction::Press
                    },
                ),
                text_follows: allow_text_input,
            });
        if !allow_text_input {
            cx.stop_propagation();
        }
    }

    fn on_page_key_up(&mut self, event: &KeyUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.mux
            .read(cx)
            .send_input(InputMessage::BrowserSurfaceKey {
                pane: self.pane,
                input: terminal_key_input(&event.keystroke, TerminalKeyAction::Release),
                text_follows: false,
            });
        cx.stop_propagation();
    }

    fn local_point(&self, point: Point<Pixels>) -> Option<(i32, i32)> {
        self.local_point_with_clamping(point, false)
    }

    fn local_point_with_clamping(&self, point: Point<Pixels>, clamp: bool) -> Option<(i32, i32)> {
        let bounds = self.content_bounds?;
        if !clamp && !bounds.contains(&point) {
            return None;
        }
        Some((
            rounded_coordinate(point.x.clamp(bounds.origin.x, bounds.right()) - bounds.origin.x),
            rounded_coordinate(point.y.clamp(bounds.origin.y, bounds.bottom()) - bounds.origin.y),
        ))
    }

    fn pointer_event(
        &self,
        position: Point<Pixels>,
        phase: PointerPhase,
        button: Option<MouseButton>,
        click_count: usize,
        modifiers: gpui::Modifiers,
        pressed: Option<MouseButton>,
    ) -> Option<PointerEvent> {
        let (x, y) =
            self.local_point_with_clamping(position, pointer_needs_clamping(phase, pressed))?;
        Some(PointerEvent {
            x,
            y,
            phase,
            button: button.and_then(pointer_button),
            click_count: i32::try_from(click_count).unwrap_or(i32::MAX),
            modifiers: browser_modifiers(modifiers, pressed, false),
        })
    }

    fn render_omnibox_results(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.address_editing || self.omnibox.suggestions.is_empty() {
            return None;
        }
        let view = cx.entity();
        let rows = self
            .omnibox
            .suggestions
            .iter()
            .enumerate()
            .map(|(index, suggestion)| {
                let view = view.clone();
                browser_omnibox_row(
                    index,
                    suggestion.title.clone(),
                    suggestion.display_url.clone(),
                    self.omnibox.selected == Some(index),
                    cx,
                )
                .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                    view.update(cx, |view, cx| {
                        view.accept_omnibox_suggestion(index, window, cx);
                    });
                    cx.stop_propagation();
                })
                .into_any_element()
            })
            .collect();
        Some(browser_omnibox_panel(rows, cx).into_any_element())
    }

    fn render_empty_state(
        &self,
        radii: Corners<Pixels>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let recents = dedupe_recents(
            recent_pages::recent(&self.profile, cx, EMPTY_STATE_RECENT_LIMIT * 8),
            EMPTY_STATE_RECENT_LIMIT,
        );
        let address_focus_handle = self.address.read(cx).focus_handle(cx);
        let empty_state_view = cx.entity();
        let content: AnyElement = if recents.is_empty() {
            BrowserEmptyHint.into_any_element()
        } else {
            div()
                .flex()
                .flex_col()
                .w_full()
                .max_w(px(360.0))
                .gap(px(4.0))
                .children(
                    recents
                        .iter()
                        .enumerate()
                        .map(|(index, page)| {
                            Self::render_recent_row(index, page, cx).into_any_element()
                        })
                        .collect::<Vec<_>>(),
                )
                .into_any_element()
        };
        round_div_radii(
            div()
                .absolute()
                .inset_0()
                .occlude()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .px(px(12.0))
                .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                    empty_state_view.update(cx, |view, cx| view.select_pane(cx));
                    address_focus_handle.focus(window, cx);
                })
                .child(content),
            radii,
        )
    }

    fn render_recent_row(
        index: usize,
        page: &RecentPage,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let url = page.url.clone();
        let view = cx.entity();
        browser_recent_row(("browser-recent", index), display_url(&page.url), cx).on_mouse_down(
            MouseButton::Left,
            move |_, window, cx| {
                view.update(cx, |view, cx| {
                    view.submit_address(&url, cx);
                    view.focus_page(window, cx);
                });
                cx.stop_propagation();
            },
        )
    }
}

impl Focusable for BrowserView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl EntityInputHandler for BrowserView {
    fn text_for_range(
        &mut self,
        _: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        actual_range.replace(0..0);
        Some(String::new())
    }

    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: 0..0,
            reversed: false,
        })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.marked_text
            .as_ref()
            .map(|text| 0..text.encode_utf16().count())
    }

    fn unmark_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.marked_text = None;
        if std::mem::take(&mut self.prompt_composition) {
            cx.notify();
            return;
        }
        self.controller.update(cx, |controller, cx| {
            controller.finish_composition(self.pane, self.active_tab, cx);
        });
        cx.notify();
    }

    fn replace_text_in_range(
        &mut self,
        _: Option<Range<usize>>,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let was_composing = self.marked_text.take().is_some();
        let prompt_composition = std::mem::take(&mut self.prompt_composition);
        let prompt_active = self.mux.read(cx).command_prompt().is_some();
        if prompt_composition || prompt_active {
            if !text.is_empty() {
                self.mux
                    .read(cx)
                    .send_input(InputMessage::BrowserSurfaceText {
                        pane: self.pane,
                        text: text.to_owned(),
                    });
            }
        } else if was_composing {
            self.controller.update(cx, |controller, cx| {
                controller.commit_composition(self.pane, self.active_tab, text, cx);
            });
        } else if !text.is_empty() {
            self.mux
                .read(cx)
                .send_input(InputMessage::BrowserSurfaceText {
                    pane: self.pane,
                    text: text.to_owned(),
                });
        }
        window.invalidate_character_coordinates();
        cx.notify();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        _: Option<Range<usize>>,
        text: &str,
        selected: Option<Range<usize>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.marked_text.is_none() {
            self.prompt_composition = self.mux.read(cx).command_prompt().is_some();
        }
        self.marked_text = (!text.is_empty()).then(|| text.to_owned());
        if self.prompt_composition {
            if text.is_empty() {
                self.prompt_composition = false;
            }
            window.invalidate_character_coordinates();
            cx.notify();
            return;
        }
        let selection = selected.unwrap_or_else(|| {
            let end = text.encode_utf16().count();
            end..end
        });
        self.controller.update(cx, |controller, cx| {
            if text.is_empty() {
                controller.cancel_composition(self.pane, self.active_tab, cx);
            } else {
                controller.set_composition(self.pane, self.active_tab, text, selection, cx);
            }
        });
        window.invalidate_character_coordinates();
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        _: Range<usize>,
        _: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        self.content_bounds.map(|bounds| {
            Bounds::new(
                bounds.origin,
                gpui::size(px(1.0), px(18.0).min(bounds.size.height)),
            )
        })
    }

    fn character_index_for_point(
        &mut self,
        _: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        Some(0)
    }
}

impl Render for BrowserChromeView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let back_browser = self.browser.clone();
        let back = browser_toolbar_button(
            cx,
            "browser-back",
            IconName::ArrowLeft,
            "Back",
            !self.state.can_go_back,
            false,
        )
        .on_click(move |_, _, cx| {
            if let Some(browser) = back_browser.upgrade() {
                browser.update(cx, BrowserView::on_back);
            }
        });
        let forward_browser = self.browser.clone();
        let forward = browser_toolbar_button(
            cx,
            "browser-forward",
            IconName::ArrowRight,
            "Forward",
            !self.state.can_go_forward,
            false,
        )
        .on_click(move |_, _, cx| {
            if let Some(browser) = forward_browser.upgrade() {
                browser.update(cx, BrowserView::on_forward);
            }
        });
        let reload_browser = self.browser.clone();
        let reload = browser_toolbar_button(
            cx,
            "browser-reload",
            IconName::Redo2,
            "Reload",
            false,
            false,
        )
        .on_click(move |_, _, cx| {
            if let Some(browser) = reload_browser.upgrade() {
                browser.update(cx, BrowserView::on_reload);
            }
        });
        let picker_browser = self.browser.clone();
        let picker = browser_toolbar_button(
            cx,
            "browser-element-picker",
            IconName::Inspector,
            if self.state.element_pick_active {
                "Cancel element picker"
            } else {
                "Pick an element"
            },
            false,
            self.state.element_pick_active,
        )
        .on_click(move |_, window, cx| {
            if let Some(browser) = picker_browser.upgrade() {
                browser.update(cx, |view, cx| view.on_element_pick(window, cx));
            }
        });
        let menu_browser = self.browser.clone();
        let menu_url = self.state.current_url.clone();
        let menu_profile = self.state.profile.clone();
        let menu_chrome_profiles = self.state.chrome_profiles.clone();
        let menu_discovery = self.state.chrome_profile_discovery;
        let menu_zoom_percent = self.state.page_zoom_percent;
        let can_clear_site_data = has_http_origin(&self.state.current_url);
        let can_import_chrome_data = chrome_import::cookie::automatic_import_supported();
        let menu_picker_active = self.state.element_pick_active;
        let more = browser_toolbar_button(
            cx,
            "browser-more",
            IconName::EllipsisVertical,
            "More browser actions",
            false,
            false,
        )
        .dropdown_menu(move |menu, window, cx| {
            let open_url = menu_url.clone();
            let copy_url = menu_url.clone();
            let current_profile_label = menu_chrome_profiles
                .iter()
                .find(|profile| profile.zz_profile == menu_profile)
                .map_or_else(
                    || {
                        if menu_profile == zz_browser::DEFAULT_BROWSER_PROFILE {
                            "Default zz profile".to_owned()
                        } else {
                            menu_profile.clone()
                        }
                    },
                    chrome_import::profiles::DetectedChromeProfile::menu_label,
                );
            let state = BrowserActionMenuState {
                current_profile_label: current_profile_label.into(),
                selected_profile: menu_profile.clone().into(),
                default_profile: zz_browser::DEFAULT_BROWSER_PROFILE.into(),
                profiles: menu_chrome_profiles
                    .iter()
                    .map(|profile| {
                        BrowserMenuProfile::new(profile.zz_profile.clone(), profile.menu_label())
                    })
                    .collect(),
                profile_discovery: match menu_discovery {
                    ChromeProfileDiscovery::Loading => BrowserProfileDiscoveryState::Loading,
                    ChromeProfileDiscovery::Failed => BrowserProfileDiscoveryState::Failed,
                    ChromeProfileDiscovery::NotStarted | ChromeProfileDiscovery::Loaded => {
                        BrowserProfileDiscoveryState::Ready
                    }
                },
                zoom_percent: menu_zoom_percent,
                can_import_chrome_data,
                can_clear_site_data,
                picker_active: menu_picker_active,
            };
            let open_browser = menu_browser.clone();
            let copy_browser = menu_browser.clone();
            let switch_browser = menu_browser.clone();
            let profile_refresh_browser = menu_browser.clone();
            let zoom_in_browser = menu_browser.clone();
            let zoom_out_browser = menu_browser.clone();
            let reset_zoom_browser = menu_browser.clone();
            let chrome_import_browser = menu_browser.clone();
            let file_import_browser = menu_browser.clone();
            let clear_browser = menu_browser.clone();
            let reload_browser = menu_browser.clone();
            let picker_browser = menu_browser.clone();
            let dev_tools_browser = menu_browser.clone();
            let actions = BrowserMenuActions::new()
                .open_url(move |_, cx| {
                    if open_browser.upgrade().is_some() {
                        cx.open_url(&open_url);
                    }
                })
                .copy_url(move |_, cx| {
                    if copy_browser.upgrade().is_some() {
                        cx.write_to_clipboard(ClipboardItem::new_string(copy_url.clone()));
                    }
                })
                .switch_profile(move |profile, _, cx| {
                    if let Some(browser) = switch_browser.upgrade() {
                        browser.update(cx, |view, cx| {
                            view.switch_profile(profile.to_string(), cx);
                        });
                    }
                })
                .refresh_profiles(move |window, cx| {
                    let profile_refresh_browser = profile_refresh_browser.clone();
                    window.defer(cx, move |window, cx| {
                        if let Some(browser) = profile_refresh_browser.upgrade() {
                            browser.update(cx, |view, cx| {
                                view.refresh_chrome_profiles(true, window, cx);
                            });
                        }
                    });
                })
                .zoom_in(move |_, cx| {
                    if let Some(browser) = zoom_in_browser.upgrade() {
                        browser.update(cx, |view, cx| view.apply_zoom(ZoomOp::In, cx));
                    }
                })
                .zoom_out(move |_, cx| {
                    if let Some(browser) = zoom_out_browser.upgrade() {
                        browser.update(cx, |view, cx| view.apply_zoom(ZoomOp::Out, cx));
                    }
                })
                .reset_zoom(move |_, cx| {
                    if let Some(browser) = reset_zoom_browser.upgrade() {
                        browser.update(cx, |view, cx| view.apply_zoom(ZoomOp::Reset, cx));
                    }
                })
                .import_chrome_data(move |source_profile, window, cx| {
                    if let Some(browser) = chrome_import_browser.upgrade() {
                        browser.update(cx, |view, cx| {
                            view.import_chrome_data(source_profile.to_string(), window, cx);
                        });
                    }
                })
                .import_cookies(move |window, cx| {
                    if let Some(browser) = file_import_browser.upgrade() {
                        browser.update(cx, |_, cx| {
                            BrowserView::prompt_cookie_import(window, cx);
                        });
                    }
                })
                .clear_site_data(move |window, cx| {
                    if let Some(browser) = clear_browser.upgrade() {
                        browser.update(cx, |_, cx| {
                            BrowserView::confirm_clear_site_data(window, cx);
                        });
                    }
                })
                .reload(move |_, cx| {
                    if let Some(browser) = reload_browser.upgrade() {
                        browser.update(cx, BrowserView::on_reload);
                    }
                })
                .toggle_picker(move |window, cx| {
                    if let Some(browser) = picker_browser.upgrade() {
                        browser.update(cx, |view, cx| view.on_element_pick(window, cx));
                    }
                })
                .dev_tools(move |_, cx| {
                    if let Some(browser) = dev_tools_browser.upgrade() {
                        browser.update(cx, BrowserView::toggle_dev_tools);
                    }
                });
            browser_action_menu(menu, window, cx, state, actions)
        })
        .anchor(Anchor::TopRight);
        let address_browser = self.browser.clone();
        let activate_browser = self.browser.clone();
        let close_browser = self.browser.clone();
        let new_tab_browser = self.browser.clone();
        let tabs = BrowserTabStrip::new(
            &self.address,
            self.state.tabs.clone(),
            self.state.active_tab_index,
        )
        .on_address_mouse_down(move |_, cx| {
            if let Some(browser) = address_browser.upgrade() {
                browser.update(cx, |view, cx| view.select_pane(cx));
            }
        })
        .on_activate(move |id, window, cx| {
            if let Some(browser) = activate_browser.upgrade() {
                browser.update(cx, |view, cx| view.activate_tab(TabId(id), window, cx));
            }
        })
        .on_close(move |id, window, cx| {
            if let Some(browser) = close_browser.upgrade() {
                browser.update(cx, |view, cx| view.close_tab_by_id(TabId(id), window, cx));
            }
        })
        .on_new_tab(move |window, cx| {
            if let Some(browser) = new_tab_browser.upgrade() {
                browser.update(cx, |view, cx| view.open_tab(None, true, window, cx));
            }
        });

        BrowserToolbar::new(back, forward, reload, tabs, picker, more)
    }
}

impl Render for BrowserView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let show_fps = resolved_config(cx).show_fps.value;
        let shows_empty_state = self.shows_empty_state();
        let shows_native_state = shows_empty_state || self.error.is_some();
        self.browser_fps.set_enabled(show_fps, Instant::now());
        if self.chrome_profile_discovery == ChromeProfileDiscovery::NotStarted {
            self.refresh_chrome_profiles(false, window, cx);
        }
        let (tabs, active_tab_index) = self.tab_strip_state();
        let snapshot = ChromeState {
            can_go_back: self.can_go_back,
            can_go_forward: self.can_go_forward,
            element_pick_active: self.element_pick_active,
            current_url: self.current_url.clone(),
            profile: self.profile.clone(),
            chrome_profiles: self.chrome_profiles.clone(),
            chrome_profile_discovery: self.chrome_profile_discovery,
            page_zoom_percent: self.page_zoom_percent,
            tabs,
            active_tab_index,
        };
        if self.chrome.read(cx).state != snapshot {
            self.chrome.update(cx, |chrome, cx| {
                chrome.state = snapshot;
                cx.notify();
            });
        }
        let content_radii = pane_content_radii(cx, self.window_corners);
        let surface_radii = Corners {
            top_left: px(0.0),
            top_right: px(0.0),
            ..content_radii
        };
        let started = diagnostics::timer(DIAGNOSTIC_TARGET);
        let retired_count = self.retired_images.len();
        let mut reclaimed = Vec::new();
        for image in self.retired_images.drain(..) {
            if let Err(error) = window.drop_image(image.clone()) {
                log::warn!("failed to release superseded browser image: {error}");
            }
            if let Ok(image) = Arc::try_unwrap(image) {
                reclaimed.extend(
                    image
                        .into_frames()
                        .into_iter()
                        .map(|frame| frame.into_buffer().into_raw()),
                );
            }
        }
        if !reclaimed.is_empty() {
            let pane = self.pane;
            let tab = self.active_tab;
            self.controller.update(cx, |controller, _| {
                for bgra in reclaimed {
                    controller.recycle_frame(pane, tab, bgra);
                }
            });
        }
        log::trace!(
            target: "zz::diagnostics::browser_render",
            "render pane={} viewport={:?} visible={} image_generation={} has_image={} has_gpu_texture={} has_mac_surface={} retired_images_released={} url={:?} title={:?} loading={} focused={} elapsed_before_tree_us={}",
            self.pane,
            self.viewport,
            self.visible,
            self.image_generation,
            self.image.is_some(),
            {
                #[cfg(any(target_os = "linux", target_os = "freebsd"))]
                {
                    self.gpu_texture.is_some()
                }
                #[cfg(target_os = "windows")]
                {
                    self.win_gpu_texture.is_some()
                }
                #[cfg(not(any(
                    target_os = "linux",
                    target_os = "freebsd",
                    target_os = "windows"
                )))]
                {
                    false
                }
            },
            {
                #[cfg(target_os = "macos")]
                {
                    self.mac_surface.is_some()
                }
                #[cfg(not(target_os = "macos"))]
                {
                    false
                }
            },
            retired_count,
            self.current_url,
            self.title,
            self.loading,
            self.focus_handle.is_focused(window),
            diagnostics::elapsed_us(started),
        );

        let browser_surface = div()
            .id("browser-content")
            .relative()
            .flex()
            .flex_1()
            .overflow_hidden()
            .cursor(self.cursor)
            .track_focus(&self.focus_handle)
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_page_mouse_down))
            .on_mouse_down(MouseButton::Right, cx.listener(Self::on_page_mouse_down))
            .on_mouse_down(MouseButton::Middle, cx.listener(Self::on_page_mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_page_mouse_up))
            .on_mouse_up(MouseButton::Right, cx.listener(Self::on_page_mouse_up))
            .on_mouse_up(MouseButton::Middle, cx.listener(Self::on_page_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_page_mouse_up_out))
            .on_mouse_up_out(MouseButton::Right, cx.listener(Self::on_page_mouse_up_out))
            .on_mouse_up_out(MouseButton::Middle, cx.listener(Self::on_page_mouse_up_out))
            .on_mouse_move(cx.listener(Self::on_page_mouse_move))
            .on_hover(cx.listener(Self::on_page_hover))
            .on_scroll_wheel(cx.listener(Self::on_page_scroll))
            .on_key_down(cx.listener(Self::on_page_key_down))
            .on_key_up(cx.listener(Self::on_page_key_up));
        #[cfg(any(target_os = "linux", target_os = "freebsd"))]
        let browser_surface = browser_surface.children(
            self.gpu_texture
                .clone()
                .filter(|_| !shows_native_state)
                .map(|texture| {
                    round_div_radii(
                        external_texture(texture)
                            .object_fit(ObjectFit::Fill)
                            .absolute()
                            .inset_0(),
                        surface_radii,
                    )
                }),
        );
        #[cfg(target_os = "windows")]
        let browser_surface = browser_surface.children(
            self.win_gpu_texture
                .clone()
                .filter(|_| !shows_native_state)
                .map(|texture| {
                    round_div_radii(
                        external_texture(texture)
                            .object_fit(ObjectFit::Fill)
                            .absolute()
                            .inset_0(),
                        surface_radii,
                    )
                }),
        );
        let visible_image = if shows_native_state {
            None
        } else {
            self.image.clone()
        };
        let browser_element = BrowserElement::new(cx.entity(), visible_image, surface_radii);
        #[cfg(target_os = "macos")]
        let browser_element = browser_element.surface(if shows_native_state {
            None
        } else {
            self.mac_surface.clone()
        });
        let mut content = round_div_radii(browser_surface.child(browser_element), surface_radii);

        if let Some(error) = self.error.clone() {
            let retry = self.recoverable.then(|| {
                let retry_view = cx.entity();
                Button::new("browser-retry")
                    .primary()
                    .small()
                    .icon(IconName::Redo2)
                    .label("Try again")
                    .on_click(move |_, _, cx| {
                        retry_view.update(cx, BrowserView::on_retry);
                    })
            });
            let mut error_panel = BrowserErrorPanel::new(error.to_string());
            if let Some(retry) = retry {
                error_panel = error_panel.retry(retry);
            }
            let panel = round_div_radii(
                div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .px(px(20.0))
                    .child(error_panel),
                surface_radii,
            );
            content = content.child(panel);
        }

        if shows_empty_state {
            content = content.child(self.render_empty_state(surface_radii, cx));
        }

        if self.element_pick_active {
            content = content.child(BrowserPickStatus::new("Select an element · Esc to cancel"));
        }

        if show_fps {
            content = content.child(
                div()
                    .absolute()
                    .top(px(8.0))
                    .right(px(8.0))
                    .child(frame_rate_badge("CEF", self.browser_fps.fps(), cx)),
            );
        }

        let omnibox_results = self.render_omnibox_results(cx);
        round_div_radii(
            div()
                .id("browser-root")
                .key_context(BROWSER_KEY_CONTEXT)
                .relative()
                .flex()
                .flex_col()
                .size_full()
                .bg(crate::theme::app_pane_background(cx))
                .on_action(cx.listener(Self::on_zoom_in))
                .on_action(cx.listener(Self::on_zoom_out))
                .on_action(cx.listener(Self::on_reset_zoom))
                .on_action(cx.listener(Self::on_toggle_dev_tools))
                .on_action(cx.listener(Self::on_configured_element_selector))
                .on_action(cx.listener(Self::on_new_tab))
                .on_action(cx.listener(Self::on_close_pane))
                .on_action(cx.listener(Self::on_next_tab))
                .on_action(cx.listener(Self::on_previous_tab))
                .on_action(cx.listener(Self::on_select_tab))
                .on_action(cx.listener(Self::on_select_last_tab))
                .on_action(cx.listener(Self::on_go_back))
                .on_action(cx.listener(Self::on_go_forward))
                .on_action(cx.listener(Self::on_reload_action))
                .on_action(cx.listener(Self::on_focus_address))
                .on_action(cx.listener(Self::on_omnibox_next))
                .on_action(cx.listener(Self::on_omnibox_previous))
                .on_action(cx.listener(Self::on_omnibox_delete))
                .on_action(cx.listener(Self::on_browser_edit))
                .on_key_down(cx.listener(Self::on_root_key_down))
                .child(
                    div()
                        .relative()
                        .h(BrowserToolbar::HEIGHT)
                        .w_full()
                        .flex_none()
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|view, _, window, cx| {
                                view.focus_page(window, cx);
                            }),
                        )
                        .child(
                            AnyView::from(self.chrome.clone()).cached(
                                StyleRefinement::default()
                                    .h(BrowserToolbar::HEIGHT)
                                    .w_full()
                                    .flex_none(),
                            ),
                        ),
                )
                .child(content)
                .children(omnibox_results)
                .children(self.context_menu.as_ref().map(|context_menu| {
                    deferred(
                        anchored()
                            .position(context_menu.position)
                            .snap_to_window_with_margin(px(8.0))
                            .child(context_menu.menu.clone()),
                    )
                    .with_priority(1)
                })),
            content_radii,
        )
    }
}

fn display_url(url: &str) -> String {
    diagnostic_url(url)
}

fn dedupe_recents(pages: Vec<RecentPage>, limit: usize) -> Vec<RecentPage> {
    let mut seen = HashSet::new();
    pages
        .into_iter()
        .filter(|page| seen.insert(display_url(&page.url)))
        .take(limit)
        .collect()
}

fn has_http_origin(url: &str) -> bool {
    url.starts_with("http://") || url.starts_with("https://")
}

#[allow(
    clippy::cast_precision_loss,
    reason = "pane coordinates stay far below f32's exact integer range"
)]
fn pane_offset(x: i32, y: i32) -> Point<Pixels> {
    point(px(x as f32), px(y as f32))
}

fn browser_menu_item(
    label: &'static str,
    enabled: bool,
    browser: &WeakEntity<BrowserView>,
    action: impl Fn(&mut BrowserView, &mut Context<BrowserView>) + 'static,
) -> PopupMenuItem {
    let browser = browser.clone();
    PopupMenuItem::new(label)
        .disabled(!enabled)
        .on_click(move |_, _, cx| {
            if let Some(browser) = browser.upgrade() {
                browser.update(cx, |view, cx| action(view, cx));
            }
        })
}

fn ignored_suffix(ignored: usize) -> String {
    if ignored > 0 {
        format!("; {ignored} skipped")
    } else {
        String::new()
    }
}

fn read_cookie_import(path: &Path) -> Result<CookieImportBatch, String> {
    let file = File::open(path).map_err(|error| format!("Could not read cookie file: {error}"))?;
    let mut input = String::new();
    file.take((MAX_COOKIE_IMPORT_BYTES + 1) as u64)
        .read_to_string(&mut input)
        .map_err(|error| format!("Could not read cookie file: {error}"))?;
    parse_cookie_import(&input).map_err(|error| format!("Could not import cookies: {error}"))
}

const fn pointer_needs_clamping(phase: PointerPhase, pressed: Option<MouseButton>) -> bool {
    match phase {
        PointerPhase::Up => true,
        PointerPhase::Move => pressed.is_some(),
        PointerPhase::Down | PointerPhase::Leave => false,
    }
}

const fn pointer_button_bit(button: MouseButton) -> u8 {
    match button {
        MouseButton::Left | MouseButton::Navigate(_) => 1,
        MouseButton::Right => 1 << 1,
        MouseButton::Middle => 1 << 2,
    }
}

const fn owns_pressed_button(buttons: u8, pressed: Option<MouseButton>) -> bool {
    match pressed {
        Some(button) => buttons & pointer_button_bit(button) != 0,
        None => true,
    }
}

fn pointer_button(button: MouseButton) -> Option<PointerButton> {
    match button {
        MouseButton::Left => Some(PointerButton::Left),
        MouseButton::Middle => Some(PointerButton::Middle),
        MouseButton::Right => Some(PointerButton::Right),
        MouseButton::Navigate(_) => None,
    }
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    reason = "CEF view dimensions are bounded integers derived from finite GPUI pixels"
)]
fn rounded_dimension(value: Pixels) -> u32 {
    f32::from(value).round().clamp(1.0, u32::MAX as f32) as u32
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    reason = "CEF input coordinates are saturating integer device-independent pixels"
)]
fn rounded_coordinate(value: Pixels) -> i32 {
    f32::from(value)
        .round()
        .clamp(i32::MIN as f32, i32::MAX as f32) as i32
}

fn browser_input_from_terminal(input: &TerminalKeyInput) -> KeyInput {
    let key = match input.key {
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
    };
    KeyInput {
        action: match input.action {
            TerminalKeyAction::Press | TerminalKeyAction::Repeat => KeyAction::Press,
            TerminalKeyAction::Release => KeyAction::Release,
        },
        key,
        modifiers: Modifiers::new(
            input.modifiers.shift(),
            input.modifiers.control(),
            input.modifiers.alt(),
            input.modifiers.platform(),
        )
        .with_repeat(input.action == TerminalKeyAction::Repeat),
    }
}

fn browser_modifiers(
    value: gpui::Modifiers,
    pressed: Option<MouseButton>,
    is_repeat: bool,
) -> Modifiers {
    let pressed = pressed.map(|button| match button {
        MouseButton::Left | MouseButton::Navigate(_) => PointerButton::Left,
        MouseButton::Right => PointerButton::Right,
        MouseButton::Middle => PointerButton::Middle,
    });
    Modifiers::new(value.shift, value.control, value.alt, value.platform)
        .with_pointer_button(pressed)
        .with_repeat(is_repeat)
}

fn browser_key(key: &str) -> BrowserKey {
    match key {
        "space" => BrowserKey::Space,
        "backspace" => BrowserKey::Backspace,
        "tab" => BrowserKey::Tab,
        "enter" => BrowserKey::Enter,
        "escape" => BrowserKey::Escape,
        "pageup" => BrowserKey::PageUp,
        "pagedown" => BrowserKey::PageDown,
        "end" => BrowserKey::End,
        "home" => BrowserKey::Home,
        "left" => BrowserKey::ArrowLeft,
        "up" => BrowserKey::ArrowUp,
        "right" => BrowserKey::ArrowRight,
        "down" => BrowserKey::ArrowDown,
        "insert" => BrowserKey::Insert,
        "delete" => BrowserKey::Delete,
        _ => {
            let mut characters = key.chars();
            if let (Some(character), None) = (characters.next(), characters.next()) {
                BrowserKey::Character(character)
            } else {
                key.strip_prefix('f')
                    .and_then(|number| number.parse::<u8>().ok())
                    .map_or(BrowserKey::Unidentified, BrowserKey::Function)
            }
        }
    }
}

fn browser_named_key(name: &str) -> Option<KeyInput> {
    let mut modifiers = Modifiers::default();
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
    let gpui_name = match name {
        "Enter" => "enter",
        "Escape" => "escape",
        "Space" => "space",
        "Tab" => "tab",
        "BSpace" => "backspace",
        "Up" => "up",
        "Down" => "down",
        "Left" => "left",
        "Right" => "right",
        "Home" => "home",
        "End" => "end",
        "PPage" => "pageup",
        "NPage" => "pagedown",
        "DC" => "delete",
        "IC" => "insert",
        value if value.chars().count() == 1 => value,
        value if value.strip_prefix('F').is_some() => {
            return Some(KeyInput {
                action: KeyAction::Press,
                key: BrowserKey::Function(value.strip_prefix('F')?.parse().ok()?),
                modifiers,
            });
        }
        _ => return None,
    };
    Some(KeyInput {
        action: KeyAction::Press,
        key: browser_key(gpui_name),
        modifiers,
    })
}

fn allows_text_input(key: BrowserKey, modifiers: gpui::Modifiers) -> bool {
    !modifiers.control
        && !modifiers.alt
        && !modifiers.platform
        && matches!(
            key,
            BrowserKey::Character(_) | BrowserKey::Space | BrowserKey::Unidentified
        )
}

fn cursor_style(cursor: BrowserCursor) -> CursorStyle {
    match cursor {
        BrowserCursor::Arrow | BrowserCursor::Wait | BrowserCursor::Help | BrowserCursor::None => {
            CursorStyle::Arrow
        }
        BrowserCursor::IBeam => CursorStyle::IBeam,
        BrowserCursor::PointingHand => CursorStyle::PointingHand,
        BrowserCursor::Crosshair => CursorStyle::Crosshair,
        BrowserCursor::Move | BrowserCursor::Grab => CursorStyle::OpenHand,
        BrowserCursor::Grabbing => CursorStyle::ClosedHand,
        BrowserCursor::ResizeHorizontal => CursorStyle::ResizeLeftRight,
        BrowserCursor::ResizeVertical => CursorStyle::ResizeUpDown,
        BrowserCursor::ResizeNorthEastSouthWest => CursorStyle::ResizeUpRightDownLeft,
        BrowserCursor::ResizeNorthWestSouthEast => CursorStyle::ResizeUpLeftDownRight,
        BrowserCursor::NotAllowed => CursorStyle::OperationNotAllowed,
    }
}

#[cfg(test)]
mod tests {
    use std::{any::TypeId, cell::RefCell, collections::HashSet, rc::Rc};

    #[cfg(not(target_os = "macos"))]
    use gpui::VisualTestContext;
    use gpui::{AsKeystroke, KeyContext, Keymap, Keystroke, TestAppContext};
    use zz_browser::BrowserError;
    #[cfg(not(target_os = "macos"))]
    use zz_browser::SessionId;
    use zz_daemon::DaemonError;
    use zz_ui::Root;

    use super::*;

    gpui::actions!(
        browser_view_test,
        [FocusNext, FocusPrevious, RootCopy, InputEdit]
    );

    #[test]
    fn element_picker_appearance_uses_resolved_theme_tokens() {
        let mut theme = zz_ui::Theme::default();
        theme.colors.background = zz_ui::parse_hex("#102030").unwrap();
        theme.colors.foreground = zz_ui::parse_hex("#abcdef").unwrap();
        theme.colors.border = zz_ui::parse_hex("#345678").unwrap();
        theme.colors.scrim = zz_ui::parse_hex("#01020366").unwrap();
        theme.radius = px(17.0);
        theme.shadow = true;
        theme.mono_font_family = "Iosevka".into();

        let appearance = element_picker_appearance(&theme, 1.25);

        assert_eq!(appearance.highlight_outline, "#abcdef");
        assert_eq!(appearance.highlight_fill, "#abcdef1f");
        assert_eq!(appearance.highlight_contrast, "#102030");
        assert_eq!(
            appearance.preview_background,
            to_hex(theme.background.raised(1).opaque())
        );
        assert_eq!(appearance.preview_foreground, "#abcdef");
        assert_eq!(appearance.preview_border, "#345678");
        assert_eq!(appearance.shadow.as_deref(), Some("#01020366"));
        assert_eq!(appearance.radius, 17.0);
        assert_eq!(appearance.font_family, "Iosevka");
        assert_eq!(appearance.page_zoom, 1.25);

        theme.shadow = false;
        assert_eq!(element_picker_appearance(&theme, 1.0).shadow, None);
    }

    #[test]
    fn recognizes_incremental_text_edits_without_treating_bulk_replacements_as_typing() {
        assert!(differs_by_one_character_edit("exa", "exam"));
        assert!(differs_by_one_character_edit("exam", "exa"));
        assert!(differs_by_one_character_edit("exam", "exXm"));
        assert!(differs_by_one_character_edit("café", "cafés"));
        assert!(!differs_by_one_character_edit("", "example.com"));
        assert!(!differs_by_one_character_edit("example", "github"));
        assert!(!differs_by_one_character_edit("same", "same"));
    }

    #[test]
    fn dedupes_recent_pages_by_display_url_before_applying_the_limit() {
        let recent = |url: &str, title: &str, visited_at| {
            RecentPage::imported(
                zz_browser::DEFAULT_BROWSER_PROFILE,
                url.to_owned(),
                title.to_owned(),
                visited_at,
                1,
                0,
            )
        };
        let pages = vec![
            recent("http://localhost:3000/insights?a=1", "Newest insights", 4),
            recent("http://localhost:3000/insights?b=2", "Older insights", 3),
            recent("http://localhost:3000/settings#profile", "Settings", 2),
            recent("http://localhost:3000/help", "Help", 1),
        ];

        let deduped = dedupe_recents(pages, 2);

        assert_eq!(
            deduped,
            vec![
                recent("http://localhost:3000/insights?a=1", "Newest insights", 4,),
                recent("http://localhost:3000/settings#profile", "Settings", 2),
            ]
        );
    }

    fn bindings_for_contexts(
        keymap: &Keymap,
        source: &str,
        context_names: &[&str],
    ) -> Vec<gpui::KeyBinding> {
        let keystroke = Keystroke::parse(source).expect("valid browser keystroke");
        let contexts = context_names
            .iter()
            .map(|context| KeyContext::parse(context).expect("valid key context"))
            .collect::<Vec<_>>();
        let (bindings, pending) = keymap.bindings_for_input(&[keystroke], &contexts);
        assert!(!pending);
        bindings.into_vec()
    }

    fn browser_bindings_for(keymap: &Keymap, source: &str) -> Vec<gpui::KeyBinding> {
        bindings_for_contexts(keymap, source, &["Root", BROWSER_KEY_CONTEXT])
    }

    fn assert_action_types(bindings: &[KeyBinding], expected: &[TypeId]) {
        let actual = bindings
            .iter()
            .map(|binding| binding.action().as_any().type_id())
            .collect::<Vec<_>>();
        assert_eq!(actual, expected);
    }

    fn assert_binding<A: gpui::Action>(keymap: &Keymap, source: &str) {
        let bindings = browser_bindings_for(keymap, source);
        assert_eq!(bindings.len(), 1, "expected one binding for {source}");
        assert_eq!(bindings[0].action().as_any().type_id(), TypeId::of::<A>());
    }

    fn browser_key_bindings() -> Vec<KeyBinding> {
        let mut bindings = raw_key_bindings().to_vec();
        bindings.extend(browser_chrome_bindings(&crate::keymap::test_chords(
            BROWSER_TABLE,
        )));
        bindings
    }

    fn browser_binding_identity(binding: &KeyBinding) -> String {
        let action = binding.action();
        if let Some(action) = action.as_any().downcast_ref::<BrowserEdit>() {
            return format!("edit:{:?}", action.command);
        }
        if let Some(action) = action.as_any().downcast_ref::<SelectTab>() {
            return format!("tab:{}", action.index);
        }
        if action
            .as_any()
            .downcast_ref::<ConfiguredElementSelector>()
            .is_some()
        {
            return "element-selector".to_owned();
        }
        for (type_id, name) in [
            (TypeId::of::<NoAction>(), "raw-tab"),
            (TypeId::of::<ZoomIn>(), "zoom-in"),
            (TypeId::of::<ZoomOut>(), "zoom-out"),
            (TypeId::of::<ResetZoom>(), "zoom-reset"),
            (TypeId::of::<ToggleDevTools>(), "devtools"),
            (TypeId::of::<NewTab>(), "new-tab"),
            (TypeId::of::<ClosePane>(), "close-pane"),
            (TypeId::of::<NextTab>(), "next-tab"),
            (TypeId::of::<PreviousTab>(), "previous-tab"),
            (TypeId::of::<SelectLastTab>(), "last-tab"),
            (TypeId::of::<FocusAddress>(), "focus-address"),
            (TypeId::of::<Reload>(), "reload"),
            (TypeId::of::<GoBack>(), "back"),
            (TypeId::of::<GoForward>(), "forward"),
            (TypeId::of::<OmniboxNext>(), "omnibox-next"),
            (TypeId::of::<OmniboxPrevious>(), "omnibox-previous"),
            (TypeId::of::<OmniboxDelete>(), "omnibox-delete"),
        ] {
            if action.as_any().type_id() == type_id {
                return name.to_owned();
            }
        }
        panic!("unrecognized browser binding action: {}", action.name());
    }

    fn audited_binding(source: &str, action: &str) -> (Keystroke, String) {
        (
            Keystroke::parse(source).expect("valid audited browser keystroke"),
            action.to_owned(),
        )
    }

    #[test]
    fn only_a_pointer_cef_is_still_holding_survives_leaving_the_page() {
        assert!(pointer_needs_clamping(PointerPhase::Up, None));
        assert!(pointer_needs_clamping(
            PointerPhase::Move,
            Some(MouseButton::Left)
        ));
        assert!(!pointer_needs_clamping(PointerPhase::Move, None));
        assert!(!pointer_needs_clamping(
            PointerPhase::Down,
            Some(MouseButton::Left)
        ));
        assert!(!pointer_needs_clamping(PointerPhase::Leave, None));
    }

    #[test]
    fn only_a_button_the_page_pressed_carries_its_motion() {
        let mut buttons = 0;
        assert!(!owns_pressed_button(buttons, Some(MouseButton::Left)));
        assert!(owns_pressed_button(buttons, None));

        buttons |= pointer_button_bit(MouseButton::Left);
        assert!(owns_pressed_button(buttons, Some(MouseButton::Left)));
        assert!(!owns_pressed_button(buttons, Some(MouseButton::Right)));
        assert!(owns_pressed_button(buttons, None));

        buttons &= !pointer_button_bit(MouseButton::Left);
        assert!(!owns_pressed_button(buttons, Some(MouseButton::Left)));
    }

    #[test]
    fn page_zoom_shortcuts_use_the_platform_browser_convention() {
        let keymap = Keymap::new(browser_key_bindings());
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        let (zoom_in, zoom_plus, zoom_out, reset) = ("cmd-=", "cmd-+", "cmd--", "cmd-0");
        #[cfg(not(any(target_os = "macos", target_os = "ios")))]
        let (zoom_in, zoom_plus, zoom_out, reset) = ("ctrl-=", "ctrl-+", "ctrl--", "ctrl-0");

        assert_binding::<ZoomIn>(&keymap, zoom_in);
        assert_binding::<ZoomIn>(&keymap, zoom_plus);
        assert_binding::<ZoomOut>(&keymap, zoom_out);
        assert_binding::<ResetZoom>(&keymap, reset);
    }

    #[test]
    fn browser_keymap_matches_the_audited_supported_set() {
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        let expected = [
            ("tab", "raw-tab"),
            ("shift-tab", "raw-tab"),
            ("cmd-z", "edit:Undo"),
            ("cmd-shift-z", "edit:Redo"),
            ("cmd-x", "edit:Cut"),
            ("cmd-c", "edit:Copy"),
            ("cmd-v", "edit:Paste"),
            ("cmd-shift-v", "edit:PasteAndMatchStyle"),
            ("cmd-a", "edit:SelectAll"),
            ("cmd-=", "zoom-in"),
            ("cmd-+", "zoom-in"),
            ("cmd--", "zoom-out"),
            ("cmd-0", "zoom-reset"),
            ("cmd-alt-i", "devtools"),
            ("cmd-t", "new-tab"),
            ("ctrl-tab", "next-tab"),
            ("ctrl-shift-tab", "previous-tab"),
            ("cmd-alt-right", "next-tab"),
            ("cmd-alt-left", "previous-tab"),
            ("cmd-shift-]", "next-tab"),
            ("cmd-shift-[", "previous-tab"),
            ("cmd-9", "last-tab"),
            ("cmd-l", "focus-address"),
            ("cmd-r", "reload"),
            ("cmd-[", "back"),
            ("cmd-]", "forward"),
            ("cmd-shift-c", "element-selector"),
            ("cmd-1", "tab:0"),
            ("cmd-2", "tab:1"),
            ("cmd-3", "tab:2"),
            ("cmd-4", "tab:3"),
            ("cmd-5", "tab:4"),
            ("cmd-6", "tab:5"),
            ("cmd-7", "tab:6"),
            ("cmd-8", "tab:7"),
            ("down", "omnibox-next"),
            ("up", "omnibox-previous"),
            ("shift-delete", "omnibox-delete"),
        ];
        #[cfg(not(any(target_os = "macos", target_os = "ios")))]
        let expected = [
            ("tab", "raw-tab"),
            ("shift-tab", "raw-tab"),
            ("ctrl-z", "edit:Undo"),
            ("ctrl-y", "edit:Redo"),
            ("ctrl-shift-z", "edit:Redo"),
            ("ctrl-x", "edit:Cut"),
            ("ctrl-c", "edit:Copy"),
            ("ctrl-v", "edit:Paste"),
            ("ctrl-shift-v", "edit:PasteAndMatchStyle"),
            ("ctrl-a", "edit:SelectAll"),
            ("ctrl-=", "zoom-in"),
            ("ctrl-+", "zoom-in"),
            ("ctrl--", "zoom-out"),
            ("ctrl-0", "zoom-reset"),
            ("ctrl-shift-i", "devtools"),
            ("ctrl-t", "new-tab"),
            ("ctrl-w", "close-pane"),
            ("ctrl-tab", "next-tab"),
            ("ctrl-shift-tab", "previous-tab"),
            ("ctrl-pagedown", "next-tab"),
            ("ctrl-pageup", "previous-tab"),
            ("ctrl-9", "last-tab"),
            ("ctrl-l", "focus-address"),
            ("ctrl-r", "reload"),
            ("f5", "reload"),
            ("alt-left", "back"),
            ("alt-right", "forward"),
            ("ctrl-shift-c", "element-selector"),
            ("ctrl-1", "tab:0"),
            ("ctrl-2", "tab:1"),
            ("ctrl-3", "tab:2"),
            ("ctrl-4", "tab:3"),
            ("ctrl-5", "tab:4"),
            ("ctrl-6", "tab:5"),
            ("ctrl-7", "tab:6"),
            ("ctrl-8", "tab:7"),
            ("down", "omnibox-next"),
            ("up", "omnibox-previous"),
            ("shift-delete", "omnibox-delete"),
        ];
        let bindings = browser_key_bindings();
        let actual = bindings
            .iter()
            .map(|binding| {
                let [keystroke] = binding.keystrokes() else {
                    panic!("browser bindings must be single keystrokes");
                };
                (
                    keystroke.as_keystroke().clone(),
                    browser_binding_identity(binding),
                )
            })
            .collect::<HashSet<_>>();
        let expected = expected
            .into_iter()
            .map(|(source, action)| audited_binding(source, action))
            .collect::<HashSet<_>>();

        assert_eq!(actual, expected);
        assert_eq!(bindings.len(), expected.len());
    }

    #[test]
    fn browser_copy_outranks_window_text_copy() {
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        let copy = "cmd-c";
        #[cfg(not(any(target_os = "macos", target_os = "ios")))]
        let copy = "ctrl-c";
        let mut bindings = vec![KeyBinding::new(
            copy,
            RootCopy,
            Some(zz_ui::ROOT_KEY_CONTEXT),
        )];
        bindings.extend(browser_key_bindings());
        let keymap = Keymap::new(bindings);
        let bindings = bindings_for_contexts(
            &keymap,
            copy,
            &["Root", zz_ui::ROOT_KEY_CONTEXT, BROWSER_KEY_CONTEXT],
        );

        assert_action_types(
            &bindings,
            &[TypeId::of::<BrowserEdit>(), TypeId::of::<RootCopy>()],
        );
    }

    #[test]
    fn focused_address_field_outranks_browser_page_edits() {
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        let edits = [
            ("cmd-z", EditCommand::Undo),
            ("cmd-shift-z", EditCommand::Redo),
            ("cmd-x", EditCommand::Cut),
            ("cmd-c", EditCommand::Copy),
            ("cmd-v", EditCommand::Paste),
            ("cmd-shift-v", EditCommand::PasteAndMatchStyle),
            ("cmd-a", EditCommand::SelectAll),
        ];
        #[cfg(not(any(target_os = "macos", target_os = "ios")))]
        let edits = [
            ("ctrl-z", EditCommand::Undo),
            ("ctrl-y", EditCommand::Redo),
            ("ctrl-shift-z", EditCommand::Redo),
            ("ctrl-x", EditCommand::Cut),
            ("ctrl-c", EditCommand::Copy),
            ("ctrl-v", EditCommand::Paste),
            ("ctrl-shift-v", EditCommand::PasteAndMatchStyle),
            ("ctrl-a", EditCommand::SelectAll),
        ];
        let mut bindings = edits
            .iter()
            .map(|(source, _)| KeyBinding::new(source, InputEdit, Some("ZzInput")))
            .collect::<Vec<_>>();
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        bindings.push(KeyBinding::new(
            "cmd-c",
            RootCopy,
            Some(zz_ui::ROOT_KEY_CONTEXT),
        ));
        #[cfg(not(any(target_os = "macos", target_os = "ios")))]
        bindings.push(KeyBinding::new(
            "ctrl-c",
            RootCopy,
            Some(zz_ui::ROOT_KEY_CONTEXT),
        ));
        bindings.extend(browser_key_bindings());
        let keymap = Keymap::new(bindings);
        let contexts = [
            "Root",
            zz_ui::ROOT_KEY_CONTEXT,
            BROWSER_KEY_CONTEXT,
            "ZzInput",
        ];

        for (source, command) in edits {
            let resolved = bindings_for_contexts(&keymap, source, &contexts);
            let expected = if command == EditCommand::Copy {
                vec![
                    TypeId::of::<InputEdit>(),
                    TypeId::of::<BrowserEdit>(),
                    TypeId::of::<RootCopy>(),
                ]
            } else {
                vec![TypeId::of::<InputEdit>(), TypeId::of::<BrowserEdit>()]
            };
            assert_action_types(&resolved, &expected);
            assert_eq!(
                resolved[1]
                    .action()
                    .as_any()
                    .downcast_ref::<BrowserEdit>()
                    .map(|action| action.command),
                Some(command)
            );
        }
    }

    #[test]
    fn focused_omnibox_outranks_single_line_input_navigation() {
        let mut bindings = vec![
            KeyBinding::new("down", InputEdit, Some("ZzInput")),
            KeyBinding::new("up", InputEdit, Some("ZzInput")),
            KeyBinding::new("shift-delete", InputEdit, Some("ZzInput")),
        ];
        bindings.extend(browser_key_bindings());
        let keymap = Keymap::new(bindings);
        let contexts = [
            "Root",
            BROWSER_KEY_CONTEXT,
            zz_ui::browser::BROWSER_OMNIBOX_KEY_CONTEXT,
            "ZzInput",
        ];

        for (source, action) in [
            ("down", TypeId::of::<OmniboxNext>()),
            ("up", TypeId::of::<OmniboxPrevious>()),
            ("shift-delete", TypeId::of::<OmniboxDelete>()),
        ] {
            let resolved = bindings_for_contexts(&keymap, source, &contexts);
            assert_eq!(resolved[0].action().as_any().type_id(), action);
            assert_eq!(
                resolved[1].action().as_any().type_id(),
                TypeId::of::<InputEdit>()
            );
        }
    }

    #[test]
    fn page_zoom_outranks_application_ui_scaling_in_the_browser_context() {
        let mut bindings =
            crate::ui_scale::key_bindings(&crate::keymap::test_chords(zz_client::UI_TABLE));
        bindings.extend(browser_key_bindings());
        let keymap = Keymap::new(bindings);
        let contexts = [
            KeyContext::parse(zz_ui::ROOT_KEY_CONTEXT).expect("valid zz root context"),
            KeyContext::parse(BROWSER_KEY_CONTEXT).expect("valid browser context"),
        ];
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        let shortcuts = [
            ("cmd-=", TypeId::of::<ZoomIn>()),
            ("cmd-+", TypeId::of::<ZoomIn>()),
            ("cmd--", TypeId::of::<ZoomOut>()),
            ("cmd-0", TypeId::of::<ResetZoom>()),
        ];
        #[cfg(not(any(target_os = "macos", target_os = "ios")))]
        let shortcuts = [
            ("ctrl-=", TypeId::of::<ZoomIn>()),
            ("ctrl-+", TypeId::of::<ZoomIn>()),
            ("ctrl--", TypeId::of::<ZoomOut>()),
            ("ctrl-0", TypeId::of::<ResetZoom>()),
        ];

        for (source, action_type) in shortcuts {
            let keystroke = Keystroke::parse(source).expect("valid zoom shortcut");
            let (bindings, pending) = keymap.bindings_for_input(&[keystroke], &contexts);
            assert!(!pending);
            assert_eq!(bindings[0].action().as_any().type_id(), action_type);
        }
    }

    #[test]
    fn tab_shortcuts_use_the_platform_browser_convention() {
        let keymap = Keymap::new(browser_key_bindings());

        assert_binding::<NextTab>(&keymap, "ctrl-tab");
        assert_binding::<PreviousTab>(&keymap, "ctrl-shift-tab");
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        {
            assert_binding::<NewTab>(&keymap, "cmd-t");
            assert!(browser_bindings_for(&keymap, "cmd-w").is_empty());
            assert_binding::<NextTab>(&keymap, "cmd-alt-right");
            assert_binding::<PreviousTab>(&keymap, "cmd-alt-left");
            assert_binding::<NextTab>(&keymap, "cmd-shift-]");
            assert_binding::<PreviousTab>(&keymap, "cmd-shift-[");
            assert_binding::<SelectLastTab>(&keymap, "cmd-9");
        }
        #[cfg(not(any(target_os = "macos", target_os = "ios")))]
        {
            assert_binding::<NewTab>(&keymap, "ctrl-t");
            assert_binding::<ClosePane>(&keymap, "ctrl-w");
            assert_binding::<NextTab>(&keymap, "ctrl-pagedown");
            assert_binding::<PreviousTab>(&keymap, "ctrl-pageup");
            assert_binding::<SelectLastTab>(&keymap, "ctrl-9");
        }
    }

    #[test]
    fn navigation_shortcuts_use_the_platform_browser_convention() {
        let keymap = Keymap::new(browser_key_bindings());

        #[cfg(any(target_os = "macos", target_os = "ios"))]
        let (back, forward, reload, address) = ("cmd-[", "cmd-]", "cmd-r", "cmd-l");
        #[cfg(not(any(target_os = "macos", target_os = "ios")))]
        let (back, forward, reload, address) = ("alt-left", "alt-right", "ctrl-r", "ctrl-l");

        assert_binding::<GoBack>(&keymap, back);
        assert_binding::<GoForward>(&keymap, forward);
        assert_binding::<Reload>(&keymap, reload);
        assert_binding::<FocusAddress>(&keymap, address);
    }

    #[test]
    fn configured_element_selector_binding_is_browser_scoped_and_carries_its_chord() {
        let hotkey = crate::config::DEFAULT_BROWSER_ELEMENT_SELECTOR_HOTKEY;
        let keymap = Keymap::new(browser_key_bindings());
        let bindings = browser_bindings_for(&keymap, hotkey);
        let chord = if cfg!(any(target_os = "macos", target_os = "ios")) {
            "D-S-c"
        } else {
            "C-S-c"
        };

        assert_eq!(bindings.len(), 1);
        assert_eq!(
            bindings[0]
                .action()
                .as_any()
                .downcast_ref::<ConfiguredElementSelector>(),
            Some(&ConfiguredElementSelector {
                chord: chord.to_owned()
            }),
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn cmd_w_resolves_to_close_pane_in_every_pane_context() {
        let mut bindings = browser_key_bindings();
        bindings.extend(crate::macos_app::key_bindings());
        let keymap = Keymap::new(bindings);

        for pane_context in ["Browser", "Terminal", "Editor", "Agent"] {
            let contexts = [
                KeyContext::parse("Root").expect("valid root context"),
                KeyContext::parse(pane_context).expect("valid pane context"),
            ];
            let keystroke = Keystroke::parse("cmd-w").expect("valid keystroke");
            let (bindings, pending) = keymap.bindings_for_input(&[keystroke], &contexts);
            assert!(!pending);
            assert_eq!(
                bindings[0].action().as_any().type_id(),
                TypeId::of::<ClosePane>(),
                "cmd-w must dispatch ClosePane in the {pane_context} context"
            );
        }
    }

    #[test]
    fn digit_shortcuts_jump_straight_to_tab_slots() {
        let keymap = Keymap::new(browser_key_bindings());
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        let modifier = "cmd";
        #[cfg(not(any(target_os = "macos", target_os = "ios")))]
        let modifier = "ctrl";

        for index in 0..8 {
            let source = format!("{modifier}-{}", index + 1);
            let bindings = browser_bindings_for(&keymap, &source);
            assert_eq!(bindings.len(), 1, "expected one binding for {source}");
            assert_eq!(
                bindings[0].action().as_any().downcast_ref::<SelectTab>(),
                Some(&SelectTab { index }),
            );
        }
    }

    #[gpui::test]
    fn first_blank_pane_edit_updates_omnibox_before_focus_event(cx: &mut TestAppContext) {
        cx.update(|cx| {
            zz_ui::init(cx);
            cx.set_global(recent_pages::RecentPages::default());
            recent_pages::record_visit(
                zz_browser::DEFAULT_BROWSER_PROFILE,
                "https://example.com",
                cx,
            );
        });
        let view_slot = Rc::new(RefCell::new(None));
        let captured_view = Rc::clone(&view_slot);
        let pane = PaneId(7);
        let (_, cx) = cx.add_window_view(move |window, cx| {
            let controller =
                cx.new(|cx| BrowserController::new(Err(BrowserError::AlreadyShutdown), cx));
            let mux = cx.new(|cx| {
                MuxClient::new(
                    Err(DaemonError::Thread("test client".to_owned())),
                    zz_daemon::default_socket_path(),
                    cx,
                )
            });
            let view = cx.new(|cx| {
                BrowserView::new(
                    pane,
                    &BrowserDescriptor::single(
                        DEFAULT_URL.to_owned(),
                        zz_browser::DEFAULT_BROWSER_PROFILE.to_owned(),
                    ),
                    controller,
                    mux,
                    window,
                    cx,
                )
            });
            captured_view.replace(Some(view.clone()));
            Root::new(view, window, cx)
        });
        let view = view_slot.borrow().clone().expect("captured browser view");
        let address = cx.update(|_, cx| view.read(cx).address.clone());

        cx.update(|window, cx| {
            assert!(!view.read(cx).address_editing);
            address.update(cx, |address, cx| {
                address.insert("exa", window, cx);
            });
        });
        cx.run_until_parked();
        cx.update(|_, cx| {
            let view = view.read(cx);
            assert!(view.address_editing);
            assert_eq!(view.omnibox.suggestions.len(), 1);
            assert_eq!(view.omnibox.suggestions[0].url, "https://example.com");
        });
    }

    #[cfg(not(target_os = "macos"))]
    #[gpui::test]
    fn component_chrome_renders_and_navigation_preserves_active_address_edits(
        cx: &mut TestAppContext,
    ) {
        cx.update(zz_ui::init);
        let view_slot = Rc::new(RefCell::new(None));
        let controller_slot = Rc::new(RefCell::new(None));
        let captured_view = Rc::clone(&view_slot);
        let captured_controller = Rc::clone(&controller_slot);
        let pane = PaneId(7);
        let (_, cx) = cx.add_window_view(move |window, cx| {
            let controller =
                cx.new(|cx| BrowserController::new(Err(BrowserError::AlreadyShutdown), cx));
            let mux = cx.new(|cx| {
                MuxClient::new(
                    Err(DaemonError::Thread("test client".to_owned())),
                    zz_daemon::default_socket_path(),
                    cx,
                )
            });
            let view = cx.new(|cx| {
                let descriptor = zz_protocol::BrowserDescriptor {
                    tabs: vec!["example.com".to_owned()],
                    active_tab: 0,
                    profile: "default".to_owned(),
                };
                BrowserView::new(pane, &descriptor, controller.clone(), mux, window, cx)
            });
            view.update(cx, |view, _| view.error = None);
            captured_view.replace(Some(view.clone()));
            captured_controller.replace(Some(controller));
            Root::new(view, window, cx)
        });
        let cx: &mut VisualTestContext = cx;
        let view = view_slot.borrow().clone().expect("captured browser view");
        let controller = controller_slot
            .borrow()
            .clone()
            .expect("captured browser controller");

        assert_eq!(
            cx.update(|_, cx| view.read(cx).address.read(cx).value().to_string()),
            "https://example.com/"
        );

        cx.update(|window, cx| {
            view.update(cx, |view, cx| {
                view.address_editing = true;
                view.address.update(cx, |address, cx| {
                    address.set_value("typing.example/path", window, cx);
                });
                view.handle_controller_event(
                    &controller,
                    &ControllerEvent::Browser {
                        pane,
                        tab: TAB_ID,
                        event: BrowserEvent::AddressChanged {
                            session: SessionId(1),
                            url: Arc::from("https://redirect.example"),
                        },
                    },
                    window,
                    cx,
                );
            });
        });
        assert_eq!(
            cx.update(|_, cx| view.read(cx).address.read(cx).value().to_string()),
            "typing.example/path"
        );

        cx.update(|window, cx| {
            view.update(cx, |view, cx| {
                view.address_editing = false;
                view.handle_controller_event(
                    &controller,
                    &ControllerEvent::Browser {
                        pane,
                        tab: TAB_ID,
                        event: BrowserEvent::AddressChanged {
                            session: SessionId(1),
                            url: Arc::from("https://settled.example"),
                        },
                    },
                    window,
                    cx,
                );
            });
            let _ = window.draw(cx);
        });
        cx.run_until_parked();
        assert_eq!(
            cx.update(|_, cx| view.read(cx).address.read(cx).value().to_string()),
            "https://settled.example"
        );
    }

    #[test]
    fn routes_plain_and_ime_keys_through_gpui_text_input() {
        assert!(allows_text_input(
            BrowserKey::Character('a'),
            gpui::Modifiers::default()
        ));
        assert!(allows_text_input(
            BrowserKey::Space,
            gpui::Modifiers {
                shift: true,
                ..Default::default()
            }
        ));
        assert!(allows_text_input(
            BrowserKey::Unidentified,
            gpui::Modifiers::default()
        ));
    }

    #[test]
    fn modified_and_navigation_keys_do_not_emit_committed_text() {
        assert!(!allows_text_input(
            BrowserKey::Character('c'),
            gpui::Modifiers {
                control: true,
                ..Default::default()
            }
        ));
        assert!(!allows_text_input(
            BrowserKey::Character('v'),
            gpui::Modifiers {
                platform: true,
                ..Default::default()
            }
        ));
        assert!(!allows_text_input(
            BrowserKey::ArrowLeft,
            gpui::Modifiers::default()
        ));
    }

    #[test]
    fn browser_context_leaves_tab_for_chromium_raw_key_input() {
        let mut bindings = vec![
            KeyBinding::new("tab", FocusNext, Some("Root")),
            KeyBinding::new("shift-tab", FocusPrevious, Some("Root")),
        ];
        bindings.extend(browser_key_bindings());
        let keymap = Keymap::new(bindings);
        let root_context = KeyContext::parse("Root").expect("valid root key context");
        let browser_context =
            KeyContext::parse(BROWSER_KEY_CONTEXT).expect("valid browser key context");

        for source in ["tab", "shift-tab"] {
            let keystroke = Keystroke::parse(source).expect("valid tab keystroke");
            let (browser_bindings, pending) = keymap.bindings_for_input(
                std::slice::from_ref(&keystroke),
                &[root_context.clone(), browser_context.clone()],
            );
            assert!(browser_bindings.is_empty());
            assert!(!pending);

            let (root_bindings, pending) = keymap.bindings_for_input(
                std::slice::from_ref(&keystroke),
                std::slice::from_ref(&root_context),
            );
            assert_eq!(root_bindings.len(), 1);
            assert!(!pending);
        }
    }
}
