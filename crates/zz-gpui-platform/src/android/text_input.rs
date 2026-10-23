use crate::android::editor::{IME_ACTION_DONE, editor_flags, is_terminal};
use crate::android::{
    bridge,
    window::{self, AndroidWindowState},
};
use std::{cell::RefCell, ops::Range, rc::Rc};
use zz_gpui::{Modifiers, PlatformInputHandler, TextInputMode};

const ID_SELECT_ALL: i32 = 0x0102001f;
const ID_CUT: i32 = 0x01020020;
const ID_COPY: i32 = 0x01020021;
const ID_PASTE: i32 = 0x01020022;

fn with_handler<R>(
    state: &Rc<RefCell<AndroidWindowState>>,
    f: impl FnOnce(&mut PlatformInputHandler) -> R,
) -> Option<R> {
    let mut handler = state.borrow_mut().take_input_handler()?;
    let result = f(&mut handler);
    state.borrow_mut().restore_input_handler(handler);
    Some(result)
}

fn current() -> Option<Rc<RefCell<AndroidWindowState>>> {
    window::with_state(Rc::clone)
}

fn document_len(handler: &mut PlatformInputHandler) -> usize {
    handler.text_length_utf16().unwrap_or_else(|| {
        let selected = handler
            .selected_text_range(false)
            .map_or(0, |selection| selection.range.end);
        let marked = handler.marked_text_range().map_or(0, |range| range.end);
        selected.max(marked)
    })
}

fn text_for_range(handler: &mut PlatformInputHandler, range: Range<usize>) -> Option<String> {
    let mut adjusted = None;
    let text = handler.text_for_range(range.clone(), &mut adjusted)?;
    let Some(adjusted) = adjusted.filter(|adjusted| *adjusted != range) else {
        return Some(text);
    };
    let units: Vec<u16> = text.encode_utf16().collect();
    let start = range.start.saturating_sub(adjusted.start).min(units.len());
    let end = range
        .end
        .saturating_sub(adjusted.start)
        .clamp(start, units.len());
    Some(String::from_utf16_lossy(&units[start..end]))
}

pub(crate) fn editor_info() -> [i32; 4] {
    let Some(state) = current() else {
        return [0, editor_flags(&Default::default()).1, -1, -1];
    };
    let configuration = state.borrow().text_input.clone();
    let (kind, options) = editor_flags(&configuration);
    let [start, end, ..] = selection().unwrap_or([-1; 4]);
    [kind, options, start, end]
}

pub(crate) fn commit(text: &str, _cursor: i32) {
    let Some(state) = current() else {
        return;
    };
    let latched = state.borrow().latched != Modifiers::default();
    let mut chars = text.chars();
    if latched && let (Some(character), None) = (chars.next(), chars.next()) {
        let (key, shift) = match character {
            '\n' | '\r' => ("enter".to_owned(), false),
            '\t' => ("tab".to_owned(), false),
            ' ' => ("space".to_owned(), false),
            character => (
                character.to_lowercase().to_string(),
                character.is_uppercase(),
            ),
        };
        let extra = Modifiers {
            shift,
            ..Default::default()
        };
        window::synthesize_keystroke(&state, &key, extra);
        sync_selection(&state);
        return;
    }
    let decimal = state.borrow().text_input.input_mode == TextInputMode::Decimal;
    let text = if decimal {
        text.replace(',', ".")
    } else {
        text.to_owned()
    };
    match text.as_str() {
        "\n" | "\r" => {
            with_handler(&state, PlatformInputHandler::unmark_text);
            window::synthesize_keystroke(&state, "enter", Modifiers::default());
        }
        "\t" => window::synthesize_keystroke(&state, "tab", Modifiers::default()),
        _ => {
            with_handler(&state, |handler| handler.replace_text_in_range(None, &text));
        }
    }
    sync_selection(&state);
}

pub(crate) fn compose(text: &str, cursor: i32) {
    let Some(state) = current() else {
        return;
    };
    let length = text.encode_utf16().count();
    let position = if cursor > 0 {
        (length + cursor as usize - 1).min(length)
    } else {
        0
    };
    with_handler(&state, |handler| {
        handler.replace_and_mark_text_in_range(None, text, Some(position..position));
        if text.is_empty() {
            handler.unmark_text();
        }
    });
    sync_selection(&state);
}

