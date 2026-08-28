//! Platform plumbing for `window-background-blur`.

use gpui::{App, Global, Pixels, Window, WindowBackgroundAppearance};

struct CompositorBlurSupport(bool);

impl Global for CompositorBlurSupport {}

pub(crate) fn compositor_supports_blur(cx: &App) -> bool {
    cx.try_global::<CompositorBlurSupport>()
        .is_none_or(|support| support.0)
}

/// Probes the platform once at startup and records whether a backdrop blur
/// request can land. Everything downstream keys off this answer.
pub fn detect_compositor_support(cx: &mut App) {
    let supported = probe_compositor();
    if !supported {
        log::info!(
            target: "zz::window_background",
            "compositor does not support background blur; window-background-blur will keep the chrome opaque"
        );
    }
    cx.set_global(CompositorBlurSupport(supported));
}

#[cfg(test)]
pub(crate) fn set_compositor_support_for_tests(supported: bool, cx: &mut App) {
    cx.set_global(CompositorBlurSupport(supported));
}

fn probe_compositor() -> bool {
    #[cfg(target_os = "linux")]
    {
        linux::probe_compositor()
    }
    #[cfg(not(target_os = "linux"))]
    {
        true
    }
}

pub(crate) const fn native_appearance(
    requested: WindowBackgroundAppearance,
) -> WindowBackgroundAppearance {
    #[cfg(target_os = "macos")]
    if matches!(requested, WindowBackgroundAppearance::Blurred) {
        // GPUI's NSVisualEffectView Selection material stopped blurring on macOS 27.
        return WindowBackgroundAppearance::Transparent;
    }

    requested
}

pub(crate) fn apply(window: &Window, requested: WindowBackgroundAppearance, corner_radius: Pixels) {
    let enabled = requested == WindowBackgroundAppearance::Blurred;

    #[cfg(target_os = "macos")]
    if let Err(error) = macos::set_blur(window, enabled) {
        log::warn!(target: "zz::window_background", "could not update macOS window blur: {error}");
    }

    #[cfg(target_os = "linux")]
    if let Err(error) = linux::set_x11_blur(window, enabled, corner_radius) {
        log::warn!(target: "zz::window_background", "could not update X11 window blur: {error}");
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    let _ = (window, enabled, corner_radius);

    #[cfg(target_os = "macos")]
    let _ = corner_radius;
}

#[cfg(any(target_os = "linux", test))]
fn rounded_region(
    width: u32,
    height: u32,
    radius: u32,
    corners: crate::window::corners::WindowCorners,
) -> Vec<u32> {
    let radius = radius.min(width / 2).min(height / 2);
    if radius == 0 || corners == crate::window::corners::WindowCorners::NONE {
        return vec![0, 0, width, height];
    }

    let top_radius = if corners.top_left() || corners.top_right() {
        radius
    } else {
        0
    };
    let bottom_radius = if corners.bottom_left_is_rounded() || corners.bottom_right_is_rounded() {
        radius
    } else {
        0
    };
    let mut region = Vec::with_capacity(((top_radius + bottom_radius + 1) * 4) as usize);

    for row in 0..top_radius {
        push_corner_row(
            &mut region,
            width,
            radius,
            row,
            row,
            corners.top_left(),
            corners.top_right(),
        );
    }

    let middle_height = height - top_radius - bottom_radius;
    if middle_height > 0 {
        region.extend_from_slice(&[0, top_radius, width, middle_height]);
    }

    for row in 0..bottom_radius {
        push_corner_row(
            &mut region,
            width,
            radius,
            bottom_radius - row - 1,
            height - bottom_radius + row,
            corners.bottom_left_is_rounded(),
            corners.bottom_right_is_rounded(),
        );
    }

    region
}

#[cfg(any(target_os = "linux", test))]
fn push_corner_row(
    region: &mut Vec<u32>,
    width: u32,
    radius: u32,
    row: u32,
    y: u32,
    round_left: bool,
    round_right: bool,
) {
    let offset = corner_offset(radius, row);
    let left = if round_left { offset } else { 0 };
    let right = if round_right { offset } else { 0 };
    region.extend_from_slice(&[left, y, width.saturating_sub(left + right), 1]);
}

#[cfg(any(target_os = "linux", test))]
fn corner_offset(radius: u32, row: u32) -> u32 {
    let radius_f = radius as f32;
    let dy = (radius - row) as f32 - 0.5;
    let dx = (radius_f.mul_add(radius_f, -(dy * dy))).max(0.0).sqrt();
    radius.saturating_sub((dx + 0.5).round() as u32)
}

#[cfg(target_os = "macos")]
#[allow(
    unsafe_code,
    reason = "the raw AppKit handle and the WindowServer blur call require unsafe access"
)]
mod macos {
    use std::fmt;

    use gpui::Window;
    use objc2::{MainThreadMarker, rc::Retained};
    use objc2_app_kit::NSView;
    use raw_window_handle::RawWindowHandle;

    const BLUR_RADIUS: i32 = 40;

