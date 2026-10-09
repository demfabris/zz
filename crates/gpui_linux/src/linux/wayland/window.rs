use std::{
    cell::{Cell, Ref, RefCell, RefMut},
    ffi::c_void,
    ptr::NonNull,
    rc::Rc,
    sync::{Arc, LazyLock},
};

use calloop::ping::Ping;
use collections::{FxHashMap, HashMap};
use futures::channel::oneshot::Receiver;

use raw_window_handle as rwh;
use wayland_backend::client::ObjectId;
use wayland_client::WEnum;
use wayland_client::{
    Proxy,
    protocol::{wl_callback, wl_output, wl_seat, wl_surface},
};
use wayland_protocols::wp::viewporter::client::wp_viewport;
use wayland_protocols::xdg::decoration::zv1::client::zxdg_toplevel_decoration_v1;
use wayland_protocols::xdg::shell::client::xdg_popup;
use wayland_protocols::xdg::shell::client::xdg_positioner;
use wayland_protocols::xdg::shell::client::xdg_surface;
use wayland_protocols::xdg::shell::client::xdg_toplevel::{self};
use wayland_protocols::{
    ext::background_effect::v1::client::ext_background_effect_surface_v1,
    wp::fractional_scale::v1::client::wp_fractional_scale_v1,
    xdg::dialog::v1::client::xdg_dialog_v1::XdgDialogV1,
};
use wayland_protocols_plasma::blur::client::org_kde_kwin_blur;
use wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_surface_v1;

use crate::linux::wayland::{display::WaylandDisplay, serial::SerialKind};
use crate::linux::{Globals, Output, WaylandClientStatePtr, get_window};
use gpui::{
    AnyWindowHandle, Bounds, Capslock, Corners, Decorations, DevicePixels, ExternalDragPayload,
    GpuSpecs, Modifiers, Pixels, PlatformAtlas, PlatformDisplay, PlatformInput,
    PlatformInputHandler, PlatformWindow, Point, PromptButton, PromptLevel, RequestFrameOptions,
    ResizeEdge, Scene, Size, Tiling, WgpuDeviceContext, WindowAppearance,
    WindowBackgroundAppearance, WindowBounds, WindowControlArea, WindowControls, WindowDecorations,
    WindowKind, WindowParams, WindowVisibility,
    layer_shell::{Anchor, LayerShellNotSupportedError},
    point,
    popup::PopupOptions,
    px, size,
};
use gpui_wgpu::{CompositorGpuHint, WgpuRenderer, WgpuSurfaceConfig, wgpu};

#[derive(Default)]
pub(crate) struct Callbacks {
    request_frame: Option<Box<dyn FnMut(RequestFrameOptions)>>,
    input: Option<Box<dyn FnMut(gpui::PlatformInput) -> gpui::DispatchEventResult>>,
    active_status_change: Option<Box<dyn FnMut(bool)>>,
    visibility_change: Option<Box<dyn FnMut(WindowVisibility)>>,
    hover_status_change: Option<Box<dyn FnMut(bool)>>,
    resize: Option<Box<dyn FnMut(Size<Pixels>, f32)>>,
    moved: Option<Box<dyn FnMut()>>,
    should_close: Option<Box<dyn FnMut() -> bool>>,
    close: Option<Box<dyn FnOnce()>>,
    appearance_changed: Option<Box<dyn FnMut()>>,
    button_layout_changed: Option<Box<dyn FnMut()>>,
}

#[derive(Debug, Clone, Copy)]
struct RawWindow {
    window: *mut c_void,
    display: *mut c_void,
}

// Safety: The raw pointers in RawWindow point to Wayland surface/display
// which are valid for the window's lifetime. These are used only for
// passing to wgpu which needs Send+Sync for surface creation.
unsafe impl Send for RawWindow {}
unsafe impl Sync for RawWindow {}

impl rwh::HasWindowHandle for RawWindow {
    fn window_handle(&self) -> Result<rwh::WindowHandle<'_>, rwh::HandleError> {
        let window = NonNull::new(self.window).unwrap();
        let handle = rwh::WaylandWindowHandle::new(window);
        Ok(unsafe { rwh::WindowHandle::borrow_raw(handle.into()) })
    }
}
impl rwh::HasDisplayHandle for RawWindow {
    fn display_handle(&self) -> Result<rwh::DisplayHandle<'_>, rwh::HandleError> {
        let display = NonNull::new(self.display).unwrap();
        let handle = rwh::WaylandDisplayHandle::new(display);
        Ok(unsafe { rwh::DisplayHandle::borrow_raw(handle.into()) })
    }
}

#[derive(Debug)]
struct InProgressConfigure {
    size: Option<Size<Pixels>>,
    fullscreen: bool,
    maximized: bool,
    resizing: bool,
    visibility: WindowVisibility,
    tiling: Tiling,
}

pub struct WaylandWindowState {
    surface_state: WaylandSurfaceState,
    parent: Option<WaylandWindowStatePtr>,
    /// Child surfaces mapped to whether they block this window's input (dialogs
    /// block, popups don't). Children are closed before this window closes.
    children: FxHashMap<ObjectId, bool>,
    pub surface: wl_surface::WlSurface,
    app_id: Option<String>,
    appearance: WindowAppearance,
    background_effect: Option<ext_background_effect_surface_v1::ExtBackgroundEffectSurfaceV1>,
    background_effect_region: Option<BackgroundEffectRegion>,
    blur: Option<org_kde_kwin_blur::OrgKdeKwinBlur>,
    viewport: Option<wp_viewport::WpViewport>,
    outputs: HashMap<ObjectId, Output>,
    display: Option<(ObjectId, Output)>,
    globals: Globals,
    renderer: WgpuRenderer,
    bounds: Bounds<Pixels>,
    scale: f32,
    input_handler: Option<PlatformInputHandler>,
    decorations: WindowDecorations,
    background_appearance: WindowBackgroundAppearance,
    fullscreen: bool,
    maximized: bool,
    /// `Hidden` while the `xdg_toplevel` `suspended` state (xdg-shell v6) is set.
    visibility: WindowVisibility,
    tiling: Tiling,
    window_bounds: Bounds<Pixels>,
    client: WaylandClientStatePtr,
    handle: AnyWindowHandle,
    active: bool,
    hovered: bool,
    redraw_requested: bool,
    presentation: PresentationState,
    pending_frame_callback: Option<wl_callback::WlCallback>,
    in_progress_configure: Option<InProgressConfigure>,
    resize_throttle: bool,
    in_progress_window_controls: Option<WindowControls>,
    window_controls: WindowControls,
    client_inset: Option<Pixels>,
    accesskit_adapter: Option<accesskit_unix::Adapter>,
}

pub enum WaylandSurfaceState {
    Xdg(WaylandXdgSurfaceState),
    LayerShell(WaylandLayerSurfaceState),
    Popup(WaylandPopupSurfaceState),
}

impl WaylandSurfaceState {
    fn new(
        surface: &wl_surface::WlSurface,
        globals: &Globals,
        params: &WindowParams,
        parent: Option<WaylandWindowStatePtr>,
        popup_grab: Option<(u32, wl_seat::WlSeat)>,
        target_output: Option<wl_output::WlOutput>,
    ) -> anyhow::Result<Self> {
        // For layer_shell windows, create a layer surface instead of an xdg surface
        if let WindowKind::LayerShell(options) = &params.kind {
            let Some(layer_shell) = globals.layer_shell.as_ref() else {
                return Err(LayerShellNotSupportedError.into());
            };

            let layer_surface = layer_shell.get_layer_surface(
                &surface,
                target_output.as_ref(),
                super::layer_shell::wayland_layer(options.layer),
                options.namespace.clone(),
                &globals.qh,
                surface.id(),
            );

            let width = f32::from(params.bounds.size.width);
            let height = f32::from(params.bounds.size.height);
            layer_surface.set_size(width as u32, height as u32);

            layer_surface.set_anchor(super::layer_shell::wayland_anchor(options.anchor));
            layer_surface.set_keyboard_interactivity(
                super::layer_shell::wayland_keyboard_interactivity(options.keyboard_interactivity),
            );

            if let Some(margin) = options.margin {
                layer_surface.set_margin(
                    f32::from(margin.0) as i32,
                    f32::from(margin.1) as i32,
                    f32::from(margin.2) as i32,
                    f32::from(margin.3) as i32,
                )
            }

            if let Some(exclusive_zone) = options.exclusive_zone {
                layer_surface.set_exclusive_zone(f32::from(exclusive_zone) as i32);
            }

            if let Some(exclusive_edge) = options.exclusive_edge {
                Self::apply_exclusive_edge(&layer_surface, options.anchor, exclusive_edge);
            }

            return Ok(WaylandSurfaceState::LayerShell(WaylandLayerSurfaceState {
                layer_surface,
                anchor: options.anchor,
            }));
        }

        if let WindowKind::AnchoredPopup(options) = &params.kind {
            let Some(parent) = parent.as_ref() else {
                return Err(anyhow::anyhow!("popup parent window not found"));
            };

            let positioner = build_popup_positioner(
                globals,
                options,
                params.bounds.size,
                parent.window_geometry(),
            );

            let xdg_surface = globals
                .wm_base
                .get_xdg_surface(&surface, &globals.qh, surface.id());

            // A layer-shell parent takes a null xdg parent and is attached via the layer
            // surface. Every other surface kind has an xdg_surface to parent to directly.
            let xdg_popup = if let Some(parent_layer_surface) = parent.layer_surface() {
                let xdg_popup = xdg_surface.get_popup(None, &positioner, &globals.qh, surface.id());
                parent_layer_surface.get_popup(&xdg_popup);
                xdg_popup
            } else {
                xdg_surface.get_popup(
                    parent.xdg_surface().as_ref(),
                    &positioner,
                    &globals.qh,
                    surface.id(),
                )
            };
            positioner.destroy();

            if let Some((serial, seat)) = popup_grab {
                xdg_popup.grab(&seat, serial);
            }

            // Non-blocking: the parent keeps its input so it can dismiss the popup on
            // clicks in its own window.
            parent.add_child(surface.id(), false);

            return Ok(WaylandSurfaceState::Popup(WaylandPopupSurfaceState {
                xdg_surface,
                xdg_popup,
                options: options.clone(),
                next_reposition_token: Cell::new(0),
            }));
        }

        // All other WindowKinds result in a regular xdg surface
        let xdg_surface = globals
            .wm_base
            .get_xdg_surface(&surface, &globals.qh, surface.id());

        let toplevel = xdg_surface.get_toplevel(&globals.qh, surface.id());
        let xdg_parent = parent.as_ref().and_then(|w| w.toplevel());

        if params.kind == WindowKind::Floating || params.kind == WindowKind::Dialog {
            toplevel.set_parent(xdg_parent.as_ref());
        }

        let dialog = if params.kind == WindowKind::Dialog {
            let dialog = globals.dialog.as_ref().map(|dialog| {
                let xdg_dialog = dialog.get_xdg_dialog(&toplevel, &globals.qh, ());
                xdg_dialog.set_modal();
                xdg_dialog
            });

            if let Some(parent) = parent.as_ref() {
                parent.add_child(surface.id(), true);
            }

            dialog
        } else {
            None
        };

        if let Some(size) = params.window_min_size {
            toplevel.set_min_size(f32::from(size.width) as i32, f32::from(size.height) as i32);
        }

        // Attempt to set up window decorations based on the requested configuration
        let decoration = globals
            .decoration_manager
            .as_ref()
            .map(|decoration_manager| {
                decoration_manager.get_toplevel_decoration(&toplevel, &globals.qh, surface.id())
            });

        Ok(WaylandSurfaceState::Xdg(WaylandXdgSurfaceState {
            xdg_surface,
            toplevel,
            decoration,
            dialog,
        }))
    }
}

pub struct WaylandXdgSurfaceState {
    xdg_surface: xdg_surface::XdgSurface,
    toplevel: xdg_toplevel::XdgToplevel,
    decoration: Option<zxdg_toplevel_decoration_v1::ZxdgToplevelDecorationV1>,
    dialog: Option<XdgDialogV1>,
}

pub struct WaylandLayerSurfaceState {
    layer_surface: zwlr_layer_surface_v1::ZwlrLayerSurfaceV1,
    anchor: Anchor,
}

pub struct WaylandPopupSurfaceState {
    xdg_surface: xdg_surface::XdgSurface,
    xdg_popup: xdg_popup::XdgPopup,
    // Kept so the popup can be re-anchored via `xdg_popup.reposition` when resized.
    options: PopupOptions,
    next_reposition_token: Cell<u32>,
}

