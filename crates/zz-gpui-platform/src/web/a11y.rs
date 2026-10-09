//! The window's accessibility tree, mirrored as transparent page elements laid
//! over its canvas.
//!
//! Each AccessKit node becomes a `div` with the matching ARIA role, name,
//! value and states, positioned over the pixels it describes. The layer never
//! takes pointer input, so real clicks still land on the canvas, while
//! assistive technology and page automation see a regular DOM tree.
//! `element.click()` on a mirrored node becomes an AccessKit `Click` action.
//!
//! In interactive mode, nodes that can be clicked or focused take pointer
//! input themselves, so tools that only target hit-testable elements (such as
//! Playwright's agent snapshots) can reach them. Their pointer and wheel
//! events are re-dispatched to the canvas, which keeps hover, drag and
//! scrolling identical to a click on the canvas itself.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use accesskit::{Action, ActionRequest, Node, NodeId, Role, Toggled, TreeId, TreeUpdate};
use wasm_bindgen::{JsCast, JsValue};
use zz_gpui::A11yCallbacks;

use crate::web::events::EventListenerHandle;

pub(crate) const NODE_ATTRIBUTE: &str = "data-zz-gpui-node";
const AUTHOR_ATTRIBUTE: &str = "data-zz-gpui-id";
const FOCUS_ATTRIBUTE: &str = "data-zz-gpui-focused";
const TEXT_LIMIT: usize = 512;
const FORWARDED: [&str; 7] = [
    "pointerdown",
    "pointermove",
    "pointerup",
    "pointercancel",
    "contextmenu",
    "dblclick",
    "wheel",
];

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct CssRect {
    pub(crate) x: f64,
    pub(crate) y: f64,
    pub(crate) width: f64,
    pub(crate) height: f64,
}

struct Mirrored {
    node: Node,
    element: web_sys::HtmlElement,
    placed: Option<CssRect>,
    origin: CssRect,
}

pub(crate) struct WebA11y {
    document: web_sys::Document,
    layer: web_sys::HtmlElement,
    nodes: RefCell<HashMap<NodeId, Mirrored>>,
    root: Cell<Option<NodeId>>,
    focus: Cell<Option<NodeId>>,
    callbacks: RefCell<Option<Rc<A11yCallbacks>>>,
    active: Cell<bool>,
    interactive: Cell<bool>,
    pressed: Cell<bool>,
    scale: Cell<f64>,
    _listeners: RefCell<Vec<EventListenerHandle>>,
}

