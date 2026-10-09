#![allow(
    unsafe_code,
    unsafe_op_in_unsafe_fn,
    non_upper_case_globals,
    unexpected_cfgs
)]

use std::{
    cell::{Cell, RefCell},
    ffi::CString,
    rc::Rc,
    sync::Arc,
};

use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use objc::{
    class,
    declare::ClassDecl,
    msg_send,
    runtime::{BOOL, Class, NO, Object, Protocol, Sel, YES},
    sel, sel_impl,
};
use raw_window_handle::RawWindowHandle;
use zpui::{Bounds, Pixels, Window};
use zz_daemon_client::InteractiveClient;

use zpui_platform::ios::{
    CGPoint, CGRect, CGSize, id, nil, ns_array, ns_string, nsstring_to_string,
};
pub use zz_client::element_picker::ElementPickerAppearance;
use zz_client::element_picker::{
    ElementPickOutcome, ElementPickState, PickGeometry, element_picker_start_script,
};

const OBSERVED: &[&str] = &["URL", "title", "loading", "canGoBack", "canGoForward"];
use zz_client::element_picker::ELEMENT_PICKER_SCRIPT as PICKER_SCRIPT;
const PICKER_HANDLER: &str = "zzElementPicker";

#[derive(Clone, Debug)]
pub enum BrowserEvent {
    Changed {
        url: Option<String>,
        title: String,
        loading: bool,
        can_go_back: bool,
        can_go_forward: bool,
        committed: bool,
    },
    Error(String),
    Open(String),
    Focused,
    ElementPicked {
        text: Arc<str>,
        screenshot: Option<Vec<u8>>,
    },
    ElementPickCancelled,
    ElementPickFailed,
}

pub struct BrowserProfile {
    store: id,
    ssh: bool,
    client: RefCell<Option<Arc<InteractiveClient>>>,
    #[allow(clippy::option_option)]
    route: RefCell<Option<Option<u16>>>,
    configured: Cell<bool>,
}

impl BrowserProfile {
    #[must_use]
    pub fn new(host: &str, profile: &str) -> Rc<Self> {
        unsafe {
            let defaults: id = msg_send![class!(NSUserDefaults), standardUserDefaults];
            let key = ns_string(&format!(
                "zz.browser.profile:{}:{host}{profile}",
                host.len()
            ));
            let saved: id = msg_send![defaults, stringForKey: key];
            let identifier: id = if saved.is_null() {
                let identifier: id = msg_send![class!(NSUUID), UUID];
                let value: id = msg_send![identifier, UUIDString];
                let _: () = msg_send![defaults, setObject: value forKey: key];
                identifier
            } else {
                let identifier: id = msg_send![class!(NSUUID), alloc];
                let identifier: id = msg_send![identifier, initWithUUIDString: saved];
                msg_send![identifier, autorelease]
            };
            let store: id =
                msg_send![class!(WKWebsiteDataStore), dataStoreForIdentifier: identifier];
            let _: id = msg_send![store, retain];
            let profile = Rc::new(Self {
                store,
                ssh: host.starts_with("ssh://"),
                client: RefCell::new(None),
                route: RefCell::new(None),
                configured: Cell::new(false),
            });
            profile.set_client(None);
            profile
        }
    }

    pub fn set_client(&self, client: Option<Arc<InteractiveClient>>) -> bool {
        let route = client.as_ref().and_then(|client| {
            if self.ssh {
                client.socks_port().map(Some)
            } else {
                Some(None)
            }
        });
        *self.client.borrow_mut() = client;
        if self.configured.replace(true) && *self.route.borrow() == route {
            return false;
        }
        *self.route.borrow_mut() = route;
        unsafe {
            if route == Some(None) {
                let _: () = msg_send![self.store, setProxyConfigurations: ns_array(&[])];
            } else {
                let port = CString::new(route.flatten().unwrap_or(9).to_string()).unwrap();
                let endpoint = nw_endpoint_create_host(c"127.0.0.1".as_ptr(), port.as_ptr());
                let proxy = nw_proxy_config_create_socksv5(endpoint);
                nw_proxy_config_set_failover_allowed(proxy, false);
                for domain in [c"localhost", c"localhost.", c"127.0.0.1", c"::1"] {
                    nw_proxy_config_add_excluded_domain(proxy, domain.as_ptr());
                }
                let _: () = msg_send![self.store, setProxyConfigurations: ns_array(&[proxy])];
                let _: () = msg_send![proxy, release];
                let _: () = msg_send![endpoint, release];
            }
        }
        true
    }

