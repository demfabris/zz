//! A `StatusNotifierItem` for the GNOME shell.
//!
//! GNOME has no built-in SNI host: without the `AppIndicator` extension the item
//! is published on the bus and simply never drawn. Nothing here fails in that
//! case — the icon is invisible, the window keeps closing the way it always
//! did — so the degradation is silent by design.

use std::{cell::RefCell, rc::Rc, sync::Arc};

use adw::prelude::*;
use async_channel::{Receiver, Sender};
use gtk::{gdk_pixbuf, gio, glib};
use ksni::blocking::TrayMethods as _;

use crate::engine::Engine;

const TRAY_ICON_PNG: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/linux/hicolor/256x256/apps/zz.png"
));

/// Until `zz/config` grows a client-local key for it, the tray is opt-in
/// through the environment.
const TRAY_ENV: &str = "ZZ_GTK_TRAY";

/// What a tray interaction asks of the window.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TrayEvent {
    Toggle,
    Quit,
}

/// What clicking the icon should do.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ToggleAction {
    Hide,
    Raise,
    Show,
}

/// Maps window visibility and activation onto what a tray click means. A
/// window that is up but buried is summoned rather than dismissed.
pub const fn toggle_action(visible: bool, active: bool) -> ToggleAction {
    match (visible, active) {
        (true, true) => ToggleAction::Hide,
        (true, false) => ToggleAction::Raise,
        (false, _) => ToggleAction::Show,
    }
}

/// Whether the tray is wanted. `ZZ_GTK_TRAY=1` turns it on; anything else,
/// including an unset variable, leaves it off.
pub fn enabled() -> bool {
    std::env::var(TRAY_ENV).is_ok_and(|value| matches!(value.as_str(), "1" | "true" | "yes"))
}

/// A live item. Dropping it takes the icon down, which is also what stops the
/// window from hiding instead of closing.
pub struct Tray {
    handle: ksni::blocking::Handle<Item>,
}

impl Drop for Tray {
    fn drop(&mut self) {
        drop(self.handle.shutdown());
    }
}

/// Put the icon up and rewire the window's close button behind it.
///
/// While the tray is live the window hides instead of closing, so the shell's
/// own close handler — which detaches the session — never runs. Quit drops the
/// item first, which is what lets that handler through on the way out.
pub fn install(window: &adw::ApplicationWindow, engine: Arc<Engine>) {
    if !enabled() {
        return;
    }
    let (sender, events) = async_channel::unbounded();
    let Some(tray) = spawn(sender) else {
        return;
    };
    let slot = Rc::new(RefCell::new(Some(tray)));

    let held = Rc::clone(&slot);
    window.connect_close_request(move |window| {
        if held.borrow().is_none() {
            return glib::Propagation::Proceed;
        }
        window.set_visible(false);
        glib::Propagation::Stop
    });

    pump(window, engine, slot, events);
}

fn pump(
    window: &adw::ApplicationWindow,
    engine: Arc<Engine>,
    slot: Rc<RefCell<Option<Tray>>>,
    events: Receiver<TrayEvent>,
) {
    let window = window.clone();
    glib::spawn_future_local(async move {
        while let Ok(event) = events.recv().await {
            match event {
                TrayEvent::Toggle => match toggle_action(window.is_visible(), window.is_active()) {
                    ToggleAction::Hide => window.set_visible(false),
                    ToggleAction::Raise | ToggleAction::Show => window.present(),
                },
                TrayEvent::Quit => {
                    // Dropping the item first is what stops the close request
                    // from hiding the window a final time.
                    slot.borrow_mut().take();
                    engine.detach();
                    window.close();
                }
            }
        }
    });
}

fn spawn(sender: Sender<TrayEvent>) -> Option<Tray> {
    let item = Item {
        sender,
        icon: icon(),
    };
    match item.spawn() {
        Ok(handle) => Some(Tray { handle }),
        Err(error) => {
            log::warn!("zz-gtk found no system tray host: {error}");
            None
        }
    }
}

struct Item {
    sender: Sender<TrayEvent>,
    icon: Vec<ksni::Icon>,
}

impl Item {
    fn send(&self, event: TrayEvent) {
        if let Err(error) = self.sender.try_send(event) {
            log::warn!("zz-gtk dropped a tray event: {error}");
        }
    }
}

impl ksni::Tray for Item {
    fn id(&self) -> String {
        super::APP_ID.to_owned()
    }

    fn title(&self) -> String {
        "zz".to_owned()
    }

    fn icon_name(&self) -> String {
        super::APP_ID.to_owned()
    }

    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        self.icon.clone()
    }

    // GNOME's AppIndicator extension opens the menu on every click rather than
    // delivering this, so the menu repeats the toggle as its first item.
    fn activate(&mut self, _x: i32, _y: i32) {
        self.send(TrayEvent::Toggle);
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        vec![
            ksni::menu::StandardItem {
                label: "Show/Hide zz".to_owned(),
                activate: Box::new(|item: &mut Self| item.send(TrayEvent::Toggle)),
                ..Default::default()
            }
            .into(),
            ksni::MenuItem::Separator,
            ksni::menu::StandardItem {
                label: "Quit zz".to_owned(),
                activate: Box::new(|item: &mut Self| item.send(TrayEvent::Quit)),
                ..Default::default()
            }
            .into(),
        ]
    }
}

/// SNI pixmaps are ARGB32 in network byte order. `GdkPixbuf` already ships with
/// the toolkit, so the icon needs no decoder of its own.
fn icon() -> Vec<ksni::Icon> {
    let stream = gio::MemoryInputStream::from_bytes(&glib::Bytes::from_static(TRAY_ICON_PNG));
    let Ok(pixbuf) = gdk_pixbuf::Pixbuf::from_stream(&stream, gio::Cancellable::NONE) else {
        log::warn!("zz-gtk could not decode the tray icon");
        return Vec::new();
    };
    let pixbuf = if pixbuf.has_alpha() {
        pixbuf
    } else {
        match pixbuf.add_alpha(false, 0, 0, 0) {
            Ok(opaque) => opaque,
            Err(error) => {
                log::warn!("zz-gtk could not widen the tray icon to RGBA: {error}");
                return Vec::new();
            }
        }
    };
    let width = pixbuf.width();
    let height = pixbuf.height();
    let stride = usize::try_from(pixbuf.rowstride()).unwrap_or_default();
    let bytes = pixbuf.read_pixel_bytes();
    let columns = usize::try_from(width).unwrap_or_default();
    let rows = usize::try_from(height).unwrap_or_default();
    let mut data = Vec::with_capacity(columns * rows * 4);
    for row in 0..rows {
        for column in 0..columns {
            let pixel = row * stride + column * 4;
            let Some(rgba) = bytes.get(pixel..pixel + 4) else {
                log::warn!("zz-gtk read past the tray icon's pixel data");
                return Vec::new();
            };
            data.extend_from_slice(&[rgba[3], rgba[0], rgba[1], rgba[2]]);
        }
    }
    vec![ksni::Icon {
        width,
        height,
        data,
    }]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_icon_dismisses_a_window_that_already_has_focus() {
        assert_eq!(toggle_action(true, true), ToggleAction::Hide);
    }

    #[test]
    fn a_buried_window_is_summoned_rather_than_dismissed() {
        assert_eq!(toggle_action(true, false), ToggleAction::Raise);
    }

    #[test]
    fn a_hidden_window_comes_back_however_focus_reads() {
        assert_eq!(toggle_action(false, false), ToggleAction::Show);
        assert_eq!(toggle_action(false, true), ToggleAction::Show);
    }
}
