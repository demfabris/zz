use std::{rc::Rc, sync::Arc};

use gpui::{
    AnyElement, App, Context, Div, Entity, Focusable as _, FontWeight, IntoElement, SharedString,
    Subscription, Window, div, prelude::*, px,
};
use zz_protocol::{AgentQuestionAnswer, MAX_AGENT_ANSWER_BYTES, agent_stream::AgentQuestion};

use crate::{
    ActiveTheme as _, Colorize as _, Disableable as _, Icon, IconName, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    input::{Input, InputContentType, InputEvent, InputState},
    v_flex,
};

const CHOICE_LINE_HEIGHT: f32 = 18.0;
const CHOICE_MARKER: f32 = 14.0;
const CHOICE_OPTICAL_DROP: f32 = 0.5;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum QuestionCardStep {
    Stay,
    Handled,
    Other(usize),
    Submit(Vec<AgentQuestionAnswer>),
    Dismiss,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuestionCardAction {
    Pick { question: usize, option: usize },
    Other { question: usize },
    Submit,
    Dismiss,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QuestionCard {
    request_id: u64,
    questions: Arc<[AgentQuestion]>,
    picks: Vec<Vec<bool>>,
    other: Vec<bool>,
    other_text: Vec<String>,
    focused: usize,
}

impl QuestionCard {
    pub fn new(request_id: u64, questions: impl Into<Arc<[AgentQuestion]>>) -> Self {
        let questions = questions.into();
        Self {
            request_id,
            picks: questions
                .iter()
                .map(|question| vec![false; question.options.len()])
                .collect(),
            other: vec![false; questions.len()],
            other_text: vec![String::new(); questions.len()],
            questions,
            focused: 0,
        }
    }

    pub const fn request_id(&self) -> u64 {
        self.request_id
    }

    pub fn questions(&self) -> &[AgentQuestion] {
        &self.questions
    }

    pub const fn focused(&self) -> usize {
        self.focused
    }

    pub fn is_picked(&self, question: usize, option: usize) -> bool {
        self.picks
            .get(question)
            .and_then(|picks| picks.get(option))
            .copied()
            .unwrap_or(false)
    }

    pub fn other_selected(&self, question: usize) -> bool {
        self.other.get(question).copied().unwrap_or(false)
    }

    fn submits_on_pick(&self) -> bool {
        matches!(&*self.questions, [only] if !only.multi_select)
    }

    pub fn focus(&mut self, question: usize) -> bool {
        if question >= self.questions.len() || self.focused == question {
            return false;
        }
        self.focused = question;
        true
    }

    pub fn pick(&mut self, question: usize, option: usize) -> QuestionCardStep {
        let Some(multi_select) = self.questions.get(question).map(|q| q.multi_select) else {
            return QuestionCardStep::Stay;
        };
        let Some(picks) = self
            .picks
            .get_mut(question)
            .filter(|picks| option < picks.len())
        else {
            return QuestionCardStep::Stay;
        };
        self.focused = question;
        if multi_select {
            picks[option] = !picks[option];
            return QuestionCardStep::Handled;
        }
        for (index, picked) in picks.iter_mut().enumerate() {
            *picked = index == option;
        }
        self.other[question] = false;
        if self.submits_on_pick() {
            return self.submit();
        }
        if question + 1 < self.questions.len() {
            self.focused = question + 1;
        }
        QuestionCardStep::Handled
    }

    pub fn select_other(&mut self, question: usize) -> QuestionCardStep {
        let Some(asked) = self.questions.get(question).filter(|q| q.allow_other) else {
            return QuestionCardStep::Stay;
        };
        self.focused = question;
        if asked.multi_select {
            self.other[question] = !self.other[question];
        } else {
            self.other[question] = true;
            self.picks[question].fill(false);
        }
        if self.other[question] {
            QuestionCardStep::Other(question)
        } else {
            QuestionCardStep::Handled
        }
    }

    pub fn set_other_text(&mut self, question: usize, text: &str) -> bool {
        let Some(multi_select) = self
            .questions
            .get(question)
            .filter(|q| q.allow_other)
            .map(|q| q.multi_select)
        else {
            return false;
        };
        if self.other_text[question] == text {
            return false;
        }
        text.clone_into(&mut self.other_text[question]);
        self.focused = question;
        if !text.trim().is_empty() && !self.other[question] {
            self.other[question] = true;
            if !multi_select {
                self.picks[question].fill(false);
            }
        }
        true
    }

    pub fn is_answered(&self, question: usize) -> bool {
        self.picks
            .get(question)
            .is_some_and(|picks| picks.iter().any(|picked| *picked))
            || (self.other_selected(question)
                && !self.other_text[question].trim().is_empty()
                && self.other_text[question].trim().len() <= MAX_AGENT_ANSWER_BYTES)
    }

    pub fn is_complete(&self) -> bool {
        !self.questions.is_empty() && (0..self.questions.len()).all(|index| self.is_answered(index))
    }

    pub fn answers(&self) -> Option<Vec<AgentQuestionAnswer>> {
        self.is_complete().then(|| {
            self.questions
                .iter()
                .enumerate()
                .map(|(index, question)| AgentQuestionAnswer {
                    id: question.id.clone(),
                    answers: question
                        .options
                        .iter()
                        .zip(&self.picks[index])
                        .filter(|(_, picked)| **picked)
                        .map(|(option, _)| option.label.clone())
                        .chain(
                            self.other[index]
                                .then(|| self.other_text[index].trim().to_owned())
                                .filter(|text| !text.is_empty()),
                        )
                        .collect(),
                })
                .collect()
        })
    }

    pub fn submit(&self) -> QuestionCardStep {
        self.answers()
            .map_or(QuestionCardStep::Handled, QuestionCardStep::Submit)
    }

    fn move_focus(&mut self, forward: bool) -> QuestionCardStep {
        let count = self.questions.len();
        if count > 1 {
            self.focused = if forward {
                (self.focused + 1) % count
            } else {
                (self.focused + count - 1) % count
            };
        }
        QuestionCardStep::Handled
    }

    pub fn key(&mut self, key: &str, shift: bool) -> QuestionCardStep {
        match key {
            "escape" => QuestionCardStep::Dismiss,
            "enter" => self.submit(),
            "tab" => self.move_focus(!shift),
            "down" => self.move_focus(true),
            "up" => self.move_focus(false),
            key => match key.parse::<usize>() {
                Ok(digit @ 1..=9) => {
                    let question = self.focused;
                    let choices = self
                        .questions
                        .get(question)
                        .map_or(0, |asked| asked.options.len());
                    match digit - 1 {
                        option if option < choices => self.pick(question, option),
                        option if option == choices => self.select_other(question),
                        _ => QuestionCardStep::Stay,
                    }
                }
                _ => QuestionCardStep::Stay,
            },
        }
    }
}

pub struct QuestionCardState {
    pub card: QuestionCard,
    others: Vec<Option<Entity<InputState>>>,
    _subscriptions: Vec<Subscription>,
}

impl QuestionCardState {
    pub fn new<V: 'static>(
        request_id: u64,
        questions: impl Into<Arc<[AgentQuestion]>>,
        window: &mut Window,
        cx: &mut Context<V>,
        on_input: impl Fn(&mut V, usize, &InputEvent, &mut Window, &mut Context<V>) + Clone + 'static,
    ) -> Self {
        let card = QuestionCard::new(request_id, questions);
        let mut subscriptions = Vec::new();
        let others = card
            .questions()
            .iter()
            .enumerate()
            .map(|(index, question)| {
                question.allow_other.then(|| {
                    let placeholder = if question.options.is_empty() {
                        "Type your answer"
                    } else {
                        "Something else"
                    };
                    let input = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder));
                    let on_input = on_input.clone();
                    subscriptions.push(cx.subscribe_in(
                        &input,
                        window,
                        move |view, _, event: &InputEvent, window, cx| {
                            on_input(view, index, event, window, cx);
                        },
                    ));
                    input
                })
            })
            .collect();
        Self {
            card,
            others,
            _subscriptions: subscriptions,
        }
    }

    pub const fn request_id(&self) -> u64 {
        self.card.request_id()
    }

    pub fn other_input(&self, question: usize) -> Option<&Entity<InputState>> {
        self.others.get(question).and_then(Option::as_ref)
    }

    pub fn editing(&self, window: &Window, cx: &App) -> bool {
        self.others
            .iter()
            .flatten()
            .any(|input| input.read(cx).focus_handle(cx).is_focused(window))
    }

    pub fn input_changed(&mut self, question: usize, cx: &App) -> bool {
        let Some(input) = self.other_input(question) else {
            return false;
        };
        let text = input.read(cx).value();
        self.card.set_other_text(question, &text)
    }

    fn edit_other(&self, question: usize, window: &mut Window, cx: &mut App) {
        if let Some(input) = self.other_input(question) {
            input.read(cx).focus_handle(cx).focus(window, cx);
        }
    }

    pub fn action(
        &mut self,
        action: QuestionCardAction,
        window: &mut Window,
        cx: &mut App,
    ) -> QuestionCardStep {
        let step = match action {
            QuestionCardAction::Pick { question, option } => self.card.pick(question, option),
            QuestionCardAction::Other { question } => self.card.select_other(question),
            QuestionCardAction::Submit => self.card.submit(),
            QuestionCardAction::Dismiss => QuestionCardStep::Dismiss,
        };
        self.follow(step, window, cx)
    }

    pub fn key(
        &mut self,
        key: &str,
        shift: bool,
        window: &mut Window,
        cx: &mut App,
    ) -> QuestionCardStep {
        let step = self.card.key(key, shift);
        self.follow(step, window, cx)
    }

    fn follow(
        &self,
        step: QuestionCardStep,
        window: &mut Window,
        cx: &mut App,
    ) -> QuestionCardStep {
        if let QuestionCardStep::Other(question) = step {
            self.edit_other(question, window, cx);
            return QuestionCardStep::Handled;
        }
        step
    }

    pub fn render(
        &self,
        id: &str,
        counter: Option<SharedString>,
        enabled: bool,
        on_action: impl Fn(QuestionCardAction, &mut Window, &mut App) + 'static,
        cx: &App,
    ) -> Div {
        let on_action: Rc<dyn Fn(QuestionCardAction, &mut Window, &mut App)> = Rc::new(on_action);
        let card = &self.card;
        let several = card.questions().len() > 1;
        let hint = if several {
            "1-9 picks · tab moves · enter submits · esc dismisses"
        } else {
            "1-9 picks · enter submits · esc dismisses"
        };
        let dismiss = Rc::clone(&on_action);
        let submit = Rc::clone(&on_action);
        v_flex()
            .w_full()
            .gap_3()
            .rounded(cx.theme().radius)
            .border_1()
            .border_color(cx.theme().accent.outline())
            .bg(cx.theme().background.raised(1).opaque())
            .when(cx.theme().shadow, gpui::Styled::shadow_xs)
            .p_3()
            .children(
                card.questions()
                    .iter()
                    .enumerate()
                    .map(|(index, question)| {
                        self.render_question(id, index, question, several, enabled, &on_action, cx)
                    }),
            )
            .child(
                h_flex()
                    .w_full()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .child(
                        h_flex()
                            .min_w_0()
                            .gap_2()
                            .when_some(counter, |row, counter| row.child(chip(counter, cx)))
                            .child(
                                div()
                                    .min_w_0()
                                    .text_size(crate::rems_from_px(9.0))
                                    .text_color(cx.theme().foreground.muted())
                                    .child(hint),
                            ),
                    )
                    .child(
                        h_flex()
                            .flex_none()
                            .gap_1()
                            .child(
                                Button::new(SharedString::from(format!("{id}-dismiss")))
                                    .ghost()
                                    .small()
                                    .label("Dismiss")
                                    .disabled(!enabled)
                                    .on_click(move |_, window, cx| {
                                        dismiss(QuestionCardAction::Dismiss, window, cx);
                                        cx.stop_propagation();
                                    }),
                            )
                            .child(
                                Button::new(SharedString::from(format!("{id}-submit")))
                                    .primary()
                                    .small()
                                    .label("Submit")
                                    .disabled(!enabled || !card.is_complete())
                                    .on_click(move |_, window, cx| {
                                        submit(QuestionCardAction::Submit, window, cx);
                                        cx.stop_propagation();
                                    }),
                            ),
                    ),
            )
    }

    fn render_question(
        &self,
        id: &str,
        index: usize,
        question: &AgentQuestion,
        several: bool,
        enabled: bool,
        on_action: &Rc<dyn Fn(QuestionCardAction, &mut Window, &mut App)>,
        cx: &App,
    ) -> AnyElement {
        let card = &self.card;
        let focused = card.focused() == index;
        let digits = focused || !several;
        let hover = cx.theme().background.hover();
        let options = question.options.iter().enumerate().map(|(option, choice)| {
            let on_action = Rc::clone(on_action);
            h_flex()
                .id((SharedString::from(format!("{id}-{index}")), option))
                .w_full()
                .items_start()
                .gap_2()
                .px_1()
                .py(px(3.0))
                .rounded(cx.theme().radius)
                .when(enabled, |row| {
                    row.cursor_pointer()
                        .hover(move |row| row.bg(hover))
                        .on_click(move |_, window, cx| {
                            on_action(
                                QuestionCardAction::Pick {
                                    question: index,
                                    option,
                                },
                                window,
                                cx,
                            );
                            cx.stop_propagation();
                        })
                })
                .child(digit_badge(option, digits, cx))
                .child(choice_marker(
                    question.multi_select,
                    card.is_picked(index, option),
                    cx,
                ))
                .child(
                    v_flex()
                        .min_w_0()
                        .flex_1()
                        .child(
                            div()
                                .text_size(crate::rems_from_px(13.0))
                                .line_height(px(CHOICE_LINE_HEIGHT))
                                .child(choice.label.clone()),
                        )
                        .when_some(choice.description.clone(), |column, description| {
                            column.child(
                                div()
                                    .text_size(crate::rems_from_px(11.0))
                                    .text_color(cx.theme().foreground.muted())
                                    .child(description),
                            )
                        }),
                )
                .into_any_element()
        });
        let other = self.other_input(index).map(|input| {
            let field = Input::new(input)
                .small()
                .w_full()
                .disabled(!enabled)
                .when(question.secret, |field| {
                    field.content_type(InputContentType::Password)
                });
            if question.options.is_empty() {
                return div().w_full().child(field).into_any_element();
            }
            let on_action = Rc::clone(on_action);
            let choice = question.options.len();
            h_flex()
                .w_full()
                .items_center()
                .gap_2()
                .px_1()
                .child(digit_badge(choice, digits, cx))
                .child(
                    div()
                        .id((SharedString::from(format!("{id}-{index}-other")), choice))
                        .flex_none()
                        .when(enabled, |marker| {
                            marker.cursor_pointer().on_click(move |_, window, cx| {
                                on_action(
                                    QuestionCardAction::Other { question: index },
                                    window,
                                    cx,
                                );
                                cx.stop_propagation();
                            })
                        })
                        .child(choice_marker(
                            question.multi_select,
                            card.other_selected(index),
                            cx,
                        )),
                )
                .child(div().min_w_0().flex_1().child(field))
                .into_any_element()
        });
        let edge = if several && focused {
            cx.theme().accent
        } else {
            cx.theme().accent.opacity(0.0)
        };
        v_flex()
            .w_full()
            .gap_1()
            .when(several, |section| {
                section.pl_2().border_l_2().border_color(edge)
            })
            .child(
                h_flex()
                    .w_full()
                    .items_center()
                    .gap_2()
                    .when_some(question.header.clone(), |row, header| {
                        row.child(chip(header.into(), cx))
                    })
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .text_size(crate::rems_from_px(13.0))
                            .font_weight(FontWeight::MEDIUM)
                            .child(question.question.clone()),
                    )
                    .when(question.multi_select, |row| {
                        row.child(
                            div()
                                .flex_none()
                                .text_size(crate::rems_from_px(10.0))
                                .text_color(cx.theme().foreground.muted())
                                .child("Pick any"),
                        )
                    }),
            )
            .child(
                v_flex()
                    .w_full()
                    .gap(px(2.0))
                    .children(options)
                    .children(other),
            )
            .into_any_element()
    }
}

