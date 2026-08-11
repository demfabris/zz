use std::{
    collections::{HashMap, HashSet, hash_map::DefaultHasher},
    hash::{Hash as _, Hasher as _},
    time::{Duration, Instant},
};

use zz_protocol::{BrowserCommand, BrowserDescriptor, PaneId};
use zz_terminal::{KittyLayer, KittyPlacement};

use crate::kitty::{FrameTransport, KittyImageData};

pub(crate) const BROWSER_IMAGE_ID: u32 = 1;
const ZOOM_STEP: f32 = 1.2;
const MIN_ZOOM: f32 = 0.4;
const MAX_ZOOM: f32 = 3.0;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum BrowserZoomStep {
    In,
    Out,
    Reset,
}
const FILE_FRAME_INTERVAL: Duration = Duration::from_millis(8);
const MAX_INLINE_FRAME_DELAY: Duration = Duration::from_millis(250);
const MAX_PUMP_WAIT: Duration = Duration::from_millis(50);
const INLINE_MAX_FRAME_BYTES: usize = 15 * 1024 * 1024;
const FILE_MAX_FRAME_BYTES: usize = 64 * 1024 * 1024;
const FRAME_BYTE_HEADROOM: usize = 1024 * 1024;
const DEFAULT_INLINE_BUDGET_BYTES_PER_SECOND: f64 = 3.0 * 1024.0 * 1024.0;

/// A complete browser frame in CEF's native OSR byte order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderFrame {
    pub width: u32,
    pub height: u32,
    pub premultiplied_bgra: Vec<u8>,
    pub damage: Option<(u32, u32, u32, u32)>,
}

/// Work produced by one browser-runtime pump.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ProviderTick {
    pub frames: Vec<(PaneId, ProviderFrame)>,
    pub navigations: Vec<(PaneId, Vec<String>, usize)>,
    pub next_due: Option<Duration>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProviderPointerPhase {
    Move,
    Down,
    Up,
    Wheel,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProviderPointerButton {
    Left,
    Middle,
    Right,
}

#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ProviderModifiers(u8);

impl ProviderModifiers {
    const SHIFT: u8 = 1 << 0;
    const CONTROL: u8 = 1 << 1;
    const ALT: u8 = 1 << 2;
    const PLATFORM: u8 = 1 << 3;

    #[must_use]
    #[allow(
        clippy::fn_params_excessive_bools,
        reason = "the constructor is the provider boundary for four modifier flags"
    )]
    pub const fn new(shift: bool, control: bool, alt: bool, platform: bool) -> Self {
        let mut bits = 0;
        if shift {
            bits |= Self::SHIFT;
        }
        if control {
            bits |= Self::CONTROL;
        }
        if alt {
            bits |= Self::ALT;
        }
        if platform {
            bits |= Self::PLATFORM;
        }
        Self(bits)
    }

    #[must_use]
    pub const fn shift(self) -> bool {
        self.0 & Self::SHIFT != 0
    }

    #[must_use]
    pub const fn control(self) -> bool {
        self.0 & Self::CONTROL != 0
    }

    #[must_use]
    pub const fn alt(self) -> bool {
        self.0 & Self::ALT != 0
    }

    #[must_use]
    pub const fn platform(self) -> bool {
        self.0 & Self::PLATFORM != 0
    }
}

/// Pointer input relative to a browser pane's content surface, in pixels.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProviderPointerInput {
    pub x: i32,
    pub y: i32,
    pub phase: ProviderPointerPhase,
    pub button: Option<ProviderPointerButton>,
    pub click_count: i32,
    pub wheel_delta_x: i32,
    pub wheel_delta_y: i32,
    pub modifiers: ProviderModifiers,
}

/// Supplies client-local browser frames without adding CEF to `zz-tui`.
///
/// Every method runs on the TUI's main thread.
pub trait BrowserFrameProvider {
    fn open(&mut self, pane: PaneId, descriptor: &BrowserDescriptor, px: (u32, u32), scale: f32);
    fn resize(&mut self, pane: PaneId, px: (u32, u32), scale: f32);
    fn close(&mut self, pane: PaneId);
    fn close_all(&mut self);
    fn pointer(&mut self, pane: PaneId, input: ProviderPointerInput);
    fn command(&mut self, pane: PaneId, command: &BrowserCommand);

    /// Drives the browser runtime from the TUI's receive loop.
    fn pump(&mut self) -> ProviderTick;
}

