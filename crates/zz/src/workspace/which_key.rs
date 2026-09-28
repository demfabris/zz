use std::{sync::Arc, time::Duration};

use gpui::{
    AnyElement, IntoElement, ParentElement as _, Pixels, SharedString, Size, Styled as _, div, px,
    size,
};
use zz_ui::which_key::{WhichKeyCap, WhichKeyHeader, WhichKeyRow, WhichKeyView};

use crate::mux::{client::MuxClient, prefix::display_keystroke};

const SHEET_MAX_WIDTH: f32 = 1360.0;
const SHEET_INSET: f32 = 16.0;
const SHEET_GAP: f32 = 8.0;

pub(super) struct WhichKeySheet {
    header: WhichKeyHeader,
    rows: Arc<[WhichKeyRow]>,
}

fn cap(keys: &[String], yours: bool) -> WhichKeyCap {
    let parsed: Option<Vec<_>> = keys.iter().map(|key| display_keystroke(key)).collect();
    WhichKeyCap {
        keys: parsed.unwrap_or_default(),
        raw: keys.join(" ").into(),
        yours,
    }
}

impl WhichKeySheet {
    pub(super) fn build(mux: &MuxClient, table: &str, all: bool) -> Option<Self> {
        let prefix = mux.canonical_prefix();
        let short = table == "prefix" && !all;
        let rows: Vec<_> =
            zz_client::which_key::rows(mux.key_tables(), table, prefix.as_deref().unwrap_or("C-b"))
                .into_iter()
                .filter(|row| row.core || !short)
                .collect();
        if rows.is_empty() {
            return None;
        }
        let more = short
            .then(|| {
                mux.prefix_bindings()
                    .iter()
                    .find(|binding| zz_client::which_key::opens_all_keys(&binding.commands))
            })
            .flatten()
            .map(|binding| cap(std::slice::from_ref(&binding.key), false));
        let prefix = if table == "prefix" {
            prefix.unwrap_or_default()
        } else {
            String::new()
        };
        Some(Self {
            header: WhichKeyHeader {
                table: table.to_owned().into(),
                prefix: display_keystroke(&prefix),
                prefix_raw: prefix.into(),
                more,
            },
            rows: rows
                .into_iter()
                .map(|row| WhichKeyRow {
                    id: row.first_key().to_owned().into(),
                    caps: row
                        .keys
                        .iter()
                        .map(|set| cap(&set.keys, set.yours))
                        .collect(),
                    label: row.label.into(),
                    group: row
                        .group
                        .map(|group| SharedString::new_static(group.title())),
                    repeat: row.repeat,
                })
                .collect(),
        })
    }

    #[cfg(test)]
    pub(super) fn header(&self) -> &WhichKeyHeader {
        &self.header
    }

    #[cfg(test)]
    pub(super) fn rows(&self) -> &[WhichKeyRow] {
        &self.rows
    }

    pub(super) fn element(&self, top: Pixels, bottom: Pixels, canvas: Size<Pixels>) -> AnyElement {
        let available = size(
            (canvas.width - px(2.0 * SHEET_INSET)).min(px(SHEET_MAX_WIDTH)),
            canvas.height - px(2.0 * SHEET_GAP),
        );
        div()
            .absolute()
            .left_0()
            .right_0()
            .top(top + px(SHEET_GAP))
            .bottom(bottom + px(SHEET_GAP))
            .px(px(SHEET_INSET))
            .flex()
            .flex_col()
            .justify_end()
            .items_center()
            .child(WhichKeyView::new(self.header.clone(), Arc::clone(&self.rows)).fit(available))
            .into_any_element()
    }
}

pub(super) fn delay(milliseconds: f32) -> Option<Duration> {
    let milliseconds = milliseconds.round();
    (milliseconds.is_finite() && milliseconds > 0.0)
        .then(|| Duration::from_secs_f64(f64::from(milliseconds) / 1000.0))
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    #[test]
    fn every_stock_prefix_row_gets_a_key_cap() {
        let tables = zz_protocol::KeyTables::default().snapshot();
        let rows = zz_client::which_key::rows(&tables, "prefix", "C-b");
        assert!(!rows.is_empty());
        for key in rows
            .iter()
            .flat_map(|row| row.keys.iter().flat_map(|set| &set.keys))
        {
            assert!(
                crate::mux::prefix::display_keystroke(key).is_some(),
                "{key:?} has no key cap"
            );
        }
    }

    #[test]
    fn delay_rounds_to_milliseconds_and_zero_turns_it_off() {
        assert_eq!(super::delay(0.0), None);
        assert_eq!(super::delay(0.4), None);
        assert_eq!(super::delay(400.0), Some(Duration::from_millis(400)));
        assert_eq!(super::delay(249.6), Some(Duration::from_millis(250)));
        assert_eq!(super::delay(2000.0), Some(Duration::from_secs(2)));
    }
}
