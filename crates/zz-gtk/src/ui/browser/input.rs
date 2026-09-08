use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use gtk::{gdk, glib, prelude::*};
use zz_browser::{
    BrowserKey, KeyAction, KeyInput, Modifiers, PointerButton, PointerEvent, PointerPhase,
    WheelEvent,
};
use zz_protocol::InputMessage;
use zz_terminal::{KeyAction as TerminalAction, KeyCode};

use super::BrowserTab;

pub(super) fn connect(tab: &Rc<BrowserTab>) {
    let im = gtk::IMMulticontext::new();
    im.set_client_widget(Some(&tab.picture));
    let pending = Rc::new(RefCell::new(None::<String>));
    let filtering = Rc::new(Cell::new(false));
    let routes = Rc::new(RefCell::new(std::collections::HashMap::<u32, bool>::new()));
    let weak = Rc::downgrade(tab);
    let commit = Rc::clone(&pending);
    let in_key = Rc::clone(&filtering);
    im.connect_commit(move |_, text| {
        if in_key.get() {
            commit.replace(Some(text.to_owned()));
        } else if let Some(pane) = weak.upgrade().and_then(|tab| tab.owner.borrow().upgrade()) {
            pane.engine.send_on(
                pane.host,
                InputMessage::BrowserSurfaceText {
                    pane: pane.pane,
                    text: text.to_owned(),
                },
            );
        }
    });
    let weak = Rc::downgrade(tab);
    im.connect_preedit_changed(move |im| {
        let Some(tab) = weak.upgrade() else {
            return;
        };
        let (text, _, cursor) = im.preedit_string();
        let position = text
            .chars()
            .take(cursor.max(0) as usize)
            .map(char::len_utf16)
            .sum();
        tab.with_session(|session| {
            if text.is_empty() {
                session.cancel_composition();
            } else {
                session.set_composition(&text, position..position);
            }
        });
    });
    let focus = gtk::EventControllerFocus::new();
    let weak_tab = Rc::downgrade(tab);
    let context = im.clone();
    focus.connect_enter(move |_| {
        context.focus_in();
        if let Some(tab) = weak_tab.upgrade() {
            tab.with_session(|session| session.set_focus(true));
            if let Some(pane) = tab.owner.borrow().upgrade() {
                pane.pane_command("select-pane");
            }
        }
    });
    let weak = Rc::downgrade(tab);
    let context = im.clone();
    let focused_routes = Rc::clone(&routes);
    focus.connect_leave(move |_| {
        context.focus_out();
        focused_routes.borrow_mut().clear();
        if let Some(tab) = weak.upgrade() {
            tab.with_session(|session| session.set_focus(false));
        }
    });
    tab.picture.add_controller(focus);

    let keyboard = gtk::EventControllerLegacy::new();
    let weak = Rc::downgrade(tab);
    keyboard.connect_event(move |_, event| {
        let Some(event) = event.downcast_ref::<gdk::KeyEvent>() else {
            return glib::Propagation::Proceed;
        };
        if crate::ui::keys::is_modifier(event.keyval()) {
            return glib::Propagation::Proceed;
        }
        let Some(pane) = weak.upgrade().and_then(|tab| tab.owner.borrow().upgrade()) else {
            return glib::Propagation::Proceed;
        };
        let action = match event.event_type() {
            gdk::EventType::KeyRelease => {
                if routes.borrow_mut().remove(&event.keycode()) != Some(true) {
                    return glib::Propagation::Stop;
                }
                TerminalAction::Release
            }
            gdk::EventType::KeyPress => {
                let previous = routes.borrow_mut().insert(event.keycode(), false);
                match previous {
                    Some(false) => return glib::Propagation::Stop,
                    Some(true) => TerminalAction::Repeat,
                    None => TerminalAction::Press,
                }
            }
            _ => return glib::Propagation::Proceed,
        };
        pending.take();
        filtering.set(true);
        let filtered = action != TerminalAction::Release && im.filter_keypress(event);
        filtering.set(false);
        let committed = pending.take();
        if filtered && committed.is_none() {
            return glib::Propagation::Stop;
        }
        let input = crate::ui::keys::key_input(
            action,
            event.keyval(),
            event.modifier_state(),
            committed.as_deref(),
        );
        if action != TerminalAction::Release {
            routes.borrow_mut().insert(event.keycode(), true);
        }
        pane.engine.send_on(
            pane.host,
            InputMessage::BrowserSurfaceKey {
                pane: pane.pane,
                input,
                text_follows: committed.is_some(),
            },
        );
        if let Some(text) = committed {
            pane.engine.send_on(
                pane.host,
                InputMessage::BrowserSurfaceText {
                    pane: pane.pane,
                    text,
                },
            );
        }
        glib::Propagation::Stop
    });
    tab.picture.add_controller(keyboard);

    let motion = gtk::EventControllerMotion::new();
    let weak = Rc::downgrade(tab);
    motion.connect_motion(move |controller, x, y| {
        if let Some(tab) = weak.upgrade() {
            tab.pointer.set((x as i32, y as i32));
            pointer(
                &tab,
                controller.current_event_state(),
                PointerPhase::Move,
                None,
                1,
            );
        }
    });
    let weak = Rc::downgrade(tab);
    motion.connect_leave(move |controller| {
        if let Some(tab) = weak.upgrade() {
            pointer(
                &tab,
                controller.current_event_state(),
                PointerPhase::Leave,
                None,
                1,
            );
        }
    });
    tab.picture.add_controller(motion);
    let click = gtk::GestureClick::new();
    click.set_button(0);
    let weak = Rc::downgrade(tab);
    click.connect_pressed(move |gesture, count, x, y| {
        if let Some(tab) = weak.upgrade() {
            tab.picture.grab_focus();
            tab.pointer.set((x as i32, y as i32));
            pointer(
                &tab,
                gesture.current_event_state(),
                PointerPhase::Down,
                button(gesture.current_button()),
                count,
            );
        }
    });
    let weak = Rc::downgrade(tab);
    click.connect_released(move |gesture, count, x, y| {
        if let Some(tab) = weak.upgrade() {
            tab.pointer.set((x as i32, y as i32));
            pointer(
                &tab,
                gesture.current_event_state(),
                PointerPhase::Up,
                button(gesture.current_button()),
                count,
            );
        }
    });
    tab.picture.add_controller(click);
    let scroll = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::BOTH_AXES);
    let weak = Rc::downgrade(tab);
    scroll.connect_scroll(move |controller, dx, dy| {
        let Some(tab) = weak.upgrade() else {
            return glib::Propagation::Proceed;
        };
        let precise = controller.unit() == gdk::ScrollUnit::Surface;
        let factor = if precise { -1.0 } else { -120.0 };
        let (x, y) = tab.pointer.get();
        tab.with_session(|session| {
            session.send_wheel(WheelEvent {
                x,
                y,
                delta_x: (dx * factor).round() as i32,
                delta_y: (dy * factor).round() as i32,
                precise,
                modifiers: modifiers(controller.current_event_state()),
            });
        });
        glib::Propagation::Stop
    });
    tab.picture.add_controller(scroll);
}

