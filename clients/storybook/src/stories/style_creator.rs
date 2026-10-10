use crate::story::{Section, Story};

pub const ID: &str = "style-creator";

/// The sections the style creator gathers, by story and section id: the
/// surfaces a style changes most, side by side.
const PICKS: &[(&str, &str)] = &[
    ("menus", "popup-menu"),
    ("menus", "highlighted-row"),
    ("command-palette", "surface"),
    ("sidebar", "sidebar"),
    ("buttons", "variants"),
    ("inputs", "field-states"),
    ("inputs", "number"),
    ("choices", "select-open"),
    ("choices", "switch"),
    ("dialogs", "dialog"),
    ("notifications", "toast-kinds"),
    ("notifications", "tooltip"),
];

/// The story a gathered section comes from.
pub fn source(section: &str) -> Option<&'static str> {
    PICKS
        .iter()
        .find(|(_, id)| *id == section)
        .map(|(story, _)| *story)
}

pub fn story(stories: &[&'static Story]) -> &'static Story {
    let sections = PICKS
        .iter()
        .filter_map(|(story, section)| {
            stories
                .iter()
                .find(|candidate| candidate.id == *story)?
                .sections
                .iter()
                .find(|candidate| candidate.id == *section)
                .copied()
        })
        .collect::<Vec<Section>>();
    Box::leak(Box::new(Story {
        id: ID,
        name: "Style creator",
        group: "Foundation",
        summary: "The surfaces a style changes most, together. Pick a style to start from under Knobs, tune it, and copy the look as JSON to turn it into a preset.",
        sections: Box::leak(sections.into_boxed_slice()),
    }))
}