pub(crate) fn composing_region(start: i32, end: i32) {
    let Some(state) = current() else {
        return;
    };
    let (start, end) = (
        start.min(end).max(0) as usize,
        start.max(end).max(0) as usize,
    );
    with_handler(&state, |handler| {
        if start == end {
            unmark(handler);
            return;
        }
        let Some(text) = text_for_range(handler, start..end) else {
            return;
        };
        let length = text.encode_utf16().count();
        handler.replace_and_mark_text_in_range(Some(start..end), &text, Some(length..length));
    });
    sync_selection(&state);
}

fn unmark(handler: &mut PlatformInputHandler) {
    let Some(marked) = handler.marked_text_range() else {
        return;
    };
    match text_for_range(handler, marked.clone()) {
        Some(text) if !text.is_empty() => handler.replace_text_in_range(Some(marked), &text),
        _ => handler.unmark_text(),
    }
}

pub(crate) fn finish() {
    let Some(state) = current() else {
        return;
    };
    with_handler(&state, unmark);
    sync_selection(&state);
}

pub(crate) fn delete(before: i32, after: i32) {
    let Some(state) = current() else {
        return;
    };
    for _ in 0..before.max(0) {
        window::synthesize_keystroke(&state, "backspace", Modifiers::default());
    }
    for _ in 0..after.max(0) {
        window::synthesize_keystroke(&state, "delete", Modifiers::default());
    }
    sync_selection(&state);
}

pub(crate) fn text(start: i32, end: i32) -> Option<String> {
    let state = current()?;
    with_handler(&state, |handler| {
        let length = document_len(handler);
        let start = (start.max(0) as usize).min(length);
        let end = (end.max(0) as usize).clamp(start, length);
        text_for_range(handler, start..end)
    })
    .flatten()
}

pub(crate) fn selection() -> Option<[i32; 4]> {
    let state = current()?;
    selection_of(&state)
}

fn selection_of(state: &Rc<RefCell<AndroidWindowState>>) -> Option<[i32; 4]> {
    with_handler(state, |handler| {
        let selection = handler.selected_text_range(false)?;
        let marked = handler.marked_text_range();
        Some([
            selection.range.start as i32,
            selection.range.end as i32,
            marked.as_ref().map_or(-1, |range| range.start as i32),
            marked.as_ref().map_or(-1, |range| range.end as i32),
        ])
    })
    .flatten()
}

pub(crate) fn set_selection(start: i32, end: i32) {
    let Some(state) = current() else {
        return;
    };
    let (start, end) = (
        start.min(end).max(0) as usize,
        start.max(end).max(0) as usize,
    );
    with_handler(&state, |handler| {
        handler.set_selected_text_range(start..end)
    });
    sync_selection(&state);
}

pub(crate) fn editor_action(action: i32) {
    let Some(state) = current() else {
        return;
    };
    window::synthesize_keystroke(&state, "enter", Modifiers::default());
    if action == IME_ACTION_DONE {
        crate::android::dispatcher::defer(|| bridge::set_keyboard_visible(false));
    }
}

pub(crate) fn menu_action(id: i32) -> bool {
    let Some(state) = current() else {
        return false;
    };
    let terminal = is_terminal(&state.borrow().text_input);
    let chord = |key: &str| {
        let modifiers = if terminal {
            Modifiers {
                control: true,
                shift: true,
                ..Default::default()
            }
        } else {
            Modifiers::control()
        };
        window::synthesize_keystroke(&state, key, modifiers);
    };
    match id {
        ID_COPY => chord("c"),
        ID_SELECT_ALL => chord("a"),
        ID_CUT if !terminal => chord("x"),
        ID_PASTE => crate::android::window::request_paste(),
        _ => return false,
    }
    true
}

pub(crate) fn insert_unhandled(state: &Rc<RefCell<AndroidWindowState>>, text: &str) {
    with_handler(state, |handler| handler.replace_text_in_range(None, text));
    sync_selection(state);
}

pub(crate) fn sync_selection(state: &Rc<RefCell<AndroidWindowState>>) {
    if let Some(selection) = selection_of(state) {
        bridge::update_selection(selection);
    }
}
