//! `rendering.geometry-residue`'s daemon-side probe.
//!
//! An Interactive client under `latest`, `largest` or `smallest` measures its
//! pane in pixels and reports the floored grid, which is what reaches the PTY.
//! `set_pane_geometry` turns that report into a window extent, and the extent
//! is what `#{pane_width}` is laid out from. It deliberately swallows a
//! difference of one cell, so the two numbers can stand one cell apart.

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

#[test]
fn a_one_cell_client_measurement_leaves_pane_width_where_the_layout_put_it() {
    let mut engine = MuxEngine::default();
    let mut context = ExecutionContext::default();
    engine
        .execute(&mut context, &command("new-session", &["-s", "work"]))
        .unwrap();
    let pane = context.pane.unwrap();

    assert!(engine.set_pane_geometry(pane, 200, 50));
    assert_eq!(engine.pane_geometry(pane), Some((200, 50)));
    assert_eq!(
        format_of(&mut engine, &mut context, "#{pane_width}x#{pane_height}"),
        "200x50"
    );

    assert!(!engine.set_pane_geometry(pane, 199, 49));
    assert_eq!(engine.pane_geometry(pane), Some((200, 50)));
    assert_eq!(
        format_of(&mut engine, &mut context, "#{pane_width}x#{pane_height}"),
        "200x50"
    );
    assert_eq!(
        format_of(
            &mut engine,
            &mut context,
            "#{window_width}x#{window_height}"
        ),
        "200x50"
    );

    assert!(engine.set_pane_geometry(pane, 198, 48));
    assert_eq!(engine.pane_geometry(pane), Some((198, 48)));
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
