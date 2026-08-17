//! Target-agnostic presentation components and the widget layer, shared by the
//! zz desktop app and the WASM showcase.

mod widget;

pub use widget::{
    button, color_picker, dialog, highlighter, icon, input, kbd, list, menu, notification, overlay,
    popover, scroll, select, separator, spinner, switch, tag, text, theme, title_bar, tooltip,
};

#[cfg(feature = "editor")]
pub use widget::code_editor;

pub use widget::foundation::{
    ActiveTheme, BASE_UI_FONT_SIZE, CHROME_GAP, Colorize, Disableable, ElementExt, IndexPath,
    InteractiveElementExt, SURFACE_RING_OUTSET, ScrollbarShow, Selectable, Side, Sizable, Size,
    StyleSized, StyledExt, Theme, ThemeColor, ThemeMode, UiZoom, control_shadow, cubic_ease,
    h_flex, oklab_lightness, parse_hex, rems_from_px, stacked_ring, surface_ring, to_hex, v_flex,
    window_border, window_paddings,
};

pub use widget::overlay::{ROOT_KEY_CONTEXT, Root, WindowExt};

pub use widget::icon::{Icon, IconName};

pub use widget::title_bar::{
    MACOS_TRAFFIC_LIGHT_INSET, MACOS_TRAFFIC_LIGHT_SPAN, TITLE_BAR_HEIGHT, TitleBar,
    WindowControls, draws_window_controls,
};

/// The SVG icon assets backing [`IconName`], embedded from `crates/zz-ui/assets`.
/// Register with `with_assets`.
pub use widget::icon::Assets;

/// Initialize the widget layer. Must run once, before any window opens.
/// The foundation goes first: it installs the globals every widget reads.
pub fn init(cx: &mut gpui::App) {
    widget::foundation::init(cx);
    #[cfg(feature = "editor")]
    widget::code_editor::init(cx);
    widget::input::init(cx);
    widget::menu::init(cx);
    widget::overlay::init(cx);
    widget::popover::init(cx);
    widget::select::init(cx);
    widget::text::init(cx);
}

#[cfg(feature = "agent")]
pub mod agent;
pub mod attachment;
pub mod browser;
pub mod chooser;
pub mod command;
pub mod feedback;
pub mod mend;
pub mod navigation;
pub mod pane;
pub mod pulse;
pub mod settings;
pub mod shell;
