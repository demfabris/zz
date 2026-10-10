use crate::story::Story;

mod browser;
pub(super) mod fixtures;
mod navigation;
mod panes;
mod settings;
pub(super) mod shell;
pub(super) mod status_bar;
mod tree;

pub const STORIES: &[Story] = &[
    shell::STORY,
    panes::STORY,
    navigation::STORY,
    status_bar::STORY,
    browser::STORY,
    settings::STORY,
];
