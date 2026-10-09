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

use zz_daemon::{terminal_default_features, terminal_feature_mask};
use zz_protocol::{MuxOptionKey, ServerHello};

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
        let cell_width_px = zz_daemon::cell_pixel_extent(
            size.ws_xpixel,
            size.ws_col,
            zz_daemon::DEFAULT_CELL_WIDTH_PX,
        );
        let cell_height_px = zz_daemon::cell_pixel_extent(
            size.ws_ypixel,
            size.ws_row,
            zz_daemon::DEFAULT_CELL_HEIGHT_PX,
        );
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

pub(crate) struct TerminalGuard {
    active: bool,
    pixel_mouse: bool,
    kitty_keyboard: bool,
    kitty_graphics: bool,
    file_probe: Option<PathBuf>,
    #[cfg(unix)]
    original: Termios,
    writer: std::rc::Rc<std::cell::RefCell<crate::writer::TerminalWriter>>,
}

const MOUSE_CLEAR_SEQUENCE: &[u8] = b"\x1b[?1016l\x1b[?1006l\x1b[?1000l\x1b[?1002l\x1b[?1003l";

/// `tty_send_requests`: the primary device attributes the kitty probe already
/// fences on, then the secondary and the extended ones, whose replies name the
/// terminal and the features it carries.
const TERMINAL_REQUESTS: &[u8] = b"\x1b[c\x1b[>c\x1b[>q";
const DEVICE_ATTRIBUTES_REQUEST: &[u8] = b"\x1b[c";

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
    zz_daemon::report_terminal_type(name);
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

pub(crate) fn adopt_negotiated_features(features: &[String]) {
    zz_daemon::adopt_negotiated_terminal_features(features);
    if terminal_colours().is_some() {
        raise_terminal_colours(zz_daemon::client_terminal_colour_count());
    }
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
    ACTIVE_OUTPUT.with(|output| {
        if let Some(writer) = output.borrow().as_ref() {
            let _ = writer.borrow_mut().control(EXTENDED_KEYS_ENABLE.to_vec());
        }
    });
}

fn terminal_carries_extended_keys() -> bool {
    terminal_carries("extkeys")
}

pub(crate) fn terminal_carries(feature: &str) -> bool {
    terminal_feature_mask([feature]) & zz_daemon::client_terminal_feature_mask() != 0
}

static CURSOR_STYLE_SENT: AtomicBool = AtomicBool::new(false);

pub(crate) fn note_cursor_style_sent() {
    CURSOR_STYLE_SENT.store(true, Ordering::Relaxed);
}

