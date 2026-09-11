mod glib_pump;

use std::{
    cell::RefCell,
    rc::Weak,
    time::{Duration, Instant},
};

use gtk::glib;
use zz_browser::{
    BrowserBootstrap, BrowserRuntime, BrowserSession, RuntimePhase, RuntimeSignal, Viewport,
};

use super::BrowserPane;

thread_local! {
    static RUNTIME: RefCell<Option<Result<BrowserRuntime, String>>> = const { RefCell::new(None) };
    static PANES: RefCell<Vec<Weak<BrowserPane>>> = const { RefCell::new(Vec::new()) };
    static RETIRED: RefCell<Vec<BrowserSession>> = const { RefCell::new(Vec::new()) };
    static PUMP: RefCell<Option<glib::SourceId>> = const { RefCell::new(None) };
}

pub fn run_subprocess() -> Option<std::process::ExitCode> {
    std::env::args()
        .any(|arg| arg.starts_with("--type="))
        .then(|| {
            std::process::ExitCode::from(
                u8::try_from(zz_browser::run_subprocess().clamp(0, 255)).unwrap_or(1),
            )
        })
}

pub(super) fn register(pane: Weak<BrowserPane>) {
    PANES.with_borrow_mut(|panes| panes.push(pane));
    PUMP.with_borrow_mut(|pump| {
        if pump.is_none() {
            *pump = Some(glib::timeout_add_local_full(
                Duration::from_millis(8),
                glib::Priority::DEFAULT,
                || {
                    pump_runtime();
                    let panes = PANES.with_borrow_mut(|panes| {
                        panes.retain(|pane| pane.strong_count() > 0);
                        panes.iter().filter_map(Weak::upgrade).collect::<Vec<_>>()
                    });
                    for pane in panes {
                        pane.tick();
                    }
                    glib::ControlFlow::Continue
                },
            ));
        }
    });
}

fn initialize() -> Result<BrowserRuntime, String> {
    let paths = zz_browser::resolve_profile_paths().map_err(|error| error.to_string())?;
    let root = paths.root.with_file_name("browser-gtk");
    let paths = zz_browser::BrowserProfilePaths {
        profile: root.join("zz-default"),
        root,
    };
    let mut runtime =
        match zz_browser::bootstrap_with_profile_paths(paths).map_err(|error| error.to_string())? {
            BrowserBootstrap::Runtime(runtime) => runtime,
            BrowserBootstrap::SubprocessExit(code) => {
                return Err(format!("Unexpected browser subprocess exit: {code}"));
            }
        };
    runtime.set_background_color(0xffff_ffff);
    let sources = glib_pump::SourceBoundary::capture();
    runtime.start().map_err(|error| error.to_string())?;
    if let Err(error) = sources.detach_cef_work_source() {
        if let Err(shutdown) = runtime.shutdown() {
            return Err(format!("{error}; browser shutdown failed: {shutdown}"));
        }
        return Err(error);
    }
    Ok(runtime)
}

fn pump_runtime() {
    RUNTIME.with_borrow_mut(|slot| {
        if slot.is_none() {
            *slot = Some(initialize());
        }
        let Some(Ok(runtime)) = slot else {
            return;
        };
        runtime.do_message_loop_work();
        while let Ok(signal) = runtime.signals().try_recv() {
            let result = match signal {
                RuntimeSignal::ContextInitialized => runtime.handle_context_initialized(),
                RuntimeSignal::RequestContextInitialized { profile } => runtime
                    .handle_request_context_initialized(&profile)
                    .map(|_| ()),
                RuntimeSignal::ScheduleMessagePump(_) => Ok(()),
            };
            if let Err(error) = result {
                log::error!("Browser initialization: {error}");
            }
        }
    });
    RETIRED.with_borrow_mut(|sessions| {
        sessions.retain_mut(|session| {
            while let Ok(event) = session.events().try_recv() {
                if matches!(event, zz_browser::BrowserEvent::Closed { .. }) {
                    session.mark_closed();
                }
            }
            session.phase() != zz_browser::SessionPhase::Closed
        });
    });
}

pub(super) fn create(
    profile: &str,
    egress: Option<&(String, u16)>,
    url: &str,
    viewport: Viewport,
) -> Result<Option<BrowserSession>, String> {
    RUNTIME.with_borrow_mut(|slot| {
        let Some(result) = slot else {
            return Ok(None);
        };
        let runtime = result.as_mut().map_err(|error| error.clone())?;
        match runtime.phase() {
            RuntimePhase::Running => {}
            RuntimePhase::Uninitialized | RuntimePhase::Initializing => return Ok(None),
            phase => return Err(format!("The browser runtime is unavailable ({phase:?})")),
        }
        let profile = match egress {
            Some((host, _)) => zz_browser::BrowserProfilePaths::egress_profile_name(profile, host)
                .map_err(|error| error.to_string())?,
            None => profile.to_owned(),
        };
        let ready = if egress.is_some() {
            runtime.ensure_egress_profile_context(&profile)
        } else {
            runtime.ensure_profile_context(&profile)
        }
        .map_err(|error| error.to_string())?;
        if !ready {
            return Ok(None);
        }
        if let Some((_, port)) = egress {
            runtime
                .set_profile_proxy(&profile, *port)
                .map_err(|error| error.to_string())?;
        }
        runtime
            .create_session(&profile, url, viewport, 1.0, None, None, false)
            .map(Some)
            .map_err(|error| error.to_string())
    })
}

pub(super) fn refresh_route(profile: &str, port: u16) -> Result<(), String> {
    RUNTIME.with_borrow_mut(|slot| {
        let Some(Ok(runtime)) = slot else {
            return Err("Browser runtime is unavailable".to_owned());
        };
        runtime
            .set_profile_proxy(profile, port)
            .map_err(|error| error.to_string())
    })
}

pub(super) fn retire(mut session: BrowserSession) {
    session.close(true);
    RETIRED.with_borrow_mut(|sessions| sessions.push(session));
}

pub fn shutdown() {
    PUMP.with_borrow_mut(|pump| {
        if let Some(pump) = pump.take() {
            pump.remove();
        }
    });
    if RUNTIME.with_borrow(Option::is_none) {
        return;
    }
    super::TRANSFERRED.with_borrow_mut(std::collections::HashMap::clear);
    let panes =
        PANES.with_borrow(|panes| panes.iter().filter_map(Weak::upgrade).collect::<Vec<_>>());
    for pane in panes {
        pane.close_sessions();
    }
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        pump_runtime();
        let done = RUNTIME.with_borrow_mut(|slot| {
            let Some(Ok(runtime)) = slot else {
                return true;
            };
            if runtime.active_session_count() != 0 || runtime.active_data_operation_count() != 0 {
                return false;
            }
            if let Err(error) = runtime.shutdown() {
                log::error!("Browser shutdown: {error}");
            }
            true
        });
        if done {
            RUNTIME.with_borrow_mut(|slot| {
                slot.take();
            });
            break;
        }
        if Instant::now() >= deadline {
            log::error!("Browser shutdown timed out waiting for Chromium");
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}