    pub fn available(&self) -> bool {
        self.route.borrow().is_some()
    }

    unsafe fn prepare(&self, url: id) -> Result<(), String> {
        if !self.available() {
            return Err("Waiting for the host connection…".into());
        }
        let scheme: id = msg_send![url, scheme];
        let scheme = nsstring_to_string(scheme)
            .unwrap_or_default()
            .to_ascii_lowercase();
        if !matches!(
            scheme.as_str(),
            "http" | "https" | "about" | "blob" | "data"
        ) {
            return Err("This address cannot open in the browser pane.".into());
        }
        if !self.ssh || !matches!(scheme.as_str(), "http" | "https") {
            return Ok(());
        }
        let host: id = msg_send![url, host];
        let host = nsstring_to_string(host)
            .unwrap_or_default()
            .to_ascii_lowercase();
        if matches!(
            host.as_str(),
            "localhost" | "localhost." | "127.0.0.1" | "::1" | "[::1]"
        ) {
            let number: id = msg_send![url, port];
            let port: u64 = if number.is_null() {
                if scheme == "https" { 443 } else { 80 }
            } else {
                msg_send![number, unsignedLongLongValue]
            };
            let port = u16::try_from(port)
                .ok()
                .filter(|port| *port != 0)
                .ok_or("Enter a port between 1 and 65535.")?;
            self.client
                .borrow()
                .as_ref()
                .ok_or("The host connection is unavailable.")?
                .forward_loopback(port)
                .map_err(|error| format!("Cannot forward localhost:{port}: {error}"))?;
        } else if host.starts_with("127.") || host == "0.0.0.0" {
            return Err("Use localhost to reach this host’s loopback server.".into());
        }
        Ok(())
    }
}

impl Drop for BrowserProfile {
    fn drop(&mut self) {
        unsafe {
            let _: () = msg_send![self.store, release];
        }
    }
}

struct DelegateState {
    profile: Rc<BrowserProfile>,
    events: UnboundedSender<BrowserEvent>,
    picker: ElementPickState,
    generation: Cell<u64>,
    picking: Cell<bool>,
}

pub struct BrowserView {
    view: id,
    delegate: id,
    _state: Rc<DelegateState>,
}

impl BrowserView {
    pub fn new(profile: Rc<BrowserProfile>) -> (Rc<Self>, UnboundedReceiver<BrowserEvent>) {
        let (events, receiver) = unbounded();
        let state = Rc::new(DelegateState {
            profile,
            events,
            picker: ElementPickState::default(),
            generation: Cell::new(0),
            picking: Cell::new(false),
        });
        unsafe {
            let configuration: id = msg_send![class!(WKWebViewConfiguration), new];
            let _: () = msg_send![configuration, setWebsiteDataStore: state.profile.store];
            let _: () = msg_send![configuration, setAllowsInlineMediaPlayback: YES];
            let view: id = msg_send![class!(WKWebView), alloc];
            let view: id =
                msg_send![view, initWithFrame: CGRect::default() configuration: configuration];
            let _: () = msg_send![configuration, release];
            let _: () = msg_send![view, setAllowsBackForwardNavigationGestures: YES];
            let _: () = msg_send![view, setInspectable: YES];
            let _: () = msg_send![view, setHidden: YES];
            let scroll: id = msg_send![view, scrollView];
            let _: () = msg_send![scroll, setContentInsetAdjustmentBehavior: 2isize];
            let delegate: id = msg_send![delegate_class(), new];
            (*delegate).set_ivar("state", (&raw const *state) as usize);
            let _: () = msg_send![view, setNavigationDelegate: delegate];
            let _: () = msg_send![view, setUIDelegate: delegate];
            let configuration: id = msg_send![view, configuration];
            let controller: id = msg_send![configuration, userContentController];
            let _: () = msg_send![controller, addScriptMessageHandler: delegate name: ns_string(PICKER_HANDLER)];
            for key in OBSERVED {
                let _: () = msg_send![view, addObserver: delegate forKeyPath: ns_string(key) options: 0usize context: std::ptr::null_mut::<std::ffi::c_void>()];
            }
            let tap: id = msg_send![class!(UITapGestureRecognizer), alloc];
            let tap: id = msg_send![tap, initWithTarget: delegate action: sel!(pageTapped:)];
            let _: () = msg_send![tap, setCancelsTouchesInView: NO];
            let _: () = msg_send![tap, setDelegate: delegate];
            let _: () = msg_send![view, addGestureRecognizer: tap];
            let _: () = msg_send![tap, release];
            (
                Rc::new(Self {
                    view,
                    delegate,
                    _state: state,
                }),
                receiver,
            )
        }
    }

