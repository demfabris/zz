use crate::{
    Bounds, DevicePixels, Font, FontId, FontMetrics, FontRun, GlyphId, Hsla, LineLayout,
    MissingGlyphSink, Pixels, PlatformTextSystem, RenderGlyphParams, Result, Scene, Size,
    TextRenderingMode,
};
use serde::Serialize;
use std::{
    borrow::Cow,
    cell::RefCell,
    collections::BTreeMap,
    fs::{File, OpenOptions},
    io::Write,
    path::PathBuf,
    sync::{Arc, OnceLock},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use web_time::Instant;

static OUTPUT_PATH: OnceLock<Option<PathBuf>> = OnceLock::new();

thread_local! {
    static RECORDER: RefCell<Recorder> = RefCell::new(Recorder::default());
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Phase {
    RequestLayout,
    Prepaint,
    Paint,
    SceneFinish,
}

pub(crate) enum Counter {
    ViewsRendered,
}

#[derive(Clone, Copy)]
struct Stamp {
    wall: Instant,
    cpu: Option<Duration>,
}

impl Stamp {
    fn now() -> Self {
        Self {
            wall: Instant::now(),
            cpu: thread_cpu_time(),
        }
    }

    fn since(self, start: Self) -> Timing {
        Timing {
            wall_ns: nanos(self.wall.saturating_duration_since(start.wall)),
            thread_cpu_ns: self
                .cpu
                .zip(start.cpu)
                .map(|(end, start)| nanos(end.saturating_sub(start))),
        }
    }
}

#[derive(Clone, Copy, Serialize)]
struct Timing {
    wall_ns: u64,
    thread_cpu_ns: Option<u64>,
}

impl Timing {
    fn zero(cpu_available: bool) -> Self {
        Self {
            wall_ns: 0,
            thread_cpu_ns: cpu_available.then_some(0),
        }
    }

    fn add(&mut self, other: Self) {
        self.wall_ns = self.wall_ns.saturating_add(other.wall_ns);
        self.thread_cpu_ns = self
            .thread_cpu_ns
            .zip(other.thread_cpu_ns)
            .map(|(left, right)| left.saturating_add(right));
    }
}

#[derive(Serialize)]
struct Phases {
    request_layout: Timing,
    prepaint: Timing,
    paint: Timing,
    scene_finish: Timing,
    present_submit: Option<Timing>,
}

impl Phases {
    fn new(cpu_available: bool) -> Self {
        Self {
            request_layout: Timing::zero(cpu_available),
            prepaint: Timing::zero(cpu_available),
            paint: Timing::zero(cpu_available),
            scene_finish: Timing::zero(cpu_available),
            present_submit: None,
        }
    }

    fn get_mut(&mut self, phase: Phase) -> &mut Timing {
        match phase {
            Phase::RequestLayout => &mut self.request_layout,
            Phase::Prepaint => &mut self.prepaint,
            Phase::Paint => &mut self.paint,
            Phase::SceneFinish => &mut self.scene_finish,
        }
    }
}

#[derive(Default, Serialize)]
struct Counts {
    views_rendered: u64,
    elements: u64,
    layout_nodes: u64,
    sprites: u64,
    quads: u64,
    paths: u64,
    glyphs_shaped: u64,
    shape_calls: u64,
    hitboxes: u64,
}

#[derive(Serialize)]
struct Frame {
    schema_version: u32,
    pid: u32,
    tid: Option<u64>,
    window_id: u64,
    frame_id: u64,
    viewport_size: ViewportSize,
    scale_factor: f32,
    timestamp_unix_ns: u64,
    drawn_at_unix_ns: u64,
    presented_at_unix_ns: Option<u64>,
    draw: Timing,
    phases: Phases,
    counts: Counts,
    accessibility_active: bool,
}

#[derive(Serialize)]
struct ViewportSize {
    width: f32,
    height: f32,
}

struct ActiveFrame {
    frame: Frame,
    started: Stamp,
    phase: Option<Phase>,
    phase_started: Stamp,
}

impl ActiveFrame {
    fn switch_phase(&mut self, phase: Option<Phase>, now: Stamp) {
        if let Some(previous) = self.phase {
            self.frame
                .phases
                .get_mut(previous)
                .add(now.since(self.phase_started));
        }
        self.phase = phase;
        self.phase_started = now;
    }
}

#[derive(Default)]
struct Recorder {
    active: Option<ActiveFrame>,
    pending: BTreeMap<u64, Frame>,
    next_frame_id: u64,
    output: Option<File>,
    output_failed: bool,
    buffer: Vec<u8>,
}

impl Recorder {
    fn write(&mut self, frame: &Frame) {
        if self.output_failed {
            return;
        }
        if self.output.is_none() {
            let Some(path) = output_path() else {
                return;
            };
            match OpenOptions::new().create(true).append(true).open(path) {
                Ok(output) => self.output = Some(output),
                Err(error) => {
                    log::error!("GPUI_FRAME_STATS: cannot open {}: {error}", path.display());
                    self.output_failed = true;
                    return;
                }
            }
        }
        self.buffer.clear();
        if let Err(error) = serde_json::to_writer(&mut self.buffer, frame) {
            log::error!("GPUI_FRAME_STATS: cannot serialize frame: {error}");
            self.output_failed = true;
            return;
        }
        self.buffer.push(b'\n');
        if let Some(output) = self.output.as_mut()
            && let Err(error) = output.write_all(&self.buffer)
        {
            log::error!("GPUI_FRAME_STATS: cannot write frame: {error}");
            self.output_failed = true;
        }
    }
}

#[inline]
fn output_path() -> Option<&'static PathBuf> {
    OUTPUT_PATH
        .get_or_init(|| {
            std::env::var_os("GPUI_FRAME_STATS")
                .filter(|path| !path.is_empty())
                .map(PathBuf::from)
        })
        .as_ref()
}

pub(crate) struct DrawGuard {
    enabled: bool,
}

pub(crate) fn begin_draw(
    window_id: u64,
    viewport_width: f32,
    viewport_height: f32,
    scale_factor: f32,
) -> DrawGuard {
    let enabled = output_path().is_some();
    if enabled {
        RECORDER.with_borrow_mut(|recorder| {
            if let Some(previous) = recorder.pending.remove(&window_id) {
                recorder.write(&previous);
            }
            let started = Stamp::now();
            recorder.next_frame_id += 1;
            recorder.active = Some(ActiveFrame {
                frame: Frame {
                    schema_version: 1,
                    pid: std::process::id(),
                    tid: thread_id(),
                    window_id,
                    frame_id: recorder.next_frame_id,
                    viewport_size: ViewportSize {
                        width: viewport_width,
                        height: viewport_height,
                    },
                    scale_factor,
                    timestamp_unix_ns: unix_ns(),
                    drawn_at_unix_ns: 0,
                    presented_at_unix_ns: None,
                    draw: Timing::zero(started.cpu.is_some()),
                    phases: Phases::new(started.cpu.is_some()),
                    counts: Counts::default(),
                    accessibility_active: false,
                },
                started,
                phase: None,
                phase_started: started,
            });
        });
    }
    DrawGuard { enabled }
}

impl Drop for DrawGuard {
    fn drop(&mut self) {
        if self.enabled {
            RECORDER.with_borrow_mut(|recorder| {
                if let Some(mut active) = recorder.active.take() {
                    let now = Stamp::now();
                    active.switch_phase(None, now);
                    active.frame.draw = now.since(active.started);
                    active.frame.drawn_at_unix_ns = unix_ns();
                    recorder
                        .pending
                        .insert(active.frame.window_id, active.frame);
                }
            });
        }
    }
}

pub(crate) struct PhaseGuard {
    previous: Option<Option<Phase>>,
}

#[inline]
pub(crate) fn phase(phase: Phase) -> PhaseGuard {
    enter_phase(phase, false)
}

#[inline]
pub(crate) fn element_phase(phase: Phase) -> PhaseGuard {
    enter_phase(phase, phase == Phase::RequestLayout)
}

#[inline]
fn enter_phase(phase: Phase, element: bool) -> PhaseGuard {
    let previous = if output_path().is_some() {
        RECORDER.with_borrow_mut(|recorder| {
            let active = recorder.active.as_mut()?;
            if element {
                active.frame.counts.elements += 1;
            }
            if active.phase == Some(phase) {
                return None;
            }
            let previous = active.phase;
            active.switch_phase(Some(phase), Stamp::now());
            Some(previous)
        })
    } else {
        None
    };
    PhaseGuard { previous }
}

impl Drop for PhaseGuard {
    #[inline]
    fn drop(&mut self) {
        if let Some(previous) = self.previous {
            RECORDER.with_borrow_mut(|recorder| {
                if let Some(active) = recorder.active.as_mut() {
                    active.switch_phase(previous, Stamp::now());
                }
            });
        }
    }
}

pub(crate) struct PresentGuard {
    window_id: u64,
    started: Option<Stamp>,
}

pub(crate) fn begin_present(window_id: u64) -> PresentGuard {
    let started = if output_path().is_some() {
        RECORDER.with_borrow(|recorder| recorder.pending.contains_key(&window_id).then(Stamp::now))
    } else {
        None
    };
    PresentGuard { window_id, started }
}

impl Drop for PresentGuard {
    fn drop(&mut self) {
        if let Some(started) = self.started {
            let timing = Stamp::now().since(started);
            let presented_at = unix_ns();
            RECORDER.with_borrow_mut(|recorder| {
                if let Some(mut frame) = recorder.pending.remove(&self.window_id) {
                    frame.phases.present_submit = Some(timing);
                    frame.presented_at_unix_ns = Some(presented_at);
                    recorder.write(&frame);
                }
            });
        }
    }
}

#[inline]
pub(crate) fn count(counter: Counter) {
    if output_path().is_some() {
        RECORDER.with_borrow_mut(|recorder| {
            if let Some(active) = recorder.active.as_mut() {
                match counter {
                    Counter::ViewsRendered => active.frame.counts.views_rendered += 1,
                }
            }
        });
    }
}

pub(crate) fn accessibility_active(active: bool) {
    if output_path().is_some() {
        RECORDER.with_borrow_mut(|recorder| {
            if let Some(frame) = recorder.active.as_mut() {
                frame.frame.accessibility_active = active;
            }
        });
    }
}

pub(crate) fn layout_nodes(count: impl FnOnce() -> usize) {
    if output_path().is_some() {
        RECORDER.with_borrow_mut(|recorder| {
            if let Some(active) = recorder.active.as_mut() {
                active.frame.counts.layout_nodes = count() as u64;
            }
        });
    }
}

pub(crate) fn text_system(inner: Arc<dyn PlatformTextSystem>) -> Arc<dyn PlatformTextSystem> {
    if output_path().is_some() {
        Arc::new(RecordingTextSystem { inner })
    } else {
        inner
    }
}

struct RecordingTextSystem {
    inner: Arc<dyn PlatformTextSystem>,
}

impl PlatformTextSystem for RecordingTextSystem {
    fn add_fonts(&self, fonts: Vec<Cow<'static, [u8]>>) -> Result<()> {
        self.inner.add_fonts(fonts)
    }

    fn set_missing_glyph_sink(&self, sink: Option<Arc<dyn MissingGlyphSink>>) {
        self.inner.set_missing_glyph_sink(sink)
    }

    fn all_font_names(&self) -> Vec<String> {
        self.inner.all_font_names()
    }

    fn font_id(&self, descriptor: &Font) -> Result<FontId> {
        self.inner.font_id(descriptor)
    }

    fn prewarm_fonts(&self, font_ids: &[FontId]) {
        self.inner.prewarm_fonts(font_ids)
    }

    fn font_generation(&self) -> u64 {
        self.inner.font_generation()
    }

    fn font_metrics(&self, font_id: FontId) -> FontMetrics {
        self.inner.font_metrics(font_id)
    }

    fn typographic_bounds(&self, font_id: FontId, glyph_id: GlyphId) -> Result<Bounds<f32>> {
        self.inner.typographic_bounds(font_id, glyph_id)
    }

    fn advance(&self, font_id: FontId, glyph_id: GlyphId) -> Result<Size<f32>> {
        self.inner.advance(font_id, glyph_id)
    }

    fn glyph_for_char(&self, font_id: FontId, ch: char) -> Option<GlyphId> {
        self.inner.glyph_for_char(font_id, ch)
    }

    fn glyph_raster_bounds(&self, params: &RenderGlyphParams) -> Result<Bounds<DevicePixels>> {
        self.inner.glyph_raster_bounds(params)
    }

    fn rasterize_glyph(
        &self,
        params: &RenderGlyphParams,
        raster_bounds: Bounds<DevicePixels>,
    ) -> Result<(Size<DevicePixels>, Vec<u8>)> {
        self.inner.rasterize_glyph(params, raster_bounds)
    }

    fn layout_line(&self, text: &str, font_size: Pixels, runs: &[FontRun]) -> LineLayout {
        let layout = self.inner.layout_line(text, font_size, runs);
        shaped(&layout);
        layout
    }

    fn recommended_rendering_mode(&self, font_id: FontId, font_size: Pixels) -> TextRenderingMode {
        self.inner.recommended_rendering_mode(font_id, font_size)
    }

    fn glyph_dilation_for_color(&self, color: Hsla) -> u8 {
        self.inner.glyph_dilation_for_color(color)
    }
}

pub(crate) fn shaped(layout: &LineLayout) {
    if output_path().is_some() {
        RECORDER.with_borrow_mut(|recorder| {
            if let Some(active) = recorder.active.as_mut() {
                active.frame.counts.shape_calls += 1;
                active.frame.counts.glyphs_shaped += layout
                    .runs
                    .iter()
                    .map(|run| run.glyphs.len() as u64)
                    .sum::<u64>();
            }
        });
    }
}

pub(crate) fn scene(scene: &Scene, hitboxes: usize) {
    if output_path().is_some() {
        RECORDER.with_borrow_mut(|recorder| {
            if let Some(active) = recorder.active.as_mut() {
                let counts = &mut active.frame.counts;
                counts.sprites = (scene.monochrome_sprites.len()
                    + scene.subpixel_sprites.len()
                    + scene.polychrome_sprites.len()) as u64;
                counts.quads = scene.quads.len() as u64;
                counts.paths = scene.paths.len() as u64;
                counts.hitboxes = hitboxes as u64;
            }
        });
    }
}

fn unix_ns() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(nanos)
        .unwrap_or_default()
}