fn chip(text: SharedString, cx: &App) -> Div {
    div()
        .flex_none()
        .rounded(cx.theme().radius)
        .bg(cx.theme().background.raised(2))
        .px_2()
        .py(px(2.0))
        .text_size(crate::rems_from_px(10.0))
        .text_color(cx.theme().foreground.muted())
        .child(text)
}

fn digit_badge(index: usize, visible: bool, cx: &App) -> Div {
    div()
        .flex_none()
        .w(px(18.0))
        .h(px(CHOICE_LINE_HEIGHT))
        .flex()
        .items_center()
        .justify_center()
        .text_size(crate::rems_from_px(9.0))
        .text_color(cx.theme().foreground.muted())
        .when(visible && index < 9, |badge| {
            badge.child(
                div()
                    .rounded(cx.theme().radius)
                    .bg(cx.theme().background.raised(2))
                    .px_1()
                    .child(format!("{}", index + 1)),
            )
        })
}

fn choice_marker(multi_select: bool, picked: bool, cx: &App) -> Div {
    let accent = cx.theme().accent;
    let edge = if picked {
        accent
    } else {
        cx.theme().foreground.muted()
    };
    let marker = div()
        .size(px(CHOICE_MARKER))
        .flex()
        .items_center()
        .justify_center()
        .border_1()
        .border_color(edge);
    let marker = if multi_select {
        marker.rounded(px(3.0)).when(picked, |marker| {
            marker.bg(accent).child(
                Icon::new(IconName::Check)
                    .size(px(10.0))
                    .text_color(accent.on()),
            )
        })
    } else {
        marker.rounded_full().when(picked, |marker| {
            marker.child(div().size(px(6.0)).rounded_full().bg(accent))
        })
    };
    div()
        .flex_none()
        .h(px(CHOICE_LINE_HEIGHT))
        .flex()
        .items_center()
        .relative()
        .top(px(CHOICE_OPTICAL_DROP))
        .child(marker)
}

