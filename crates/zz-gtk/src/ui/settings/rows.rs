use std::{cell::Cell, rc::Rc};

use adw::prelude::*;
use gtk::gdk;

use crate::config::{
    Provenance,
    schema::{self, Choice, Kind, Setting, Support},
};

/// What a row does when the user changes it: write the key, or delete its line
/// when the value is `None`. Rows never touch live state — the poller does.
pub type Write = Rc<dyn Fn(&'static Setting, Option<String>)>;

/// Raised while [`Row::sync`] pushes file state into the widgets, so the
/// change signals that fire do not write the value straight back out.
pub type Syncing = Rc<Cell<bool>>;

const NONE_CHOICE: Choice = Choice {
    value: "",
    title: "None",
};

enum Control {
    Toggle(adw::SwitchRow),
    Number(adw::SpinRow),
    Choice(adw::ComboRow, Vec<Choice>),
    Color(gtk::ColorDialogButton),
    Text(adw::EntryRow),
}

/// One key, rendered. Every kind carries the same two annotations the desktop
/// gives each setting: where the effective value came from, and a reset that is
/// live only while this client's own file has a line to delete.
pub struct Row {
    pub setting: &'static Setting,
    row: adw::PreferencesRow,
    control: Control,
    badge: gtk::Label,
    reset: gtk::Button,
}

impl Row {
    pub fn build(setting: &'static Setting, write: &Write, syncing: &Syncing) -> Self {
        let badge = gtk::Label::new(None);
        badge.add_css_class("dim-label");
        badge.add_css_class("caption");
        badge.set_valign(gtk::Align::Center);

        let reset = gtk::Button::from_icon_name("edit-undo-symbolic");
        reset.add_css_class("flat");
        reset.set_valign(gtk::Align::Center);
        reset.set_tooltip_text(Some("Reset to the default"));
        let target = write.clone();
        reset.connect_clicked(move |_| target(setting, None));

        let suffix = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        suffix.append(&badge);
        suffix.append(&reset);

        let (row, control) = match setting.kind {
            Kind::Toggle { .. } => toggle(setting, write, syncing, &suffix),
            Kind::Number {
                min,
                max,
                step,
                digits,
                ..
            } => number(setting, write, syncing, &suffix, (min, max, step, digits)),
            Kind::Choice { default, options } => {
                choice(setting, write, syncing, &suffix, default, options)
            }
            Kind::Color => color(setting, write, syncing, &suffix),
            Kind::Text { placeholder } => text(setting, write, syncing, &suffix, placeholder),
        };
        row.set_title(setting.title);
        Self {
            setting,
            row,
            control,
            badge,
            reset,
        }
    }

    pub fn widget(&self) -> &adw::PreferencesRow {
        &self.row
    }

    /// Push the resolved value and its provenance into the widgets. `overridden`
    /// is whether this client's file carries the line, which is the only thing
    /// reset can act on — a daemon value sourced from `mux.conf` or a theme file
    /// is not this client's to delete.
    pub fn sync(&self, value: &str, provenance: Provenance, overridden: bool, syncing: &Syncing) {
        syncing.set(true);
        match &self.control {
            Control::Toggle(row) => {
                let default = matches!(self.setting.kind, Kind::Toggle { default: true });
                row.set_active(match value {
                    "true" => true,
                    "false" => false,
                    _ => default,
                });
            }
            Control::Number(row) => {
                row.set_value(
                    value
                        .parse::<f64>()
                        .unwrap_or_else(|_| default_number(self)),
                );
            }
            Control::Choice(row, options) => {
                let selected = options
                    .iter()
                    .position(|option| option.value == value)
                    .unwrap_or(0);
                row.set_selected(selected as u32);
            }
            Control::Color(button) => {
                button.set_rgba(&gdk::RGBA::parse(value).unwrap_or(gdk::RGBA::BLACK));
            }
            Control::Text(row) => {
                if row.text() != value {
                    row.set_text(value);
                }
            }
        }
        self.badge.set_text(provenance.badge());
        self.reset.set_sensitive(overridden);
        syncing.set(false);
    }
}

fn default_number(row: &Row) -> f64 {
    match row.setting.kind {
        Kind::Number { default, .. } => f64::from(default),
        _ => 0.0,
    }
}

/// The subtitle carries the description plus, when this client cannot render a
/// key, why. The key is still written: the file is shared with the zz app, so
/// refusing the edit would be worse than admitting it does nothing here.
fn subtitle(setting: &Setting) -> String {
    match setting.support {
        Support::Honored => setting.description.to_owned(),
        Support::Unwired(note) => {
            format!(
                "{}  ·  Written, not rendered here — {note}",
                setting.description
            )
        }
        Support::Inapplicable(note) => {
            format!("{}  ·  Not used here — {note}", setting.description)
        }
    }
}

fn dress(row: &impl IsA<adw::PreferencesRow>, setting: &Setting) {
    let row = row.as_ref();
    row.set_title(setting.title);
    if matches!(setting.support, Support::Inapplicable(_)) {
        row.add_css_class("dim-label");
    }
}

fn toggle(
    setting: &'static Setting,
    write: &Write,
    syncing: &Syncing,
    suffix: &gtk::Box,
) -> (adw::PreferencesRow, Control) {
    let row = adw::SwitchRow::builder()
        .subtitle(subtitle(setting))
        .build();
    row.set_subtitle_lines(0);
    row.add_suffix(suffix);
    dress(&row, setting);
    let target = write.clone();
    let guard = syncing.clone();
    row.connect_active_notify(move |row| {
        if !guard.get() {
            target(setting, Some(schema::boolean(row.is_active()).to_owned()));
        }
    });
    (row.clone().upcast(), Control::Toggle(row))
}

fn number(
    setting: &'static Setting,
    write: &Write,
    syncing: &Syncing,
    suffix: &gtk::Box,
    (min, max, step, digits): (f64, f64, f64, u32),
) -> (adw::PreferencesRow, Control) {
    let row = adw::SpinRow::builder()
        .subtitle(subtitle(setting))
        .adjustment(&gtk::Adjustment::new(min, min, max, step, step, 0.0))
        .digits(digits)
        .build();
    row.set_subtitle_lines(0);
    row.add_suffix(suffix);
    dress(&row, setting);
    let target = write.clone();
    let guard = syncing.clone();
    row.connect_value_notify(move |row| {
        if !guard.get() {
            target(setting, Some(schema::number(row.value())));
        }
    });
    (row.clone().upcast(), Control::Number(row))
}

fn choice(
    setting: &'static Setting,
    write: &Write,
    syncing: &Syncing,
    suffix: &gtk::Box,
    default: &'static str,
    options: &'static [Choice],
) -> (adw::PreferencesRow, Control) {
    let mut options = options.to_vec();
    if default.is_empty() {
        options.insert(0, NONE_CHOICE);
    }
    let titles: Vec<&str> = options.iter().map(|option| option.title).collect();
    let row = adw::ComboRow::builder()
        .subtitle(subtitle(setting))
        .model(&gtk::StringList::new(&titles))
        .build();
    row.set_subtitle_lines(0);
    row.add_suffix(suffix);
    dress(&row, setting);
    let target = write.clone();
    let guard = syncing.clone();
    let chosen = options.clone();
    row.connect_selected_notify(move |row| {
        if guard.get() {
            return;
        }
        let Some(option) = chosen.get(row.selected() as usize) else {
            return;
        };
        target(
            setting,
            (!option.value.is_empty()).then(|| option.value.to_owned()),
        );
    });
    (row.clone().upcast(), Control::Choice(row, options))
}

/// Colors that carry alpha: the daemon stores these as RGBA and the desktop
/// writes `#rrggbbaa` for them. The rest are RGB and must stay six digits or
/// the daemon's parser sees a value it will not round-trip.
fn carries_alpha(setting: &Setting) -> bool {
    matches!(
        setting.key,
        "selection-background"
            | "zz-search-match-color"
            | "zz-search-current-color"
            | "zz-copy-cursor-color"
            | "chrome-background"
            | "chrome-foreground"
            | "chrome-border"
            | "chrome-success"
            | "chrome-warning"
            | "chrome-danger"
    )
}

fn color(
    setting: &'static Setting,
    write: &Write,
    syncing: &Syncing,
    suffix: &gtk::Box,
) -> (adw::PreferencesRow, Control) {
    let alpha = carries_alpha(setting);
    let button = gtk::ColorDialogButton::new(Some(
        gtk::ColorDialog::builder()
            .with_alpha(alpha)
            .title(setting.title)
            .build(),
    ));
    button.set_valign(gtk::Align::Center);
    let row = adw::ActionRow::builder()
        .subtitle(subtitle(setting))
        .build();
    row.set_subtitle_lines(0);
    row.add_suffix(&button);
    row.add_suffix(suffix);
    dress(&row, setting);
    let target = write.clone();
    let guard = syncing.clone();
    button.connect_rgba_notify(move |button| {
        if !guard.get() {
            target(setting, Some(hex(button.rgba(), alpha)));
        }
    });
    (row.upcast(), Control::Color(button))
}

fn hex(color: gdk::RGBA, alpha: bool) -> String {
    let channel = |value: f32| (value.clamp(0.0, 1.0) * 255.0).round() as u8;
    let (red, green, blue) = (
        channel(color.red()),
        channel(color.green()),
        channel(color.blue()),
    );
    if alpha && color.alpha() < 1.0 {
        format!(
            "#{red:02X}{green:02X}{blue:02X}{:02X}",
            channel(color.alpha())
        )
    } else {
        format!("#{red:02X}{green:02X}{blue:02X}")
    }
}

fn text(
    setting: &'static Setting,
    write: &Write,
    syncing: &Syncing,
    suffix: &gtk::Box,
    placeholder: &'static str,
) -> (adw::PreferencesRow, Control) {
    let row = adw::EntryRow::builder()
        .show_apply_button(true)
        .tooltip_text(subtitle(setting))
        .build();
    row.add_suffix(suffix);
    if !placeholder.is_empty() {
        row.set_text("");
        row.set_tooltip_text(Some(&format!(
            "{}\nEmpty means: {placeholder}",
            subtitle(setting)
        )));
    }
    dress(&row, setting);
    let target = write.clone();
    let guard = syncing.clone();
    row.connect_apply(move |row| {
        if guard.get() {
            return;
        }
        let value = row.text().trim().to_owned();
        target(setting, (!value.is_empty()).then_some(value));
    });
    (row.clone().upcast(), Control::Text(row))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rgb_keys_never_grow_an_alpha_channel() {
        let translucent = gdk::RGBA::new(1.0, 0.0, 0.0, 0.5);

        assert_eq!(hex(translucent, false), "#FF0000");
        assert_eq!(hex(translucent, true), "#FF000080");
        assert_eq!(hex(gdk::RGBA::new(1.0, 0.0, 0.0, 1.0), true), "#FF0000");
    }
}
