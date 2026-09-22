use crate::{CGPoint, CGRect, CGSize, NSRange, id, nil};
use gpui::{Bounds, Pixels, PlatformInputHandler, point, px};
use objc::{
    class,
    declare::ClassDecl,
    msg_send,
    runtime::{BOOL, Class, NO, Object, Protocol, Sel, YES},
    sel, sel_impl,
};
use std::{ops::Range, ptr, sync::Once};

static REGISTER_CLASSES: Once = Once::new();
static mut POSITION_CLASS: *const Class = ptr::null();
static mut RANGE_CLASS: *const Class = ptr::null();

const INDEX_IVAR: &str = "zzIndex";
const START_IVAR: &str = "zzStart";
const END_IVAR: &str = "zzEnd";
const DELEGATE_IVAR: &str = "zzInputDelegate";
const TOKENIZER_IVAR: &str = "zzTokenizer";

const DIRECTION_LEFT: isize = 3;
const DIRECTION_UP: isize = 4;

pub(crate) unsafe fn register(decl: &mut ClassDecl) {
    REGISTER_CLASSES.call_once(register_classes);
    if let Some(protocol) = Protocol::get("UITextInput") {
        decl.add_protocol(protocol);
    }
    decl.add_ivar::<id>(DELEGATE_IVAR);
    decl.add_ivar::<id>(TOKENIZER_IVAR);
    decl.add_method(
        sel!(textInRange:),
        text_in_range as extern "C" fn(&Object, Sel, id) -> id,
    );
    decl.add_method(
        sel!(replaceRange:withText:),
        replace_range as extern "C" fn(&Object, Sel, id, id),
    );
    decl.add_method(
        sel!(selectedTextRange),
        selected_text_range as extern "C" fn(&Object, Sel) -> id,
    );
    decl.add_method(
        sel!(setSelectedTextRange:),
        ignore_object as extern "C" fn(&Object, Sel, id),
    );
    decl.add_method(
        sel!(markedTextRange),
        marked_text_range as extern "C" fn(&Object, Sel) -> id,
    );
    decl.add_method(
        sel!(markedTextStyle),
        nil_object as extern "C" fn(&Object, Sel) -> id,
    );
    decl.add_method(
        sel!(setMarkedTextStyle:),
        ignore_object as extern "C" fn(&Object, Sel, id),
    );
    decl.add_method(
        sel!(setMarkedText:selectedRange:),
        set_marked_text as extern "C" fn(&Object, Sel, id, NSRange),
    );
    decl.add_method(sel!(unmarkText), unmark_text as extern "C" fn(&Object, Sel));
    decl.add_method(
        sel!(beginningOfDocument),
        beginning_of_document as extern "C" fn(&Object, Sel) -> id,
    );
    decl.add_method(
        sel!(endOfDocument),
        end_of_document as extern "C" fn(&Object, Sel) -> id,
    );
    decl.add_method(
        sel!(textRangeFromPosition:toPosition:),
        range_between as extern "C" fn(&Object, Sel, id, id) -> id,
    );
    decl.add_method(
        sel!(positionFromPosition:offset:),
        position_from_offset as extern "C" fn(&Object, Sel, id, isize) -> id,
    );
    decl.add_method(
        sel!(positionFromPosition:inDirection:offset:),
        position_in_direction as extern "C" fn(&Object, Sel, id, isize, isize) -> id,
    );
    decl.add_method(
        sel!(comparePosition:toPosition:),
        compare_positions as extern "C" fn(&Object, Sel, id, id) -> isize,
    );
    decl.add_method(
        sel!(offsetFromPosition:toPosition:),
        offset_between as extern "C" fn(&Object, Sel, id, id) -> isize,
    );
    decl.add_method(
        sel!(inputDelegate),
        input_delegate as extern "C" fn(&Object, Sel) -> id,
    );
    decl.add_method(
        sel!(setInputDelegate:),
        set_input_delegate as extern "C" fn(&mut Object, Sel, id),
    );
    decl.add_method(
        sel!(tokenizer),
        tokenizer as extern "C" fn(&mut Object, Sel) -> id,
    );
    decl.add_method(
        sel!(positionWithinRange:farthestInDirection:),
        position_within_range as extern "C" fn(&Object, Sel, id, isize) -> id,
    );
    decl.add_method(
        sel!(characterRangeByExtendingPosition:inDirection:),
        extend_position as extern "C" fn(&Object, Sel, id, isize) -> id,
    );
    decl.add_method(
        sel!(baseWritingDirectionForPosition:inDirection:),
        writing_direction as extern "C" fn(&Object, Sel, id, isize) -> isize,
    );
    decl.add_method(
        sel!(setBaseWritingDirection:forRange:),
        set_writing_direction as extern "C" fn(&Object, Sel, isize, id),
    );
    decl.add_method(
        sel!(firstRectForRange:),
        first_rect_for_range as extern "C" fn(&Object, Sel, id) -> CGRect,
    );
    decl.add_method(
        sel!(caretRectForPosition:),
        caret_rect_for_position as extern "C" fn(&Object, Sel, id) -> CGRect,
    );
    decl.add_method(
        sel!(selectionRectsForRange:),
        selection_rects as extern "C" fn(&Object, Sel, id) -> id,
    );
    decl.add_method(
        sel!(closestPositionToPoint:),
        closest_position as extern "C" fn(&Object, Sel, CGPoint) -> id,
    );
    decl.add_method(
        sel!(closestPositionToPoint:withinRange:),
        closest_position_within as extern "C" fn(&Object, Sel, CGPoint, id) -> id,
    );
    decl.add_method(
        sel!(characterRangeAtPoint:),
        character_range_at_point as extern "C" fn(&Object, Sel, CGPoint) -> id,
    );
}