pub(crate) struct NodeSnapshot {
    pub(crate) id: NodeId,
    pub(crate) role: Role,
    pub(crate) name: Option<String>,
    pub(crate) value: Option<String>,
    pub(crate) text: Option<String>,
    pub(crate) author_id: Option<String>,
    pub(crate) bounds: Option<CssRect>,
    pub(crate) focused: bool,
    pub(crate) states: Vec<(&'static str, String)>,
    pub(crate) children: Vec<NodeSnapshot>,
}

impl WebA11y {
    pub(crate) fn new(
        document: &web_sys::Document,
        canvas: &web_sys::HtmlCanvasElement,
        window_id: u32,
    ) -> anyhow::Result<Rc<Self>> {
        let layer: web_sys::HtmlElement = document
            .create_element("div")
            .map_err(|error| anyhow::anyhow!("creating the accessibility layer: {error:?}"))?
            .unchecked_into();
        layer.set_attribute("data-zz-gpui-layer", "").ok();
        layer
            .set_attribute("data-zz-gpui-window", &window_id.to_string())
            .ok();
        let style = layer.style();
        for (property, value) in [
            ("position", "absolute"),
            ("left", "0"),
            ("top", "0"),
            ("width", "0"),
            ("height", "0"),
            ("margin", "0"),
            ("padding", "0"),
            ("border", "0"),
            ("overflow", "visible"),
            ("pointer-events", "none"),
            ("color", "transparent"),
            ("background", "transparent"),
            ("caret-color", "transparent"),
            ("user-select", "none"),
            ("-webkit-user-select", "none"),
            ("contain", "layout style"),
        ] {
            style.set_property(property, value).ok();
        }
        if let Some(parent) = canvas.parent_node() {
            parent
                .insert_before(&layer, canvas.next_sibling().as_ref())
                .map_err(|error| anyhow::anyhow!("attaching the accessibility layer: {error:?}"))?;
        }

        let this = Rc::new(Self {
            document: document.clone(),
            layer,
            nodes: RefCell::new(HashMap::new()),
            root: Cell::new(None),
            focus: Cell::new(None),
            callbacks: RefCell::new(None),
            active: Cell::new(false),
            interactive: Cell::new(false),
            pressed: Cell::new(false),
            scale: Cell::new(1.0),
            _listeners: RefCell::new(Vec::new()),
        });
        let weak = Rc::downgrade(&this);
        let click = EventListenerHandle::add(this.layer.as_ref(), "click", move |event| {
            let Some(this) = weak.upgrade() else {
                return;
            };
            let event: web_sys::Event = event.unchecked_into();
            if this.pressed.replace(false) {
                return;
            }
            let Some(target) = event
                .target()
                .and_then(|target| target.dyn_into::<web_sys::Element>().ok())
            else {
                return;
            };
            let Ok(Some(element)) = target.closest(&format!("[{NODE_ATTRIBUTE}]")) else {
                return;
            };
            if let Some(id) = element
                .get_attribute(NODE_ATTRIBUTE)
                .and_then(|id| id.parse::<u64>().ok())
            {
                event.prevent_default();
                this.request(NodeId(id), Action::Click);
            }
        });
        let mut listeners = vec![click];
        for event_name in FORWARDED {
            let weak = Rc::downgrade(&this);
            let canvas = canvas.clone();
            listeners.push(EventListenerHandle::add_non_passive(
                this.layer.as_ref(),
                event_name,
                move |event| {
                    if let Some(this) = weak.upgrade() {
                        this.forward(&canvas, event);
                    }
                },
            ));
        }
        *this._listeners.borrow_mut() = listeners;
        Ok(this)
    }

    fn forward(&self, canvas: &web_sys::HtmlCanvasElement, event: JsValue) {
        let original: web_sys::Event = event.clone().unchecked_into();
        let Ok(constructor) = js_sys::Reflect::get(&event, &"constructor".into()) else {
            return;
        };
        let Ok(constructor) = constructor.dyn_into::<js_sys::Function>() else {
            return;
        };
        let arguments = js_sys::Array::of2(&original.type_().into(), &event);
        let Ok(copy) = js_sys::Reflect::construct(&constructor, &arguments) else {
            return;
        };
        let copy: web_sys::Event = copy.unchecked_into();
        if original.type_() == "pointerdown" {
            self.pressed.set(true);
        }
        original.stop_propagation();
        if !canvas.dispatch_event(&copy).unwrap_or(true) {
            original.prevent_default();
        }
    }

    pub(crate) fn contains(&self, target: Option<web_sys::EventTarget>) -> bool {
        target
            .and_then(|target| target.dyn_into::<web_sys::Node>().ok())
            .is_some_and(|node| self.layer.contains(Some(&node)))
    }

    pub(crate) fn set_interactive(&self, interactive: bool) {
        if self.interactive.replace(interactive) == interactive {
            return;
        }
        for mirrored in self.nodes.borrow().values() {
            set_pointer_events(&mirrored.element, &mirrored.node, interactive);
        }
    }

    pub(crate) fn is_active(&self) -> bool {
        self.active.get()
    }

    pub(crate) fn set_callbacks(&self, callbacks: A11yCallbacks) {
        *self.callbacks.borrow_mut() = Some(Rc::new(callbacks));
    }

    pub(crate) fn activate(&self) {
        if self.active.replace(true) {
            return;
        }
        let callbacks = self.callbacks.borrow().clone();
        if let Some(callbacks) = callbacks
            && let Some(update) = (callbacks.activation)()
        {
            self.apply(update);
        }
    }