fn nanos(duration: Duration) -> u64 {
    duration.as_nanos().min(u64::MAX as u128) as u64
}

#[cfg(target_os = "linux")]
fn thread_id() -> Option<u64> {
    // SAFETY: gettid has no arguments and does not access caller-owned memory.
    Some(unsafe { libc::gettid() } as u64)
}

#[cfg(not(target_os = "linux"))]
fn thread_id() -> Option<u64> {
    None
}

#[cfg(unix)]
fn thread_cpu_time() -> Option<Duration> {
    let mut time = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: clock_gettime writes to a valid timespec and is checked before reading it.
    let result = unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &mut time) };
    (result == 0).then(|| Duration::new(time.tv_sec as u64, time.tv_nsec as u32))
}

#[cfg(not(unix))]
fn thread_cpu_time() -> Option<Duration> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phase_switches_do_not_double_count_nested_layout() {
        let start = Stamp::now();
        let at = |nanos| Stamp {
            wall: start.wall + Duration::from_nanos(nanos),
            cpu: Some(Duration::from_nanos(nanos)),
        };
        let start = at(0);
        let mut active = ActiveFrame {
            frame: Frame {
                schema_version: 1,
                pid: 0,
                tid: None,
                window_id: 1,
                frame_id: 1,
                viewport_size: ViewportSize {
                    width: 800.0,
                    height: 600.0,
                },
                scale_factor: 1.0,
                timestamp_unix_ns: 0,
                drawn_at_unix_ns: 0,
                presented_at_unix_ns: None,
                draw: Timing::zero(true),
                phases: Phases::new(true),
                counts: Counts::default(),
                accessibility_active: false,
            },
            started: start,
            phase: None,
            phase_started: start,
        };
        active.switch_phase(Some(Phase::Prepaint), at(0));
        active.switch_phase(Some(Phase::RequestLayout), at(10));
        active.switch_phase(Some(Phase::Prepaint), at(30));
        active.switch_phase(Some(Phase::Paint), at(40));
        active.switch_phase(None, at(100));
        assert_eq!(active.frame.phases.prepaint.wall_ns, 20);
        assert_eq!(active.frame.phases.request_layout.wall_ns, 20);
        assert_eq!(active.frame.phases.paint.wall_ns, 60);
        assert_eq!(active.frame.phases.prepaint.thread_cpu_ns, Some(20));
        assert_eq!(active.frame.phases.request_layout.thread_cpu_ns, Some(20));
        assert_eq!(active.frame.phases.paint.thread_cpu_ns, Some(60));
    }

    #[test]
    fn unsupported_cpu_clock_remains_null() {
        let mut timing = Timing::zero(false);
        timing.add(Timing {
            wall_ns: 12,
            thread_cpu_ns: Some(8),
        });
        let json = serde_json::to_value(timing).expect("timing should serialize");
        assert_eq!(json["wall_ns"], 12);
        assert!(json["thread_cpu_ns"].is_null());
    }
}
