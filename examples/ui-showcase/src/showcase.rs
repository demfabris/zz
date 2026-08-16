mod stories;

use std::sync::Arc;

use gpui::{
    AnyElement, App, Context, Div, Entity, IntoElement, ListAlignment, ListState,
    ParentElement as _, Pixels, Render, Styled as _, Subscription, Window, div, prelude::*, px,
    relative,
};
use zz_ui::agent::{AgentTimelineStore, TimelineRow, fold_timeline_rows};
use zz_ui::settings::SettingsSelectItem;
use zz_ui::{
    ActiveTheme as _, BASE_UI_FONT_SIZE, Disableable as _, Icon, IconName, IndexPath, Root,
    Sizable as _, StyledExt as _, Theme, ThemeMode,
    button::{Button, ButtonVariants as _},
    code_editor::CodeEditorState,
    color_picker::ColorPickerState,
    input::{Input, InputEvent, InputState},
    list::ListItem,
    scroll::ScrollableElement as _,
    select::SelectState,
    tag::Tag,
};

use stories::ThreadFixture;
use zz_ui::Colorize as _;

const GROUPS: [&str; 3] = ["Start", "Primitives", "Compositions"];

const MIN_UI_FONT_SIZE: f32 = 11.0;
const MAX_UI_FONT_SIZE: f32 = 22.0;
const UI_FONT_SIZE_STEP: f32 = 1.0;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum StoryId {
    Overview,
    Buttons,
    TagsBadges,
    InputsSelects,
    TogglesKeys,
    Navigation,
    PanesTerminal,
    CommandsChoosers,
    Browser,
    Editor,
    Agent,
    Settings,
    Feedback,
}

impl StoryId {
    pub(super) const ALL: [Self; 13] = [
        Self::Overview,
        Self::Buttons,
        Self::TagsBadges,
        Self::InputsSelects,
        Self::TogglesKeys,
        Self::Navigation,
        Self::PanesTerminal,
        Self::CommandsChoosers,
        Self::Browser,
        Self::Editor,
        Self::Agent,
        Self::Settings,
        Self::Feedback,
    ];

    fn index(self) -> usize {
        Self::ALL
            .iter()
            .position(|story| *story == self)
            .expect("every story is listed")
    }