    pub(crate) fn deactivate(&self) {
        if !self.active.replace(false) {
            return;
        }
        let callbacks = self.callbacks.borrow().clone();
        if let Some(callbacks) = callbacks {
            (callbacks.deactivation)();
        }
        for (_, mirrored) in self.nodes.borrow_mut().drain() {
            mirrored.element.remove();
        }
        self.root.set(None);
        self.focus.set(None);
    }

    pub(crate) fn request(&self, target: NodeId, action: Action) -> bool {
        let callbacks = self.callbacks.borrow().clone();
        let Some(callbacks) = callbacks else {
            return false;
        };
        if !self.nodes.borrow().contains_key(&target) {
            return false;
        }
        (callbacks.action)(ActionRequest {
            action,
            target_tree: TreeId::ROOT,
            target_node: target,
            data: None,
        });
        true
    }

    pub(crate) fn place_layer(&self, canvas: &web_sys::HtmlCanvasElement, device_pixel_ratio: f64) {
        self.scale.set(device_pixel_ratio.max(f64::MIN_POSITIVE));
        let style = self.layer.style();
        style
            .set_property("left", &format!("{}px", canvas.offset_left()))
            .ok();
        style
            .set_property("top", &format!("{}px", canvas.offset_top()))
            .ok();
        style
            .set_property("width", &format!("{}px", canvas.offset_width()))
            .ok();
        style
            .set_property("height", &format!("{}px", canvas.offset_height()))
            .ok();
    }

    pub(crate) fn apply(&self, update: TreeUpdate) {
        if !self.active.get() {
            return;
        }
        if let Some(tree) = &update.tree {
            self.root.set(Some(tree.root));
        }
        {
            let mut nodes = self.nodes.borrow_mut();
            for (id, node) in update.nodes {
                match nodes.get_mut(&id) {
                    Some(mirrored) if mirrored.node == node => {}
                    Some(mirrored) => {
                        write_attributes(&mirrored.element, &node);
                        set_pointer_events(&mirrored.element, &node, self.interactive.get());
                        mirrored.node = node;
                    }
                    None => {
                        let Ok(element) = self.document.create_element("div") else {
                            continue;
                        };
                        let element: web_sys::HtmlElement = element.unchecked_into();
                        let style = element.style();
                        for (property, value) in [
                            ("position", "absolute"),
                            ("display", "block"),
                            ("margin", "0"),
                            ("padding", "0"),
                            ("border", "0"),
                            ("box-sizing", "border-box"),
                            ("overflow", "visible"),
                            ("white-space", "pre"),
                            ("touch-action", "none"),
                        ] {
                            style.set_property(property, value).ok();
                        }
                        element
                            .set_attribute(NODE_ATTRIBUTE, &id.0.to_string())
                            .ok();
                        write_attributes(&element, &node);
                        set_pointer_events(&element, &node, self.interactive.get());
                        nodes.insert(
                            id,
                            Mirrored {
                                node,
                                element,
                                placed: None,
                                origin: CssRect::default(),
                            },
                        );
                    }
                }
            }
        }
        self.restructure();
        self.set_focus(update.focus);
    }

