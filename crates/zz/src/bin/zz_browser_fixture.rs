use std::{
    env,
    ffi::OsStr,
    io::{Read, Write},
    net::{Ipv4Addr, SocketAddrV4, TcpListener, TcpStream},
    process::{Command, ExitCode},
    sync::mpsc::{self, Sender},
    thread,
    time::{Duration, Instant},
};

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
use std::sync::Arc;

use env_logger::{Builder, Env, WriteStyle};
#[cfg(any(target_os = "linux", target_os = "freebsd"))]
use gpui::{
    App, Bounds, Context, ObjectFit, Render, RenderImage, Window, WindowBounds, WindowOptions, div,
    external_texture, img, prelude::*, px, size,
};
#[cfg(any(target_os = "linux", target_os = "freebsd"))]
use gpui_platform::application;
#[cfg(any(target_os = "linux", target_os = "freebsd"))]
use image::{Frame as ImageFrame, ImageBuffer, Rgba};
#[cfg(any(target_os = "linux", target_os = "freebsd"))]
use parking_lot::Mutex;
#[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
use zz_browser::FrameTier;
use zz_browser::{
    AcceleratedPaintDiagnostics, BrowserBootstrap, BrowserEvent, BrowserRuntime, BrowserSession,
    FrameMailboxDiagnostics, RuntimePhase, RuntimeSignal, SessionPhase, Viewport,
};
#[cfg(any(target_os = "linux", target_os = "freebsd"))]
use zz_browser::{BrowserGpuContext, OsrFrame, SessionId};

const DEFAULT_PORT: u16 = 9324;
const DEFAULT_SPIKE_SECONDS: u64 = 10;
const MAX_REQUEST_BYTES: usize = 16 * 1024;
const SHARED_TEXTURE_SPIKE_FLAG: &str = "--shared-texture-spike";
const INTERNAL_SHARED_TEXTURE_SPIKE_FLAG: &str = "--run-shared-texture-spike";
#[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
const PUMP_INTERVAL: Duration = Duration::from_millis(4);
const STARTUP_TIMEOUT: Duration = Duration::from_secs(15);
const CLOSE_TIMEOUT: Duration = Duration::from_secs(2);

