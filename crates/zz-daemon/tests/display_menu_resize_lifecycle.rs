//! What a live `display-menu` keeps when its owning client resizes.
//!
//! The pin answers `MSG_RESIZE` with `menu_resize_cb`, which moves the box
//! back inside the new viewport and touches nothing else — not the width, not
//! the rows, not the choice, not the commands — and never closes the menu, not
//! even when the box no longer fits.

#![cfg(all(unix, feature = "daemon"))]

mod overlay;

use zz_protocol::{CommandInvocation, InputMessage, MenuAction, MenuState};

use overlay::Overlays;

const ROW_NAME: &str = "a menu row wide enough to shed";

fn origin_for_viewport(origin: u16, extent: u16, available: u16) -> u16 {
    if origin.saturating_add(extent) > available {
        available.saturating_sub(extent)
    } else {
        origin
    }
}

fn rows(state: &MenuState) -> Vec<(String, Option<String>, bool)> {
    state
        .items
        .iter()
        .map(|item| {
            item.as_ref().map_or_else(
                || (String::new(), None, false),
                |item| (item.name.clone(), item.key.clone(), item.enabled),
            )
        })
        .collect()
}

fn wide_menu_args(client: &str) -> Vec<String> {
    let mut args = vec![
        "-c".to_owned(),
        client.to_owned(),
        "-O".to_owned(),
        "-C".to_owned(),
        "2".to_owned(),
        "-x".to_owned(),
        "40".to_owned(),
        "-y".to_owned(),
        "22".to_owned(),
        "-T".to_owned(),
        "resize".to_owned(),
    ];
    for index in 0..12u8 {
        args.push(format!("{ROW_NAME} {index}"));
        args.push(char::from(b'a' + index).to_string());
        args.push(format!("set-environment -g MENU_RESIZE_ROW row-{index}"));
    }
    args
}

#[test]
fn a_resize_moves_the_menu_and_keeps_everything_else() {
    let mut overlays = Overlays::start("menu-resize");
    let args = wide_menu_args(&overlays.client_name);
    let command = overlays.spawn_command(CommandInvocation::new("display-menu", args));

    let opened = overlays.await_menu();
    assert_eq!((opened.client_columns, opened.client_rows), (80, 24));
    assert_eq!(opened.selected, Some(2));
    assert!(opened.stay_open);
    assert_eq!(opened.items.len(), 12);

    overlays.resize_cells(60, 20, 9, 18);
    let narrow = overlays.await_menu_matching(|state| state.client_columns == 60);
    assert_eq!((narrow.client_columns, narrow.client_rows), (60, 20));
    assert_eq!(
        (narrow.width, narrow.height),
        (opened.width, opened.height),
        "the box keeps the size it was built with"
    );
    assert_eq!(rows(&narrow), rows(&opened));
    assert_eq!(narrow.selected, opened.selected);
    assert_eq!(narrow.stay_open, opened.stay_open);
    assert_eq!(narrow.title, opened.title);
    assert_eq!(narrow.style, opened.style);
    assert_eq!(narrow.selected_style, opened.selected_style);
    assert_eq!(narrow.border_style, opened.border_style);
    assert_eq!(narrow.border_lines, opened.border_lines);
    assert_eq!(
        narrow.left,
        origin_for_viewport(opened.left, opened.width, 60)
    );
    assert_eq!(
        narrow.top,
        origin_for_viewport(opened.top, opened.height, 20)
    );
    assert_eq!((narrow.cell_width_px, narrow.cell_height_px), (9, 18));
    assert!(
        narrow.left < opened.left || narrow.top < opened.top,
        "the probe wants a viewport the box no longer sits inside"
    );

    overlays.resize(30, 10);
    let tiny = overlays.await_menu_matching(|state| state.client_columns == 30);
    assert!(
        tiny.width > tiny.client_columns && tiny.height > tiny.client_rows,
        "the probe wants a menu the viewport can no longer hold"
    );
    assert_eq!(
        (tiny.left, tiny.top),
        (0, 0),
        "a box larger than the viewport parks at the origin"
    );
    assert_eq!((tiny.width, tiny.height), (opened.width, opened.height));
    assert_eq!(rows(&tiny), rows(&opened));
    assert_eq!(tiny.selected, opened.selected);

    overlays
        .client
        .send_input(InputMessage::Menu {
            action: MenuAction::Choose(4),
        })
        .expect("choose a row the resize never touched");
    command
        .join()
        .expect("the display-menu thread")
        .expect("display-menu");

    let chosen = overlays
        .commands
        .execute(CommandInvocation::new(
            "show-environment",
            ["-g", "MENU_RESIZE_ROW"],
        ))
        .expect("read the chosen row");
    assert_eq!(chosen.trim(), "MENU_RESIZE_ROW=row-4");
}
