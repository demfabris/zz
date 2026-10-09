//! `globalThis.zpui`: the page-level handle agents and tests use to find,
//! drive and capture GPUI windows.
//!
//! ```js
//! await zpui.enableAccessibility();   // or ({ interactive: true })
//! zpui.windows();                        // [{ id, mount, title, width, height, active, accessibility }]
//! zpui.tree(1);                          // the window's accessibility tree
//! const [send] = zpui.find({ role: "button", name: "Send" });
//! await zpui.click(send.ref);            // an AccessKit Click on that node
//! const png = await zpui.capture(1);     // data URL of the window's pixels
//! await zpui.idle();                     // resolves once no window has a frame pending
//! ```
//!
//! Accessibility starts off, because building the tree costs every frame. A
//! page opts in at boot with `<html data-zpui-a11y>` or
//! `globalThis.zpuiAccessibility = true`, or later with
//! `zpui.enableAccessibility()`. Until then a visually hidden "Enable
//! accessibility" button lets a screen reader turn it on.
//!
//! Interactive mode (`<html data-zpui-a11y="interactive">` or
//! `enableAccessibility({ interactive: true })`) lets clickable nodes take
//! pointer input and forward it to the canvas, for tools that only act on
//! hit-testable elements.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use accesskit::{Action, NodeId};
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;

use crate::a11y::{CssRect, NodeSnapshot, role_name};
use crate::window::WebWindowInner;

const IDLE_TICK_MS: i32 = 16;
const IDLE_TICK_LIMIT: u32 = 300;

thread_local! {
    static WINDOWS: RefCell<Vec<Weak<WebWindowInner>>> = const { RefCell::new(Vec::new()) };
    static NEXT_ID: Cell<u32> = const { Cell::new(1) };
    static ENABLED: Cell<Option<bool>> = const { Cell::new(None) };
    static INTERACTIVE: Cell<bool> = const { Cell::new(false) };
    static INSTALLED: Cell<bool> = const { Cell::new(false) };
    static ENABLE_BUTTON: RefCell<Option<web_sys::HtmlElement>> = const { RefCell::new(None) };
}

pub(crate) fn next_window_id() -> u32 {
    NEXT_ID.with(|id| {
        let next = id.get();
        id.set(next + 1);
        next
    })
}

pub(crate) fn accessibility_enabled() -> bool {
    ENABLED.with(|enabled| {
        *enabled.get().get_or_insert_with(|| {
            let global = js_sys::global();
            let flag = js_sys::Reflect::get(&global, &"zpuiAccessibility".into())
                .is_ok_and(|value| value.is_truthy());
            let attribute = web_sys::window()
                .and_then(|window| window.document())
                .and_then(|document| document.document_element())
                .and_then(|root| root.get_attribute("data-zpui-a11y"));
            if attribute.as_deref() == Some("interactive") {
                INTERACTIVE.with(|interactive| interactive.set(true));
            }
            flag || attribute.is_some()
        })
    })
}

pub(crate) fn register(window: &Rc<WebWindowInner>) {
    WINDOWS.with(|windows| {
        let mut windows = windows.borrow_mut();
        windows.retain(|window| window.strong_count() > 0);
        windows.push(Rc::downgrade(window));
    });
    if !INSTALLED.replace(true) {
        install();
    }
    if !accessibility_enabled() {
        show_enable_button(&window.browser_window);
    }
}

pub(crate) fn interactive() -> bool {
    INTERACTIVE.with(Cell::get)
}

fn windows() -> Vec<Rc<WebWindowInner>> {
    WINDOWS.with(|windows| {
        let mut windows = windows.borrow_mut();
        windows.retain(|window| window.strong_count() > 0);
        windows.iter().filter_map(Weak::upgrade).collect()
    })
}

fn window(id: Option<u32>) -> Option<Rc<WebWindowInner>> {
    let windows = windows();
    match id {
        Some(id) => windows.into_iter().find(|window| window.id == id),
        None => windows.into_iter().next(),
    }
}

fn set_interactive(interactive: bool) {
    INTERACTIVE.with(|cell| cell.set(interactive));
    for window in windows() {
        window.a11y.set_interactive(interactive);
    }
}

