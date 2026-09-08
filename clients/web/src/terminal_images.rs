use std::{collections::HashMap, sync::Arc};

use gpui::RenderImage;
use image::{Frame, ImageBuffer, Rgba};
use zz_client::CoreEvent;
use zz_protocol::{MAX_KITTY_IMAGE_BYTES, MAX_KITTY_IMAGE_CHUNK_BYTES, PaneId};

const MAX_CACHE_BYTES: usize = 64 * 1024 * 1024;
const MAX_CACHE_ENTRIES: usize = 512;

struct CachedImage {
    generation: u64,
    sequence: u64,
    total_bytes: usize,
    image: Arc<RenderImage>,
}

struct Assembly {
    generation: u64,
    sequence: u64,
    width: u32,
    height: u32,
    total_bytes: usize,
    bytes: Vec<u8>,
}

#[derive(Default)]
pub struct PaneImages {
    images: HashMap<u32, CachedImage>,
    pending: HashMap<u32, Assembly>,
}

impl zz_ui::terminal::TerminalImageSource for PaneImages {
    fn image(&self, image_id: u32, generation: u64) -> Option<Arc<RenderImage>> {
        self.images
            .get(&image_id)
            .filter(|image| image.generation == generation)
            .map(|image| image.image.clone())
    }
}

#[derive(Default)]
pub struct TerminalImages {
    panes: HashMap<PaneId, PaneImages>,
    retired: Vec<CachedImage>,
    next_sequence: u64,
}

impl TerminalImages {
    pub fn pane(&self, pane: PaneId) -> Option<&PaneImages> {
        self.panes.get(&pane)
    }

    pub fn take_retired(&mut self) -> Vec<Arc<RenderImage>> {
        std::mem::take(&mut self.retired)
            .into_iter()
            .map(|image| image.image)
            .collect()
    }

    fn usage(&self) -> (usize, usize) {
        self.panes.values().fold(
            (
                self.retired.iter().map(|image| image.total_bytes).sum(),
                self.retired.len(),
            ),
            |(bytes, entries), pane| {
                (
                    bytes
                        + pane
                            .images
                            .values()
                            .map(|image| image.total_bytes)
                            .sum::<usize>()
                        + pane
                            .pending
                            .values()
                            .map(|image| image.total_bytes)
                            .sum::<usize>(),
                    entries + pane.images.len() + pane.pending.len(),
                )
            },
        )
    }

    fn make_room(&mut self, total_bytes: usize) -> bool {
        loop {
            let (bytes, entries) = self.usage();
            if bytes + total_bytes <= MAX_CACHE_BYTES && entries < MAX_CACHE_ENTRIES {
                return true;
            }
            let retired_bytes = self
                .retired
                .iter()
                .map(|image| image.total_bytes)
                .sum::<usize>();
            if bytes - retired_bytes + total_bytes <= MAX_CACHE_BYTES
                && entries - self.retired.len() < MAX_CACHE_ENTRIES
            {
                return false;
            }
            let oldest = self
                .panes
                .iter()
                .flat_map(|(pane_id, pane)| {
                    pane.pending
                        .iter()
                        .map(|(id, image)| (image.sequence, *pane_id, *id, true))
                        .chain(
                            pane.images
                                .iter()
                                .map(|(id, image)| (image.sequence, *pane_id, *id, false)),
                        )
                })
                .min_by_key(|(sequence, _, _, _)| *sequence);
            let Some((_, pane_id, image_id, pending)) = oldest else {
                return false;
            };
            let pane = self
                .panes
                .get_mut(&pane_id)
                .expect("oldest image pane exists");
            if pending {
                pane.pending.remove(&image_id);
            } else if let Some(image) = pane.images.remove(&image_id) {
                self.retired.push(image);
            }
            if pane.pending.is_empty() && pane.images.is_empty() {
                self.panes.remove(&pane_id);
            }
        }
    }

    pub fn apply(&mut self, event: &CoreEvent) {
        self.apply_event(event);
        self.panes
            .retain(|_, pane| !pane.pending.is_empty() || !pane.images.is_empty());
    }

