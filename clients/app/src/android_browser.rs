//! The browser pane's native half on Android: one `android.webkit.WebView` per
//! tab, hosted in `ZzBrowser` over the GPUI surface. It mirrors the iOS bridge's
//! API so `browser_pane` drives both the same way.

use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    ffi::c_void,
    rc::{Rc, Weak},
    sync::{Arc, Once},
};

use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use zz_daemon_client::InteractiveClient;
use zz_gpui::{Bounds, Pixels, Point, RenderImage, Window};
use zz_gpui_platform::android::{
    jni::{
        JNIEnv, NativeMethod,
        objects::{GlobalRef, JByteArray, JClass, JString, JValue},
        sys::{JNI_FALSE, JNI_TRUE, jboolean, jfloat, jint, jlong},
    },
    load_class, with_activity,
};

pub use zz_client::element_picker::ElementPickerAppearance;
use zz_client::element_picker::{
    ELEMENT_PICKER_SCRIPT as PICKER_SCRIPT, ElementPickOutcome, ElementPickState, PickGeometry,
    element_picker_start_script,
};

const CLASS: &str = "dev/zz/android/ZzBrowser";
const SNAPSHOT_PNG: jint = 0;
const SNAPSHOT_PIXELS: jint = 1;
const MAX_SCREENSHOT: usize = 16 * 1024 * 1024;

/// Where the web view sits and what floats over it: its frame, its lower
/// corners, how much of its bottom edge app UI covers, and the cutout that
/// UI shows through, all in window pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BrowserMount {
    pub bounds: Bounds<Pixels>,
    pub radius: Pixels,
    pub covered_bottom: Pixels,
    pub hole: Option<(Bounds<Pixels>, Pixels)>,
    pub backdrop: Option<Bounds<Pixels>>,
}

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
    Scrolled {
        travel: f32,
        at_top: bool,
    },
    Refresh,
    Backdrop {
        bounds: Bounds<Pixels>,
        image: Arc<RenderImage>,
    },
}

pub struct BrowserProfile {
    name: String,
    ssh: bool,
    client: RefCell<Option<Arc<InteractiveClient>>>,
    #[allow(clippy::option_option)]
    route: RefCell<Option<Option<u16>>>,
}

