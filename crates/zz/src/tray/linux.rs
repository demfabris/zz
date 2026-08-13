use async_channel::Sender;
use ksni::blocking::TrayMethods as _;

use super::TrayEvent;

const TRAY_ICON_PNG: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/linux/hicolor/256x256/apps/zz.png"
));

/// A live `StatusNotifierItem`. Dropping it shuts the service down, which is what
/// removes the icon.
pub(super) struct Service {
    handle: ksni::blocking::Handle<SniTray>,
}

impl Drop for Service {
    fn drop(&mut self) {
        drop(self.handle.shutdown());
    }
}

pub(super) fn spawn(sender: Sender<TrayEvent>) -> Option<Service> {
    let tray = SniTray {
        sender,
        icon: tray_icon(),
    };
    match tray.spawn() {
        Ok(handle) => Some(Service { handle }),
        Err(error) => {
            log::warn!(target: "zz::tray", "no system tray host: {error}");
            None
        }
    }
}

struct SniTray {
    sender: Sender<TrayEvent>,
    icon: Vec<ksni::Icon>,
}

impl SniTray {
    fn send(&self, event: TrayEvent) {
        if let Err(error) = self.sender.try_send(event) {
            log::warn!(target: "zz::tray", "dropped a tray event: {error}");
        }
    }
}

impl ksni::Tray for SniTray {
    fn id(&self) -> String {
        "zz".into()
    }

    fn title(&self) -> String {
        "zz".into()
    }

    fn icon_name(&self) -> String {
        "zz".into()
    }

    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        self.icon.clone()
    }

    // GNOME's AppIndicator extension opens the menu on every click instead of
    // delivering this, so the menu repeats the toggle as its first item.
    fn activate(&mut self, _x: i32, _y: i32) {
        self.send(TrayEvent::Toggle);
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        vec![
            ksni::menu::StandardItem {
                label: "Show/Hide".into(),
                activate: Box::new(|tray: &mut Self| tray.send(TrayEvent::Toggle)),
                ..Default::default()
            }
            .into(),
            ksni::MenuItem::Separator,
            ksni::menu::StandardItem {
                label: "Quit zz".into(),
                activate: Box::new(|tray: &mut Self| tray.send(TrayEvent::Quit)),
                ..Default::default()
            }
            .into(),
        ]
    }
}

// SNI pixel data is ARGB32 in network byte order.
fn tray_icon() -> Vec<ksni::Icon> {
    let Ok(decoded) = image::load_from_memory_with_format(TRAY_ICON_PNG, image::ImageFormat::Png)
    else {
        log::warn!(target: "zz::tray", "could not decode the tray icon");
        return Vec::new();
    };
    let rgba = decoded.into_rgba8();
    let (width, height) = rgba.dimensions();
    let data = rgba
        .pixels()
        .flat_map(|pixel| {
            let [r, g, b, a] = pixel.0;
            [a, r, g, b]
        })
        .collect();
    vec![ksni::Icon {
        width: width as i32,
        height: height as i32,
        data,
    }]
}
