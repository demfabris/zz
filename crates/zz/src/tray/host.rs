use std::{
    io,
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, ExitCode, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

#[cfg(not(target_os = "macos"))]
use async_channel::Receiver;
use async_channel::Sender;
use parking_lot::Mutex;
use zz_daemon::CommandClient;
use zz_protocol::CommandInvocation;

use super::{
    Tray, TrayEvent,
    ipc::{self, DesktopEvent, DesktopPeer},
    settings::Settings,
};

const HELPER_ARGUMENT: &str = "--bootstrap-tray";
const OBSERVER_ARGUMENT: &str = "--bootstrap-tray-observer";

pub(super) enum HostEvent {
    Action(TrayEvent),
    ConfigChanged,
    Connected(u64, DesktopPeer),
    Disconnected(u64),
    Focused(u64),
    LaunchFinished,
    QuitFailed,
    #[cfg(target_os = "macos")]
    Reopen,
    Stop,
}

pub(crate) struct DaemonTray {
    stopping: Arc<AtomicBool>,
    lifetime: Arc<Mutex<Option<ChildStdin>>>,
}

impl Drop for DaemonTray {
    fn drop(&mut self) {
        self.stopping.store(true, Ordering::Release);
        self.lifetime.lock().take();
    }
}

pub(crate) fn start_daemon_helper(socket: &Path, server_id: u64) -> Option<DaemonTray> {
    if !has_desktop_session() {
        return None;
    }
    let executable = std::env::current_exe().ok()?;
    let socket = socket.to_path_buf();
    let (mut child, stdin) = match spawn_helper(&executable, &socket, server_id) {
        Ok(child) => child,
        Err(error) => {
            log::warn!(target: "zz::tray", "could not start tray helper: {error}");
            return None;
        }
    };
    let stopping = Arc::new(AtomicBool::new(false));
    let lifetime = Arc::new(Mutex::new(Some(stdin)));
    let guard = DaemonTray {
        stopping: stopping.clone(),
        lifetime: lifetime.clone(),
    };
    let result = thread::Builder::new()
        .name("zz-tray-supervisor".into())
        .spawn(move || {
            let mut delay = Duration::from_secs(1);
            loop {
                let status = child.wait();
                if stopping.load(Ordering::Acquire) || status.is_ok_and(|status| status.success()) {
                    break;
                }
                lifetime.lock().take();
                thread::sleep(delay);
                if stopping.load(Ordering::Acquire) {
                    break;
                }
                match spawn_helper(&executable, &socket, server_id) {
                    Ok((replacement, stdin)) => {
                        child = replacement;
                        let mut pipe = lifetime.lock();
                        if !stopping.load(Ordering::Acquire) {
                            *pipe = Some(stdin);
                        }
                    }
                    Err(error) => {
                        log::warn!(target: "zz::tray", "could not restart tray helper: {error}");
                        break;
                    }
                }
                delay = (delay * 2).min(Duration::from_secs(5));
            }
        });
    if let Err(error) = result {
        log::warn!(target: "zz::tray", "could not supervise tray helper: {error}");
        return None;
    }
    Some(guard)
}

fn spawn_helper(
    executable: &Path,
    socket: &Path,
    server_id: u64,
) -> io::Result<(Child, ChildStdin)> {
    let mut child = Command::new(executable)
        .arg(HELPER_ARGUMENT)
        .arg(socket)
        .arg(server_id.to_string())
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    let stdin = child.stdin.take().expect("piped helper stdin");
    Ok((child, stdin))
}

fn has_desktop_session() -> bool {
    #[cfg(target_os = "linux")]
    {
        std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_some()
            || (std::env::var_os("XDG_RUNTIME_DIR").is_some()
                && (std::env::var_os("WAYLAND_DISPLAY").is_some()
                    || std::env::var_os("DISPLAY").is_some()))
    }
    #[cfg(target_os = "macos")]
    {
        std::env::var_os("SSH_CONNECTION").is_none()
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        true
    }
}

pub(crate) fn run_if_requested() -> Option<ExitCode> {
    let mut args = std::env::args_os().skip(1);
    let argument = args.next();
    let observe = match argument.as_deref().and_then(|argument| argument.to_str()) {
        Some(HELPER_ARGUMENT) => false,
        Some(OBSERVER_ARGUMENT) => true,
        _ => return None,
    };
    let socket = args.next().map(PathBuf::from);
    let server_id = args.next().and_then(|value| value.to_str()?.parse().ok());
    let (Some(socket), Some(server_id), None) = (socket, server_id, args.next()) else {
        return Some(ExitCode::FAILURE);
    };
    let _ = env_logger::try_init();
    Some(match run(socket, server_id, observe) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) if error.kind() == io::ErrorKind::AddrInUse => ExitCode::SUCCESS,
        Err(error) => {
            log::warn!(target: "zz::tray", "tray helper stopped: {error}");
            ExitCode::FAILURE
        }
    })
}