impl BrowserProfile {
    #[must_use]
    pub fn new(host: &str, profile: &str) -> Rc<Self> {
        let name = format!("zz-{host}-{profile}")
            .chars()
            .map(|character| {
                if character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.') {
                    character
                } else {
                    '_'
                }
            })
            .collect();
        let profile = Rc::new(Self {
            name,
            ssh: host.starts_with("ssh://"),
            client: RefCell::new(None),
            route: RefCell::new(None),
        });
        profile.set_client(None);
        profile
    }

    /// Takes the host connection the pages route through, returning whether the
    /// route changed. On an ssh host every page goes through the host's SOCKS
    /// forward, except `localhost` addresses, which reach the host's loopback
    /// through port forwards. The route is the whole app's, like the connection.
    pub fn set_client(&self, client: Option<Arc<InteractiveClient>>) -> bool {
        let route = client.as_ref().and_then(|client| {
            if self.ssh {
                client.socks_port().map(Some)
            } else {
                Some(None)
            }
        });
        *self.client.borrow_mut() = client;
        if self.route.replace(route) == route {
            return false;
        }
        if let Some(port) = route {
            let port = port.map_or(-1, jint::from);
            with_activity(|env, activity| {
                let class = load_class(env, activity, CLASS)?;
                env.call_static_method(class, "route", "(I)V", &[JValue::Int(port)])
                    .map(drop)
            });
        }
        true
    }

    pub fn available(&self) -> bool {
        self.route.borrow().is_some()
    }

    fn prepare(&self, address: &str) -> Result<(), String> {
        if !self.available() {
            return Err("Waiting for the host connection…".into());
        }
        let url = url::Url::parse(address).map_err(|_| "Enter an HTTP or HTTPS address.")?;
        let scheme = url.scheme().to_ascii_lowercase();
        if !matches!(
            scheme.as_str(),
            "http" | "https" | "about" | "blob" | "data"
        ) {
            return Err("This address cannot open in the browser pane.".into());
        }
        if !self.ssh || !matches!(scheme.as_str(), "http" | "https") {
            return Ok(());
        }
        let host = url.host_str().unwrap_or_default().to_ascii_lowercase();
        if matches!(
            host.as_str(),
            "localhost" | "localhost." | "127.0.0.1" | "::1" | "[::1]"
        ) {
            let port = url
                .port_or_known_default()
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

struct Shared {
    profile: Rc<BrowserProfile>,
    events: UnboundedSender<BrowserEvent>,
    java: Option<GlobalRef>,
    picker: ElementPickState,
    generation: Cell<u64>,
    picking: Cell<bool>,
    picked: RefCell<Option<(Arc<str>, u64)>>,
    scale: Cell<f32>,
    size: Cell<(f64, f64)>,
    backdrop: Cell<Option<(Bounds<Pixels>, [i32; 4])>>,
    visible: Cell<bool>,
    snapshotting: Cell<bool>,
    stale: Cell<bool>,
}

impl Shared {
    fn send(&self, event: BrowserEvent) {
        let _ = self.events.unbounded_send(event);
    }

    fn call(&self, name: &str, sig: &str, args: &[JValue]) {
        let Some(java) = &self.java else {
            return;
        };
        with_activity(|env, _| env.call_method(java.as_obj(), name, sig, args).map(drop));
    }

    fn evaluate(&self, script: &str) {
        let Some(java) = &self.java else {
            return;
        };
        with_activity(|env, _| {
            let script = env.new_string(script)?;
            env.call_method(
                java.as_obj(),
                "evaluate",
                "(Ljava/lang/String;)V",
                &[JValue::Object(&script)],
            )
            .map(drop)
        });
    }

    fn snapshot(&self, kind: jint, [x, y, width, height]: [i32; 4]) {
        self.call(
            "snapshot",
            "(IIIII)V",
            &[
                JValue::Int(kind),
                JValue::Int(x),
                JValue::Int(y),
                JValue::Int(width),
                JValue::Int(height),
            ],
        );
    }

    fn request_backdrop(&self) {
        let Some((_, rect)) = self.backdrop.get() else {
            return;
        };
        if !self.visible.get() || rect[2] < 1 || rect[3] < 1 {
            return;
        }
        if self.snapshotting.replace(true) {
            self.stale.set(true);
            return;
        }
        self.stale.set(false);
        self.snapshot(SNAPSHOT_PIXELS, rect);
    }

    fn cancel_pick(&self) {
        if !self.picking.replace(false) {
            return;
        }
        self.generation.set(self.generation.get().wrapping_add(1));
        self.picker.cancel();
        self.picked.replace(None);
        self.evaluate("globalThis.__zzElementPicker?.cancel();");
        self.send(BrowserEvent::ElementPickCancelled);
    }
}

thread_local! {
    static VIEWS: RefCell<HashMap<jlong, Weak<Shared>>> = RefCell::new(HashMap::new());
    static NEXT_ID: Cell<jlong> = const { Cell::new(1) };
}

fn shared(id: jlong) -> Option<Rc<Shared>> {
    VIEWS.with_borrow(|views| views.get(&id).and_then(Weak::upgrade))
}

pub struct BrowserView {
    id: jlong,
    shared: Rc<Shared>,
    mounted: Cell<Option<BrowserMount>>,
}

impl BrowserView {
    pub fn new(profile: Rc<BrowserProfile>) -> (Rc<Self>, UnboundedReceiver<BrowserEvent>) {
        register_natives();
        let (events, receiver) = unbounded();
        let id = NEXT_ID.get();
        NEXT_ID.set(id + 1);
        let java = with_activity(|env, activity| {
            let class = load_class(env, activity, CLASS)?;
            let name = env.new_string(&profile.name)?;
            let browser = env
                .call_static_method(
                    class,
                    "create",
                    "(Ldev/zz/android/ZzActivity;JLjava/lang/String;)Ldev/zz/android/ZzBrowser;",
                    &[
                        JValue::Object(activity),
                        JValue::Long(id),
                        JValue::Object(&name),
                    ],
                )?
                .l()?;
            env.new_global_ref(browser)
        });
        if java.is_none() {
            log::error!("could not create the Android web view");
        }
        let shared = Rc::new(Shared {
            profile,
            events,
            java,
            picker: ElementPickState::default(),
            generation: Cell::new(0),
            picking: Cell::new(false),
            picked: RefCell::new(None),
            scale: Cell::new(1.0),
            size: Cell::new((0.0, 0.0)),
            backdrop: Cell::new(None),
            visible: Cell::new(false),
            snapshotting: Cell::new(false),
            stale: Cell::new(false),
        });
        VIEWS.with_borrow_mut(|views| views.insert(id, Rc::downgrade(&shared)));
        (
            Rc::new(Self {
                id,
                shared,
                mounted: Cell::new(None),
            }),
            receiver,
        )
    }

    pub fn load(&self, address: &str) -> Result<(), String> {
        self.cancel_element_pick();
        if address.contains('\0') {
            return Err("The address contains an invalid character.".into());
        }
        self.shared.profile.prepare(address)?;
        let Some(java) = &self.shared.java else {
            return Err("The web view is unavailable.".into());
        };
        with_activity(|env, _| {
            let address = env.new_string(address)?;
            env.call_method(
                java.as_obj(),
                "load",
                "(Ljava/lang/String;)V",
                &[JValue::Object(&address)],
            )
            .map(drop)
        });
        Ok(())
    }

    pub fn back(&self) {
        self.cancel_element_pick();
        self.shared.call("back", "()V", &[]);
    }

    pub fn forward(&self) {
        self.cancel_element_pick();
        self.shared.call("forward", "()V", &[]);
    }

    pub fn reload(&self) {
        self.cancel_element_pick();
        self.shared.call("reload", "()V", &[]);
    }

    pub fn stop(&self) {
        self.cancel_element_pick();
        self.shared.call("stop", "()V", &[]);
    }

    pub fn set_visible(&self, visible: bool) {
        if self.shared.visible.replace(visible) == visible {
            return;
        }
        if !visible {
            self.cancel_element_pick();
        }
        self.shared
            .call("setVisible", "(Z)V", &[JValue::Bool(u8::from(visible))]);
        if visible {
            self.shared.request_backdrop();
        }
    }

    pub fn set_interactive(&self, interactive: bool) {
        if !interactive {
            self.cancel_element_pick();
        }
        self.shared.call(
            "setInteractive",
            "(Z)V",
            &[JValue::Bool(u8::from(interactive))],
        );
    }

    pub fn start_element_pick(&self, appearance: &ElementPickerAppearance) -> Result<(), String> {
        self.cancel_element_pick();
        let token = self
            .shared
            .picker
            .begin()
            .ok_or("Could not start the element picker.")?;
        let start =
            element_picker_start_script(&token, appearance).map_err(|error| error.to_string())?;
        let script = format!(
            "globalThis.__zzElementPickerQuery = o => {{ window.zzElementPicker.postMessage(o.request); o.onSuccess?.(''); }};\n{PICKER_SCRIPT}\n{start}"
        );
        self.shared.picking.set(true);
        self.shared.evaluate(&script);
        Ok(())
    }

    pub fn cancel_element_pick(&self) {
        self.shared.cancel_pick();
    }

    pub fn point_element_pick(&self, at: Point<Pixels>) -> bool {
        let Some(mount) = self.mounted.get().filter(|_| self.shared.picking.get()) else {
            return false;
        };
        let local = at - mount.bounds.origin;
        self.shared.evaluate(&format!(
            "globalThis.__zzElementPicker?.point({},{},{});",
            f32::from(local.x),
            f32::from(local.y),
            f32::from(mount.bounds.size.width)
        ));
        mount.bounds.contains(&at)
    }

    pub fn release_element_pick(&self) {
        if self.shared.picking.get() {
            self.shared
                .evaluate("globalThis.__zzElementPicker?.release();");
        }
    }

    pub fn refresh_backdrop(&self) {
        self.shared.request_backdrop();
    }

    pub fn end_refresh(&self) {
        self.shared.call("endRefresh", "()V", &[]);
    }

    pub fn mount(&self, mount: BrowserMount, window: &Window) {
        if self.mounted.get() == Some(mount) {
            return;
        }
        self.mounted.set(Some(mount));
        let scale = window.scale_factor();
        self.shared.scale.set(scale);
        let device = |value: Pixels| (f32::from(value) * scale).round() as i32;
        let bounds = mount.bounds;
        let (width, height) = (device(bounds.size.width), device(bounds.size.height));
        self.shared.size.set((f64::from(width), f64::from(height)));
        let (hole, hole_radius) = mount.hole.map_or(([0; 4], 0.0), |(hole, radius)| {
            (
                [
                    device(hole.origin.x - bounds.origin.x),
                    device(hole.origin.y - bounds.origin.y),
                    device(hole.size.width),
                    device(hole.size.height),
                ],
                f32::from(radius) * scale,
            )
        });
        self.shared.call(
            "mount",
            "(IIIIFIIIIF)V",
            &[
                JValue::Int(device(bounds.origin.x)),
                JValue::Int(device(bounds.origin.y)),
                JValue::Int(width),
                JValue::Int(height),
                JValue::Float(f32::from(mount.radius) * scale),
                JValue::Int(hole[0]),
                JValue::Int(hole[1]),
                JValue::Int(hole[2]),
                JValue::Int(hole[3]),
                JValue::Float(hole_radius),
            ],
        );
        self.shared.backdrop.set(mount.backdrop.map(|strip| {
            (
                strip,
                [
                    device(strip.origin.x - bounds.origin.x),
                    device(strip.origin.y - bounds.origin.y),
                    device(strip.size.width),
                    device(strip.size.height),
                ],
            )
        }));
        self.shared.request_backdrop();
    }
}

impl Drop for BrowserView {
    fn drop(&mut self) {
        self.cancel_element_pick();
        self.shared.call("destroy", "()V", &[]);
        VIEWS.with_borrow_mut(|views| views.remove(&self.id));
    }
}

fn pick_rect(geometry: &PickGeometry, (width, height): (f64, f64)) -> Option<[i32; 4]> {
    let (offset_x, offset_y, view_width, view_height) = geometry.visual_viewport.map_or(
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
    if view_width <= 0.0 || view_height <= 0.0 {
        return None;
    }
    let scale_x = width / view_width;
    let scale_y = height / view_height;
    let x = ((geometry.x - offset_x) * scale_x).max(0.0);
    let y = ((geometry.y - offset_y) * scale_y).max(0.0);
    let right = ((geometry.x - offset_x + geometry.width) * scale_x).min(width);
    let bottom = ((geometry.y - offset_y + geometry.height) * scale_y).min(height);
    (right > x && bottom > y).then(|| {
        [
            x.round() as i32,
            y.round() as i32,
            (right - x).round() as i32,
            (bottom - y).round() as i32,
        ]
    })
}

fn register_natives() {
    static REGISTERED: Once = Once::new();
    REGISTERED.call_once(|| {
        let methods = [
            (
                "nativeAllow",
                "(JLjava/lang/String;)Z",
                native_allow as *mut c_void,
            ),
            (
                "nativeChanged",
                "(JLjava/lang/String;Ljava/lang/String;ZZZZ)V",
                native_changed as *mut c_void,
            ),
            (
                "nativeError",
                "(JLjava/lang/String;)V",
                native_error as *mut c_void,
            ),
            (
                "nativeOpen",
                "(JLjava/lang/String;)V",
                native_open as *mut c_void,
            ),
            ("nativeFocused", "(J)V", native_focused as *mut c_void),
            (
                "nativeMessage",
                "(JLjava/lang/String;)V",
                native_message as *mut c_void,
            ),
            ("nativeScrolled", "(JFZ)V", native_scrolled as *mut c_void),
            ("nativeRefresh", "(J)V", native_refresh as *mut c_void),
            ("nativeTerminated", "(J)V", native_terminated as *mut c_void),
            (
                "nativeSnapshot",
                "(JI[BII)V",
                native_snapshot as *mut c_void,
            ),
        ]
        .map(|(name, sig, fn_ptr)| NativeMethod {
            name: name.into(),
            sig: sig.into(),
            fn_ptr,
        });
        let registered = with_activity(|env, activity| {
            let class = load_class(env, activity, CLASS)?;
            env.register_native_methods(class, &methods)
        });
        if registered.is_none() {
            log::error!("registering the browser natives failed");
        }
    });
}

fn string(env: &mut JNIEnv, value: &JString) -> Option<String> {
    if value.is_null() {
        return None;
    }
    env.get_string(value).ok().map(Into::into)
}

extern "system" fn native_allow<'local>(
    mut env: JNIEnv<'local>,
    _: JClass<'local>,
    id: jlong,
    url: JString<'local>,
) -> jboolean {
    let (Some(shared), Some(url)) = (shared(id), string(&mut env, &url)) else {
        return JNI_FALSE;
    };
    match shared.profile.prepare(&url) {
        Ok(()) => {
            shared.cancel_pick();
            JNI_TRUE
        }
        Err(error) => {
            shared.send(BrowserEvent::Error(error));
            JNI_FALSE
        }
    }
}

extern "system" fn native_changed<'local>(
    mut env: JNIEnv<'local>,
    _: JClass<'local>,
    id: jlong,
    url: JString<'local>,
    title: JString<'local>,
    loading: jboolean,
    back: jboolean,
    forward: jboolean,
    committed: jboolean,
) {
    let Some(shared) = shared(id) else {
        return;
    };
    let loading = loading == JNI_TRUE;
    if !loading {
        shared.request_backdrop();
    }
    shared.send(BrowserEvent::Changed {
        url: string(&mut env, &url),
        title: string(&mut env, &title).unwrap_or_default(),
        loading,
        can_go_back: back == JNI_TRUE,
        can_go_forward: forward == JNI_TRUE,
        committed: committed == JNI_TRUE,
    });
}

extern "system" fn native_error<'local>(
    mut env: JNIEnv<'local>,
    _: JClass<'local>,
    id: jlong,
    message: JString<'local>,
) {
    if let (Some(shared), Some(message)) = (shared(id), string(&mut env, &message)) {
        shared.send(BrowserEvent::Error(message));
    }
}

