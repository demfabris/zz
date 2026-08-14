use gpui::{
    App, Context, FocusHandle, Focusable, IntoElement, KeyDownEvent, Keystroke, MouseButton,
    Render, Window, div, prelude::*, px,
};
use zz_protocol::{CommandInvocation, PaneId};
use zz_ui::{
    ActiveTheme as _, Icon, IconName, Sizable as _, kbd::Kbd, navigation::workspace_row_highlight,
};

use crate::mux::client::MuxClient;
use crate::window::corners::{WindowCorners, round_div_radii};
use crate::{browser, config};
use zz_ui::Colorize as _;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PaneChoice {
    Terminal,
    Browser,
    Editor,
    Agent,
}

const CHOICES: [PaneChoice; 4] = [
    PaneChoice::Terminal,
    PaneChoice::Browser,
    PaneChoice::Editor,
    PaneChoice::Agent,
];

fn choices(cx: &App) -> Vec<PaneChoice> {
    CHOICES
        .into_iter()
        .filter(|choice| match choice {
            PaneChoice::Terminal => true,
            PaneChoice::Browser => browser::controller::is_available(cx),
            PaneChoice::Editor => config::editor_pane_enabled(cx),
            PaneChoice::Agent => config::agent_pane_enabled(cx),
        })
        .collect()
}

impl PaneChoice {
    const fn command_argument(self) -> &'static str {
        match self {
            Self::Terminal => "terminal",
            Self::Browser => "browser",
            Self::Editor => "editor",
            Self::Agent => "agent",
        }
    }

    const fn title(self) -> &'static str {
        match self {
            Self::Terminal => "Terminal",
            Self::Browser => "Browser",
            Self::Editor => "Editor",
            Self::Agent => "Agent",
        }
    }

    const fn shortcut(self) -> &'static str {
        match self {
            Self::Terminal => "t",
            Self::Browser => "b",
            Self::Editor => "e",
            Self::Agent => "a",
        }
    }

    const fn icon(self) -> IconName {
        match self {
            Self::Terminal => IconName::SquareTerminal,
            Self::Browser => IconName::Globe,
            Self::Editor => IconName::File,
            Self::Agent => IconName::Bot,
        }
    }

    const fn element_id(self) -> &'static str {
        match self {
            Self::Terminal => "pane-picker-terminal",
            Self::Browser => "pane-picker-browser",
            Self::Editor => "pane-picker-editor",
            Self::Agent => "pane-picker-agent",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PickerAction {
    Activate(PaneChoice),
    Move(isize),
    Confirm,
    Close,
}

fn picker_action(keystroke: &Keystroke, choices: &[PaneChoice]) -> Option<PickerAction> {
    let modifiers = keystroke.modifiers;
    if modifiers.control || modifiers.alt || modifiers.platform {
        return None;
    }
    if let Some(choice) = choices
        .iter()
        .find(|choice| choice.shortcut() == keystroke.key)
    {
        return Some(PickerAction::Activate(*choice));
    }
    match keystroke.key.as_str() {
        "j" | "down" => Some(PickerAction::Move(1)),
        "k" | "up" => Some(PickerAction::Move(-1)),
        "enter" => Some(PickerAction::Confirm),
        "escape" => Some(PickerAction::Close),
        _ => None,
    }
}

fn wrapping_step(selected: usize, delta: isize, len: usize) -> usize {
    (selected + len).wrapping_add_signed(delta) % len
}

fn materialize_command(pane: PaneId, choice: PaneChoice) -> CommandInvocation {
    CommandInvocation::new(
        "select-pane-kind",
        [
            "-t".to_owned(),
            pane.to_string(),
            choice.command_argument().to_owned(),
        ],
    )
}

fn close_command(pane: PaneId) -> CommandInvocation {
    CommandInvocation::new("kill-pane", ["-t".to_owned(), pane.to_string()])
}

fn select_command(pane: PaneId) -> CommandInvocation {
    CommandInvocation::new("select-pane", ["-t".to_owned(), pane.to_string()])
}

pub(crate) struct PanePickerView {
    pane: PaneId,
    mux: gpui::Entity<MuxClient>,
    focus_handle: FocusHandle,
    selected: usize,
    window_corners: WindowCorners,
}

impl PanePickerView {
    pub(crate) fn new(pane: PaneId, mux: gpui::Entity<MuxClient>, cx: &mut Context<Self>) -> Self {
        Self {
            pane,
            mux,
            focus_handle: cx.focus_handle(),
            selected: 0,
            window_corners: WindowCorners::NONE,
        }
    }

    pub(crate) fn focus(&self) -> &FocusHandle {
        &self.focus_handle
    }

    pub(crate) fn set_window_corners(&mut self, corners: WindowCorners, cx: &mut Context<Self>) {
        if self.window_corners != corners {
            self.window_corners = corners;
            cx.notify();
        }
    }

    fn activate(&self, choice: PaneChoice, cx: &Context<Self>) {
        self.mux
            .read(cx)
            .execute(materialize_command(self.pane, choice));
    }

    fn close(&self, cx: &Context<Self>) {
        self.mux.read(cx).execute(close_command(self.pane));
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let choices = choices(cx);
        match picker_action(&event.keystroke, &choices) {
            Some(PickerAction::Activate(choice)) => self.activate(choice, cx),
            Some(PickerAction::Move(delta)) => {
                self.selected = wrapping_step(self.selected, delta, choices.len());
                cx.notify();
            }
            Some(PickerAction::Confirm) => {
                if let Some(choice) = choices.get(self.selected).copied() {
                    self.activate(choice, cx);
                }
            }
            Some(PickerAction::Close) => self.close(cx),
            None => return,
        }
        cx.stop_propagation();
    }

    fn row(&self, index: usize, choice: PaneChoice, cx: &mut Context<Self>) -> impl IntoElement {
        let selected = self.selected == index;
        let rest = cx.theme().background.washed(1);
        let highlight = workspace_row_highlight(cx);
        let view = cx.entity();
        let hover_view = view.clone();
        div()
            .id((choice.element_id(), self.pane.0))
            .flex()
            .w_full()
            .h(px(40.0))
            .flex_none()
            .items_center()
            .gap(px(10.0))
            .px(px(12.0))
            .rounded(cx.theme().radius)
            .bg(if selected { highlight } else { rest })
            .cursor_pointer()
            .on_mouse_move(move |_, _, cx| {
                hover_view.update(cx, |picker, cx| {
                    if picker.selected != index {
                        picker.selected = index;
                        cx.notify();
                    }
                });
            })
            .child(
                Icon::new(choice.icon())
                    .with_size(px(16.0))
                    .text_color(cx.theme().foreground.muted()),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .text_size(zz_ui::rems_from_px(12.0))
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .child(choice.title()),
            )
            .child(
                Kbd::new(Keystroke::parse(choice.shortcut()).expect("static pane picker shortcut"))
                    .lowercase()
                    .bg(cx.theme().background.raised(4)),
            )
            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                view.update(cx, |picker, cx| picker.activate(choice, cx));
                cx.stop_propagation();
            })
    }
}