pub(super) fn start_desktop_helper(socket: &Path, server_id: u64) {
    let result = Command::new(std::env::current_exe().unwrap_or_default())
        .arg(OBSERVER_ARGUMENT)
        .arg(socket)
        .arg(server_id.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    if let Ok(mut child) = result {
        let _ = thread::Builder::new()
            .name("zz-tray-reap".into())
            .spawn(move || {
                let _ = child.wait();
            });
    }
}

fn run(socket: PathBuf, server_id: u64, observe: bool) -> io::Result<()> {
    let (events, receiver) = async_channel::unbounded();
    let lifetime_events = events.clone();
    let observer = if observe {
        let client = CommandClient::connect(&socket).map_err(io::Error::other)?;
        if client.server_hello().server_id != server_id {
            return Ok(());
        }
        Some(client)
    } else {
        None
    };
    thread::Builder::new()
        .name("zz-tray-lifetime".into())
        .spawn(move || {
            if let Some(observer) = observer {
                observer.wait_for_disconnect();
            } else {
                wait_for_daemon(io::stdin().lock());
            }
            let _ = lifetime_events.try_send(HostEvent::Stop);
        })?;
    let listener = ipc::listen(ipc::endpoint(&socket, server_id), events.clone())?;
    let settings = Settings::new(events.clone()).map_err(io::Error::other)?;
    let (actions, action_receiver) = async_channel::unbounded();
    let action_events = events.clone();
    thread::Builder::new()
        .name("zz-tray-actions".into())
        .spawn(move || {
            while let Ok(action) = action_receiver.recv_blocking() {
                if action_events.try_send(HostEvent::Action(action)).is_err() {
                    break;
                }
            }
        })?;
    let host = Host {
        socket,
        server_id,
        events,
        actions,
        settings,
        tray: None,
        available: false,
        desktops: Vec::new(),
        opening: false,
        quitting: false,
    };
    #[cfg(target_os = "macos")]
    super::macos::run_helper(host, receiver)?;
    #[cfg(not(target_os = "macos"))]
    run_events(host, &receiver);
    drop(listener);
    Ok(())
}

fn wait_for_daemon(mut lifetime: impl io::Read) {
    let mut bytes = [0; 64];
    loop {
        match lifetime.read(&mut bytes) {
            Ok(0) => break,
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(_) => break,
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn run_events(mut host: Host, receiver: &Receiver<HostEvent>) {
    host.handle(HostEvent::ConfigChanged);
    while let Ok(event) = receiver.recv_blocking() {
        if !host.handle(event) {
            break;
        }
    }
}

pub(super) struct Host {
    socket: PathBuf,
    server_id: u64,
    events: Sender<HostEvent>,
    actions: Sender<TrayEvent>,
    settings: Settings,
    tray: Option<Tray>,
    available: bool,
    desktops: Vec<(u64, DesktopPeer)>,
    opening: bool,
    quitting: bool,
}

impl Host {
    pub(super) fn handle(&mut self, event: HostEvent) -> bool {
        match event {
            HostEvent::Stop => return false,
            HostEvent::ConfigChanged => {
                self.settings.refresh();
                if self.settings.enabled() {
                    if self.tray.is_none() {
                        self.tray = super::spawn(self.actions.clone());
                        self.set_available(self.tray.is_some());
                    }
                } else {
                    self.tray.take();
                    self.set_available(false);
                }
            }
            HostEvent::Connected(id, peer) => {
                if peer
                    .sender
                    .try_send(DesktopEvent::Available(self.available))
                    .is_ok()
                {
                    self.desktops.push((id, peer));
                    self.opening = false;
                }
            }
            HostEvent::Disconnected(id) => self.desktops.retain(|(other, _)| *other != id),
            HostEvent::Focused(id) => {
                if let Some(index) = self.desktops.iter().position(|(other, _)| *other == id) {
                    let desktop = self.desktops.remove(index);
                    self.desktops.push(desktop);
                }
            }
            HostEvent::LaunchFinished => self.opening = false,
            HostEvent::QuitFailed => self.quitting = false,
            HostEvent::Action(TrayEvent::Toggle) if !self.quitting => {
                self.open(DesktopEvent::Toggle);
            }
            #[cfg(target_os = "macos")]
            HostEvent::Reopen => {
                if !self.quitting {
                    self.open(DesktopEvent::Show);
                }
            }
            HostEvent::Action(TrayEvent::Quit) if !self.quitting => self.quit(),
            HostEvent::Action(TrayEvent::Available(available)) => {
                self.set_available(available && self.tray.is_some());
            }
            HostEvent::Action(_) => {}
        }
        true
    }

    fn set_available(&mut self, available: bool) {
        if self.available != available {
            self.available = available;
            self.broadcast(DesktopEvent::Available(available));
        }
    }

    fn broadcast(&mut self, event: DesktopEvent) {
        self.desktops
            .retain(|(_, peer)| peer.sender.try_send(event).is_ok());
    }

    fn open(&mut self, event: DesktopEvent) {
        while let Some((_, desktop)) = self.desktops.last() {
            if desktop.sender.try_send(event).is_ok() {
                return;
            }
            self.desktops.pop();
        }
        if self.opening {
            return;
        }
        match launch_app(&self.socket) {
            Ok(mut child) => {
                self.opening = true;
                let events = self.events.clone();
                let _ = thread::Builder::new()
                    .name("zz-tray-launch".into())
                    .spawn(move || {
                        let _ = child.wait();
                        let _ = events.try_send(HostEvent::LaunchFinished);
                    });
            }
            Err(error) => log::warn!(target: "zz::tray", "could not reopen zz: {error}"),
        }
    }

    fn quit(&mut self) {
        self.quitting = true;
        let socket = self.socket.clone();
        let server_id = self.server_id;
        let desktops = self.desktops.clone();
        let events = self.events.clone();
        let result = thread::Builder::new()
            .name("zz-tray-quit".into())
            .spawn(move || {
                let Ok(mut client) = CommandClient::connect(&socket) else {
                    let _ = events.try_send(HostEvent::QuitFailed);
                    return;
                };
                if client.server_hello().server_id != server_id {
                    let _ = events.try_send(HostEvent::QuitFailed);
                    return;
                }
                for (_, desktop) in &desktops {
                    let _ = desktop.sender.try_send(DesktopEvent::Quit);
                }
                let deadline = std::time::Instant::now() + Duration::from_secs(2);
                for (_, desktop) in desktops {
                    let timeout = deadline.saturating_duration_since(std::time::Instant::now());
                    let _ = desktop.acknowledged.recv_timeout(timeout);
                }
                if let Err(error) =
                    client.execute(CommandInvocation::new("kill-server", [] as [&str; 0]))
                {
                    log::warn!(target: "zz::tray", "daemon stop request: {error}");
                    let _ = events.try_send(HostEvent::QuitFailed);
                }
            });
        if result.is_err() {
            self.quitting = false;
        }
    }
}

fn launch_app(socket: &Path) -> io::Result<Child> {
    Command::new(std::env::current_exe()?)
        .arg("app")
        .env("ZZ_SOCKET", socket)
        .env_remove("TMUX")
        .env_remove("TMUX_PANE")
        .env_remove("ZZ_STARTUP_REENTRY")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn tray_quit_notifies_desktops_and_stops_all_sessions() {
        let directory = tempfile::tempdir_in("/tmp").expect("directory");
        let socket = directory.path().join("daemon.sock");
        let daemon = zz_daemon::Daemon::new(&socket).without_user_config();
        let (ready, started) = crossbeam_channel::bounded(1);
        let (done, finished) = crossbeam_channel::bounded(1);
        let worker = thread::spawn(move || {
            let result = daemon.run_foreground_with_ready(|server_id| {
                ready.send(server_id).expect("ready");
            });
            done.send(result).expect("done");
        });
        let server_id = started
            .recv_timeout(Duration::from_secs(10))
            .expect("daemon");
        let mut commands = CommandClient::connect(&socket).expect("commands");
        for name in ["first", "second"] {
            commands
                .execute(CommandInvocation::new(
                    "new-session",
                    ["-d", "-s", name, "sleep 3600"],
                ))
                .expect("session");
        }
        let (events, _receiver) = async_channel::unbounded();
        let (actions, _actions) = async_channel::unbounded();
        let (sender, desktop) = async_channel::bounded(1);
        let (acknowledge, acknowledged) = crossbeam_channel::bounded(1);
        let mut host = Host {
            socket,
            server_id,
            settings: Settings::new(events.clone()).expect("settings"),
            events,
            actions,
            tray: None,
            available: false,
            desktops: vec![(
                1,
                DesktopPeer {
                    sender,
                    acknowledged,
                },
            )],
            opening: false,
            quitting: false,
        };
        #[cfg(target_os = "macos")]
        {
            assert!(host.handle(HostEvent::Reopen));
            assert_eq!(
                desktop.recv_blocking().expect("reopen event"),
                DesktopEvent::Show
            );
        }
        assert!(host.handle(HostEvent::Action(TrayEvent::Quit)));
        assert_eq!(
            desktop.recv_blocking().expect("desktop event"),
            DesktopEvent::Quit
        );
        assert!(finished.try_recv().is_err());
        acknowledge.send(()).expect("desktop accepts quit");
        finished
            .recv_timeout(Duration::from_secs(10))
            .expect("daemon stops")
            .expect("shutdown");
        assert!(
            commands
                .execute(CommandInvocation::new("list-sessions", [] as [&str; 0]))
                .is_err()
        );
        worker.join().expect("daemon worker");
    }

    #[cfg(unix)]
    #[test]
    fn lifetime_waits_for_the_daemons_pipe_to_close() {
        use std::os::unix::net::UnixStream;
        let (reader, writer) = UnixStream::pair().expect("lifetime pipe");
        let (done, finished) = std::sync::mpsc::channel();
        let worker = thread::spawn(move || {
            wait_for_daemon(reader);
            done.send(()).expect("done");
        });
        assert!(finished.recv_timeout(Duration::from_millis(30)).is_err());
        drop(writer);
        finished
            .recv_timeout(Duration::from_secs(2))
            .expect("daemon exit wakes helper");
        worker.join().expect("worker");
    }
}