extern "system" fn native_open<'local>(
    mut env: JNIEnv<'local>,
    _: JClass<'local>,
    id: jlong,
    url: JString<'local>,
) {
    if let (Some(shared), Some(url)) = (shared(id), string(&mut env, &url)) {
        shared.send(BrowserEvent::Open(url));
    }
}

extern "system" fn native_focused<'local>(_: JNIEnv<'local>, _: JClass<'local>, id: jlong) {
    if let Some(shared) = shared(id) {
        shared.send(BrowserEvent::Focused);
    }
}

extern "system" fn native_message<'local>(
    mut env: JNIEnv<'local>,
    _: JClass<'local>,
    id: jlong,
    message: JString<'local>,
) {
    let (Some(shared), Some(message)) = (shared(id), string(&mut env, &message)) else {
        return;
    };
    let Ok(outcome) = shared.picker.consume(&message) else {
        return;
    };
    match outcome {
        ElementPickOutcome::Picked(text, geometry) => {
            let rect = geometry
                .as_ref()
                .and_then(|geometry| pick_rect(geometry, shared.size.get()));
            if let Some(rect) = rect {
                shared.picked.replace(Some((text, shared.generation.get())));
                shared.snapshot(SNAPSHOT_PNG, rect);
            } else {
                shared.picking.set(false);
                shared.send(BrowserEvent::ElementPicked {
                    text,
                    screenshot: None,
                });
            }
        }
        ElementPickOutcome::Cancelled => {
            shared.picking.set(false);
            shared.send(BrowserEvent::ElementPickCancelled);
        }
        ElementPickOutcome::Failed => {
            shared.picking.set(false);
            shared.send(BrowserEvent::ElementPickFailed);
        }
    }
}

