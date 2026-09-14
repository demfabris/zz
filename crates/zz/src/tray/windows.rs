//! `Shell_NotifyIcon` on a real window: `TaskbarCreated` skips message-only windows.

use std::sync::{
    atomic::{AtomicUsize, Ordering},
    mpsc,
};
use std::thread::{self, JoinHandle};

use async_channel::Sender;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::CreateBitmap;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Shell::{
    NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY, NOTIFYICONDATAW,
    Shell_NotifyIconW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CW_USEDEFAULT, CreateIconIndirect, CreatePopupMenu, CreateWindowExW,
    DefWindowProcW, DestroyIcon, DestroyMenu, DestroyWindow, DispatchMessageW, GetCursorPos,
    GetMessageW, GetWindowLongPtrW, HICON, ICONINFO, MF_GRAYED, MF_SEPARATOR, MF_STRING, MSG,
    PostMessageW, PostQuitMessage, RegisterClassW, RegisterWindowMessageW, SetForegroundWindow,
    SetWindowLongPtrW, TPM_NONOTIFY, TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenu,
    TranslateMessage, WINDOW_EX_STYLE, WM_APP, WM_CLOSE, WM_DESTROY, WM_LBUTTONUP, WM_NCCREATE,
    WM_NCDESTROY, WM_RBUTTONUP, WNDCLASSW, WS_POPUP,
};
use windows::core::{PCWSTR, w};

use super::{
    TrayEvent,
    facts::{MenuEntry, Source},
};

const TRAY_ICON_ICO: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/windows/zz.ico"
));

// `WM_APP`, not `WM_USER`: that range is private to controls.
const WM_TRAY: u32 = WM_APP + 1;
const WM_ATTENTION: u32 = WM_APP + 2;
const MENU_ACTION_BASE: usize = 1;
const TRAY_ICON_ID: u32 = 1;

/// A live notification icon. Dropping it removes the icon and ends the tray
/// thread.
pub(super) struct NotifyIcon {
    hwnd: isize,
    thread: Option<JoinHandle<()>>,
}

// SAFETY: the handle is only ever turned back into an `HWND` for
// `PostMessageW`, which is documented cross-thread.
#[allow(
    unsafe_code,
    reason = "a Send assertion for a raw handle has no safe form"
)]
unsafe impl Send for NotifyIcon {}

impl NotifyIcon {
    #[allow(unsafe_code, reason = "posting an icon update to the owning thread")]
    pub(super) fn set_attention(&self, count: usize) {
        // SAFETY: the window lives until Drop and PostMessageW supports other threads.
        unsafe {
            let _ = PostMessageW(
                Some(HWND(self.hwnd as _)),
                WM_ATTENTION,
                WPARAM(count),
                LPARAM(0),
            );
        }
    }
}