fn pointer(
    tab: &BrowserTab,
    state: gdk::ModifierType,
    phase: PointerPhase,
    button: Option<PointerButton>,
    click_count: i32,
) {
    let (x, y) = tab.pointer.get();
    tab.with_session(|session| {
        session.send_pointer(PointerEvent {
            x,
            y,
            phase,
            button,
            click_count,
            modifiers: modifiers(state),
        });
    });
}

fn button(value: u32) -> Option<PointerButton> {
    match value {
        1 => Some(PointerButton::Left),
        2 => Some(PointerButton::Middle),
        3 => Some(PointerButton::Right),
        _ => None,
    }
}

fn modifiers(state: gdk::ModifierType) -> Modifiers {
    Modifiers::new(
        state.contains(gdk::ModifierType::SHIFT_MASK),
        state.contains(gdk::ModifierType::CONTROL_MASK),
        state.contains(gdk::ModifierType::ALT_MASK),
        state.contains(gdk::ModifierType::SUPER_MASK),
    )
    .with_pointer_button(if state.contains(gdk::ModifierType::BUTTON1_MASK) {
        Some(PointerButton::Left)
    } else if state.contains(gdk::ModifierType::BUTTON2_MASK) {
        Some(PointerButton::Middle)
    } else if state.contains(gdk::ModifierType::BUTTON3_MASK) {
        Some(PointerButton::Right)
    } else {
        None
    })
}

pub(super) fn translate(input: &zz_terminal::KeyInput) -> KeyInput {
    let key = match input.key {
        KeyCode::Character(c) => BrowserKey::Character(c),
        KeyCode::Backspace => BrowserKey::Backspace,
        KeyCode::Enter => BrowserKey::Enter,
        KeyCode::Tab => BrowserKey::Tab,
        KeyCode::Escape => BrowserKey::Escape,
        KeyCode::Delete => BrowserKey::Delete,
        KeyCode::Insert => BrowserKey::Insert,
        KeyCode::Home => BrowserKey::Home,
        KeyCode::End => BrowserKey::End,
        KeyCode::PageUp => BrowserKey::PageUp,
        KeyCode::PageDown => BrowserKey::PageDown,
        KeyCode::ArrowUp => BrowserKey::ArrowUp,
        KeyCode::ArrowDown => BrowserKey::ArrowDown,
        KeyCode::ArrowLeft => BrowserKey::ArrowLeft,
        KeyCode::ArrowRight => BrowserKey::ArrowRight,
        KeyCode::Function(n) => BrowserKey::Function(n),
        KeyCode::Unidentified => BrowserKey::Unidentified,
    };
    KeyInput {
        key,
        action: if input.action == TerminalAction::Release {
            KeyAction::Release
        } else {
            KeyAction::Press
        },
        modifiers: Modifiers::new(
            input.modifiers.shift(),
            input.modifiers.control(),
            input.modifiers.alt(),
            input.modifiers.platform(),
        )
        .with_repeat(input.action == TerminalAction::Repeat),
    }
}

pub(super) fn named(mut name: &str) -> Option<KeyInput> {
    let mut modifiers = Modifiers::default();
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
        name if name.starts_with('F') => BrowserKey::Function(name[1..].parse().ok()?),
        name if name.chars().count() == 1 => BrowserKey::Character(name.chars().next()?),
        _ => return None,
    };
    Some(KeyInput {
        action: KeyAction::Press,
        key,
        modifiers,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browser_releases_preserve_the_shared_key_contract() {
        let input = zz_terminal::KeyInput {
            key: KeyCode::Function(12),
            action: TerminalAction::Release,
            modifiers: zz_terminal::Modifiers::new(true, true, false, false),
            text: None,
            unshifted_codepoint: None,
        };
        let result = translate(&input);
        assert_eq!(result.key, BrowserKey::Function(12));
        assert_eq!(result.action, KeyAction::Release);
        assert!(result.modifiers.shift());
        assert!(result.modifiers.control());
        assert!(!result.modifiers.is_repeat());
    }

    #[test]
    fn browser_named_keys_keep_control_and_meta_modifiers() {
        let result = named("C-M-F12").expect("a supported daemon key token");
        assert_eq!(result.key, BrowserKey::Function(12));
        assert!(result.modifiers.control());
        assert!(result.modifiers.alt());
        assert_eq!(named("not-a-key"), None);
    }
}