extern "system" fn native_scrolled<'local>(
    _: JNIEnv<'local>,
    _: JClass<'local>,
    id: jlong,
    travel: jfloat,
    at_top: jboolean,
) {
    if let Some(shared) = shared(id) {
        shared.request_backdrop();
        shared.send(BrowserEvent::Scrolled {
            travel,
            at_top: at_top == JNI_TRUE,
        });
    }
}

extern "system" fn native_refresh<'local>(_: JNIEnv<'local>, _: JClass<'local>, id: jlong) {
    if let Some(shared) = shared(id) {
        shared.send(BrowserEvent::Refresh);
    }
}

extern "system" fn native_terminated<'local>(_: JNIEnv<'local>, _: JClass<'local>, id: jlong) {
    let Some(shared) = shared(id) else {
        return;
    };
    shared
        .generation
        .set(shared.generation.get().wrapping_add(1));
    shared.picker.cancel();
    shared.picking.set(false);
    shared.picked.replace(None);
    shared.send(BrowserEvent::ElementPickCancelled);
    shared.send(BrowserEvent::Error(
        "The page stopped. Reload to continue.".into(),
    ));
}

extern "system" fn native_snapshot<'local>(
    env: JNIEnv<'local>,
    _: JClass<'local>,
    id: jlong,
    kind: jint,
    data: JByteArray<'local>,
    width: jint,
    height: jint,
) {
    let Some(shared) = shared(id) else {
        return;
    };
    let bytes = (!data.is_null())
        .then(|| env.convert_byte_array(&data).ok())
        .flatten()
        .filter(|bytes| !bytes.is_empty());
    if kind == SNAPSHOT_PNG {
        let Some((text, generation)) = shared.picked.take() else {
            return;
        };
        if generation != shared.generation.get() {
            return;
        }
        shared.picking.set(false);
        shared.send(BrowserEvent::ElementPicked {
            text,
            screenshot: bytes.filter(|bytes| bytes.len() <= MAX_SCREENSHOT),
        });
        return;
    }
    shared.snapshotting.set(false);
    if let (Some(mut pixels), Some((bounds, _))) = (bytes, shared.backdrop.get())
        && width > 0
        && height > 0
    {
        for pixel in pixels.chunks_exact_mut(4) {
            pixel.swap(0, 2);
        }
        if let Some(buffer) = image::RgbaImage::from_raw(width as u32, height as u32, pixels) {
            shared.send(BrowserEvent::Backdrop {
                bounds,
                image: Arc::new(RenderImage::new(vec![image::Frame::new(buffer)])),
            });
        }
    }
    if shared.stale.get() {
        shared.request_backdrop();
    }
}