pub(crate) unsafe fn release(view: id) {
    let tokenizer: id = *(*view).get_ivar(TOKENIZER_IVAR);
    if !tokenizer.is_null() {
        let _: () = msg_send![tokenizer, release];
        (*view).set_ivar(TOKENIZER_IVAR, nil);
    }
}

pub(crate) fn composing(this: &Object) -> bool {
    with_handler(this, |handler| handler.marked_text_range().is_some()).unwrap_or(false)
}

fn register_classes() {
    unsafe {
        let mut position = ClassDecl::new("ZZTextPosition", class!(UITextPosition)).unwrap();
        position.add_ivar::<usize>(INDEX_IVAR);
        POSITION_CLASS = position.register();

        let mut range = ClassDecl::new("ZZTextRange", class!(UITextRange)).unwrap();
        range.add_ivar::<usize>(START_IVAR);
        range.add_ivar::<usize>(END_IVAR);
        range.add_method(
            sel!(start),
            range_start as extern "C" fn(&Object, Sel) -> id,
        );
        range.add_method(sel!(end), range_end as extern "C" fn(&Object, Sel) -> id);
        range.add_method(
            sel!(isEmpty),
            range_is_empty as extern "C" fn(&Object, Sel) -> BOOL,
        );
        RANGE_CLASS = range.register();
    }
}

fn position(index: usize) -> id {
    unsafe {
        let position: id = msg_send![POSITION_CLASS, new];
        (*position).set_ivar(INDEX_IVAR, index);
        msg_send![position, autorelease]
    }
}

fn index_of(position: id) -> Option<usize> {
    unsafe {
        if position.is_null() {
            return None;
        }
        let kind: BOOL = msg_send![position, isKindOfClass: POSITION_CLASS];
        (kind == YES).then(|| *(*position).get_ivar::<usize>(INDEX_IVAR))
    }
}

fn text_range(range: Range<usize>) -> id {
    unsafe {
        let text_range: id = msg_send![RANGE_CLASS, new];
        (*text_range).set_ivar(START_IVAR, range.start.min(range.end));
        (*text_range).set_ivar(END_IVAR, range.end.max(range.start));
        msg_send![text_range, autorelease]
    }
}

fn range_of(text_range: id) -> Option<Range<usize>> {
    unsafe {
        if text_range.is_null() {
            return None;
        }
        let kind: BOOL = msg_send![text_range, isKindOfClass: RANGE_CLASS];
        (kind == YES).then(|| {
            *(*text_range).get_ivar::<usize>(START_IVAR)..*(*text_range).get_ivar::<usize>(END_IVAR)
        })
    }
}

extern "C" fn range_start(this: &Object, _: Sel) -> id {
    position(unsafe { *this.get_ivar::<usize>(START_IVAR) })
}

