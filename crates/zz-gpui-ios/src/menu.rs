use crate::{id, nil, ns_array, ns_string};
use gpui::{Action, Menu, MenuItem, SharedString};
use objc::{class, msg_send, sel, sel_impl};

#[derive(Clone, Debug, PartialEq, gpui::Action)]
#[action(namespace = zz_ios, no_json)]
pub struct MenuCommand {
    pub id: SharedString,
    pub shortcut: Option<SharedString>,
}

enum Item {
    Separator,
    Submenu(SharedString, Vec<Item>),
    Command {
        title: SharedString,
        action: usize,
        key: Option<(String, usize)>,
    },
}

#[derive(Default)]
pub(crate) struct MenuState {
    menus: Vec<(SharedString, Vec<Item>)>,
    actions: Vec<Box<dyn Action>>,
}

impl MenuState {
    pub(crate) fn new(menus: Vec<Menu>) -> Self {
        let mut actions = Vec::new();
        let menus = menus
            .into_iter()
            .map(|menu| (menu.name, items(menu.items, &mut actions)))
            .collect();
        Self { menus, actions }
    }

    pub(crate) fn action(&self, index: usize) -> Option<Box<dyn Action>> {
        self.actions.get(index).map(|action| action.boxed_clone())
    }

    pub(crate) fn build(&self, builder: id) {
        unsafe {
            let system: id = msg_send![builder, system];
            let main: id = msg_send![class!(UIMenuSystem), mainSystem];
            if system != main {
                return;
            }
            for (index, (title, items)) in self.menus.iter().enumerate() {
                let children = children(items);
                if index == 0 {
                    let _: () = msg_send![
                        builder,
                        insertChildMenu: ui_menu("", &children, true)
                        atStartOfMenuForIdentifier: UIMenuApplication
                    ];
                } else if let Some(identifier) = standard_menu(title) {
                    let _: () = msg_send![
                        builder,
                        insertChildMenu: ui_menu("", &children, true)
                        atEndOfMenuForIdentifier: identifier
                    ];
                } else {
                    let _: () = msg_send![
                        builder,
                        insertSiblingMenu: ui_menu(title, &children, false)
                        beforeMenuForIdentifier: UIMenuWindow
                    ];
                }
            }
            let _: () = msg_send![builder, removeMenuForIdentifier: UIMenuFormat];
        }
    }
}

pub(crate) fn rebuild() {
    unsafe {
        let system: id = msg_send![class!(UIMenuSystem), mainSystem];
        let _: () = msg_send![system, setNeedsRebuild];
    }
}

pub(crate) fn command_index(command: id) -> Option<usize> {
    unsafe {
        let property: id = msg_send![command, propertyList];
        if property.is_null() {
            return None;
        }
        let index: usize = msg_send![property, unsignedIntegerValue];
        Some(index)
    }
}

fn items(items: Vec<MenuItem>, actions: &mut Vec<Box<dyn Action>>) -> Vec<Item> {
    items
        .into_iter()
        .filter_map(|item| match item {
            MenuItem::Separator => Some(Item::Separator),
            MenuItem::Submenu(menu) => {
                Some(Item::Submenu(menu.name, self::items(menu.items, actions)))
            }
            MenuItem::Action {
                name,
                action,
                os_action: None,
                ..
            } => {
                let key = action
                    .as_any()
                    .downcast_ref::<MenuCommand>()
                    .and_then(|command| command.shortcut.as_deref())
                    .and_then(crate::keyboard::key_command);
                actions.push(action);
                Some(Item::Command {
                    title: name,
                    action: actions.len() - 1,
                    key,
                })
            }
            MenuItem::Action { .. } | MenuItem::SystemMenu(_) => None,
        })
        .collect()
}

fn children(items: &[Item]) -> Vec<id> {
    let groups: Vec<Vec<id>> = items
        .split(|item| matches!(item, Item::Separator))
        .map(|group| group.iter().filter_map(element).collect::<Vec<_>>())
        .filter(|group| !group.is_empty())
        .collect();
    match groups.len() {
        1 => groups.into_iter().next().unwrap_or_default(),
        _ => groups
            .iter()
            .map(|group| ui_menu("", group, true))
            .collect(),
    }
}

fn element(item: &Item) -> Option<id> {
    match item {
        Item::Separator => None,
        Item::Submenu(title, items) => Some(ui_menu(title, &children(items), false)),
        Item::Command { title, action, key } => unsafe {
            let property: id = msg_send![class!(NSNumber), numberWithUnsignedInteger: *action];
            Some(match key {
                Some((input, flags)) => msg_send![
                    class!(UIKeyCommand),
                    commandWithTitle: ns_string(title)
                    image: nil
                    action: sel!(zzMenuCommand:)
                    input: ns_string(input)
                    modifierFlags: *flags
                    propertyList: property
                ],
                None => msg_send![
                    class!(UICommand),
                    commandWithTitle: ns_string(title)
                    image: nil
                    action: sel!(zzMenuCommand:)
                    propertyList: property
                ],
            })
        },
    }
}

fn ui_menu(title: &str, children: &[id], inline: bool) -> id {
    unsafe {
        msg_send![
            class!(UIMenu),
            menuWithTitle: ns_string(title)
            image: nil
            identifier: nil
            options: inline as usize
            children: ns_array(children)
        ]
    }
}

fn standard_menu(title: &str) -> Option<id> {
    unsafe {
        Some(match title {
            "File" => UIMenuFile,
            "Edit" => UIMenuEdit,
            "View" => UIMenuView,
            "Window" => UIMenuWindow,
            "Help" => UIMenuHelp,
            _ => return None,
        })
    }
}

#[link(name = "UIKit", kind = "framework")]
unsafe extern "C" {
    static UIMenuApplication: id;
    static UIMenuFile: id;
    static UIMenuEdit: id;
    static UIMenuView: id;
    static UIMenuWindow: id;
    static UIMenuHelp: id;
    static UIMenuFormat: id;
}
