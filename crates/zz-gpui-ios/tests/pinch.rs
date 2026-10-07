#[path = "../src/pinch.rs"]
mod pinch;

use gpui::{TouchPhase, point, px};
use pinch::Pinch;

#[test]
fn pinch_starts_past_the_slop_and_scales_from_the_span_where_it_started() {
    let mut pinch = Pinch::new(
        [1, 2],
        [point(px(100.0), px(200.0)), point(px(200.0), px(200.0))],
    );
    assert!(pinch.owns(1) && pinch.owns(2) && !pinch.owns(3));
    assert_eq!(
        pinch.touch(2, TouchPhase::Moved, point(px(207.0), px(200.0))),
        None
    );
    let started = pinch
        .touch(2, TouchPhase::Moved, point(px(209.0), px(200.0)))
        .unwrap();
    assert_eq!(started.phase, TouchPhase::Started);
    assert_eq!(started.delta, 0.0);
    assert_eq!(started.position, point(px(154.5), px(200.0)));
    let grown = pinch
        .touch(1, TouchPhase::Moved, point(px(45.5), px(200.0)))
        .unwrap();
    assert_eq!(grown.phase, TouchPhase::Moved);
    assert!((grown.delta - 0.5).abs() < 1e-4, "{}", grown.delta);
    let shrunk = pinch
        .touch(1, TouchPhase::Moved, point(px(100.0), px(200.0)))
        .unwrap();
    assert!((grown.delta + shrunk.delta).abs() < 1e-4);
    let ended = pinch
        .touch(1, TouchPhase::Ended, point(px(100.0), px(200.0)))
        .unwrap();
    assert_eq!(ended.phase, TouchPhase::Ended);
    assert!(!pinch.owns(1) && pinch.owns(2) && !pinch.finished());
    assert_eq!(
        pinch.touch(2, TouchPhase::Moved, point(px(400.0), px(200.0))),
        None
    );
    assert_eq!(
        pinch.touch(2, TouchPhase::Ended, point(px(400.0), px(200.0))),
        None
    );
    assert!(pinch.finished());
}

#[test]
fn still_or_crowded_fingers_never_start_a_pinch() {
    let mut tap = Pinch::new(
        [1, 2],
        [point(px(100.0), px(200.0)), point(px(160.0), px(260.0))],
    );
    assert_eq!(
        tap.touch(1, TouchPhase::Moved, point(px(101.0), px(201.0))),
        None
    );
    assert_eq!(
        tap.touch(1, TouchPhase::Ended, point(px(101.0), px(201.0))),
        None
    );
    assert_eq!(
        tap.touch(2, TouchPhase::Cancelled, point(px(160.0), px(260.0))),
        None
    );
    assert!(tap.finished());
    let mut crowded = Pinch::new(
        [1, 2],
        [point(px(197.0), px(436.0)), point(px(205.0), px(438.0))],
    );
    assert_eq!(
        crowded.touch(2, TouchPhase::Moved, point(px(211.0), px(442.0))),
        None
    );
}