extern "C" fn range_end(this: &Object, _: Sel) -> id {
    position(unsafe { *this.get_ivar::<usize>(END_IVAR) })
}

extern "C" fn range_is_empty(this: &Object, _: Sel) -> BOOL {
    let (start, end) = unsafe {
        (
            *this.get_ivar::<usize>(START_IVAR),
            *this.get_ivar::<usize>(END_IVAR),
        )
    };
    if start == end { YES } else { NO }
}

fn with_handler<R>(this: &Object, f: impl FnOnce(&mut PlatformInputHandler) -> R) -> Option<R> {
    let state = unsafe { crate::window::try_window_state(this) }?;
    let mut handler = state.borrow_mut().take_input_handler()?;
    let result = f(&mut handler);
    state.borrow_mut().restore_input_handler(handler);
    Some(result)
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

fn ns_text(text: &str) -> id {
    unsafe { crate::ns_string(&text.replace('\0', "")) }
}

fn rect(bounds: Bounds<Pixels>) -> CGRect {
    CGRect {
        origin: CGPoint {
            x: f32::from(bounds.origin.x) as f64,
            y: f32::from(bounds.origin.y) as f64,
        },
        size: CGSize {
            width: f32::from(bounds.size.width) as f64,
            height: f32::from(bounds.size.height) as f64,
        },
    }
}

extern "C" fn text_in_range(this: &Object, _: Sel, range: id) -> id {
    let Some(range) = range_of(range) else {
        return nil;
    };
    with_handler(this, |handler| text_for_range(handler, range))
        .flatten()
        .map_or(nil, |text| ns_text(&text))
}

extern "C" fn replace_range(this: &Object, _: Sel, range: id, text: id) {
    let text = unsafe { crate::nsstring_to_string(text) }.unwrap_or_default();
    let range = range_of(range);
    with_handler(this, |handler| handler.replace_text_in_range(range, &text));
}

extern "C" fn selected_text_range(this: &Object, _: Sel) -> id {
    with_handler(this, |handler| handler.selected_text_range(false))
        .flatten()
        .map_or(nil, |selection| text_range(selection.range))
}

extern "C" fn marked_text_range(this: &Object, _: Sel) -> id {
    with_handler(this, |handler| handler.marked_text_range())
        .flatten()
        .map_or(nil, text_range)
}

extern "C" fn nil_object(_: &Object, _: Sel) -> id {
    nil
}

extern "C" fn ignore_object(_: &Object, _: Sel, _: id) {}

extern "C" fn set_marked_text(this: &Object, _: Sel, text: id, selected: NSRange) {
    let text = unsafe { crate::nsstring_to_string(text) }.unwrap_or_default();
    let selected = (selected.location != isize::MAX as usize)
        .then(|| selected.location..selected.location + selected.length);
    with_handler(this, |handler| {
        handler.replace_and_mark_text_in_range(None, &text, selected)
    });
}

extern "C" fn unmark_text(this: &Object, _: Sel) {
    with_handler(this, |handler| {
        let Some(marked) = handler.marked_text_range() else {
            return;
        };
        match text_for_range(handler, marked.clone()) {
            Some(text) if !text.is_empty() => handler.replace_text_in_range(Some(marked), &text),
            _ => handler.unmark_text(),
        }
    });
}

extern "C" fn beginning_of_document(_: &Object, _: Sel) -> id {
    position(0)
}

extern "C" fn end_of_document(this: &Object, _: Sel) -> id {
    position(with_handler(this, document_len).unwrap_or(0))
}

extern "C" fn range_between(_: &Object, _: Sel, from: id, to: id) -> id {
    match (index_of(from), index_of(to)) {
        (Some(from), Some(to)) => text_range(from.min(to)..from.max(to)),
        _ => nil,
    }
}

fn offset_position(this: &Object, from: id, offset: isize) -> id {
    let Some(from) = index_of(from) else {
        return nil;
    };
    let len = with_handler(this, document_len).unwrap_or(0);
    match from.checked_add_signed(offset) {
        Some(index) if index <= len => position(index),
        _ => nil,
    }
}

extern "C" fn position_from_offset(this: &Object, _: Sel, from: id, offset: isize) -> id {
    offset_position(this, from, offset)
}

extern "C" fn position_in_direction(
    this: &Object,
    _: Sel,
    from: id,
    direction: isize,
    offset: isize,
) -> id {
    let offset = if matches!(direction, DIRECTION_LEFT | DIRECTION_UP) {
        -offset
    } else {
        offset
    };
    offset_position(this, from, offset)
}

extern "C" fn compare_positions(_: &Object, _: Sel, left: id, right: id) -> isize {
    match (index_of(left), index_of(right)) {
        (Some(left), Some(right)) => left.cmp(&right) as isize,
        _ => 0,
    }
}

extern "C" fn offset_between(_: &Object, _: Sel, from: id, to: id) -> isize {
    match (index_of(from), index_of(to)) {
        (Some(from), Some(to)) => to as isize - from as isize,
        _ => 0,
    }
}

extern "C" fn input_delegate(this: &Object, _: Sel) -> id {
    unsafe { *this.get_ivar(DELEGATE_IVAR) }
}

extern "C" fn set_input_delegate(this: &mut Object, _: Sel, delegate: id) {
    unsafe { this.set_ivar(DELEGATE_IVAR, delegate) };
}

extern "C" fn tokenizer(this: &mut Object, _: Sel) -> id {
    unsafe {
        let existing: id = *this.get_ivar(TOKENIZER_IVAR);
        if !existing.is_null() {
            return existing;
        }
        let tokenizer: id = msg_send![class!(UITextInputStringTokenizer), alloc];
        let tokenizer: id = msg_send![tokenizer, initWithTextInput: this as *mut Object];
        this.set_ivar(TOKENIZER_IVAR, tokenizer);
        tokenizer
    }
}

extern "C" fn position_within_range(_: &Object, _: Sel, range: id, direction: isize) -> id {
    range_of(range).map_or(nil, |range| {
        position(if matches!(direction, DIRECTION_LEFT | DIRECTION_UP) {
            range.start
        } else {
            range.end
        })
    })
}

extern "C" fn extend_position(this: &Object, _: Sel, from: id, direction: isize) -> id {
    let Some(from) = index_of(from) else {
        return nil;
    };
    if matches!(direction, DIRECTION_LEFT | DIRECTION_UP) {
        text_range(from.saturating_sub(1)..from)
    } else {
        let len = with_handler(this, document_len).unwrap_or(0);
        text_range(from..(from + 1).min(len.max(from)))
    }
}

extern "C" fn writing_direction(_: &Object, _: Sel, _: id, _: isize) -> isize {
    0
}

extern "C" fn set_writing_direction(_: &Object, _: Sel, _: isize, _: id) {}

extern "C" fn first_rect_for_range(this: &Object, _: Sel, range: id) -> CGRect {
    range_of(range)
        .and_then(|range| with_handler(this, |handler| handler.bounds_for_range(range)).flatten())
        .map(rect)
        .unwrap_or_default()
}

extern "C" fn caret_rect_for_position(this: &Object, _: Sel, at: id) -> CGRect {
    index_of(at)
        .and_then(|index| {
            with_handler(this, |handler| handler.bounds_for_range(index..index)).flatten()
        })
        .map(|bounds| {
            let mut caret = rect(bounds);
            caret.size.width = 2.0;
            caret
        })
        .unwrap_or_default()
}

extern "C" fn selection_rects(_: &Object, _: Sel, _: id) -> id {
    unsafe { msg_send![class!(NSArray), array] }
}

fn index_at(this: &Object, at: CGPoint) -> Option<usize> {
    with_handler(this, |handler| {
        handler.character_index_for_point(point(px(at.x as f32), px(at.y as f32)))
    })
    .flatten()
}

extern "C" fn closest_position(this: &Object, _: Sel, at: CGPoint) -> id {
    index_at(this, at).map_or(nil, position)
}

extern "C" fn closest_position_within(this: &Object, _: Sel, at: CGPoint, range: id) -> id {
    let Some(range) = range_of(range) else {
        return nil;
    };
    index_at(this, at).map_or(nil, |index| position(index.clamp(range.start, range.end)))
}

extern "C" fn character_range_at_point(this: &Object, _: Sel, at: CGPoint) -> id {
    index_at(this, at).map_or(nil, |index| text_range(index..index + 1))
}