    pub fn load(&self, address: &str) -> Result<(), String> {
        self.cancel_element_pick();
        if address.contains('\0') {
            return Err("The address contains an invalid character.".into());
        }
        unsafe {
            let url: id = msg_send![class!(NSURL), URLWithString: ns_string(address)];
            if url.is_null() {
                return Err("Enter an HTTP or HTTPS address.".into());
            }
            self._state.profile.prepare(url)?;
            let request: id = msg_send![class!(NSURLRequest), requestWithURL: url];
            let _: id = msg_send![self.view, loadRequest: request];
        }
        Ok(())
    }

    pub fn back(&self) {
        self.cancel_element_pick();
        unsafe {
            let _: id = msg_send![self.view, goBack];
        }
    }
    pub fn forward(&self) {
        self.cancel_element_pick();
        unsafe {
            let _: id = msg_send![self.view, goForward];
        }
    }
    pub fn reload(&self) {
        self.cancel_element_pick();
        unsafe {
            let _: id = msg_send![self.view, reload];
        }
    }
    pub fn stop(&self) {
        self.cancel_element_pick();
        unsafe {
            let _: () = msg_send![self.view, stopLoading];
        }
    }

    pub fn set_visible(&self, visible: bool) {
        unsafe {
            let hidden: BOOL = msg_send![self.view, isHidden];
            if (hidden == YES) == visible {
                if !visible {
                    self.cancel_element_pick();
                    let _: BOOL = msg_send![self.view, endEditing: YES];
                }
                let _: () = msg_send![self.view, setHidden: if visible { NO } else { YES }];
            }
        }
    }

    pub fn set_interactive(&self, interactive: bool) {
        if !interactive {
            self.cancel_element_pick();
        }
        unsafe {
            let _: () =
                msg_send![self.view, setUserInteractionEnabled: if interactive { YES } else { NO }];
        }
    }

    pub fn start_element_pick(&self, appearance: &ElementPickerAppearance) -> Result<(), String> {
        self.cancel_element_pick();
        let token = self
            ._state
            .picker
            .begin()
            .ok_or("Could not start the element picker.")?;
        let start =
            element_picker_start_script(&token, appearance).map_err(|error| error.to_string())?;
        let script = format!(
            "globalThis.__zzElementPickerQuery = o => {{ window.webkit.messageHandlers.{PICKER_HANDLER}.postMessage(o.request); o.onSuccess?.(''); }};\n{PICKER_SCRIPT}\n{start}"
        );
        self._state.picking.set(true);
        let state = self._state.clone();
        let generation = state.generation.get();
        let completion = block2::RcBlock::new(
            move |_: *mut std::ffi::c_void, error: *mut std::ffi::c_void| {
                if !error.is_null() && state.generation.get() == generation {
                    state.picker.cancel();
                    state.picking.set(false);
                    let _ = state.events.unbounded_send(BrowserEvent::ElementPickFailed);
                }
            },
        );
        unsafe {
            let _: () = msg_send![self.view, evaluateJavaScript: ns_string(&script) completionHandler: &*completion];
        }
        Ok(())
    }

    pub fn cancel_element_pick(&self) {
        unsafe {
            cancel_picker(&self._state, self.view);
        }
    }

