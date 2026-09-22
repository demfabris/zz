use async_channel::Sender;
use ksni::blocking::TrayMethods as _;

use super::{
    TrayEvent,
    facts::{MenuEntry, Source},
};

const TRAY_ICON_PNG: &[u8] = if zz_protocol::app_identity::DEVELOPMENT {
    include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../assets/linux/hicolor/256x256/apps/zz-dev.png"
    ))
} else {
    include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../assets/linux/hicolor/256x256/apps/zz.png"
    ))
};

const TRAY_ICON_SIZE: u32 = 48;

/// A live `StatusNotifierItem`. Dropping it shuts the service down, which is what
/// removes the icon.
pub(super) struct Service {
    handle: ksni::blocking::Handle<SniTray>,
}

impl Service {
    pub(super) fn set_attention(&self, count: usize) {
        let _ = self.handle.update(move |tray| tray.attention = count);
    }
}

impl Drop for Service {
    fn drop(&mut self) {
        drop(self.handle.shutdown());
    }
}

pub(super) fn spawn(sender: Sender<TrayEvent>, source: Source) -> Option<Service> {
    let tray = SniTray {
        sender,
        icon: tray_icon(),
        source,
        attention: 0,
    };
    match tray.assume_sni_available(true).spawn() {
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
    source: Source,
    attention: usize,
}

impl SniTray {
    fn send(&self, event: TrayEvent) {
        if let Err(error) = self.sender.try_send(event) {
            log::warn!(target: "zz::tray", "dropped a tray event: {error}");
        }
    }
}

impl ksni::Tray for SniTray {
    fn watcher_online(&self) {
        self.send(TrayEvent::Available(true));
    }

    fn watcher_offline(&self, _reason: ksni::OfflineReason) -> bool {
        self.send(TrayEvent::Available(false));
        true
    }

    fn id(&self) -> String {
        zz_protocol::app_identity::DIRECTORY.into()
    }

    fn title(&self) -> String {
        if self.attention == 0 {
            zz_protocol::app_identity::DISPLAY_NAME.into()
        } else {
            format!("zz · {} waiting", self.attention)
        }
    }

    fn status(&self) -> ksni::Status {
        if self.attention > 0 {
            ksni::Status::NeedsAttention
        } else {
            ksni::Status::Active
        }
    }

    fn icon_name(&self) -> String {
        if zz_protocol::app_identity::DEVELOPMENT {
            String::new()
        } else {
            "zz".into()
        }
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
        self.source
            .menu()
            .into_iter()
            .map(|entry| {
                let label = entry.label();
                match entry {
                    MenuEntry::Separator => ksni::MenuItem::Separator,
                    MenuEntry::Item { action, .. } => ksni::menu::StandardItem {
                        label,
                        enabled: action.is_some(),
                        activate: Box::new(move |tray: &mut Self| {
                            if let Some(action) = &action {
                                tray.send(action.clone());
                            }
                        }),
                        ..Default::default()
                    }
                    .into(),
                }
            })
            .collect()
    }
}

// SNI pixel data is ARGB32 in network byte order.
fn tray_icon() -> Vec<ksni::Icon> {
    let Ok(decoded) = image::load_from_memory_with_format(TRAY_ICON_PNG, image::ImageFormat::Png)
    else {
        log::warn!(target: "zz::tray", "could not decode the tray icon");
        return Vec::new();
    };
    let rgba = decoded
        .resize(
            TRAY_ICON_SIZE,
            TRAY_ICON_SIZE,
            image::imageops::FilterType::Lanczos3,
        )
        .into_rgba8();
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