    fn restructure(&self) {
        let Some(root) = self.root.get() else {
            return;
        };
        let scale = self.scale.get();
        let mut nodes = self.nodes.borrow_mut();
        let mut reachable = HashSet::with_capacity(nodes.len());
        let mut stack = vec![(root, None::<NodeId>, CssRect::default())];
        while let Some((id, parent, parent_origin)) = stack.pop() {
            if !reachable.insert(id) {
                continue;
            }
            let Some(mirrored) = nodes.get_mut(&id) else {
                continue;
            };
            let absolute = mirrored.node.bounds().map(|rect| CssRect {
                x: rect.x0 / scale,
                y: rect.y0 / scale,
                width: (rect.x1 - rect.x0) / scale,
                height: (rect.y1 - rect.y0) / scale,
            });
            let origin = absolute.unwrap_or(parent_origin);
            let placed = CssRect {
                x: origin.x - parent_origin.x,
                y: origin.y - parent_origin.y,
                width: absolute.map_or(0.0, |rect| rect.width),
                height: absolute.map_or(0.0, |rect| rect.height),
            };
            if mirrored.placed != Some(placed) {
                let style = mirrored.element.style();
                style.set_property("left", &format!("{}px", placed.x)).ok();
                style.set_property("top", &format!("{}px", placed.y)).ok();
                style
                    .set_property("width", &format!("{}px", placed.width))
                    .ok();
                style
                    .set_property("height", &format!("{}px", placed.height))
                    .ok();
                mirrored.placed = Some(placed);
            }
            mirrored.origin = origin;
            let element = mirrored.element.clone();
            let children: Vec<NodeId> = mirrored.node.children().to_vec();
            let children: Vec<NodeId> = children
                .into_iter()
                .filter(|child| !nodes.get(child).is_some_and(|child| child.node.is_hidden()))
                .collect();
            let parent_element: web_sys::Element = match parent {
                Some(parent) => match nodes.get(&parent) {
                    Some(parent) => parent.element.clone().into(),
                    None => continue,
                },
                None => self.layer.clone().into(),
            };
            if element.parent_element().as_ref() != Some(&parent_element) {
                parent_element.append_child(&element).ok();
            }
            let mut cursor = element.first_element_child();
            for child in &children {
                let Some(child_element) = nodes.get(child).map(|child| child.element.clone())
                else {
                    continue;
                };
                let child_element: web_sys::Element = child_element.into();
                if cursor.as_ref() != Some(&child_element) {
                    element
                        .insert_before(&child_element, cursor.as_ref().map(AsRef::as_ref))
                        .ok();
                } else {
                    cursor = cursor.and_then(|current| current.next_element_sibling());
                }
            }
            for child in children.into_iter().rev() {
                stack.push((child, Some(id), origin));
            }
        }
        nodes.retain(|id, mirrored| {
            let keep = reachable.contains(id);
            if !keep {
                mirrored.element.remove();
            }
            keep
        });
    }

    fn set_focus(&self, focus: NodeId) {
        let previous = self.focus.replace(Some(focus));
        if previous == Some(focus) {
            return;
        }
        let nodes = self.nodes.borrow();
        if let Some(previous) = previous.and_then(|previous| nodes.get(&previous)) {
            previous.element.remove_attribute(FOCUS_ATTRIBUTE).ok();
        }
        if let Some(current) = nodes.get(&focus) {
            current.element.set_attribute(FOCUS_ATTRIBUTE, "").ok();
        }
    }

    pub(crate) fn snapshot(&self, client_origin: (f64, f64)) -> Option<NodeSnapshot> {
        let root = self.root.get()?;
        let nodes = self.nodes.borrow();
        let focus = self.focus.get();
        let scale = self.scale.get();
        Some(snapshot_node(root, &nodes, focus, scale, client_origin, 0))
    }