fn build_popup_positioner(
    globals: &Globals,
    options: &PopupOptions,
    size: Size<Pixels>,
    parent_geometry: Bounds<Pixels>,
) -> xdg_positioner::XdgPositioner {
    let positioner = globals.wm_base.create_positioner(&globals.qh, ());
    // A zero or negative size is a protocol error.
    positioner.set_size(
        f32::from(size.width).max(1.0) as i32,
        f32::from(size.height).max(1.0) as i32,
    );

    // The protocol wants the anchor rect relative to the parent's window geometry, while
    // `options.anchor_rect` is in gpui window coordinates (surface-local). A rect extending
    // outside the geometry or with a zero size is a protocol error, so translate, then clamp
    // to at least one pixel inside the geometry, pulling the origin inward at the edges.
    let anchor_rect = Bounds {
        origin: options.anchor_rect.origin - parent_geometry.origin,
        size: options.anchor_rect.size,
    };
    let one = Point::new(px(1.0), px(1.0));
    let geometry_bottom_right: Point<Pixels> = parent_geometry.size.into();
    let top_left = anchor_rect
        .origin
        .min(&(geometry_bottom_right - one))
        .max(&Point::default());
    let bottom_right = anchor_rect
        .bottom_right()
        .min(&geometry_bottom_right)
        .max(&(top_left + one));
    let anchor_rect = Bounds::from_corners(top_left, bottom_right);
    positioner.set_anchor_rect(
        f32::from(anchor_rect.origin.x) as i32,
        f32::from(anchor_rect.origin.y) as i32,
        f32::from(anchor_rect.size.width) as i32,
        f32::from(anchor_rect.size.height) as i32,
    );

    positioner.set_anchor(super::popup::wayland_anchor(options.anchor));
    positioner.set_gravity(super::popup::wayland_gravity(options.gravity));
    positioner.set_constraint_adjustment(super::popup::wayland_constraint_adjustment(
        options.constraint_adjustment,
    ));
    positioner.set_offset(
        f32::from(options.offset.x) as i32,
        f32::from(options.offset.y) as i32,
    );
    positioner
}

impl WaylandSurfaceState {
    fn ack_configure(&self, serial: u32) {
        match self {
            WaylandSurfaceState::Xdg(WaylandXdgSurfaceState { xdg_surface, .. }) => {
                xdg_surface.ack_configure(serial);
            }
            WaylandSurfaceState::LayerShell(WaylandLayerSurfaceState { layer_surface, .. }) => {
                layer_surface.ack_configure(serial);
            }
            WaylandSurfaceState::Popup(WaylandPopupSurfaceState { xdg_surface, .. }) => {
                xdg_surface.ack_configure(serial);
            }
        }
    }

    fn decoration(&self) -> Option<&zxdg_toplevel_decoration_v1::ZxdgToplevelDecorationV1> {
        if let WaylandSurfaceState::Xdg(WaylandXdgSurfaceState { decoration, .. }) = self {
            decoration.as_ref()
        } else {
            None
        }
    }

    fn toplevel(&self) -> Option<&xdg_toplevel::XdgToplevel> {
        if let WaylandSurfaceState::Xdg(WaylandXdgSurfaceState { toplevel, .. }) = self {
            Some(toplevel)
        } else {
            None
        }
    }

    fn xdg_surface(&self) -> Option<&xdg_surface::XdgSurface> {
        match self {
            WaylandSurfaceState::Xdg(WaylandXdgSurfaceState { xdg_surface, .. }) => {
                Some(xdg_surface)
            }
            WaylandSurfaceState::Popup(WaylandPopupSurfaceState { xdg_surface, .. }) => {
                Some(xdg_surface)
            }
            WaylandSurfaceState::LayerShell(_) => None,
        }
    }

    fn layer_surface(&self) -> Option<&zwlr_layer_surface_v1::ZwlrLayerSurfaceV1> {
        if let WaylandSurfaceState::LayerShell(WaylandLayerSurfaceState { layer_surface, .. }) =
            self
        {
            Some(layer_surface)
        } else {
            None
        }
    }

    fn set_geometry(&self, x: i32, y: i32, width: i32, height: i32) {
        match self {
            WaylandSurfaceState::Xdg(WaylandXdgSurfaceState { xdg_surface, .. }) => {
                xdg_surface.set_window_geometry(x, y, width, height);
            }
            WaylandSurfaceState::LayerShell(WaylandLayerSurfaceState { layer_surface, .. }) => {
                // cannot set window position of a layer surface
                layer_surface.set_size(width as u32, height as u32);
            }
            WaylandSurfaceState::Popup(WaylandPopupSurfaceState { xdg_surface, .. }) => {
                xdg_surface.set_window_geometry(x, y, width, height);
            }
        }
    }

    // Re-anchors a mapped popup at a new size via `xdg_popup.reposition`. Repositioning an
    // unmapped popup (before the first configure) is a protocol error.
    fn reposition_popup(
        &self,
        globals: &Globals,
        size: Size<Pixels>,
        parent_geometry: Bounds<Pixels>,
    ) {
        if let WaylandSurfaceState::Popup(WaylandPopupSurfaceState {
            xdg_popup,
            options,
            next_reposition_token,
            ..
        }) = self
            && xdg_popup.version() >= xdg_popup::REQ_REPOSITION_SINCE
        {
            let token = next_reposition_token.get();
            next_reposition_token.set(token.wrapping_add(1));

            let positioner = build_popup_positioner(globals, options, size, parent_geometry);
            xdg_popup.reposition(&positioner, token);
            positioner.destroy();
        }
    }

    fn set_exclusive_zone(&self, zone: i32) -> bool {
        if let WaylandSurfaceState::LayerShell(WaylandLayerSurfaceState { layer_surface, .. }) =
            self
        {
            layer_surface.set_exclusive_zone(zone);
            true
        } else {
            false
        }
    }

    /// An exclusive edge must be a single edge that the surface is anchored to,
    /// otherwise the compositor raises a fatal `invalid_exclusive_edge` protocol
    /// error. An invalid edge is logged and ignored. Returns whether it applied.
    fn apply_exclusive_edge(
        layer_surface: &zwlr_layer_surface_v1::ZwlrLayerSurfaceV1,
        anchor: Anchor,
        edge: Anchor,
    ) -> bool {
        if edge.bits().count_ones() == 1 && anchor.contains(edge) {
            layer_surface.set_exclusive_edge(super::layer_shell::wayland_anchor(edge));
            true
        } else {
            log::warn!(
                "ignoring exclusive edge {edge:?}: must be a single edge of the surface anchor {anchor:?}"
            );
            false
        }
    }

    fn set_exclusive_edge(&self, edge: Anchor) -> bool {
        if let WaylandSurfaceState::LayerShell(WaylandLayerSurfaceState {
            layer_surface,
            anchor,
            ..
        }) = self
        {
            Self::apply_exclusive_edge(layer_surface, *anchor, edge)
        } else {
            false
        }
    }

    fn destroy(&mut self) {
        match self {
            WaylandSurfaceState::Xdg(WaylandXdgSurfaceState {
                xdg_surface,
                toplevel,
                decoration: _decoration,
                dialog,
            }) => {
                // drop the dialog before toplevel so compositor can explicitly unapply it's effects
                if let Some(dialog) = dialog {
                    dialog.destroy();
                }

                // The role object (toplevel) must always be destroyed before the xdg_surface.
                // See https://wayland.app/protocols/xdg-shell#xdg_surface:request:destroy
                toplevel.destroy();
                xdg_surface.destroy();
            }
            WaylandSurfaceState::LayerShell(WaylandLayerSurfaceState { layer_surface, .. }) => {
                layer_surface.destroy();
            }
            WaylandSurfaceState::Popup(WaylandPopupSurfaceState {
                xdg_surface,
                xdg_popup,
                ..
            }) => {
                // Role object before its xdg_surface, as with the toplevel above.
                xdg_popup.destroy();
                xdg_surface.destroy();
            }
        }
    }
}

#[derive(Clone)]
pub struct WaylandWindowStatePtr {
    state: Rc<RefCell<WaylandWindowState>>,
    callbacks: Rc<RefCell<Callbacks>>,
    frame_loop: Rc<Cell<FrameLoop>>,
    frame_ping: Ping,
}

impl WaylandWindowState {
    pub(crate) fn new(
        handle: AnyWindowHandle,
        surface: wl_surface::WlSurface,
        surface_state: WaylandSurfaceState,
        appearance: WindowAppearance,
        viewport: Option<wp_viewport::WpViewport>,
        client: WaylandClientStatePtr,
        globals: Globals,
        gpu_context: gpui_wgpu::GpuContext,
        compositor_gpu: Option<CompositorGpuHint>,
        options: WindowParams,
        parent: Option<WaylandWindowStatePtr>,
    ) -> anyhow::Result<Self> {
        let renderer = {
            let raw_window = RawWindow {
                window: surface.id().as_ptr().cast::<c_void>(),
                display: surface
                    .backend()
                    .upgrade()
                    .unwrap()
                    .display_ptr()
                    .cast::<c_void>(),
            };
            let config = WgpuSurfaceConfig {
                size: Size {
                    width: DevicePixels(f32::from(options.bounds.size.width) as i32),
                    height: DevicePixels(f32::from(options.bounds.size.height) as i32),
                },
                transparent: true,
                // Prefer Mailbox to avoid blocking. Falls back to FIFO if Mailbox is unsupported.
                preferred_present_mode: Some(wgpu::PresentMode::Mailbox),
            };
            WgpuRenderer::new(gpu_context, &raw_window, config, compositor_gpu)?
        };

        if let WaylandSurfaceState::Xdg(ref xdg_state) = surface_state {
            if let Some(title) = options.titlebar.and_then(|titlebar| titlebar.title) {
                xdg_state.toplevel.set_title(title.to_string());
            }

            if let Some(app_id) = options.app_id.as_ref() {
                xdg_state.toplevel.set_app_id(app_id.clone());
            }

            // Set max window size based on the GPU's maximum texture dimension.
            // This prevents the window from being resized larger than what the GPU can render.
            let max_texture_size = renderer.max_texture_size() as i32;
            xdg_state
                .toplevel
                .set_max_size(max_texture_size, max_texture_size);
        }

        Ok(Self {
            surface_state,
            parent,
            children: FxHashMap::default(),
            surface,
            app_id: options.app_id,
            background_effect: None,
            background_effect_region: None,
            blur: None,
            viewport,
            globals,
            outputs: HashMap::default(),
            display: None,
            renderer,
            bounds: options.bounds,
            scale: 1.0,
            input_handler: None,
            decorations: WindowDecorations::Client,
            background_appearance: WindowBackgroundAppearance::Opaque,
            fullscreen: false,
            maximized: false,
            visibility: WindowVisibility::Visible,
            tiling: Tiling::default(),
            window_bounds: options.bounds,
            in_progress_configure: None,
            resize_throttle: false,
            client,
            appearance,
            handle,
            active: false,
            hovered: false,
            redraw_requested: false,
            presentation: PresentationState::Unpresented,
            pending_frame_callback: None,
            in_progress_window_controls: None,
            window_controls: WindowControls::default(),
            client_inset: None,
            accesskit_adapter: None,
        })
    }

    pub fn is_transparent(&self) -> bool {
        self.decorations == WindowDecorations::Client
            || self.background_appearance != WindowBackgroundAppearance::Opaque
    }

    fn update_subpixel_layout(&mut self) {
        use wayland_client::protocol::wl_output::Subpixel;
        let is_bgr = self
            .display
            .as_ref()
            .and_then(|(_, output)| output.subpixel)
            .is_some_and(|s| s == Subpixel::HorizontalBgr);
        self.renderer.set_subpixel_layout(is_bgr);
    }

    pub fn primary_output_scale(&mut self) -> i32 {
        let mut scale = 1;
        let mut current_output = self.display.take();
        for (id, output) in self.outputs.iter() {
            if let Some((_, output_data)) = &current_output {
                if output.scale > output_data.scale {
                    current_output = Some((id.clone(), output.clone()));
                }
            } else {
                current_output = Some((id.clone(), output.clone()));
            }
            scale = scale.max(output.scale);
        }
        self.display = current_output;
        scale
    }

