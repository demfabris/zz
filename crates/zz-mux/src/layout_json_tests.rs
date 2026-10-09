use zz_protocol::{CommandInvocation, PaneId, ServerError};

use crate::{
    CellLayout, ExecutionContext, LayoutFormat, LeafState, MuxEngine, legacy_layout,
    legacy_layouts_in,
};

struct Probe {
    engine: MuxEngine,
    context: ExecutionContext,
}

impl Probe {
    fn new() -> Self {
        let mut probe = Self {
            engine: MuxEngine::default(),
            context: ExecutionContext::default(),
        };
        probe.run(&["new-session", "-s", "w"]);
        probe
    }

    fn try_run(&mut self, words: &[&str]) -> Result<String, ServerError> {
        self.engine
            .execute(
                &mut self.context,
                &CommandInvocation::new(words[0], words[1..].iter().copied()),
            )
            .map(|execution| execution.output.to_string())
    }

    fn run(&mut self, words: &[&str]) -> String {
        self.try_run(words)
            .unwrap_or_else(|error| panic!("{words:?}: {error:?}"))
    }

    fn fmt(&mut self, format: &str) -> String {
        self.run(&["display-message", "-p", "-t", "w", format])
            .trim_end()
            .to_owned()
    }
}

const SPLIT: &str = r#"{"V":2,"L":{"t":"h","w":80,"h":24,"x":0,"y":0,"c":[{"t":"p","w":40,"h":24,"x":0,"y":0,"l":2,"i":0,"I":"%0"},{"t":"v","w":39,"h":24,"x":41,"y":0,"c":[{"t":"p","w":39,"h":12,"x":41,"y":0,"l":1,"i":1,"I":"%1"},{"t":"h","w":39,"h":11,"x":41,"y":13,"c":[{"t":"p","w":19,"h":11,"x":41,"y":13,"l":0,"i":2,"I":"%2"},{"t":"p","w":19,"h":11,"x":61,"y":13,"a":true,"i":3,"I":"%3"}]}]}]}}"#;
const ZOOMED: &str = r#"{"V":2,"L":{"t":"h","w":80,"h":24,"x":0,"y":0,"c":[{"t":"p","w":40,"h":24,"x":0,"y":0,"l":0,"i":0,"I":"%0"},{"t":"v","w":39,"h":24,"x":41,"y":0,"c":[{"t":"p","w":39,"h":12,"x":41,"y":0,"a":true,"i":1,"I":"%1"},{"t":"h","w":39,"h":11,"x":41,"y":13,"c":[{"t":"p","w":19,"h":11,"x":41,"y":13,"l":2,"i":2,"I":"%2"},{"t":"p","w":19,"h":11,"x":61,"y":13,"l":1,"i":3,"I":"%3"}]}]}]}}"#;

