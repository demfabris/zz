//! What a live `display-popup` does when its owning client resizes.
//!
//! The pin keeps the position and size the popup asked for in `ppx`, `ppy`,
//! `psx` and `psy`, and `popup_resize_cb` rebuilds the live box from them on
//! every `MSG_RESIZE`: the size is the smaller of the request and the
//! viewport, the origin is the preferred one whenever the box fits there, and
//! a viewport that grows again restores exactly what was asked for.

#![cfg(all(unix, feature = "daemon"))]

mod overlay;

use zz_protocol::{CommandInvocation, PopupState};

use overlay::Overlays;

fn placement(state: &PopupState) -> (u16, u16, u16, u16) {
    (state.left, state.top, state.width, state.height)
}

fn chrome(state: &PopupState) -> (String, String, String, bool, bool, bool) {
    (
        state.title.clone(),
        state.style.clone(),
        state.border_style.clone(),
        state.close_on_exit,
        state.close_on_exit_zero,
        state.close_on_any_key,
    )
}

#[test]
fn a_resize_rebuilds_the_popup_from_what_it_asked_for() {
    let overlays = Overlays::start("popup-resize");
    let opening = overlays.spawn_command(CommandInvocation::new(
        "display-popup",
        [
            "-c",
            &overlays.client_name,
            "-w",
            "40",
            "-h",
            "10",
            "-x",
            "30",
            "-y",
            "20",
            "-T",
            "popup",
            "cat",
        ],
    ));

    let opened = overlays.await_popup();
    assert_eq!(placement(&opened), (30, 10, 40, 10));
    assert_eq!((opened.client_columns, opened.client_rows), (80, 24));

    overlays.resize_cells(30, 8, 9, 18);
    let squeezed = overlays.await_popup_matching(|state| state.client_columns == 30);
    assert_eq!(
        placement(&squeezed),
        (0, 0, 30, 8),
        "the live box is the smaller of the request and the viewport, parked so it fits"
    );
    assert_eq!((squeezed.client_columns, squeezed.client_rows), (30, 8));
    assert_eq!((squeezed.cell_width_px, squeezed.cell_height_px), (9, 18));
    assert_eq!(chrome(&squeezed), chrome(&opened));
    assert_eq!(squeezed.pane, opened.pane);
    assert_eq!(squeezed.border_lines, opened.border_lines);
    assert!(!squeezed.dead);

    overlays.resize(50, 24);
    let middle = overlays.await_popup_matching(|state| state.client_columns == 50);
    assert_eq!(
        placement(&middle),
        (10, 10, 40, 10),
        "a width that fits keeps the request and slides left only as far as it must"
    );

    overlays.resize(80, 24);
    let restored = overlays.await_popup_matching(|state| state.client_columns == 80);
    assert_eq!(
        placement(&restored),
        placement(&opened),
        "a viewport that grows back restores the preferred placement"
    );
    assert_eq!(chrome(&restored), chrome(&opened));

    overlays
        .spawn_command(CommandInvocation::new(
            "display-popup",
            ["-c", &overlays.client_name, "-C"],
        ))
        .join()
        .expect("the clearing thread")
        .expect("display-popup -C");
    opening
        .join()
        .expect("the display-popup thread")
        .expect_err("a popup cleared under the job reports the job's signal");
}

#[test]
fn a_borderless_popup_resizes_to_the_whole_viewport() {
    let overlays = Overlays::start("popup-resize-borderless");
    let opening = overlays.spawn_command(CommandInvocation::new(
        "display-popup",
        [
            "-c",
            &overlays.client_name,
            "-B",
            "-w",
            "60",
            "-h",
            "20",
            "-x",
            "0",
            "-y",
            "20",
            "cat",
        ],
    ));

    let opened = overlays.await_popup();
    assert_eq!(placement(&opened), (0, 0, 60, 20));

    overlays.resize(20, 6);
    let squeezed = overlays.await_popup_matching(|state| state.client_columns == 20);
    assert_eq!(placement(&squeezed), (0, 0, 20, 6));

    overlays
        .spawn_command(CommandInvocation::new(
            "display-popup",
            ["-c", &overlays.client_name, "-C"],
        ))
        .join()
        .expect("the clearing thread")
        .expect("display-popup -C");
    opening
        .join()
        .expect("the display-popup thread")
        .expect_err("a popup cleared under the job reports the job's signal");
}
