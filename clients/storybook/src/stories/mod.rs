use std::sync::LazyLock;

use crate::story::Story;

mod agent;
mod foundation;
mod kit;
mod zz;

pub static STORIES: LazyLock<Vec<&'static Story>> = LazyLock::new(|| {
    [
        foundation::STORIES,
        agent::STORIES,
        zz::STORIES,
        kit::STORIES,
    ]
    .into_iter()
    .flatten()
    .collect()
});

#[cfg(test)]
mod tests {
    use super::*;

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
