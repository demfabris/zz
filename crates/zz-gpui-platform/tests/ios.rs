use std::time::{Duration, Instant};
use zz_gpui::{Point, TouchPhase, point, px};
use zz_gpui_platform::ios::{keyboard, momentum::Momentum, pinch::Pinch};

#[test]
fn chrome_chords_become_menu_key_commands() {
    assert_eq!(keyboard::key_command("D-k"), Some(("k".into(), 1 << 20)));
    assert_eq!(
        keyboard::key_command("D-S-n"),
        Some(("n".into(), (1 << 20) | (1 << 17)))
    );
    assert_eq!(keyboard::key_command("D--"), Some(("-".into(), 1 << 20)));
    assert_eq!(keyboard::key_command("D-,"), Some((",".into(), 1 << 20)));
    assert_eq!(keyboard::key_command("D-Up"), None);
}

#[test]
fn fling_coasts_the_uiscrollview_distance_and_stops() {
    let start = Instant::now();
    let origin = point(px(40.0), px(90.0));
    let mut momentum = Momentum::fling(origin, point(30.0, -1000.0), start).unwrap();
    assert_eq!(momentum.position, origin);
    let mut travelled = 0.0f32;
    let mut frames = 0;
    loop {
        frames += 1;
        let (delta, done) = momentum.step(start + Duration::from_millis(frames * 16));
        assert_eq!(delta.x, px(0.0));
        assert!(f32::from(delta.y) <= 0.0);
        travelled += f32::from(delta.y);
        if done {
            break;
        }
    }
    assert!((travelled + 494.6).abs() < 0.5, "{travelled}");
    assert!((140..150).contains(&frames), "{frames}");
}

#[test]
fn slow_release_does_not_fling() {
    assert!(Momentum::fling(Point::default(), point(0.0, 40.0), Instant::now()).is_none());
}

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
