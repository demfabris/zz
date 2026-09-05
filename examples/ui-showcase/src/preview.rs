mod settings;

use std::{rc::Rc, sync::Arc};

use gpui::{
    AnyElement, App, Context, Corners, Entity, Global, IntoElement, ListAlignment,
    ListSizingBehavior, ListState, ParentElement as _, Render, SharedString,
    UniformListScrollHandle, Window, div, prelude::*, px, uniform_list,
};
use serde::{Deserialize, Serialize};
use zz_ui::{
    ActiveTheme as _, Colorize as _, Disableable as _, Icon, IconName, Root, Sizable as _,
    StyledExt as _, UiZoom,
    agent::{
        AgentEntry, AgentTimeline, AgentTimelineStore, TimelineRow, agent_pane_header,
        composer::{AgentComposer, composer_tail_clearance},
        fold_timeline_rows,
    },
    browser::{
        BrowserEmptyHint, BrowserTabInfo, BrowserTabStrip, BrowserToolbar, browser_toolbar_button,
    },
    button::{Button, ButtonVariants as _},
    h_flex,
    input::InputState,
    navigation::{
        WORKSPACE_CONTROL_TRAFFIC_LIGHT_INSET, WORKSPACE_SIDEBAR_DEFAULT_WIDTH,
        WorkspaceStatusWindowState, workspace_chrome_controls, workspace_chrome_controls_width,
        workspace_layout_button, workspace_settings_button, workspace_sidebar_surface,
        workspace_sidebar_titlebar_with_inset, workspace_status_item, workspace_status_window,
        workspace_tree_action_button, workspace_tree_action_row, workspace_tree_disclosure,
        workspace_tree_marker, workspace_tree_row,
    },
    pane::{PaneChrome, PaneSplitAxis, pane_border_color, pane_split_surface, pane_surface},
    scroll::ScrollableElement as _,
    settings::{SettingsSection, settings_navigation_button, settings_navigation_group_label},
    shell::{
        WorkspaceStatusPlacement, WorkspaceStatusSlots, app_shell_surface, app_workspace_surface,
        workspace_status_bar,
    },
};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[allow(clippy::struct_excessive_bools)]
#[serde(default)]
pub(crate) struct PreviewOptions {
    pub scene: String,
    pub width: f32,
    pub height: f32,
    pub dark: bool,
    pub zoom: f32,
    pub sidebar: bool,
    pub gaps: bool,
    pub blur: bool,
    pub macos: bool,
    pub radius: f32,
    pub shadow_strength: f32,
    pub pane_margin: f32,
    pub pane_radius: f32,
    pub pane_border: f32,
    pub inactive_opacity: f32,
    pub settings_section: String,
    pub chrome_colors: [Option<String>; 6],
    pub ui_font: String,
    pub mono_font: String,
}

impl Default for PreviewOptions {
    fn default() -> Self {
        Self {
            scene: "workspace".into(),
            width: 1200.0,
            height: 760.0,
            dark: true,
            zoom: 1.0,
            sidebar: true,
            gaps: false,
            blur: false,
            macos: cfg!(target_os = "macos"),
            radius: 6.0,
            shadow_strength: 1.0,
            pane_margin: 6.0,
            pane_radius: 13.5,
            pane_border: 0.5,
            inactive_opacity: 0.7,
            settings_section: "appearance".into(),
            chrome_colors: Default::default(),
            ui_font: String::new(),
            mono_font: String::new(),
        }
    }
}

impl Global for PreviewOptions {}

