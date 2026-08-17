//! Icons: the glyph set, and the element that draws one.
//!
//! The artwork in `assets/icons/` is [Iconoir](https://iconoir.com) regular
//! (MIT, `assets/icons/LICENSE-ICONOIR`), plus the `openai` and `claude` brand
//! marks from [Simple Icons](https://simpleicons.org) (CC0-1.0) and the four
//! hand-drawn `window-*` control glyphs. Iconoir draws only the left panel, so
//! `panel-right` and `panel-bottom` are `sidebar-collapse` with a mirror and a
//! rotation baked into the file — lossless, because that glyph's rect is a
//! true square, so the stroke stays 1.5px on every edge.

mod assets;

pub use assets::Assets;

use gpui::{
    AnyElement, App, Hsla, IntoElement, RenderOnce, SharedString, StyleRefinement, Styled, Svg,
    Transformation, Window, prelude::FluentBuilder as _, svg,
};

use crate::{Sizable, Size};

/// A single SVG glyph, sized and tinted like the text around it.
///
/// Size falls back in ascending priority: ambient font size, an explicit
/// width/height through [`Styled`], then [`Sizable::with_size`].
#[derive(Clone, Default, IntoElement)]
pub struct Icon {
    path: SharedString,
    style: StyleRefinement,
    text_color: Option<Hsla>,
    size: Option<Size>,
    transformation: Option<Transformation>,
}

impl Icon {
    #[must_use]
    pub fn new(icon: impl Into<Self>) -> Self {
        icon.into()
    }

    /// An icon-shaped blank. Keeps rows aligned where one item carries a glyph
    /// and its neighbour does not.
    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }

    /// Point the icon at an arbitrary asset path, such as `icons/globe.svg`.
    /// Resolved through the [`gpui::AssetSource`] the app was built with.
    #[must_use]
    pub fn path(mut self, path: impl Into<SharedString>) -> Self {
        self.path = path.into();
        self
    }

    /// Apply an SVG transformation: rotation, scale, translation. Paint-only,
    /// so layout and hit testing do not move.
    #[must_use]
    pub fn transform(mut self, transformation: Transformation) -> Self {
        self.transformation = Some(transformation);
        self
    }
}

impl Styled for Icon {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }

    fn text_color(mut self, color: impl Into<Hsla>) -> Self {
        self.text_color = Some(color.into());
        self
    }
}

impl Sizable for Icon {
    fn with_size(mut self, size: impl Into<Size>) -> Self {
        self.size = Some(size.into());
        self
    }
}

impl RenderOnce for Icon {
    fn render(self, window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let color = self.text_color.unwrap_or_else(|| window.text_style().color);
        let ambient = window.text_style().font_size.to_pixels(window.rem_size());
        let styled_size = self.style.size.width.is_some() || self.style.size.height.is_some();

        let mut base = svg();
        *base.style() = self.style;

        base.flex_shrink_0()
            .text_color(color)
            .when(!styled_size, |this| this.size(ambient))
            .when_some(self.size, |this, size| match size {
                Size::Size(size) => this.size(size),
                Size::XSmall => this.size_3(),
                Size::Small => this.size_3p5(),
                Size::Medium => this.size_4(),
                Size::Large => this.size_6(),
            })
            .when_some(self.transformation, Svg::with_transformation)
            .path(self.path)
    }
}

impl From<Icon> for AnyElement {
    fn from(icon: Icon) -> Self {
        icon.into_any_element()
    }
}

/// Every icon shipped in `assets/icons/`, one variant per file.
#[derive(Clone, Debug, PartialEq, Eq, Hash, IntoElement)]
pub enum IconName {
    ArrowDown,
    ArrowLeft,
    ArrowRight,
    ArrowUp,
    Asterisk,
    AppWindow,
    Bell,
    Bot,
    CaseSensitive,
    ChatPlus,
    Check,
    ChevronDown,
    ChevronRight,
    ChevronUp,
    CircleCheck,
    CircleUser,
    CircleX,
    Claude,
    Close,
    Copy,
    Cpu,
    Delete,
    Ellipsis,
    EllipsisVertical,
    ExternalLink,
    File,
    Folder,
    GalleryVerticalEnd,
    GitBranch,
    Globe,
    HardDrive,
    History,
    Inbox,
    Info,
    Inspector,
    Layers,
    LayoutDashboard,
    Loader,
    Minus,
    Moon,
    Openai,
    Palette,
    PanelBottom,
    PanelLeft,
    PanelRight,
    PanelsTopLeft,
    Plus,
    Redo2,
    Search,
    Settings,
    SquareTerminal,
    Star,
    Sun,
    TriangleAlert,
    Undo2,
    User,
    WindowClose,
    WindowMaximize,
    WindowMinimize,
    WindowRestore,
}

impl IconName {
    /// Every variant, in declaration order.
    pub const ALL: &[Self] = &[
        Self::ArrowDown,
        Self::ArrowLeft,
        Self::ArrowRight,
        Self::ArrowUp,
        Self::Asterisk,
        Self::AppWindow,
        Self::Bell,
        Self::Bot,
        Self::CaseSensitive,
        Self::ChatPlus,
        Self::Check,
        Self::ChevronDown,
        Self::ChevronRight,
        Self::ChevronUp,
        Self::CircleCheck,
        Self::CircleUser,
        Self::CircleX,
        Self::Claude,
        Self::Close,
        Self::Copy,
        Self::Cpu,
        Self::Delete,
        Self::Ellipsis,
        Self::EllipsisVertical,
        Self::ExternalLink,
        Self::File,
        Self::Folder,
        Self::GalleryVerticalEnd,
        Self::GitBranch,
        Self::Globe,
        Self::HardDrive,
        Self::History,
        Self::Inbox,
        Self::Info,
        Self::Inspector,
        Self::Layers,
        Self::LayoutDashboard,
        Self::Loader,
        Self::Minus,
        Self::Moon,
        Self::Openai,
        Self::Palette,
        Self::PanelBottom,
        Self::PanelLeft,
        Self::PanelRight,
        Self::PanelsTopLeft,
        Self::Plus,
        Self::Redo2,
        Self::Search,
        Self::Settings,
        Self::SquareTerminal,
        Self::Star,
        Self::Sun,
        Self::TriangleAlert,
        Self::Undo2,
        Self::User,
        Self::WindowClose,
        Self::WindowMaximize,
        Self::WindowMinimize,
        Self::WindowRestore,
    ];