    pub fn inset(&self) -> Pixels {
        match self.decorations {
            WindowDecorations::Server => px(0.0),
            WindowDecorations::Client => self.client_inset.unwrap_or(px(0.0)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PresentationState {
    Unpresented,
    Presented,
    RetryBeforeFirstPresent,
    RetryAfterPresent,
}

impl PresentationState {
    fn requires_presentation(self) -> bool {
        matches!(
            self,
            Self::RetryBeforeFirstPresent | Self::RetryAfterPresent
        )
    }

    fn failed(self) -> Self {
        match self {
            Self::Unpresented | Self::RetryBeforeFirstPresent => Self::RetryBeforeFirstPresent,
            Self::Presented | Self::RetryAfterPresent => Self::RetryAfterPresent,
        }
    }
}

#[cfg(test)]
mod presentation_state_tests {
    use super::PresentationState;

    #[test]
    fn failure_tracks_whether_the_surface_has_presented() {
        assert_eq!(
            PresentationState::Unpresented.failed(),
            PresentationState::RetryBeforeFirstPresent
        );
        assert_eq!(
            PresentationState::RetryBeforeFirstPresent.failed(),
            PresentationState::RetryBeforeFirstPresent
        );
        assert_eq!(
            PresentationState::Presented.failed(),
            PresentationState::RetryAfterPresent
        );
        assert_eq!(
            PresentationState::RetryAfterPresent.failed(),
            PresentationState::RetryAfterPresent
        );
    }

    #[test]
    fn only_retry_states_require_presentation() {
        assert!(!PresentationState::Unpresented.requires_presentation());
        assert!(!PresentationState::Presented.requires_presentation());
        assert!(PresentationState::RetryBeforeFirstPresent.requires_presentation());
        assert!(PresentationState::RetryAfterPresent.requires_presentation());
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FrameLoop {
    Unconfigured,
    Ticking,
    RescheduleRequested,
    PresentationFailed,
    AwaitingCallback,
    Scheduled,
    RetryScheduled,
    Parked,
}

pub(crate) struct WaylandWindow(pub WaylandWindowStatePtr);
pub enum ImeInput {
    InsertText(String),
    SetMarkedText(String),
    UnmarkText,
    DeleteText,
}

impl Drop for WaylandWindow {
    fn drop(&mut self) {
        self.0.frame_loop.set(FrameLoop::Parked);

        let mut state = self.0.state.borrow_mut();
        let surface_id = state.surface.id();

        if let Some(parent) = state.parent.as_ref() {
            parent.state.borrow_mut().children.remove(&surface_id);
        }

        let client = state.client.clone();

        state.renderer.destroy();

        // Destroy background effects first, as they depend on the wl_surface.
        if let Some(background_effect) = &state.background_effect {
            background_effect.destroy();
        }
        if let Some(blur) = &state.blur {
            blur.release();
        }

        // Decorations must be destroyed before the xdg state.
        // See https://wayland.app/protocols/xdg-decoration-unstable-v1#zxdg_toplevel_decoration_v1
        if let Some(decoration) = &state.surface_state.decoration() {
            decoration.destroy();
        }

        // Surface state might contain xdg_toplevel/xdg_surface which can be destroyed now that
        // decorations are gone. layer_surface has no dependencies.
        state.surface_state.destroy();

        // Viewport must be destroyed before the wl_surface.
        // See https://wayland.app/protocols/viewporter#wp_viewport
        if let Some(viewport) = &state.viewport {
            viewport.destroy();
        }

        // The wl_surface itself should always be destroyed last.
        state.surface.destroy();

        let state_ptr = self.0.clone();
        state
            .globals
            .executor
            .spawn(async move {
                state_ptr.close();
                client.drop_window(&surface_id)
            })
            .detach();
        drop(state);
    }
}

impl WaylandWindow {
    fn borrow(&self) -> Ref<'_, WaylandWindowState> {
        self.0.state.borrow()
    }

    fn borrow_mut(&self) -> RefMut<'_, WaylandWindowState> {
        self.0.state.borrow_mut()
    }

    pub fn new(
        handle: AnyWindowHandle,
        globals: Globals,
        gpu_context: gpui_wgpu::GpuContext,
        compositor_gpu: Option<CompositorGpuHint>,
        client: WaylandClientStatePtr,
        params: WindowParams,
        appearance: WindowAppearance,
        parent: Option<WaylandWindowStatePtr>,
        popup_grab: Option<(u32, wl_seat::WlSeat)>,
        target_output: Option<wl_output::WlOutput>,
    ) -> anyhow::Result<(Self, ObjectId)> {
        let surface = globals.compositor.create_surface(&globals.qh, ());
        let surface_state = WaylandSurfaceState::new(
            &surface,
            &globals,
            &params,
            parent.clone(),
            popup_grab,
            target_output,
        )?;

        if let Some(fractional_scale_manager) = globals.fractional_scale_manager.as_ref() {
            fractional_scale_manager.get_fractional_scale(&surface, &globals.qh, surface.id());
        }

        let viewport = globals
            .viewporter
            .as_ref()
            .map(|viewporter| viewporter.get_viewport(&surface, &globals.qh, ()));

        let frame_ping = globals.frame_ping.clone();
        let this = Self(WaylandWindowStatePtr {
            state: Rc::new(RefCell::new(WaylandWindowState::new(
                handle,
                surface.clone(),
                surface_state,
                appearance,
                viewport,
                client,
                globals,
                gpu_context,
                compositor_gpu,
                params,
                parent,
            )?)),
            callbacks: Rc::new(RefCell::new(Callbacks::default())),
            frame_loop: Rc::new(Cell::new(FrameLoop::Unconfigured)),
            frame_ping,
        });

        // Kick things off
        surface.commit();

        Ok((this, surface.id()))
    }
}

impl WaylandWindowStatePtr {
    pub(crate) fn update_background_effect(&self) {
        update_window(self.state.borrow_mut());
    }

    pub fn handle(&self) -> AnyWindowHandle {
        self.state.borrow().handle
    }

    pub fn surface(&self) -> wl_surface::WlSurface {
        self.state.borrow().surface.clone()
    }

    pub fn toplevel(&self) -> Option<xdg_toplevel::XdgToplevel> {
        self.state.borrow().surface_state.toplevel().cloned()
    }

    /// The `xdg_surface` backing this window, if it has one. Used to anchor child popups.
    pub fn xdg_surface(&self) -> Option<xdg_surface::XdgSurface> {
        self.state.borrow().surface_state.xdg_surface().cloned()
    }

    /// The layer-shell surface backing this window, if it is one. Used to anchor child popups.
    pub fn layer_surface(&self) -> Option<zwlr_layer_surface_v1::ZwlrLayerSurfaceV1> {
        self.state.borrow().surface_state.layer_surface().cloned()
    }

    /// This window's xdg window geometry in surface-local coordinates. Child popup anchor
    /// rectangles are relative to it, while gpui coordinates are surface-local.
    pub fn window_geometry(&self) -> Bounds<Pixels> {
        let state = self.state.borrow();
        inset_by_tiling(
            state.bounds.map_origin(|_| px(0.0)),
            state.inset(),
            state.tiling,
        )
    }

    pub fn ptr_eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.state, &other.state)
    }

    pub fn add_child(&self, child: ObjectId, blocking: bool) {
        let mut state = self.state.borrow_mut();
        state.children.insert(child, blocking);
    }

    pub fn is_blocked(&self) -> bool {
        let state = self.state.borrow();
        state.children.values().any(|&blocking| blocking)
    }

    pub fn frame(&self) {
        self.frame_loop.set(FrameLoop::Ticking);
        let mut state = self.state.borrow_mut();
        state.resize_throttle = false;
        // GPUI may throttle this tick without calling draw, so leave the request
        // latched until a draw actually reaches the renderer.
        let force_render = state.redraw_requested;
        let require_presentation = state.presentation.requires_presentation();
        drop(state);

        let mut callbacks = self.callbacks.borrow_mut();
        let Some(request_frame_callback) = callbacks.request_frame.as_mut() else {
            self.frame_loop.set(FrameLoop::Parked);
            return;
        };
        request_frame_callback(RequestFrameOptions {
            force_render,
            require_presentation,
        });
        self.update_ime_enabled();
        drop(callbacks);

        self.complete_frame();
    }

    fn complete_frame(&self) {
        let mut state = self.state.borrow_mut();

        let frame_loop = self.frame_loop.get();
        if frame_loop == FrameLoop::AwaitingCallback {
            return;
        }

        if state.presentation.requires_presentation() {
            // Before the first present, or when throttling skipped draw, a
            // callback may never arrive. Otherwise let the compositor pace
            // retries so an occluded window does not keep polling.
            if frame_loop == FrameLoop::PresentationFailed
                && state.presentation == PresentationState::RetryAfterPresent
            {
                if state.pending_frame_callback.is_none() {
                    let callback = state.surface.frame(&state.globals.qh, state.surface.id());
                    state.pending_frame_callback = Some(callback);
                }
                state.surface.commit();
                self.frame_loop.set(FrameLoop::AwaitingCallback);
                return;
            }

            self.frame_loop.set(FrameLoop::RetryScheduled);
            let surface_id = state.surface.id();
            let client = state.client.clone();
            drop(state);
            client.schedule_frame_retry(&surface_id);
            return;
        }

        if frame_loop == FrameLoop::RescheduleRequested || state.redraw_requested {
            self.frame_loop.set(FrameLoop::RetryScheduled);
            let surface_id = state.surface.id();
            let client = state.client.clone();
            drop(state);
            client.schedule_frame_retry(&surface_id);
            return;
        }

        self.frame_loop.set(FrameLoop::Parked);
    }

    pub fn frame_callback_fired(&self) {
        // Another wl_surface commit may have carried this callback while a retry
        // timer owned the render-loop wakeup.
        self.state.borrow_mut().pending_frame_callback = None;
        if self.frame_loop.get() == FrameLoop::AwaitingCallback {
            self.frame();
        }
    }

    pub fn scheduled_frame_fired(&self) {
        if self.frame_loop.get() == FrameLoop::Scheduled {
            self.frame();
        }
    }

    pub fn retry_timer_fired(&self) {
        if self.frame_loop.get() == FrameLoop::RetryScheduled {
            self.frame();
        }
    }

    pub fn is_configured(&self) -> bool {
        self.frame_loop.get() != FrameLoop::Unconfigured
    }

    pub fn schedule_frame(&self) {
        match self.frame_loop.get() {
            FrameLoop::Parked => {
                self.frame_loop.set(FrameLoop::Scheduled);
                self.frame_ping.ping();
            }
            FrameLoop::Ticking => {
                self.frame_loop.set(FrameLoop::RescheduleRequested);
            }
            // A wake is already armed: a ping or retry timer is in flight, or a
            // presented buffer guarantees a compositor frame callback.
            _ => {}
        }
    }

    pub(crate) fn request_redraw(&self) {
        self.state.borrow_mut().redraw_requested = true;
        self.schedule_frame();
    }

    fn update_ime_enabled(&self) {
        let mut state = self.state.borrow_mut();
        if !state.active {
            return;
        }
        let client = state.client.clone();
        let ime_enabled = if let Some(mut input_handler) = state.input_handler.take() {
            drop(state);
            let ime_enabled = ime_enabled_for(&mut input_handler);
            self.state.borrow_mut().input_handler = Some(input_handler);
            ime_enabled
        } else {
            drop(state);
            false
        };
        if Some(ime_enabled) == client.ime_enabled() {
            return;
        }

        if ime_enabled {
            client.enable_ime();
        } else {
            client.disable_ime();
        }
    }

    pub fn handle_xdg_surface_event(&self, event: xdg_surface::Event) {
        if let xdg_surface::Event::Configure { serial } = event {
            {
                let mut state = self.state.borrow_mut();
                if let Some(window_controls) = state.in_progress_window_controls.take() {
                    state.window_controls = window_controls;

                    drop(state);
                    let mut callbacks = self.callbacks.borrow_mut();
                    if let Some(appearance_changed) = callbacks.appearance_changed.as_mut() {
                        appearance_changed();
                    }
                }
            }
            {
                let mut state = self.state.borrow_mut();

                if let Some(mut configure) = state.in_progress_configure.take() {
                    let got_unmaximized = state.maximized && !configure.maximized;
                    state.fullscreen = configure.fullscreen;
                    state.maximized = configure.maximized;
                    state.tiling = configure.tiling;
                    let visibility_changed = state.visibility != configure.visibility;
                    state.visibility = configure.visibility;
                    // Limit interactive resizes to once per vblank
                    let throttled = configure.resizing && state.resize_throttle;
                    if throttled {
                        state.surface_state.ack_configure(serial);
                    } else {
                        if configure.resizing {
                            state.resize_throttle = true;
                        }
                        if !configure.fullscreen && !configure.maximized {
                            configure.size = if got_unmaximized {
                                Some(state.window_bounds.size)
                            } else {
                                compute_outer_size(state.inset(), configure.size, state.tiling)
                            };
                            if let Some(size) = configure.size {
                                state.window_bounds = Bounds {
                                    origin: Point::default(),
                                    size,
                                };
                            }
                        }
                    }
                    drop(state);
                    if visibility_changed {
                        self.report_visibility(configure.visibility);
                    }
                    if throttled {
                        return;
                    }
                    if let Some(size) = configure.size {
                        self.resize(size);
                    }
                }
            }
            let state = self.state.borrow_mut();
            state.surface_state.ack_configure(serial);

            let window_geometry = inset_by_tiling(
                state.bounds.map_origin(|_| px(0.0)),
                state.inset(),
                state.tiling,
            )
            .map(|v| f32::from(v) as i32)
            .map_size(|v| if v <= 0 { 1 } else { v });

            state.surface_state.set_geometry(
                window_geometry.origin.x,
                window_geometry.origin.y,
                window_geometry.size.width,
                window_geometry.size.height,
            );

            let initial_configure = self.frame_loop.get() == FrameLoop::Unconfigured;
            drop(state);
            if initial_configure {
                self.frame();
            } else {
                self.request_redraw();
            }
        }
    }

    pub fn handle_toplevel_decoration_event(&self, event: zxdg_toplevel_decoration_v1::Event) {
        if let zxdg_toplevel_decoration_v1::Event::Configure { mode } = event {
            match mode {
                WEnum::Value(zxdg_toplevel_decoration_v1::Mode::ServerSide) => {
                    self.state.borrow_mut().decorations = WindowDecorations::Server;
                    let callback = self.callbacks.borrow_mut().appearance_changed.take();
                    if let Some(mut fun) = callback {
                        fun();
                        self.callbacks.borrow_mut().appearance_changed = Some(fun);
                    }
                }
                WEnum::Value(zxdg_toplevel_decoration_v1::Mode::ClientSide) => {
                    self.state.borrow_mut().decorations = WindowDecorations::Client;
                    // Update background to be transparent
                    let callback = self.callbacks.borrow_mut().appearance_changed.take();
                    if let Some(mut fun) = callback {
                        fun();
                        self.callbacks.borrow_mut().appearance_changed = Some(fun);
                    }
                }
                WEnum::Value(_) => {
                    log::warn!("Unknown decoration mode");
                    return;
                }
                WEnum::Unknown(v) => {
                    log::warn!("Unknown decoration mode: {}", v);
                    return;
                }
            }
            update_window(self.state.borrow_mut());
            self.request_redraw();
        }
    }

    pub fn handle_fractional_scale_event(&self, event: wp_fractional_scale_v1::Event) {
        if let wp_fractional_scale_v1::Event::PreferredScale { scale } = event {
            self.rescale(scale as f32 / 120.0);
            self.request_redraw();
        }
    }

    pub fn handle_toplevel_event(&self, event: xdg_toplevel::Event) -> bool {
        match event {
            xdg_toplevel::Event::Configure {
                width,
                height,
                states,
            } => {
                let size = if width == 0 || height == 0 {
                    None
                } else {
                    Some(size(px(width as f32), px(height as f32)))
                };

                let states = extract_states::<xdg_toplevel::State>(&states);

                let mut tiling = Tiling::default();
                let mut fullscreen = false;
                let mut maximized = false;
                let mut resizing = false;
                let mut visibility = WindowVisibility::Visible;

                for state in states {
                    match state {
                        xdg_toplevel::State::Maximized => {
                            maximized = true;
                        }
                        xdg_toplevel::State::Fullscreen => {
                            fullscreen = true;
                        }
                        xdg_toplevel::State::Resizing => resizing = true,
                        xdg_toplevel::State::Suspended => visibility = WindowVisibility::Hidden,
                        xdg_toplevel::State::TiledTop => {
                            tiling.top = true;
                        }
                        xdg_toplevel::State::TiledLeft => {
                            tiling.left = true;
                        }
                        xdg_toplevel::State::TiledRight => {
                            tiling.right = true;
                        }
                        xdg_toplevel::State::TiledBottom => {
                            tiling.bottom = true;
                        }
                        _ => {
                            // noop
                        }
                    }
                }

                if fullscreen || maximized {
                    tiling = Tiling::tiled();
                }

                let mut state = self.state.borrow_mut();
                state.in_progress_configure = Some(InProgressConfigure {
                    size,
                    fullscreen,
                    maximized,
                    resizing,
                    visibility,
                    tiling,
                });

                false
            }
            xdg_toplevel::Event::Close => {
                let mut cb = self.callbacks.borrow_mut();
                if let Some(mut should_close) = cb.should_close.take() {
                    let result = (should_close)();
                    cb.should_close = Some(should_close);
                    if result {
                        drop(cb);
                        self.close();
                    }
                    result
                } else {
                    true
                }
            }
            xdg_toplevel::Event::WmCapabilities { capabilities } => {
                let mut window_controls = WindowControls {
                    maximize: false,
                    minimize: false,
                    fullscreen: false,
                    window_menu: false,
                };

                let states = extract_states::<xdg_toplevel::WmCapabilities>(&capabilities);

                for state in states {
                    match state {
                        xdg_toplevel::WmCapabilities::Maximize => {
                            window_controls.maximize = true;
                        }
                        xdg_toplevel::WmCapabilities::Minimize => {
                            window_controls.minimize = true;
                        }
                        xdg_toplevel::WmCapabilities::Fullscreen => {
                            window_controls.fullscreen = true;
                        }
                        xdg_toplevel::WmCapabilities::WindowMenu => {
                            window_controls.window_menu = true;
                        }
                        _ => {}
                    }
                }

                let mut state = self.state.borrow_mut();
                state.in_progress_window_controls = Some(window_controls);
                false
            }
            _ => false,
        }
    }

    pub fn handle_layersurface_event(&self, event: zwlr_layer_surface_v1::Event) -> bool {
        match event {
            zwlr_layer_surface_v1::Event::Configure {
                width,
                height,
                serial,
            } => {
                let size = if width == 0 || height == 0 {
                    None
                } else {
                    Some(size(px(width as f32), px(height as f32)))
                };

                let mut state = self.state.borrow_mut();
                state.in_progress_configure = Some(InProgressConfigure {
                    size,
                    fullscreen: false,
                    maximized: false,
                    resizing: false,
                    visibility: WindowVisibility::Visible,
                    tiling: Tiling::default(),
                });
                drop(state);

                // just do the same thing we'd do as an xdg_surface
                self.handle_xdg_surface_event(xdg_surface::Event::Configure { serial });

                false
            }
            zwlr_layer_surface_v1::Event::Closed => {
                // unlike xdg, we don't have a choice here: the surface is closing.
                true
            }
            _ => false,
        }
    }

    // Returns `true` if the popup should be closed.
    pub fn handle_popup_event(&self, event: xdg_popup::Event) -> bool {
        match event {
            // Only the size is needed, the position is the compositor's. The following
            // xdg_surface.configure applies the change.
            xdg_popup::Event::Configure { width, height, .. } => {
                let size = if width <= 0 || height <= 0 {
                    None
                } else {
                    Some(size(px(width as f32), px(height as f32)))
                };

                self.state.borrow_mut().in_progress_configure = Some(InProgressConfigure {
                    size,
                    fullscreen: false,
                    maximized: false,
                    resizing: false,
                    visibility: WindowVisibility::Visible,
                    tiling: Tiling::default(),
                });

                false
            }
            xdg_popup::Event::PopupDone => true,
            // Precedes the reposition's Configure, which does the work. The token is not needed.
            xdg_popup::Event::Repositioned { .. } => false,
            _ => false,
        }
    }

    #[allow(clippy::mutable_key_type)]
    pub fn handle_surface_event(
        &self,
        event: wl_surface::Event,
        outputs: HashMap<ObjectId, Output>,
    ) {
        let mut state = self.state.borrow_mut();

        match event {
            wl_surface::Event::Enter { output } => {
                let id = output.id();

                let Some(output) = outputs.get(&id) else {
                    return;
                };

                state.outputs.insert(id, output.clone());

                let scale = state.primary_output_scale();
                state.update_subpixel_layout();

                if output_scale_sets_buffer_scale(
                    state.surface.version(),
                    state.globals.fractional_scale_manager.is_some(),
                ) {
                    state.surface.set_buffer_scale(scale);
                    drop(state);
                    self.rescale(scale as f32);
                } else {
                    drop(state);
                }
                self.request_redraw();
            }
            wl_surface::Event::Leave { output } => {
                state.outputs.remove(&output.id());

                let scale = state.primary_output_scale();
                state.update_subpixel_layout();

                if output_scale_sets_buffer_scale(
                    state.surface.version(),
                    state.globals.fractional_scale_manager.is_some(),
                ) {
                    state.surface.set_buffer_scale(scale);
                    drop(state);
                    self.rescale(scale as f32);
                } else {
                    drop(state);
                }
                self.request_redraw();
            }
            wl_surface::Event::PreferredBufferScale { factor } => {
                // We use `WpFractionalScale` instead to set the scale if it's available
                if state.globals.fractional_scale_manager.is_none() {
                    state.surface.set_buffer_scale(factor);
                    drop(state);
                    self.rescale(factor as f32);
                    self.request_redraw();
                }
            }
            _ => {}
        }
    }

    pub fn handle_ime(&self, ime: ImeInput) {
        if self.is_blocked() {
            return;
        }
        let mut state = self.state.borrow_mut();
        if let Some(mut input_handler) = state.input_handler.take() {
            drop(state);
            match ime {
                ImeInput::InsertText(text) => {
                    input_handler.replace_text_in_range(None, &text);
                }
                ImeInput::SetMarkedText(text) => {
                    input_handler.replace_and_mark_text_in_range(None, &text, None);
                }
                ImeInput::UnmarkText => {
                    input_handler.unmark_text();
                }
                ImeInput::DeleteText => {
                    if let Some(marked) = input_handler.marked_text_range() {
                        input_handler.replace_text_in_range(Some(marked), "");
                    }
                }
            }
            self.state.borrow_mut().input_handler = Some(input_handler);
        }
    }

    pub fn get_ime_area(&self) -> Option<Bounds<Pixels>> {
        let mut state = self.state.borrow_mut();
        let mut bounds: Option<Bounds<Pixels>> = None;
        if let Some(mut input_handler) = state.input_handler.take() {
            drop(state);
            bounds = input_handler.ime_candidate_bounds();
            self.state.borrow_mut().input_handler = Some(input_handler);
        }
        bounds
    }

    pub fn set_size_and_scale(&self, size: Option<Size<Pixels>>, scale: Option<f32>) {
        let (size, scale) = {
            let mut state = self.state.borrow_mut();
            if size.is_none_or(|size| size == state.bounds.size)
                && scale.is_none_or(|scale| scale == state.scale)
            {
                return;
            }
            if let Some(size) = size {
                state.bounds.size = size;
            }
            if let Some(scale) = scale {
                state.scale = scale;
            }
            let device_bounds = state.bounds.to_device_pixels(state.scale);
            state.renderer.update_drawable_size(device_bounds.size);
            (state.bounds.size, state.scale)
        };

        let callback = self.callbacks.borrow_mut().resize.take();
        if let Some(mut fun) = callback {
            fun(size, scale);
            self.callbacks.borrow_mut().resize = Some(fun);
        }

        {
            let state = self.state.borrow();
            if let Some(viewport) = &state.viewport {
                viewport
                    .set_destination(f32::from(size.width) as i32, f32::from(size.height) as i32);
            }
        }
    }

    pub fn resize(&self, size: Size<Pixels>) {
        self.set_size_and_scale(Some(size), None);
    }

    pub fn rescale(&self, scale: f32) {
        self.set_size_and_scale(None, Some(scale));
    }

    pub fn close(&self) {
        let state = self.state.borrow();
        let client = state.client.get_client();
        let children = state.children.keys().cloned().collect::<Vec<_>>();
        drop(state);

        for child in children {
            let mut client_state = client.borrow_mut();
            let window = get_window(&mut client_state, &child);
            drop(client_state);

            if let Some(child) = window {
                child.close();
            }
        }
        let mut callbacks = self.callbacks.borrow_mut();
        if let Some(fun) = callbacks.close.take() {
            fun()
        }
    }

    pub fn handle_input(&self, input: PlatformInput) {
        if self.is_blocked() {
            return;
        }
        let callback = self.callbacks.borrow_mut().input.take();
        if let Some(mut fun) = callback {
            let result = fun(input.clone());
            self.callbacks.borrow_mut().input = Some(fun);
            if !result.propagate {
                return;
            }
        }
        if let PlatformInput::KeyDown(event) = input
            && event.keystroke.modifiers.is_subset_of(&Modifiers::shift())
            && let Some(key_char) = &event.keystroke.key_char
        {
            let mut state = self.state.borrow_mut();
            if let Some(mut input_handler) = state.input_handler.take() {
                drop(state);
                input_handler.replace_text_in_range(None, key_char);
                self.state.borrow_mut().input_handler = Some(input_handler);
            }
        }
    }

    pub fn set_focused(&self, focus: bool) {
        self.state.borrow_mut().active = focus;
        let callback = self.callbacks.borrow_mut().active_status_change.take();
        if let Some(mut fun) = callback {
            fun(focus);
            self.callbacks.borrow_mut().active_status_change = Some(fun);
        }
        if let Some(adapter) = self.state.borrow_mut().accesskit_adapter.as_mut() {
            adapter.update_window_focus_state(focus);
        }
    }

    pub fn set_hovered(&self, focus: bool) {
        let callback = self.callbacks.borrow_mut().hover_status_change.take();
        if let Some(mut fun) = callback {
            fun(focus);
            self.callbacks.borrow_mut().hover_status_change = Some(fun);
        }
    }

    fn report_visibility(&self, visibility: WindowVisibility) {
        let callback = self.callbacks.borrow_mut().visibility_change.take();
        if let Some(mut callback) = callback {
            callback(visibility);
            self.callbacks.borrow_mut().visibility_change = Some(callback);
        }
    }

    pub fn set_appearance(&mut self, appearance: WindowAppearance) {
        self.state.borrow_mut().appearance = appearance;

        let callback = self.callbacks.borrow_mut().appearance_changed.take();
        if let Some(mut fun) = callback {
            fun();
            self.callbacks.borrow_mut().appearance_changed = Some(fun);
        }
    }

    pub fn set_button_layout(&self) {
        let callback = self.callbacks.borrow_mut().button_layout_changed.take();
        if let Some(mut fun) = callback {
            fun();
            self.callbacks.borrow_mut().button_layout_changed = Some(fun);
        }
    }

    pub fn primary_output_scale(&self) -> i32 {
        self.state.borrow_mut().primary_output_scale()
    }
}

fn extract_states<'a, S: TryFrom<u32> + 'a>(states: &'a [u8]) -> impl Iterator<Item = S> + 'a
where
    <S as TryFrom<u32>>::Error: 'a,
{
    states
        .chunks_exact(4)
        .flat_map(TryInto::<[u8; 4]>::try_into)
        .map(u32::from_ne_bytes)
        .flat_map(S::try_from)
}

impl rwh::HasWindowHandle for WaylandWindow {
    fn window_handle(&self) -> Result<rwh::WindowHandle<'_>, rwh::HandleError> {
        let surface = self.0.surface().id().as_ptr() as *mut libc::c_void;
        let c_ptr = NonNull::new(surface).ok_or(rwh::HandleError::Unavailable)?;
        let handle = rwh::WaylandWindowHandle::new(c_ptr);
        let raw_handle = rwh::RawWindowHandle::Wayland(handle);
        Ok(unsafe { rwh::WindowHandle::borrow_raw(raw_handle) })
    }
}

impl rwh::HasDisplayHandle for WaylandWindow {
    fn display_handle(&self) -> Result<rwh::DisplayHandle<'_>, rwh::HandleError> {
        let display = self
            .0
            .surface()
            .backend()
            .upgrade()
            .ok_or(rwh::HandleError::Unavailable)?
            .display_ptr() as *mut libc::c_void;

        let c_ptr = NonNull::new(display).ok_or(rwh::HandleError::Unavailable)?;
        let handle = rwh::WaylandDisplayHandle::new(c_ptr);
        let raw_handle = rwh::RawDisplayHandle::Wayland(handle);
        Ok(unsafe { rwh::DisplayHandle::borrow_raw(raw_handle) })
    }
}

impl PlatformWindow for WaylandWindow {
    fn bounds(&self) -> Bounds<Pixels> {
        self.borrow().bounds
    }

