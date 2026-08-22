use std::{
    fs,
    io::{self, Write as _},
    path::PathBuf,
};

use base64::{Engine as _, engine::general_purpose::STANDARD};

#[cfg(unix)]
use rustix::termios::{OptionalActions, Termios};

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

pub(crate) const MOUSE_DISABLE_SEQUENCE: &[u8] = b"\x1b[?1016l\x1b[?1006l\x1b[?1003l";

pub(crate) fn mouse_enable_sequence(pixel_mouse: bool) -> Vec<u8> {
    let mut sequence = b"\x1b[?1003h\x1b[?1006h".to_vec();
    if pixel_mouse {
        sequence.extend_from_slice(b"\x1b[?1016h");
    }
    sequence
}

impl TerminalGuard {
    #[cfg(unix)]
    pub fn enter(mouse: bool) -> io::Result<Self> {
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
        let mut output = io::stdout().lock();
        output.write_all(b"\x1b[?1049h\x1b[?7l\x1b[?25l\x1b[?1004h")?;
        if mouse {
            output.write_all(&mouse_enable_sequence(guard.pixel_mouse))?;
        }
        output.write_all(b"\x1b[?2004h")?;
        if guard.kitty_keyboard {
            output.write_all(b"\x1b[>3u")?;
        }
        write!(
            output,
            "\x1b_Gi={PROBE_IMAGE_ID},s=1,v=1,a=q,t=d,f=24;AAAA\x1b\\\x1b_Gi={FILE_PROBE_IMAGE_ID},s=1,v=1,a=q,t=f,f=32;{encoded_probe_path}\x1b\\\x1b[c"
        )?;
        output.write_all(b"\x1b[16t\x1b[2J")?;
        output.flush()?;
        Ok(guard)
    }

    #[cfg(not(unix))]
    pub fn enter(_mouse: bool) -> io::Result<Self> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "zz-tui currently requires a Unix terminal",
        ))
    }

    pub const fn pixel_mouse(&self) -> bool {
        self.pixel_mouse
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
        let _ = output.write_all(
            b"\x1b[?2004l\x1b[?1016l\x1b[?1006l\x1b[?1003l\x1b[?1004l\x1b[?7h\x1b[?25h\x1b[?1049l",
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
        assert_eq!(mouse_enable_sequence(false), b"\x1b[?1003h\x1b[?1006h");
        assert_eq!(
            mouse_enable_sequence(true),
            b"\x1b[?1003h\x1b[?1006h\x1b[?1016h"
        );
        assert_eq!(MOUSE_DISABLE_SEQUENCE, b"\x1b[?1016l\x1b[?1006l\x1b[?1003l");
    }
}