impl PreviewOptions {
    pub fn normalize(&mut self) {
        for (value, minimum, maximum, fallback) in [
            (&mut self.width, 360.0, 3840.0, 1200.0),
            (&mut self.height, 300.0, 2160.0, 760.0),
        ] {
            *value = if value.is_finite() {
                value.clamp(minimum, maximum)
            } else {
                fallback
            };
        }
        self.zoom = if self.zoom.is_finite() {
            self.zoom.clamp(0.5, 3.0)
        } else {
            1.0
        };
        self.radius = if self.radius.is_finite() {
            self.radius.clamp(0.0, 24.0)
        } else {
            6.0
        };
        for (value, maximum, fallback) in [
            (&mut self.pane_margin, 32.0, 6.0),
            (&mut self.pane_radius, 32.0, 13.5),
            (&mut self.pane_border, 8.0, 0.5),
            (&mut self.inactive_opacity, 1.0, 0.7),
            (&mut self.shadow_strength, 1.0, 1.0),
        ] {
            *value = if value.is_finite() {
                value.clamp(0.0, maximum)
            } else {
                fallback
            };
        }
        if !["workspace", "browser", "agent", "settings", "catalog"].contains(&self.scene.as_str())
        {
            self.scene = "workspace".into();
        }
    }

    fn apply_colors(&self, cx: &mut App) {
        let mode = zz_ui::Theme::global(cx).mode;
        let base = zz_ui::ThemeColor::for_mode(mode);
        let defaults = [
            base.background,
            base.foreground,
            base.border,
            base.success,
            base.warning,
            base.danger,
        ];
        let colors = &mut zz_ui::Theme::global_mut(cx).colors;
        for (index, color) in [
            &mut colors.background,
            &mut colors.foreground,
            &mut colors.border,
            &mut colors.success,
            &mut colors.warning,
            &mut colors.danger,
        ]
        .into_iter()
        .enumerate()
        {
            *color = self.chrome_colors[index]
                .as_deref()
                .and_then(|color| zz_ui::parse_hex(color).ok())
                .unwrap_or(defaults[index]);
        }
    }

    pub(super) fn remember(&self) {
        let _ = self;
        #[cfg(target_family = "wasm")]
        if let Some(storage) = web_sys::window().and_then(|w| w.local_storage().ok().flatten()) {
            if let Ok(json) = serde_json::to_string(self) {
                let _ = storage.set_item("zz-preview", &json);
            }
        }
    }
}

pub(crate) struct Preview {
    options: PreviewOptions,
    address: Entity<InputState>,
    tabs: Vec<BrowserTabInfo>,
    active_tab: usize,
    input: Entity<InputState>,
    timeline: Entity<AgentTimelineStore>,
    rows: Arc<Vec<TimelineRow>>,
    scroll: ListState,
    settings: SettingsSection,
    selected: usize,
    collapsed: bool,
    tree_scroll: UniformListScrollHandle,
    settings_state: settings::SettingsFixture,
}

