#[path = "../src/momentum.rs"]
mod momentum;

use gpui::{Point, point, px};
use momentum::Momentum;
use std::time::{Duration, Instant};

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
