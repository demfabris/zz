use std::f32::consts::{FRAC_PI_2, TAU};
use std::fmt::Write as _;
use std::sync::LazyLock;

use serde_json::Value;

pub const PREFIX: &str = "icon-drafts/";

const CORNER_FRACTION: f32 = 0.45;
const SAMPLES: usize = 16;

static SETS: LazyLock<Vec<Value>> = LazyLock::new(|| {
    [include_str!("../icons/sets/mac.json")]
        .into_iter()
        .map(|json| serde_json::from_str(json).expect("icon set parses"))
        .collect()
});

type Point = (f32, f32, f32);

fn set(name: &str) -> Option<&'static Value> {
    SETS.iter().find(|set| set["name"] == name)
}

fn icons(set: &Value) -> &[Value] {
    set["icons"].as_array().map_or(&[], Vec::as_slice)
}

pub fn names() -> impl Iterator<Item = &'static str> {
    SETS.first()
        .map(|set| icons(set))
        .unwrap_or_default()
        .iter()
        .filter_map(|icon| icon["name"].as_str())
}

pub fn has(set_name: &str, name: &str) -> bool {
    set(set_name).is_some_and(|set| icons(set).iter().any(|icon| icon["name"] == name))
}

pub fn path(set: &str, name: &str, radius: f32, smoothing: f32) -> String {
    format!("{PREFIX}{set}/{radius:.2}-{smoothing:.2}/{name}.svg")
}

pub fn load(path: &str) -> Option<String> {
    let mut segments = path.strip_prefix(PREFIX)?.split('/');
    let (set_name, style, file) = (segments.next()?, segments.next()?, segments.next()?);
    let (radius, smoothing) = style.split_once('-')?;
    svg(
        set(set_name)?,
        file.strip_suffix(".svg")?,
        radius.parse().ok()?,
        smoothing.parse().ok()?,
    )
}

fn svg(set: &Value, name: &str, radius: f32, smoothing: f32) -> Option<String> {
    let icon = icons(set).iter().find(|icon| icon["name"] == name)?;
    let stroke = set["stroke"].as_f64().unwrap_or(2.0);
    let smoothing = smoothing.max(2.0);
    let (cap, join) = if radius > 0.0 {
        ("round", "round")
    } else {
        ("square", "miter")
    };
    let mut out = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="{stroke}" stroke-linecap="{cap}" stroke-linejoin="{join}">"#
    );
    for part in icon["parts"].as_array()? {
        out.push_str(&draw(part.as_array()?, radius, smoothing)?);
    }
    out.push_str("</svg>");
    Some(out)
}