#[cfg(test)]
pub(crate) fn cursor_style_needs_restore() -> bool {
    CURSOR_STYLE_SENT.load(Ordering::Relaxed)
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

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct TerminalOptions {
    pub extended_keys: bool,
    pub focus_events: bool,
}

impl TerminalOptions {
    pub fn from_hello(hello: &ServerHello) -> Option<Self> {
        Some(Self {
            extended_keys: extended_keys_armed(
                &hello.mux_options.get(MuxOptionKey::ExtendedKeys)?.value,
            ),
            focus_events: hello.mux_options.get(MuxOptionKey::FocusEvents)?.value == "on",
        })
    }
}

fn extended_keys_armed(value: &str) -> bool {
    !matches!(value.trim(), "" | "off")
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

thread_local! {
    static ACTIVE_OUTPUT: std::cell::RefCell<Option<std::rc::Rc<std::cell::RefCell<crate::writer::TerminalWriter>>>> = const { std::cell::RefCell::new(None) };
}

impl TerminalGuard {
    pub fn writer(&self) -> std::rc::Rc<std::cell::RefCell<crate::writer::TerminalWriter>> {
        std::rc::Rc::clone(&self.writer)
    }

    #[cfg(unix)]
    pub fn enter(mouse: MouseArming, extended_keys: bool, focus_events: bool) -> io::Result<Self> {
        let original = rustix::termios::tcgetattr(io::stdin())?;
        let file_probe = supports_kitty_graphics()
            .then(create_probe_file)
            .transpose()?;
        let writer = std::rc::Rc::new(std::cell::RefCell::new(
            crate::writer::TerminalWriter::terminal()?,
        ));
        let mut guard = Self {
            active: false,
            pixel_mouse: supports_pixel_mouse(),
            kitty_keyboard: supports_kitty_keyboard(),
            kitty_graphics: false,
            file_probe,
            original,
            writer: std::rc::Rc::clone(&writer),
        };
        ACTIVE_OUTPUT.with(|output| *output.borrow_mut() = Some(writer));
        guard.resume(mouse, extended_keys, focus_events)?;
        Ok(guard)
    }

    #[cfg(unix)]
    pub fn resume(
        &mut self,
        mouse: MouseArming,
        extended_keys: bool,
        focus_events: bool,
    ) -> io::Result<()> {
        if self.active {
            return Ok(());
        }
        let mut raw = self.original.clone();
        raw.make_raw();
        rustix::termios::tcsetattr(io::stdin(), OptionalActions::Now, &raw)?;
        self.active = true;
        EXTENDED_KEYS_OPTION.store(extended_keys, Ordering::Relaxed);
        TERMINAL_COLOURS.store(zz_daemon::client_terminal_colour_count(), Ordering::Relaxed);
        let mut output = Vec::new();
        output.write_all(b"\x1b[?1049h\x1b[?25l")?;
        if focus_events {
            output.write_all(FOCUS_EVENTS_ENABLE)?;
        }
        output.write_all(KEYPAD_TRANSMIT)?;
        output.write_all(&mouse_mode_sequence(mouse, self.pixel_mouse))?;
        output.write_all(b"\x1b[?2004h")?;
        if self.kitty_keyboard {
            output.write_all(b"\x1b[>3u")?;
        }
        if let Some(file_probe) = &self.file_probe {
            write_kitty_probe(&mut output, file_probe)?;
        }
        output.write_all(TERMINAL_REQUESTS)?;
        output.write_all(THEME_SUBSCRIBE)?;
        output.write_all(b"\x1b[16t\x1b[2J")?;
        self.writer.borrow_mut().control(output)?;
        arm_extended_keys();
        Ok(())
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

    pub const fn kitty_probe_sent(&self) -> bool {
        self.file_probe.is_some()
    }

    pub fn probe_kitty_graphics(&mut self) -> io::Result<()> {
        if self.file_probe.is_some() {
            return Ok(());
        }
        let file_probe = create_probe_file()?;
        let mut output = Vec::new();
        write_kitty_probe(&mut output, &file_probe)?;
        output.write_all(DEVICE_ATTRIBUTES_REQUEST)?;
        self.writer.borrow_mut().control(output)?;
        self.file_probe = Some(file_probe);
        Ok(())
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

impl TerminalGuard {
    pub fn suspend(&mut self) {
        if !self.active {
            return;
        }
        self.active = false;
        cleanup_frame_slot_files();
        let mut output = Vec::new();
        let _ = output.write_all(b"\x1b[?2026l\x1b[0m\x1b]112\x07");
        if CURSOR_STYLE_SENT.swap(false, Ordering::Relaxed) {
            let _ = output.write_all(b"\x1b[2 q");
        }
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
        let _ = self.writer.borrow_mut().control(output);
        #[cfg(unix)]
        let _ = rustix::termios::tcsetattr(io::stdin(), OptionalActions::Now, &self.original);
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        self.finish_file_probe();
        self.suspend();
        ACTIVE_OUTPUT.with(|output| output.borrow_mut().take());
    }
}

fn create_probe_file() -> io::Result<PathBuf> {
    let file_probe = probe_file_path();
    remove_file_if_present(&file_probe)?;
    fs::write(&file_probe, [0_u8; 4])?;
    Ok(file_probe)
}

fn write_kitty_probe(output: &mut impl io::Write, file_probe: &std::path::Path) -> io::Result<()> {
    let encoded_probe_path = STANDARD.encode(file_probe.as_os_str().as_encoded_bytes());
    write!(
        output,
        "\x1b_Gi={PROBE_IMAGE_ID},s=1,v=1,a=q,t=d,f=24;AAAA\x1b\\\x1b_Gi={FILE_PROBE_IMAGE_ID},s=1,v=1,a=q,t=f,f=32;{encoded_probe_path}\x1b\\"
    )
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

fn supports_kitty_graphics() -> bool {
    terminal_supports(["ghostty", "kitty", "wezterm", "konsole", "zz"])
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
    fn the_hello_arms_the_terminal_from_subscribed_options() {
        let mut hello = ServerHello {
            protocol_version: zz_protocol::PROTOCOL_VERSION,
            server_id: 1,
            client_id: zz_protocol::ClientId(1),
            client_instance_id: zz_protocol::ClientInstanceId::default(),
            capabilities: Vec::new(),
            appearance: zz_terminal::TerminalAppearance::default(),
            appearance_provenance: zz_terminal::AppearanceProvenance::default(),
            mux_options: zz_protocol::MuxOptions::default(),
            status: zz_protocol::StatusLine::default(),
            key_tables: Vec::new(),
        };
        assert_eq!(
            TerminalOptions::from_hello(&hello),
            Some(TerminalOptions::default())
        );
        hello.mux_options.set(
            MuxOptionKey::ExtendedKeys,
            "always",
            zz_protocol::MuxOptionSource::RuntimeCommand,
        );
        hello.mux_options.set(
            MuxOptionKey::FocusEvents,
            "on",
            zz_protocol::MuxOptionSource::RuntimeCommand,
        );
        assert_eq!(
            TerminalOptions::from_hello(&hello),
            Some(TerminalOptions {
                extended_keys: true,
                focus_events: true,
            })
        );
    }

    #[test]
    fn pixel_geometry_uses_ioctl_values_or_documented_fallbacks() {
        assert_eq!(zz_daemon::cell_pixel_extent(1600, 200, 8), 8);
        assert_eq!(zz_daemon::cell_pixel_extent(0, 200, 8), 8);
        assert_eq!(zz_daemon::cell_pixel_extent(40, 80, 8), 1);
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
    fn a_negotiated_extkeys_feature_writes_the_extended_key_request() {
        let written = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = std::sync::Arc::clone(&written);
        let writer = crate::writer::TerminalWriter::with_sink(Box::new(move |bytes| {
            sink.lock().unwrap().extend_from_slice(bytes);
            Ok(())
        }));
        ACTIVE_OUTPUT.with(|output| {
            *output.borrow_mut() = Some(std::rc::Rc::new(std::cell::RefCell::new(writer)));
        });
        EXTENDED_KEYS_OPTION.store(true, Ordering::Relaxed);
        adopt_negotiated_features(&["extkeys".to_owned()]);
        let armed = EXTENDED_KEYS_ARMED.swap(false, Ordering::Relaxed);
        EXTENDED_KEYS_OPTION.store(false, Ordering::Relaxed);
        ACTIVE_OUTPUT.with(|output| output.borrow_mut().take());
        assert!(armed);
        assert_eq!(written.lock().unwrap().as_slice(), EXTENDED_KEYS_ENABLE);
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
