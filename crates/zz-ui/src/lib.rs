//! zz's application UI on [`zpui_kit`]: panes, terminal, agent, command palette,
//! settings, and navigation, shared by the desktop app and the thin clients.
//! It re-exports the kit, so zz code imports UI from one place.

pub mod chrome_palette;
pub mod tmux_style;

pub use zpui_kit::{
    button, color_picker, dialog, highlighter, icon, input, kbd, list, menu, notification, overlay,
    popover, pulse, scroll, select, separator, slider, spinner, switch, tag, text, theme,
    title_bar, tooltip, touch,
};

#[cfg(feature = "editor")]
pub use zpui_kit::code_editor;

pub use zpui_kit::{
    ActiveTheme, Assets, BASE_UI_FONT_SIZE, CHROME_GAP, Colorize, Disableable, ElementExt, Icon,
    IconName, IndexPath, InteractiveElementExt, MACOS_TRAFFIC_LIGHT_INSET,
    MACOS_TRAFFIC_LIGHT_SPAN, ROOT_KEY_CONTEXT, Root, SURFACE_RING_OUTSET, ScrollbarShow,
    Selectable, Side, Sizable, Size, StyleSized, StyledExt, TITLE_BAR_HEIGHT, Theme, ThemeColor,
    ThemeMode, TitleBar, UiZoom, WindowControls, WindowExt, control_shadow, cubic_ease,
    draws_window_controls, h_flex, init, macos_traffic_light_clearance, oklab_lightness, parse_hex,
    rems_from_px, stacked_ring, surface_ring, to_hex, v_flex, window_border, window_controls_width,
    window_paddings,
};

#[cfg(feature = "agent")]
pub mod agent;
pub mod attachment;
pub mod browser;
pub mod chooser;
pub mod command;
pub mod compact;
pub mod feedback;
pub mod mend;
pub mod navigation;
pub mod pane;
pub mod path_picker;
pub mod picker;
pub mod settings;
pub mod shell;
pub mod terminal;
pub mod terminal_images;
pub mod which_key;