    pub(crate) fn remove(&self) {
        self.layer.remove();
    }
}

impl Drop for WebA11y {
    fn drop(&mut self) {
        self.layer.remove();
    }
}

fn snapshot_node(
    id: NodeId,
    nodes: &HashMap<NodeId, Mirrored>,
    focus: Option<NodeId>,
    scale: f64,
    client_origin: (f64, f64),
    depth: usize,
) -> NodeSnapshot {
    let Some(mirrored) = nodes.get(&id) else {
        return NodeSnapshot {
            id,
            role: Role::Unknown,
            name: None,
            value: None,
            text: None,
            author_id: None,
            bounds: None,
            focused: false,
            states: Vec::new(),
            children: Vec::new(),
        };
    };
    let node = &mirrored.node;
    let children = if depth > 256 {
        Vec::new()
    } else {
        node.children()
            .iter()
            .filter(|child| {
                nodes
                    .get(child)
                    .is_some_and(|child| !child.node.is_hidden())
            })
            .map(|child| snapshot_node(*child, nodes, focus, scale, client_origin, depth + 1))
            .collect()
    };
    NodeSnapshot {
        id,
        role: node.role(),
        name: node.label().map(str::to_owned),
        value: node.value().map(str::to_owned),
        text: Some(descendant_text(&children))
            .filter(|text| !text.is_empty())
            .or_else(|| node.label().or(node.value()).map(str::to_owned)),
        author_id: node.author_id().map(str::to_owned),
        bounds: node.bounds().map(|rect| CssRect {
            x: client_origin.0 + rect.x0 / scale,
            y: client_origin.1 + rect.y0 / scale,
            width: (rect.x1 - rect.x0) / scale,
            height: (rect.y1 - rect.y0) / scale,
        }),
        focused: focus == Some(id),
        states: states(node),
        children,
    }
}

fn descendant_text(children: &[NodeSnapshot]) -> String {
    let mut text = String::new();
    for child in children {
        let part = child
            .text
            .as_deref()
            .or(child.name.as_deref())
            .or(child.value.as_deref())
            .unwrap_or_default();
        if part.is_empty() {
            continue;
        }
        if !text.is_empty() {
            text.push(' ');
        }
        text.push_str(part);
        if text.len() > TEXT_LIMIT {
            let mut end = TEXT_LIMIT;
            while !text.is_char_boundary(end) {
                end -= 1;
            }
            text.truncate(end);
            break;
        }
    }
    text
}

fn is_interactive(node: &Node) -> bool {
    node.supports_action(Action::Click)
        || node.supports_action(Action::Focus)
        || is_text_input(node.role())
}

fn set_pointer_events(element: &web_sys::HtmlElement, node: &Node, interactive: bool) {
    let value = if interactive && is_interactive(node) {
        "auto"
    } else {
        "none"
    };
    let style = element.style();
    if style.get_property_value("pointer-events").ok().as_deref() != Some(value) {
        style.set_property("pointer-events", value).ok();
    }
}

fn write_attributes(element: &web_sys::HtmlElement, node: &Node) {
    let role = node.role();
    set(element, "role", aria_role(role));
    let text_leaf = node.children().is_empty() && is_text_role(role);
    let text = if text_leaf {
        node.label().or(node.value())
    } else if is_text_input(role) {
        node.value()
    } else {
        None
    };
    element.set_text_content(text);
    set(
        element,
        "aria-label",
        if text_leaf { None } else { node.label() },
    );
    set(element, "aria-description", node.description());
    set(element, "aria-placeholder", node.placeholder());
    set(element, "aria-roledescription", node.role_description());
    set(element, "aria-keyshortcuts", node.keyboard_shortcut());
    set(element, AUTHOR_ATTRIBUTE, node.author_id());
    set(
        element,
        "aria-valuetext",
        if is_text_input(role) || text_leaf {
            None
        } else {
            node.value()
        },
    );
    set(
        element,
        "aria-valuenow",
        node.numeric_value()
            .map(|value| value.to_string())
            .as_deref(),
    );
    set(
        element,
        "aria-valuemin",
        node.min_numeric_value()
            .map(|value| value.to_string())
            .as_deref(),
    );
    set(
        element,
        "aria-valuemax",
        node.max_numeric_value()
            .map(|value| value.to_string())
            .as_deref(),
    );
    set(
        element,
        "aria-level",
        node.level().map(|level| level.to_string()).as_deref(),
    );
    set(
        element,
        "aria-posinset",
        node.position_in_set()
            .map(|position| position.to_string())
            .as_deref(),
    );
    set(
        element,
        "aria-setsize",
        node.size_of_set().map(|size| size.to_string()).as_deref(),
    );
    let toggled = node.toggled().map(|toggled| match toggled {
        Toggled::True => "true",
        Toggled::False => "false",
        Toggled::Mixed => "mixed",
    });
    let pressed = matches!(role, Role::Button | Role::DefaultButton);
    set(element, "aria-pressed", toggled.filter(|_| pressed));
    set(element, "aria-checked", toggled.filter(|_| !pressed));
    set(element, "aria-selected", node.is_selected().map(bool_str));
    set(element, "aria-expanded", node.is_expanded().map(bool_str));
    set(
        element,
        "aria-disabled",
        node.is_disabled().then_some("true"),
    );
    set(
        element,
        "aria-readonly",
        node.is_read_only().then_some("true"),
    );
    set(
        element,
        "aria-required",
        node.is_required().then_some("true"),
    );
    set(element, "aria-modal", node.is_modal().then_some("true"));
    set(
        element,
        "aria-multiselectable",
        node.is_multiselectable().then_some("true"),
    );
    set(
        element,
        "aria-orientation",
        node.orientation().map(|orientation| match orientation {
            accesskit::Orientation::Horizontal => "horizontal",
            accesskit::Orientation::Vertical => "vertical",
        }),
    );
    set(
        element,
        "aria-haspopup",
        node.has_popup().map(|popup| match popup {
            accesskit::HasPopup::Menu => "menu",
            accesskit::HasPopup::Listbox => "listbox",
            accesskit::HasPopup::Tree => "tree",
            accesskit::HasPopup::Grid => "grid",
            accesskit::HasPopup::Dialog => "dialog",
        }),
    );
}

fn set(element: &web_sys::HtmlElement, name: &str, value: Option<&str>) {
    match value {
        Some(value) => {
            if element.get_attribute(name).as_deref() != Some(value) {
                element.set_attribute(name, value).ok();
            }
        }
        None => {
            if element.has_attribute(name) {
                element.remove_attribute(name).ok();
            }
        }
    }
}

fn bool_str(value: bool) -> &'static str {
    if value { "true" } else { "false" }
}