#[cfg(test)]
mod tests {
    use super::*;
    use zz_protocol::agent_stream::AgentQuestionOption;

    fn question(id: &str, labels: &[&str], multi_select: bool) -> AgentQuestion {
        AgentQuestion {
            id: id.to_owned(),
            header: None,
            question: format!("{id}?"),
            options: labels
                .iter()
                .map(|label| AgentQuestionOption {
                    label: (*label).to_owned(),
                    description: None,
                })
                .collect(),
            multi_select,
            allow_other: true,
            secret: false,
        }
    }

    fn answer(id: &str, answers: &[&str]) -> AgentQuestionAnswer {
        AgentQuestionAnswer {
            id: id.to_owned(),
            answers: answers.iter().map(|answer| (*answer).to_owned()).collect(),
        }
    }

    #[test]
    fn one_plain_question_answers_on_the_first_pick() {
        let mut card = QuestionCard::new(4, vec![question("fruit", &["apple", "pear"], false)]);
        assert_eq!(
            card.key("2", false),
            QuestionCardStep::Submit(vec![answer("fruit", &["pear"])])
        );
    }

    #[test]
    fn several_questions_wait_for_every_answer_then_submit_in_order() {
        let mut card = QuestionCard::new(
            4,
            vec![
                question("fruit", &["apple", "pear"], false),
                question("tools", &["saw", "drill", "glue"], true),
            ],
        );
        assert_eq!(card.key("enter", false), QuestionCardStep::Handled);
        assert_eq!(card.key("1", false), QuestionCardStep::Handled);
        assert_eq!(card.focused(), 1, "a single-select pick moves on");
        assert_eq!(card.key("3", false), QuestionCardStep::Handled);
        assert_eq!(card.key("1", false), QuestionCardStep::Handled);
        assert_eq!(card.key("3", false), QuestionCardStep::Handled);
        assert_eq!(card.key("2", false), QuestionCardStep::Handled);
        assert!(card.is_picked(1, 1));
        assert!(
            !card.is_picked(1, 2),
            "a second press clears a multi-select choice"
        );
        assert_eq!(
            card.key("enter", false),
            QuestionCardStep::Submit(vec![
                answer("fruit", &["apple"]),
                answer("tools", &["saw", "drill"])
            ])
        );
        assert_eq!(card.key("up", false), QuestionCardStep::Handled);
        assert_eq!(card.focused(), 0);
        assert_eq!(card.key("tab", true), QuestionCardStep::Handled);
        assert_eq!(card.focused(), 1);
        assert_eq!(card.key("escape", false), QuestionCardStep::Dismiss);
        assert_eq!(card.key("9", false), QuestionCardStep::Stay);
        assert_eq!(card.key("x", false), QuestionCardStep::Stay);
    }