    pub fn mount(&self, bounds: Bounds<Pixels>, radius: Pixels, window: &Window) {
        let Ok(handle) = raw_window_handle::HasWindowHandle::window_handle(window) else {
            return;
        };
        let RawWindowHandle::UiKit(handle) = handle.as_raw() else {
            return;
        };
        unsafe {
            let parent = handle.ui_view.as_ptr() as id;
            let previous: id = msg_send![self.view, superview];
            if previous != parent {
                let _: () = msg_send![parent, addSubview: self.view];
            }
            let native_scale: f64 = msg_send![parent, contentScaleFactor];
            let scale = f64::from(window.scale_factor()) / native_scale;
            let frame = CGRect {
                origin: CGPoint {
                    x: f64::from(f32::from(bounds.origin.x)) * scale,
                    y: f64::from(f32::from(bounds.origin.y)) * scale,
                },
                size: CGSize {
                    width: f64::from(f32::from(bounds.size.width)) * scale,
                    height: f64::from(f32::from(bounds.size.height)) * scale,
                },
            };
            let _: () = msg_send![self.view, setFrame: frame];
            let layer: id = msg_send![self.view, layer];
            let _: () = msg_send![layer, setCornerRadius: f64::from(f32::from(radius)) * scale];
            let _: () = msg_send![layer, setMaskedCorners: 12usize];
            let _: () = msg_send![layer, setMasksToBounds: YES];
        }
    }
}

impl Drop for BrowserView {
    fn drop(&mut self) {
        self.cancel_element_pick();
        unsafe {
            self.stop();
            let _: () = msg_send![self.view, setNavigationDelegate: nil];
            let _: () = msg_send![self.view, setUIDelegate: nil];
            let configuration: id = msg_send![self.view, configuration];
            let controller: id = msg_send![configuration, userContentController];
            let _: () =
                msg_send![controller, removeScriptMessageHandlerForName: ns_string(PICKER_HANDLER)];
            for key in OBSERVED {
                let _: () =
                    msg_send![self.view, removeObserver: self.delegate forKeyPath: ns_string(key)];
            }
            let _: () = msg_send![self.view, removeFromSuperview];
            let _: () = msg_send![self.view, release];
            let _: () = msg_send![self.delegate, release];
        }
    }
}

unsafe fn state(this: &Object) -> Rc<DelegateState> {
    let pointer = *this.get_ivar::<usize>("state") as *const DelegateState;
    Rc::increment_strong_count(pointer);
    Rc::from_raw(pointer)
}

unsafe fn cancel_picker(state: &DelegateState, view: id) {
    if !state.picking.replace(false) {
        return;
    }
    state.generation.set(state.generation.get().wrapping_add(1));
    state.picker.cancel();
    let _: () = msg_send![view, evaluateJavaScript: ns_string("globalThis.__zzElementPicker?.cancel();") completionHandler: nil];
    let _ = state
        .events
        .unbounded_send(BrowserEvent::ElementPickCancelled);
}

extern "C" fn navigation_started(this: &Object, _: Sel, view: id, _: id) {
    unsafe {
        cancel_picker(&state(this), view);
    }
}

extern "C" fn picker_message(this: &Object, _: Sel, _: id, message: id) {
    unsafe {
        let frame: id = msg_send![message, frameInfo];
        let main: BOOL = msg_send![frame, isMainFrame];
        let body: id = msg_send![message, body];
        let is_string: BOOL = msg_send![body, isKindOfClass: class!(NSString)];
        if main != YES || is_string != YES {
            return;
        }
        let Some(request) = nsstring_to_string(body) else {
            return;
        };
        let state = state(this);
        let Ok(outcome) = state.picker.consume(&request) else {
            return;
        };
        match outcome {
            ElementPickOutcome::Picked(text, geometry) => {
                let view: id = msg_send![message, webView];
                let generation = state.generation.get();
                snapshot(view, geometry, move |screenshot| {
                    if state.generation.get() == generation {
                        state.picking.set(false);
                        let _ = state.events.unbounded_send(BrowserEvent::ElementPicked {
                            text: text.clone(),
                            screenshot,
                        });
                    }
                });
            }
            ElementPickOutcome::Cancelled | ElementPickOutcome::Failed => {
                state.picking.set(false);
                let event = if matches!(outcome, ElementPickOutcome::Cancelled) {
                    BrowserEvent::ElementPickCancelled
                } else {
                    BrowserEvent::ElementPickFailed
                };
                let _ = state.events.unbounded_send(event);
            }
        }
    }
}