    fn is_maximized(&self) -> bool {
        self.borrow().maximized
    }

    fn window_bounds(&self) -> WindowBounds {
        let state = self.borrow();
        if state.fullscreen {
            WindowBounds::Fullscreen(state.window_bounds)
        } else if state.maximized {
            WindowBounds::Maximized(state.window_bounds)
        } else {
            drop(state);
            WindowBounds::Windowed(self.bounds())
        }
    }

    fn inner_window_bounds(&self) -> WindowBounds {
        let state = self.borrow();
        if state.fullscreen {
            WindowBounds::Fullscreen(state.window_bounds)
        } else if state.maximized {
            WindowBounds::Maximized(state.window_bounds)
        } else {
            let inset = state.inset();
            drop(state);
            WindowBounds::Windowed(self.bounds().inset(inset))
        }
    }

    fn content_size(&self) -> Size<Pixels> {
        self.borrow().bounds.size
    }

    fn resize(&mut self, size: Size<Pixels>) {
        let state = self.borrow();
        let state_ptr = self.0.clone();

        // A popup's placement is the compositor's, so a resize re-runs the positioner and the
        // configure reply drives the buffer resize. Before the first configure the popup is
        // unmapped and cannot reposition, but the initial positioner already carries the size.
        if matches!(state.surface_state, WaylandSurfaceState::Popup(_)) {
            if self.0.is_configured() {
                let parent_geometry = state
                    .parent
                    .as_ref()
                    .map(|parent| parent.window_geometry())
                    .unwrap_or_default();
                state
                    .surface_state
                    .reposition_popup(&state.globals, size, parent_geometry);
            }
            return;
        }

        // Keep window geometry consistent with configure handling. On Wayland, window geometry is
        // surface-local: resizing should not attempt to translate the window; the compositor
        // controls placement. We also account for client-side decoration insets and tiling.
        let window_geometry = inset_by_tiling(
            Bounds {
                origin: Point::default(),
                size,
            },
            state.inset(),
            state.tiling,
        )
        .map(|v| f32::from(v) as i32)
        .map_size(|v| if v <= 0 { 1 } else { v });

        state.surface_state.set_geometry(
            window_geometry.origin.x,
            window_geometry.origin.y,
            window_geometry.size.width,
            window_geometry.size.height,
        );

        state
            .globals
            .executor
            .spawn(async move { state_ptr.resize(size) })
            .detach();
    }

