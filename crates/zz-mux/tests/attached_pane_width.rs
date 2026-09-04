//! `rendering.geometry-residue`'s daemon-side probe.
//!
//! An Interactive client under `latest`, `largest` or `smallest` measures its
//! pane in pixels and reports the floored grid. tmux never learns a pane size
//! that way: a client's tty sizes the window, `layout_fix_panes` hands every
//! pane its cell, and `window_pane_resize` moves that cell, the pane's screen
//! and the pty together, so on pinned tmux d77c9dc6 a pane's `stty size` is
//! always `#{pane_height} #{pane_width}` - measured on a 199x49 window split
//! `-h`, where both panes report 99x49 and both shells see `49 99`.
//!
//! zz has no tty size for a GUI client, only the per-pane grid, so it
//! back-solves the window extent from the report. That inverse is exact on an
//! axis the pane spans in full and a ratio on an axis it shares with siblings,
//! where one cell of drift on a narrow pane would move the window by many, so
//! the shared axis swallows a single cell.

use zz_mux::{ExecutionContext, MuxEngine};
use zz_protocol::CommandInvocation;

fn command(name: &str, args: &[&str]) -> CommandInvocation {
    CommandInvocation::new(name, args.iter().copied())
}

fn format_of(engine: &mut MuxEngine, context: &mut ExecutionContext, format: &str) -> String {
    engine
        .execute(context, &command("display-message", &["-p", format]))
        .expect("display-message")
        .output
        .trim_end()
        .to_owned()
}

fn pane_family(engine: &mut MuxEngine, context: &mut ExecutionContext) -> String {
    format_of(
        engine,
        context,
        "#{pane_width}x#{pane_height} #{pane_left},#{pane_top} #{pane_right},#{pane_bottom} \
         #{pane_at_left}#{pane_at_top}#{pane_at_right}#{pane_at_bottom}",
    )
}

fn select(engine: &mut MuxEngine, context: &mut ExecutionContext, pane: &str) {
    engine
        .execute(context, &command("select-pane", &["-t", pane]))
        .expect("select-pane");
}

#[test]
fn an_unsplit_window_takes_the_client_measurement_exactly() {
    let mut engine = MuxEngine::default();
    let mut context = ExecutionContext::default();
    engine
        .execute(&mut context, &command("new-session", &["-s", "work"]))
        .unwrap();
    let pane = context.pane.unwrap();

    assert!(engine.set_pane_geometry(pane, 200, 50));
    assert_eq!(engine.pane_geometry(pane), Some((200, 50)));

    assert!(engine.set_pane_geometry(pane, 199, 49));
    assert_eq!(engine.pane_geometry(pane), Some((199, 49)));
    assert_eq!(
        format_of(&mut engine, &mut context, "#{pane_width}x#{pane_height}"),
        "199x49"
    );
    assert_eq!(
        format_of(
            &mut engine,
            &mut context,
            "#{window_width}x#{window_height}"
        ),
        "199x49"
    );
    assert_eq!(
        pane_family(&mut engine, &mut context),
        "199x49 0,0 198,48 1111"
    );
    assert!(
        format_of(&mut engine, &mut context, "#{window_layout}").ends_with("199x49,0,0,0"),
        "window_layout encodes the same cell"
    );

    assert!(!engine.set_pane_geometry(pane, 199, 49));
}

#[test]
fn a_one_cell_measurement_on_a_narrow_pane_would_move_the_window_by_many() {
    let mut engine = MuxEngine::default();
    let mut context = ExecutionContext::default();
    engine
        .execute(&mut context, &command("new-session", &["-s", "work"]))
        .unwrap();
    let first = context.pane.unwrap();
    engine.set_pane_geometry(first, 200, 50);
    engine
        .execute(&mut context, &command("split-window", &["-h", "-l", "10"]))
        .unwrap();
    let narrow = context.pane.unwrap();
    assert_eq!(engine.pane_geometry(narrow), Some((10, 50)));

    assert!(!engine.set_pane_geometry(narrow, 9, 50));
    assert_eq!(
        format_of(
            &mut engine,
            &mut context,
            "#{window_width}x#{window_height}"
        ),
        "200x50"
    );

    assert!(engine.set_pane_geometry(narrow, 8, 50));
    assert_eq!(
        format_of(
            &mut engine,
            &mut context,
            "#{window_width}x#{window_height}"
        ),
        "160x50"
    );
}

#[test]
fn a_shared_axis_swallows_a_cell_while_the_spanned_axis_stays_exact() {
    let mut engine = MuxEngine::default();
    let mut context = ExecutionContext::default();
    engine
        .execute(&mut context, &command("new-session", &["-s", "work"]))
        .unwrap();
    let first = context.pane.unwrap();
    engine.set_pane_geometry(first, 200, 50);
    engine
        .execute(&mut context, &command("split-window", &["-h", "-l", "10"]))
        .unwrap();
    let narrow = context.pane.unwrap();

    assert!(engine.set_pane_geometry(narrow, 9, 45));
    assert_eq!(
        format_of(
            &mut engine,
            &mut context,
            "#{window_width}x#{window_height}"
        ),
        "200x45"
    );
    assert_eq!(engine.pane_geometry(narrow), Some((10, 45)));
    assert_eq!(
        pane_family(&mut engine, &mut context),
        "10x45 190,0 199,44 0111"
    );
    select(&mut engine, &mut context, &first.to_string());
    assert_eq!(
        pane_family(&mut engine, &mut context),
        "189x45 0,0 188,44 1101"
    );
    assert!(
        format_of(&mut engine, &mut context, "#{window_layout}")
            .contains("200x45,0,0{189x45,0,0,0,10x45,190,0,1}"),
        "window_layout encodes the same cells"
    );
}