    /// The icon's asset path, such as `icons/arrow-down.svg`. Resolved through
    /// the [`gpui::AssetSource`] the app was built with, [`Assets`] by default.
    #[must_use]
    pub const fn path(&self) -> &'static str {
        match self {
            Self::ArrowDown => "icons/arrow-down.svg",
            Self::ArrowLeft => "icons/arrow-left.svg",
            Self::ArrowRight => "icons/arrow-right.svg",
            Self::ArrowUp => "icons/arrow-up.svg",
            Self::Asterisk => "icons/asterisk.svg",
            Self::AppWindow => "icons/app-window.svg",
            Self::Bell => "icons/bell.svg",
            Self::Bot => "icons/bot.svg",
            Self::CaseSensitive => "icons/case-sensitive.svg",
            Self::ChatPlus => "icons/chat-plus.svg",
            Self::Check => "icons/check.svg",
            Self::ChevronDown => "icons/chevron-down.svg",
            Self::ChevronRight => "icons/chevron-right.svg",
            Self::ChevronUp => "icons/chevron-up.svg",
            Self::CircleCheck => "icons/circle-check.svg",
            Self::CircleUser => "icons/circle-user.svg",
            Self::CircleX => "icons/circle-x.svg",
            Self::Claude => "icons/claude.svg",
            Self::Close => "icons/close.svg",
            Self::Copy => "icons/copy.svg",
            Self::Cpu => "icons/cpu.svg",
            Self::Delete => "icons/delete.svg",
            Self::Ellipsis => "icons/ellipsis.svg",
            Self::EllipsisVertical => "icons/ellipsis-vertical.svg",
            Self::ExternalLink => "icons/external-link.svg",
            Self::File => "icons/file.svg",
            Self::Folder => "icons/folder.svg",
            Self::GalleryVerticalEnd => "icons/gallery-vertical-end.svg",
            Self::GitBranch => "icons/git-branch.svg",
            Self::Globe => "icons/globe.svg",
            Self::HardDrive => "icons/hard-drive.svg",
            Self::History => "icons/history.svg",
            Self::Inbox => "icons/inbox.svg",
            Self::Info => "icons/info.svg",
            Self::Inspector => "icons/inspector.svg",
            Self::Layers => "icons/layers.svg",
            Self::LayoutDashboard => "icons/layout-dashboard.svg",
            Self::Loader => "icons/loader.svg",
            Self::Minus => "icons/minus.svg",
            Self::Moon => "icons/moon.svg",
            Self::Openai => "icons/openai.svg",
            Self::Palette => "icons/palette.svg",
            Self::PanelBottom => "icons/panel-bottom.svg",
            Self::PanelLeft => "icons/panel-left.svg",
            Self::PanelRight => "icons/panel-right.svg",
            Self::PanelsTopLeft => "icons/panels-top-left.svg",
            Self::Plus => "icons/plus.svg",
            Self::Redo2 => "icons/redo-2.svg",
            Self::Search => "icons/search.svg",
            Self::Settings => "icons/settings.svg",
            Self::SquareTerminal => "icons/square-terminal.svg",
            Self::Star => "icons/star.svg",
            Self::Sun => "icons/sun.svg",
            Self::TriangleAlert => "icons/triangle-alert.svg",
            Self::Undo2 => "icons/undo-2.svg",
            Self::User => "icons/user.svg",
            Self::WindowClose => "icons/window-close.svg",
            Self::WindowMaximize => "icons/window-maximize.svg",
            Self::WindowMinimize => "icons/window-minimize.svg",
            Self::WindowRestore => "icons/window-restore.svg",
        }
    }
}

impl From<IconName> for Icon {
    fn from(name: IconName) -> Self {
        Self::default().path(name.path())
    }
}

impl From<IconName> for AnyElement {
    fn from(name: IconName) -> Self {
        Icon::from(name).into_any_element()
    }
}

impl RenderOnce for IconName {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        Icon::from(self)
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::HashSet, path::PathBuf};

    use gpui::AssetSource as _;

    use super::{Assets, IconName};

    fn icons_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/icons")
    }

    #[test]
    fn every_variant_loads_from_the_asset_source() {
        for icon in IconName::ALL {
            let path = icon.path();
            let loaded = Assets
                .load(path)
                .unwrap_or_else(|error| panic!("{icon:?} ({path}) failed to load: {error}"));
            assert!(loaded.is_some(), "{icon:?} ({path}) is not in assets/icons");
        }
    }

    #[test]
    fn every_shipped_file_has_a_variant() {
        let named: HashSet<&str> = IconName::ALL.iter().map(IconName::path).collect();

        for entry in std::fs::read_dir(icons_dir()).expect("assets/icons is readable") {
            let file_name = entry.expect("readable directory entry").file_name();
            let file_name = file_name.to_string_lossy();
            if !file_name.ends_with(".svg") {
                continue;
            }

            assert!(
                named.contains(format!("icons/{file_name}").as_str()),
                "assets/icons/{file_name} has no IconName variant"
            );
        }
    }
}
