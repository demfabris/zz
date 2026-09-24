use std::{
    cell::RefCell,
    collections::{HashMap, hash_map::Entry},
    sync::{Arc, Weak},
};

use gpui::{
    App, Bounds, Element, ElementId, GlobalElementId, InspectorElementId, IntoElement, LayoutId,
    Pixels, Position, Style, Window,
};
use objc2::{MainThreadMarker, MainThreadOnly as _, rc::Retained, runtime::AnyObject};
use objc2_app_kit::{NSAutoresizingMaskOptions, NSView, NSWindowOrderingMode};
use objc2_core_foundation::{CGPoint, CGRect, CGSize};
use objc2_quartz_core::{CALayer, CATransaction, kCAGravityResize};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use zz_browser::{MacFramePresenter, MacIoSurface};

struct Layer(Retained<CALayer>);

// SAFETY: Core Animation accepts layer property changes from any thread inside
// an explicit transaction, and this wrapper only exposes such changes.
#[allow(
    unsafe_code,
    reason = "CALayer is shared with Metal's completion thread"
)]
unsafe impl Send for Layer {}
#[allow(
    unsafe_code,
    reason = "CALayer is shared with Metal's completion thread"
)]
unsafe impl Sync for Layer {}

impl Layer {
    #[allow(
        unsafe_code,
        reason = "an IOSurfaceRef is toll-free bridged to the IOSurface object CALayer.contents takes"
    )]
    fn show(&self, surface: &MacIoSurface) {
        // SAFETY: MacIoSurface retains a live IOSurfaceRef for the duration of this call.
        let contents = unsafe { &*surface.as_ptr().cast::<AnyObject>() };
        CATransaction::begin();
        CATransaction::setDisableActions(true);
        // SAFETY: CALayer retains the IOSurface it is given as contents.
        unsafe { self.0.setContents(Some(contents)) };
        CATransaction::commit();
    }
}

pub(crate) struct PaneUnderlay(Arc<Layer>);

#[derive(Clone)]
pub(crate) struct UnderlayHandle(Arc<Layer>);

impl PaneUnderlay {
    #[allow(unsafe_code, reason = "reads an immutable Core Animation constant")]
    pub(crate) fn new(first_frame: &MacIoSurface) -> Self {
        let layer = CALayer::new();
        // SAFETY: kCAGravityResize is an immutable framework constant.
        layer.setContentsGravity(unsafe { kCAGravityResize });
        layer.setMasksToBounds(true);
        layer.setHidden(true);
        let layer = Arc::new(Layer(layer));
        layer.show(first_frame);
        Self(layer)
    }

    pub(crate) fn presenter(&self) -> MacFramePresenter {
        let layer = Arc::clone(&self.0);
        Arc::new(move |surface| layer.show(surface))
    }

    pub(crate) fn handle(&self) -> UnderlayHandle {
        UnderlayHandle(Arc::clone(&self.0))
    }
}

impl Drop for PaneUnderlay {
    fn drop(&mut self) {
        CATransaction::begin();
        CATransaction::setDisableActions(true);
        self.0.0.removeFromSuperlayer();
        CATransaction::commit();
    }
}

struct Host {
    view: Retained<NSView>,
    root: Retained<CALayer>,
    placed: Vec<(Weak<Layer>, bool)>,
}

impl Host {
    fn new(gpui_view: &NSView, mtm: MainThreadMarker) -> Option<Self> {
        let content = gpui_view_superview(gpui_view)?;
        let root = CALayer::new();
        let view = NSView::initWithFrame(NSView::alloc(mtm), content.bounds());
        view.setLayer(Some(&root));
        view.setWantsLayer(true);
        view.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewWidthSizable
                | NSAutoresizingMaskOptions::ViewHeightSizable,
        );
        content.addSubview_positioned_relativeTo(
            &view,
            NSWindowOrderingMode::Below,
            Some(gpui_view),
        );
        Some(Self {
            view,
            root,
            placed: Vec::new(),
        })
    }

    fn serves(&self, gpui_view: &NSView) -> bool {
        match (
            gpui_view_superview(&self.view),
            gpui_view_superview(gpui_view),
        ) {
            (Some(ours), Some(theirs)) => Retained::as_ptr(&ours) == Retained::as_ptr(&theirs),
            _ => false,
        }
    }
}