const INDEX_HTML: &str = r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>zz browser fixture</title>
  <style>
    :root { color-scheme: light; font-family: system-ui, sans-serif; }
    * { box-sizing: border-box; }
    body { margin: 0; color: #172033; background: #f5f7fb; }
    header { padding: 24px 32px; color: white; background: #243b6b; }
    main { width: min(880px, calc(100% - 48px)); margin: 24px auto 80px; }
    .proof { display: grid; grid-template-columns: repeat(3, 1fr); height: 96px; overflow: hidden; border-radius: 12px; }
    .red { background: #e53935; }
    .green { background: #2eaf62; }
    .blue { background: #2878d0; }
    .card { margin-top: 20px; padding: 20px; border: 1px solid #d7deea; border-radius: 12px; background: white; box-shadow: 0 8px 24px #243b6b14; }
    label { display: block; margin-bottom: 8px; font-weight: 650; }
    input { width: 100%; padding: 11px 12px; border: 2px solid #8392ae; border-radius: 8px; font: inherit; }
    button, a { display: inline-block; margin-top: 14px; padding: 9px 13px; border: 0; border-radius: 8px; color: white; background: #5b4bc4; font: inherit; text-decoration: none; cursor: pointer; }
    output, code { color: #5b2387; font-weight: 650; }
    .spacer { height: 900px; margin-top: 20px; padding: 24px; border-radius: 12px; color: #49617e; background: linear-gradient(#eef3fa, #d7e3f3); }
    #scroll-proof { margin-top: 820px; padding: 18px; border-radius: 8px; color: white; background: #bf4b8a; }
  </style>
</head>
<body>
  <header><h1>zz Chromium + GPUI proof</h1><div id="launch">Loading persistent state…</div></header>
  <main>
    <section class="proof" aria-label="BGRA color proof"><div class="red"></div><div class="green"></div><div class="blue"></div></section>
    <section class="card">
      <label for="typing">Browser input</label>
      <input id="typing" autofocus placeholder="Type here">
      <p>Input received: <output id="typed">nothing yet</output></p>
      <button id="mutate" type="button">Mutate title</button>
      <a id="navigate" href="/next">Navigate to page two</a>
    </section>
    <section class="card">
      <strong>Persistent cookie:</strong> <code id="cookie"></code><br>
      <strong>Persistent launch count:</strong> <code id="count"></code>
    </section>
    <section class="spacer">Scroll inside Chromium.<div id="scroll-proof">Scroll input reached the page.</div></section>
  </main>
  <script>
    const previous = Number(localStorage.getItem('zzLaunchCount') || '0');
    const count = previous + 1;
    localStorage.setItem('zzLaunchCount', String(count));
    document.cookie = 'zz_poc=persisted; Max-Age=31536000; Path=/; SameSite=Lax';
    document.querySelector('#count').textContent = String(count);
    document.querySelector('#cookie').textContent = document.cookie || 'missing';
    document.querySelector('#launch').textContent = `Persistent fixture launch ${count}`;
    document.querySelector('#typing').addEventListener('input', event => {
      document.querySelector('#typed').textContent = event.target.value || 'nothing yet';
    });
    document.querySelector('#mutate').addEventListener('click', () => {
      document.title = 'zz title mutation passed';
      document.querySelector('#mutate').textContent = 'Title changed';
    });
  </script>
</body>
</html>"#;

const NEXT_HTML: &str = r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>zz fixture page two</title>
  <style>
    body { margin: 0; padding: 64px; color: white; background: #187c76; font: 18px system-ui, sans-serif; }
    a { color: #fff5ae; }
  </style>
</head>
<body>
  <h1>Navigation passed</h1>
  <p>This is the second same-session history entry.</p>
  <a href="/">Return to the fixture</a>
</body>
</html>"#;

const SHARED_TEXTURE_SPIKE_HTML: &str = r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>zz shared-texture spike</title>
  <style>
    html, body { width: 100%; height: 100%; margin: 0; overflow: hidden; background: #101318; }
    canvas { width: 100%; height: 100%; }
  </style>
</head>
<body>
  <canvas id="animation"></canvas>
  <script>
    const canvas = document.querySelector('#animation');
    const context = canvas.getContext('2d');
    function paint(time) {
      const scale = devicePixelRatio || 1;
      const width = Math.max(1, Math.floor(innerWidth * scale));
      const height = Math.max(1, Math.floor(innerHeight * scale));
      if (canvas.width !== width || canvas.height !== height) {
        canvas.width = width;
        canvas.height = height;
      }
      const phase = time / 1000;
      const gradient = context.createLinearGradient(0, 0, width, height);
      gradient.addColorStop(0, `hsl(${(phase * 73) % 360} 75% 48%)`);
      gradient.addColorStop(1, `hsl(${(phase * 73 + 140) % 360} 75% 32%)`);
      context.fillStyle = gradient;
      context.fillRect(0, 0, width, height);
      context.fillStyle = '#ffffff';
      context.font = `${Math.round(28 * scale)}px system-ui, sans-serif`;
      context.fillText(`accelerated OSR frame ${Math.floor(time)}`, 32 * scale, 56 * scale);
      requestAnimationFrame(paint);
    }
    requestAnimationFrame(paint);
  </script>
</body>
</html>"#;

fn main() -> ExitCode {
    if env::args_os().skip(1).any(|argument| {
        argument == OsStr::new("--type") || argument.to_string_lossy().starts_with("--type=")
    }) {
        let code = zz_browser::run_subprocess().clamp(0, 255);
        return ExitCode::from(u8::try_from(code).unwrap_or_default());
    }

    let mode = match parse_mode(env::args().skip(1)) {
        Ok(mode) => mode,
        Err(message) => {
            eprintln!("{message}");
            return ExitCode::from(2);
        }
    };
    match mode {
        FixtureMode::Serve { port } => run_server(port),
        FixtureMode::SharedTextureSpike { seconds, port } => {
            launch_shared_texture_spike(seconds, port)
        }
        FixtureMode::RunSharedTextureSpike { seconds, port } => {
            run_shared_texture_spike(seconds, port)
        }
        FixtureMode::ProbeEgressPref => probe_egress_pref(),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FixtureMode {
    Serve { port: u16 },
    SharedTextureSpike { seconds: u64, port: u16 },
    RunSharedTextureSpike { seconds: u64, port: u16 },
    ProbeEgressPref,
}

fn parse_mode(mut args: impl Iterator<Item = String>) -> Result<FixtureMode, String> {
    let Some(first) = args.next() else {
        return Ok(FixtureMode::Serve { port: DEFAULT_PORT });
    };
    if first == "--help" || first == "-h" {
        return Err(usage());
    }
    if first == "--probe-egress-pref" {
        if args.next().is_some() {
            return Err("unexpected argument after --probe-egress-pref".to_owned());
        }
        return Ok(FixtureMode::ProbeEgressPref);
    }
    if first == INTERNAL_SHARED_TEXTURE_SPIKE_FLAG {
        let seconds = parse_spike_seconds(
            &args
                .next()
                .ok_or_else(|| "missing internal spike duration".to_owned())?,
        )?;
        let port = parse_port_value(
            &args
                .next()
                .ok_or_else(|| "missing internal spike port".to_owned())?,
        )?;
        if args.next().is_some() {
            return Err("unexpected internal spike argument".to_owned());
        }
        return Ok(FixtureMode::RunSharedTextureSpike { seconds, port });
    }
    if first != SHARED_TEXTURE_SPIKE_FLAG {
        return parse_port(std::iter::once(first).chain(args))
            .map(|port| FixtureMode::Serve { port });
    }

    let mut seconds = DEFAULT_SPIKE_SECONDS;
    let mut port = DEFAULT_PORT;
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--seconds" => {
                let value = args
                    .next()
                    .ok_or_else(|| "--seconds requires a value".to_owned())?;
                seconds = parse_spike_seconds(&value)?;
            }
            "--port" => {
                let value = args
                    .next()
                    .ok_or_else(|| "--port requires a value".to_owned())?;
                port = parse_port_value(&value)?;
            }
            _ => return Err(format!("unknown shared-texture spike argument: {argument}")),
        }
    }
    Ok(FixtureMode::SharedTextureSpike { seconds, port })
}

fn usage() -> String {
    format!(
        "usage: zz_browser_fixture [PORT]\n       zz_browser_fixture {SHARED_TEXTURE_SPIKE_FLAG} [--seconds N] [--port PORT]\ndefault port: {DEFAULT_PORT}; default spike duration: {DEFAULT_SPIKE_SECONDS} seconds"
    )
}

fn parse_spike_seconds(value: &str) -> Result<u64, String> {
    value
        .parse::<u64>()
        .ok()
        .filter(|seconds| *seconds > 0)
        .ok_or_else(|| format!("invalid positive spike duration: {value}"))
}

fn parse_port_value(value: &str) -> Result<u16, String> {
    value
        .parse::<u16>()
        .map_err(|_| format!("invalid TCP port: {value}"))
}

fn run_server(port: u16) -> ExitCode {
    let address = SocketAddrV4::new(Ipv4Addr::LOCALHOST, port);
    let listener = match TcpListener::bind(address) {
        Ok(listener) => listener,
        Err(error) => {
            eprintln!("could not bind browser fixture at http://{address}: {error}");
            return ExitCode::FAILURE;
        }
    };
    println!("zz browser fixture: http://{address}");

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => spawn_request_worker(stream),
            Err(error) => eprintln!("fixture connection failed: {error}"),
        }
    }
    ExitCode::SUCCESS
}

fn parse_port(mut args: impl Iterator<Item = String>) -> Result<u16, String> {
    let Some(argument) = args.next() else {
        return Ok(DEFAULT_PORT);
    };
    if argument == "--help" || argument == "-h" {
        return Err(usage());
    }
    if args.next().is_some() {
        return Err("expected at most one port argument".to_owned());
    }
    parse_port_value(&argument)
}

fn launch_shared_texture_spike(seconds: u64, port: u16) -> ExitCode {
    let executable = match env::current_exe() {
        Ok(executable) => executable,
        Err(error) => {
            eprintln!("could not locate the browser fixture executable: {error}");
            return ExitCode::FAILURE;
        }
    };
    println!("zz shared-texture spike: setting ZZ_BROWSER_GPU=1 and ZZ_BROWSER_SHARED_TEXTURE=1");
    println!(
        "the fixture will paint GPU mailbox frames through GPUI and report readback fallback separately"
    );
    match Command::new(executable)
        .arg(INTERNAL_SHARED_TEXTURE_SPIKE_FLAG)
        .arg(seconds.to_string())
        .arg(port.to_string())
        .env("ZZ_BROWSER_GPU", "1")
        .env("ZZ_BROWSER_SHARED_TEXTURE", "1")
        .status()
    {
        Ok(status) if status.success() => ExitCode::SUCCESS,
        Ok(status) => {
            eprintln!("shared-texture spike process exited with {status}");
            ExitCode::FAILURE
        }
        Err(error) => {
            eprintln!("could not start the shared-texture spike process: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run_shared_texture_spike(seconds: u64, port: u16) -> ExitCode {
    if env::var_os("ZZ_BROWSER_GPU").is_none_or(|value| value != "1")
        || env::var_os("ZZ_BROWSER_SHARED_TEXTURE").is_none_or(|value| value != "1")
    {
        eprintln!(
            "internal shared-texture spike requires ZZ_BROWSER_GPU=1 and ZZ_BROWSER_SHARED_TEXTURE=1"
        );
        return ExitCode::FAILURE;
    }
    let _ =
        Builder::from_env(Env::default().default_filter_or("zz_browser=info,cef=warn,wgpu=warn"))
            .write_style(WriteStyle::Never)
            .try_init();

    match run_shared_texture_spike_inner(Duration::from_secs(seconds), port) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("shared-texture spike failed: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
fn run_shared_texture_spike_inner(duration: Duration, port: u16) -> Result<(), String> {
    let (address, _server) = start_background_server(port)
        .map_err(|error| format!("could not start the animated fixture: {error}"))?;
    let url = format!("http://{address}/shared-texture-spike");
    println!("zz shared-texture animated fixture: {url}");

    let mut runtime = match zz_browser::bootstrap().map_err(|error| error.to_string())? {
        BrowserBootstrap::SubprocessExit(code) => {
            return Err(format!("unexpected CEF subprocess exit {code}"));
        }
        BrowserBootstrap::Runtime(runtime) => runtime,
    };
    if !runtime.shared_texture_enabled() {
        return Err("CEF did not enable shared-texture OSR".to_owned());
    }
    runtime
        .start()
        .map_err(|error| format!("could not initialize CEF: {error}"))?;

    let outcome = Arc::new(Mutex::new(None));
    let outcome_for_app = Arc::clone(&outcome);
    application().run(move |cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(800.0), px(600.0)), cx);
        let outcome_for_view = Arc::clone(&outcome_for_app);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            move |window, cx| {
                let gpu = window
                    .wgpu_device_context()
                    .expect("the shared-texture fixture requires GPUI's wgpu renderer");
                let gpu = BrowserGpuContext::new(gpu.device, gpu.queue);
                cx.new(|_| {
                    SharedTextureSpikeView::new(runtime, gpu, url, duration, outcome_for_view)
                })
            },
        )
        .expect("could not open the shared-texture fixture window");
        cx.on_window_closed(|cx, _| cx.quit()).detach();
        cx.activate(true);
    });

    outcome
        .lock()
        .take()
        .unwrap_or_else(|| Err("the GPUI fixture exited before recording a result".to_owned()))
}

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
enum FixturePaintSurface {
    OwnedBgra {
        session: SessionId,
        generation: u64,
        logical_size: (u32, u32),
        device_size: (u32, u32),
        image: Arc<RenderImage>,
    },
    Gpu {
        session: SessionId,
        generation: u64,
        logical_size: (u32, u32),
        device_size: (u32, u32),
        pool_generation: u64,
        sequence: u64,
        texture: gpui::wgpu::Texture,
    },
}

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
impl FixturePaintSurface {
    fn identity(&self) -> (SessionId, u64) {
        match self {
            Self::OwnedBgra {
                session,
                generation,
                ..
            }
            | Self::Gpu {
                session,
                generation,
                ..
            } => (*session, *generation),
        }
    }
}

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
struct SharedTextureSpikeView {
    runtime: Option<BrowserRuntime>,
    signals: async_channel::Receiver<RuntimeSignal>,
    session: Option<BrowserSession>,
    events: Option<async_channel::Receiver<BrowserEvent>>,
    gpu: BrowserGpuContext,
    url: String,
    duration: Duration,
    startup_started: Instant,
    observation_started: Option<Instant>,
    closing_started: Option<Instant>,
    terminal_error: Option<String>,
    force_readback: bool,
    fallback_recreate: bool,
    surface: Option<FixturePaintSurface>,
    retired_images: Vec<Arc<RenderImage>>,
    last_paint_identity: Option<(SessionId, u64)>,
    observations: FixtureObservations,
    outcome: Arc<Mutex<Option<Result<(), String>>>>,
    finished: bool,
}

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
impl SharedTextureSpikeView {
    fn new(
        runtime: BrowserRuntime,
        gpu: BrowserGpuContext,
        url: String,
        duration: Duration,
        outcome: Arc<Mutex<Option<Result<(), String>>>>,
    ) -> Self {
        let signals = runtime.signals();
        Self {
            runtime: Some(runtime),
            signals,
            session: None,
            events: None,
            gpu,
            url,
            duration,
            startup_started: Instant::now(),
            observation_started: None,
            closing_started: None,
            terminal_error: None,
            force_readback: false,
            fallback_recreate: false,
            surface: None,
            retired_images: Vec::new(),
            last_paint_identity: None,
            observations: FixtureObservations::default(),
            outcome,
            finished: false,
        }
    }

    fn drive(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.finished {
            return;
        }
        self.reclaim_retired_images(window);
        if let Err(error) = self.pump_runtime() {
            self.terminal_error.get_or_insert(error);
        }
        self.create_session_if_ready(window);
        self.handle_events();

        if self.fallback_recreate
            && self
                .session
                .as_ref()
                .is_some_and(|session| session.phase() == SessionPhase::Closed)
        {
            self.session.take();
            self.events = None;
            self.fallback_recreate = false;
        }

        if self.session.is_none()
            && self.terminal_error.is_none()
            && self.startup_started.elapsed() >= STARTUP_TIMEOUT
        {
            self.terminal_error =
                Some("CEF did not initialize before the spike timeout".to_owned());
        }
        if self.terminal_error.is_some() && self.closing_started.is_none() {
            self.begin_close();
        }
        if self
            .observation_started
            .is_some_and(|started| started.elapsed() >= self.duration)
            && self.closing_started.is_none()
        {
            self.begin_close();
        }

        if self
            .session
            .as_ref()
            .is_some_and(|session| session.phase() == SessionPhase::Closed)
        {
            self.finish(cx);
            return;
        }
        if self.session.is_none() && self.terminal_error.is_some() {
            self.finish(cx);
            return;
        }
        if self.session.is_none() && self.closing_started.is_some() {
            self.finish(cx);
            return;
        }
        if self
            .closing_started
            .is_some_and(|started| started.elapsed() >= CLOSE_TIMEOUT)
        {
            self.terminal_error.get_or_insert_with(|| {
                "CEF browser did not close before the spike timeout".to_owned()
            });
            self.finish(cx);
        }
    }

    fn pump_runtime(&mut self) -> Result<(), String> {
        let Some(runtime) = self.runtime.as_mut() else {
            return Ok(());
        };
        runtime.do_message_loop_work();
        while let Ok(signal) = self.signals.try_recv() {
            match signal {
                RuntimeSignal::ContextInitialized => runtime
                    .handle_context_initialized()
                    .map_err(|error| error.to_string())?,
                RuntimeSignal::RequestContextInitialized { profile } => {
                    let _ = runtime
                        .handle_request_context_initialized(&profile)
                        .map_err(|error| error.to_string())?;
                }
                RuntimeSignal::ScheduleMessagePump(_) => {}
            }
        }
        Ok(())
    }

    fn create_session_if_ready(&mut self, window: &Window) {
        if self.session.is_some() || self.terminal_error.is_some() || self.closing_started.is_some()
        {
            return;
        }
        let Some(runtime) = self.runtime.as_mut() else {
            return;
        };
        if runtime.phase() != RuntimePhase::Running {
            return;
        }
        let viewport = Viewport {
            width: 800,
            height: 600,
            scale_factor: window.scale_factor(),
            screen_x: 0,
            screen_y: 0,
            visible: true,
        };
        match runtime.create_session(
            "default",
            &self.url,
            viewport,
            1.0,
            None,
            Some(self.gpu.clone()),
            !self.force_readback,
        ) {
            Ok(session) => {
                self.events = Some(session.events());
                self.session = Some(session);
                self.observation_started.get_or_insert_with(Instant::now);
                println!(
                    "observing end-to-end shared-texture delivery for {:.3} seconds at logical {}x{} scale {:.3}",
                    self.duration.as_secs_f64(),
                    viewport.width,
                    viewport.height,
                    viewport.scale_factor,
                );
            }
            Err(error) => {
                self.terminal_error = Some(format!("could not create the spike browser: {error}"));
            }
        }
    }

    fn handle_events(&mut self) {
        let Some(events) = self.events.clone() else {
            return;
        };
        while let Ok(event) = events.try_recv() {
            match event {
                BrowserEvent::Created { .. } => {
                    if let Some(session) = self.session.as_mut() {
                        session.mark_ready();
                    }
                }
                BrowserEvent::FrameReady { .. } => {
                    let frame = self.session.as_ref().and_then(BrowserSession::take_frame);
                    if let Some(frame) = frame
                        && let Err(error) = self.install_frame(frame)
                    {
                        self.terminal_error.get_or_insert(error);
                    }
                }
                BrowserEvent::SharedTextureFailed { reason, .. } => {
                    eprintln!(
                        "shared-texture fixture import failed; recreating atomically in readback mode: {reason}"
                    );
                    self.force_readback = true;
                    self.fallback_recreate = true;
                    self.observations.runtime_fallback_recreations += 1;
                    if let Some(session) = self.session.as_mut() {
                        self.observations.retired_accelerated =
                            Some(session.accelerated_paint_diagnostics());
                        self.observations.retired_mailbox =
                            Some(session.frame_mailbox_diagnostics());
                        session.close(true);
                    }
                }
                BrowserEvent::LoadFailed {
                    code, description, ..
                } => eprintln!("animated fixture load failed ({code}): {description}"),
                BrowserEvent::RenderProcessTerminated {
                    status, error_code, ..
                } => {
                    if let Some(session) = self.session.as_mut() {
                        session.mark_crashed();
                    }
                    self.terminal_error.get_or_insert_with(|| {
                        format!("CEF renderer terminated ({error_code}): {status}")
                    });
                }
                BrowserEvent::Closed { .. } => {
                    if let Some(session) = self.session.as_mut() {
                        session.mark_closed();
                    }
                }
                BrowserEvent::AddressChanged { .. }
                | BrowserEvent::TitleChanged { .. }
                | BrowserEvent::LoadingChanged { .. }
                | BrowserEvent::CursorChanged { .. }
                | BrowserEvent::ElementPicked { .. }
                | BrowserEvent::ElementPickCancelled { .. }
                | BrowserEvent::ElementPickFailed { .. }
                | BrowserEvent::ContextMenuRequested { .. }
                | BrowserEvent::PopupRequested { .. } => {}
            }
        }
    }

    fn install_frame(&mut self, frame: OsrFrame) -> Result<(), String> {
        let surface = match frame {
            OsrFrame::OwnedBgra(frame) => {
                let buffer = ImageBuffer::<Rgba<u8>, Vec<u8>>::from_raw(
                    frame.width,
                    frame.height,
                    frame.bgra,
                )
                .ok_or_else(|| "could not decode the fixture's owned BGRA frame".to_owned())?;
                self.observations.mailbox_owned_bgra_frames += 1;
                FixturePaintSurface::OwnedBgra {
                    session: frame.session,
                    generation: frame.generation,
                    logical_size: (800, 600),
                    device_size: (frame.width, frame.height),
                    image: Arc::new(RenderImage::new(vec![ImageFrame::new(buffer)])),
                }
            }
            OsrFrame::Gpu(frame) => {
                self.observations.mailbox_gpu_frames += 1;
                FixturePaintSurface::Gpu {
                    session: frame.session,
                    generation: frame.generation,
                    logical_size: (frame.logical_width, frame.logical_height),
                    device_size: (frame.device_width, frame.device_height),
                    pool_generation: frame.pool_generation,
                    sequence: frame.sequence,
                    texture: frame.texture,
                }
            }
        };
        if let Some(FixturePaintSurface::OwnedBgra { image, .. }) = self.surface.replace(surface) {
            self.retired_images.push(image);
        }
        Ok(())
    }

    fn reclaim_retired_images(&mut self, window: &mut Window) {
        let mut recycled = Vec::new();
        for image in self.retired_images.drain(..) {
            if let Err(error) = window.drop_image(image.clone()) {
                log::warn!("failed to release superseded fixture image: {error}");
            }
            if let Ok(image) = Arc::try_unwrap(image) {
                recycled.extend(
                    image
                        .into_frames()
                        .into_iter()
                        .map(|frame| frame.into_buffer().into_raw()),
                );
            }
        }
        if let Some(session) = self.session.as_ref() {
            for bgra in recycled {
                session.recycle_frame(bgra);
            }
        }
    }

    fn record_paint_submission(&mut self) {
        let Some(surface) = self.surface.as_ref() else {
            return;
        };
        let identity = surface.identity();
        if self.last_paint_identity == Some(identity) {
            return;
        }
        self.last_paint_identity = Some(identity);
        match surface {
            FixturePaintSurface::OwnedBgra {
                logical_size,
                device_size,
                ..
            } => {
                self.observations.painted_owned_bgra_frames += 1;
                self.observations.last_owned_bgra_painted_size =
                    Some((*logical_size, *device_size));
            }
            FixturePaintSurface::Gpu {
                logical_size,
                device_size,
                pool_generation,
                sequence,
                ..
            } => {
                self.observations.painted_gpu_frames += 1;
                self.observations.last_gpu_painted_size = Some(PaintedGpuFrame {
                    logical_size: *logical_size,
                    device_size: *device_size,
                    pool_generation: *pool_generation,
                    sequence: *sequence,
                });
            }
        }
    }

    fn begin_close(&mut self) {
        self.closing_started = Some(Instant::now());
        if let Some(session) = self.session.as_mut() {
            session.close(true);
        }
    }

    fn finish(&mut self, cx: &mut Context<Self>) {
        if self.finished {
            return;
        }
        let (accelerated, mailbox) = self.session.as_ref().map_or_else(
            || {
                (
                    AcceleratedPaintDiagnostics::default(),
                    FrameMailboxDiagnostics::default(),
                )
            },
            |session| {
                (
                    session.accelerated_paint_diagnostics(),
                    session.frame_mailbox_diagnostics(),
                )
            },
        );
        print_accelerated_paint_report(
            self.observation_started
                .map_or(Duration::ZERO, |started| started.elapsed()),
            &accelerated,
            &mailbox,
            &self.observations,
        );
        self.surface = None;
        self.session.take();
        let shutdown_error = self.runtime.as_mut().and_then(|runtime| {
            runtime
                .shutdown()
                .err()
                .map(|error| format!("could not shut down CEF: {error}"))
        });
        self.runtime.take();
        let result = self
            .terminal_error
            .take()
            .or(shutdown_error)
            .map_or(Ok(()), Err);
        self.outcome.lock().replace(result);
        self.finished = true;
        cx.quit();
    }
}

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
impl Render for SharedTextureSpikeView {
    #[allow(
        clippy::disallowed_methods,
        reason = "the fixture uses fixed proof colors to validate browser texture rendering"
    )]
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.drive(window, cx);
        self.record_paint_submission();
        window.request_animation_frame();

        let content = div().size_full().bg(gpui::rgb(0x10_13_18));
        match self.surface.as_ref() {
            Some(FixturePaintSurface::OwnedBgra { image, .. }) => {
                content.child(img(image.clone()).object_fit(ObjectFit::Fill).size_full())
            }
            Some(FixturePaintSurface::Gpu { texture, .. }) => content.child(
                external_texture(texture.clone())
                    .object_fit(ObjectFit::Fill)
                    .size_full(),
            ),
            None => content.child(
                div()
                    .size_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(gpui::rgb(0xe8_ee_f7))
                    .child("Waiting for the first CEF frame…"),
            ),
        }
    }
}

#[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
fn run_shared_texture_spike_inner(duration: Duration, port: u16) -> Result<(), String> {
    let (address, _server) = start_background_server(port)
        .map_err(|error| format!("could not start the animated fixture: {error}"))?;
    let url = format!("http://{address}/shared-texture-spike");
    println!("zz shared-texture animated fixture: {url}");

    let mut runtime = match zz_browser::bootstrap().map_err(|error| error.to_string())? {
        BrowserBootstrap::SubprocessExit(code) => {
            return Err(format!("unexpected CEF subprocess exit {code}"));
        }
        BrowserBootstrap::Runtime(runtime) => runtime,
    };
    if !runtime.shared_texture_enabled() {
        runtime
            .shutdown()
            .map_err(|error| format!("could not shut down disabled CEF runtime: {error}"))?;
        return Err("CEF did not enable shared-texture OSR".to_owned());
    }
    initialize_runtime(&mut runtime)?;

    let viewport = Viewport {
        width: 800,
        height: 600,
        scale_factor: 1.0,
        screen_x: 0,
        screen_y: 0,
        visible: true,
    };
    let mut session = match runtime.create_session("default", &url, viewport, 1.0, None, None, true)
    {
        Ok(session) => session,
        Err(error) => {
            let _ = runtime.shutdown();
            return Err(format!("could not create the spike browser: {error}"));
        }
    };
    let events = session.events();
    let signals = runtime.signals();
    let mut fixture_observations = FixtureObservations::default();
    println!(
        "observing accelerated-paint callbacks for {:.3} seconds (GPUI external textures are unavailable on this platform)",
        duration.as_secs_f64()
    );
    let deadline = Instant::now() + duration;
    while Instant::now() < deadline {
        pump_runtime(&mut runtime, &signals)?;
        handle_session_events(&mut session, &events, &mut fixture_observations);
        thread::sleep(PUMP_INTERVAL);
    }

    print_accelerated_paint_report(
        duration,
        &session.accelerated_paint_diagnostics(),
        &session.frame_mailbox_diagnostics(),
        &fixture_observations,
    );
    close_spike_session(&runtime, &mut session, &events, &mut fixture_observations)?;
    drop(session);
    runtime
        .shutdown()
        .map_err(|error| format!("could not shut down CEF: {error}"))?;
    Ok(())
}

#[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
fn probe_egress_pref() -> ExitCode {
    match probe_egress_pref_inner() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
fn probe_egress_pref() -> ExitCode {
    eprintln!("--probe-egress-pref is not supported on this platform");
    ExitCode::FAILURE
}

#[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
fn probe_egress_pref_inner() -> Result<(), String> {
    let mut runtime = match zz_browser::bootstrap().map_err(|error| error.to_string())? {
        BrowserBootstrap::SubprocessExit(code) => {
            return Err(format!("unexpected CEF subprocess exit {code}"));
        }
        BrowserBootstrap::Runtime(runtime) => runtime,
    };
    initialize_runtime(&mut runtime)?;
    let signals = runtime.signals();

    let composite = zz_browser::BrowserProfilePaths::egress_profile_name("default", "probe@remote")
        .map_err(|error| format!("could not derive the composite profile: {error}"))?;
    let mut failed = false;
    for (label, profile, port) in [
        ("composite", composite.as_str(), 12345_u16),
        ("plain", "default", 12346_u16),
    ] {
        let deadline = Instant::now() + STARTUP_TIMEOUT;
        loop {
            let ready = if label == "composite" {
                runtime.ensure_egress_profile_context(profile)
            } else {
                runtime.ensure_profile_context(profile)
            }
            .map_err(|error| format!("probe {label}: could not create the context: {error}"))?;
            if ready {
                break;
            }
            if Instant::now() >= deadline {
                return Err(format!("probe {label}: the context never became ready"));
            }
            pump_runtime(&mut runtime, &signals)?;
            thread::sleep(PUMP_INTERVAL);
        }
        match runtime.set_profile_proxy(profile, port) {
            Ok(()) => println!("probe {label} profile={profile}: proxy preference OK"),
            Err(error) => {
                failed = true;
                println!("probe {label} profile={profile}: FAILED: {error}");
                println!(
                    "probe {label} details: {}",
                    runtime.probe_preference_system(profile)
                );
            }
        }
    }
    runtime
        .shutdown()
        .map_err(|error| format!("could not shut down CEF: {error}"))?;
    if failed {
        Err("the proxy preference probe failed".to_owned())
    } else {
        Ok(())
    }
}

#[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
fn initialize_runtime(runtime: &mut BrowserRuntime) -> Result<(), String> {
    runtime
        .start()
        .map_err(|error| format!("could not initialize CEF: {error}"))?;
    let signals = runtime.signals();
    let deadline = Instant::now() + STARTUP_TIMEOUT;
    while runtime.phase() != RuntimePhase::Running {
        pump_runtime(runtime, &signals)?;
        if Instant::now() >= deadline {
            return Err("CEF did not initialize before the spike timeout".to_owned());
        }
        thread::sleep(PUMP_INTERVAL);
    }
    Ok(())
}

#[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
fn pump_runtime(
    runtime: &mut BrowserRuntime,
    signals: &async_channel::Receiver<RuntimeSignal>,
) -> Result<(), String> {
    runtime.do_message_loop_work();
    while let Ok(signal) = signals.try_recv() {
        match signal {
            RuntimeSignal::ContextInitialized => runtime
                .handle_context_initialized()
                .map_err(|error| error.to_string())?,
            RuntimeSignal::RequestContextInitialized { profile } => {
                let _ = runtime
                    .handle_request_context_initialized(&profile)
                    .map_err(|error| error.to_string())?;
            }
            RuntimeSignal::ScheduleMessagePump(_) => {}
        }
    }
    Ok(())
}

#[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
fn handle_session_events(
    session: &mut BrowserSession,
    events: &async_channel::Receiver<BrowserEvent>,
    observations: &mut FixtureObservations,
) {
    while let Ok(event) = events.try_recv() {
        match event {
            BrowserEvent::Created { .. } => session.mark_ready(),
            BrowserEvent::FrameReady { .. } => {
                if let Some(frame) = session.take_frame() {
                    match frame.tier() {
                        FrameTier::OwnedBgra => observations.mailbox_owned_bgra_frames += 1,
                        FrameTier::Gpu => observations.mailbox_gpu_frames += 1,
                        #[cfg(target_os = "macos")]
                        FrameTier::MacGpu => observations.mailbox_gpu_frames += 1,
                        #[cfg(target_os = "windows")]
                        FrameTier::WinGpu => observations.mailbox_gpu_frames += 1,
                    }
                }
                if observations.mailbox_owned_bgra_frames.is_power_of_two() {
                    eprintln!(
                        "shared-texture spike received readback fallback frame #{}; check CEF GPU/DRM availability",
                        observations.mailbox_owned_bgra_frames
                    );
                }
            }
            BrowserEvent::SharedTextureFailed { reason, .. } => {
                eprintln!("shared-texture import failed: {reason}");
            }
            BrowserEvent::LoadFailed {
                code, description, ..
            } => eprintln!("animated fixture load failed ({code}): {description}"),
            BrowserEvent::RenderProcessTerminated {
                status, error_code, ..
            } => {
                session.mark_crashed();
                eprintln!("CEF renderer terminated ({error_code}): {status}");
            }
            BrowserEvent::Closed { .. } => session.mark_closed(),
            BrowserEvent::AddressChanged { .. }
            | BrowserEvent::TitleChanged { .. }
            | BrowserEvent::LoadingChanged { .. }
            | BrowserEvent::CursorChanged { .. }
            | BrowserEvent::ElementPicked { .. }
            | BrowserEvent::ElementPickCancelled { .. }
            | BrowserEvent::ElementPickFailed { .. }
            | BrowserEvent::ContextMenuRequested { .. }
            | BrowserEvent::PopupRequested { .. } => {}
        }
    }
}

#[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
fn close_spike_session(
    runtime: &BrowserRuntime,
    session: &mut BrowserSession,
    events: &async_channel::Receiver<BrowserEvent>,
    observations: &mut FixtureObservations,
) -> Result<(), String> {
    session.close(true);
    let deadline = Instant::now() + CLOSE_TIMEOUT;
    while session.phase() != SessionPhase::Closed {
        runtime.do_message_loop_work();
        handle_session_events(session, events, observations);
        if Instant::now() >= deadline {
            return Err("CEF browser did not close before the spike timeout".to_owned());
        }
        thread::sleep(PUMP_INTERVAL);
    }
    Ok(())
}

#[derive(Default)]
struct FixtureObservations {
    mailbox_owned_bgra_frames: u64,
    mailbox_gpu_frames: u64,
    painted_owned_bgra_frames: u64,
    painted_gpu_frames: u64,
    runtime_fallback_recreations: u64,
    retired_accelerated: Option<AcceleratedPaintDiagnostics>,
    retired_mailbox: Option<FrameMailboxDiagnostics>,
    last_owned_bgra_painted_size: Option<((u32, u32), (u32, u32))>,
    last_gpu_painted_size: Option<PaintedGpuFrame>,
}

#[derive(Clone, Copy)]
struct PaintedGpuFrame {
    logical_size: (u32, u32),
    device_size: (u32, u32),
    pool_generation: u64,
    sequence: u64,
}

fn print_accelerated_paint_report(
    duration: Duration,
    diagnostics: &AcceleratedPaintDiagnostics,
    mailbox: &FrameMailboxDiagnostics,
    observations: &FixtureObservations,
) {
    println!(
        "shared-texture spike observations after {:.3} seconds:",
        duration.as_secs_f64()
    );
    println!(
        "  callbacks={} view={} popup={} missing_info={} unique_handles={} handle_transitions={} consecutive_handle_reuses={} readback_fallback_frames={}",
        diagnostics.callback_count,
        diagnostics.view_count,
        diagnostics.popup_count,
        diagnostics.missing_info_count,
        diagnostics.unique_handle_count,
        diagnostics.handle_transition_count,
        diagnostics.consecutive_handle_reuse_count,
        observations.mailbox_owned_bgra_frames,
    );
    println!(
        "  gpu_import_attempts={} gpu_frames_delivered={} gpu_import_failures={} helper_blank_fallbacks={} stale_pool_frames={} readback_frames_delivered={} latest_pool_generation={}",
        diagnostics.gpu_import_attempt_count,
        diagnostics.gpu_frame_delivered_count,
        diagnostics.gpu_import_failure_count,
        diagnostics.gpu_helper_fallback_count,
        diagnostics.stale_pool_frame_count,
        diagnostics.readback_frame_delivered_count,
        diagnostics.latest_pool_generation,
    );
    println!(
        "  mailbox owned_bgra_published={} gpu_published={} owned_bgra_taken={} gpu_taken={} active_tier={:?} delivery_generation={} tier_transitions={} fallback_pending={} gpu_import_failures={}",
        mailbox.owned_bgra_published,
        mailbox.gpu_published,
        mailbox.owned_bgra_taken,
        mailbox.gpu_taken,
        mailbox.active_tier,
        mailbox.delivery_generation,
        mailbox.tier_transition_count,
        mailbox.fallback_pending,
        mailbox.gpu_import_failure_count,
    );
    if let Some(retired) = &observations.retired_accelerated {
        println!(
            "  failed_shared_texture_session callbacks={} gpu_import_attempts={} gpu_frames_delivered={} gpu_import_failures={} helper_blank_fallbacks={} stale_pool_frames={} latest_pool_generation={}",
            retired.callback_count,
            retired.gpu_import_attempt_count,
            retired.gpu_frame_delivered_count,
            retired.gpu_import_failure_count,
            retired.gpu_helper_fallback_count,
            retired.stale_pool_frame_count,
            retired.latest_pool_generation,
        );
    }
    if let Some(retired) = observations.retired_mailbox {
        println!(
            "  failed_session_mailbox owned_bgra_published={} gpu_published={} owned_bgra_taken={} gpu_taken={} active_tier={:?} delivery_generation={} tier_transitions={} fallback_pending={} gpu_import_failures={}",
            retired.owned_bgra_published,
            retired.gpu_published,
            retired.owned_bgra_taken,
            retired.gpu_taken,
            retired.active_tier,
            retired.delivery_generation,
            retired.tier_transition_count,
            retired.fallback_pending,
            retired.gpu_import_failure_count,
        );
    }
    let detected_tier = if observations.runtime_fallback_recreations > 0 {
        "runtime-readback-fallback"
    } else if observations.painted_gpu_frames > 0 {
        "accelerated-gpu"
    } else if observations.painted_owned_bgra_frames > 0 {
        "readback-fallback"
    } else {
        "no-painted-frame"
    };
    println!(
        "  fixture tier={detected_tier} mailbox_gpu_frames={} mailbox_owned_bgra_frames={} painted_gpu_frames={} painted_owned_bgra_frames={} runtime_fallback_recreations={}",
        observations.mailbox_gpu_frames,
        observations.mailbox_owned_bgra_frames,
        observations.painted_gpu_frames,
        observations.painted_owned_bgra_frames,
        observations.runtime_fallback_recreations,
    );
    if let Some(frame) = observations.last_gpu_painted_size {
        println!(
            "  last_gpu_painted logical={}x{} device={}x{} pool_generation={} sequence={}",
            frame.logical_size.0,
            frame.logical_size.1,
            frame.device_size.0,
            frame.device_size.1,
            frame.pool_generation,
            frame.sequence,
        );
    }
    if let Some((logical, device)) = observations.last_owned_bgra_painted_size {
        println!(
            "  last_owned_bgra_painted logical={}x{} device={}x{}",
            logical.0, logical.1, device.0, device.1,
        );
    }
    let metadata_diagnostics = if diagnostics.last_observation.is_some() {
        diagnostics
    } else {
        observations
            .retired_accelerated
            .as_ref()
            .unwrap_or(diagnostics)
    };
    if let Some(observation) = &metadata_diagnostics.last_observation {
        let modifier = observation
            .drm_modifier
            .map_or_else(|| "n/a".to_owned(), |value| format!("{value:#018x}"));
        println!(
            "  last callback={} element={} dimensions={}x{} pixel_format={}({}) drm_modifier={} plane_count={} handle_identity={}",
            observation.callback,
            observation.paint_element,
            observation.width,
            observation.height,
            observation.pixel_format,
            observation.pixel_format_raw,
            modifier,
            observation.plane_count,
            observation.handle_identity,
        );
        for (index, plane) in observation.planes.iter().enumerate() {
            println!(
                "  plane[{index}] fd={} stride={} offset={} size={}",
                plane.fd, plane.stride, plane.offset, plane.size
            );
        }
    } else {
        println!("  no accelerated-paint metadata was observed");
    }
    for handle in &metadata_diagnostics.handles {
        println!(
            "  pool_handle identity={} uses={} first_callback={} last_callback={} reuse_gap_callbacks={:?}..{:?}",
            handle.identity,
            handle.use_count,
            handle.first_callback,
            handle.last_callback,
            handle.minimum_reuse_gap,
            handle.maximum_reuse_gap,
        );
    }
}

struct BackgroundServer {
    stop: Sender<()>,
    worker: Option<thread::JoinHandle<()>>,
}

impl Drop for BackgroundServer {
    fn drop(&mut self) {
        let _ = self.stop.send(());
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn start_background_server(port: u16) -> std::io::Result<(SocketAddrV4, BackgroundServer)> {
    let address = SocketAddrV4::new(Ipv4Addr::LOCALHOST, port);
    let listener = TcpListener::bind(address)?;
    listener.set_nonblocking(true)?;
    let (stop, stopped) = mpsc::channel();
    let worker = thread::Builder::new()
        .name("zz-fixture-server".to_owned())
        .spawn(move || {
            loop {
                if stopped.try_recv().is_ok() {
                    break;
                }
                match listener.accept() {
                    Ok((stream, _)) => spawn_request_worker(stream),
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(error) => {
                        eprintln!("fixture connection failed: {error}");
                        thread::sleep(Duration::from_millis(20));
                    }
                }
            }
        })?;
    Ok((
        address,
        BackgroundServer {
            stop,
            worker: Some(worker),
        },
    ))
}

fn spawn_request_worker(mut stream: TcpStream) {
    thread::Builder::new()
        .name("zz-fixture-request".to_owned())
        .spawn(move || {
            if let Err(error) = serve(&mut stream)
                && !matches!(
                    error.kind(),
                    std::io::ErrorKind::BrokenPipe
                        | std::io::ErrorKind::ConnectionReset
                        | std::io::ErrorKind::TimedOut
                        | std::io::ErrorKind::WouldBlock
                )
            {
                eprintln!("fixture request failed: {error}");
            }
        })
        .expect("could not spawn fixture request worker");
}

fn serve(stream: &mut TcpStream) -> std::io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    stream.set_write_timeout(Some(Duration::from_secs(2)))?;
    let mut request = Vec::with_capacity(1024);
    while request.len() < MAX_REQUEST_BYTES && !request.windows(4).any(|bytes| bytes == b"\r\n\r\n")
    {
        let remaining = MAX_REQUEST_BYTES - request.len();
        let mut chunk = [0_u8; 1024];
        let chunk_length = remaining.min(chunk.len());
        let length = stream.read(&mut chunk[..chunk_length])?;
        if length == 0 {
            break;
        }
        request.extend_from_slice(&chunk[..length]);
    }
    let request = String::from_utf8_lossy(&request);
    let path = request
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .unwrap_or("/");
    println!("fixture request: {path}");
    let response = response_for(path);
    write!(
        stream,
        "HTTP/1.1 {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        response.status,
        response.content_type,
        response.body.len()
    )?;
    stream.write_all(response.body.as_bytes())?;
    stream.flush()
}

struct Response {
    status: &'static str,
    content_type: &'static str,
    body: &'static str,
}

fn response_for(path: &str) -> Response {
    match path.split('?').next().unwrap_or(path) {
        "/" => Response {
            status: "200 OK",
            content_type: "text/html; charset=utf-8",
            body: INDEX_HTML,
        },
        "/next" => Response {
            status: "200 OK",
            content_type: "text/html; charset=utf-8",
            body: NEXT_HTML,
        },
        "/shared-texture-spike" => Response {
            status: "200 OK",
            content_type: "text/html; charset=utf-8",
            body: SHARED_TEXTURE_SPIKE_HTML,
        },
        "/favicon.ico" => Response {
            status: "204 No Content",
            content_type: "image/x-icon",
            body: "",
        },
        _ => Response {
            status: "404 Not Found",
            content_type: "text/plain; charset=utf-8",
            body: "not found\n",
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_default_and_explicit_ports() {
        assert_eq!(parse_port(std::iter::empty()), Ok(DEFAULT_PORT));
        assert_eq!(parse_port(["8080".to_owned()].into_iter()), Ok(8080));
        assert!(parse_port(["nope".to_owned()].into_iter()).is_err());
    }

    #[test]
    fn parses_shared_texture_spike_options() {
        assert_eq!(
            parse_mode([SHARED_TEXTURE_SPIKE_FLAG.to_owned()].into_iter()),
            Ok(FixtureMode::SharedTextureSpike {
                seconds: DEFAULT_SPIKE_SECONDS,
                port: DEFAULT_PORT,
            })
        );
        assert_eq!(
            parse_mode(
                [
                    SHARED_TEXTURE_SPIKE_FLAG,
                    "--seconds",
                    "3",
                    "--port",
                    "8123",
                ]
                .map(str::to_owned)
                .into_iter()
            ),
            Ok(FixtureMode::SharedTextureSpike {
                seconds: 3,
                port: 8123,
            })
        );
        assert!(
            parse_mode(
                [SHARED_TEXTURE_SPIKE_FLAG, "--seconds", "0"]
                    .map(str::to_owned)
                    .into_iter()
            )
            .is_err()
        );
    }

    #[test]
    fn routes_fixture_pages_without_query_strings() {
        assert_eq!(response_for("/?restart=2").status, "200 OK");
        assert!(response_for("/next").body.contains("Navigation passed"));
        assert!(
            response_for("/shared-texture-spike")
                .body
                .contains("requestAnimationFrame")
        );
        assert_eq!(response_for("/missing").status, "404 Not Found");
    }
}