    const fn group(self) -> &'static str {
        match self {
            Self::Overview => "Start",
            Self::Buttons | Self::TagsBadges | Self::InputsSelects | Self::TogglesKeys => {
                "Primitives"
            }
            Self::Navigation
            | Self::PanesTerminal
            | Self::CommandsChoosers
            | Self::Browser
            | Self::Editor
            | Self::Agent
            | Self::Settings
            | Self::Feedback => "Compositions",
        }
    }

    pub(super) const fn title(self) -> &'static str {
        match self {
            Self::Overview => "Atomic UI catalog",
            Self::Buttons => "Buttons",
            Self::TagsBadges => "Tags & badges",
            Self::InputsSelects => "Inputs & selects",
            Self::TogglesKeys => "Toggles, keys & feedback atoms",
            Self::Navigation => "Navigation",
            Self::PanesTerminal => "Panes & terminal",
            Self::CommandsChoosers => "Commands & choosers",
            Self::Browser => "Browser",
            Self::Editor => "Code editor",
            Self::Agent => "Agent",
            Self::Settings => "Settings",
            Self::Feedback => "Dialogs & notifications",
        }
    }

    const fn description(self) -> &'static str {
        match self {
            Self::Overview => "Every atomic piece the app uses, each rendered on its own.",
            Self::Buttons => "Every Button variant, size, and state, plus the icon-only form.",
            Self::TagsBadges => "Tags and the status badges built from them.",
            Self::InputsSelects => "Text inputs, the bounded number input, and dropdown selects.",
            Self::TogglesKeys => "Switches, keyboard hints, spinners, and separators.",
            Self::Navigation => {
                "Host-tree rows, the sidebar controls, the titlebar strip's chips, and the tmux status section."
            }
            Self::PanesTerminal => {
                "Pane indicators, the overlays layered over a terminal grid, and the workspace's connection states."
            }
            Self::CommandsChoosers => {
                "Palette rows and hints, and the tree and buffer chooser rows."
            }
            Self::Browser => "Toolbar buttons, the address bar, recent rows, and recovery states.",
            Self::Editor => "The native rope-backed editor with Rust syntax highlighting.",
            Self::Agent => {
                "The agent pane header and its flat ACP transcript rows."
            }
            Self::Settings => "Navigation buttons, setting cards, rows, badges, and reset actions.",
            Self::Feedback => {
                "The shared confirmation dialogs, the prompt dialogs, and the four notification tones."
            }
        }
    }

    pub(super) const fn source(self) -> &'static str {
        match self {
            Self::Overview => "crates/zz-ui/src",
            Self::Buttons => "zz_ui::button",
            Self::TagsBadges => "zz_ui::tag + pane/command/settings badges",
            Self::InputsSelects => "zz_ui::input, zz_ui::select",
            Self::TogglesKeys => "zz_ui::{switch,kbd,spinner,separator}",
            Self::Navigation => "crates/zz-ui/src/navigation.rs",
            Self::PanesTerminal => "crates/zz-ui/src/{pane,shell}.rs",
            Self::CommandsChoosers => "crates/zz-ui/src/{command,chooser}.rs",
            Self::Browser => "crates/zz-ui/src/browser.rs",
            Self::Editor => "zz_ui::code_editor",
            Self::Agent => "crates/zz-ui/src/agent.rs",
            Self::Settings => "crates/zz-ui/src/settings.rs",
            Self::Feedback => "crates/zz-ui/src/feedback.rs + Dialog, Notification",
        }
    }

    const fn keywords(self) -> &'static str {
        match self {
            Self::Overview => "inventory atomic components ui pieces storybook wasm catalog",
            Self::Buttons => "button icon ghost outline primary danger success warning loading",
            Self::TagsBadges => "tag badge pill sync fps provenance kind status",
            Self::InputsSelects => {
                "input text number select dropdown prefix borderless loading masked password"
            }
            Self::TogglesKeys => "switch toggle kbd keyboard spinner separator divider",
            Self::Navigation => {
                "sidebar tree host fleet ssh session window pane strip titlebar badge pill disclosure chevron connecting unreachable status tmux clock"
            }
            Self::PanesTerminal => {
                "pane indicator number sync unzoom search copy mode uri status connecting daemon"
            }
            Self::CommandsChoosers => "command palette completion chooser tree buffer hint",
            Self::Browser => "browser toolbar address url recent empty error picker menu",
            Self::Editor => "editor code rust rope line numbers syntax highlight text",
            Self::Agent => {
                "agent acp thread timeline tool diff reasoning plan markdown codex claude"
            }
            Self::Settings => "settings card row provenance reset select switch number nav",
            Self::Feedback => {
                "dialog alert prompt password askpass add host notification info success warning error toast"
            }
        }
    }

    const fn icon(self) -> IconName {
        match self {
            Self::Overview => IconName::LayoutDashboard,
            Self::Buttons => IconName::Inspector,
            Self::TagsBadges => IconName::Star,
            Self::InputsSelects => IconName::CaseSensitive,
            Self::TogglesKeys => IconName::Loader,
            Self::Navigation => IconName::PanelLeft,
            Self::PanesTerminal => IconName::SquareTerminal,
            Self::CommandsChoosers => IconName::GalleryVerticalEnd,
            Self::Browser => IconName::Globe,
            Self::Editor => IconName::File,
            Self::Agent => IconName::Bot,
            Self::Settings => IconName::Settings,
            Self::Feedback => IconName::Bell,
        }
    }

    fn matches(self, query: &str) -> bool {
        query.is_empty()
            || self.title().to_lowercase().contains(query)
            || self.description().to_lowercase().contains(query)
            || self.source().to_lowercase().contains(query)
            || self.keywords().contains(query)
    }
}