#[test]
fn window_layouts_print_the_pinned_json_v2_bytes() {
    let mut probe = Probe::new();
    let single = r#"{"V":2,"L":{"t":"p","w":80,"h":24,"x":0,"y":0,"a":true,"i":0,"I":"%0"}}"#;
    assert_eq!(
        probe.fmt("#{window_layout}|#{window_visible_layout}"),
        format!("{single}|{single}")
    );
    assert!(
        probe
            .run(&["list-windows", "-t", "w"])
            .contains(&format!("[layout {single}]"))
    );
    probe.run(&["split-window", "-h", "-t", "w:0.0"]);
    probe.run(&["split-window", "-v", "-t", "w:0.1"]);
    probe.run(&["split-window", "-h", "-t", "w:0.2"]);
    assert_eq!(probe.fmt("#{window_layout}"), SPLIT);
    probe.run(&["select-pane", "-t", "w:0.0"]);
    assert_eq!(
        probe.fmt("#{window_layout}"),
        r#"{"V":2,"L":{"t":"h","w":80,"h":24,"x":0,"y":0,"c":[{"t":"p","w":40,"h":24,"x":0,"y":0,"a":true,"i":0,"I":"%0"},{"t":"v","w":39,"h":24,"x":41,"y":0,"c":[{"t":"p","w":39,"h":12,"x":41,"y":0,"l":2,"i":1,"I":"%1"},{"t":"h","w":39,"h":11,"x":41,"y":13,"c":[{"t":"p","w":19,"h":11,"x":41,"y":13,"l":1,"i":2,"I":"%2"},{"t":"p","w":19,"h":11,"x":61,"y":13,"l":0,"i":3,"I":"%3"}]}]}]}}"#
    );
    probe.run(&["resize-pane", "-Z", "-t", "w:0.1"]);
    assert_eq!(probe.fmt("#{window_layout}"), ZOOMED);
    assert_eq!(
        probe.fmt("#{window_visible_layout}"),
        r#"{"V":2,"L":{"t":"p","w":80,"h":24,"x":0,"y":0,"a":true,"i":1,"I":"%1"}}"#
    );
    assert!(
        probe
            .run(&["list-windows", "-t", "w"])
            .contains(&format!("[layout {ZOOMED}]"))
    );
    probe.run(&["resize-pane", "-Z", "-t", "w:0.1"]);
    probe.run(&["resize-window", "-t", "w", "-x", "100", "-y", "30"]);
    assert_eq!(
        probe.fmt("#{window_layout}"),
        r#"{"V":2,"L":{"t":"h","w":100,"h":30,"x":0,"y":0,"c":[{"t":"p","w":50,"h":30,"x":0,"y":0,"l":0,"i":0,"I":"%0"},{"t":"v","w":49,"h":30,"x":51,"y":0,"c":[{"t":"p","w":49,"h":15,"x":51,"y":0,"a":true,"i":1,"I":"%1"},{"t":"h","w":49,"h":14,"x":51,"y":16,"c":[{"t":"p","w":29,"h":14,"x":51,"y":16,"l":2,"i":2,"I":"%2"},{"t":"p","w":19,"h":14,"x":81,"y":16,"l":1,"i":3,"I":"%3"}]}]}]}}"#
    );
    let presets = [
        (
            "even-horizontal",
            r#"{"V":2,"L":{"t":"h","w":100,"h":30,"x":0,"y":0,"c":[{"t":"p","w":25,"h":30,"x":0,"y":0,"l":0,"i":0,"I":"%0"},{"t":"p","w":24,"h":30,"x":26,"y":0,"a":true,"i":1,"I":"%1"},{"t":"p","w":24,"h":30,"x":51,"y":0,"l":2,"i":2,"I":"%2"},{"t":"p","w":24,"h":30,"x":76,"y":0,"l":1,"i":3,"I":"%3"}]}}"#,
        ),
        (
            "even-vertical",
            r#"{"V":2,"L":{"t":"v","w":100,"h":30,"x":0,"y":0,"c":[{"t":"p","w":100,"h":7,"x":0,"y":0,"l":0,"i":0,"I":"%0"},{"t":"p","w":100,"h":7,"x":0,"y":8,"a":true,"i":1,"I":"%1"},{"t":"p","w":100,"h":7,"x":0,"y":16,"l":2,"i":2,"I":"%2"},{"t":"p","w":100,"h":6,"x":0,"y":24,"l":1,"i":3,"I":"%3"}]}}"#,
        ),
        (
            "main-horizontal",
            r#"{"V":2,"L":{"t":"v","w":100,"h":30,"x":0,"y":0,"c":[{"t":"p","w":100,"h":24,"x":0,"y":0,"l":0,"i":0,"I":"%0"},{"t":"h","w":100,"h":5,"x":0,"y":25,"c":[{"t":"p","w":33,"h":5,"x":0,"y":25,"a":true,"i":1,"I":"%1"},{"t":"p","w":33,"h":5,"x":34,"y":25,"l":2,"i":2,"I":"%2"},{"t":"p","w":32,"h":5,"x":68,"y":25,"l":1,"i":3,"I":"%3"}]}]}}"#,
        ),
        (
            "main-vertical",
            r#"{"V":2,"L":{"t":"h","w":100,"h":30,"x":0,"y":0,"c":[{"t":"p","w":80,"h":30,"x":0,"y":0,"l":0,"i":0,"I":"%0"},{"t":"v","w":19,"h":30,"x":81,"y":0,"c":[{"t":"p","w":19,"h":10,"x":81,"y":0,"a":true,"i":1,"I":"%1"},{"t":"p","w":19,"h":9,"x":81,"y":11,"l":2,"i":2,"I":"%2"},{"t":"p","w":19,"h":9,"x":81,"y":21,"l":1,"i":3,"I":"%3"}]}]}}"#,
        ),
        (
            "tiled",
            r#"{"V":2,"L":{"t":"v","w":100,"h":30,"x":0,"y":0,"c":[{"t":"h","w":100,"h":14,"x":0,"y":0,"c":[{"t":"p","w":49,"h":14,"x":0,"y":0,"l":0,"i":0,"I":"%0"},{"t":"p","w":50,"h":14,"x":50,"y":0,"a":true,"i":1,"I":"%1"}]},{"t":"h","w":100,"h":15,"x":0,"y":15,"c":[{"t":"p","w":49,"h":15,"x":0,"y":15,"l":2,"i":2,"I":"%2"},{"t":"p","w":50,"h":15,"x":50,"y":15,"l":1,"i":3,"I":"%3"}]}]}}"#,
        ),
        (
            "main-horizontal-mirrored",
            r#"{"V":2,"L":{"t":"v","w":100,"h":30,"x":0,"y":0,"c":[{"t":"h","w":100,"h":5,"x":0,"y":0,"c":[{"t":"p","w":33,"h":5,"x":0,"y":0,"a":true,"i":1,"I":"%1"},{"t":"p","w":33,"h":5,"x":34,"y":0,"l":2,"i":2,"I":"%2"},{"t":"p","w":32,"h":5,"x":68,"y":0,"l":1,"i":3,"I":"%3"}]},{"t":"p","w":100,"h":24,"x":0,"y":6,"l":0,"i":0,"I":"%0"}]}}"#,
        ),
        (
            "main-vertical-mirrored",
            r#"{"V":2,"L":{"t":"h","w":100,"h":30,"x":0,"y":0,"c":[{"t":"v","w":19,"h":30,"x":0,"y":0,"c":[{"t":"p","w":19,"h":10,"x":0,"y":0,"a":true,"i":1,"I":"%1"},{"t":"p","w":19,"h":9,"x":0,"y":11,"l":2,"i":2,"I":"%2"},{"t":"p","w":19,"h":9,"x":0,"y":21,"l":1,"i":3,"I":"%3"}]},{"t":"p","w":80,"h":30,"x":20,"y":0,"l":0,"i":0,"I":"%0"}]}}"#,
        ),
    ];
    for (preset, expected) in presets {
        probe.run(&["select-layout", "-t", "w", preset]);
        assert_eq!(probe.fmt("#{window_layout}"), expected, "{preset}");
    }
    probe.run(&["set-window-option", "-t", "w", "pane-base-index", "3"]);
    assert_eq!(
        probe.fmt("#{window_layout}"),
        r#"{"V":2,"L":{"t":"h","w":100,"h":30,"x":0,"y":0,"c":[{"t":"v","w":19,"h":30,"x":0,"y":0,"c":[{"t":"p","w":19,"h":10,"x":0,"y":0,"a":true,"i":4,"I":"%1"},{"t":"p","w":19,"h":9,"x":0,"y":11,"l":2,"i":5,"I":"%2"},{"t":"p","w":19,"h":9,"x":0,"y":21,"l":1,"i":6,"I":"%3"}]},{"t":"p","w":80,"h":30,"x":20,"y":0,"l":0,"i":3,"I":"%0"}]}}"#
    );
    let window = probe.context.window.unwrap();
    let snapshot = probe.engine.state.snapshot();
    let snapshot_window = &snapshot.sessions[0].windows[0];
    assert_eq!(snapshot_window.id, window);
    assert_eq!(snapshot_window.layout_dump, probe.fmt("#{window_layout}"));
}

