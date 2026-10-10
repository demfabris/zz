use crate::story::Story;

mod buttons;
mod code_editor;
mod color_picker;
mod dialogs;
mod display;
mod icons;
mod inputs;
mod markdown;
pub(super) mod menus;
mod notifications;
mod scrollbars;
mod selection;
mod sheet;
mod support;
mod title_bar;

pub const STORIES: &[Story] = &[
    buttons::STORY,
    inputs::STORY,
    selection::STORY,
    display::STORY,
    menus::STORY,
    dialogs::STORY,
    notifications::STORY,
    sheet::STORY,
    scrollbars::STORY,
    markdown::STORY,
    code_editor::STORY,
    color_picker::STORY,
    icons::STORY,
    title_bar::STORY,
];