    fn apply_event(&mut self, event: &CoreEvent) {
        match event {
            CoreEvent::HelloReceived | CoreEvent::Attached { .. } => {
                for pane in std::mem::take(&mut self.panes).into_values() {
                    self.retired.extend(pane.images.into_values());
                }
            }
            CoreEvent::PaneRemoved { pane } => {
                if let Some(pane) = self.panes.remove(pane) {
                    self.retired.extend(pane.images.into_values());
                }
            }
            CoreEvent::KittyImageBegin {
                pane,
                image_id,
                generation,
                width,
                height,
                total_bytes,
            } => {
                let expected = width
                    .checked_mul(*height)
                    .and_then(|pixels| pixels.checked_mul(4));
                if *width == 0
                    || *height == 0
                    || *total_bytes > MAX_KITTY_IMAGE_BYTES
                    || expected != Some(*total_bytes)
                {
                    return;
                }
                if let Some(images) = self.panes.get_mut(pane) {
                    if images
                        .images
                        .get(image_id)
                        .is_some_and(|image| image.generation >= *generation)
                        || images
                            .pending
                            .get(image_id)
                            .is_some_and(|image| image.generation >= *generation)
                    {
                        return;
                    }
                    images.pending.remove(image_id);
                }
                if !self.make_room(*total_bytes as usize) {
                    return;
                }
                let sequence = self.next_sequence;
                self.next_sequence = self.next_sequence.saturating_add(1);
                self.panes.entry(*pane).or_default().pending.insert(
                    *image_id,
                    Assembly {
                        generation: *generation,
                        sequence,
                        width: *width,
                        height: *height,
                        total_bytes: *total_bytes as usize,
                        bytes: Vec::with_capacity(*total_bytes as usize),
                    },
                );
            }
            CoreEvent::KittyImageChunk {
                pane,
                image_id,
                generation,
                bytes,
            } => {
                let Some(images) = self.panes.get_mut(pane) else {
                    return;
                };
                let Some(assembly) = images.pending.get_mut(image_id) else {
                    return;
                };
                if assembly.generation != *generation {
                    return;
                }
                if bytes.is_empty()
                    || bytes.len() > MAX_KITTY_IMAGE_CHUNK_BYTES
                    || assembly
                        .bytes
                        .len()
                        .checked_add(bytes.len())
                        .is_none_or(|length| length > assembly.total_bytes)
                {
                    images.pending.remove(image_id);
                    return;
                }
                assembly.bytes.extend_from_slice(bytes);
                if assembly.bytes.len() != assembly.total_bytes {
                    return;
                }
                let assembly = images
                    .pending
                    .remove(image_id)
                    .expect("completed assembly is present");
                let Some(buffer) = ImageBuffer::<Rgba<u8>, Vec<u8>>::from_raw(
                    assembly.width,
                    assembly.height,
                    assembly.bytes,
                ) else {
                    return;
                };
                let image = Arc::new(RenderImage::new(vec![Frame::new(buffer)]));
                if let Some(previous) = images.images.insert(
                    *image_id,
                    CachedImage {
                        generation: *generation,
                        sequence: assembly.sequence,
                        total_bytes: assembly.total_bytes,
                        image,
                    },
                ) {
                    self.retired.push(previous);
                }
            }
            CoreEvent::KittyImagesRemoved { pane, image_ids } => {
                if let Some(images) = self.panes.get_mut(pane) {
                    for image_id in image_ids {
                        images.pending.remove(image_id);
                        if let Some(previous) = images.images.remove(image_id) {
                            self.retired.push(previous);
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zz_ui::terminal::TerminalImageSource as _;

    fn begin(generation: u64) -> CoreEvent {
        CoreEvent::KittyImageBegin {
            pane: PaneId(1),
            image_id: 7,
            generation,
            width: 1,
            height: 1,
            total_bytes: 4,
        }
    }

    fn chunk(generation: u64, bytes: &[u8]) -> CoreEvent {
        CoreEvent::KittyImageChunk {
            pane: PaneId(1),
            image_id: 7,
            generation,
            bytes: bytes.to_vec(),
        }
    }

    #[test]
    fn images_assemble_before_a_view_exists_and_retire_replaced_generations() {
        let mut images = TerminalImages::default();
        images.apply(&begin(1));
        images.apply(&chunk(2, &[9, 9, 9, 9]));
        images.apply(&chunk(1, &[10, 20]));
        assert!(images.pane(PaneId(1)).unwrap().image(7, 1).is_none());
        images.apply(&chunk(1, &[30, 255]));
        let first = images.pane(PaneId(1)).unwrap().image(7, 1).unwrap();
        images.apply(&begin(1));
        images.apply(&chunk(1, &[0, 0, 0, 0]));
        assert!(Arc::ptr_eq(
            &first,
            &images.pane(PaneId(1)).unwrap().image(7, 1).unwrap()
        ));
        images.apply(&begin(2));
        images.apply(&chunk(2, &[50, 60, 70, 255]));
        assert!(images.pane(PaneId(1)).unwrap().image(7, 1).is_none());
        assert!(images.pane(PaneId(1)).unwrap().image(7, 2).is_some());
        let retired = images.take_retired();
        assert_eq!(retired.len(), 1);
        assert!(Arc::ptr_eq(&first, &retired[0]));
        assert!(images.take_retired().is_empty());
    }

    #[test]
    fn malformed_dimensions_and_overlong_chunks_never_publish_images() {
        let mut images = TerminalImages::default();
        for (width, height, total_bytes) in [
            (0, 1, 0),
            (1, 1, 3),
            (u32::MAX, u32::MAX, 4),
            (1, MAX_KITTY_IMAGE_BYTES, MAX_KITTY_IMAGE_BYTES),
        ] {
            images.apply(&CoreEvent::KittyImageBegin {
                pane: PaneId(1),
                image_id: 7,
                generation: 1,
                width,
                height,
                total_bytes,
            });
            assert!(images.pane(PaneId(1)).is_none());
        }
        images.apply(&begin(1));
        images.apply(&chunk(1, &[0; 5]));
        images.apply(&chunk(1, &[0; 4]));
        assert!(images.pane(PaneId(1)).is_none());
    }

    #[test]
    fn removals_cancel_partial_transfers_and_retire_completed_images() {
        let mut images = TerminalImages::default();
        images.apply(&begin(1));
        images.apply(&CoreEvent::KittyImagesRemoved {
            pane: PaneId(1),
            image_ids: vec![7],
        });
        images.apply(&chunk(1, &[0; 4]));
        assert!(images.pane(PaneId(1)).is_none());
        images.apply(&begin(2));
        images.apply(&chunk(2, &[0; 4]));
        images.apply(&CoreEvent::PaneRemoved { pane: PaneId(1) });
        assert!(images.pane(PaneId(1)).is_none());
        assert_eq!(images.take_retired().len(), 1);
        images.apply(&begin(3));
        images.apply(&chunk(3, &[0; 4]));
        images.apply(&CoreEvent::Attached {
            session: zz_protocol::SessionId(2),
        });
        assert!(images.pane(PaneId(1)).is_none());
        assert_eq!(images.take_retired().len(), 1);
    }

    #[test]
    fn pending_transfers_reserve_their_full_size_and_evict_the_oldest() {
        let mut images = TerminalImages::default();
        let image_count = MAX_CACHE_BYTES / MAX_KITTY_IMAGE_BYTES as usize;
        for id in 0..=image_count {
            images.apply(&CoreEvent::KittyImageBegin {
                pane: PaneId(id as u64),
                image_id: id as u32,
                generation: 1,
                width: 1,
                height: MAX_KITTY_IMAGE_BYTES / 4,
                total_bytes: MAX_KITTY_IMAGE_BYTES,
            });
            assert!(images.usage().0 <= MAX_CACHE_BYTES);
        }
        assert_eq!(images.usage(), (MAX_CACHE_BYTES, image_count));
        assert!(images.pane(PaneId(0)).is_none());
        assert!(images.pane(PaneId(image_count as u64)).is_some());
    }

    #[test]
    fn retired_images_keep_consuming_capacity_until_gpu_cleanup() {
        let mut images = TerminalImages::default();
        for id in 0..MAX_CACHE_ENTRIES as u32 {
            images.apply(&CoreEvent::KittyImageBegin {
                pane: PaneId(1),
                image_id: id,
                generation: 1,
                width: 1,
                height: 1,
                total_bytes: 4,
            });
            images.apply(&CoreEvent::KittyImageChunk {
                pane: PaneId(1),
                image_id: id,
                generation: 1,
                bytes: vec![0; 4],
            });
        }
        let overflow = CoreEvent::KittyImageBegin {
            pane: PaneId(1),
            image_id: MAX_CACHE_ENTRIES as u32,
            generation: 1,
            width: 1,
            height: 1,
            total_bytes: 4,
        };
        images.apply(&overflow);
        assert_eq!(images.retired.len(), 1);
        assert_eq!(images.usage(), (MAX_CACHE_ENTRIES * 4, MAX_CACHE_ENTRIES));
        assert!(images.pane(PaneId(1)).unwrap().pending.is_empty());
        images.apply(&overflow);
        assert_eq!(images.retired.len(), 1);
        assert_eq!(images.take_retired().len(), 1);
        images.apply(&overflow);
        assert_eq!(images.pane(PaneId(1)).unwrap().pending.len(), 1);
        assert_eq!(images.usage(), (MAX_CACHE_ENTRIES * 4, MAX_CACHE_ENTRIES));
    }

    #[test]
    fn stale_or_duplicate_begins_do_not_cancel_a_newer_transfer() {
        let mut images = TerminalImages::default();
        images.apply(&begin(2));
        images.apply(&chunk(2, &[1, 2]));
        images.apply(&begin(1));
        images.apply(&begin(2));
        images.apply(&chunk(1, &[0; 4]));
        images.apply(&chunk(2, &[3, 255]));
        assert!(images.pane(PaneId(1)).unwrap().image(7, 2).is_some());
        images.apply(&begin(1));
        images.apply(&chunk(1, &[0; 4]));
        assert!(images.pane(PaneId(1)).unwrap().image(7, 2).is_some());
        assert!(images.retired.is_empty());
    }
}
