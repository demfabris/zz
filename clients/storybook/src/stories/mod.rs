use std::sync::LazyLock;

use crate::story::Story;

mod agent;
mod console;
mod foundation;
mod kit;
mod style_creator;
mod workspace;

pub use style_creator::{ID as STYLE_CREATOR, source as style_creator_source};

pub static STORIES: LazyLock<Vec<&'static Story>> = LazyLock::new(|| {
    let stories = [
        foundation::STORIES,
        agent::STORIES,
        workspace::STORIES,
        console::STORIES,
        kit::STORIES,
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>();
    std::iter::once(style_creator::story(&stories))
        .chain(stories)
        .collect()
});

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_style_creator_finds_every_section_it_gathers() {
        let creator = STORIES[0];
        assert_eq!(creator.id, STYLE_CREATOR);
        assert_eq!(creator.sections.len(), 12);
    }

    #[test]
    fn ids_are_unique_and_url_safe() {
        let url_safe = |id: &str| id.chars().all(|c| c.is_ascii_lowercase() || c == '-');
        for (index, story) in STORIES.iter().enumerate() {
            assert!(url_safe(story.id), "{}", story.id);
            assert!(
                STORIES[..index].iter().all(|other| other.id != story.id),
                "duplicate story {}",
                story.id
            );
            for (position, section) in story.sections.iter().enumerate() {
                assert!(url_safe(section.id), "{}/{}", story.id, section.id);
                assert!(
                    story.sections[..position]
                        .iter()
                        .all(|other| other.id != section.id),
                    "duplicate section {}/{}",
                    story.id,
                    section.id
                );
            }
        }
    }
}