impl Preview {
    pub fn new(options: PreviewOptions, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let entries = vec![
            AgentEntry::User { id: 1, markdown: "Make the workspace feel a little more spacious. Keep the sidebar compact.".into(), images: Arc::from([]) },
            AgentEntry::Assistant { id: 2, markdown: "I'll adjust the pane spacing and check the sidebar at the same window size.\n\nThe browser preview uses the **same GPUI components** as the desktop app. Changes to their spacing, typography and colors reach both.\n\n```rust\npane_surface(id, content, overlays, chrome, cx)\n```".into() },
            AgentEntry::Assistant { id: 3, markdown: "The workspace is ready to compare. Open Settings to inspect the controls, or switch between the browser and agent scenes.".into() },
        ];
        let folded = fold_timeline_rows(&entries);
        let rows: Arc<Vec<TimelineRow>> = folded.rows;
        let scroll = ListState::new(rows.len(), ListAlignment::Top, px(300.0));
        Self {
            tabs: vec![
                BrowserTabInfo::new(1, "zzmux.sh", "https://zzmux.sh"),
                BrowserTabInfo::new(2, "GitHub", "https://github.com/demfabris/zz"),
            ],
            active_tab: 0,
            address: cx.new(|cx| InputState::new(window, cx).default_value("https://zzmux.sh")),
            input: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder("Ask anything…")
                    .auto_grow(2, 8)
            }),
            timeline: cx.new(|_| AgentTimelineStore::default()),
            settings: if options.settings_section == "panes" {
                SettingsSection::Panes
            } else {
                SettingsSection::Appearance
            },
            rows,
            scroll,
            options,
            selected: 3,
            collapsed: false,
            tree_scroll: UniformListScrollHandle::new(),
            settings_state: settings::SettingsFixture::new(window, cx),
        }
    }

    fn remember(&self, cx: &mut App) {
        self.options.remember();
        cx.set_global(self.options.clone());
    }

    fn chrome_background(&self, cx: &App) -> gpui::Hsla {
        zz_ui::shell::chrome_background(cx.theme().background, self.options.blur)
    }

    fn scene(&mut self, scene: &str, cx: &mut Context<Self>) {
        self.options.scene = scene.into();
        self.remember(cx);
        cx.notify();
    }

    fn inset(&self, cx: &App) -> gpui::Pixels {
        if self.options.macos {
            UiZoom::unzoomed(px(WORKSPACE_CONTROL_TRAFFIC_LIGHT_INSET), cx)
        } else {
            px(8.0)
        }
    }

    fn controls(cx: &mut Context<Self>) -> AnyElement {
        workspace_chrome_controls(
            workspace_settings_button("workspace-settings")
                .on_click(cx.listener(|this, _, _, cx| this.scene("settings", cx))),
            Some(
                workspace_layout_button("workspace-layout")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.options.sidebar = !this.options.sidebar;
                        this.remember(cx);
                        cx.notify();
                    }))
                    .into_any_element(),
            ),
        )
        .into_any_element()
    }

    fn sidebar(&self, cx: &mut Context<Self>) -> AnyElement {
        let titlebar = workspace_sidebar_titlebar_with_inset(
            "workspace-titlebar",
            if self.options.scene == "settings" {
                div().into_any_element()
            } else {
                Self::controls(cx)
            },
            self.inset(cx),
        );
        let navigation = if self.options.scene == "settings" {
            self.settings_navigation(cx)
        } else {
            self.tree(cx)
        };
        workspace_sidebar_surface(
            "workspace-sidebar",
            WORKSPACE_SIDEBAR_DEFAULT_WIDTH,
            titlebar,
            navigation,
            cx,
        )
        .bg(self.chrome_background(cx))
        .when(self.options.gaps, |sidebar| {
            sidebar.border_color(cx.theme().transparent)
        })
        .into_any_element()
    }

    fn tree(&self, cx: &mut Context<Self>) -> AnyElement {
        let depths: Rc<[usize]> = if self.collapsed {
            vec![0, 0]
        } else {
            vec![0, 1, 2, 3, 3, 3, 2, 0]
        }
        .into();
        let color = cx.theme().foreground.muted();
        let guides = zz_ui::navigation::tree::WorkspaceIndentGuides::new(
            depths.clone(),
            Some(self.selected),
            px(20.0),
            px(17.0),
            px(4.0),
            zz_ui::navigation::tree::IndentGuideColors {
                default: color.wash(),
                active: color,
            },
        );
        let rows = uniform_list(
            "workspace-tree-rows",
            depths.len(),
            cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                range
                    .map(|index| {
                        if index == if this.collapsed { 1 } else { 7 } {
                            workspace_tree_action_row(
                                "preview-add-host",
                                0,
                                IconName::Plus,
                                "Add host",
                                cx,
                            )
                            .into_any_element()
                        } else {
                            this.tree_row(index, cx)
                        }
                    })
                    .collect::<Vec<_>>()
            }),
        )
        .size_full()
        .with_sizing_behavior(ListSizingBehavior::Auto)
        .track_scroll(&self.tree_scroll)
        .with_decoration(guides);
        div()
            .id("workspace-tree")
            .size_full()
            .min_h_0()
            .child(rows)
            .vertical_scrollbar(&self.tree_scroll)
            .into_any_element()
    }

    fn tree_row(&self, index: usize, cx: &mut Context<Self>) -> AnyElement {
        let nodes = [
            (0, "MacBook Pro", IconName::Cpu),
            (1, "zz", IconName::Layers),
            (2, "workspace", IconName::AppWindow),
            (3, "~/dev/zz", IconName::SquareTerminal),
            (3, "zzmux.sh", IconName::Globe),
            (3, "UI iteration", IconName::Bot),
            (2, "notes", IconName::AppWindow),
        ];
        let (depth, label, icon) = nodes[index].clone();
        let active = index <= 2 || index == self.selected;
        let foreground = if active {
            cx.theme().foreground
        } else {
            cx.theme().foreground.muted()
        };
        let glyph = if index == 0 {
            gpui::img(settings::sidebar_logo())
                .size(zz_ui::rems_from_px(14.0))
                .into_any_element()
        } else {
            Icon::new(icon)
                .size(zz_ui::rems_from_px(14.0))
                .text_color(foreground)
                .into_any_element()
        };
        let marker = workspace_tree_marker(glyph);
        let group: SharedString = format!("preview-tree-{index}").into();
        let marker = if depth < 3 {
            workspace_tree_disclosure(
                format!("preview-disclosure-{index}"),
                marker,
                !self.collapsed,
                group.clone(),
                cx,
            )
            .on_click(cx.listener(|this, _, _, cx| {
                this.collapsed = !this.collapsed;
                cx.stop_propagation();
                cx.notify();
            }))
            .into_any_element()
        } else {
            marker.into_any_element()
        };
        let actions: &[IconName] = match depth {
            0 => &[IconName::Plus],
            1 => &[IconName::Plus, IconName::Xmark],
            2 => &[IconName::LayoutColumns, IconName::Xmark],
            _ => &[IconName::Xmark],
        };
        let actions =
            h_flex()
                .h_full()
                .flex_none()
                .pr(px(4.0))
                .children(actions.iter().enumerate().map(|(i, icon)| {
                    workspace_tree_action_button(
                        format!("preview-action-{index}-{i}"),
                        icon.clone(),
                        if i == 0 && depth < 3 {
                            "New pane"
                        } else {
                            "Close"
                        },
                        false,
                        cx,
                    )
                }));
        workspace_tree_row(
            format!("preview-node-{index}"),
            depth,
            index == self.selected,
            false,
            false,
            true,
            true,
            true,
            group,
            marker,
            div().flex().min_w_0().items_baseline().gap_2().child(
                div()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .text_color(foreground)
                    .child(label),
            ),
            actions,
            cx,
        )
        .on_click(cx.listener(move |this, _, _, cx| {
            this.selected = index;
            this.scene(
                match index {
                    4 => "browser",
                    5 => "agent",
                    _ => "workspace",
                },
                cx,
            );
        }))
        .into_any_element()
    }

    fn settings_navigation(&self, cx: &mut Context<Self>) -> AnyElement {
        let mut items = vec![
            Button::new("settings-back")
                .w_full()
                .px(px(8.0))
                .small()
                .ghost()
                .icon(IconName::ArrowLeft)
                .label("Back")
                .child(div().flex_1())
                .on_click(cx.listener(|this, _, _, cx| this.scene("workspace", cx)))
                .into_any_element(),
        ];
        let mut group = None;
        for section in SettingsSection::ALL {
            if group != Some(section.navigation_group()) {
                group = Some(section.navigation_group());
                items.push(
                    settings_navigation_group_label(section.navigation_group(), cx)
                        .into_any_element(),
                );
            }
            items.push(
                settings_navigation_button(section, section == self.settings, cx)
                    .disabled(!matches!(
                        section,
                        SettingsSection::Appearance | SettingsSection::Panes
                    ))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.settings = section;
                        this.options.settings_section = if section == SettingsSection::Panes {
                            "panes"
                        } else {
                            "appearance"
                        }
                        .into();
                        this.remember(cx);
                        cx.notify();
                    }))
                    .into_any_element(),
            );
        }
        div()
            .flex()
            .flex_col()
            .w_full()
            .gap(px(2.0))
            .px(px(6.0))
            .pt(px(6.0))
            .children(items)
            .into_any_element()
    }

    fn status(&self, top: bool, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let windows = ["workspace", "notes"]
            .into_iter()
            .enumerate()
            .map(|(i, name)| {
                workspace_status_window(
                    format!("preview-window-{i}"),
                    format!("{i}").into(),
                    name.into(),
                    name.into(),
                    WorkspaceStatusWindowState {
                        connected: true,
                        active: i == 0,
                        agent: i == 0,
                        ..Default::default()
                    },
                    cx,
                )
                .into_any_element()
            })
            .collect();
        let session = workspace_status_item(
            "gui-status-session",
            Some(IconName::SquareTerminal),
            "zz".into(),
            cx,
        )
        .flex_none()
        .px(px(8.0))
        .rounded(cx.theme().radius)
        .bg(zz_ui::navigation::workspace_row_highlight(cx))
        .when(cx.theme().shadow, |item| {
            item.border(px(0.5)).control_highlight(cx)
        })
        .text_color(cx.theme().foreground)
        .into_any_element();
        let controls = top.then(|| {
            (
                Self::controls(cx),
                workspace_chrome_controls_width(true, window),
            )
        });
        workspace_status_bar(
            if top {
                WorkspaceStatusPlacement::Titlebar
            } else {
                WorkspaceStatusPlacement::Bottom
            },
            false,
            self.options.gaps,
            self.chrome_background(cx),
            self.inset(cx),
            WorkspaceStatusSlots {
                session: Some(session),
                windows,
                right: vec![
                    workspace_status_item(
                        "preview-agents",
                        Some(IconName::Bot),
                        "1 agent".into(),
                        cx,
                    )
                    .into_any_element(),
                    workspace_status_item(
                        "preview-clock",
                        Some(IconName::Clock),
                        "14:32".into(),
                        cx,
                    )
                    .into_any_element(),
                ],
                titlebar_controls: controls,
                ..Default::default()
            },
            cx,
        )
        .into_any_element()
    }

    fn pane(&self, id: &'static str, content: AnyElement, active: bool, cx: &App) -> AnyElement {
        let radius = px(if self.options.gaps {
            self.options.pane_radius
        } else {
            0.0
        });
        let chrome = PaneChrome::new(
            Corners::all(radius),
            px(if self.options.gaps {
                self.options.pane_border
            } else {
                0.0
            }),
            pane_border_color(active, cx),
            self.chrome_background(cx),
            self.options.gaps,
        )
        .dimmed(!active, self.options.inactive_opacity);
        pane_surface(id, content, [], chrome, cx).into_any_element()
    }

    fn terminal(cx: &App) -> AnyElement {
        let lines = [
            ("~/dev/zz  main", false),
            ("$ just showcase", false),
            ("   Compiling zz-ui", true),
            ("   Compiling zz-ui-showcase", true),
            ("    Finished dev profile", true),
            ("", false),
            ("  VITE  ready", true),
            ("  Local: http://localhost:3131", false),
            ("", false),
            ("$ git diff --stat", false),
            (" crates/zz-ui/src/shell.rs       |  shared chrome", false),
            (
                " examples/ui-showcase/src       |  workspace preview",
                false,
            ),
            ("", false),
            ("$ ", false),
        ];
        div()
            .flex()
            .flex_col()
            .size_full()
            .overflow_hidden()
            .p(px(8.0))
            .font_family(cx.theme().mono_font_family.clone())
            .text_size(px(13.0))
            .line_height(px(18.0))
            .bg(cx.theme().background.opaque())
            .text_color(cx.theme().foreground)
            .children(lines.into_iter().map(|(text, muted)| {
                div()
                    .flex_none()
                    .h(px(18.0))
                    .when(muted, |line| line.text_color(cx.theme().foreground.muted()))
                    .child(text)
            }))
            .into_any_element()
    }

    fn browser(&self, cx: &mut Context<Self>) -> AnyElement {
        let toolbar = BrowserToolbar::new(
            browser_toolbar_button(
                cx,
                "preview-back",
                IconName::ArrowLeft,
                "Back",
                false,
                false,
            ),
            browser_toolbar_button(
                cx,
                "preview-forward",
                IconName::ArrowRight,
                "Forward",
                true,
                false,
            ),
            browser_toolbar_button(
                cx,
                "preview-reload",
                IconName::Redo2,
                "Reload",
                false,
                false,
            ),
            BrowserTabStrip::new(&self.address, self.tabs.clone(), self.active_tab)
                .on_activate(cx.processor(|this, id, window, cx| {
                    if let Some(index) = this.tabs.iter().position(|tab| tab.id == id) {
                        this.active_tab = index;
                        let url = this.tabs[index].detail.clone();
                        this.address
                            .update(cx, |input, cx| input.set_value(url, window, cx));
                        cx.notify();
                    }
                }))
                .on_close(cx.processor(|this, id, window, cx| {
                    if this.tabs.len() > 1 {
                        let active_id = this.tabs[this.active_tab].id;
                        this.tabs.retain(|tab| tab.id != id);
                        this.active_tab = this
                            .tabs
                            .iter()
                            .position(|tab| tab.id == active_id)
                            .unwrap_or(0);
                        let url = this.tabs[this.active_tab].detail.clone();
                        this.address
                            .update(cx, |input, cx| input.set_value(url, window, cx));
                        cx.notify();
                    }
                }))
                .on_new_tab({
                    let view = cx.entity();
                    move |window, cx| {
                        view.update(cx, |this, cx| {
                            let id = this.tabs.iter().map(|tab| tab.id).max().unwrap_or(0) + 1;
                            this.tabs.push(BrowserTabInfo::new(id, "New tab", ""));
                            this.active_tab = this.tabs.len() - 1;
                            this.address
                                .update(cx, |input, cx| input.set_value("", window, cx));
                            cx.notify();
                        });
                    }
                }),
            browser_toolbar_button(
                cx,
                "preview-picker",
                IconName::Inspector,
                "Pick element",
                false,
                false,
            ),
            browser_toolbar_button(
                cx,
                "preview-browser-menu",
                IconName::Ellipsis,
                "More",
                false,
                false,
            ),
        );
        div()
            .flex()
            .flex_col()
            .size_full()
            .overflow_hidden()
            .bg(cx.theme().background.opaque())
            .child(toolbar)
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .child(zz_ui::browser::browser_start_surface(BrowserEmptyHint)),
            )
            .into_any_element()
    }

    fn agent(&self, cx: &App) -> AnyElement {
        let radius = px(if self.options.gaps {
            self.options.pane_radius
        } else {
            0.0
        });
        let composer = AgentComposer {
            input: self.input.clone(),
            action: Button::compact_icon("preview-send", IconName::ArrowUp)
                .primary()
                .rounded_full()
                .into_any_element(),
            settings: vec![
                Button::new("preview-model")
                    .ghost()
                    .xsmall()
                    .label("Default model")
                    .into_any_element(),
            ],
            usage: None,
            git: Some(
                h_flex()
                    .gap_1()
                    .child(Icon::new(IconName::GitBranch).xsmall())
                    .child("main")
                    .into_any_element(),
            ),
            directory: Button::new("preview-directory")
                .ghost()
                .xsmall()
                .icon(IconName::Folder)
                .label("zz")
                .into_any_element(),
            command_hint: None,
            prefix: Vec::new(),
            attachments: None,
            radii: Corners::all(radius),
            background: cx.theme().background.opaque(),
        };
        div()
            .relative()
            .flex()
            .flex_col()
            .size_full()
            .overflow_hidden()
            .bg(cx.theme().background.opaque())
            .child(agent_pane_header(
                Button::new("preview-agent-picker")
                    .ghost()
                    .small()
                    .icon(IconName::Openai)
                    .label("Codex"),
                h_flex()
                    .gap(px(zz_ui::CHROME_GAP))
                    .child(
                        Button::compact_icon("preview-new-thread", IconName::ChatPlus)
                            .tooltip("New conversation"),
                    )
                    .child(
                        Button::compact_icon("preview-history", IconName::History)
                            .tooltip("History"),
                    ),
                cx,
            ))
            .child(
                div()
                    .id("agent-thread-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    .child(
                        AgentTimeline::new(
                            self.rows.clone(),
                            self.scroll.clone(),
                            self.timeline.clone(),
                        )
                        .bottom_padding(composer_tail_clearance()),
                    ),
            )
            .child(composer)
            .into_any_element()
    }

    fn workspace(&self, cx: &mut Context<Self>) -> AnyElement {
        let content = match self.options.scene.as_str() {
            "browser" => self.pane("browser-pane", self.browser(cx), true, cx),
            "agent" => self.pane("agent-pane", self.agent(cx), true, cx),
            _ => {
                let terminal = self.pane("terminal-pane", Self::terminal(cx), true, cx);
                let browser = self.pane("browser-pane", self.browser(cx), false, cx);
                let agent = self.pane("agent-pane", self.agent(cx), false, cx);
                let right = self.split(
                    "preview-right",
                    PaneSplitAxis::Vertical,
                    0.42,
                    browser,
                    agent,
                    cx,
                );
                self.split(
                    "preview-main",
                    PaneSplitAxis::Horizontal,
                    0.43,
                    terminal,
                    right,
                    cx,
                )
            }
        };
        let content = if self.options.gaps {
            let margin = px(self.options.pane_margin);
            let top = if self.options.sidebar {
                margin
            } else {
                px(0.0)
            };
            div()
                .relative()
                .size_full()
                .child(
                    div()
                        .absolute()
                        .inset_0()
                        .border(margin)
                        .border_t(top)
                        .border_color(self.chrome_background(cx)),
                )
                .child(
                    div()
                        .absolute()
                        .left(margin)
                        .top(top)
                        .right(margin)
                        .bottom(margin)
                        .flex()
                        .child(content),
                )
                .into_any_element()
        } else {
            content
        };
        app_workspace_surface("workspace-content", content, [], cx).into_any_element()
    }

    fn split(
        &self,
        id: &'static str,
        axis: PaneSplitAxis,
        ratio: f32,
        first: AnyElement,
        second: AnyElement,
        cx: &App,
    ) -> AnyElement {
        pane_split_surface(
            id,
            axis,
            ratio,
            false,
            self.options.gaps,
            px(if self.options.gaps {
                self.options.pane_margin
            } else {
                0.0
            }),
            None,
            None,
            first,
            second,
            div(),
            self.chrome_background(cx),
            cx,
        )
        .into_any_element()
    }
}