fn enable_accessibility() {
    ENABLED.with(|enabled| enabled.set(Some(true)));
    ENABLE_BUTTON.with(|button| {
        if let Some(button) = button.borrow_mut().take() {
            button.remove();
        }
    });
    for window in windows() {
        window.a11y.activate();
    }
}

fn disable_accessibility() {
    ENABLED.with(|enabled| enabled.set(Some(false)));
    for window in windows() {
        window.a11y.deactivate();
    }
}

fn show_enable_button(browser_window: &web_sys::Window) {
    if ENABLE_BUTTON.with(|button| button.borrow().is_some()) {
        return;
    }
    let Some(document) = browser_window.document() else {
        return;
    };
    let Some(body) = document.body() else {
        return;
    };
    let Ok(button) = document.create_element("button") else {
        return;
    };
    let button: web_sys::HtmlElement = button.unchecked_into();
    button.set_text_content(Some("Enable accessibility"));
    button.set_attribute("data-zpui-enable-a11y", "").ok();
    let style = button.style();
    for (property, value) in [
        ("position", "fixed"),
        ("left", "0"),
        ("top", "0"),
        ("width", "1px"),
        ("height", "1px"),
        ("padding", "0"),
        ("margin", "-1px"),
        ("overflow", "hidden"),
        ("clip-path", "inset(50%)"),
        ("white-space", "nowrap"),
        ("border", "0"),
    ] {
        style.set_property(property, value).ok();
    }
    let on_click = Closure::<dyn FnMut()>::new(enable_accessibility);
    button.set_onclick(Some(on_click.as_ref().unchecked_ref()));
    on_click.forget();
    if body.prepend_with_node_1(&button).is_ok() {
        ENABLE_BUTTON.with(|slot| *slot.borrow_mut() = Some(button));
    }
}

fn install() {
    let api = js_sys::Object::new();
    let set = |name: &str, value: JsValue| {
        js_sys::Reflect::set(&api, &name.into(), &value).ok();
    };
    set("version", JsValue::from(1));
    set(
        "windows",
        Closure::<dyn Fn() -> JsValue>::new(|| {
            windows()
                .iter()
                .map(|window| JsValue::from(window_info(window)))
                .collect::<js_sys::Array>()
                .into()
        })
        .into_js_value(),
    );
    set(
        "enableAccessibility",
        Closure::<dyn Fn(JsValue) -> js_sys::Promise>::new(|options: JsValue| {
            if let Ok(interactive) = js_sys::Reflect::get(&options, &"interactive".into())
                && !interactive.is_undefined()
            {
                set_interactive(interactive.is_truthy());
            }
            enable_accessibility();
            idle()
        })
        .into_js_value(),
    );
    set(
        "disableAccessibility",
        Closure::<dyn Fn()>::new(disable_accessibility).into_js_value(),
    );
    set(
        "setInteractive",
        Closure::<dyn Fn(JsValue)>::new(|interactive: JsValue| {
            set_interactive(interactive.is_truthy());
        })
        .into_js_value(),
    );
    set(
        "tree",
        Closure::<dyn Fn(JsValue) -> JsValue>::new(|id: JsValue| {
            let Some(window) = window(window_id(&id)) else {
                return JsValue::NULL;
            };
            window
                .a11y
                .snapshot(client_origin(&window))
                .map_or(JsValue::NULL, |root| {
                    node_object(window.id, &root, true).into()
                })
        })
        .into_js_value(),
    );
    set(
        "find",
        Closure::<dyn Fn(JsValue, JsValue) -> JsValue>::new(|query: JsValue, id: JsValue| {
            let query = Query::from_js(&query);
            let only = window_id(&id);
            let mut found = Vec::new();
            for window in windows() {
                if only.is_some_and(|only| only != window.id) {
                    continue;
                }
                if let Some(root) = window.a11y.snapshot(client_origin(&window)) {
                    collect(window.id, &root, &query, &mut found);
                }
            }
            found.into_iter().collect::<js_sys::Array>().into()
        })
        .into_js_value(),
    );
    set(
        "click",
        Closure::<dyn Fn(JsValue) -> js_sys::Promise>::new(|reference: JsValue| {
            act(&reference, Action::Click)
        })
        .into_js_value(),
    );
    set(
        "focus",
        Closure::<dyn Fn(JsValue) -> js_sys::Promise>::new(|reference: JsValue| {
            act(&reference, Action::Focus)
        })
        .into_js_value(),
    );
    set(
        "capture",
        Closure::<dyn Fn(JsValue) -> js_sys::Promise>::new(|id: JsValue| {
            let Some(window) = window(window_id(&id)) else {
                return js_sys::Promise::reject(&"no such window".into());
            };
            window.render_now();
            match window.canvas.to_data_url() {
                Ok(url) => js_sys::Promise::resolve(&JsValue::from(url)),
                Err(error) => js_sys::Promise::reject(&error),
            }
        })
        .into_js_value(),
    );
    set(
        "idle",
        Closure::<dyn Fn() -> js_sys::Promise>::new(idle).into_js_value(),
    );
    js_sys::Reflect::set(&js_sys::global(), &"zpui".into(), &api).ok();
}