    type ConnectionId = u32;
    type WindowId = u32;

    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGSMainConnectionID() -> ConnectionId;
        fn CGSSetWindowBackgroundBlurRadius(
            connection: ConnectionId,
            window: WindowId,
            radius: i32,
        ) -> i32;
    }

    #[derive(Debug)]
    pub(super) enum BlurError {
        WindowHandle(raw_window_handle::HandleError),
        UnexpectedWindowHandle,
        NotMainThread,
        MissingView,
        MissingWindow,
        WindowServer(i32),
    }

    impl fmt::Display for BlurError {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            match self {
                Self::WindowHandle(error) => {
                    write!(formatter, "could not read native window handle: {error}")
                }
                Self::UnexpectedWindowHandle => {
                    formatter.write_str("GPUI returned a non-AppKit window handle")
                }
                Self::NotMainThread => {
                    formatter.write_str("WindowServer blur update ran outside the main thread")
                }
                Self::MissingView => formatter.write_str("AppKit window view is unavailable"),
                Self::MissingWindow => {
                    formatter.write_str("AppKit view is not attached to an on-screen window")
                }
                Self::WindowServer(status) => {
                    write!(
                        formatter,
                        "WindowServer rejected the blur radius (status {status})"
                    )
                }
            }
        }
    }

    pub(super) fn set_blur(window: &Window, enabled: bool) -> Result<(), BlurError> {
        MainThreadMarker::new().ok_or(BlurError::NotMainThread)?;
        let handle = raw_window_handle::HasWindowHandle::window_handle(window)
            .map_err(BlurError::WindowHandle)?;
        let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
            return Err(BlurError::UnexpectedWindowHandle);
        };
        // SAFETY: raw-window-handle guarantees this is a live NSView for the lifetime of
        // `handle`, and this function only runs on GPUI's AppKit main thread.
        let view = unsafe { Retained::<NSView>::retain(handle.ns_view.as_ptr().cast()) }
            .ok_or(BlurError::MissingView)?;
        let native_window = view.window().ok_or(BlurError::MissingWindow)?;
        let window_id = WindowId::try_from(native_window.windowNumber())
            .ok()
            .filter(|id| *id != 0)
            .ok_or(BlurError::MissingWindow)?;
        let radius = if enabled { BLUR_RADIUS } else { 0 };
        // SAFETY: both ids are plain integers owned by this process's WindowServer connection;
        // the call has no memory effects and reports failure through its status code.
        let status =
            unsafe { CGSSetWindowBackgroundBlurRadius(CGSMainConnectionID(), window_id, radius) };
        if status == 0 {
            Ok(())
        } else {
            Err(BlurError::WindowServer(status))
        }
    }
}

// Test builds on other unixes compile this module so backend changes
// type-check from a mac; Windows cannot join in because wayland-sys does not
// compile there at all.
#[cfg(any(target_os = "linux", all(test, unix)))]
mod linux {
    use std::{fmt, sync::OnceLock};

    use gpui::Window;
    use raw_window_handle::RawWindowHandle;
    use x11rb::{
        connection::Connection as _,
        protocol::xproto::{AtomEnum, ConnectionExt as _, PropMode},
        rust_connection::RustConnection,
        wrapper::ConnectionExt as _,
    };

    use crate::{window::background::rounded_region, window::corners::WindowCorners};

    struct BlurConnection {
        connection: RustConnection,
        atom: u32,
        root: u32,
    }

    static BLUR_CONNECTION: OnceLock<Result<BlurConnection, String>> = OnceLock::new();

    #[derive(Debug)]
    pub(super) enum BlurError {
        WindowHandle(raw_window_handle::HandleError),
        Connection(String),
        X11(Box<dyn std::error::Error + Send + Sync>),
    }

