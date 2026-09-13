use std::{
    fs,
    io::{self, Write as _},
    path::PathBuf,
    sync::{
        OnceLock,
        atomic::{AtomicBool, AtomicU32, Ordering},
    },
};

use base64::{Engine as _, engine::general_purpose::STANDARD};

#[cfg(unix)]
use rustix::termios::{OptionalActions, Termios};

use zz_daemon::{CommandClient, Endpoint, terminal_default_features, terminal_feature_mask};
use zz_protocol::CommandInvocation;

use crate::kitty::{FILE_PROBE_IMAGE_ID, PROBE_IMAGE_ID, cleanup_frame_slot_files};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct TerminalSize {
    pub columns: u16,
    pub rows: u16,
    pub cell_width_px: u32,
    pub cell_height_px: u32,
}

impl TerminalSize {
    #[cfg(unix)]
    pub fn detect() -> io::Result<Self> {
        let size = rustix::termios::tcgetwinsize(io::stdout())?;
        let cell_width_px = pixel_cell_extent(size.ws_xpixel, size.ws_col, 8);
        let cell_height_px = pixel_cell_extent(size.ws_ypixel, size.ws_row, 16);
        Ok(Self {
            columns: size.ws_col,
            rows: size.ws_row,
            cell_width_px,
            cell_height_px,
        })
    }

    pub const fn with_cell_pixels(mut self, width_px: u32, height_px: u32) -> Self {
        self.cell_width_px = width_px;
        self.cell_height_px = height_px;
        self
    }

    #[cfg(not(unix))]
    pub fn detect() -> io::Result<Self> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "zz-tui currently requires a Unix terminal",
        ))
    }
}

fn pixel_cell_extent(pixels: u16, cells: u16, fallback: u32) -> u32 {
    if pixels == 0 || cells == 0 {
        fallback
    } else {
        (u32::from(pixels) / u32::from(cells)).max(1)
    }
}

pub(crate) struct TerminalGuard {
    pixel_mouse: bool,
    kitty_keyboard: bool,
    kitty_graphics: bool,
    file_probe: Option<PathBuf>,
    #[cfg(unix)]
    original: Termios,
}

const MOUSE_CLEAR_SEQUENCE: &[u8] = b"\x1b[?1016l\x1b[?1006l\x1b[?1000l\x1b[?1002l\x1b[?1003l";

/// `tty_send_requests`: the primary device attributes the kitty probe already
/// fences on, then the secondary and the extended ones, whose replies name the
/// terminal and the features it carries.
const TERMINAL_REQUESTS: &[u8] = b"\x1b[c\x1b[>c\x1b[>q";

/// How many colours the terminal this client writes to takes. `tty.c` asks it
/// of every cell it sends (`tty_check_fg` and `tty_check_bg`) and never lowers
/// it: `tty_term_create` decides it from `TERM`, `COLORTERM` and the requested
/// features, and `tty_update_features` raises it when the terminal answers a
/// request. Zero until a terminal is entered, and a writer with no terminal
/// behind it changes no colour at all.
static TERMINAL_COLOURS: AtomicU32 = AtomicU32::new(0);

pub(crate) fn terminal_colours() -> Option<u32> {
    match TERMINAL_COLOURS.load(Ordering::Relaxed) {
        0 => None,
        colours => Some(colours),
    }
}

fn raise_terminal_colours(colours: u32) {
    TERMINAL_COLOURS.fetch_max(colours, Ordering::Relaxed);
}

/// `CLIENT_UTF8`, the flag `tty_check_codeset` reads before it writes a cell.
/// `tmux.c` decides it in the client process from `-u` and the locale and
/// never revisits it, so this reads it once too.
pub(crate) fn terminal_takes_utf8() -> bool {
    static TAKES_UTF8: OnceLock<bool> = OnceLock::new();
    *TAKES_UTF8.get_or_init(zz_daemon::client_takes_utf8_terminal)
}

/// `tty_keys_device_attributes2` reads the first parameter of a secondary DA
/// as a letter and hands `tty_default_features` the terminal it names.
pub(crate) fn note_secondary_device_attributes(kind: u8) {
    learn_terminal_features(terminal_default_features(secondary_device_attributes_name(
        kind,
    )));
}

fn secondary_device_attributes_name(kind: u8) -> &'static str {
    match kind {
        b'M' => "mintty",
        b'T' => "tmux",
        b'U' => "rxvt-unicode",
        _ => "",
    }
}

/// `tty_keys_extended_device_attributes`: an XTVERSION reply names the
/// terminal outright, and `tty_default_features` decides what that name
/// carries.
pub(crate) fn note_extended_device_attributes(name: &str) {
    learn_terminal_features(terminal_default_features(extended_device_attributes_name(
        name,
    )));
}