fn act(reference: &JsValue, action: Action) -> js_sys::Promise {
    let Some((window_id, node)) = reference
        .as_string()
        .and_then(|reference| parse_ref(&reference))
    else {
        return js_sys::Promise::reject(&"expected a node ref like \"1:42\"".into());
    };
    let Some(window) = window(Some(window_id)) else {
        return js_sys::Promise::reject(&"no such window".into());
    };
    if !window.a11y.request(node, action) {
        return js_sys::Promise::reject(&"no such node; is accessibility enabled?".into());
    }
    idle()
}

fn idle() -> js_sys::Promise {
    js_sys::Promise::new(&mut |resolve, _reject| {
        let Some(browser_window) = web_sys::window() else {
            resolve.call0(&JsValue::NULL).ok();
            return;
        };
        let ticks = Rc::new(Cell::new(0u32));
        let quiet = Rc::new(Cell::new(0u32));
        let tick: Rc<RefCell<Option<Closure<dyn FnMut()>>>> = Rc::new(RefCell::new(None));
        let schedule = tick.clone();
        let timer_window = browser_window.clone();
        *tick.borrow_mut() = Some(Closure::new(move || {
            ticks.set(ticks.get() + 1);
            let hidden = timer_window
                .document()
                .is_some_and(|document| document.hidden());
            let windows = windows();
            if hidden {
                for window in &windows {
                    if window.has_pending_frame() {
                        window.render_now();
                    }
                }
            }
            let busy = windows.iter().any(|window| window.has_pending_frame());
            quiet.set(if busy { 0 } else { quiet.get() + 1 });
            if quiet.get() >= 2 || ticks.get() >= IDLE_TICK_LIMIT {
                schedule.borrow_mut().take();
                resolve.call0(&JsValue::NULL).ok();
                return;
            }
            if let Some(callback) = schedule.borrow().as_ref() {
                timer_window
                    .set_timeout_with_callback_and_timeout_and_arguments_0(
                        callback.as_ref().unchecked_ref(),
                        IDLE_TICK_MS,
                    )
                    .ok();
            }
        }));
        if let Some(callback) = tick.borrow().as_ref() {
            browser_window
                .set_timeout_with_callback_and_timeout_and_arguments_0(
                    callback.as_ref().unchecked_ref(),
                    IDLE_TICK_MS,
                )
                .ok();
        }
    })
}

fn window_id(value: &JsValue) -> Option<u32> {
    value.as_f64().map(|id| id as u32)
}

fn parse_ref(reference: &str) -> Option<(u32, NodeId)> {
    let (window, node) = reference.split_once(':')?;
    Some((window.parse().ok()?, NodeId(node.parse().ok()?)))
}

fn client_origin(window: &WebWindowInner) -> (f64, f64) {
    let rect = window.canvas.get_bounding_client_rect();
    (rect.left(), rect.top())
}