impl Focusable for PanePickerView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for PanePickerView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let focus_handle = self.focus_handle.clone();
        let select_mux = self.mux.clone();
        let pane = self.pane;
        let choices = choices(cx);
        self.selected = self.selected.min(choices.len() - 1);
        let rows = choices
            .iter()
            .enumerate()
            .map(|(index, choice)| self.row(index, *choice, cx).into_any_element())
            .collect::<Vec<_>>();
        round_div_radii(
            div()
                .id(("pane-picker", self.pane.0))
                .track_focus(&self.focus_handle)
                .on_key_down(cx.listener(Self::on_key_down))
                .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                    select_mux.read(cx).execute(select_command(pane));
                    focus_handle.focus(window, cx);
                })
                .flex()
                .size_full()
                .items_center()
                .justify_center()
                .overflow_hidden()
                .bg(crate::theme::app_pane_background(cx))
                .text_color(cx.theme().foreground)
                .px(px(12.0))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .w_full()
                        .max_w(px(360.0))
                        .gap(px(4.0))
                        .children(rows),
                ),
            config::pane_content_radii(cx, self.window_corners),
        )
    }
}

#[cfg(test)]
mod tests {
    use gpui::Modifiers;

    use super::*;

    fn key(value: &str) -> Keystroke {
        Keystroke {
            modifiers: Modifiers::default(),
            key: value.to_owned(),
            key_char: Some(value.to_owned()),
        }
    }