fn draw(part: &[Value], radius: f32, smoothing: f32) -> Option<String> {
    let kind = part.first()?.as_str()?;
    let args = &part[1..];
    let n = |index: usize| args.get(index).and_then(Value::as_f64).map(|v| v as f32);
    let points = || -> Option<Vec<Point>> {
        args.iter()
            .map(|point| {
                let point = point.as_array()?;
                let at = |index: usize| point.get(index).and_then(Value::as_f64).map(|v| v as f32);
                Some((at(0)?, at(1)?, at(2).unwrap_or(0.0)))
            })
            .collect()
    };
    let stroke = |d: String| format!(r#"<path d="{d}"/>"#);
    Some(match kind {
        "line" => stroke(format!(
            "M{} {}L{} {}",
            num(n(0)?),
            num(n(1)?),
            num(n(2)?),
            num(n(3)?)
        )),
        "path" => stroke(outline(
            &points()?,
            false,
            radius,
            smoothing,
            CORNER_FRACTION,
        )),
        "shape" => stroke(outline(
            &points()?,
            true,
            radius,
            smoothing,
            CORNER_FRACTION,
        )),
        "rect" => {
            let (x0, y0, x1, y1, s) = (n(0)?, n(1)?, n(2)?, n(3)?, n(4).unwrap_or(1.0));
            let corners = [(x0, y0, s), (x1, y0, s), (x1, y1, s), (x0, y1, s)];
            stroke(outline(&corners, true, radius, smoothing, CORNER_FRACTION))
        }
        "circle" => format!(
            r#"<circle cx="{}" cy="{}" r="{}"/>"#,
            num(n(0)?),
            num(n(1)?),
            num(n(2)?)
        ),
        "ellipse" => format!(
            r#"<ellipse cx="{}" cy="{}" rx="{}" ry="{}"/>"#,
            num(n(0)?),
            num(n(1)?),
            num(n(2)?),
            num(n(3)?)
        ),
        "arc" => {
            let (cx, cy, r, from, to) = (n(0)?, n(1)?, n(2)?, n(3)?, n(4)?);
            let at = |degrees: f32| {
                let angle = degrees.to_radians();
                (cx + r * angle.cos(), cy + r * angle.sin())
            };
            let ((sx, sy), (ex, ey)) = (at(from), at(to));
            let large = u8::from(to - from > 180.0);
            stroke(format!(
                "M{} {}A{} {} 0 {large} 1 {} {}",
                num(sx),
                num(sy),
                num(r),
                num(r),
                num(ex),
                num(ey)
            ))
        }
        "dot" => {
            let (cx, cy, half) = (n(0)?, n(1)?, n(2).unwrap_or(2.0) / 2.0);
            let corners = [
                (cx - half, cy - half, 1.0),
                (cx + half, cy - half, 1.0),
                (cx + half, cy + half, 1.0),
                (cx - half, cy + half, 1.0),
            ];
            let d = outline(&corners, true, radius.min(half), smoothing, 0.5);
            format!(r#"<path d="{d}" fill="currentColor" stroke="none"/>"#)
        }
        "gear" => {
            let teeth = n(0)? as usize;
            stroke(outline(
                &gear(teeth, n(1)?, n(2)?, n(3)?, n(4)?, n(5).unwrap_or(0.5)),
                true,
                radius,
                smoothing,
                CORNER_FRACTION,
            ))
        }
        "curve" => stroke(format!(
            "M{} {}Q{} {} {} {}",
            num(n(0)?),
            num(n(1)?),
            num(n(2)?),
            num(n(3)?),
            num(n(4)?),
            num(n(5)?)
        )),
        "cubic" => stroke(format!(
            "M{} {}C{} {} {} {} {} {}",
            num(n(0)?),
            num(n(1)?),
            num(n(2)?),
            num(n(3)?),
            num(n(4)?),
            num(n(5)?),
            num(n(6)?),
            num(n(7)?)
        )),
        "solid" => {
            let inner = draw(args.first()?.as_array()?, radius, smoothing)?;
            format!(
                r#"{} fill="currentColor" stroke="none"/>"#,
                inner.strip_suffix("/>")?
            )
        }
        "fade" => {
            let inner = draw(args.get(1)?.as_array()?, radius, smoothing)?;
            format!(
                r#"{} opacity="{}"/>"#,
                inner.strip_suffix("/>")?,
                num(n(0)?)
            )
        }
        "d" => stroke(args.first()?.as_str()?.to_owned()),
        _ => return None,
    })
}

fn outline(points: &[Point], closed: bool, radius: f32, smoothing: f32, cap: f32) -> String {
    let count = points.len();
    let mut d = String::new();
    let mut emit = |x: f32, y: f32| {
        let command = if d.is_empty() { 'M' } else { 'L' };
        let _ = write!(d, "{command}{} {}", num(x), num(y));
    };
    for index in 0..count {
        let (x, y, scale) = points[index];
        let inner = closed || (index > 0 && index + 1 < count);
        if !inner || scale <= 0.0 || radius <= 0.0 {
            emit(x, y);
            continue;
        }
        let (px, py, _) = points[(index + count - 1) % count];
        let (nx, ny, _) = points[(index + 1) % count];
        let incoming = (px - x).hypot(py - y);
        let outgoing = (nx - x).hypot(ny - y);
        let r = (radius * scale).min(cap * incoming.min(outgoing));
        let back = ((px - x) / incoming, (py - y) / incoming);
        let ahead = ((nx - x) / outgoing, (ny - y) / outgoing);
        for step in 0..=SAMPLES {
            let theta = FRAC_PI_2 * (1.0 - step as f32 / SAMPLES as f32);
            let along = 1.0 - theta.cos().max(0.0).powf(2.0 / smoothing);
            let across = 1.0 - theta.sin().max(0.0).powf(2.0 / smoothing);
            emit(
                x + back.0 * r * along + ahead.0 * r * across,
                y + back.1 * r * along + ahead.1 * r * across,
            );
        }
    }
    if closed {
        d.push('Z');
    }
    d
}

fn gear(
    teeth: usize,
    root: f32,
    tip: f32,
    tip_width: f32,
    root_width: f32,
    scale: f32,
) -> Vec<Point> {
    let period = TAU / teeth as f32;
    let mut points = Vec::with_capacity(teeth * 4);
    for tooth in 0..teeth {
        let center = tooth as f32 * period;
        for (angle, reach) in [
            (center - root_width * period / 2.0, root),
            (center - tip_width * period / 2.0, tip),
            (center + tip_width * period / 2.0, tip),
            (center + root_width * period / 2.0, root),
        ] {
            points.push((
                12.0 + reach * angle.sin(),
                12.0 - reach * angle.cos(),
                scale,
            ));
        }
    }
    points
}

fn num(value: f32) -> String {
    let text = format!("{value:.2}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    if text == "-0" {
        "0".into()
    } else {
        text.into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_icon_in_every_set_draws_at_flat_default_and_round() {
        for set in ["mac"] {
            assert!(names().count() > 60);
            for name in names() {
                for radius in [0.0, 3.0, 12.5] {
                    let svg = load(&path(set, name, radius, 4.0)).expect(name);
                    assert!(svg.ends_with("</svg>"), "{set}/{name}");
                }
            }
        }
    }
}