fn window_info(window: &WebWindowInner) -> js_sys::Object {
    let info = js_sys::Object::new();
    let state = window.state.borrow();
    let set = |key: &str, value: JsValue| {
        js_sys::Reflect::set(&info, &key.into(), &value).ok();
    };
    set("id", window.id.into());
    set(
        "mount",
        window
            .mount_selector
            .as_deref()
            .map_or(JsValue::NULL, JsValue::from),
    );
    set("title", state.title.as_str().into());
    set(
        "width",
        f64::from(f32::from(state.bounds.size.width)).into(),
    );
    set(
        "height",
        f64::from(f32::from(state.bounds.size.height)).into(),
    );
    set("active", state.is_active.into());
    set("accessibility", window.a11y.is_active().into());
    info
}

fn node_object(window: u32, node: &NodeSnapshot, with_children: bool) -> js_sys::Object {
    let object = js_sys::Object::new();
    let set = |key: &str, value: JsValue| {
        js_sys::Reflect::set(&object, &key.into(), &value).ok();
    };
    set("ref", format!("{window}:{}", node.id.0).into());
    set("window", window.into());
    set("role", role_name(node.role).into());
    set("name", optional(node.name.as_deref()));
    set("value", optional(node.value.as_deref()));
    set("text", optional(node.text.as_deref()));
    set("id", optional(node.author_id.as_deref()));
    set(
        "bounds",
        node.bounds
            .map_or(JsValue::NULL, |rect| rect_object(rect).into()),
    );
    set("focused", node.focused.into());
    let states = js_sys::Object::new();
    for (key, value) in &node.states {
        js_sys::Reflect::set(&states, &(*key).into(), &value.as_str().into()).ok();
    }
    set("states", states.into());
    if with_children {
        set(
            "children",
            node.children
                .iter()
                .map(|child| JsValue::from(node_object(window, child, true)))
                .collect::<js_sys::Array>()
                .into(),
        );
    }
    object
}

fn rect_object(rect: CssRect) -> js_sys::Object {
    let object = js_sys::Object::new();
    for (key, value) in [
        ("x", rect.x),
        ("y", rect.y),
        ("width", rect.width),
        ("height", rect.height),
    ] {
        js_sys::Reflect::set(&object, &key.into(), &value.into()).ok();
    }
    object
}

fn optional(value: Option<&str>) -> JsValue {
    value.map_or(JsValue::NULL, JsValue::from)
}

#[derive(Default)]
struct Query {
    role: Option<String>,
    name: Option<String>,
    text: Option<String>,
    id: Option<String>,
}

impl Query {
    fn from_js(value: &JsValue) -> Self {
        if let Some(text) = value.as_string() {
            return Self {
                text: Some(text.to_lowercase()),
                ..Self::default()
            };
        }
        let field = |key: &str| {
            js_sys::Reflect::get(value, &key.into())
                .ok()
                .and_then(|value| value.as_string())
        };
        Self {
            role: field("role"),
            name: field("name"),
            text: field("text").map(|text| text.to_lowercase()),
            id: field("id"),
        }
    }

    fn matches(&self, node: &NodeSnapshot) -> bool {
        self.role
            .as_deref()
            .is_none_or(|role| role == role_name(node.role))
            && self
                .name
                .as_deref()
                .is_none_or(|name| node.name.as_deref() == Some(name))
            && self
                .id
                .as_deref()
                .is_none_or(|id| node.author_id.as_deref() == Some(id))
            && self.text.as_deref().is_none_or(|text| {
                [
                    node.name.as_deref(),
                    node.value.as_deref(),
                    node.text.as_deref(),
                ]
                .into_iter()
                .flatten()
                .any(|field| field.to_lowercase().contains(text))
            })
    }
}

fn collect(
    window: u32,
    node: &NodeSnapshot,
    query: &Query,
    found: &mut Vec<js_sys::Object>,
) -> bool {
    let mut below = Vec::new();
    let mut descendant_matched = false;
    for child in &node.children {
        descendant_matched |= collect(window, child, query, &mut below);
    }
    let matched = query.matches(node) && !(query.text.is_some() && descendant_matched);
    if matched {
        found.push(node_object(window, node, false));
    }
    found.extend(below);
    matched || descendant_matched
}