impl Drop for NotifyIcon {
    fn drop(&mut self) {
        #[allow(
            unsafe_code,
            reason = "no safe binding posts to a foreign thread's window"
        )]
        // SAFETY: the window outlives every drop path: it is destroyed only by
        // this message, and the join below outwaits the teardown.
        unsafe {
            let _ = PostMessageW(Some(HWND(self.hwnd as _)), WM_CLOSE, WPARAM(0), LPARAM(0));
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

struct TrayState {
    sender: Sender<TrayEvent>,
    taskbar_created: u32,
    icon: HICON,
    attention_icon: HICON,
    attention: AtomicUsize,
    source: Source,
}

impl TrayState {
    fn send(&self, event: TrayEvent) {
        if let Err(error) = self.sender.try_send(event) {
            log::warn!(target: "zz::tray", "dropped a tray event: {error}");
        }
    }

    fn notify_icon_data(&self, hwnd: HWND) -> NOTIFYICONDATAW {
        let mut data = NOTIFYICONDATAW {
            cbSize: size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: hwnd,
            uID: TRAY_ICON_ID,
            uFlags: NIF_MESSAGE | NIF_ICON | NIF_TIP,
            uCallbackMessage: WM_TRAY,
            hIcon: if self.attention.load(Ordering::Relaxed) > 0 {
                self.attention_icon
            } else {
                self.icon
            },
            ..Default::default()
        };
        let title = if self.attention.load(Ordering::Relaxed) == 0 {
            "zz".to_owned()
        } else {
            format!("zz · {} waiting", self.attention.load(Ordering::Relaxed))
        };
        for (slot, value) in data.szTip.iter_mut().take(127).zip(title.encode_utf16()) {
            *slot = value;
        }
        data
    }
}

pub(super) fn spawn(sender: Sender<TrayEvent>, source: Source) -> Option<NotifyIcon> {
    let (ready_sender, ready) = mpsc::channel();
    let thread = thread::Builder::new()
        .name("zz-tray".into())
        .spawn(move || pump(sender, source, ready_sender))
        .ok()?;
    match ready.recv() {
        Ok(Some(hwnd)) => Some(NotifyIcon {
            hwnd,
            thread: Some(thread),
        }),
        Ok(None) | Err(_) => {
            let _ = thread.join();
            None
        }
    }
}

#[allow(
    unsafe_code,
    reason = "win32 window and shell-icon calls have no safe binding"
)]
fn pump(sender: Sender<TrayEvent>, source: Source, ready: mpsc::Sender<Option<isize>>) {
    // SAFETY: plain win32 setup on this thread's own windows; the state Box
    // handed to `WM_NCCREATE` is reclaimed in `WM_NCDESTROY` below.
    let created = unsafe {
        let instance = match GetModuleHandleW(None) {
            Ok(instance) => instance,
            Err(error) => {
                log::warn!(target: "zz::tray", "no module handle for the tray window: {error}");
                let _ = ready.send(None);
                return;
            }
        };
        let class = WNDCLASSW {
            lpfnWndProc: Some(window_procedure),
            hInstance: instance.into(),
            lpszClassName: w!("zz-tray"),
            ..Default::default()
        };
        // A zero atom means the class already exists, which is not fatal.
        RegisterClassW(&class);

        let state = Box::new(TrayState {
            sender,
            taskbar_created: RegisterWindowMessageW(w!("TaskbarCreated")),
            icon: tray_icon(false),
            attention_icon: tray_icon(true),
            attention: AtomicUsize::new(0),
            source,
        });
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("zz-tray"),
            PCWSTR::null(),
            WS_POPUP,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            0,
            0,
            None,
            None,
            Some(instance.into()),
            Some(Box::into_raw(state) as *const _),
        )
    };
    let hwnd = match created {
        Ok(hwnd) => hwnd,
        Err(error) => {
            log::warn!(target: "zz::tray", "could not create the tray window: {error}");
            let _ = ready.send(None);
            return;
        }
    };

    // SAFETY: `hwnd` was created just above on this thread and `state` was
    // stashed by `WM_NCCREATE`.
    let added = unsafe {
        let state =
            &*(GetWindowLongPtrW(hwnd, windows::Win32::UI::WindowsAndMessaging::GWLP_USERDATA)
                as *const TrayState);
        Shell_NotifyIconW(NIM_ADD, &state.notify_icon_data(hwnd)).as_bool()
    };
    if !added {
        log::warn!(target: "zz::tray", "the shell rejected the notification icon");
        // SAFETY: same-thread window teardown; `WM_NCDESTROY` frees the state.
        unsafe {
            let _ = DestroyWindow(hwnd);
        }
        let _ = ready.send(None);
        return;
    }
    let _ = ready.send(Some(hwnd.0 as isize));

    // SAFETY: the canonical pump, on the thread that owns the window.
    unsafe {
        let mut message = MSG::default();
        while GetMessageW(&mut message, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
}

#[allow(unsafe_code, reason = "a window procedure is unsafe by construction")]
unsafe extern "system" fn window_procedure(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    use windows::Win32::UI::WindowsAndMessaging::{CREATESTRUCTW, GWLP_USERDATA};

    // SAFETY: `GWLP_USERDATA` only ever holds the `TrayState` box installed at
    // `WM_NCCREATE` and reclaimed at `WM_NCDESTROY`; between the two it is
    // this thread's alone.
    unsafe {
        if message == WM_NCCREATE {
            let create = &*(lparam.0 as *const CREATESTRUCTW);
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, create.lpCreateParams as isize);
            return DefWindowProcW(hwnd, message, wparam, lparam);
        }
        let state = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut TrayState;
        if state.is_null() {
            return DefWindowProcW(hwnd, message, wparam, lparam);
        }
        if message == WM_NCDESTROY {
            let state = Box::from_raw(state);
            if !state.icon.is_invalid() {
                let _ = DestroyIcon(state.icon);
            }
            if !state.attention_icon.is_invalid() {
                let _ = DestroyIcon(state.attention_icon);
            }
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            return DefWindowProcW(hwnd, message, wparam, lparam);
        }
        if message == WM_ATTENTION {
            (*state).attention.store(wparam.0, Ordering::Relaxed);
            let _ = Shell_NotifyIconW(NIM_MODIFY, &(*state).notify_icon_data(hwnd));
            return LRESULT(0);
        }
        let state = &*state;

        match message {
            WM_TRAY => match lparam.0 as u32 {
                WM_LBUTTONUP => state.send(TrayEvent::Toggle),
                WM_RBUTTONUP => show_menu(hwnd, state),
                _ => {}
            },
            WM_CLOSE => {
                let _ = Shell_NotifyIconW(NIM_DELETE, &state.notify_icon_data(hwnd));
                let _ = DestroyWindow(hwnd);
                return LRESULT(0);
            }
            WM_DESTROY => {
                PostQuitMessage(0);
                return LRESULT(0);
            }
            message if message == state.taskbar_created => {
                let available = Shell_NotifyIconW(NIM_ADD, &state.notify_icon_data(hwnd)).as_bool();
                state.send(TrayEvent::Available(available));
                if !available {
                    log::warn!(target: "zz::tray", "could not restore the icon after a shell restart");
                }
            }
            _ => {}
        }
        DefWindowProcW(hwnd, message, wparam, lparam)
    }
}

