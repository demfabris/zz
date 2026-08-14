use std::time::{SystemTime, UNIX_EPOCH};

use gpui::{AnyElement, Entity, MouseButton, prelude::*};
use zz_protocol::{ChooseBufferAction, ChooseBufferItem, ChooseBufferState, InputMessage};
use zz_terminal::KeyInput;

use crate::{
    chooser::{Chooser, ChooserHint, ChooserRowTheme, ChooserSearch, ChooserSpec},
    mux::client::MuxClient,
    terminal::view::TERMINAL_FONT,
};
use zz_ui::chooser::buffer_chooser_row;

const BUFFER_HINTS: &[ChooserHint] = &[
    ChooserHint {
        keys: &["up", "down"],
        label: "navigate",
    },
    ChooserHint {
        keys: &["enter"],
        label: "paste",
    },
    ChooserHint {
        keys: &["d"],
        label: "delete",
    },
    ChooserHint {
        keys: &["/"],
        label: "search",
    },
    ChooserHint {
        keys: &["escape"],
        label: "close",
    },
];

pub(crate) struct BufferChooser;

pub(crate) type ChooseBufferView = Chooser<BufferChooser>;

impl ChooserSpec for BufferChooser {
    type State = ChooseBufferState;
    type Item = ChooseBufferItem;
    type Action = ChooseBufferAction;

    const OVERLAY_ID: &'static str = "choose-buffer-overlay";
    const MODAL_ID: &'static str = "choose-buffer-modal";
    const ROWS_ID: &'static str = "choose-buffer-rows";
    const ROW_ID: &'static str = "choose-buffer-row";
    const CLOSE_ID: &'static str = "choose-buffer-close";
    const WIDTH: f32 = 0.76;
    const MAX_WIDTH: f32 = 840.0;
    const HEIGHT: f32 = 0.68;
    const MIN_HEIGHT: f32 = 280.0;
    const MAX_HEIGHT: f32 = 580.0;
    const HINTS: &'static [ChooserHint] = BUFFER_HINTS;

    fn state(mux: &MuxClient) -> Option<Self::State> {
        mux.choose_buffer().cloned()
    }

    fn selected(state: &Self::State) -> u32 {
        state.selected
    }

    fn items(state: &Self::State) -> &[Self::Item] {
        &state.items
    }

    fn search(state: &Self::State) -> Option<ChooserSearch<'_>> {
        state.search.as_ref().map(|search| ChooserSearch {
            query: &search.query,
            reverse: search.reverse,
        })
    }

    fn title(_: &Self::State) -> &'static str {
        "Paste buffer"
    }

    fn subtitle(_: &Self::State, count: usize) -> String {
        format!("{count} buffers · daemon-owned")
    }

    fn row(
        item: Self::Item,
        index: usize,
        selected: bool,
        mux: Entity<MuxClient>,
        theme: ChooserRowTheme,
    ) -> AnyElement {
        buffer_row(item, index, selected, mux, theme)
    }

    fn key(input: KeyInput) -> Self::Action {
        ChooseBufferAction::Key(input)
    }

    fn search_append(text: String) -> Self::Action {
        ChooseBufferAction::SearchAppend(text)
    }

    fn close() -> Self::Action {
        ChooseBufferAction::Close
    }

    fn send(mux: &MuxClient, action: Self::Action) {
        mux.send_input(InputMessage::ChooseBuffer { action });
    }
}

fn buffer_row(
    item: ChooseBufferItem,
    index: usize,
    selected: bool,
    mux: Entity<MuxClient>,
    theme: ChooserRowTheme,
) -> AnyElement {
    buffer_chooser_row(
        BufferChooser::ROW_ID,
        index,
        item.name,
        item.preview,
        format_buffer_size(item.size_bytes),
        format_buffer_age(item.created_unix_seconds),
        selected,
        theme,
        TERMINAL_FONT,
    )
    .on_mouse_down(MouseButton::Left, move |event, _, cx| {
        let index = u32::try_from(index).unwrap_or(u32::MAX);
        BufferChooser::send(
            mux.read(cx),
            if event.click_count >= 2 {
                ChooseBufferAction::PasteIndex(index)
            } else {
                ChooseBufferAction::Select(index)
            },
        );
        cx.stop_propagation();
    })
    .into_any_element()
}

fn format_buffer_size(bytes: u64) -> String {
    const KIB: u64 = 1_024;
    const MIB: u64 = KIB * 1_024;
    const GIB: u64 = MIB * 1_024;
    if bytes < KIB {
        return format!("{bytes} B");
    }
    let (unit, label) = if bytes < MIB {
        (KIB, "KiB")
    } else if bytes < GIB {
        (MIB, "MiB")
    } else {
        (GIB, "GiB")
    };
    let tenths = bytes.saturating_mul(10).saturating_add(unit / 2) / unit;
    format!("{}.{:01} {label}", tenths / 10, tenths % 10)
}

fn format_buffer_age(created_unix_seconds: u64) -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let age = now.saturating_sub(created_unix_seconds);
    match age {
        0..=59 => format!("{age}s"),
        60..=3_599 => format!("{}m", age / 60),
        3_600..=86_399 => format!("{}h", age / 3_600),
        _ => format!("{}d", age / 86_400),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_and_ages_are_compact() {
        assert_eq!(format_buffer_size(999), "999 B");
        assert_eq!(format_buffer_size(1_536), "1.5 KiB");
        assert_eq!(format_buffer_age(u64::MAX), "0s");
    }
}
