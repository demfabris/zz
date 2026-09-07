use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

use gpui::{AnyWindowHandle, App, Entity, Global, Task};
use interprocess::TryClone as _;
use interprocess::local_socket::Stream;
use parking_lot::Mutex;

use crate::mux::client::MuxClient;

use super::ipc::{self, DesktopEvent};

#[derive(Default)]
pub(crate) struct DesktopTray {
    server_id: Option<u64>,
    available: bool,
    pub(crate) stopping_daemon: bool,
    cancel: Option<Arc<AtomicBool>>,
    writer: Option<Arc<Mutex<Stream>>>,
    _task: Option<Task<()>>,
}

enum ConnectionEvent {
    Writer(Stream),
    Event(DesktopEvent),
}

impl Global for DesktopTray {}

impl Drop for DesktopTray {
    fn drop(&mut self) {
        if let Some(cancel) = &self.cancel {
            cancel.store(true, Ordering::Release);
        }
    }
}

impl DesktopTray {
    pub(crate) fn available(&self) -> bool {
        self.available
    }
}

pub(crate) fn init_desktop(
    mux: &Entity<MuxClient>,
    socket: PathBuf,
    window: AnyWindowHandle,
    cx: &mut App,
) {
    reconnect(mux.read(cx).local_server_id(), socket.clone(), window, cx);
    cx.observe(mux, move |mux, cx| {
        reconnect(mux.read(cx).local_server_id(), socket.clone(), window, cx);
    })
    .detach();
}

fn reconnect(server_id: Option<u64>, socket: PathBuf, window: AnyWindowHandle, cx: &mut App) {
    if cx.global::<DesktopTray>().server_id == server_id {
        return;
    }
    let was_available = cx.global::<DesktopTray>().available;
    let tray = cx.global_mut::<DesktopTray>();
    if let Some(cancel) = tray.cancel.take() {
        cancel.store(true, Ordering::Release);
    }
    tray._task.take();
    let retired_writer = tray.writer.take();
    tray.server_id = server_id;
    tray.available = false;
    if let Some(writer) = retired_writer {
        cx.background_executor()
            .spawn(async move {
                let _ = ipc::disconnect(&mut writer.lock());
            })
            .detach();
    }
    if was_available {
        show_if_hidden(window, cx);
    }
    let Some(server_id) = server_id else { return };
    let path = ipc::endpoint(&socket, server_id);
    let cancel = Arc::new(AtomicBool::new(false));
    let worker_cancel = cancel.clone();
    let (events, receiver) = async_channel::unbounded();
    if thread::Builder::new()
        .name("zz-tray-desktop".into())
        .spawn(move || {
            loop {
                let mut connected = None;
                for attempt in 0..24 {
                    if worker_cancel.load(Ordering::Acquire) || events.is_closed() {
                        return;
                    }
                    if let Ok(mut stream) = ipc::connect(&path)
                        && ipc::register(&mut stream).is_ok()
                    {
                        connected = Some(stream);
                        break;
                    }
                    if attempt == 4 {
                        super::host::start_desktop_helper(&socket, server_id);
                    }
                    thread::sleep(Duration::from_millis((50u64 << attempt.min(4)).min(500)));
                }
                let Some(mut stream) = connected else { return };
                let Ok(writer) = stream.try_clone() else {
                    return;
                };
                if events.try_send(ConnectionEvent::Writer(writer)).is_err() {
                    return;
                }
                while let Ok(event) = ipc::receive(&mut stream) {
                    if worker_cancel.load(Ordering::Acquire)
                        || events.try_send(ConnectionEvent::Event(event)).is_err()
                    {
                        return;
                    }
                }
                if events
                    .try_send(ConnectionEvent::Event(DesktopEvent::Available(false)))
                    .is_err()
                {
                    return;
                }
            }
        })
        .is_err()
    {
        return;
    }
    let task = cx.spawn(async move |cx| {
        while let Ok(event) = receiver.recv().await {
            cx.update(|cx| match event {
                ConnectionEvent::Writer(writer) => {
                    cx.global_mut::<DesktopTray>().writer = Some(Arc::new(Mutex::new(writer)));
                    focused(cx);
                }
                ConnectionEvent::Event(DesktopEvent::Available(available)) => {
                    let was_available = cx.global::<DesktopTray>().available;
                    cx.global_mut::<DesktopTray>().available = available;
                    if was_available && !available {
                        show_if_hidden(window, cx);
                    }
                }
                ConnectionEvent::Event(DesktopEvent::Toggle) => crate::toggle_from_tray(window, cx),
                ConnectionEvent::Event(DesktopEvent::Show) => {
                    let _ = window.update(cx, |_, window, cx| {
                        window.set_window_visible(true);
                        window.activate_window();
                        cx.activate(true);
                    });
                }
                ConnectionEvent::Event(DesktopEvent::Quit) => {
                    cx.global_mut::<DesktopTray>().stopping_daemon = true;
                    if let Some(writer) = cx.global::<DesktopTray>().writer.clone() {
                        cx.background_executor()
                            .spawn(async move {
                                let _ = ipc::acknowledge_quit(&mut writer.lock());
                            })
                            .detach();
                    }
                    cx.quit();
                }
            });
        }
    });
    let tray = cx.global_mut::<DesktopTray>();
    tray.cancel = Some(cancel);
    tray._task = Some(task);
}

pub(crate) fn focused(cx: &App) {
    if let Some(writer) = cx.global::<DesktopTray>().writer.clone() {
        cx.background_executor()
            .spawn(async move {
                let _ = ipc::focus(&mut writer.lock());
            })
            .detach();
    }
}

fn show_if_hidden(_window: AnyWindowHandle, cx: &mut App) {
    #[cfg(target_os = "macos")]
    if let Some(mtm) = objc2::MainThreadMarker::new()
        && objc2_app_kit::NSApplication::sharedApplication(mtm).isHidden()
    {
        cx.activate(true);
    }
    #[cfg(not(target_os = "macos"))]
    let _ = _window.update(cx, |_, window, _| {
        if !window.is_window_visible() {
            window.set_window_visible(true);
            window.activate_window();
        }
    });
}