fn states(node: &Node) -> Vec<(&'static str, String)> {
    let mut states = Vec::new();
    if let Some(toggled) = node.toggled() {
        states.push((
            "checked",
            match toggled {
                Toggled::True => "true",
                Toggled::False => "false",
                Toggled::Mixed => "mixed",
            }
            .to_owned(),
        ));
    }
    if let Some(selected) = node.is_selected() {
        states.push(("selected", selected.to_string()));
    }
    if let Some(expanded) = node.is_expanded() {
        states.push(("expanded", expanded.to_string()));
    }
    if node.is_disabled() {
        states.push(("disabled", "true".to_owned()));
    }
    if let Some(value) = node.numeric_value() {
        states.push(("valuenow", value.to_string()));
    }
    if let Some(level) = node.level() {
        states.push(("level", level.to_string()));
    }
    if let Some(description) = node.description() {
        states.push(("description", description.to_owned()));
    }
    if let Some(placeholder) = node.placeholder() {
        states.push(("placeholder", placeholder.to_owned()));
    }
    states
}

fn is_text_role(role: Role) -> bool {
    matches!(
        role,
        Role::Label
            | Role::TextRun
            | Role::Paragraph
            | Role::Heading
            | Role::Code
            | Role::Strong
            | Role::Emphasis
            | Role::Mark
            | Role::Time
            | Role::Caption
            | Role::FigureCaption
            | Role::Legend
            | Role::Term
            | Role::Definition
            | Role::Abbr
    )
}

fn is_text_input(role: Role) -> bool {
    matches!(
        role,
        Role::TextInput
            | Role::MultilineTextInput
            | Role::SearchInput
            | Role::EmailInput
            | Role::NumberInput
            | Role::PasswordInput
            | Role::PhoneNumberInput
            | Role::UrlInput
            | Role::DateInput
            | Role::DateTimeInput
            | Role::WeekInput
            | Role::MonthInput
            | Role::TimeInput
            | Role::EditableComboBox
            | Role::Terminal
    )
}

pub(crate) fn role_name(role: Role) -> &'static str {
    aria_role(role).unwrap_or("generic")
}

