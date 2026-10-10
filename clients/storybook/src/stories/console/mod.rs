use crate::story::Story;

mod chooser;
mod dialogs;
mod menus;
pub(super) mod palette;
mod path_picker;
mod phone;
mod terminal;
mod tmux_styles;
mod which_key;

pub const STORIES: &[Story] = &[
    palette::STORY,
    palette::PROMPTS,
    chooser::STORY,
    path_picker::STORY,
    which_key::STORY,
    menus::STORY,
    dialogs::STORY,
    tmux_styles::STORY,
    terminal::STORY,
    phone::STORY,
];
