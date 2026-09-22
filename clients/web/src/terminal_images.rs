use std::{collections::HashMap, sync::Arc};
use zz_protocol::PaneId;
pub use zz_ui::terminal_images::{PaneImages, TerminalImages};

const MAX_CACHE_BYTES: usize = 64 * 1024 * 1024;
const MAX_CACHE_ENTRIES: usize = 512;

struct PastedEntry {
    sequence: u64,
    state: PastedState,
}

enum PastedState {
    Requested,
    Loading {
        format: gpui::ImageFormat,
        total: usize,
        bytes: Vec<u8>,
    },
    Ready(Arc<gpui::Image>),
    Unavailable,
}

impl PastedState {
    fn bytes(&self) -> usize {
        match self {
            Self::Loading { total, .. } => *total,
            Self::Ready(image) => image.bytes.len(),
            _ => 0,
        }
    }
}

#[derive(Default)]
pub struct PastedImages {
    entries: HashMap<(PaneId, u32), PastedEntry>,
    retired: Vec<Arc<gpui::Image>>,
    sequence: u64,
}

impl PastedImages {
    pub fn clear(&mut self) {
        for entry in self.entries.drain().map(|(_, entry)| entry) {
            if let PastedState::Ready(image) = entry.state {
                self.retired.push(image);
            }
        }
    }

    pub fn remove_pane(&mut self, pane: PaneId) {
        self.entries.retain(|(id, _), entry| {
            if *id != pane {
                return true;
            }
            if let PastedState::Ready(image) = &entry.state {
                self.retired.push(image.clone());
            }
            false
        });
    }

    pub fn release_retired(&mut self, cx: &mut gpui::App) {
        for image in self.retired.drain(..) {
            gpui::ImageSource::Image(image).remove_asset(cx);
        }
    }

    fn remove(&mut self, key: &(PaneId, u32)) {
        if let Some(PastedEntry {
            state: PastedState::Ready(image),
            ..
        }) = self.entries.remove(key)
        {
            self.retired.push(image);
        }
    }

    pub fn image(&self, pane: PaneId, number: u32) -> Option<Arc<gpui::Image>> {
        match &self.entries.get(&(pane, number))?.state {
            PastedState::Ready(image) => Some(image.clone()),
            _ => None,
        }
    }

    pub fn request(&mut self, pane: PaneId, number: u32) -> bool {
        if self.entries.contains_key(&(pane, number)) {
            return false;
        }
        if self.entries.len() >= MAX_CACHE_ENTRIES {
            let oldest = self
                .entries
                .iter()
                .min_by_key(|(_, entry)| entry.sequence)
                .map(|(key, _)| *key);
            if let Some(key) = oldest {
                self.remove(&key);
            }
        }
        self.sequence = self.sequence.wrapping_add(1);
        self.entries.insert(
            (pane, number),
            PastedEntry {
                sequence: self.sequence,
                state: PastedState::Requested,
            },
        );
        true
    }

    pub fn begin(
        &mut self,
        pane: PaneId,
        number: u32,
        format: zz_protocol::PastedImageFormat,
        total: u32,
    ) {
        if !self
            .entries
            .get(&(pane, number))
            .is_some_and(|entry| matches!(entry.state, PastedState::Requested))
        {
            return;
        }
        if total > 0 && total <= zz_protocol::MAX_PASTE_UPLOAD_BYTES {
            while self
                .entries
                .values()
                .map(|entry| entry.state.bytes())
                .sum::<usize>()
                + total as usize
                > MAX_CACHE_BYTES
            {
                let oldest = self
                    .entries
                    .iter()
                    .filter(|(key, _)| **key != (pane, number))
                    .min_by_key(|(_, entry)| entry.sequence)
                    .map(|(key, _)| *key);
                let Some(key) = oldest else {
                    break;
                };
                self.remove(&key);
            }
        }
        let used: usize = self.entries.values().map(|entry| entry.state.bytes()).sum();
        let Some(entry) = self.entries.get_mut(&(pane, number)) else {
            return;
        };
        if !matches!(entry.state, PastedState::Requested) {
            return;
        }
        if total == 0
            || total > zz_protocol::MAX_PASTE_UPLOAD_BYTES
            || used + total as usize > MAX_CACHE_BYTES
        {
            entry.state = PastedState::Unavailable;
            return;
        }
        let format = match format {
            zz_protocol::PastedImageFormat::Png => gpui::ImageFormat::Png,
            zz_protocol::PastedImageFormat::Jpeg => gpui::ImageFormat::Jpeg,
            zz_protocol::PastedImageFormat::Gif => gpui::ImageFormat::Gif,
            zz_protocol::PastedImageFormat::Webp => gpui::ImageFormat::Webp,
        };
        entry.state = PastedState::Loading {
            format,
            total: total as usize,
            bytes: Vec::with_capacity(total as usize),
        };
    }