unsafe fn snapshot(
    view: id,
    geometry: Option<PickGeometry>,
    complete: impl Fn(Option<Vec<u8>>) + 'static,
) {
    let Some(geometry) = geometry.filter(|_| !view.is_null()) else {
        complete(None);
        return;
    };
    let bounds: CGRect = msg_send![view, bounds];
    let (offset_x, offset_y, width, height) = geometry.visual_viewport.map_or(
        (0.0, 0.0, geometry.viewport_width, geometry.viewport_height),
        |viewport| {
            (
                viewport.offset_left,
                viewport.offset_top,
                viewport.width,
                viewport.height,
            )
        },
    );
    let scale_x = bounds.size.width / width;
    let scale_y = bounds.size.height / height;
    let x = ((geometry.x - offset_x) * scale_x).max(0.0);
    let y = ((geometry.y - offset_y) * scale_y).max(0.0);
    let right = ((geometry.x - offset_x + geometry.width) * scale_x).min(bounds.size.width);
    let bottom = ((geometry.y - offset_y + geometry.height) * scale_y).min(bounds.size.height);
    if right <= x || bottom <= y {
        complete(None);
        return;
    }
    let rect = CGRect {
        origin: CGPoint { x, y },
        size: CGSize {
            width: right - x,
            height: bottom - y,
        },
    };
    let configuration: id = msg_send![class!(WKSnapshotConfiguration), new];
    let _: () = msg_send![configuration, setRect: rect];
    let _: () = msg_send![configuration, setAfterScreenUpdates: YES];
    let completion = block2::RcBlock::new(
        move |image: *mut std::ffi::c_void, _: *mut std::ffi::c_void| {
            let image = image as id;
            let data = if image.is_null() {
                nil
            } else {
                UIImagePNGRepresentation(image)
            };
            let length: usize = if data.is_null() {
                0
            } else {
                msg_send![data, length]
            };
            let screenshot = if length > 0 && length <= 16 * 1024 * 1024 {
                let bytes: *const u8 = msg_send![data, bytes];
                (!bytes.is_null()).then(|| std::slice::from_raw_parts(bytes, length).to_vec())
            } else {
                None
            };
            complete(screenshot);
        },
    );
    let _: () = msg_send![view, takeSnapshotWithConfiguration: configuration completionHandler: &*completion];
    let _: () = msg_send![configuration, release];
}

unsafe fn changed(this: &Object, view: id, committed: bool) {
    let url: id = msg_send![view, URL];
    let address: id = msg_send![url, absoluteString];
    let title: id = msg_send![view, title];
    let loading: BOOL = msg_send![view, isLoading];
    let back: BOOL = msg_send![view, canGoBack];
    let forward: BOOL = msg_send![view, canGoForward];
    let _ = state(this).events.unbounded_send(BrowserEvent::Changed {
        url: nsstring_to_string(address),
        title: nsstring_to_string(title).unwrap_or_default(),
        loading: loading == YES,
        can_go_back: back == YES,
        can_go_forward: forward == YES,
        committed,
    });
}

fn delegate_class() -> &'static Class {
    static CLASS: std::sync::OnceLock<&Class> = std::sync::OnceLock::new();
    CLASS.get_or_init(|| unsafe {
        let mut decl = ClassDecl::new("ZZBrowserDelegate", class!(NSObject)).unwrap();
        decl.add_ivar::<usize>("state");
        for name in [
            "WKNavigationDelegate",
            "WKUIDelegate",
            "UIGestureRecognizerDelegate",
            "WKScriptMessageHandler",
        ] {
            if let Some(protocol) = Protocol::get(name) {
                decl.add_protocol(protocol);
            }
        }
        decl.add_method(
            sel!(userContentController:didReceiveScriptMessage:),
            picker_message as extern "C" fn(&Object, Sel, id, id),
        );
        decl.add_method(
            sel!(observeValueForKeyPath:ofObject:change:context:),
            observe as extern "C" fn(&Object, Sel, id, id, id, *mut std::ffi::c_void),
        );
        decl.add_method(
            sel!(webView:decidePolicyForNavigationAction:decisionHandler:),
            policy as extern "C" fn(&Object, Sel, id, id, id),
        );
        decl.add_method(
            sel!(webView:didFailProvisionalNavigation:withError:),
            failed as extern "C" fn(&Object, Sel, id, id, id),
        );
        decl.add_method(
            sel!(webView:didFailNavigation:withError:),
            failed as extern "C" fn(&Object, Sel, id, id, id),
        );
        decl.add_method(
            sel!(webView:didCommitNavigation:),
            committed as extern "C" fn(&Object, Sel, id, id),
        );
        decl.add_method(
            sel!(webView:didStartProvisionalNavigation:),
            navigation_started as extern "C" fn(&Object, Sel, id, id),
        );
        decl.add_method(
            sel!(webView:didFinishNavigation:),
            committed as extern "C" fn(&Object, Sel, id, id),
        );
        decl.add_method(
            sel!(webViewWebContentProcessDidTerminate:),
            terminated as extern "C" fn(&Object, Sel, id),
        );
        decl.add_method(
            sel!(webView:createWebViewWithConfiguration:forNavigationAction:windowFeatures:),
            open as extern "C" fn(&Object, Sel, id, id, id, id) -> id,
        );
        decl.add_method(sel!(pageTapped:), tapped as extern "C" fn(&Object, Sel, id));
        decl.add_method(
            sel!(gestureRecognizer:shouldRecognizeSimultaneouslyWithGestureRecognizer:),
            simultaneous as extern "C" fn(&Object, Sel, id, id) -> BOOL,
        );
        decl.register()
    })
}

