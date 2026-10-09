use crate::story::Story;

mod browser;
mod fixtures;
mod navigation;
mod panes;
mod settings;
mod shell;
mod status_bar;
mod tree;

pub const STORIES: &[Story] = &[
    shell::STORY,
    panes::STORY,
    navigation::STORY,
    status_bar::STORY,
    browser::STORY,
    settings::STORY,
];
