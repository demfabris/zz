use std::time::{Duration, Instant};
use zz_gpui::{Pixels, PlatformInput, Point, Size, TouchEvent, TouchId, TouchPhase, point, px};

const BURST_GAP: f64 = 0.1;
const LINK_REPORT: f64 = 5.0;
const MIN_BURST: usize = 5;
const BENCH_DELAY: Duration = Duration::from_secs(8);
const DEFAULT_CYCLES: u32 = 16;

#[derive(Default)]
pub(crate) struct Perf {
    log: Option<FrameLog>,
    bench: Option<Bench>,
    tick_started: Option<Instant>,
    drew: bool,
    scene: (usize, usize, usize),
}

impl Perf {
    pub(crate) fn place_frame_stats() {
        if let Some(path) = std::env::var_os("GPUI_FRAME_STATS")
            .map(std::path::PathBuf::from)
            .filter(|path| path.is_relative() && !path.as_os_str().is_empty())
        {
            unsafe { std::env::set_var("GPUI_FRAME_STATS", std::env::temp_dir().join(path)) };
        }
    }

    pub(crate) fn from_env() -> Self {
        let log = std::env::var("ZZ_GPUI_FRAME_LOG")
            .is_ok_and(|value| !value.is_empty() && value != "0")
            .then(FrameLog::default);
        let bench = std::env::var("ZZ_GPUI_BENCH")
            .ok()
            .and_then(|value| Bench::parse(&value));
        Self {
            log,
            bench,
            tick_started: None,
            drew: false,
            scene: (0, 0, 0),
        }
    }

    pub(crate) fn wants_frames(&self) -> bool {
        self.bench.as_ref().is_some_and(|bench| !bench.finished)
    }

    pub(crate) fn begin(&mut self) {
        if self.log.is_some() {
            self.tick_started = Some(Instant::now());
            self.drew = false;
        }
    }

    pub(crate) fn touch_moved(&mut self) {
        if let Some(log) = self.log.as_mut() {
            log.window_touches += 1;
        }
    }

    pub(crate) fn drew(&mut self, scene: &zz_gpui::Scene) {
        if self.log.is_none() {
            return;
        }
        let identity = (
            scene.len(),
            scene.quads.as_ptr() as usize,
            scene.monochrome_sprites.as_ptr() as usize,
        );
        if identity != self.scene {
            self.scene = identity;
            self.drew = true;
        }
    }

    pub(crate) fn end(&mut self, timestamp: f64, target: f64) {
        let (Some(log), Some(started)) = (self.log.as_mut(), self.tick_started.take()) else {
            return;
        };
        let cpu = started.elapsed().as_secs_f64() * 1000.0;
        log.tick(timestamp, target - timestamp, self.drew.then_some(cpu));
        if self.bench.as_ref().is_some_and(|bench| bench.finished) {
            self.bench = None;
            log.flush();
            log.report("total", &log.total);
            eprintln!("[zz-ios] bench done");
        }
    }

    pub(crate) fn bench_input(
        &mut self,
        now: Instant,
        size: Size<Pixels>,
    ) -> Option<PlatformInput> {
        let (phase, position) = self.bench.as_mut()?.touch(now, size)?;
        Some(PlatformInput::Touch(TouchEvent {
            id: TouchId(u64::MAX),
            phase,
            position,
            predicted_position: None,
            force: None,
        }))
    }
}

#[derive(Clone, Default)]
struct Samples {
    intervals: Vec<f64>,
    cpu: Vec<f64>,
    dropped: u32,
    period: f64,
}

impl Samples {
    fn extend(&mut self, other: &Samples) {
        self.intervals.extend_from_slice(&other.intervals);
        self.cpu.extend_from_slice(&other.cpu);
        self.dropped += other.dropped;
        self.period = other.period;
    }
}

#[derive(Default)]
struct FrameLog {
    last_draw: Option<f64>,
    burst: Samples,
    total: Samples,
    window_start: Option<f64>,
    window_ticks: u32,
    window_draws: u32,
    window_touches: u32,
}

impl FrameLog {
    fn tick(&mut self, timestamp: f64, period: f64, cpu: Option<f64>) {
        let start = *self.window_start.get_or_insert(timestamp);
        if timestamp - start >= LINK_REPORT {
            eprintln!(
                "[zz-ios] link: ticks={} draws={} touch_moves={} over {:.1}s",
                self.window_ticks,
                self.window_draws,
                self.window_touches,
                timestamp - start
            );
            self.window_start = Some(timestamp);
            self.window_ticks = 0;
            self.window_draws = 0;
            self.window_touches = 0;
        }
        self.window_ticks += 1;
        self.window_draws += u32::from(cpu.is_some());
        if self
            .last_draw
            .is_some_and(|last| timestamp - last > BURST_GAP)
        {
            self.flush();
        }
        let Some(cpu) = cpu else {
            return;
        };
        if let Some(last) = self.last_draw {
            let interval = timestamp - last;
            if period > 0.0 {
                self.burst.dropped += ((interval / period).round() as u32).saturating_sub(1);
            }
            self.burst.intervals.push(interval * 1000.0);
        }
        self.burst.period = period * 1000.0;
        self.burst.cpu.push(cpu);
        self.last_draw = Some(timestamp);
    }