    fn scale_factor(&self) -> f32 {
        self.borrow().scale
    }

    fn appearance(&self) -> WindowAppearance {
        self.borrow().appearance
    }

    fn display(&self) -> Option<Rc<dyn PlatformDisplay>> {
        let state = self.borrow();
        state.display.as_ref().map(|(id, display)| {
            Rc::new(WaylandDisplay {
                id: id.clone(),
                name: display.name.clone(),
                bounds: display.bounds.to_pixels(state.scale),
                refresh_millihertz: display.refresh_millihertz,
            }) as Rc<dyn PlatformDisplay>
        })
    }

    fn mouse_position(&self) -> Point<Pixels> {
        self.borrow()
            .client
            .get_client()
            .borrow()
            .mouse_location
            .unwrap_or_default()
    }

    fn modifiers(&self) -> Modifiers {
        self.borrow().client.get_client().borrow().modifiers
    }

    fn capslock(&self) -> Capslock {
        self.borrow().client.get_client().borrow().capslock
    }

    fn set_input_handler(&mut self, input_handler: PlatformInputHandler) {
        self.borrow_mut().input_handler = Some(input_handler);
    }

    fn take_input_handler(&mut self) -> Option<PlatformInputHandler> {
        self.borrow_mut().input_handler.take()
    }

    fn prompt(
        &self,
        _level: PromptLevel,
        _msg: &str,
        _detail: Option<&str>,
        _answers: &[PromptButton],
    ) -> Option<Receiver<usize>> {
        None
    }

    fn activate(&self) {
        // Try to request an activation token. Even though the activation is likely going to be rejected,
        // KWin and Mutter can use the app_id to visually indicate we're requesting attention.
        let state = self.borrow();
        if let (Some(activation), Some(app_id)) = (&state.globals.activation, state.app_id.clone())
        {
            state.client.set_pending_activation(state.surface.id());
            let token = activation.get_activation_token(&state.globals.qh, ());
            // The serial isn't exactly important here, since the activation is probably going to be rejected anyway.
            let serial = state.client.get_serial(SerialKind::MousePress);
            token.set_app_id(app_id);
            token.set_serial(serial.as_raw(), &state.globals.seat);
            token.set_surface(&state.surface);
            token.commit();
        }
    }

    fn request_attention(&self) {}

    fn is_active(&self) -> bool {
        self.borrow().active
    }

    fn visibility(&self) -> WindowVisibility {
        self.borrow().visibility
    }

    fn is_hovered(&self) -> bool {
        self.borrow().hovered
    }

    fn set_title(&mut self, title: &str) {
        if let Some(toplevel) = self.borrow().surface_state.toplevel() {
            toplevel.set_title(title.to_string());
        }
    }

    fn set_app_id(&mut self, app_id: &str) {
        let mut state = self.borrow_mut();
        if let Some(toplevel) = state.surface_state.toplevel() {
            toplevel.set_app_id(app_id.to_owned());
        }
        state.app_id = Some(app_id.to_owned());
    }

    fn set_background_appearance(&self, background_appearance: WindowBackgroundAppearance) {
        let mut state = self.borrow_mut();
        if state.background_appearance == background_appearance {
            return;
        }
        state.background_appearance = background_appearance;
        update_window(state);
        self.0.request_redraw();
    }

    fn background_appearance(&self) -> WindowBackgroundAppearance {
        self.borrow().background_appearance
    }

    fn supports_backdrop_sampling(&self) -> bool {
        self.borrow().renderer.supports_backdrop_sampling()
    }

    fn is_subpixel_rendering_supported(&self) -> bool {
        let client = self.borrow().client.get_client();
        let state = client.borrow();
        state
            .gpu_context
            .borrow()
            .as_ref()
            .is_some_and(|ctx| ctx.supports_dual_source_blending())
    }

    fn minimize(&self) {
        if let Some(toplevel) = self.borrow().surface_state.toplevel() {
            toplevel.set_minimized();
        }
    }

    fn set_visible(&self, visible: bool) {
        // An xdg toplevel cannot unmap and come back without repeating its
        // initial configure, so hiding degrades to minimizing here. Showing is
        // then nothing: the default `is_visible` keeps reporting true, and
        // `activate` is what asks the compositor to bring the window back.
        if !visible {
            self.minimize();
        }
    }

    fn zoom(&self) {
        let state = self.borrow();
        if let Some(toplevel) = state.surface_state.toplevel() {
            if !state.maximized {
                toplevel.set_maximized();
            } else {
                toplevel.unset_maximized();
            }
        }
    }

    fn toggle_fullscreen(&self) {
        let state = self.borrow();
        if let Some(toplevel) = state.surface_state.toplevel() {
            if !state.fullscreen {
                toplevel.set_fullscreen(None);
            } else {
                toplevel.unset_fullscreen();
            }
        }
    }

    fn is_fullscreen(&self) -> bool {
        self.borrow().fullscreen
    }

    fn on_request_frame(&self, callback: Box<dyn FnMut(RequestFrameOptions)>) {
        self.0.callbacks.borrow_mut().request_frame = Some(callback);
    }

    fn on_input(&self, callback: Box<dyn FnMut(PlatformInput) -> gpui::DispatchEventResult>) {
        self.0.callbacks.borrow_mut().input = Some(callback);
    }

    fn on_active_status_change(&self, callback: Box<dyn FnMut(bool)>) {
        self.0.callbacks.borrow_mut().active_status_change = Some(callback);
    }

    fn on_visibility_change(&self, callback: Box<dyn FnMut(WindowVisibility)>) {
        self.0.callbacks.borrow_mut().visibility_change = Some(callback);
    }

    fn on_hover_status_change(&self, callback: Box<dyn FnMut(bool)>) {
        self.0.callbacks.borrow_mut().hover_status_change = Some(callback);
    }

    fn on_resize(&self, callback: Box<dyn FnMut(Size<Pixels>, f32)>) {
        self.0.callbacks.borrow_mut().resize = Some(callback);
    }

    fn on_moved(&self, callback: Box<dyn FnMut()>) {
        self.0.callbacks.borrow_mut().moved = Some(callback);
    }

    fn on_should_close(&self, callback: Box<dyn FnMut() -> bool>) {
        self.0.callbacks.borrow_mut().should_close = Some(callback);
    }

    fn on_close(&self, callback: Box<dyn FnOnce()>) {
        self.0.callbacks.borrow_mut().close = Some(callback);
    }

    fn on_hit_test_window_control(&self, _callback: Box<dyn FnMut() -> Option<WindowControlArea>>) {
    }

    fn on_appearance_changed(&self, callback: Box<dyn FnMut()>) {
        self.0.callbacks.borrow_mut().appearance_changed = Some(callback);
    }

    fn on_button_layout_changed(&self, callback: Box<dyn FnMut()>) {
        self.0.callbacks.borrow_mut().button_layout_changed = Some(callback);
    }

