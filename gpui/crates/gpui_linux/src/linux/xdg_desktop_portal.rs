//! Provides a [calloop] event source from [XDG Desktop Portal] events
//!
//! This module uses the [ashpd] crate

use ashpd::desktop::settings::{ColorScheme, Settings};
use calloop::channel::Channel;
use calloop::{EventSource, Poll, PostAction, Readiness, Token, TokenFactory};
use smol::stream::StreamExt;
use std::time::Duration;

use gpui::{BackgroundExecutor, WindowAppearance};

pub enum Event {
    WindowAppearance(WindowAppearance),
    #[cfg_attr(feature = "x11", allow(dead_code))]
    CursorTheme(String),
    #[cfg_attr(feature = "x11", allow(dead_code))]
    CursorSize(u32),
    ButtonLayout(String),
    SystemFont(Vec<String>),
}

pub struct XDPEventSource {
    channel: Channel<Event>,
}

impl XDPEventSource {
    pub fn new(executor: &BackgroundExecutor) -> Self {
        let (sender, channel) = calloop::channel::channel();

        let background = executor.clone();

        executor
            .spawn(async move {
                let settings = Settings::new().await?;

                if let Ok(initial_appearance) = settings.color_scheme().await {
                    sender.send(Event::WindowAppearance(
                        window_appearance_from_color_scheme(initial_appearance),
                    ))?;
                }
                for (namespace, key, parse) in SYSTEM_FONT_SETTINGS {
                    if let Ok(font) = settings.read::<String>(namespace, key).await {
                        sender.send(Event::SystemFont(parse(&font)))?;
                        break;
                    }
                }
                if let Ok(initial_theme) = settings
                    .read::<String>("org.gnome.desktop.interface", "cursor-theme")
                    .await
                {
                    sender.send(Event::CursorTheme(initial_theme))?;
                }

                // If u32 is used here, it throws invalid type error
                if let Ok(initial_size) = settings
                    .read::<i32>("org.gnome.desktop.interface", "cursor-size")
                    .await
                {
                    sender.send(Event::CursorSize(initial_size as u32))?;
                }

                if let Ok(initial_layout) = settings
                    .read::<String>("org.gnome.desktop.wm.preferences", "button-layout")
                    .await
                {
                    sender.send(Event::ButtonLayout(initial_layout))?;
                }

                if let Ok(mut cursor_theme_changed) = settings
                    .receive_setting_changed_with_args(
                        "org.gnome.desktop.interface",
                        "cursor-theme",
                    )
                    .await
                {
                    let sender = sender.clone();
                    background
                        .spawn(async move {
                            while let Some(theme) = cursor_theme_changed.next().await {
                                let theme = theme?;
                                sender.send(Event::CursorTheme(theme))?;
                            }
                            anyhow::Ok(())
                        })
                        .detach();
                }

                if let Ok(mut cursor_size_changed) = settings
                    .receive_setting_changed_with_args::<i32>(
                        "org.gnome.desktop.interface",
                        "cursor-size",
                    )
                    .await
                {
                    let sender = sender.clone();
                    background
                        .spawn(async move {
                            while let Some(size) = cursor_size_changed.next().await {
                                let size = size?;
                                sender.send(Event::CursorSize(size as u32))?;
                            }
                            anyhow::Ok(())
                        })
                        .detach();
                }

                if let Ok(mut button_layout_changed) = settings
                    .receive_setting_changed_with_args(
                        "org.gnome.desktop.wm.preferences",
                        "button-layout",
                    )
                    .await
                {
                    let sender = sender.clone();
                    background
                        .spawn(async move {
                            while let Some(layout) = button_layout_changed.next().await {
                                let layout = layout?;
                                sender.send(Event::ButtonLayout(layout))?;
                            }
                            anyhow::Ok(())
                        })
                        .detach();
                }

                for (namespace, key, parse) in SYSTEM_FONT_SETTINGS {
                    if let Ok(mut font_changed) = settings
                        .receive_setting_changed_with_args::<String>(namespace, key)
                        .await
                    {
                        let sender = sender.clone();
                        background
                            .spawn(async move {
                                while let Some(font) = font_changed.next().await {
                                    sender.send(Event::SystemFont(parse(&font?)))?;
                                }
                                anyhow::Ok(())
                            })
                            .detach();
                    }
                }

                let mut appearance_changed = settings.receive_color_scheme_changed().await?;
                while let Some(scheme) = appearance_changed.next().await {
                    sender.send(Event::WindowAppearance(
                        window_appearance_from_color_scheme(scheme),
                    ))?;
                }

                anyhow::Ok(())
            })
            .detach();

        Self { channel }
    }
}