pub(crate) struct Showcase {
    active: StoryId,
    search: Entity<InputState>,
    browser_address: Entity<InputState>,
    browser_address_loading: Entity<InputState>,
    browser_tab_address: Entity<InputState>,
    code_editor: Entity<CodeEditorState>,
    command_input: Entity<InputState>,
    value_input: Entity<InputState>,
    host_input: Entity<InputState>,
    secret_input: Entity<InputState>,
    pane_corner_radius: Entity<InputState>,
    mux_prefix: Entity<InputState>,
    mux_history: Entity<InputState>,
    mux_mode_keys: Entity<SelectState<Vec<SettingsSelectItem>>>,
    mux_set_clipboard: Entity<SelectState<Vec<SettingsSelectItem>>>,
    chrome_background: Entity<ColorPickerState>,
    window_background_blur: bool,
    synchronize_panes: bool,
    agent_timeline_store: Entity<AgentTimelineStore>,
    agent_threads: Vec<AgentThread>,
    _subscriptions: Vec<Subscription>,
}

struct AgentThread {
    fixture: ThreadFixture,
    rows: Arc<Vec<TimelineRow>>,
    list_state: ListState,
}

impl Showcase {
    pub(crate) fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Filter zz UI…"));
        let browser_address = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Search or enter address")
                .default_value("https://gpui.rs")
        });
        let browser_address_loading = cx.new(|cx| {
            let mut state = InputState::new(window, cx).default_value("https://zed.dev");
            state.set_loading(true, window, cx);
            state
        });
        let browser_tab_address = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Search or enter address")
                .default_value("https://github.com/zz")
        });
        let command_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Type a tmux command…")
                .default_value("split-")
        });
        let code_editor = cx.new(|cx| {
            CodeEditorState::new(window, cx)
                .language("rust")
                .default_value(
                    "use std::path::Path;\n\n\
                     fn describe(path: &Path) -> &'static str {\n\
                     \tif path.is_file() { \"file\" } else { \"directory\" }\n\
                     }\n",
                )
        });
        let value_input = cx.new(|cx| InputState::new(window, cx).default_value("workspace"));
        let host_input = cx.new(|cx| InputState::new(window, cx).placeholder("user@desktop"));
        let secret_input = cx.new(|cx| InputState::new(window, cx).default_value("hunter2"));
        let pane_corner_radius = numeric_input("0", window, cx);
        let mux_prefix = cx.new(|cx| InputState::new(window, cx).default_value("C-b"));
        let mux_history = cx.new(|cx| InputState::new(window, cx).default_value("2000"));
        let mux_mode_keys = cx.new(|cx| {
            SelectState::new(
                vec![
                    SettingsSelectItem::new("Vi", "vi"),
                    SettingsSelectItem::new("Emacs", "emacs"),
                ],
                Some(IndexPath::default()),
                window,
                cx,
            )
        });
        let mux_set_clipboard = cx.new(|cx| {
            SelectState::new(
                vec![
                    SettingsSelectItem::new("On", "on"),
                    SettingsSelectItem::new("External", "external"),
                    SettingsSelectItem::new("Off", "off"),
                ],
                Some(IndexPath::default()),
                window,
                cx,
            )
        });
        let chrome_background = cx.new(|cx| ColorPickerState::new(None, window, cx));
        let search_subscription = cx.subscribe(&search, |_, _, event, cx| {
            if matches!(event, InputEvent::Change) {
                cx.notify();
            }
        });

        Self {
            active: StoryId::Overview,
            search,
            browser_address,
            browser_address_loading,
            browser_tab_address,
            code_editor,
            command_input,
            value_input,
            host_input,
            secret_input,
            pane_corner_radius,
            mux_prefix,
            mux_history,
            mux_mode_keys,
            mux_set_clipboard,
            chrome_background,
            window_background_blur: true,
            synchronize_panes: false,
            agent_timeline_store: cx.new(|_| AgentTimelineStore::default()),
            agent_threads: ThreadFixture::ALL
                .into_iter()
                .map(|fixture| {
                    let rows = fold_timeline_rows(&fixture.entries()).rows;
                    let list_state = ListState::new(rows.len(), ListAlignment::Top, px(1_200.0));
                    AgentThread {
                        fixture,
                        rows,
                        list_state,
                    }
                })
                .collect(),
            _subscriptions: vec![search_subscription],
        }
    }

    fn agent_rows(&self, fixture: ThreadFixture) -> Arc<Vec<TimelineRow>> {
        self.agent_thread(fixture).rows.clone()
    }

    fn agent_list_state(&self, fixture: ThreadFixture) -> ListState {
        self.agent_thread(fixture).list_state.clone()
    }

    fn agent_thread(&self, fixture: ThreadFixture) -> &AgentThread {
        self.agent_threads
            .iter()
            .find(|thread| thread.fixture == fixture)
            .expect("every fixture thread is built in `new`")
    }

    fn render_sidebar(&self, cx: &mut Context<Self>) -> Div {
        let query = self.search.read(cx).value().trim().to_lowercase();
        let visible_count = StoryId::ALL
            .iter()
            .filter(|story| story.matches(&query))
            .count();
        let mut group_elements = Vec::new();

        for group in GROUPS {
            let items = StoryId::ALL
                .iter()
                .copied()
                .filter(|story| story.group() == group && story.matches(&query))
                .map(|story| {
                    let active = self.active == story;
                    ListItem::new(("showcase-story", story.index()))
                        .selected(active)
                        .rounded(cx.theme().radius)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.active = story;
                            cx.notify();
                        }))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .min_w_0()
                                .text_sm()
                                .child(Icon::new(story.icon()).small())
                                .child(
                                    div()
                                        .min_w_0()
                                        .overflow_hidden()
                                        .whitespace_nowrap()
                                        .text_ellipsis()
                                        .child(story.title()),
                                ),
                        )
                        .into_any_element()
                })
                .collect::<Vec<AnyElement>>();

            if !items.is_empty() {
                group_elements.push(
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(
                            div()
                                .px_3()
                                .pt_3()
                                .pb_1()
                                .text_xs()
                                .font_medium()
                                .text_color(cx.theme().foreground.muted())
                                .child(group),
                        )
                        .children(items)
                        .into_any_element(),
                );
            }
        }

        div()
            .flex()
            .flex_col()
            .w(px(282.0))
            .h_full()
            .flex_shrink_0()
            .border_r_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().background.raised(1))
            .text_color(cx.theme().foreground)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .h(px(64.0))
                    .px_4()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_center()
                            .size_8()
                            .rounded(cx.theme().radius)
                            .bg(cx.theme().foreground)
                            .text_color(cx.theme().foreground.on())
                            .child(Icon::new(IconName::LayoutDashboard).small()),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .line_height(relative(1.2))
                            .child(div().text_sm().font_medium().child("zz UI inventory"))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().foreground.muted())
                                    .child("canonical shared components"),
                            ),
                    ),
            )
            .child(
                div()
                    .m_3()
                    .rounded(cx.theme().radius)
                    .bg(cx.theme().background.raised(2))
                    .child(
                        Input::new(&self.search)
                            .small()
                            .appearance(false)
                            .cleanable(true)
                            .prefix(Icon::new(IconName::Search).xsmall()),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scrollbar()
                    .px_2()
                    .children(group_elements)
                    .when(visible_count == 0, |this| {
                        this.child(
                            div()
                                .p_4()
                                .text_sm()
                                .text_color(cx.theme().foreground.muted())
                                .child("No matching zz UI pieces."),
                        )
                    }),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .h(px(40.0))
                    .px_4()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .text_xs()
                    .text_color(cx.theme().foreground.muted())
                    .child(format!("{visible_count} / {} pieces", StoryId::ALL.len()))
                    .child(Tag::secondary().small().outline().child("same Rust source")),
            )
    }

    fn render_toolbar(&self, cx: &mut Context<Self>) -> Div {
        let story = self.active;
        let dark = cx.theme().is_dark();

        div()
            .flex()
            .items_center()
            .justify_between()
            .min_h(px(72.0))
            .gap_4()
            .px_5()
            .py_3()
            .border_b_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().background)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .min_w_0()
                    .gap_0p5()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .text_base()
                            .font_medium()
                            .child(Icon::new(story.icon()).small())
                            .child(story.title()),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().foreground.muted())
                            .child(story.description()),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        Tag::primary()
                            .small()
                            .outline()
                            .child(story.group().to_lowercase()),
                    )
                    .child(Tag::secondary().small().outline().child(story.source()))
                    .child(
                        Button::new("showcase-theme")
                            .ghost()
                            .small()
                            .icon(if dark { IconName::Sun } else { IconName::Moon })
                            .tooltip(if dark {
                                "Use light theme"
                            } else {
                                "Use dark theme"
                            })
                            .on_click(|_, window, cx| {
                                let mode = if cx.theme().is_dark() {
                                    ThemeMode::Light
                                } else {
                                    ThemeMode::Dark
                                };
                                Theme::change(mode, Some(window), cx);
                            }),
                    )
                    .child(ui_scale_control(cx)),
            )
            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
    }

    fn render_active_story(&mut self, cx: &mut Context<Self>) -> AnyElement {
        stories::render(self, cx)
    }
}