    fn draw(&self, scene: &Scene) {
        let mut state = self.borrow_mut();

        if state.renderer.device_lost() {
            let raw_window = RawWindow {
                window: state.surface.id().as_ptr().cast::<std::ffi::c_void>(),
                display: state
                    .surface
                    .backend()
                    .upgrade()
                    .unwrap()
                    .display_ptr()
                    .cast::<std::ffi::c_void>(),
            };
            match state.renderer.recover(&raw_window) {
                Ok(()) => {}
                Err(err) => {
                    log::warn!("GPU recovery failed, will retry on next frame: {err}");
                }
            }

            state.redraw_requested = true;
            return;
        }

        // ext-background-effect defines its region in wl_surface coordinates.
        // KWin 6.7 instead anchors it at the xdg window geometry (the client
        // content rect), shifting a CSD region down and right by its exposed
        // top/left inset. Submit the inverse translation on KWin so the effect
        // and the rendered mask meet at the same pixels.
        let compositor_offset = if kwin_uses_content_local_background_effect_coordinates() {
            let inset = state.inset();
            point(
                if state.tiling.left { px(0.0) } else { inset },
                if state.tiling.top { px(0.0) } else { inset },
            )
        } else {
            Point::default()
        };
        let background_effect_region = BackgroundEffectRegion::for_scene(
            scene,
            state.bounds.size,
            state.renderer.viewport_size(),
            compositor_offset,
        );
        if state.background_effect_region.as_ref() != Some(&background_effect_region) {
            if let Some(background_effect) = state.background_effect.as_ref() {
                set_background_effect_region(&state, background_effect, &background_effect_region);
            }
            state.background_effect_region = Some(background_effect_region);
        }

        // A compositor effect applies independently of buffer alpha. Extending
        // it into a translucent client-side shadow therefore reveals either a
        // raw rail or a blurred corner wedge. Keep the exact rounded effect and
        // make the unused CSD margin fully transparent while ext blur is active.
        let clip_window_shadows =
            state.background_effect.is_some() && scene.window_corner_mask.is_some();
        state.renderer.set_clip_window_shadows(clip_window_shadows);

        // Surface state changed during this GPUI tick is included in this presentation.
        state.redraw_requested = false;
        if state.pending_frame_callback.is_none() {
            let callback = state.surface.frame(&state.globals.qh, state.surface.id());
            state.pending_frame_callback = Some(callback);
        }
        if state.renderer.draw(scene) {
            state.presentation = PresentationState::Presented;
            self.0.frame_loop.set(FrameLoop::AwaitingCallback);
        } else {
            state.presentation = state.presentation.failed();
            self.0.frame_loop.set(FrameLoop::PresentationFailed);
        }

        if state.renderer.needs_redraw() {
            state.redraw_requested = true;
        }
    }

    fn schedule_frame(&self) {
        self.0.schedule_frame();
    }

    fn sprite_atlas(&self) -> Arc<dyn PlatformAtlas> {
        let state = self.borrow();
        state.renderer.sprite_atlas().clone()
    }

    fn show_window_menu(&self, position: Point<Pixels>) {
        let state = self.borrow();
        let serial = state.client.get_serial(SerialKind::MousePress);
        if let Some(toplevel) = state.surface_state.toplevel() {
            toplevel.show_window_menu(
                &state.globals.seat,
                serial.as_raw(),
                f32::from(position.x) as i32,
                f32::from(position.y) as i32,
            );
        }
    }

    fn start_window_move(&self) {
        let state = self.borrow();
        let serial = state.client.get_serial(SerialKind::MousePress);
        if let Some(toplevel) = state.surface_state.toplevel() {
            toplevel._move(&state.globals.seat, serial.as_raw());
        }
    }

    fn can_start_external_drag(&self) -> bool {
        true
    }

    fn start_external_drag(&self, payload: &ExternalDragPayload) -> bool {
        let state = self.borrow();
        state.client.start_external_drag(&state.surface, payload)
    }

    fn start_window_resize(&self, edge: gpui::ResizeEdge) {
        let state = self.borrow();
        if let Some(toplevel) = state.surface_state.toplevel() {
            toplevel.resize(
                &state.globals.seat,
                state.client.get_serial(SerialKind::MousePress).as_raw(),
                edge.to_xdg(),
            )
        }
    }

    fn set_exclusive_zone(&self, zone: Pixels) {
        let state = self.borrow();
        if state
            .surface_state
            .set_exclusive_zone(f32::from(zone) as i32)
        {
            // Commit to apply it immediately, otherwise it only takes effect
            // on the next frame.
            state.surface.commit();
        }
    }

    fn set_exclusive_edge(&self, edge: Anchor) {
        let state = self.borrow();
        if state.surface_state.set_exclusive_edge(edge) {
            // Commit to apply it immediately, otherwise it only takes effect
            // on the next frame.
            state.surface.commit();
        }
    }

    fn set_input_region(&self, region: Option<&[Bounds<Pixels>]>) {
        let state = self.borrow();
        match region {
            // No region means the whole surface receives input.
            None => state.surface.set_input_region(None),
            // A region restricts input to its rectangles. An empty region
            // receives no input at all.
            Some(rects) => {
                let wl_region = state
                    .globals
                    .compositor
                    .create_region(&state.globals.qh, ());
                for rect in rects {
                    let rect = rect.map(|pixels| f32::from(pixels) as i32);
                    wl_region.add(
                        rect.origin.x,
                        rect.origin.y,
                        rect.size.width,
                        rect.size.height,
                    );
                }
                state.surface.set_input_region(Some(&wl_region));
                wl_region.destroy();
            }
        }

        // Commit so the new input region applies immediately. Otherwise it
        // waits for the next frame, which could be the very click we want to
        // allow passing through.
        state.surface.commit();
    }

    fn window_decorations(&self) -> Decorations {
        let state = self.borrow();
        match state.decorations {
            WindowDecorations::Server => Decorations::Server,
            WindowDecorations::Client => Decorations::Client {
                tiling: state.tiling,
            },
        }
    }

    fn request_decorations(&self, decorations: WindowDecorations) {
        let mut state = self.borrow_mut();
        match state.surface_state.decoration().as_ref() {
            Some(decoration) => {
                decoration.set_mode(decorations.to_xdg());
                state.decorations = decorations;
                update_window(state);
            }
            None => {
                if matches!(decorations, WindowDecorations::Server) {
                    log::info!(
                        "Server-side decorations requested, but the Wayland server does not support them. Falling back to client-side decorations."
                    );
                }
                state.decorations = WindowDecorations::Client;
                update_window(state);
            }
        }
        self.0.request_redraw();
    }

    fn window_controls(&self) -> WindowControls {
        self.borrow().window_controls
    }

    fn set_client_inset(&self, inset: Pixels) {
        let mut state = self.borrow_mut();
        if Some(inset) != state.client_inset {
            state.client_inset = Some(inset);
            // xdg window geometry is double-buffered surface state. Updating
            // only the cached inset leaves the compositor using the old CSD
            // origin until an unrelated resize/configure arrives, which makes
            // zoomed rendering and compositor effects disagree in the meantime.
            let window_geometry = inset_by_tiling(
                state.bounds.map_origin(|_| px(0.0)),
                state.inset(),
                state.tiling,
            )
            .map(|value| f32::from(value) as i32)
            .map_size(|value| if value <= 0 { 1 } else { value });
            state.surface_state.set_geometry(
                window_geometry.origin.x,
                window_geometry.origin.y,
                window_geometry.size.width,
                window_geometry.size.height,
            );
            update_window(state);
            self.0.request_redraw();
        }
    }

    fn update_ime_position(&self, bounds: Bounds<Pixels>) {
        let state = self.borrow();
        if !state.active {
            return;
        }
        state.client.update_ime_position(bounds);
    }

    fn gpu_specs(&self) -> Option<GpuSpecs> {
        self.borrow().renderer.gpu_specs()
    }

    fn wgpu_device_context(&self) -> Option<WgpuDeviceContext> {
        self.borrow().renderer.device_context()
    }

    fn play_system_bell(&self) {
        let state = self.borrow();
        let surface = if state.surface_state.toplevel().is_some() {
            Some(&state.surface)
        } else {
            None
        };
        if let Some(bell) = state.globals.system_bell.as_ref() {
            bell.ring(surface);
        }
    }

    fn a11y_init(&self, callbacks: gpui::A11yCallbacks) {
        let activation_handler = TrivialActivationHandler {
            callback: callbacks.activation,
        };
        let action_handler = TrivialActionHandler(callbacks.action);
        let deactivation_handler = TrivialDeactivationHandler {
            callback: callbacks.deactivation,
        };

        let adapter =
            accesskit_unix::Adapter::new(activation_handler, action_handler, deactivation_handler);

        self.borrow_mut().accesskit_adapter = Some(adapter);
    }

    fn a11y_tree_update(&self, tree_update: accesskit::TreeUpdate) {
        let mut state = self.borrow_mut();
        if let Some(adapter) = state.accesskit_adapter.as_mut() {
            adapter.update_if_active(|| tree_update);
        }
    }