    pub fn chunk(&mut self, pane: PaneId, number: u32, chunk: &[u8]) {
        let Some(entry) = self.entries.get_mut(&(pane, number)) else {
            return;
        };
        let PastedState::Loading {
            format,
            total,
            bytes,
        } = &mut entry.state
        else {
            return;
        };
        if chunk.is_empty()
            || chunk.len() > zz_protocol::MAX_PASTE_UPLOAD_CHUNK_BYTES
            || chunk.len() > total.saturating_sub(bytes.len())
        {
            entry.state = PastedState::Unavailable;
            return;
        }
        bytes.extend_from_slice(chunk);
        if bytes.len() == *total {
            entry.state = PastedState::Ready(Arc::new(gpui::Image::from_bytes(
                *format,
                std::mem::take(bytes),
            )));
        }
    }

    pub fn unavailable(&mut self, pane: PaneId, number: u32) {
        if let Some(entry) = self.entries.get_mut(&(pane, number))
            && let PastedState::Ready(image) =
                std::mem::replace(&mut entry.state, PastedState::Unavailable)
        {
            self.retired.push(image);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zz_ui::terminal::TerminalImageSource as _;

    #[test]
    fn pasted_images_deduplicate_and_publish_only_complete_transfers() {
        let mut cache = PastedImages::default();
        let pane = PaneId(1);
        assert!(cache.request(pane, 7));
        assert!(!cache.request(pane, 7));
        cache.begin(pane, 7, zz_protocol::PastedImageFormat::Png, 4);
        cache.chunk(pane, 7, &[1, 2]);
        assert!(cache.image(pane, 7).is_none());
        cache.begin(pane, 7, zz_protocol::PastedImageFormat::Jpeg, 2);
        cache.chunk(pane, 8, &[9, 9]);
        cache.chunk(pane, 7, &[3, 4]);
        let image = cache.image(pane, 7).unwrap();
        assert_eq!(image.format, gpui::ImageFormat::Png);
        assert_eq!(image.bytes, [1, 2, 3, 4]);
        cache.chunk(pane, 7, &[5]);
        assert!(Arc::ptr_eq(&image, &cache.image(pane, 7).unwrap()));
        assert!(!cache.request(pane, 7));
    }

    #[test]
    fn pasted_images_reject_unrequested_invalid_and_overlong_transfers() {
        let mut cache = PastedImages::default();
        let pane = PaneId(1);
        cache.begin(pane, 0, zz_protocol::PastedImageFormat::Png, 1);
        cache.chunk(pane, 0, &[1]);
        assert!(cache.entries.is_empty());
        for (number, total) in [(1, 0), (2, zz_protocol::MAX_PASTE_UPLOAD_BYTES + 1)] {
            cache.request(pane, number);
            cache.begin(pane, number, zz_protocol::PastedImageFormat::Png, total);
            assert!(matches!(
                cache.entries[&(pane, number)].state,
                PastedState::Unavailable
            ));
        }
        cache.request(pane, 3);
        cache.begin(pane, 3, zz_protocol::PastedImageFormat::Png, 2);
        cache.chunk(pane, 3, &[1, 2, 3]);
        cache.chunk(pane, 3, &[1, 2]);
        assert!(cache.image(pane, 3).is_none());
        cache.request(pane, 4);
        cache.begin(pane, 4, zz_protocol::PastedImageFormat::Png, 2);
        cache.chunk(pane, 4, &[]);
        assert!(matches!(
            cache.entries[&(pane, 4)].state,
            PastedState::Unavailable
        ));
    }

    #[test]
    fn pasted_images_bound_reserved_bytes_and_request_entries() {
        let mut cache = PastedImages::default();
        for number in 0..20 {
            cache.request(PaneId(1), number);
            cache.begin(
                PaneId(1),
                number,
                zz_protocol::PastedImageFormat::Webp,
                zz_protocol::MAX_PASTE_UPLOAD_BYTES,
            );
            assert!(
                cache
                    .entries
                    .values()
                    .map(|entry| entry.state.bytes())
                    .sum::<usize>()
                    <= MAX_CACHE_BYTES
            );
        }
        assert!(!cache.entries.contains_key(&(PaneId(1), 0)));
        for number in 20..(MAX_CACHE_ENTRIES as u32 + 40) {
            cache.request(PaneId(1), number);
            assert!(cache.entries.len() <= MAX_CACHE_ENTRIES);
        }
    }

    #[test]
    fn pasted_images_clear_pending_and_retire_completed_images() {
        let mut cache = PastedImages::default();
        for pane in [PaneId(1), PaneId(2)] {
            cache.request(pane, 1);
            cache.begin(pane, 1, zz_protocol::PastedImageFormat::Gif, 1);
            cache.chunk(pane, 1, &[1]);
            cache.request(pane, 2);
            cache.begin(pane, 2, zz_protocol::PastedImageFormat::Png, 1);
        }
        cache.unavailable(PaneId(1), 2);
        cache.chunk(PaneId(1), 2, &[1]);
        assert!(cache.image(PaneId(1), 2).is_none());
        assert!(!cache.request(PaneId(1), 2));
        cache.remove_pane(PaneId(1));
        assert_eq!(cache.retired.len(), 1);
        assert!(cache.image(PaneId(1), 1).is_none());
        assert!(cache.image(PaneId(2), 1).is_some());
        cache.clear();
        assert_eq!(cache.retired.len(), 2);
        assert!(cache.entries.is_empty());
        assert!(cache.request(PaneId(2), 1));
    }
}