impl Render for Showcase {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let sidebar = self.render_sidebar(cx);
        let toolbar = self.render_toolbar(cx);
        let story = self.render_active_story(cx);

        div()
            .flex()
            .size_full()
            .overflow_hidden()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(sidebar)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .child(toolbar)
                    .child(
                        div().flex_1().min_h_0().overflow_y_scrollbar().child(
                            div()
                                .w_full()
                                .max_w(px(1240.0))
                                .mx_auto()
                                .p_6()
                                .child(story),
                        ),
                    ),
            )
    }
}

pub(crate) struct ShowcaseShell {
    showcase: Entity<Showcase>,
}

impl ShowcaseShell {
    pub(crate) fn new(showcase: Entity<Showcase>) -> Self {
        Self { showcase }
    }
}

impl Render for ShowcaseShell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        window.set_rem_size(cx.theme().font_size);

        let dialog_layer = Root::render_dialog_layer(window, cx);
        let notification_layer = Root::render_notification_layer(window, cx);

        div()
            .relative()
            .size_full()
            .overflow_hidden()
            .child(self.showcase.clone())
            .children(dialog_layer)
            .children(notification_layer)
    }
}

fn ui_scale_control(cx: &App) -> Div {
    let font_size = cx.theme().font_size;
    let percent = (font_size.as_f32() / BASE_UI_FONT_SIZE * 100.0).round();
    div()
        .flex()
        .items_center()
        .child(
            Button::new("showcase-ui-smaller")
                .ghost()
                .small()
                .icon(IconName::Dash)
                .tooltip("Smaller UI")
                .disabled(font_size <= px(MIN_UI_FONT_SIZE))
                .on_click(|_, _, cx| adjust_ui_font_size(-UI_FONT_SIZE_STEP, cx)),
        )
        .child(
            Button::new("showcase-ui-reset")
                .ghost()
                .small()
                .label(format!("{percent:.0}%"))
                .tooltip("Reset UI size")
                .disabled(font_size == px(BASE_UI_FONT_SIZE))
                .on_click(|_, _, cx| {
                    set_ui_font_size(px(BASE_UI_FONT_SIZE), cx);
                }),
        )
        .child(
            Button::new("showcase-ui-larger")
                .ghost()
                .small()
                .icon(IconName::Plus)
                .tooltip("Larger UI")
                .disabled(font_size >= px(MAX_UI_FONT_SIZE))
                .on_click(|_, _, cx| adjust_ui_font_size(UI_FONT_SIZE_STEP, cx)),
        )
}

fn adjust_ui_font_size(delta: f32, cx: &mut App) {
    let next = (cx.theme().font_size.as_f32() + delta).clamp(MIN_UI_FONT_SIZE, MAX_UI_FONT_SIZE);
    set_ui_font_size(px(next), cx);
}

fn set_ui_font_size(font_size: Pixels, cx: &mut App) {
    if Theme::global(cx).font_size == font_size {
        return;
    }
    Theme::global_mut(cx).font_size = font_size;
    cx.refresh_windows();
}

fn numeric_input(
    value: &'static str,
    window: &mut Window,
    cx: &mut Context<Showcase>,
) -> Entity<InputState> {
    cx.new(|cx| {
        InputState::new(window, cx)
            .default_value(value)
            .step(1.0)
            .min(0.0)
            .max(256.0)
    })
}