    fn a11y_update_window_bounds(&self) {
        // Wayland doesn't expose window position, so this is a no-op
    }
}

struct TrivialActivationHandler {
    callback: Box<dyn Fn() -> Option<accesskit::TreeUpdate> + Send + 'static>,
}

impl accesskit::ActivationHandler for TrivialActivationHandler {
    fn request_initial_tree(&mut self) -> Option<accesskit::TreeUpdate> {
        (self.callback)()
    }
}

struct TrivialActionHandler(Box<dyn Fn(accesskit::ActionRequest) + Send + 'static>);

impl accesskit::ActionHandler for TrivialActionHandler {
    fn do_action(&mut self, request: accesskit::ActionRequest) {
        (self.0)(request);
    }
}

struct TrivialDeactivationHandler {
    callback: Box<dyn Fn() + Send + 'static>,
}

impl accesskit::DeactivationHandler for TrivialDeactivationHandler {
    fn deactivate_accessibility(&mut self) {
        (self.callback)();
    }
}

#[derive(Clone, Debug, PartialEq)]
struct BackgroundEffectRegion {
    bounds: Bounds<Pixels>,
    corner_radii: Corners<Pixels>,
    corner_smoothing: f32,
}

fn kwin_uses_content_local_background_effect_coordinates() -> bool {
    static RUNNING_UNDER_KWIN: LazyLock<bool> = LazyLock::new(|| {
        std::env::var("KDE_FULL_SESSION").is_ok_and(|value| value == "true")
            || ["XDG_CURRENT_DESKTOP", "XDG_SESSION_DESKTOP"]
                .into_iter()
                .filter_map(|name| std::env::var(name).ok())
                .any(|desktops| {
                    desktops
                        .split(':')
                        .any(|desktop| desktop.eq_ignore_ascii_case("kde"))
                })
    });
    *RUNNING_UNDER_KWIN
}

impl BackgroundEffectRegion {
    fn for_scene(
        scene: &Scene,
        surface_size: Size<Pixels>,
        buffer_size: Size<DevicePixels>,
        compositor_offset: Point<Pixels>,
    ) -> Self {
        let Some(mask) = scene.window_corner_mask else {
            return Self::full_surface(surface_size);
        };

        // The scene mask uses device pixels, while ext-background-effect uses
        // integer surface-local coordinates. Fractional output scales round the
        // drawable dimensions, so dividing by the advertised scale can drift at
        // the far edge. Map each axis through the drawable that was configured.
        let x_scale = f32::from(surface_size.width) / i32::from(buffer_size.width).max(1) as f32;
        let y_scale = f32::from(surface_size.height) / i32::from(buffer_size.height).max(1) as f32;
        // Drawable rounding can make the two ratios differ slightly. The
        // protocol has only scalar corner radii, so choose the smaller ratio:
        // its isotropic curve stays inside the renderer's slightly elliptical
        // projection instead of leaking a compositor-effect pixel outside it.
        let radius_scale = x_scale.min(y_scale);
        // The effect changes pixels independently of the surface buffer's
        // alpha. Keep it on the rendered window mask: extending it through a
        // soft CSD shadow exposes the protocol's hard integer-region edge as a
        // bright halo or corner wedge.
        Self {
            bounds: Bounds {
                origin: point(
                    px(mask.bounds.origin.x.as_f32() * x_scale) - compositor_offset.x,
                    px(mask.bounds.origin.y.as_f32() * y_scale) - compositor_offset.y,
                ),
                size: size(
                    px(mask.bounds.size.width.as_f32() * x_scale),
                    px(mask.bounds.size.height.as_f32() * y_scale),
                ),
            },
            corner_radii: mask
                .corner_radii
                .map(|value| px(value.as_f32() * radius_scale)),
            corner_smoothing: mask.corner_smoothing,
        }
    }

    fn full_surface(surface_size: Size<Pixels>) -> Self {
        Self {
            bounds: Bounds {
                origin: Point::default(),
                size: surface_size,
            },
            corner_radii: Corners::default(),
            corner_smoothing: 2.0,
        }
    }
}

fn background_effect_rectangles(region: &BackgroundEffectRegion) -> Vec<Bounds<i32>> {
    let left_edge = f32::from(region.bounds.origin.x);
    let top_edge = f32::from(region.bounds.origin.y);
    let right_edge = left_edge + f32::from(region.bounds.size.width);
    let bottom_edge = top_edge + f32::from(region.bounds.size.height);
    let left = left_edge.ceil() as i32;
    let top = top_edge.ceil() as i32;
    let right = right_edge.floor() as i32;
    let bottom = bottom_edge.floor() as i32;
    if left >= right || top >= bottom {
        return Vec::new();
    }

    let width = right_edge - left_edge;
    let height = bottom_edge - top_edge;
    let max_radius = width.min(height) / 2.0;
    let radius = |radius: Pixels| f32::from(radius).clamp(0.0, max_radius);
    let top_left = radius(region.corner_radii.top_left);
    let top_right = radius(region.corner_radii.top_right);
    let bottom_right = radius(region.corner_radii.bottom_right);
    let bottom_left = radius(region.corner_radii.bottom_left);
    let top_rows = top_left.max(top_right).ceil() as i32;
    let bottom_rows = bottom_left.max(bottom_right).ceil() as i32;
    let top_end = (top + top_rows).min(bottom);
    let bottom_start = (bottom - bottom_rows).max(top_end);
    let mut rectangles = Vec::with_capacity((top_rows + bottom_rows + 1) as usize);

    for y in top..top_end {
        let edge_distance = y as f32 + 0.5 - top_edge;
        push_background_effect_row(
            &mut rectangles,
            left_edge,
            right_edge,
            y,
            corner_inset(top_left, edge_distance, region.corner_smoothing, max_radius),
            corner_inset(
                top_right,
                edge_distance,
                region.corner_smoothing,
                max_radius,
            ),
        );
    }

    if top_end < bottom_start {
        push_background_effect_rectangle(
            &mut rectangles,
            left,
            top_end,
            right - left,
            bottom_start - top_end,
        );
    }

    for y in bottom_start..bottom {
        let edge_distance = bottom_edge - (y as f32 + 0.5);
        push_background_effect_row(
            &mut rectangles,
            left_edge,
            right_edge,
            y,
            corner_inset(
                bottom_left,
                edge_distance,
                region.corner_smoothing,
                max_radius,
            ),
            corner_inset(
                bottom_right,
                edge_distance,
                region.corner_smoothing,
                max_radius,
            ),
        );
    }

    rectangles
}

fn corner_inset(radius: f32, edge_distance: f32, smoothing: f32, max_radius: f32) -> f32 {
    if radius <= 0.0 || edge_distance >= radius {
        return 0.0;
    }

    let smoothing = if radius >= max_radius - 0.01 {
        2.0
    } else {
        smoothing.max(2.0)
    };
    let normalized_y = ((radius - edge_distance) / radius).clamp(0.0, 1.0);
    let normalized_x = (1.0 - normalized_y.powf(smoothing))
        .max(0.0)
        .powf(smoothing.recip());
    radius * (1.0 - normalized_x)
}

fn push_background_effect_row(
    rectangles: &mut Vec<Bounds<i32>>,
    left_edge: f32,
    right_edge: f32,
    y: i32,
    left_inset: f32,
    right_inset: f32,
) {
    let left = (left_edge + left_inset).ceil() as i32;
    let right = (right_edge - right_inset).floor() as i32;
    push_background_effect_rectangle(rectangles, left, y, right - left, 1);
}

fn push_background_effect_rectangle(
    rectangles: &mut Vec<Bounds<i32>>,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
) {
    if width <= 0 || height <= 0 {
        return;
    }

    if let Some(previous) = rectangles.last_mut()
        && previous.origin.x == x
        && previous.size.width == width
        && previous.origin.y + previous.size.height == y
    {
        previous.size.height += height;
        return;
    }

    rectangles.push(Bounds {
        origin: Point { x, y },
        size: Size { width, height },
    });
}

fn set_background_effect_region(
    state: &WaylandWindowState,
    background_effect: &ext_background_effect_surface_v1::ExtBackgroundEffectSurfaceV1,
    region: &BackgroundEffectRegion,
) {
    let wl_region = state
        .globals
        .compositor
        .create_region(&state.globals.qh, ());
    for rectangle in background_effect_rectangles(region) {
        wl_region.add(
            rectangle.origin.x,
            rectangle.origin.y,
            rectangle.size.width,
            rectangle.size.height,
        );
    }
    background_effect.set_blur_region(Some(&wl_region));
    wl_region.destroy();
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BackgroundBlurProtocol {
    Ext,
    Kde,
}

fn ime_enabled_for(input_handler: &mut PlatformInputHandler) -> bool {
    input_handler.query_prefers_ime_for_printable_keys()
}

fn output_scale_sets_buffer_scale(surface_version: u32, fractional_scaling: bool) -> bool {
    !fractional_scaling && surface_version < wl_surface::EVT_PREFERRED_BUFFER_SCALE_SINCE
}

fn background_blur_protocol(
    has_ext_manager: bool,
    ext_blur_supported: bool,
    has_kde_manager: bool,
) -> Option<BackgroundBlurProtocol> {
    if has_ext_manager && ext_blur_supported {
        Some(BackgroundBlurProtocol::Ext)
    } else if has_kde_manager {
        Some(BackgroundBlurProtocol::Kde)
    } else {
        None
    }
}

fn update_window(mut state: RefMut<WaylandWindowState>) {
    let opaque = !state.is_transparent();

    state.renderer.update_transparency(!opaque);
    let surface_area = state
        .bounds
        .map_origin(|_| Pixels::default())
        .map(|v| f32::from(v) as i32);

    let region = state
        .globals
        .compositor
        .create_region(&state.globals.qh, ());
    region.add(
        surface_area.origin.x,
        surface_area.origin.y,
        surface_area.size.width,
        surface_area.size.height,
    );

    // Note that rounded corners make this rectangle API hard to work with.
    // As this is common when using CSD, let's just disable this API.
    if state.background_appearance == WindowBackgroundAppearance::Opaque
        && state.decorations == WindowDecorations::Server
    {
        // Promise the compositor that this region of the window surface
        // contains no transparent pixels. This allows the compositor to skip
        // updating whatever is behind the surface for better performance.
        state.surface.set_opaque_region(Some(&region));
    } else {
        state.surface.set_opaque_region(None);
    }

    let background_effect_manager = state.globals.background_effect_manager.clone();
    let blur_manager = state.globals.blur_manager.clone();
    let protocol = background_blur_protocol(
        background_effect_manager.is_some(),
        state.globals.background_effect_blur_supported.get(),
        blur_manager.is_some(),
    );
    let wants_blur = state.background_appearance == WindowBackgroundAppearance::Blurred;
    let mut commit_surface = false;

    match (wants_blur, protocol) {
        (true, Some(BackgroundBlurProtocol::Ext)) => {
            if state.blur.is_some() {
                if let Some(blur_manager) = &blur_manager {
                    blur_manager.unset(&state.surface);
                }
                if let Some(blur) = state.blur.take() {
                    blur.release();
                }
            }

            if state.background_effect.is_none() {
                let background_effect = background_effect_manager
                    .as_ref()
                    .unwrap()
                    .get_background_effect(&state.surface, &state.globals.qh, ());
                state.background_effect = Some(background_effect);
            }
            let background_effect_region = state
                .background_effect_region
                .clone()
                .unwrap_or_else(|| BackgroundEffectRegion::full_surface(state.bounds.size));
            set_background_effect_region(
                &state,
                state.background_effect.as_ref().unwrap(),
                &background_effect_region,
            );
            commit_surface = true;
        }
        (true, Some(BackgroundBlurProtocol::Kde)) => {
            if let Some(background_effect) = state.background_effect.take() {
                background_effect.destroy();
                commit_surface = true;
            }

            if state.blur.is_none() {
                let blur =
                    blur_manager
                        .as_ref()
                        .unwrap()
                        .create(&state.surface, &state.globals.qh, ());
                state.blur = Some(blur);
            }
            state.blur.as_ref().unwrap().commit();
        }
        _ => {
            if let Some(background_effect) = state.background_effect.take() {
                background_effect.destroy();
                commit_surface = true;
            }

            if let Some(blur_manager) = &blur_manager {
                // It probably doesn't hurt to clear the blur for opaque windows.
                blur_manager.unset(&state.surface);
            }
            if let Some(blur) = state.blur.take() {
                blur.release();
            }
        }
    }

    region.destroy();
    if commit_surface {
        // ext-background-effect state is double-buffered with the wl_surface.
        state.surface.commit();
    }
}

pub(crate) trait WindowDecorationsExt {
    fn to_xdg(self) -> zxdg_toplevel_decoration_v1::Mode;
}

impl WindowDecorationsExt for WindowDecorations {
    fn to_xdg(self) -> zxdg_toplevel_decoration_v1::Mode {
        match self {
            WindowDecorations::Client => zxdg_toplevel_decoration_v1::Mode::ClientSide,
            WindowDecorations::Server => zxdg_toplevel_decoration_v1::Mode::ServerSide,
        }
    }
}

pub(crate) trait ResizeEdgeWaylandExt {
    fn to_xdg(self) -> xdg_toplevel::ResizeEdge;
}

impl ResizeEdgeWaylandExt for ResizeEdge {
    fn to_xdg(self) -> xdg_toplevel::ResizeEdge {
        match self {
            ResizeEdge::Top => xdg_toplevel::ResizeEdge::Top,
            ResizeEdge::TopRight => xdg_toplevel::ResizeEdge::TopRight,
            ResizeEdge::Right => xdg_toplevel::ResizeEdge::Right,
            ResizeEdge::BottomRight => xdg_toplevel::ResizeEdge::BottomRight,
            ResizeEdge::Bottom => xdg_toplevel::ResizeEdge::Bottom,
            ResizeEdge::BottomLeft => xdg_toplevel::ResizeEdge::BottomLeft,
            ResizeEdge::Left => xdg_toplevel::ResizeEdge::Left,
            ResizeEdge::TopLeft => xdg_toplevel::ResizeEdge::TopLeft,
        }
    }
}

/// The configuration event is in terms of the window geometry, which we are constantly
/// updating to account for the client decorations. But that's not the area we want to render
/// to, due to our intrusize CSD. So, here we calculate the 'actual' size, by adding back in the insets
fn compute_outer_size(
    inset: Pixels,
    new_size: Option<Size<Pixels>>,
    tiling: Tiling,
) -> Option<Size<Pixels>> {
    new_size.map(|mut new_size| {
        if !tiling.top {
            new_size.height += inset;
        }
        if !tiling.bottom {
            new_size.height += inset;
        }
        if !tiling.left {
            new_size.width += inset;
        }
        if !tiling.right {
            new_size.width += inset;
        }

        new_size
    })
}

fn inset_by_tiling(mut bounds: Bounds<Pixels>, inset: Pixels, tiling: Tiling) -> Bounds<Pixels> {
    if !tiling.top {
        bounds.origin.y += inset;
        bounds.size.height -= inset;
    }
    if !tiling.bottom {
        bounds.size.height -= inset;
    }
    if !tiling.left {
        bounds.origin.x += inset;
        bounds.size.width -= inset;
    }
    if !tiling.right {
        bounds.size.width -= inset;
    }

    bounds
}

#[cfg(test)]
mod tests {
    use std::ops::Range;

    use gpui::{
        App, Bounds, Corners, DevicePixels, InputHandler, KeyBinding, Pixels, PlatformInputHandler,
        Point, Scene, Size, TestAppContext, UTF16Selection, Window, WindowCornerMask, actions,
        point, px, size,
    };

    use super::{
        BackgroundBlurProtocol, BackgroundEffectRegion, background_blur_protocol,
        background_effect_rectangles, ime_enabled_for, output_scale_sets_buffer_scale,
    };

    actions!(wayland_window_test, [ChordAction]);

    struct TextField;

    impl InputHandler for TextField {
        fn selected_text_range(
            &mut self,
            _: bool,
            _: &mut Window,
            _: &mut App,
        ) -> Option<UTF16Selection> {
            None
        }

        fn marked_text_range(&mut self, _: &mut Window, _: &mut App) -> Option<Range<usize>> {
            None
        }

        fn text_for_range(
            &mut self,
            _: Range<usize>,
            _: &mut Option<Range<usize>>,
            _: &mut Window,
            _: &mut App,
        ) -> Option<String> {
            None
        }

        fn replace_text_in_range(
            &mut self,
            _: Option<Range<usize>>,
            _: &str,
            _: &mut Window,
            _: &mut App,
        ) {
        }

        fn replace_and_mark_text_in_range(
            &mut self,
            _: Option<Range<usize>>,
            _: &str,
            _: Option<Range<usize>>,
            _: &mut Window,
            _: &mut App,
        ) {
        }

        fn unmark_text(&mut self, _: &mut Window, _: &mut App) {}

        fn bounds_for_range(
            &mut self,
            _: Range<usize>,
            _: &mut Window,
            _: &mut App,
        ) -> Option<Bounds<Pixels>> {
            None
        }

        fn character_index_for_point(
            &mut self,
            _: Point<Pixels>,
            _: &mut Window,
            _: &mut App,
        ) -> Option<usize> {
            None
        }

        fn prefers_ime_for_printable_keys(&mut self, _: &mut Window, _: &mut App) -> bool {
            true
        }
    }

    #[gpui::test]
    fn ime_stays_off_until_a_pending_chord_resolves(cx: &mut TestAppContext) {
        cx.update(|cx| cx.bind_keys([KeyBinding::new("ctrl-k m", ChordAction, None)]));
        let cx = cx.add_empty_window();
        let mut input_handler = cx.update(|window, cx| {
            PlatformInputHandler::new(window.to_async(cx), Box::new(TextField))
        });

        assert!(ime_enabled_for(&mut input_handler));
        cx.simulate_keystrokes("ctrl-k");
        cx.update(|window, _| assert!(window.has_pending_keystrokes()));
        assert!(!ime_enabled_for(&mut input_handler));
        cx.simulate_keystrokes("m");
        cx.update(|window, _| assert!(!window.has_pending_keystrokes()));
        assert!(ime_enabled_for(&mut input_handler));
    }

    fn rectangles_cover(rectangles: &[Bounds<i32>], x: i32, y: i32) -> bool {
        rectangles.iter().any(|rectangle| {
            x >= rectangle.origin.x
                && y >= rectangle.origin.y
                && x < rectangle.origin.x + rectangle.size.width
                && y < rectangle.origin.y + rectangle.size.height
        })
    }

    #[test]
    fn output_scale_yields_to_fractional_and_preferred_buffer_scale() {
        assert!(output_scale_sets_buffer_scale(5, false));
        assert!(!output_scale_sets_buffer_scale(5, true));
        assert!(!output_scale_sets_buffer_scale(6, false));
        assert!(!output_scale_sets_buffer_scale(6, true));
    }

    #[test]
    fn prefers_ext_background_effect_and_falls_back_to_kde_blur() {
        assert_eq!(
            background_blur_protocol(true, true, true),
            Some(BackgroundBlurProtocol::Ext)
        );
        assert_eq!(
            background_blur_protocol(true, false, true),
            Some(BackgroundBlurProtocol::Kde)
        );
        assert_eq!(background_blur_protocol(true, false, false), None);
    }

    #[test]
    fn full_background_effect_region_covers_the_surface_once() {
        assert_eq!(
            background_effect_rectangles(&BackgroundEffectRegion::full_surface(size(
                px(100.0),
                px(60.0),
            ))),
            [Bounds {
                origin: Point { x: 0, y: 0 },
                size: Size {
                    width: 100,
                    height: 60,
                },
            }]
        );
    }

    #[test]
    fn rounded_background_effect_region_stays_inside_the_window_mask() {
        let scale = 2.5;
        let mut scene = Scene::default();
        scene.window_corner_mask = Some(WindowCornerMask {
            bounds: Bounds {
                origin: point(px(12.0), px(12.0)),
                size: size(px(100.0), px(60.0)),
            }
            .scale(scale),
            corner_radii: Corners::all(px(14.0)).scale(scale),
            corner_smoothing: 4.0,
        });
        let region = BackgroundEffectRegion::for_scene(
            &scene,
            size(px(124.0), px(84.0)),
            size(DevicePixels(310), DevicePixels(210)),
            Point::default(),
        );
        let rectangles = background_effect_rectangles(&region);
        let first = rectangles.first().unwrap();
        let last = rectangles.last().unwrap();

        assert_eq!(region.bounds.origin, point(px(12.0), px(12.0)));
        assert_eq!(region.bounds.size, size(px(100.0), px(60.0)));
        assert_eq!(region.corner_radii, Corners::all(px(14.0)));
        assert_eq!(region.corner_smoothing, 4.0);
        assert_eq!(first.origin.y, 12);
        assert!(first.origin.x > 12);
        assert!(first.size.width < 100);
        assert_eq!(last.origin.y + last.size.height, 72);
        assert_eq!(last.origin.x, first.origin.x);
        assert_eq!(last.size.width, first.size.width);
        assert!(rectangles.iter().all(|rectangle| {
            rectangle.origin.x >= 12
                && rectangle.origin.y >= 12
                && rectangle.origin.x + rectangle.size.width <= 112
                && rectangle.origin.y + rectangle.size.height <= 72
        }));
        assert!(rectangles.iter().any(|rectangle| {
            rectangle.origin.x == 12 && rectangle.size.width == 100 && rectangle.size.height > 1
        }));
    }

    #[test]
    fn compositor_content_offset_cancels_the_csd_surface_origin() {
        let mut scene = Scene::default();
        scene.window_corner_mask = Some(WindowCornerMask {
            bounds: Bounds {
                origin: point(gpui::ScaledPixels(12.0), gpui::ScaledPixels(12.0)),
                size: size(gpui::ScaledPixels(100.0), gpui::ScaledPixels(60.0)),
            },
            corner_radii: Corners::all(gpui::ScaledPixels(14.0)),
            corner_smoothing: 4.0,
        });

        let region = BackgroundEffectRegion::for_scene(
            &scene,
            size(px(124.0), px(84.0)),
            size(DevicePixels(124), DevicePixels(84)),
            point(px(12.0), px(12.0)),
        );

        assert_eq!(region.bounds.origin, Point::default());
        assert_eq!(region.bounds.size, size(px(100.0), px(60.0)));
        assert_eq!(region.corner_radii, Corners::all(px(14.0)));
    }

    #[test]
    fn background_effect_uses_the_actual_buffer_to_surface_ratio() {
        let mut scene = Scene::default();
        scene.window_corner_mask = Some(WindowCornerMask {
            bounds: Bounds {
                origin: point(gpui::ScaledPixels(12.0), gpui::ScaledPixels(13.0)),
                size: size(gpui::ScaledPixels(102.0), gpui::ScaledPixels(50.0)),
            },
            corner_radii: Corners::all(gpui::ScaledPixels(17.5)),
            corner_smoothing: 4.0,
        });
        let surface_size = size(px(101.0), px(61.0));
        let buffer_size = size(DevicePixels(126), DevicePixels(76));
        let region =
            BackgroundEffectRegion::for_scene(&scene, surface_size, buffer_size, Point::default());
        let x_scale = 101.0 / 126.0;
        let y_scale = 61.0 / 76.0;
        let close = |actual: f32, expected: f32| {
            assert!((actual - expected).abs() < 0.0001, "{actual} != {expected}")
        };

        close(f32::from(region.bounds.origin.x), 12.0 * x_scale);
        close(f32::from(region.bounds.origin.y), 13.0 * y_scale);
        close(f32::from(region.bounds.size.width), 102.0 * x_scale);
        close(f32::from(region.bounds.size.height), 50.0 * y_scale);
        close(
            f32::from(region.corner_radii.top_left),
            17.5 * x_scale.min(y_scale),
        );
    }

    #[test]
    fn window_mask_region_remains_symmetric_through_zoom_and_output_scale() {
        let surface_size = size(px(801.0), px(601.0));
        let snap = |value: f32| (value.abs() - 0.5).ceil().copysign(value);

        for output_scale in [1.0_f32, 1.25, 2.0] {
            let buffer_size = size(
                DevicePixels((801.0 * output_scale).round() as i32),
                DevicePixels((601.0 * output_scale).round() as i32),
            );

            for zoom in [0.8, 1.0, 1.5, 2.0] {
                let effective_scale = output_scale * zoom;
                let viewport_size = surface_size.map(|length| length / zoom);
                let left = snap(12.0 * effective_scale);
                let top = snap(12.0 * effective_scale);
                let right = snap((f32::from(viewport_size.width) - 12.0) * effective_scale);
                let bottom = snap((f32::from(viewport_size.height) - 12.0) * effective_scale);
                let mut scene = Scene::default();
                scene.window_corner_mask = Some(WindowCornerMask {
                    bounds: Bounds::from_corners(
                        point(gpui::ScaledPixels(left), gpui::ScaledPixels(top)),
                        point(gpui::ScaledPixels(right), gpui::ScaledPixels(bottom)),
                    ),
                    corner_radii: Corners::all(gpui::ScaledPixels(14.0 * effective_scale)),
                    corner_smoothing: 4.0,
                });
                let region = BackgroundEffectRegion::for_scene(
                    &scene,
                    surface_size,
                    buffer_size,
                    Point::default(),
                );
                let left_inset = f32::from(region.bounds.left());
                let top_inset = f32::from(region.bounds.top());
                let right_inset = f32::from(surface_size.width) - f32::from(region.bounds.right());
                let bottom_inset =
                    f32::from(surface_size.height) - f32::from(region.bounds.bottom());
                let radius = f32::from(region.corner_radii.top_left);

                assert!(
                    (left_inset - right_inset).abs() < 1.1,
                    "output_scale={output_scale}, zoom={zoom}: {left_inset} != {right_inset}",
                );
                assert!(
                    (top_inset - bottom_inset).abs() < 1.1,
                    "output_scale={output_scale}, zoom={zoom}: {top_inset} != {bottom_inset}",
                );
                assert!(
                    (radius - 14.0 * zoom).abs() < 1.1,
                    "output_scale={output_scale}, zoom={zoom}: radius={radius}",
                );

                let rectangles = background_effect_rectangles(&region);
                assert!(rectangles_cover(&rectangles, 400, top_inset.ceil() as i32));
                assert!(rectangles_cover(&rectangles, left_inset.ceil() as i32, 300));
                assert!(!rectangles_cover(&rectangles, 400, 0));
                assert!(!rectangles_cover(&rectangles, 0, 300));
                assert!(!rectangles_cover(&rectangles, 0, 0));
                assert!(!rectangles_cover(&rectangles, 800, 600));
            }
        }
    }

    #[test]
    fn window_mask_region_respects_partially_tiled_edges() {
        let mut scene = Scene::default();
        scene.window_corner_mask = Some(WindowCornerMask {
            bounds: Bounds {
                origin: point(gpui::ScaledPixels(0.0), gpui::ScaledPixels(12.0)),
                size: size(gpui::ScaledPixels(112.0), gpui::ScaledPixels(60.0)),
            },
            corner_radii: Corners {
                top_left: gpui::ScaledPixels(0.0),
                top_right: gpui::ScaledPixels(14.0),
                bottom_right: gpui::ScaledPixels(14.0),
                bottom_left: gpui::ScaledPixels(0.0),
            },
            corner_smoothing: 4.0,
        });
        let region = BackgroundEffectRegion::for_scene(
            &scene,
            size(px(124.0), px(84.0)),
            size(DevicePixels(124), DevicePixels(84)),
            Point::default(),
        );
        let rectangles = background_effect_rectangles(&region);

        assert_eq!(
            region.corner_radii,
            Corners {
                top_left: px(0.0),
                top_right: px(14.0),
                bottom_right: px(14.0),
                bottom_left: px(0.0),
            }
        );
        assert!(rectangles_cover(&rectangles, 0, 12));
        assert!(rectangles_cover(&rectangles, 0, 71));
        assert!(!rectangles_cover(&rectangles, 0, 11));
        assert!(!rectangles_cover(&rectangles, 123, 42));
    }

    #[test]
    fn square_window_mask_keeps_the_shadow_outside_the_blur_region() {
        let mut scene = Scene::default();
        scene.window_corner_mask = Some(WindowCornerMask {
            bounds: Bounds {
                origin: point(px(12.0), px(12.0)),
                size: size(px(100.0), px(60.0)),
            }
            .scale(1.0),
            corner_radii: Corners::default().scale(1.0),
            corner_smoothing: 4.0,
        });
        let region = BackgroundEffectRegion::for_scene(
            &scene,
            size(px(124.0), px(84.0)),
            size(DevicePixels(124), DevicePixels(84)),
            Point::default(),
        );
        let rectangles = background_effect_rectangles(&region);

        assert_eq!(
            rectangles,
            [Bounds {
                origin: Point { x: 12, y: 12 },
                size: Size {
                    width: 100,
                    height: 60,
                },
            }]
        );
    }

    #[test]
    fn squircle_blur_region_tracks_the_window_masks_fuller_corner() {
        let region = |corner_smoothing| BackgroundEffectRegion {
            bounds: Bounds {
                origin: point(px(0.0), px(0.0)),
                size: size(px(100.0), px(60.0)),
            },
            corner_radii: Corners::all(px(14.0)),
            corner_smoothing,
        };
        let circular = background_effect_rectangles(&region(2.0));
        let squircle = background_effect_rectangles(&region(4.0));

        assert!(squircle[0].origin.x < circular[0].origin.x);
        assert!(squircle[0].size.width > circular[0].size.width);
    }
}