fn extended_device_attributes_name(reply: &str) -> &'static str {
    const NAMED: [(&str, &str); 7] = [
        ("iTerm2 ", "iTerm2"),
        ("tmux ", "tmux"),
        ("XTerm(", "XTerm"),
        ("mintty ", "mintty"),
        ("foot(", "foot"),
        ("WezTerm ", "WezTerm"),
        ("ghostty ", "ghostty"),
    ];
    NAMED
        .iter()
        .find(|(prefix, _)| reply.starts_with(prefix))
        .map_or("", |(_, name)| *name)
}

/// `tty_update_features`: the reply reaches this client's own feature set,
/// which decides the colours the cell writer may send, the roster its daemon
/// publishes and whether the extended-key request goes out at all.
fn learn_terminal_features(features: &str) {
    if features.is_empty() {
        return;
    }
    zz_daemon::learn_client_terminal_features(features);
    raise_terminal_colours(zz_daemon::client_terminal_colour_count());
    arm_extended_keys();
}

static EXTENDED_KEYS_OPTION: AtomicBool = AtomicBool::new(false);
static EXTENDED_KEYS_ARMED: AtomicBool = AtomicBool::new(false);

/// `tty_update_features` writes `Eneks` while `extended-keys` is on, and
/// `tty_term_string` answers empty unless the terminal carries the `extkeys`
/// feature, so a terminal that has not named itself never sees the request.
/// `tty_start_tty` writes it never: the pin's own first chance is the reply,
/// or the query timeout firing with none.
fn arm_extended_keys() {
    if !EXTENDED_KEYS_OPTION.load(Ordering::Relaxed) || !terminal_carries_extended_keys() {
        return;
    }
    if EXTENDED_KEYS_ARMED.swap(true, Ordering::Relaxed) {
        return;
    }
    let mut output = io::stdout().lock();
    let _ = output.write_all(EXTENDED_KEYS_ENABLE);
    let _ = output.flush();
}

fn terminal_carries_extended_keys() -> bool {
    terminal_feature_mask(["extkeys"]) & zz_daemon::client_terminal_feature_mask() != 0
}
/// `smkx` and `rmkx` on every vt100-like terminal: `tty_start_tty` puts the
/// keypad and the cursor keys into application mode for the whole attach and
/// `tty_stop_tty` puts them back, and `tty_default_raw_keys` decodes what they
/// then send.
const KEYPAD_TRANSMIT: &[u8] = b"\x1b[?1h\x1b=";
const KEYPAD_LOCAL: &[u8] = b"\x1b[?1l\x1b>";

/// `tty_start_tty` subscribes to theme changes and asks for the theme now;
/// `tty_stop_tty` unsubscribes. The terminal answers, and goes on answering,
/// with `\e[?997;1n` for dark and `\e[?997;2n` for light.
const THEME_SUBSCRIBE: &[u8] = b"\x1b[?2031h\x1b[?996n";
const THEME_UNSUBSCRIBE: &[u8] = b"\x1b[?2031l";
const FOCUS_EVENTS_ENABLE: &[u8] = b"\x1b[?1004h";
const EXTENDED_KEYS_ENABLE: &[u8] = b"\x1b[>4;2m";
const EXTENDED_KEYS_DISABLE: &[u8] = b"\x1b[>4m";

pub(crate) fn extended_keys_option(endpoint: &Endpoint) -> bool {
    let Endpoint::Local(path) = endpoint else {
        return false;
    };
    CommandClient::connect(path)
        .and_then(|mut client| {
            client.execute(CommandInvocation::new(
                "show-options",
                ["-sv", "extended-keys"],
            ))
        })
        .is_ok_and(|value| extended_keys_armed(&value))
}

fn extended_keys_armed(value: &str) -> bool {
    !matches!(value.trim(), "" | "off")
}

/// `tty_start_tty` writes `Enfcs` only while the server option is on, and its
/// default is off (options-table.c). The option is read once, where the pin
/// reads it, because the pin arms once too.
pub(crate) fn focus_events_option(endpoint: &Endpoint) -> bool {
    let Endpoint::Local(path) = endpoint else {
        return false;
    };
    CommandClient::connect(path)
        .and_then(|mut client| {
            client.execute(CommandInvocation::new(
                "show-options",
                ["-sv", "focus-events"],
            ))
        })
        .is_ok_and(|value| value.trim() == "on")
}

/// `tty_update_mode`: which of the pin's three mouse trackings the outer
/// terminal is put in. `Button` is `\e[?1000h\e[?1002h`, `Any` adds
/// `\e[?1003h`, and every change clears all four first.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum MouseArming {
    #[default]
    Off,
    Button,
    Any,
}