    fn flush(&mut self) {
        let burst = std::mem::take(&mut self.burst);
        self.last_draw = None;
        if burst.cpu.len() < MIN_BURST {
            return;
        }
        self.report("burst", &burst);
        self.total.extend(&burst);
    }

    fn report(&self, label: &str, samples: &Samples) {
        let mut intervals = samples.intervals.clone();
        let mut cpu = samples.cpu.clone();
        eprintln!(
            "[zz-ios] frames {label}: n={} hz={:.0} interval p50={:.2} p95={:.2} max={:.2} dropped={} cpu p50={:.2} p95={:.2} max={:.2}",
            cpu.len(),
            if samples.period > 0.0 {
                1000.0 / samples.period
            } else {
                0.0
            },
            percentile(&mut intervals, 0.5),
            percentile(&mut intervals, 0.95),
            percentile(&mut intervals, 1.0),
            samples.dropped,
            percentile(&mut cpu, 0.5),
            percentile(&mut cpu, 0.95),
            percentile(&mut cpu, 1.0),
        );
    }
}

fn percentile(values: &mut [f64], fraction: f64) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    values.sort_by(f64::total_cmp);
    let index = ((values.len() - 1) as f64 * fraction).round() as usize;
    values[index]
}

#[derive(Clone, Copy, PartialEq)]
enum BenchKind {
    Swipe,
    Scroll,
    Drag,
    Fling,
}

struct Bench {
    kind: BenchKind,
    cycles: u32,
    started: Option<Instant>,
    touching: bool,
    finished: bool,
}

impl Bench {
    fn parse(value: &str) -> Option<Self> {
        let (kind, cycles) = value.split_once(':').unwrap_or((value, ""));
        let kind = match kind {
            "swipe" => BenchKind::Swipe,
            "scroll" => BenchKind::Scroll,
            "drag" => BenchKind::Drag,
            "fling" => BenchKind::Fling,
            _ => return None,
        };
        Some(Self {
            kind,
            cycles: cycles.parse().unwrap_or(DEFAULT_CYCLES),
            started: None,
            touching: false,
            finished: false,
        })
    }

    fn timing(&self, cycle: u32) -> (Duration, Duration, Duration) {
        match self.kind {
            BenchKind::Swipe => (
                Duration::from_millis(1400),
                Duration::from_millis(160),
                Duration::ZERO,
            ),
            BenchKind::Scroll => (
                Duration::from_millis(3000),
                Duration::from_millis(120),
                Duration::ZERO,
            ),
            BenchKind::Drag if cycle % 2 == 1 => (
                Duration::from_millis(3000),
                Duration::from_millis(120),
                Duration::ZERO,
            ),
            BenchKind::Drag => (
                Duration::from_millis(3000),
                Duration::from_millis(1500),
                Duration::from_millis(150),
            ),
            BenchKind::Fling => (
                Duration::from_millis(4000),
                Duration::from_millis(120),
                Duration::ZERO,
            ),
        }
    }

    fn position(&self, cycle: u32, progress: f32, size: Size<Pixels>) -> Point<Pixels> {
        let (width, height) = (f32::from(size.width), f32::from(size.height));
        let older = match self.kind {
            BenchKind::Fling => cycle % 5 < 2,
            _ => cycle.is_multiple_of(2),
        };
        let progress = if older { progress } else { 1.0 - progress };
        match self.kind {
            BenchKind::Swipe => point(px(width * (0.8 - 0.6 * progress)), px(height * 0.4)),
            BenchKind::Scroll | BenchKind::Drag | BenchKind::Fling => {
                point(px(width * 0.5), px(height * (0.35 + 0.3 * progress)))
            }
        }
    }

    fn touch(&mut self, now: Instant, size: Size<Pixels>) -> Option<(TouchPhase, Point<Pixels>)> {
        if self.finished {
            return None;
        }
        let started = *self.started.get_or_insert(now + BENCH_DELAY);
        let elapsed = now.checked_duration_since(started)?;
        let period = self.timing(0).0;
        let cycle = (elapsed.as_millis() / period.as_millis()) as u32;
        let (_, stroke, hold) = self.timing(cycle);
        if cycle >= self.cycles {
            self.finished = !self.touching;
            return self.touching.then(|| {
                self.touching = false;
                (TouchPhase::Ended, self.position(self.cycles - 1, 1.0, size))
            });
        }
        let into = elapsed - period * cycle;
        if into <= stroke {
            let progress = into.as_secs_f32() / stroke.as_secs_f32();
            let phase = if self.touching {
                TouchPhase::Moved
            } else {
                TouchPhase::Started
            };
            self.touching = true;
            Some((phase, self.position(cycle, progress, size)))
        } else if self.touching && into > stroke + hold {
            self.touching = false;
            Some((TouchPhase::Ended, self.position(cycle, 1.0, size)))
        } else {
            None
        }
    }
}