impl Render for Preview {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.options.apply_colors(cx);
        window.set_rem_size(cx.theme().font_size);
        window.set_zoom(self.options.zoom);
        window.set_background_appearance(if self.options.blur {
            gpui::WindowBackgroundAppearance::Blurred
        } else {
            gpui::WindowBackgroundAppearance::Opaque
        });
        cx.set_global(UiZoom(self.options.zoom));
        let settings = self.options.scene == "settings";
        let sidebar = if settings || self.options.sidebar {
            self.sidebar(cx)
        } else {
            div().into_any_element()
        };
        let titlebar = (!settings && !self.options.sidebar).then(|| self.status(true, window, cx));
        let bottom = (!settings && self.options.sidebar).then(|| self.status(false, window, cx));
        let content = if settings {
            self.settings_page(cx)
        } else {
            self.workspace(cx)
        };
        let overlays = Root::render_dialog_layer(window, cx)
            .into_iter()
            .map(IntoElement::into_any_element)
            .chain(
                Root::render_notification_layer(window, cx)
                    .into_iter()
                    .map(IntoElement::into_any_element),
            )
            .collect::<Vec<_>>();
        app_shell_surface("app-shell", sidebar, titlebar, content, bottom, overlays)
            .text_color(cx.theme().foreground)
    }
}