extern "C" fn observe(this: &Object, _: Sel, _: id, view: id, _: id, _: *mut std::ffi::c_void) {
    unsafe {
        changed(this, view, false);
    }
}

extern "C" fn committed(this: &Object, _: Sel, view: id, _: id) {
    unsafe {
        changed(this, view, true);
    }
}

extern "C" fn policy(this: &Object, _: Sel, _: id, action: id, handler: id) {
    unsafe {
        let request: id = msg_send![action, request];
        let url: id = msg_send![request, URL];
        let result = state(this).profile.prepare(url);
        let allow = result.is_ok();
        if let Err(error) = result {
            let _ = state(this)
                .events
                .unbounded_send(BrowserEvent::Error(error));
        }
        let block = &*(handler as *const block2::Block<dyn Fn(isize)>);
        block.call((isize::from(allow),));
    }
}

extern "C" fn failed(this: &Object, _: Sel, _: id, _: id, error: id) {
    unsafe {
        let code: isize = msg_send![error, code];
        if code == -999 {
            return;
        }
        let text: id = msg_send![error, localizedDescription];
        if let Some(error) = nsstring_to_string(text) {
            let _ = state(this)
                .events
                .unbounded_send(BrowserEvent::Error(error));
        }
    }
}

extern "C" fn terminated(this: &Object, _: Sel, _: id) {
    unsafe {
        let state = state(this);
        state.generation.set(state.generation.get().wrapping_add(1));
        state.picker.cancel();
        state.picking.set(false);
        let _ = state
            .events
            .unbounded_send(BrowserEvent::ElementPickCancelled);
        let _ = state.events.unbounded_send(BrowserEvent::Error(
            "The page stopped. Reload to continue.".into(),
        ));
    }
}

extern "C" fn open(this: &Object, _: Sel, _: id, _: id, action: id, _: id) -> id {
    unsafe {
        let request: id = msg_send![action, request];
        let url: id = msg_send![request, URL];
        let address: id = msg_send![url, absoluteString];
        if let Some(url) = nsstring_to_string(address) {
            let _ = state(this).events.unbounded_send(BrowserEvent::Open(url));
        }
    }
    nil
}

extern "C" fn tapped(this: &Object, _: Sel, _: id) {
    unsafe {
        let _ = state(this).events.unbounded_send(BrowserEvent::Focused);
    }
}

extern "C" fn simultaneous(_: &Object, _: Sel, _: id, _: id) -> BOOL {
    YES
}

#[link(name = "WebKit", kind = "framework")]
unsafe extern "C" {}

#[link(name = "UIKit", kind = "framework")]
unsafe extern "C" {
    fn UIImagePNGRepresentation(image: id) -> id;
}

#[link(name = "Network", kind = "framework")]
unsafe extern "C" {
    fn nw_endpoint_create_host(host: *const std::ffi::c_char, port: *const std::ffi::c_char) -> id;
    fn nw_proxy_config_create_socksv5(endpoint: id) -> id;
    fn nw_proxy_config_set_failover_allowed(proxy: id, allowed: bool);
    fn nw_proxy_config_add_excluded_domain(proxy: id, domain: *const std::ffi::c_char);
}