    #[test]
    fn direct_shortcuts_activate_each_choice_and_escape_closes() {
        assert_eq!(
            CHOICES,
            [
                PaneChoice::Terminal,
                PaneChoice::Browser,
                PaneChoice::Editor,
                PaneChoice::Agent,
            ],
            "the ungated picker catalog contains all four pane kinds"
        );
        assert_eq!(
            picker_action(&key("t"), &CHOICES),
            Some(PickerAction::Activate(PaneChoice::Terminal))
        );
        assert_eq!(
            picker_action(&key("b"), &CHOICES),
            Some(PickerAction::Activate(PaneChoice::Browser))
        );
        assert_eq!(
            picker_action(&key("e"), &CHOICES),
            Some(PickerAction::Activate(PaneChoice::Editor))
        );
        assert_eq!(
            picker_action(&key("a"), &CHOICES),
            Some(PickerAction::Activate(PaneChoice::Agent))
        );
        assert_eq!(
            picker_action(&key("escape"), &CHOICES),
            Some(PickerAction::Close)
        );
    }

    #[test]
    fn gated_out_choices_take_their_hotkeys_with_them() {
        let agent_gated = [
            PaneChoice::Terminal,
            PaneChoice::Browser,
            PaneChoice::Editor,
        ];
        assert_eq!(picker_action(&key("a"), &agent_gated), None);
        assert_eq!(
            picker_action(&key("e"), &agent_gated),
            Some(PickerAction::Activate(PaneChoice::Editor))
        );
        let both_gated = [PaneChoice::Terminal, PaneChoice::Browser];
        assert_eq!(picker_action(&key("e"), &both_gated), None);
        assert_eq!(picker_action(&key("a"), &both_gated), None);
        assert_eq!(
            picker_action(&key("t"), &both_gated),
            Some(PickerAction::Activate(PaneChoice::Terminal))
        );
    }

    #[test]
    fn vertical_keys_move_the_cursor_and_enter_takes_it() {
        for key_name in ["j", "down"] {
            assert_eq!(
                picker_action(&key(key_name), &CHOICES),
                Some(PickerAction::Move(1))
            );
        }
        for key_name in ["k", "up"] {
            assert_eq!(
                picker_action(&key(key_name), &CHOICES),
                Some(PickerAction::Move(-1))
            );
        }
        assert_eq!(
            picker_action(&key("enter"), &CHOICES),
            Some(PickerAction::Confirm)
        );
    }

    #[test]
    fn horizontal_keys_do_not_drive_the_picker() {
        for key_name in ["left", "right", "h", "l"] {
            assert_eq!(picker_action(&key(key_name), &CHOICES), None);
        }
    }

    #[test]
    fn the_cursor_wraps_at_both_ends() {
        assert_eq!(wrapping_step(0, 1, CHOICES.len()), 1);
        assert_eq!(wrapping_step(CHOICES.len() - 1, 1, CHOICES.len()), 0);
        assert_eq!(wrapping_step(0, -1, CHOICES.len()), CHOICES.len() - 1);
        assert_eq!(wrapping_step(1, -1, CHOICES.len()), 0);
    }

    #[test]
    fn command_modifiers_do_not_drive_the_picker() {
        let mut keystroke = key("t");
        keystroke.modifiers.platform = true;
        assert_eq!(picker_action(&keystroke, &CHOICES), None);
    }

    #[test]
    fn activation_targets_the_existing_picker_pane() {
        assert_eq!(
            materialize_command(PaneId(42), PaneChoice::Browser),
            CommandInvocation::new("select-pane-kind", ["-t", "%42", "browser"])
        );
        assert_eq!(
            materialize_command(PaneId(42), PaneChoice::Editor),
            CommandInvocation::new("select-pane-kind", ["-t", "%42", "editor"])
        );
        assert_eq!(
            materialize_command(PaneId(7), PaneChoice::Agent),
            CommandInvocation::new("select-pane-kind", ["-t", "%7", "agent"])
        );
        assert_eq!(
            close_command(PaneId(7)),
            CommandInvocation::new("kill-pane", ["-t", "%7"])
        );
        assert_eq!(
            select_command(PaneId(7)),
            CommandInvocation::new("select-pane", ["-t", "%7"])
        );
    }
}