#[must_use]
pub(crate) fn clamp_surface_px(px: (u32, u32), max_surface_bytes: u64) -> (u32, u32) {
    let bytes = u64::from(px.0) * u64::from(px.1) * 4;
    if bytes <= max_surface_bytes || px.0 == 0 || px.1 == 0 {
        return px;
    }
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "surface dimensions are far below f64 precision limits"
    )]
    {
        let ratio = (max_surface_bytes as f64 / bytes as f64).sqrt();
        (
            ((f64::from(px.0) * ratio) as u32).max(1),
            ((f64::from(px.1) * ratio) as u32).max(1),
        )
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct BrowserSurface {
    pub pane: PaneId,
    pub descriptor: BrowserDescriptor,
    pub cells: (u16, u16),
    pub px: (u32, u32),
    pub base_scale: f32,
}

#[derive(Debug)]
pub(crate) struct BrowserFrameUpdate {
    pub image: KittyImageData,
    pub placement: KittyPlacement,
}

#[derive(Default)]
pub(crate) struct BrowserPumpOutput {
    pub frames: Vec<BrowserFrameUpdate>,
    pub navigations: Vec<(PaneId, Vec<String>, usize)>,
}

#[derive(Default)]
pub(crate) struct SurfaceChanges {
    pub closed: Vec<PaneId>,
    pub resized: Vec<(PaneId, (u16, u16))>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum BrowserWait {
    Blocking,
    Timeout(Duration),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FrameIdentity {
    Damaged,
    Hashed { width: u32, height: u32, hash: u64 },
}

impl FrameIdentity {
    fn dedupes(self, installed: Self) -> bool {
        matches!(self, Self::Hashed { .. }) && self == installed
    }
}

struct PendingFrame {
    frame: ProviderFrame,
    identity: FrameIdentity,
}

struct InstalledFrame {
    identity: FrameIdentity,
    at: Instant,
    transmit_bytes: usize,
}

pub(crate) struct BrowserState {
    provider: Option<Box<dyn BrowserFrameProvider>>,
    enabled: bool,
    provider_closed: bool,
    surfaces: HashMap<PaneId, BrowserSurface>,
    zooms: HashMap<PaneId, f32>,
    pending_frames: HashMap<PaneId, PendingFrame>,
    installed_frames: HashMap<PaneId, InstalledFrame>,
    generations: HashMap<PaneId, u64>,
    next_pump: Option<Instant>,
    transport: FrameTransport,
    max_frame_bytes: usize,
    inline_budget_bytes_per_second: f64,
}

impl BrowserState {
    pub fn new(provider: Option<Box<dyn BrowserFrameProvider>>) -> Self {
        Self {
            provider,
            enabled: false,
            provider_closed: false,
            surfaces: HashMap::new(),
            zooms: HashMap::new(),
            pending_frames: HashMap::new(),
            installed_frames: HashMap::new(),
            generations: HashMap::new(),
            next_pump: None,
            transport: FrameTransport::Inline,
            max_frame_bytes: INLINE_MAX_FRAME_BYTES,
            inline_budget_bytes_per_second: configured_inline_budget_bytes_per_second(),
        }
    }

    pub fn set_transport(&mut self, transport: FrameTransport, now: Instant) -> bool {
        if self.transport == transport {
            return false;
        }
        self.transport = transport;
        self.max_frame_bytes = max_frame_bytes(transport);
        if self.active() {
            self.next_pump = Some(now);
        }
        true
    }

    pub const fn surface_byte_budget(&self) -> u64 {
        self.max_frame_bytes as u64
    }

    pub fn note_transmit_cost(&mut self, pane: PaneId, bytes: usize) {
        if let Some(installed) = self.installed_frames.get_mut(&pane) {
            installed.transmit_bytes = bytes;
        }
    }

    fn effective_scale(&self, pane: PaneId, base_scale: f32) -> f32 {
        let base = if base_scale.is_finite() && base_scale > 0.0 {
            base_scale
        } else {
            1.0
        };
        base * self.zooms.get(&pane).copied().unwrap_or(1.0)
    }

    pub fn zoom(&mut self, pane: PaneId, step: BrowserZoomStep) -> bool {
        if !self.has_surface(pane) || self.provider.is_none() {
            return false;
        }
        let zoom = self.zooms.entry(pane).or_insert(1.0);
        let next = match step {
            BrowserZoomStep::In => (*zoom * ZOOM_STEP).min(MAX_ZOOM),
            BrowserZoomStep::Out => (*zoom / ZOOM_STEP).max(MIN_ZOOM),
            BrowserZoomStep::Reset => 1.0,
        };
        if (*zoom - next).abs() < f32::EPSILON {
            return false;
        }
        *zoom = next;
        let Some(surface) = self.surfaces.get(&pane) else {
            return false;
        };
        let px = surface.px;
        let scale = self.effective_scale(pane, surface.base_scale);
        self.pending_frames.remove(&pane);
        self.provider
            .as_mut()
            .expect("provider checked above")
            .resize(pane, px, scale);
        true
    }

    pub const fn enable(&mut self) {
        self.enabled = true;
    }

    pub fn disable(&mut self) {
        self.enabled = false;
        self.close_all();
    }

    pub fn reset_connection(&mut self) {
        self.close_all();
    }

    pub fn close_all(&mut self) {
        if !self.provider_closed {
            if let Some(provider) = self.provider.as_mut() {
                provider.close_all();
            }
            self.provider_closed = true;
        }
        self.surfaces.clear();
        self.zooms.clear();
        self.pending_frames.clear();
        self.installed_frames.clear();
        self.generations.clear();
        self.next_pump = None;
    }

    pub fn reconcile_surfaces(
        &mut self,
        surfaces: Vec<BrowserSurface>,
        now: Instant,
    ) -> SurfaceChanges {
        if !self.enabled || self.provider.is_none() {
            return SurfaceChanges::default();
        }

        let desired = surfaces
            .into_iter()
            .filter(|surface| {
                surface.cells.0 > 0 && surface.cells.1 > 0 && surface.px.0 > 0 && surface.px.1 > 0
            })
            .collect::<Vec<_>>();
        let desired_panes = desired
            .iter()
            .map(|surface| surface.pane)
            .collect::<HashSet<_>>();
        let stale_panes = self
            .surfaces
            .keys()
            .filter(|pane| !desired_panes.contains(pane))
            .copied()
            .collect::<Vec<_>>();
        let mut changes = SurfaceChanges::default();
        let provider = self
            .provider
            .as_mut()
            .expect("provider checked before reconciling browser surfaces");

        for pane in stale_panes {
            provider.close(pane);
            self.surfaces.remove(&pane);
            self.zooms.remove(&pane);
            self.pending_frames.remove(&pane);
            self.installed_frames.remove(&pane);
            self.generations.remove(&pane);
            changes.closed.push(pane);
        }

        let mut provider_changed = false;
        for surface in desired {
            let zoom = self.zooms.get(&surface.pane).copied().unwrap_or(1.0);
            let scale = if surface.base_scale.is_finite() && surface.base_scale > 0.0 {
                surface.base_scale * zoom
            } else {
                zoom
            };
            match self.surfaces.get(&surface.pane) {
                None => {
                    provider.open(surface.pane, &surface.descriptor, surface.px, scale);
                    provider_changed = true;
                }
                Some(previous) => {
                    let descriptor_changed = previous.descriptor != surface.descriptor;
                    let pixels_changed = previous.px != surface.px
                        || (previous.base_scale - surface.base_scale).abs() > f32::EPSILON;
                    if descriptor_changed {
                        provider.open(surface.pane, &surface.descriptor, surface.px, scale);
                        provider_changed = true;
                    } else if pixels_changed {
                        provider.resize(surface.pane, surface.px, scale);
                        provider_changed = true;
                    }
                    if descriptor_changed || pixels_changed {
                        self.pending_frames.remove(&surface.pane);
                    }
                    if previous.cells != surface.cells || pixels_changed {
                        changes.resized.push((surface.pane, surface.cells));
                    }
                }
            }
            self.surfaces.insert(surface.pane, surface);
        }

        if !changes.closed.is_empty() || provider_changed {
            self.provider_closed = false;
        }
        if self.surfaces.is_empty() {
            self.pending_frames.clear();
            self.next_pump = None;
        } else if provider_changed || self.next_pump.is_none() {
            self.next_pump = Some(now);
        }
        changes
    }

    pub fn has_surface(&self, pane: PaneId) -> bool {
        self.enabled && self.surfaces.contains_key(&pane)
    }

    pub fn pointer(&mut self, pane: PaneId, input: ProviderPointerInput) -> bool {
        if !self.has_surface(pane) {
            return false;
        }
        let Some(provider) = self.provider.as_mut() else {
            return false;
        };
        provider.pointer(pane, input);
        true
    }

    pub fn command(&mut self, pane: PaneId, command: &BrowserCommand) -> bool {
        if !self.has_surface(pane) {
            return false;
        }
        let Some(provider) = self.provider.as_mut() else {
            return false;
        };
        provider.command(pane, command);
        true
    }

    pub fn should_pump(&self, now: Instant) -> bool {
        self.active() && self.next_pump.is_none_or(|next_pump| next_pump <= now)
    }

    pub fn wait(&self, now: Instant) -> BrowserWait {
        if !self.active() {
            return BrowserWait::Blocking;
        }
        BrowserWait::Timeout(
            self.next_pump
                .map_or(MAX_PUMP_WAIT, |next_pump| {
                    next_pump.saturating_duration_since(now)
                })
                .min(MAX_PUMP_WAIT),
        )
    }

    pub fn pump(&mut self, now: Instant) -> BrowserPumpOutput {
        if !self.active() {
            return BrowserPumpOutput::default();
        }
        let tick = self
            .provider
            .as_mut()
            .expect("active browser state has a provider")
            .pump();
        self.next_pump = Some(deadline(
            now,
            tick.next_due.unwrap_or(MAX_PUMP_WAIT).min(MAX_PUMP_WAIT),
        ));

        let latest = tick.frames.into_iter().collect::<HashMap<_, _>>();
        for (pane, frame) in latest {
            let Some(surface) = self.surfaces.get(&pane) else {
                continue;
            };
            let Some(identity) = frame_identity(&frame, surface.px, self.max_frame_bytes) else {
                log::warn!(
                    "discarding malformed browser frame for {pane}: got {}x{} ({} bytes), surface expects {}x{}",
                    frame.width,
                    frame.height,
                    frame.premultiplied_bgra.len(),
                    surface.px.0,
                    surface.px.1,
                );
                continue;
            };
            self.pending_frames
                .insert(pane, PendingFrame { frame, identity });
            if self
                .installed_frames
                .get(&pane)
                .is_some_and(|installed| identity.dedupes(installed.identity))
            {
                self.pending_frames.remove(&pane);
            }
        }

        let mut frames = Vec::new();
        let due = self
            .pending_frames
            .keys()
            .filter(|pane| {
                self.installed_frames
                    .get(pane)
                    .is_none_or(|installed| self.frame_deadline(installed) <= now)
            })
            .copied()
            .collect::<Vec<_>>();
        for pane in due {
            let Some(pending) = self.pending_frames.remove(&pane) else {
                continue;
            };
            let Some(surface) = self.surfaces.get(&pane) else {
                continue;
            };
            let generation = self.generations.entry(pane).or_default();
            *generation = generation.wrapping_add(1).max(1);
            let image_generation = *generation;
            let frame = pending.frame;
            frames.push(BrowserFrameUpdate {
                image: KittyImageData {
                    pane,
                    image_id: BROWSER_IMAGE_ID,
                    generation: image_generation,
                    width: frame.width,
                    height: frame.height,
                    bytes: frame.premultiplied_bgra,
                },
                placement: KittyPlacement {
                    image_id: BROWSER_IMAGE_ID,
                    image_generation,
                    layer: KittyLayer::AboveText,
                    viewport_col: 0,
                    viewport_row: 0,
                    absolute_row: 0,
                    cell_offset_x: 0,
                    cell_offset_y: 0,
                    grid_cols: u32::from(surface.cells.0),
                    grid_rows: u32::from(surface.cells.1),
                    pixel_width: frame.width,
                    pixel_height: frame.height,
                    source_rect: None,
                },
            });
            self.installed_frames.insert(
                pane,
                InstalledFrame {
                    identity: pending.identity,
                    at: now,
                    transmit_bytes: 0,
                },
            );
        }

        if let Some(next_frame) = self
            .pending_frames
            .keys()
            .filter_map(|pane| {
                self.installed_frames
                    .get(pane)
                    .map(|installed| self.frame_deadline(installed))
            })
            .min()
        {
            self.next_pump = Some(
                self.next_pump
                    .map_or(next_frame, |next_pump| next_pump.min(next_frame)),
            );
        }

        BrowserPumpOutput {
            frames,
            navigations: tick
                .navigations
                .into_iter()
                .filter(|(pane, _, _)| self.surfaces.contains_key(pane))
                .collect(),
        }
    }

    fn active(&self) -> bool {
        self.enabled && self.provider.is_some() && !self.surfaces.is_empty()
    }

    fn frame_deadline(&self, installed: &InstalledFrame) -> Instant {
        let delay = match self.transport {
            FrameTransport::File => FILE_FRAME_INTERVAL,
            FrameTransport::Inline => inline_transmit_delay(
                installed.transmit_bytes,
                self.inline_budget_bytes_per_second,
            )
            .min(MAX_INLINE_FRAME_DELAY),
        };
        deadline(installed.at, delay)
    }
}

impl Drop for BrowserState {
    fn drop(&mut self) {
        self.close_all();
    }
}

fn frame_identity(
    frame: &ProviderFrame,
    expected_px: (u32, u32),
    max_frame_bytes: usize,
) -> Option<FrameIdentity> {
    const SCALE_ROUNDING_TOLERANCE: u32 = 4;
    let close = |actual: u32, expected: u32| actual.abs_diff(expected) <= SCALE_ROUNDING_TOLERANCE;
    if frame.width == 0
        || frame.height == 0
        || !close(frame.width, expected_px.0)
        || !close(frame.height, expected_px.1)
    {
        return None;
    }
    let expected_bytes = usize::try_from(frame.width)
        .ok()?
        .checked_mul(usize::try_from(frame.height).ok()?)?
        .checked_mul(4)?;
    if expected_bytes != frame.premultiplied_bgra.len()
        || expected_bytes > max_frame_bytes.saturating_add(FRAME_BYTE_HEADROOM)
    {
        return None;
    }
    if frame.damage.is_some() {
        return Some(FrameIdentity::Damaged);
    }
    let mut hasher = DefaultHasher::new();
    frame.width.hash(&mut hasher);
    frame.height.hash(&mut hasher);
    frame.premultiplied_bgra.hash(&mut hasher);
    Some(FrameIdentity::Hashed {
        width: frame.width,
        height: frame.height,
        hash: hasher.finish(),
    })
}

const fn max_frame_bytes(transport: FrameTransport) -> usize {
    match transport {
        FrameTransport::File => FILE_MAX_FRAME_BYTES,
        FrameTransport::Inline => INLINE_MAX_FRAME_BYTES,
    }
}

fn configured_inline_budget_bytes_per_second() -> f64 {
    std::env::var("ZZ_TUI_FRAME_BUDGET_MBPS")
        .ok()
        .and_then(|value| value.trim().parse::<f64>().ok())
        .filter(|value| value.is_finite() && *value > 0.0)
        .map_or(DEFAULT_INLINE_BUDGET_BYTES_PER_SECOND, |megabytes| {
            megabytes * 1024.0 * 1024.0
        })
}

#[allow(
    clippy::cast_precision_loss,
    reason = "transmit costs are bounded by the 64 MiB frame ceiling"
)]
fn inline_transmit_delay(bytes: usize, bytes_per_second: f64) -> Duration {
    if bytes == 0 || !bytes_per_second.is_finite() || bytes_per_second <= 0.0 {
        return Duration::ZERO;
    }
    let seconds = (bytes as f64 / bytes_per_second).min(Duration::MAX.as_secs_f64());
    Duration::from_secs_f64(seconds)
}

fn deadline(now: Instant, after: Duration) -> Instant {
    now.checked_add(after).unwrap_or(now)
}

#[cfg(test)]
pub(crate) mod test_support {
    use std::{cell::RefCell, collections::VecDeque, rc::Rc};

    use super::*;

    #[derive(Clone, Debug, Eq, PartialEq)]
    pub(crate) enum FakeCall {
        Open(PaneId, BrowserDescriptor, (u32, u32), u32),
        Resize(PaneId, (u32, u32), u32),
        Close(PaneId),
        CloseAll,
        Pointer(PaneId, ProviderPointerInput),
        Command(PaneId, BrowserCommand),
        Pump,
    }

    #[derive(Default)]
    struct FakeState {
        calls: Vec<FakeCall>,
        ticks: VecDeque<ProviderTick>,
    }

    #[derive(Clone)]
    pub(crate) struct FakeHandle(Rc<RefCell<FakeState>>);

    impl FakeHandle {
        pub fn push_tick(&self, tick: ProviderTick) {
            self.0.borrow_mut().ticks.push_back(tick);
        }

        pub fn calls(&self) -> Vec<FakeCall> {
            self.0.borrow().calls.clone()
        }

        pub fn take_calls(&self) -> Vec<FakeCall> {
            std::mem::take(&mut self.0.borrow_mut().calls)
        }
    }

    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "test scales are finite positive values near one"
    )]
    pub(crate) fn scale_key(scale: f32) -> u32 {
        (scale * 100.0).round().max(0.0) as u32
    }

    pub(crate) fn fake_provider() -> (Box<dyn BrowserFrameProvider>, FakeHandle) {
        let state = Rc::new(RefCell::new(FakeState::default()));
        (Box::new(FakeProvider(Rc::clone(&state))), FakeHandle(state))
    }

    struct FakeProvider(Rc<RefCell<FakeState>>);

    impl BrowserFrameProvider for FakeProvider {
        fn open(
            &mut self,
            pane: PaneId,
            descriptor: &BrowserDescriptor,
            px: (u32, u32),
            scale: f32,
        ) {
            self.0.borrow_mut().calls.push(FakeCall::Open(
                pane,
                descriptor.clone(),
                px,
                scale_key(scale),
            ));
        }

        fn resize(&mut self, pane: PaneId, px: (u32, u32), scale: f32) {
            self.0
                .borrow_mut()
                .calls
                .push(FakeCall::Resize(pane, px, scale_key(scale)));
        }

        fn close(&mut self, pane: PaneId) {
            self.0.borrow_mut().calls.push(FakeCall::Close(pane));
        }

        fn close_all(&mut self) {
            self.0.borrow_mut().calls.push(FakeCall::CloseAll);
        }

        fn pointer(&mut self, pane: PaneId, input: ProviderPointerInput) {
            self.0
                .borrow_mut()
                .calls
                .push(FakeCall::Pointer(pane, input));
        }

        fn command(&mut self, pane: PaneId, command: &BrowserCommand) {
            self.0
                .borrow_mut()
                .calls
                .push(FakeCall::Command(pane, command.clone()));
        }

        fn pump(&mut self) -> ProviderTick {
            let mut state = self.0.borrow_mut();
            state.calls.push(FakeCall::Pump);
            state.ticks.pop_front().unwrap_or_default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{test_support::*, *};
    use crate::{kitty::KittyBridge, layout::Rect};

    fn descriptor(url: &str) -> BrowserDescriptor {
        BrowserDescriptor::single(url.to_owned(), "default".to_owned())
    }

    fn surface(pane: u64, url: &str, cells: (u16, u16), px: (u32, u32)) -> BrowserSurface {
        BrowserSurface {
            pane: PaneId(pane),
            descriptor: descriptor(url),
            cells,
            px,
            base_scale: 1.0,
        }
    }

    fn frame(width: u32, height: u32, blue: u8) -> ProviderFrame {
        ProviderFrame {
            width,
            height,
            premultiplied_bgra: [blue, 0, 0, 255].repeat(usize::try_from(width * height).unwrap()),
            damage: None,
        }
    }

    fn damaged_frame(width: u32, height: u32, blue: u8) -> ProviderFrame {
        ProviderFrame {
            damage: Some((0, 0, width, height)),
            ..frame(width, height, blue)
        }
    }

    #[test]
    fn surface_diff_opens_resizes_reopens_and_closes() {
        let (provider, fake) = fake_provider();
        let mut state = BrowserState::new(Some(provider));
        let now = Instant::now();
        state.enable();

        state.reconcile_surfaces(vec![surface(1, "https://one", (4, 3), (32, 48))], now);
        state.reconcile_surfaces(vec![surface(1, "https://one", (5, 3), (40, 48))], now);
        state.reconcile_surfaces(vec![surface(1, "https://two", (5, 3), (40, 48))], now);
        state.reconcile_surfaces(Vec::new(), now);

        assert_eq!(
            fake.take_calls(),
            [
                FakeCall::Open(PaneId(1), descriptor("https://one"), (32, 48), 100),
                FakeCall::Resize(PaneId(1), (40, 48), 100),
                FakeCall::Open(PaneId(1), descriptor("https://two"), (40, 48), 100),
                FakeCall::Close(PaneId(1)),
            ]
        );
        assert_eq!(state.wait(now), BrowserWait::Blocking);
    }

    #[test]
    fn frames_coalesce_until_the_file_floor_and_keep_generation_in_lockstep() {
        let (provider, fake) = fake_provider();
        let mut state = BrowserState::new(Some(provider));
        let started = Instant::now();
        state.set_transport(FrameTransport::File, started);
        state.enable();
        state.reconcile_surfaces(vec![surface(1, "https://one", (2, 1), (2, 1))], started);
        fake.take_calls();

        fake.push_tick(ProviderTick {
            frames: vec![(PaneId(1), frame(2, 1, 1))],
            ..ProviderTick::default()
        });
        let mut first = state.pump(started).frames;
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].image.generation, 1);
        assert_eq!(first[0].placement.image_generation, 1);

        let update = first.pop().unwrap();
        let mut bridge = KittyBridge::default();
        let mut control = Vec::new();
        let mut placement_output = Vec::new();
        bridge.enable(&mut control);
        bridge.install(update.image, &mut control);
        bridge.reconcile(
            [(
                PaneId(1),
                Rect {
                    width: 2,
                    height: 1,
                    ..Rect::default()
                },
                std::slice::from_ref(&update.placement),
            )],
            &mut placement_output,
        );
        assert!(String::from_utf8_lossy(&control).contains("\x1b_Ga=t"));
        assert!(String::from_utf8_lossy(&placement_output).contains("\x1b_Ga=p"));

        fake.push_tick(ProviderTick {
            frames: vec![(PaneId(1), frame(2, 1, 2))],
            ..ProviderTick::default()
        });
        assert!(
            state
                .pump(deadline(started, Duration::from_millis(4)))
                .frames
                .is_empty()
        );
        fake.push_tick(ProviderTick {
            frames: vec![(PaneId(1), frame(2, 1, 3))],
            ..ProviderTick::default()
        });
        assert!(
            state
                .pump(deadline(started, Duration::from_millis(7)))
                .frames
                .is_empty()
        );
        fake.push_tick(ProviderTick::default());
        let second = state.pump(deadline(started, FILE_FRAME_INTERVAL)).frames;
        assert_eq!(second.len(), 1);
        assert_eq!(second[0].image.generation, 2);
        assert_eq!(second[0].placement.image_generation, 2);
        assert_eq!(second[0].image.bytes, frame(2, 1, 3).premultiplied_bgra);

        fake.push_tick(ProviderTick {
            frames: vec![(PaneId(1), frame(2, 1, 3))],
            ..ProviderTick::default()
        });
        assert!(
            state
                .pump(deadline(started, FILE_FRAME_INTERVAL * 2))
                .frames
                .is_empty()
        );
    }

    #[test]
    fn scale_rounding_overshoot_past_the_budget_still_validates() {
        let expected_px = (1425, 2758);
        let overshoot = ProviderFrame {
            width: 1426,
            height: 2758,
            premultiplied_bgra: vec![0; 1426 * 2758 * 4],
            damage: Some((0, 0, 1426, 2758)),
        };
        assert!(frame_identity(&overshoot, expected_px, INLINE_MAX_FRAME_BYTES).is_some());
        let far_past = ProviderFrame {
            premultiplied_bgra: vec![0; 1426 * 2758 * 4],
            ..overshoot
        };
        assert!(
            frame_identity(
                &far_past,
                expected_px,
                INLINE_MAX_FRAME_BYTES - FRAME_BYTE_HEADROOM
            )
            .is_none()
        );
    }

    #[test]
    fn surface_clamp_uses_the_transport_budget() {
        let inline_budget = INLINE_MAX_FRAME_BYTES as u64;
        let file_budget = FILE_MAX_FRAME_BYTES as u64;
        assert_eq!(clamp_surface_px((800, 600), inline_budget), (800, 600));
        let (w, h) = clamp_surface_px((5120, 2880), inline_budget);
        assert!(u64::from(w) * u64::from(h) * 4 <= inline_budget);
        let original = f64::from(5120u32) / f64::from(2880u32);
        let clamped = f64::from(w) / f64::from(h);
        assert!((original - clamped).abs() < 0.01);
        assert_eq!(clamp_surface_px((5120, 2880), file_budget), (5120, 2880));
        assert_eq!(clamp_surface_px((0, 5000), inline_budget), (0, 5000));
    }

    #[test]
    fn damage_installs_each_paint_while_unknown_damage_dedupes_by_hash() {
        let (provider, fake) = fake_provider();
        let mut state = BrowserState::new(Some(provider));
        let started = Instant::now();
        state.set_transport(FrameTransport::File, started);
        state.enable();
        state.reconcile_surfaces(vec![surface(1, "https://one", (2, 1), (2, 1))], started);

        fake.push_tick(ProviderTick {
            frames: vec![(PaneId(1), damaged_frame(2, 1, 7))],
            ..ProviderTick::default()
        });
        assert_eq!(state.pump(started).frames.len(), 1);
        fake.push_tick(ProviderTick {
            frames: vec![(PaneId(1), damaged_frame(2, 1, 7))],
            ..ProviderTick::default()
        });
        assert_eq!(
            state
                .pump(deadline(started, FILE_FRAME_INTERVAL))
                .frames
                .len(),
            1
        );

        fake.push_tick(ProviderTick {
            frames: vec![(PaneId(1), frame(2, 1, 9))],
            ..ProviderTick::default()
        });
        assert_eq!(
            state
                .pump(deadline(started, FILE_FRAME_INTERVAL * 2))
                .frames
                .len(),
            1
        );
        fake.push_tick(ProviderTick {
            frames: vec![(PaneId(1), frame(2, 1, 9))],
            ..ProviderTick::default()
        });
        assert!(
            state
                .pump(deadline(started, FILE_FRAME_INTERVAL * 3))
                .frames
                .is_empty()
        );
    }

    #[test]
    fn inline_debt_uses_the_budget_and_caps_the_install_delay() {
        let six_mebibytes = 6 * 1024 * 1024;
        let three_mebibytes_per_second = 3.0 * 1024.0 * 1024.0;
        assert_eq!(
            inline_transmit_delay(six_mebibytes, three_mebibytes_per_second),
            Duration::from_secs(2)
        );

        let (provider, fake) = fake_provider();
        let mut state = BrowserState::new(Some(provider));
        state.inline_budget_bytes_per_second = three_mebibytes_per_second;
        let started = Instant::now();
        state.enable();
        state.reconcile_surfaces(vec![surface(1, "https://one", (1, 1), (1, 1))], started);
        fake.push_tick(ProviderTick {
            frames: vec![(PaneId(1), damaged_frame(1, 1, 1))],
            ..ProviderTick::default()
        });
        assert_eq!(state.pump(started).frames.len(), 1);
        state.note_transmit_cost(PaneId(1), six_mebibytes);

        fake.push_tick(ProviderTick {
            frames: vec![(PaneId(1), damaged_frame(1, 1, 2))],
            ..ProviderTick::default()
        });
        assert!(
            state
                .pump(deadline(
                    started,
                    MAX_INLINE_FRAME_DELAY
                        .checked_sub(Duration::from_millis(1))
                        .unwrap()
                ))
                .frames
                .is_empty()
        );
        fake.push_tick(ProviderTick::default());
        assert_eq!(
            state
                .pump(deadline(started, MAX_INLINE_FRAME_DELAY))
                .frames
                .len(),
            1
        );
    }

    #[test]
    fn file_transport_enforces_the_eight_millisecond_floor() {
        let (provider, fake) = fake_provider();
        let mut state = BrowserState::new(Some(provider));
        let started = Instant::now();
        state.set_transport(FrameTransport::File, started);
        state.enable();
        state.reconcile_surfaces(vec![surface(1, "https://one", (1, 1), (1, 1))], started);
        fake.push_tick(ProviderTick {
            frames: vec![(PaneId(1), damaged_frame(1, 1, 1))],
            ..ProviderTick::default()
        });
        assert_eq!(state.pump(started).frames.len(), 1);
        fake.push_tick(ProviderTick {
            frames: vec![(PaneId(1), damaged_frame(1, 1, 2))],
            ..ProviderTick::default()
        });
        assert!(
            state
                .pump(deadline(
                    started,
                    FILE_FRAME_INTERVAL
                        .checked_sub(Duration::from_millis(1))
                        .unwrap()
                ))
                .frames
                .is_empty()
        );
        fake.push_tick(ProviderTick::default());
        assert_eq!(
            state
                .pump(deadline(started, FILE_FRAME_INTERVAL))
                .frames
                .len(),
            1
        );
    }

    #[test]
    fn zoom_steps_scale_the_surface_and_reset_returns_to_base() {
        let (provider, fake) = fake_provider();
        let mut state = BrowserState::new(Some(provider));
        let now = Instant::now();
        state.enable();
        let mut hidpi = surface(1, "https://one", (4, 3), (32, 48));
        hidpi.base_scale = 2.0;
        state.reconcile_surfaces(vec![hidpi], now);
        assert_eq!(
            fake.take_calls(),
            [FakeCall::Open(
                PaneId(1),
                descriptor("https://one"),
                (32, 48),
                200,
            )]
        );

        assert!(state.zoom(PaneId(1), BrowserZoomStep::In));
        assert!(state.zoom(PaneId(1), BrowserZoomStep::Reset));
        assert!(!state.zoom(PaneId(1), BrowserZoomStep::Reset));
        assert!(!state.zoom(PaneId(2), BrowserZoomStep::In));
        assert_eq!(
            fake.take_calls(),
            [
                FakeCall::Resize(PaneId(1), (32, 48), 240),
                FakeCall::Resize(PaneId(1), (32, 48), 200),
            ]
        );
    }

    #[test]
    fn provider_routes_commands_and_pointer_only_for_open_surfaces() {
        let (provider, fake) = fake_provider();
        let mut state = BrowserState::new(Some(provider));
        let now = Instant::now();
        let pointer = ProviderPointerInput {
            x: 9,
            y: 7,
            phase: ProviderPointerPhase::Down,
            button: Some(ProviderPointerButton::Left),
            click_count: 1,
            wheel_delta_x: 0,
            wheel_delta_y: 0,
            modifiers: ProviderModifiers::default(),
        };
        state.enable();
        state.reconcile_surfaces(vec![surface(1, "https://one", (2, 2), (16, 32))], now);
        fake.take_calls();

        assert!(state.pointer(PaneId(1), pointer));
        assert!(state.command(PaneId(1), &BrowserCommand::Reload));
        assert!(!state.pointer(PaneId(2), pointer));
        assert!(!state.command(PaneId(2), &BrowserCommand::Reload));
        assert_eq!(
            fake.take_calls(),
            [
                FakeCall::Pointer(PaneId(1), pointer),
                FakeCall::Command(PaneId(1), BrowserCommand::Reload),
            ]
        );
    }

    #[test]
    fn resize_discards_a_throttled_frame_from_the_old_pixel_viewport() {
        let (provider, fake) = fake_provider();
        let mut state = BrowserState::new(Some(provider));
        let started = Instant::now();
        state.set_transport(FrameTransport::File, started);
        state.enable();
        state.reconcile_surfaces(vec![surface(1, "https://one", (1, 1), (1, 1))], started);
        fake.push_tick(ProviderTick {
            frames: vec![(PaneId(1), frame(1, 1, 1))],
            ..ProviderTick::default()
        });
        assert_eq!(state.pump(started).frames.len(), 1);

        fake.push_tick(ProviderTick {
            frames: vec![(PaneId(1), frame(1, 1, 2))],
            ..ProviderTick::default()
        });
        assert!(
            state
                .pump(deadline(started, Duration::from_millis(4)))
                .frames
                .is_empty()
        );
        state.reconcile_surfaces(
            vec![surface(1, "https://one", (2, 1), (2, 1))],
            deadline(started, Duration::from_millis(5)),
        );
        fake.push_tick(ProviderTick::default());
        assert!(
            state
                .pump(deadline(started, FILE_FRAME_INTERVAL))
                .frames
                .is_empty()
        );

        fake.push_tick(ProviderTick {
            frames: vec![(PaneId(1), frame(2, 1, 3))],
            ..ProviderTick::default()
        });
        let resized = state
            .pump(deadline(
                started,
                FILE_FRAME_INTERVAL + Duration::from_millis(1),
            ))
            .frames;
        assert_eq!(resized.len(), 1);
        assert_eq!((resized[0].image.width, resized[0].image.height), (2, 1));
    }

    #[test]
    fn receive_timeout_exists_only_while_a_provider_surface_is_active() {
        let (provider, fake) = fake_provider();
        let mut state = BrowserState::new(Some(provider));
        let now = Instant::now();
        assert_eq!(state.wait(now), BrowserWait::Blocking);
        state.enable();
        assert_eq!(state.wait(now), BrowserWait::Blocking);

        state.reconcile_surfaces(vec![surface(1, "https://one", (2, 2), (16, 32))], now);
        assert_eq!(state.wait(now), BrowserWait::Timeout(Duration::ZERO));
        fake.push_tick(ProviderTick {
            next_due: Some(Duration::from_secs(2)),
            ..ProviderTick::default()
        });
        state.pump(now);
        assert_eq!(state.wait(now), BrowserWait::Timeout(MAX_PUMP_WAIT));

        state.disable();
        assert_eq!(state.wait(now), BrowserWait::Blocking);
        assert!(fake.calls().contains(&FakeCall::CloseAll));
    }

    #[test]
    fn reconnect_teardown_closes_every_surface_and_clears_pending_work() {
        let (provider, fake) = fake_provider();
        let mut state = BrowserState::new(Some(provider));
        let now = Instant::now();
        state.enable();
        state.reconcile_surfaces(vec![surface(1, "https://one", (2, 2), (16, 32))], now);
        fake.take_calls();

        state.reset_connection();

        assert_eq!(fake.take_calls(), [FakeCall::CloseAll]);
        assert!(!state.has_surface(PaneId(1)));
        assert_eq!(state.wait(now), BrowserWait::Blocking);
    }

    #[test]
    fn absent_or_frameless_providers_leave_browser_cards_in_degraded_mode() {
        let now = Instant::now();
        let mut absent = BrowserState::new(None);
        absent.enable();
        absent.reconcile_surfaces(vec![surface(1, "https://one", (2, 2), (16, 32))], now);
        assert!(!absent.has_surface(PaneId(1)));
        assert_eq!(absent.wait(now), BrowserWait::Blocking);

        let (provider, fake) = fake_provider();
        let mut frameless = BrowserState::new(Some(provider));
        frameless.enable();
        frameless.reconcile_surfaces(vec![surface(1, "https://one", (2, 2), (16, 32))], now);
        fake.push_tick(ProviderTick::default());
        assert!(frameless.pump(now).frames.is_empty());
    }
}