#[test]
fn control_formats_print_the_v1_compat_copy() {
    let mut probe = Probe::new();
    probe.run(&["split-window", "-h", "-t", "w:0.0"]);
    probe.run(&["split-window", "-v", "-t", "w:0.1"]);
    probe.run(&["split-window", "-h", "-t", "w:0.2"]);
    probe.run(&["select-pane", "-t", "w:0.0"]);
    probe.run(&["resize-pane", "-Z", "-t", "w:0.1"]);
    let layout = "1558,80x24,0,0{40x24,0,0,0,39x24,41,0[39x12,41,0,1,39x11,41,13{19x11,41,13,2,19x11,61,13,3}]}";
    let shown = probe.fmt("#{window_layout} #{window_visible_layout}");
    assert_eq!(
        legacy_layouts_in(&shown),
        format!("{layout} b25e,80x24,0,0,1")
    );
    assert_eq!(legacy_layout(ZOOMED), layout);
    assert_eq!(probe.fmt("#{window_layout}"), ZOOMED);
    let listed = probe.run(&["list-windows", "-t", "w"]);
    let converted = legacy_layouts_in(&listed);
    assert!(
        converted.contains(&format!("[layout {layout}] @")),
        "{converted}"
    );
    assert_eq!(legacy_layouts_in("no layout {here}"), "no layout {here}");
    assert_eq!(
        legacy_layouts_in(r#"cut {"V":2,"L":{"t":"p""#),
        r#"cut {"V":2,"L":{"t":"p""#
    );
}

#[test]
fn select_layout_reads_v2_back_like_the_pin() {
    let mut probe = Probe::new();
    probe.run(&["split-window", "-h", "-t", "w:0.0"]);
    probe.run(&["split-window", "-v", "-t", "w:0.1"]);
    let panes = "#{pane_index}:#{pane_id}:#{pane_active}:#{pane_last}";
    let geometry = "#{pane_width}x#{pane_height}@#{pane_left},#{pane_top}";
    let list =
        |probe: &mut Probe, format: &str| probe.run(&["list-panes", "-t", "w", "-F", format]);

    probe.run(&[
        "select-layout",
        "-t",
        "w",
        r#"{"V":2,"L":{"t":"h","w":80,"h":24,"x":0,"y":0,"c":[{"t":"p","w":40,"h":24,"x":0,"y":0,"a":true,"i":2,"I":"%9"},{"t":"v","w":39,"h":24,"x":41,"y":0,"c":[{"t":"p","w":39,"h":12,"x":41,"y":0,"l":0,"i":0},{"t":"p","w":39,"h":11,"x":41,"y":13,"l":1,"i":1}]}]}}"#,
    ]);
    assert_eq!(
        list(&mut probe, &format!("{panes}:{geometry}")),
        "0:%0:0:1:39x12@41,0\n1:%1:0:0:39x11@41,13\n2:%2:1:0:40x24@0,0"
    );
    assert_eq!(
        probe.fmt("#{window_layout}"),
        r#"{"V":2,"L":{"t":"h","w":80,"h":24,"x":0,"y":0,"c":[{"t":"p","w":40,"h":24,"x":0,"y":0,"a":true,"i":2,"I":"%2"},{"t":"v","w":39,"h":24,"x":41,"y":0,"c":[{"t":"p","w":39,"h":12,"x":41,"y":0,"l":0,"i":0,"I":"%0"},{"t":"p","w":39,"h":11,"x":41,"y":13,"l":1,"i":1,"I":"%1"}]}]}}"#
    );

    probe.run(&[
        "select-layout",
        "-t",
        "w",
        r#"{"V":2,"L":{"t":"v","w":80,"h":24,"x":0,"y":0,"c":[{"t":"p","w":80,"h":12,"x":0,"y":0,"i":5},{"t":"p","w":80,"h":5,"x":0,"y":13,"l":3,"i":6},{"t":"p","w":80,"h":5,"x":0,"y":19,"i":7}]}}"#,
    ]);
    assert_eq!(list(&mut probe, panes), "0:%0:0:0\n1:%1:0:1\n2:%2:1:0");

    probe.run(&[
        "select-layout",
        "-t",
        "w",
        r#"{"V":2,"L":{"t":"v","w":80,"h":24,"x":0,"y":0,"c":[{"t":"p","w":80,"h":12,"x":0,"y":0,"i":0},{"t":"p","w":80,"h":5,"x":0,"y":13,"i":1},{"t":"p","w":80,"h":5,"x":0,"y":19,"i":2},{"t":"p","w":80,"h":5,"x":0,"y":19,"i":3,"z":0}]}}"#,
    ]);
    assert_eq!(
        probe.fmt("#{window_layout}"),
        r#"{"V":2,"L":{"t":"v","w":80,"h":24,"x":0,"y":0,"c":[{"t":"p","w":80,"h":12,"x":0,"y":0,"i":0,"I":"%0"},{"t":"p","w":80,"h":5,"x":0,"y":13,"i":1,"I":"%1"},{"t":"p","w":80,"h":5,"x":0,"y":19,"a":true,"i":2,"I":"%2"}]}}"#
    );

    probe.run(&[
        "select-layout",
        "-t",
        "w",
        r#"{"V":2,"L":{"t":"h","w":80,"h":24,"x":0,"y":0,"c":[{"t":"p","w":20,"h":24,"x":0,"y":0,"i":3,"l":0},{"t":"p","w":19,"h":24,"x":21,"y":0,"i":1},{"t":"p","w":19,"h":24,"x":41,"y":0,"i":2,"a":true},{"t":"p","w":19,"h":24,"x":61,"y":0,"i":0,"l":1}]}}"#,
    ]);
    assert_eq!(list(&mut probe, panes), "0:%0:0:0\n1:%1:1:0\n2:%2:0:1");
    assert_eq!(
        probe.fmt("#{window_layout}"),
        r#"{"V":2,"L":{"t":"h","w":80,"h":24,"x":0,"y":0,"c":[{"t":"p","w":20,"h":24,"x":0,"y":0,"l":0,"i":2,"I":"%2"},{"t":"p","w":19,"h":24,"x":21,"y":0,"i":0,"I":"%0"},{"t":"p","w":39,"h":24,"x":41,"y":0,"a":true,"i":1,"I":"%1"}]}}"#
    );

    probe.run(&[
        "select-layout",
        "-t",
        "w",
        "d67e,80x24,0,0{40x24,0,0,0,39x24,41,0[39x12,41,0,1,39x11,41,13,2]}",
    ]);
    assert_eq!(
        probe.fmt("#{window_layout}"),
        r#"{"V":2,"L":{"t":"h","w":80,"h":24,"x":0,"y":0,"c":[{"t":"p","w":40,"h":24,"x":0,"y":0,"i":0,"I":"%0"},{"t":"v","w":39,"h":24,"x":41,"y":0,"c":[{"t":"p","w":39,"h":12,"x":41,"y":0,"a":true,"i":1,"I":"%1"},{"t":"p","w":39,"h":11,"x":41,"y":13,"l":0,"i":2,"I":"%2"}]}]}}"#
    );
}

#[test]
fn select_layout_errors_match_the_pin() {
    let mut probe = Probe::new();
    probe.run(&["split-window", "-h"]);
    let before = probe.fmt("#{window_layout}");
    let cases = [
        ("{", "invalid key: {"),
        ("", "malformed layout header"),
        ("   ", "malformed layout header"),
        ("{}", r#"key "V" not found"#),
        (r#"{"V":2}"#, r#"key "L" not found"#),
        (r#"{"V":"2","L":{}}"#, r#"key "V" expected a number"#),
        (r#"{"V":2,"L":{}}"#, r#"key "t" not found"#),
        (r#"{"V":2,"L":[]}"#, r#"key "L" expected an object"#),
        (
            r#"{"V":3,"L":{"t":"p","w":80,"h":24,"x":0,"y":0,"i":0}}"#,
            "version mismatch",
        ),
        (
            r#"{"V":2,"L":{"t":"p","w":80,"h":24,"x":0,"y":0,"i":0}}"#,
            "have 2 panes but need 1",
        ),
        (
            r#"{"V":2,"L":{"t":"q","w":80,"h":24,"x":0,"y":0,"i":0}}"#,
            r#"unknown cell type "q""#,
        ),
        (
            r#"{"V":2,"L":{"t":"p","w":0,"h":24,"x":0,"y":0,"i":0}}"#,
            "invalid width 0",
        ),
        (
            r#"{"V":2,"L":{"t":"p","w":80,"h":10001,"x":0,"y":0,"i":0}}"#,
            "invalid height 10001",
        ),
        (
            r#"{"V":2,"L":{"t":"p","w":80,"h":24,"x":-10001,"y":0,"i":0}}"#,
            "invalid x-offset -10001",
        ),
        (
            r#"{"V":2,"L":{"t":"p","w":80,"h":24,"x":0,"y":0,"i":-1}}"#,
            "invalid index -1",
        ),
        (
            r#"{"V":2,"L":{"t":"p","w":80,"h":24,"x":0,"y":0,"i":0,"a":1}}"#,
            r#"key "a" expected a boolean"#,
        ),
        (
            r#"{"V":2,"L":{"t":"p","w":80,"h":24,"x":0,"y":0,"i":0,"l":-1}}"#,
            "invalid last -1",
        ),
        (
            r#"{"V":2,"L":{"t":"p","w":80,"h":24,"x":0,"y":0,"i":0,"z":-1}}"#,
            "invalid floating zindex -1",
        ),
        (
            r#"{"V":2,"L":{"t":"h","w":80,"h":24,"x":0,"y":0,"c":[]}}"#,
            "nodes must have more than one child",
        ),
        (
            r#"{"V":2,"L":{"t":"p","w":80,"h":24,"x":0,"y":0,"i":0,"c":[]}}"#,
            "panes cannot have children",
        ),
        (
            r#"{"V":2,"L":{"t":"h","w":80,"h":24,"x":0,"y":0,"c":[{"t":"p","w":40,"h":24,"x":0,"y":0,"i":0,"a":true},{"t":"p","w":39,"h":24,"x":41,"y":0,"i":1,"a":true}]}}"#,
            "more than one active pane",
        ),
        (
            r#"{"V":2,"L":{"t":"h","w":80,"h":24,"x":0,"y":0,"c":[{"t":"p","w":40,"h":24,"x":0,"y":0,"i":0},{"t":"p","w":39,"h":24,"x":41,"y":0,"i":0}]}}"#,
            "duplicate pane index",
        ),
        (
            r#"{"V":2,"L":{"t":"h","w":80,"h":24,"x":0,"y":0,"c":[{"t":"p","w":40,"h":24,"x":0,"y":0,"i":0,"l":0},{"t":"p","w":39,"h":24,"x":41,"y":0,"i":1,"l":0}]}}"#,
            "duplicate last pane index",
        ),
        (
            r#"{"V":2,"L":{"t":"p","w":80,"h":24,"x":0,"y":0,"i":0}} x"#,
            "tokenization error: x",
        ),
        (r#"{"V":2,"V":2}"#, "duplicate key: :2}"),
        (r#"{"V":2x}"#, "invalid number: 2x}"),
        (r#"{"V":[{},]}"#, "invalid array: ,]}"),
        (r#"{"V":""}"#, r#"invalid string: ""}"#),
        (
            r#"{"V":2,"L":{"t":"p","w":80,"h":24,"x":0,"y":0,"i":0}}}"#,
            "unexpected trailing data: }",
        ),
        ("abc", "malformed layout header"),
        ("0000,80x24,0,0,0", "invalid layout checksum"),
    ];
    for (layout, cause) in cases {
        assert_eq!(
            probe.try_run(&["select-layout", "-t", "w", layout]),
            Err(ServerError::InvalidCommand(format!("{cause}: {layout}"))),
            "{layout}"
        );
        assert_eq!(probe.fmt("#{window_layout}"), before, "{layout}");
    }
    assert_eq!(
        probe.try_run(&[
            "select-layout",
            "-t",
            "w",
            r#"{"V":2,"L":{"t":"v","w":80,"h":24,"x":0,"y":0,"c":[{"t":"p","w":80,"h":24,"x":0,"y":0,"i":0},{"t":"p","w":20,"h":5,"x":3,"y":3,"i":1,"z":0}]}}"#,
        ]),
        Ok(String::new())
    );
}

#[test]
fn the_v2_writer_emits_floating_leaves_and_the_v1_copy_drops_them() {
    let input = r#"{"V":2,"L":{"t":"h","w":80,"h":24,"x":0,"y":0,"c":[{"t":"p","w":80,"h":24,"x":0,"y":0,"a":true,"i":4,"I":"%4"},{"t":"p","w":39,"h":7,"x":-3,"y":2,"i":5,"z":0,"I":"%5"}]}}"#;
    let parsed = CellLayout::parse(input).unwrap();
    let mut next = 0;
    let (layout, _) = parsed.into_layout(&[PaneId(4), PaneId(5)], &mut || {
        next += 1;
        zz_protocol::SplitId(next)
    });
    let leaf = |pane: PaneId| LeafState {
        active: pane == PaneId(4),
        last: None,
        index: u32::try_from(pane.0).unwrap(),
        z: (pane == PaneId(5)).then_some(0),
    };
    assert_eq!(layout.dump_as(LayoutFormat::V2, &leaf), input);
    assert_eq!(layout.dump_as(LayoutFormat::V1, &leaf), "b261,80x24,0,0,4");
}

#[test]
fn legacy_layout_matches_the_harness_converter() {
    let cases = [
        (
            r#"{"V":2,"L":{"t":"h","w":80,"h":24,"x":0,"y":0,"c":[{"t":"p","w":37,"h":24,"x":0,"y":0,"l":1,"i":0,"I":"%0"},{"t":"v","w":42,"h":24,"x":38,"y":0,"c":[{"t":"p","w":42,"h":12,"x":38,"y":0,"l":0,"i":1,"I":"%1"},{"t":"h","w":42,"h":11,"x":38,"y":13,"c":[{"t":"p","w":21,"h":11,"x":38,"y":13,"a":true,"i":2,"I":"%2"},{"t":"p","w":20,"h":11,"x":60,"y":13,"i":3,"I":"%3"}]}]}]}}"#,
            "154e,80x24,0,0{37x24,0,0,0,42x24,38,0[42x12,38,0,1,42x11,38,13{21x11,38,13,2,20x11,60,13,3}]}",
        ),
        (
            r#"{"V":2,"L":{"t":"v","w":80,"h":24,"x":0,"y":0,"c":[{"t":"p","w":80,"h":24,"x":0,"y":0,"a":true,"i":0,"I":"%0"},{"t":"p","w":18,"h":4,"x":6,"y":5,"i":1,"z":0,"I":"%1"}]}}"#,
            "b25d,80x24,0,0,0",
        ),
        (
            r#"{"V":2,"L":{"t":"h","w":80,"h":24,"x":0,"y":0,"c":[{"t":"p","w":40,"h":24,"x":0,"y":0,"l":0,"i":0,"I":"%0"},{"t":"v","w":39,"h":24,"x":41,"y":0,"c":[{"t":"p","w":39,"h":24,"x":41,"y":0,"a":true,"i":1,"I":"%2"},{"t":"p","w":8,"h":3,"x":51,"y":16,"i":2,"z":0,"I":"%3"}]}]}}"#,
            "0206,80x24,0,0{40x24,0,0,0,39x24,41,0,2}",
        ),
        (
            r#"{"V":2,"L":{"t":"h","w":80,"h":24,"x":0,"y":0,"c":[{"t":"p","w":40,"h":24,"x":0,"y":0,"l":1,"i":0,"I":"%4"},{"t":"v","w":39,"h":24,"x":41,"y":0,"c":[{"t":"p","w":39,"h":12,"x":41,"y":0,"l":0,"i":1,"I":"%5"},{"t":"p","w":8,"h":3,"x":46,"y":3,"i":3,"z":1,"I":"%7"},{"t":"p","w":39,"h":11,"x":41,"y":13,"a":true,"i":2,"I":"%6"},{"t":"p","w":8,"h":3,"x":46,"y":16,"i":4,"z":0,"I":"%8"}]}]}}"#,
            "da83,80x24,0,0{40x24,0,0,4,39x24,41,0[39x12,41,0,5,39x11,41,13,6]}",
        ),
        (
            r#"{"V":2,"L":{"t":"p","w":8,"h":3,"x":4,"y":2,"i":0,"z":0,"I":"%1"}}"#,
            "0000,",
        ),
        ("b25d,80x24,0,0,0", "b25d,80x24,0,0,0"),
    ];
    for (layout, expected) in cases {
        assert_eq!(legacy_layout(layout), expected, "{layout}");
    }
}

fn panes_line(probe: &mut Probe) -> String {
    probe
        .run(&[
            "list-panes",
            "-t",
            "w",
            "-F",
            "#{pane_index}:#{pane_id}:a#{pane_active}:l#{pane_last}",
        ])
        .replace('\n', " ")
}

#[test]
fn select_layout_restore_brings_back_the_saved_selection_like_the_pin() {
    let mut probe = Probe::new();
    probe.run(&["split-window", "-h", "-t", "w"]);
    probe.run(&["split-window", "-v", "-t", "w:0.1"]);
    let start = probe.fmt("#{window_layout}");
    assert_eq!(panes_line(&mut probe), "0:%0:a0:l0 1:%1:a0:l1 2:%2:a1:l0");
    probe.run(&[
        "select-layout",
        "-t",
        "w",
        r#"{"V":2,"L":{"t":"h","w":80,"h":24,"x":0,"y":0,"c":[{"t":"p","w":40,"h":24,"x":0,"y":0,"a":true,"i":0},{"t":"v","w":39,"h":24,"x":41,"y":0,"c":[{"t":"p","w":39,"h":12,"x":41,"y":0,"i":1},{"t":"p","w":39,"h":11,"x":41,"y":13,"i":2}]}]}}"#,
    ]);
    let applied = probe.fmt("#{window_layout}");
    assert_eq!(panes_line(&mut probe), "0:%0:a1:l0 1:%1:a0:l0 2:%2:a0:l0");
    probe.run(&["select-layout", "-t", "w", "-o"]);
    assert_eq!(panes_line(&mut probe), "0:%0:a0:l0 1:%1:a0:l1 2:%2:a1:l0");
    assert_eq!(probe.fmt("#{window_layout}"), start);
    probe.run(&["select-layout", "-t", "w", "-o"]);
    assert_eq!(panes_line(&mut probe), "0:%0:a1:l0 1:%1:a0:l0 2:%2:a0:l0");
    assert_eq!(probe.fmt("#{window_layout}"), applied);
    probe.run(&["select-layout", "-t", "w", "even-horizontal"]);
    probe.run(&["select-pane", "-t", "w:0.2"]);
    assert_eq!(panes_line(&mut probe), "0:%0:a0:l1 1:%1:a0:l0 2:%2:a1:l0");
    probe.run(&["select-layout", "-t", "w", "-o"]);
    assert_eq!(panes_line(&mut probe), "0:%0:a1:l0 1:%1:a0:l0 2:%2:a0:l0");
    assert_eq!(probe.fmt("#{window_layout}"), applied);
    assert!(probe.engine.state.validate().is_ok());
}

#[test]
fn a_last_index_on_the_active_pane_keeps_it_on_the_stack_like_the_pin() {
    let mut probe = Probe::new();
    probe.run(&["split-window", "-h", "-t", "w"]);
    assert_eq!(panes_line(&mut probe), "0:%0:a0:l1 1:%1:a1:l0");
    probe.run(&[
        "select-layout",
        "-t",
        "w",
        r#"{"V":2,"L":{"t":"h","w":80,"h":24,"x":0,"y":0,"c":[{"t":"p","w":40,"h":24,"x":0,"y":0,"i":0},{"t":"p","w":39,"h":24,"x":41,"y":0,"l":0,"i":1}]}}"#,
    ]);
    assert_eq!(panes_line(&mut probe), "0:%0:a0:l0 1:%1:a1:l1");
    assert!(probe.engine.state.validate().is_ok());
    probe.run(&["last-pane", "-t", "w"]);
    assert_eq!(panes_line(&mut probe), "0:%0:a0:l0 1:%1:a1:l1");
    probe.run(&["select-pane", "-t", "w:0.0"]);
    assert_eq!(panes_line(&mut probe), "0:%0:a1:l0 1:%1:a0:l1");
    probe.run(&["last-pane", "-t", "w"]);
    assert_eq!(panes_line(&mut probe), "0:%0:a0:l1 1:%1:a1:l0");
    assert!(probe.engine.state.validate().is_ok());
}

#[test]
fn legacy_layout_converts_trees_deeper_than_the_json_parse_limit() {
    let depth = 230;
    let mut body = "1x1,0,0,0".to_owned();
    let mut width = 1;
    for _ in 0..depth {
        width += 2;
        body = format!("{width}x1,0,0{{{body},1x1,0,0,0}}");
    }
    let checksum = body.bytes().fold(0_u16, |checksum, byte| {
        checksum.rotate_right(1).wrapping_add(u16::from(byte))
    });
    let parsed = CellLayout::parse(&format!("{checksum:04x},{body}")).unwrap();
    let panes = (0..=depth).map(PaneId).collect::<Vec<_>>();
    let mut next = 0;
    let (layout, _) = parsed.into_layout(&panes, &mut || {
        next += 1;
        zz_protocol::SplitId(next)
    });
    let leaf = |_| LeafState::default();
    let v2 = layout.dump_as(LayoutFormat::V2, &leaf);
    assert!(v2.starts_with("{\"V\":2"));
    assert_eq!(legacy_layout(&v2), layout.dump_as(LayoutFormat::V1, &leaf));
    assert_eq!(legacy_layout("{\"V\":2,\"L\":{}}"), "0000,");
}

#[test]
fn killing_an_inactive_pane_keeps_the_active_pane_on_the_stack_like_the_pin() {
    let mut probe = Probe::new();
    probe.run(&["split-window", "-h", "-t", "w"]);
    probe.run(&["split-window", "-h", "-t", "w"]);
    probe.run(&[
        "select-layout",
        "-t",
        "w",
        r#"{"V":2,"L":{"t":"h","w":80,"h":24,"x":0,"y":0,"c":[{"t":"p","w":26,"h":24,"x":0,"y":0,"l":1,"i":0},{"t":"p","w":26,"h":24,"x":27,"y":0,"i":1},{"t":"p","w":26,"h":24,"x":54,"y":0,"l":0,"i":2}]}}"#,
    ]);
    assert_eq!(panes_line(&mut probe), "0:%0:a0:l0 1:%1:a0:l0 2:%2:a1:l1");
    probe.run(&["kill-pane", "-t", "w:0.1"]);
    assert_eq!(panes_line(&mut probe), "0:%0:a0:l0 1:%2:a1:l1");
    probe.run(&["last-pane", "-t", "w"]);
    assert_eq!(panes_line(&mut probe), "0:%0:a0:l0 1:%2:a1:l1");
    assert!(probe.engine.state.validate().is_ok());
}
