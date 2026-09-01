//! How `display-menu` fits its rows into the cells a client has.
//!
//! The pin builds every row against `c->tty.sx - 4` (menu.c `menu_add_item`):
//! an action key is annotated when its bracketed form fits a quarter of that
//! room or the whole name still fits beside it, an overlong name is trimmed by
//! `format_trim_right` and marked with `>`, and a row keeps its action key even
//! when the annotation is dropped.

#![cfg(all(unix, feature = "daemon"))]

mod overlay;

use zz_protocol::{CommandInvocation, InputMessage, MenuAction};

use overlay::Overlays;

const CLIENT_COLUMNS: u16 = 40;

/// Resize the attached client and wait until the window extent a menu is laid
/// out against reports the new width.
fn settle_columns(overlays: &mut Overlays, columns: u16) {
    overlays.resize(columns, 24);
    let client = overlays.client_name.clone();
    let mut last = String::new();
    for _ in 0..400 {
        let reported = overlays
            .commands
            .execute(CommandInvocation::new(
                "display-message",
                ["-c", &client, "-p", "#{window_width}"],
            ))
            .expect("read the client width");
        if reported.trim() == columns.to_string() {
            return;
        }
        last.clone_from(&reported);
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!("the client never reported {columns} columns, last was {last:?}");
}

#[test]
fn menu_rows_fit_the_clients_cells_and_keep_their_action_keys() {
    let mut overlays = Overlays::start("menu-cell-layout");
    settle_columns(&mut overlays, CLIENT_COLUMNS);
    let overlong = "A".repeat(200);
    let wide_name = "B".repeat(30);
    let narrow_name = "C".repeat(10);
    let command = overlays.spawn_command(CommandInvocation::new(
        "display-menu",
        [
            "-c",
            &overlays.client_name,
            "-T",
            "",
            &overlong,
            "a",
            "",
            &wide_name,
            "M-Enter",
            "",
            &narrow_name,
            "M-Enter",
            "",
            "SHORT",
            "a",
            "",
        ],
    ));

    let state = overlays.await_menu_matching(|state| state.client_columns == CLIENT_COLUMNS);
    let rows = state
        .items
        .iter()
        .map(|item| item.as_ref().expect("no separator rows were asked for"))
        .collect::<Vec<_>>();
    assert_eq!(rows.len(), 4);

    assert_eq!(rows[0].name, format!("{}>", "A".repeat(31)));
    assert_eq!(rows[0].key.as_deref(), Some("a"));
    assert_eq!(rows[0].annotation.as_deref(), Some("a"));

    assert_eq!(rows[1].name, wide_name);
    assert_eq!(rows[1].key.as_deref(), Some("M-Enter"));
    assert_eq!(rows[1].annotation, None);

    assert_eq!(rows[2].name, narrow_name);
    assert_eq!(rows[2].key.as_deref(), Some("M-Enter"));
    assert_eq!(rows[2].annotation.as_deref(), Some("M-Enter"));

    assert_eq!(rows[3].name, "SHORT");
    assert_eq!(rows[3].key.as_deref(), Some("a"));
    assert_eq!(rows[3].annotation.as_deref(), Some("a"));

    assert_eq!(state.width, CLIENT_COLUMNS);
    assert!(state.width <= state.client_columns);

    overlays
        .client
        .send_input(InputMessage::Menu {
            action: MenuAction::Cancel,
        })
        .expect("close the menu");
    command
        .join()
        .expect("the display-menu thread")
        .expect("display-menu");
}

#[test]
fn the_title_seeds_the_menu_width_and_an_overwide_title_refuses_the_menu() {
    let mut overlays = Overlays::start("menu-cell-layout-title");
    settle_columns(&mut overlays, CLIENT_COLUMNS);
    let wide_title = "T".repeat(50);
    let refused = overlays
        .commands
        .execute(CommandInvocation::new(
            "display-menu",
            [
                "-c",
                &overlays.client_name,
                "-T",
                &wide_title,
                "SHORT",
                "a",
                "",
            ],
        ))
        .expect("a refused menu still completes");
    assert_eq!(refused, "");

    let fitting_title = "T".repeat(20);
    let command = overlays.spawn_command(CommandInvocation::new(
        "display-menu",
        [
            "-c",
            &overlays.client_name,
            "-T",
            &fitting_title,
            "SHORT",
            "a",
            "",
        ],
    ));
    let state = overlays.await_menu_matching(|state| state.client_columns == CLIENT_COLUMNS);
    assert_eq!(state.title, fitting_title);
    assert_eq!(state.width, 24);

    overlays
        .client
        .send_input(InputMessage::Menu {
            action: MenuAction::Cancel,
        })
        .expect("close the menu");
    command
        .join()
        .expect("the display-menu thread")
        .expect("display-menu");
}

#[test]
fn a_long_row_still_opens_a_menu_a_narrow_client_can_show() {
    let mut overlays = Overlays::start("menu-cell-layout-narrow");
    settle_columns(&mut overlays, 24);
    let overlong = "N".repeat(120);
    let command = overlays.spawn_command(CommandInvocation::new(
        "display-menu",
        ["-c", &overlays.client_name, "-T", "", &overlong, "a", ""],
    ));

    let state = overlays.await_menu_matching(|state| state.client_columns == 24);
    let row = state.items[0].as_ref().expect("the only row");
    assert_eq!(row.name, format!("{}>", "N".repeat(15)));
    assert_eq!(row.annotation.as_deref(), Some("a"));
    assert_eq!(state.width, 24);

    overlays
        .client
        .send_input(InputMessage::Menu {
            action: MenuAction::Cancel,
        })
        .expect("close the menu");
    command
        .join()
        .expect("the display-menu thread")
        .expect("display-menu");
}