    impl fmt::Display for BlurError {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            match self {
                Self::WindowHandle(error) => {
                    write!(formatter, "could not read native window handle: {error}")
                }
                Self::Connection(error) => formatter.write_str(error),
                Self::X11(error) => write!(formatter, "X11 request failed: {error}"),
            }
        }
    }

    pub(super) fn set_x11_blur(
        window: &Window,
        enabled: bool,
        corner_radius: gpui::Pixels,
    ) -> Result<(), BlurError> {
        let handle = raw_window_handle::HasWindowHandle::window_handle(window)
            .map_err(BlurError::WindowHandle)?;
        let RawWindowHandle::Xcb(handle) = handle.as_raw() else {
            return Ok(());
        };
        let window_id = handle.window.get();
        let blur = blur_connection()?;
        if enabled {
            let geometry = blur
                .connection
                .get_geometry(window_id)
                .map_err(x11_error)?
                .reply()
                .map_err(x11_error)?;
            let radius = (f32::from(corner_radius) * window.scale_factor())
                .round()
                .max(0.0) as u32;
            let region = rounded_region(
                u32::from(geometry.width),
                u32::from(geometry.height),
                radius,
                WindowCorners::for_window(window),
            );
            blur.connection
                .change_property32(
                    PropMode::REPLACE,
                    window_id,
                    blur.atom,
                    AtomEnum::CARDINAL,
                    &region,
                )
                .map_err(x11_error)?;
        } else {
            blur.connection
                .delete_property(window_id, blur.atom)
                .map_err(x11_error)?;
        }
        blur.connection.flush().map_err(x11_error)?;
        Ok(())
    }

    /// Whether the running compositor can honor a blur request. Wayland answers
    /// through advertised registry globals, X11 through KWin's blur atom on the
    /// root window. picom stays undetected: its blur is local config only.
    pub(super) fn probe_compositor() -> bool {
        let wayland = ["WAYLAND_DISPLAY", "WAYLAND_SOCKET"]
            .iter()
            .any(|key| std::env::var_os(key).is_some_and(|value| !value.is_empty()));
        if wayland {
            wayland_advertises_blur()
        } else {
            x11_root_announces_blur()
        }
    }

    fn wayland_advertises_blur() -> bool {
        use wayland_client::{
            Connection as WaylandConnection, Dispatch, QueueHandle,
            protocol::wl_registry::{self, WlRegistry},
        };

        struct RegistryScan {
            blur: bool,
        }

        impl Dispatch<WlRegistry, ()> for RegistryScan {
            fn event(
                scan: &mut Self,
                _: &WlRegistry,
                event: wl_registry::Event,
                (): &(),
                _: &WaylandConnection,
                _: &QueueHandle<Self>,
            ) {
                if let wl_registry::Event::Global { interface, .. } = event
                    && matches!(
                        interface.as_str(),
                        "org_kde_kwin_blur_manager" | "ext_background_effect_manager_v1"
                    )
                {
                    scan.blur = true;
                }
            }
        }

        let Ok(connection) = WaylandConnection::connect_to_env() else {
            return false;
        };
        let mut queue = connection.new_event_queue();
        let _registry = connection.display().get_registry(&queue.handle(), ());
        let mut scan = RegistryScan { blur: false };
        queue.roundtrip(&mut scan).is_ok() && scan.blur
    }

    fn x11_root_announces_blur() -> bool {
        fn root_lists_blur_atom() -> Result<bool, BlurError> {
            let blur = blur_connection()?;
            let properties = blur
                .connection
                .list_properties(blur.root)
                .map_err(x11_error)?
                .reply()
                .map_err(x11_error)?;
            Ok(properties.atoms.contains(&blur.atom))
        }
        root_lists_blur_atom().unwrap_or(false)
    }

    fn blur_connection() -> Result<&'static BlurConnection, BlurError> {
        BLUR_CONNECTION
            .get_or_init(|| {
                let (connection, screen) =
                    x11rb::connect(None).map_err(|error| error.to_string())?;
                let root = connection.setup().roots[screen].root;
                let atom = connection
                    .intern_atom(false, b"_KDE_NET_WM_BLUR_BEHIND_REGION")
                    .map_err(|error| error.to_string())?
                    .reply()
                    .map_err(|error| error.to_string())?
                    .atom;
                Ok(BlurConnection {
                    connection,
                    atom,
                    root,
                })
            })
            .as_ref()
            .map_err(|error| BlurError::Connection(error.clone()))
    }

    fn x11_error(error: impl std::error::Error + Send + Sync + 'static) -> BlurError {
        BlurError::X11(Box::new(error))
    }
}

#[cfg(test)]
mod tests {
    use gpui::Tiling;

    use super::*;
    use crate::window::corners::WindowCorners;

    #[test]
    fn square_blur_region_is_one_full_window_rectangle() {
        assert_eq!(
            rounded_region(100, 60, 14, WindowCorners::NONE),
            [0, 0, 100, 60]
        );
    }

    #[test]
    fn rounded_blur_region_carves_out_only_exposed_corners() {
        let corners = WindowCorners::from_tiling(Tiling::default());
        let region = rounded_region(100, 60, 4, corners);

        assert_eq!(region.len(), (4 + 1 + 4) * 4);
        assert_eq!(&region[4 * 4..5 * 4], &[0, 4, 100, 52]);
        assert!(region[0] > 0);
        assert!(region[2] < 100);
        let last = &region[region.len() - 4..];
        assert_eq!(last[1], 59);
        assert_eq!(last[0], region[0]);
        assert_eq!(last[2], region[2]);
    }

    #[test]
    fn tiled_top_edge_keeps_top_rows_square() {
        let corners = WindowCorners::from_tiling(Tiling {
            top: true,
            ..Tiling::default()
        });
        let region = rounded_region(100, 60, 4, corners);

        assert_eq!(&region[..4], &[0, 0, 100, 56]);
        assert_eq!(region.len(), (1 + 4) * 4);
    }

    #[cfg(all(unix, not(target_os = "linux")))]
    #[test]
    fn x11_backend_type_checks_on_other_test_platforms() {
        let _ = linux::set_x11_blur;
        let _ = linux::probe_compositor;
    }
}
