use std::{sync::Arc, time::Duration};

use gpui::{
    AnyElement, IntoElement, ParentElement as _, Pixels, SharedString, Styled as _, div, px,
};
use zz_ui::which_key::{WhichKeyHeader, WhichKeyRow, WhichKeyView};

use crate::mux::{client::MuxClient, prefix::display_keystroke};

const SHEET_MAX_WIDTH: f32 = 920.0;
const SHEET_INSET: f32 = 16.0;

pub(super) struct WhichKeySheet {
    header: WhichKeyHeader,
    rows: Arc<[WhichKeyRow]>,
}

impl WhichKeySheet {
    pub(super) fn build(mux: &MuxClient, table: &str) -> Option<Self> {
        let rows = zz_client::which_key::rows(
            mux.key_tables(),
            table,
            mux.prefix_option().unwrap_or("C-b"),
        );
        if rows.is_empty() {
            return None;
        }
        let prefix = if table == "prefix" {
            mux.canonical_prefix().unwrap_or_default()
        } else {
            String::new()
        };
        Some(Self {
            header: WhichKeyHeader {
                table: table.to_owned().into(),
                prefix: display_keystroke(&prefix),
                prefix_raw: prefix.into(),
            },
            rows: rows
                .into_iter()
                .map(|row| WhichKeyRow {
                    key: display_keystroke(&row.key),
                    raw: row.key.into(),
                    label: row.label.into(),
                    group: row
                        .group
                        .map(|group| SharedString::new_static(group.title())),
                    repeat: row.repeat,
                    yours: row.yours,
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

    pub(super) fn element(&self, bottom: Pixels) -> AnyElement {
        div()
            .absolute()
            .left_0()
            .right_0()
            .bottom(bottom)
            .px(px(SHEET_INSET))
            .flex()
            .justify_center()
            .child(
                div()
                    .w_full()
                    .max_w(px(SHEET_MAX_WIDTH))
                    .child(WhichKeyView::new(
                        self.header.clone(),
                        Arc::clone(&self.rows),
                    )),
            )
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
        for row in rows {
            assert!(
                crate::mux::prefix::display_keystroke(&row.key).is_some(),
                "{:?} has no key cap",
                row.key
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