fn aria_role(role: Role) -> Option<&'static str> {
    Some(match role {
        Role::Unknown | Role::GenericContainer | Role::Pane | Role::Canvas => return None,
        Role::TextRun | Role::Label | Role::Caret => "text",
        Role::Cell | Role::LayoutTableCell => "cell",
        Role::GridCell => "gridcell",
        Role::Image | Role::SvgRoot | Role::GraphicsObject | Role::GraphicsSymbol => "img",
        Role::Link => "link",
        Role::Row | Role::LayoutTableRow => "row",
        Role::ListItem => "listitem",
        Role::ListMarker => "presentation",
        Role::TreeItem => "treeitem",
        Role::ListBoxOption | Role::MenuListOption => "option",
        Role::MenuItem => "menuitem",
        Role::MenuItemCheckBox => "menuitemcheckbox",
        Role::MenuItemRadio => "menuitemradio",
        Role::Paragraph => "paragraph",
        Role::CheckBox => "checkbox",
        Role::RadioButton => "radio",
        Role::TextInput
        | Role::MultilineTextInput
        | Role::EmailInput
        | Role::NumberInput
        | Role::PasswordInput
        | Role::PhoneNumberInput
        | Role::UrlInput
        | Role::DateInput
        | Role::DateTimeInput
        | Role::WeekInput
        | Role::MonthInput
        | Role::TimeInput
        | Role::Terminal => "textbox",
        Role::SearchInput => "searchbox",
        Role::Button | Role::DefaultButton | Role::DisclosureTriangle | Role::ColorWell => "button",
        Role::RowHeader => "rowheader",
        Role::ColumnHeader => "columnheader",
        Role::RowGroup => "rowgroup",
        Role::List | Role::DescriptionList => "list",
        Role::Table | Role::LayoutTable => "table",
        Role::Switch => "switch",
        Role::Menu | Role::MenuListPopup => "menu",
        Role::MenuBar => "menubar",
        Role::Abbr | Role::Ruby | Role::RubyAnnotation | Role::Keyboard => return None,
        Role::Alert => "alert",
        Role::AlertDialog => "alertdialog",
        Role::Application => "application",
        Role::Article => "article",
        Role::Audio | Role::Video | Role::EmbeddedObject | Role::PluginObject => "group",
        Role::Banner | Role::Header => "banner",
        Role::Blockquote => "blockquote",
        Role::Caption | Role::FigureCaption => "caption",
        Role::Code => "code",
        Role::ComboBox | Role::EditableComboBox => "combobox",
        Role::Complementary => "complementary",
        Role::Comment => "comment",
        Role::ContentDeletion => "deletion",
        Role::ContentInsertion => "insertion",
        Role::ContentInfo | Role::Footer => "contentinfo",
        Role::Definition => "definition",
        Role::Details | Role::Group | Role::Section | Role::SectionHeader | Role::SectionFooter => {
            "group"
        }
        Role::Dialog => "dialog",
        Role::Document | Role::RootWebArea | Role::PdfRoot | Role::GraphicsDocument => "document",
        Role::Emphasis => "emphasis",
        Role::Feed => "feed",
        Role::Figure => "figure",
        Role::Form => "form",
        Role::Grid | Role::ListGrid => "grid",
        Role::Heading => "heading",
        Role::Iframe | Role::IframePresentational | Role::WebView => "document",
        Role::ImeCandidate | Role::Suggestion => "option",
        Role::Legend => "caption",
        Role::LineBreak => "presentation",
        Role::ListBox => "listbox",
        Role::Log => "log",
        Role::Main => "main",
        Role::Mark => "mark",
        Role::Marquee => "marquee",
        Role::Math => "math",
        Role::Meter => "meter",
        Role::Navigation => "navigation",
        Role::Note => "note",
        Role::ProgressIndicator => "progressbar",
        Role::RadioGroup => "radiogroup",
        Role::Region => "region",
        Role::ScrollBar => "scrollbar",
        Role::ScrollView => "group",
        Role::Search => "search",
        Role::Slider => "slider",
        Role::SpinButton => "spinbutton",
        Role::Splitter => "separator",
        Role::Status => "status",
        Role::Strong => "strong",
        Role::Tab => "tab",
        Role::TabList => "tablist",
        Role::TabPanel => "tabpanel",
        Role::Term => "term",
        Role::Time => "time",
        Role::Timer => "timer",
        Role::TitleBar | Role::Toolbar => "toolbar",
        Role::Tooltip => "tooltip",
        Role::Tree => "tree",
        Role::TreeGrid => "treegrid",
        Role::Window => "region",
        Role::PdfActionableHighlight => "button",
        _ => "generic",
    })
}