    #[test]
    fn typed_answers_replace_a_single_choice_and_join_a_multiple_one() {
        let mut card = QuestionCard::new(
            4,
            vec![
                question("fruit", &["apple", "pear"], false),
                question("tools", &["saw", "drill"], true),
            ],
        );
        card.pick(0, 0);
        assert_eq!(card.key("3", false), QuestionCardStep::Other(1));
        card.focus(0);
        assert_eq!(card.key("3", false), QuestionCardStep::Other(0));
        assert!(
            !card.is_picked(0, 0),
            "the typed answer replaces the choice"
        );
        assert!(!card.is_answered(0), "until something is typed");
        assert!(card.set_other_text(0, " quince "));
        card.pick(1, 1);
        assert!(card.set_other_text(1, "chisel"));
        assert_eq!(
            card.answers(),
            Some(vec![
                answer("fruit", &["quince"]),
                answer("tools", &["drill", "chisel"])
            ])
        );
        card.pick(0, 1);
        assert_eq!(
            card.answers().expect("complete")[0],
            answer("fruit", &["pear"])
        );
    }

    #[test]
    fn a_free_text_question_answers_with_what_was_typed() {
        let mut card = QuestionCard::new(4, vec![question("name", &[], false)]);
        assert_eq!(card.key("1", false), QuestionCardStep::Other(0));
        assert_eq!(card.submit(), QuestionCardStep::Handled);
        card.set_other_text(0, "zz");
        assert_eq!(
            card.submit(),
            QuestionCardStep::Submit(vec![answer("name", &["zz"])])
        );
    }
}