#[allow(unsafe_code, reason = "menus have no safe binding")]
fn show_menu(hwnd: HWND, state: &TrayState) {
    // SAFETY: menu creation and tracking on the window's own thread.
    unsafe {
        let Ok(menu) = CreatePopupMenu() else {
            return;
        };
        let entries = state.source.menu();
        let mut actions = Vec::new();
        for entry in entries {
            let label: Vec<u16> = entry
                .label()
                .replace('&', "&&")
                .encode_utf16()
                .chain(Some(0))
                .collect();
            match entry {
                MenuEntry::Separator => {
                    let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
                }
                MenuEntry::Item { action, .. } => {
                    let flags = if action.is_some() {
                        MF_STRING
                    } else {
                        MF_STRING | MF_GRAYED
                    };
                    let _ = AppendMenuW(
                        menu,
                        flags,
                        MENU_ACTION_BASE + actions.len(),
                        PCWSTR(label.as_ptr()),
                    );
                    actions.push(action);
                }
            }
        }

        let mut cursor = Default::default();
        let _ = GetCursorPos(&mut cursor);
        // Without this the notification-icon menu cannot be dismissed by
        // clicking away.
        let _ = SetForegroundWindow(hwnd);
        let picked = TrackPopupMenu(
            menu,
            TPM_RIGHTBUTTON | TPM_RETURNCMD | TPM_NONOTIFY,
            cursor.x,
            cursor.y,
            None,
            hwnd,
            None,
        );
        let _ = DestroyMenu(menu);
        if let Some(index) = (picked.0 as usize).checked_sub(MENU_ACTION_BASE)
            && let Some(Some(action)) = actions.get(index)
        {
            state.send(action.clone());
        }
    }
}

// `CreateIconIndirect` requires a mask bitmap but ignores it at 32bpp.
#[allow(unsafe_code, reason = "GDI bitmaps have no safe binding")]
fn tray_icon(attention: bool) -> HICON {
    let Ok(decoded) = image::load_from_memory_with_format(TRAY_ICON_ICO, image::ImageFormat::Ico)
    else {
        log::warn!(target: "zz::tray", "could not decode the tray icon");
        return HICON::default();
    };
    let mut rgba = decoded.into_rgba8();
    let (width, height) = rgba.dimensions();
    if attention {
        let radius = (width.min(height) / 6).max(1) as i64;
        let center_x = i64::from(width) - radius - 1;
        let center_y = radius;
        for (x, y, pixel) in rgba.enumerate_pixels_mut() {
            if (i64::from(x) - center_x).pow(2) + (i64::from(y) - center_y).pow(2) <= radius.pow(2)
            {
                *pixel = image::Rgba([255, 90, 60, 255]);
            }
        }
    }
    let bgra: Vec<u8> = rgba
        .pixels()
        .flat_map(|pixel| {
            let [r, g, b, a] = pixel.0;
            [b, g, r, a]
        })
        .collect();
    let mask = vec![0xffu8; (width as usize).div_ceil(8) * height as usize];

    // SAFETY: both bitmaps are built from exactly-sized local buffers and
    // released after `CreateIconIndirect` copies them.
    unsafe {
        let color = CreateBitmap(width as i32, height as i32, 1, 32, Some(bgra.as_ptr() as _));
        let monochrome = CreateBitmap(width as i32, height as i32, 1, 1, Some(mask.as_ptr() as _));
        let info = ICONINFO {
            fIcon: true.into(),
            hbmMask: monochrome,
            hbmColor: color,
            ..Default::default()
        };
        let icon = CreateIconIndirect(&info).unwrap_or_default();
        let _ = windows::Win32::Graphics::Gdi::DeleteObject(color.into());
        let _ = windows::Win32::Graphics::Gdi::DeleteObject(monochrome.into());
        icon
    }
}