thread_local! {
    static HOSTS: RefCell<HashMap<usize, Host>> = RefCell::new(HashMap::new());
}

#[allow(
    unsafe_code,
    reason = "NSView.superview is marked unsafe in objc2-app-kit"
)]
fn gpui_view_superview(view: &NSView) -> Option<Retained<NSView>> {
    // SAFETY: the view is alive and this runs on the main thread.
    unsafe { view.superview() }
}

#[allow(
    unsafe_code,
    reason = "raw-window-handle hands out the window's NSView as a raw pointer"
)]
fn gpui_view(window: &Window) -> Option<Retained<NSView>> {
    let handle = HasWindowHandle::window_handle(window).ok()?;
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
        return None;
    };
    // SAFETY: raw-window-handle guarantees a live NSView, and paint runs on the main thread.
    unsafe { Retained::retain(handle.ns_view.as_ptr().cast()) }
}

pub(crate) fn place(window: &Window, underlay: &UnderlayHandle, bounds: Bounds<Pixels>) -> bool {
    let Some(mtm) = MainThreadMarker::new() else {
        return false;
    };
    let Some(gpui_view) = gpui_view(window) else {
        return false;
    };
    let key = Retained::as_ptr(&gpui_view).addr();
    HOSTS.with_borrow_mut(|hosts| {
        if hosts.get(&key).is_some_and(|host| !host.serves(&gpui_view)) {
            hosts.remove(&key);
        }
        let host = match hosts.entry(key) {
            Entry::Occupied(entry) => entry.into_mut(),
            Entry::Vacant(entry) => {
                let Some(host) = Host::new(&gpui_view, mtm) else {
                    return false;
                };
                entry.insert(host)
            }
        };
        let layer = &underlay.0.0;
        CATransaction::begin();
        CATransaction::setDisableActions(true);
        if layer
            .superlayer()
            .is_none_or(|parent| Retained::as_ptr(&parent) != Retained::as_ptr(&host.root))
        {
            host.root.addSublayer(layer);
        }
        let zoom = f64::from(window.zoom());
        let point = |value: Pixels| f64::from(f32::from(value)) * zoom;
        layer.setContentsScale(f64::from(window.scale_factor()) / zoom);
        let height = point(bounds.size.height);
        let y = host.view.bounds().size.height - point(bounds.origin.y) - height;
        layer.setFrame(CGRect::new(
            CGPoint::new(point(bounds.origin.x), y),
            CGSize::new(point(bounds.size.width), height),
        ));
        layer.setHidden(false);
        CATransaction::commit();
        match host
            .placed
            .iter_mut()
            .find(|(placed, _)| placed.as_ptr() == Arc::as_ptr(&underlay.0))
        {
            Some((_, painted)) => *painted = true,
            None => host.placed.push((Arc::downgrade(&underlay.0), true)),
        }
        true
    })
}

fn finish_frame(window: &Window) {
    let Some(gpui_view) = gpui_view(window) else {
        return;
    };
    let key = Retained::as_ptr(&gpui_view).addr();
    let any_painted = HOSTS.with_borrow_mut(|hosts| {
        let host = hosts.get_mut(&key)?;
        let mut any_painted = false;
        CATransaction::begin();
        CATransaction::setDisableActions(true);
        host.placed.retain_mut(|(layer, painted)| {
            let Some(layer) = layer.upgrade() else {
                return false;
            };
            if *painted {
                any_painted = true;
            } else {
                layer.0.setHidden(true);
            }
            *painted = false;
            true
        });
        CATransaction::commit();
        Some(any_painted)
    });
    if let Some(any_painted) = any_painted {
        window.set_underlay_active(any_painted);
    }
}

pub(crate) struct UnderlayFlush;

impl IntoElement for UnderlayFlush {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for UnderlayFlush {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let style = Style {
            position: Position::Absolute,
            ..Style::default()
        };
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        (): &mut Self::RequestLayoutState,
        _: &mut Window,
        _: &mut App,
    ) -> Self::PrepaintState {
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        (): &mut Self::RequestLayoutState,
        (): &mut Self::PrepaintState,
        window: &mut Window,
        _: &mut App,
    ) {
        finish_frame(window);
    }
}