impl EventSource for XDPEventSource {
    type Event = Event;
    type Metadata = ();
    type Ret = ();
    type Error = anyhow::Error;

    fn process_events<F>(
        &mut self,
        readiness: Readiness,
        token: Token,
        mut callback: F,
    ) -> Result<PostAction, Self::Error>
    where
        F: FnMut(Self::Event, &mut Self::Metadata) -> Self::Ret,
    {
        self.channel.process_events(readiness, token, |evt, _| {
            if let calloop::channel::Event::Msg(msg) = evt {
                (callback)(msg, &mut ())
            }
        })?;

        Ok(PostAction::Continue)
    }

    fn register(
        &mut self,
        poll: &mut Poll,
        token_factory: &mut TokenFactory,
    ) -> calloop::Result<()> {
        self.channel.register(poll, token_factory)?;

        Ok(())
    }

    fn reregister(
        &mut self,
        poll: &mut Poll,
        token_factory: &mut TokenFactory,
    ) -> calloop::Result<()> {
        self.channel.reregister(poll, token_factory)?;

        Ok(())
    }

    fn unregister(&mut self, poll: &mut Poll) -> calloop::Result<()> {
        self.channel.unregister(poll)?;

        Ok(())
    }
}

/// Reads the desktop interface font before any text is shaped, so windows
/// never draw their first frames in a fallback family. Later changes arrive
/// through [`XDPEventSource`].
#[allow(
    clippy::disallowed_methods,
    reason = "runs while the platform is constructed, before any GPUI executor can drive a timer"
)]
pub(crate) fn read_system_font(timeout: Duration) -> Option<Vec<String>> {
    smol::block_on(smol::future::or(
        async {
            let settings = Settings::new().await.ok()?;
            for (namespace, key, parse) in SYSTEM_FONT_SETTINGS {
                if let Ok(font) = settings.read::<String>(namespace, key).await {
                    return Some(parse(&font));
                }
            }
            None
        },
        async {
            smol::Timer::after(timeout).await;
            None
        },
    ))
}

type FontSettingParser = fn(&str) -> Vec<String>;

const SYSTEM_FONT_SETTINGS: [(&str, &str, FontSettingParser); 2] = [
    (
        "org.gnome.desktop.interface",
        "font-name",
        pango_font_families,
    ),
    ("org.kde.kdeglobals.General", "font", qt_font_families),
];

/// Candidate families for a Pango description such as `Ubuntu Sans Bold 11`,
/// longest first, since style words and the family share one space-separated list.
fn pango_font_families(description: &str) -> Vec<String> {
    let mut families = description.split(',').map(str::trim).collect::<Vec<_>>();
    let Some(last) = families.pop() else {
        return Vec::new();
    };
    let mut words = last.split_whitespace().collect::<Vec<_>>();
    if words
        .last()
        .is_some_and(|size| size.trim_end_matches("px").parse::<f32>().is_ok())
    {
        words.pop();
    }
    let mut candidates = families
        .into_iter()
        .filter(|family| !family.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    candidates.extend((1..=words.len()).rev().map(|len| words[..len].join(" ")));
    candidates
}

/// The family of a Qt font string such as `Noto Sans,10,-1,5,50,0,0,0,0,0`.
fn qt_font_families(description: &str) -> Vec<String> {
    description
        .split(',')
        .next()
        .map(str::trim)
        .filter(|family| !family.is_empty())
        .map(|family| vec![family.to_owned()])
        .unwrap_or_default()
}

fn window_appearance_from_color_scheme(cs: ColorScheme) -> WindowAppearance {
    match cs {
        ColorScheme::PreferDark => WindowAppearance::Dark,
        ColorScheme::PreferLight => WindowAppearance::Light,
        ColorScheme::NoPreference => WindowAppearance::Light,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pango_descriptions_offer_the_family_before_style_and_size() {
        assert_eq!(
            pango_font_families("Ubuntu Sans 11"),
            ["Ubuntu Sans", "Ubuntu"]
        );
        assert_eq!(
            pango_font_families("Noto Sans Bold 10.5"),
            ["Noto Sans Bold", "Noto Sans", "Noto"]
        );
        assert_eq!(
            pango_font_families("Cantarell, Noto Sans 11"),
            ["Cantarell", "Noto Sans", "Noto"]
        );
        assert!(pango_font_families("").is_empty());
    }

    #[test]
    fn qt_font_strings_offer_their_family() {
        assert_eq!(
            qt_font_families("Noto Sans,10,-1,5,50,0,0,0,0,0"),
            ["Noto Sans"]
        );
        assert!(qt_font_families(",10").is_empty());
    }
}