pub(crate) fn mouse_mode_sequence(arming: MouseArming, pixel_mouse: bool) -> Vec<u8> {
    let mut sequence = MOUSE_CLEAR_SEQUENCE.to_vec();
    match arming {
        MouseArming::Off => return sequence,
        MouseArming::Button => sequence.extend_from_slice(b"\x1b[?1006h\x1b[?1000h\x1b[?1002h"),
        MouseArming::Any => {
            sequence.extend_from_slice(b"\x1b[?1006h\x1b[?1000h\x1b[?1002h\x1b[?1003h");
        }
    }
    if pixel_mouse {
        sequence.extend_from_slice(b"\x1b[?1016h");
    }
    sequence
}

impl TerminalGuard {
    #[cfg(unix)]
    pub fn enter(mouse: MouseArming, extended_keys: bool, focus_events: bool) -> io::Result<Self> {
        let original = rustix::termios::tcgetattr(io::stdin())?;
        let file_probe = probe_file_path();
        remove_file_if_present(&file_probe)?;
        fs::write(&file_probe, [0_u8; 4])?;
        let encoded_probe_path = STANDARD.encode(file_probe.as_os_str().as_encoded_bytes());
        let mut raw = original.clone();
        raw.make_raw();
        if let Err(error) = rustix::termios::tcsetattr(io::stdin(), OptionalActions::Now, &raw) {
            let _ = fs::remove_file(&file_probe);
            return Err(error.into());
        }
        let guard = Self {
            pixel_mouse: supports_pixel_mouse(),
            kitty_keyboard: supports_kitty_keyboard(),
            kitty_graphics: false,
            file_probe: Some(file_probe),
            original,
        };
        EXTENDED_KEYS_OPTION.store(extended_keys, Ordering::Relaxed);
        TERMINAL_COLOURS.store(zz_daemon::client_terminal_colour_count(), Ordering::Relaxed);
        let mut output = io::stdout().lock();
        output.write_all(b"\x1b[?1049h\x1b[?25l")?;
        if focus_events {
            output.write_all(FOCUS_EVENTS_ENABLE)?;
        }
        output.write_all(KEYPAD_TRANSMIT)?;
        if mouse != MouseArming::Off {
            output.write_all(&mouse_mode_sequence(mouse, guard.pixel_mouse))?;
        }
        output.write_all(b"\x1b[?2004h")?;
        if guard.kitty_keyboard {
            output.write_all(b"\x1b[>3u")?;
        }
        write!(
            output,
            "\x1b_Gi={PROBE_IMAGE_ID},s=1,v=1,a=q,t=d,f=24;AAAA\x1b\\\x1b_Gi={FILE_PROBE_IMAGE_ID},s=1,v=1,a=q,t=f,f=32;{encoded_probe_path}\x1b\\"
        )?;
        output.write_all(TERMINAL_REQUESTS)?;
        output.write_all(THEME_SUBSCRIBE)?;
        output.write_all(b"\x1b[16t\x1b[2J")?;
        output.flush()?;
        drop(output);
        arm_extended_keys();
        Ok(guard)
    }

    #[cfg(not(unix))]
    pub fn enter(
        _mouse: MouseArming,
        _extended_keys: bool,
        _focus_events: bool,
    ) -> io::Result<Self> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "zz-tui currently requires a Unix terminal",
        ))
    }

    pub const fn pixel_mouse(&self) -> bool {
        self.pixel_mouse
    }

    pub const fn kitty_keyboard(&self) -> bool {
        self.kitty_keyboard
    }

    pub const fn activate_kitty_graphics(&mut self) {
        self.kitty_graphics = true;
    }

    pub fn finish_file_probe(&mut self) {
        if let Some(path) = self.file_probe.take() {
            let _ = fs::remove_file(path);
        }
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        self.finish_file_probe();
        cleanup_frame_slot_files();
        let mut output = io::stdout().lock();
        let _ = output.write_all(b"\x1b[?2026l\x1b[0m\x1b]112\x07");
        if self.kitty_graphics {
            let _ = output.write_all(b"\x1b_Ga=d,d=A,q=2\x1b\\");
        }
        if self.kitty_keyboard {
            let _ = output.write_all(b"\x1b[<1u");
        }
        if EXTENDED_KEYS_ARMED.swap(false, Ordering::Relaxed) {
            let _ = output.write_all(EXTENDED_KEYS_DISABLE);
        }
        let _ = output.write_all(THEME_UNSUBSCRIBE);
        let _ = output.write_all(KEYPAD_LOCAL);
        let _ = output.write_all(
            b"\x1b[?2004l\x1b[?1016l\x1b[?1006l\x1b[?1000l\x1b[?1002l\x1b[?1003l\x1b[?1004l\x1b[?25h\x1b[?1049l",
        );
        let _ = output.flush();
        #[cfg(unix)]
        let _ = rustix::termios::tcsetattr(io::stdin(), OptionalActions::Now, &self.original);
    }
}

fn probe_file_path() -> PathBuf {
    std::env::temp_dir().join(format!("zz-tui-{}-probe.rgba", std::process::id()))
}

fn remove_file_if_present(path: &std::path::Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn supports_pixel_mouse() -> bool {
    terminal_supports(["ghostty", "kitty", "wezterm", "foot"])
}

fn supports_kitty_keyboard() -> bool {
    terminal_supports(["ghostty", "kitty", "wezterm", "foot", "zz"])
}

fn terminal_supports<const N: usize>(names: [&str; N]) -> bool {
    let term = std::env::var("TERM")
        .unwrap_or_default()
        .to_ascii_lowercase();
    let program = std::env::var("TERM_PROGRAM")
        .unwrap_or_default()
        .to_ascii_lowercase();
    names
        .into_iter()
        .any(|name| term.contains(name) || program.contains(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pixel_geometry_uses_ioctl_values_or_documented_fallbacks() {
        assert_eq!(pixel_cell_extent(1600, 200, 8), 8);
        assert_eq!(pixel_cell_extent(0, 200, 8), 8);
        assert_eq!(pixel_cell_extent(40, 80, 8), 1);
    }

    #[test]
    fn mouse_sequences_emit_and_retract_the_tmux_outer_modes() {
        assert_eq!(
            mouse_mode_sequence(MouseArming::Button, false),
            b"\x1b[?1016l\x1b[?1006l\x1b[?1000l\x1b[?1002l\x1b[?1003l\x1b[?1006h\x1b[?1000h\x1b[?1002h"
        );
        assert_eq!(
            mouse_mode_sequence(MouseArming::Any, false),
            b"\x1b[?1016l\x1b[?1006l\x1b[?1000l\x1b[?1002l\x1b[?1003l\x1b[?1006h\x1b[?1000h\x1b[?1002h\x1b[?1003h"
        );
        assert_eq!(
            mouse_mode_sequence(MouseArming::Any, true),
            b"\x1b[?1016l\x1b[?1006l\x1b[?1000l\x1b[?1002l\x1b[?1003l\x1b[?1006h\x1b[?1000h\x1b[?1002h\x1b[?1003h\x1b[?1016h"
        );
        assert_eq!(
            mouse_mode_sequence(MouseArming::Off, true),
            b"\x1b[?1016l\x1b[?1006l\x1b[?1000l\x1b[?1002l\x1b[?1003l"
        );
    }

    #[test]
    fn the_keypad_is_armed_on_entry_and_put_back_on_exit() {
        assert_eq!(KEYPAD_TRANSMIT, b"\x1b[?1h\x1b=");
        assert_eq!(KEYPAD_LOCAL, b"\x1b[?1l\x1b>");
        assert_eq!(THEME_SUBSCRIBE, b"\x1b[?2031h\x1b[?996n");
        assert_eq!(THEME_UNSUBSCRIBE, b"\x1b[?2031l");
    }

    #[test]
    fn only_the_terminals_the_pin_names_carry_features() {
        assert_eq!(secondary_device_attributes_name(b'T'), "tmux");
        assert_eq!(secondary_device_attributes_name(b'M'), "mintty");
        assert_eq!(secondary_device_attributes_name(b'U'), "rxvt-unicode");
        assert_eq!(secondary_device_attributes_name(b'V'), "");
        assert_eq!(extended_device_attributes_name("ghostty 1.2.3"), "ghostty");
        assert_eq!(extended_device_attributes_name("XTerm(400)"), "XTerm");
        assert_eq!(extended_device_attributes_name("tmux 3.8"), "tmux");
        assert_eq!(extended_device_attributes_name("Konsole 2.0"), "");
        assert_eq!(extended_device_attributes_name("tmux"), "");
        assert!(terminal_default_features("tmux").contains("RGB"));
        assert!(terminal_default_features("rxvt-unicode").contains("256"));
        assert_eq!(terminal_default_features(""), "");
    }

    #[test]
    fn the_startup_requests_carry_the_pin_three_attribute_queries() {
        assert!(TERMINAL_REQUESTS.ends_with(b"\x1b[c\x1b[>c\x1b[>q"));
    }

    #[test]
    fn extended_keys_arm_for_every_value_but_off() {
        assert!(extended_keys_armed("on\n"));
        assert!(extended_keys_armed("always"));
        assert!(!extended_keys_armed("off\n"));
        assert!(!extended_keys_armed(""));
        assert_eq!(EXTENDED_KEYS_ENABLE, b"\x1b[>4;2m");
        assert_eq!(EXTENDED_KEYS_DISABLE, b"\x1b[>4m");
    }
}
