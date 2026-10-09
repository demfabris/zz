use anyhow::{Context as _, Result};
use derive_more::{Deref, DerefMut};
use etagere::BucketedAtlasAllocator;
use gpui::{
    AtlasBackend, AtlasKey, AtlasState, AtlasTextureId, AtlasTextureKind, AtlasTextureList,
    AtlasTile, Bounds, DevicePixels, PlatformAtlas, Point, Size,
};
use metal::Device;
use parking_lot::Mutex;
use std::{borrow::Cow, cell::Cell, collections::BTreeSet, sync::Arc};

pub struct MetalAtlas(Mutex<AtlasState<MetalAtlasTextures>>);

impl MetalAtlas {
    pub(crate) fn new(device: Device, is_apple_gpu: bool) -> Self {
        MetalAtlas(Mutex::new(AtlasState::new(MetalAtlasTextures {
            device: AssertSend(device),
            is_apple_gpu,
            monochrome_textures: Default::default(),
            polychrome_textures: Default::default(),
            next_frame: 0,
            active_frames: BTreeSet::new(),
            pending_allocations: 0,
        })))
    }

    /// Returns the GPU texture backing `id`, or `None` once every tile in it
    /// has been removed. A scene can still reference such a texture when a
    /// cached view replays a paint from before the image was dropped, so
    /// callers must skip those sprites rather than assume the texture exists.
    pub(crate) fn metal_texture(&self, id: AtlasTextureId) -> Option<metal::Texture> {
        Some(self.0.lock().backend.texture(id)?.metal_texture.clone())
    }

    pub(crate) fn begin_frame(self: &Arc<Self>) -> MetalAtlasFrame {
        let mut state = self.0.lock();
        let textures = &mut state.backend;
        let serial = textures.next_frame;
        textures.next_frame = serial.checked_add(1).expect("atlas frame serial overflow");
        textures.active_frames.insert(serial);
        MetalAtlasFrame {
            atlas: self.clone(),
            serial: Cell::new(Some(serial)),
        }
    }
}

pub(crate) struct MetalAtlasFrame {
    atlas: Arc<MetalAtlas>,
    serial: Cell<Option<u64>>,
}

impl MetalAtlasFrame {
    pub(crate) fn complete(&self) {
        if let Some(serial) = self.serial.take() {
            self.atlas.0.lock().backend.complete_frame(serial);
        }
    }
}

impl Drop for MetalAtlasFrame {
    fn drop(&mut self) {
        self.complete();
    }
}

struct MetalAtlasTextures {
    device: AssertSend<Device>,
    is_apple_gpu: bool,
    monochrome_textures: AtlasTextureList<MetalAtlasTexture>,
    polychrome_textures: AtlasTextureList<MetalAtlasTexture>,
    next_frame: u64,
    active_frames: BTreeSet<u64>,
    pending_allocations: usize,
}

impl PlatformAtlas for MetalAtlas {
    fn get_or_insert_with<'a>(
        &self,
        key: AtlasKey,
        build: &mut dyn FnMut() -> Result<Option<(Size<DevicePixels>, Cow<'a, [u8]>)>>,
    ) -> Result<Option<AtlasTile>> {
        self.0.lock().get_or_insert_with(key, build)
    }

    fn remove(&self, key: &AtlasKey) {
        self.0.lock().remove(key);
    }
}

impl AtlasBackend for MetalAtlasTextures {
    fn insert(
        &mut self,
        kind: AtlasTextureKind,
        size: Size<DevicePixels>,
        bytes: &[u8],
    ) -> Result<AtlasTile> {
        let tile = self.allocate(size, kind).context("failed to allocate")?;
        let texture = self
            .texture(tile.texture_id)
            .context("allocated tile refers to a missing texture")?;
        texture.upload(tile.bounds, bytes);
        Ok(tile)
    }

    fn remove(&mut self, tile: AtlasTile) {
        let id = tile.texture_id;
        let retire_after = self.active_frames.last().copied();

        let textures = match id.kind {
            AtlasTextureKind::Monochrome => &mut self.monochrome_textures,
            AtlasTextureKind::Polychrome => &mut self.polychrome_textures,
            AtlasTextureKind::Subpixel => unreachable!(),
        };

        let Some(texture_slot) = textures
            .textures
            .iter_mut()
            .find(|texture| texture.as_ref().is_some_and(|v| v.id == id))
        else {
            return;
        };

        if let Some(mut texture) = texture_slot.take() {
            let previously_pending = texture.retired_allocations.len();
            texture.decrement_ref_count();
            let pending = if texture.is_unreferenced() {
                textures.free_list.push(id.index as usize);
                0
            } else {
                if let Some(serial) = retire_after {
                    texture
                        .retired_allocations
                        .push((serial, tile.tile_id.into()));
                } else {
                    texture.allocator.deallocate(tile.tile_id.into());
                }
                let pending = texture.retired_allocations.len();
                *texture_slot = Some(texture);
                pending
            };
            self.pending_allocations = self.pending_allocations - previously_pending + pending;
        }
    }
}

impl MetalAtlasTextures {
    fn complete_frame(&mut self, serial: u64) {
        let removed = self.active_frames.remove(&serial);
        debug_assert!(removed);
        if self.pending_allocations == 0 {
            return;
        }
        let first_active = self.active_frames.first().copied();
        for texture in self
            .monochrome_textures
            .iter_mut()
            .chain(self.polychrome_textures.iter_mut())
        {
            texture.retired_allocations.retain(|(serial, allocation)| {
                if first_active.is_none_or(|active| active > *serial) {
                    texture.allocator.deallocate(*allocation);
                    self.pending_allocations -= 1;
                    false
                } else {
                    true
                }
            });
        }
    }

    fn allocate(
        &mut self,
        size: Size<DevicePixels>,
        texture_kind: AtlasTextureKind,
    ) -> Option<AtlasTile> {
        {
            let textures = match texture_kind {
                AtlasTextureKind::Monochrome => &mut self.monochrome_textures,
                AtlasTextureKind::Polychrome => &mut self.polychrome_textures,
                AtlasTextureKind::Subpixel => unreachable!(),
            };

            if let Some(tile) = textures
                .iter_mut()
                .rev()
                .find_map(|texture| texture.allocate(size))
            {
                return Some(tile);
            }
        }

        let texture = self.push_texture(size, texture_kind);
        texture.allocate(size)
    }

    fn push_texture(
        &mut self,
        min_size: Size<DevicePixels>,
        kind: AtlasTextureKind,
    ) -> &mut MetalAtlasTexture {
        const DEFAULT_ATLAS_SIZE: Size<DevicePixels> = Size {
            width: DevicePixels(1024),
            height: DevicePixels(1024),
        };
        // Max texture size on all modern Apple GPUs. Anything bigger than that crashes in validateWithDevice.
        const MAX_ATLAS_SIZE: Size<DevicePixels> = Size {
            width: DevicePixels(16384),
            height: DevicePixels(16384),
        };
        let size = min_size.min(&MAX_ATLAS_SIZE).max(&DEFAULT_ATLAS_SIZE);
        let texture_descriptor = metal::TextureDescriptor::new();
        texture_descriptor.set_width(size.width.into());
        texture_descriptor.set_height(size.height.into());
        let pixel_format;
        let usage;
        match kind {
            AtlasTextureKind::Monochrome => {
                pixel_format = metal::MTLPixelFormat::A8Unorm;
                usage = metal::MTLTextureUsage::ShaderRead;
            }
            AtlasTextureKind::Polychrome => {
                pixel_format = metal::MTLPixelFormat::BGRA8Unorm;
                usage = metal::MTLTextureUsage::ShaderRead;
            }
            AtlasTextureKind::Subpixel => unreachable!(),
        }
        texture_descriptor.set_pixel_format(pixel_format);
        texture_descriptor.set_usage(usage);
        // Shared memory mode can be used only on Apple GPU families
        // https://developer.apple.com/documentation/metal/mtlresourceoptions/storagemodeshared
        texture_descriptor.set_storage_mode(if self.is_apple_gpu {
            metal::MTLStorageMode::Shared
        } else {
            metal::MTLStorageMode::Managed
        });
        let metal_texture = self.device.new_texture(&texture_descriptor);

        let texture_list = match kind {
            AtlasTextureKind::Monochrome => &mut self.monochrome_textures,
            AtlasTextureKind::Polychrome => &mut self.polychrome_textures,
            AtlasTextureKind::Subpixel => unreachable!(),
        };

        let index = texture_list.free_list.pop();

        let atlas_texture = MetalAtlasTexture {
            id: AtlasTextureId {
                index: index.unwrap_or(texture_list.textures.len()) as u32,
                kind,
            },
            allocator: etagere::BucketedAtlasAllocator::new(size_to_etagere(size)),
            metal_texture: AssertSend(metal_texture),
            live_atlas_keys: 0,
            retired_allocations: Vec::new(),
        };

        if let Some(ix) = index {
            texture_list.textures[ix] = Some(atlas_texture);
            texture_list.textures.get_mut(ix)
        } else {
            texture_list.textures.push(Some(atlas_texture));
            texture_list.textures.last_mut()
        }
        .unwrap()
        .as_mut()
        .unwrap()
    }

    fn texture(&self, id: AtlasTextureId) -> Option<&MetalAtlasTexture> {
        let textures = match id.kind {
            AtlasTextureKind::Monochrome => &self.monochrome_textures,
            AtlasTextureKind::Polychrome => &self.polychrome_textures,
            AtlasTextureKind::Subpixel => unreachable!(),
        };
        textures.textures.get(id.index as usize)?.as_ref()
    }
}

struct MetalAtlasTexture {
    id: AtlasTextureId,
    allocator: BucketedAtlasAllocator,
    metal_texture: AssertSend<metal::Texture>,
    live_atlas_keys: u32,
    retired_allocations: Vec<(u64, etagere::AllocId)>,
}

impl MetalAtlasTexture {
    fn allocate(&mut self, size: Size<DevicePixels>) -> Option<AtlasTile> {
        let allocation = self.allocator.allocate(size_to_etagere(size))?;
        let tile = AtlasTile {
            texture_id: self.id,
            tile_id: allocation.id.into(),
            bounds: Bounds {
                origin: point_from_etagere(allocation.rectangle.min),
                size,
            },
            padding: 0,
        };
        self.live_atlas_keys += 1;
        Some(tile)
    }

    fn upload(&self, bounds: Bounds<DevicePixels>, bytes: &[u8]) {
        let region = metal::MTLRegion::new_2d(
            bounds.origin.x.into(),
            bounds.origin.y.into(),
            bounds.size.width.into(),
            bounds.size.height.into(),
        );
        self.metal_texture.replace_region(
            region,
            0,
            bytes.as_ptr() as *const _,
            bounds.size.width.to_bytes(self.bytes_per_pixel()) as u64,
        );
    }

    fn bytes_per_pixel(&self) -> u8 {
        use metal::MTLPixelFormat::*;
        match self.metal_texture.pixel_format() {
            A8Unorm | R8Unorm => 1,
            RGBA8Unorm | BGRA8Unorm => 4,
            _ => unimplemented!(),
        }
    }

    fn decrement_ref_count(&mut self) {
        self.live_atlas_keys -= 1;
    }

    fn is_unreferenced(&mut self) -> bool {
        self.live_atlas_keys == 0
    }
}

fn size_to_etagere(size: Size<DevicePixels>) -> etagere::Size {
    etagere::Size::new(size.width.into(), size.height.into())
}

fn point_from_etagere(value: etagere::Point) -> Point<DevicePixels> {
    Point {
        x: DevicePixels::from(value.x),
        y: DevicePixels::from(value.y),
    }
}

#[derive(Deref, DerefMut)]
struct AssertSend<T>(T);

unsafe impl<T> Send for AssertSend<T> {}

#[cfg(test)]
mod tests {
    use super::*;
    use block2::RcBlock;
    use foreign_types::ForeignType;
    use gpui::PlatformAtlas;
    use objc2::runtime::AnyObject;
    use std::{borrow::Cow, ptr, sync::mpsc, time::Duration};

    fn create_atlas() -> Option<MetalAtlas> {
        let device = metal::Device::system_default()?;
        Some(MetalAtlas::new(device, true))
    }

    fn make_image_key(image_id: usize, frame_index: usize) -> AtlasKey {
        AtlasKey::Image(gpui::RenderImageParams {
            image_id: gpui::ImageId(image_id),
            frame_index,
        })
    }

    fn insert_tile(atlas: &MetalAtlas, key: AtlasKey, size: Size<DevicePixels>) -> AtlasTile {
        atlas
            .get_or_insert_with(key, &mut || {
                let byte_count = (size.width.0 as usize) * (size.height.0 as usize) * 4;
                Ok(Some((size, Cow::Owned(vec![0u8; byte_count]))))
            })
            .expect("allocation should succeed")
            .expect("callback returns Some")
    }

    #[test]
    fn test_remove_clears_stale_keys_from_tiles_by_key() {
        let Some(atlas) = create_atlas() else {
            return;
        };

        let small = Size {
            width: DevicePixels(64),
            height: DevicePixels(64),
        };

        let key_a = make_image_key(1, 0);
        let key_b = make_image_key(2, 0);
        let key_c = make_image_key(3, 0);

        let tile_a = insert_tile(&atlas, key_a.clone(), small);
        let tile_b = insert_tile(&atlas, key_b.clone(), small);
        let tile_c = insert_tile(&atlas, key_c.clone(), small);

        assert_eq!(tile_a.texture_id, tile_b.texture_id);
        assert_eq!(tile_b.texture_id, tile_c.texture_id);

        // Remove A: texture still has B and C, so it stays.
        // The key for A must be removed from tiles_by_key.
        atlas.remove(&key_a);

        // Remove B: texture still has C.
        atlas.remove(&key_b);

        // Remove C: texture becomes unreferenced and is deleted.
        atlas.remove(&key_c);

        // Re-inserting A must allocate a fresh tile on a new texture,
        // NOT return a stale tile referencing the deleted texture.
        let tile_a2 = insert_tile(&atlas, key_a, small);

        // The texture must actually exist — this would panic before the fix.
        assert!(atlas.metal_texture(tile_a2.texture_id).is_some());
    }

    #[test]
    fn test_metal_texture_is_none_after_last_tile_removed() {
        let Some(atlas) = create_atlas() else {
            return;
        };

        let key = make_image_key(1, 0);
        let tile = insert_tile(
            &atlas,
            key.clone(),
            Size {
                width: DevicePixels(64),
                height: DevicePixels(64),
            },
        );
        assert!(atlas.metal_texture(tile.texture_id).is_some());

        // A scene built before the removal may still carry `tile`; looking its
        // texture up must report the gap instead of panicking.
        atlas.remove(&key);
        assert!(atlas.metal_texture(tile.texture_id).is_none());
    }

    #[test]
    fn test_remove_deallocates_tile_space_for_reuse() {
        let Some(atlas) = create_atlas() else {
            return;
        };

        let small = Size {
            width: DevicePixels(64),
            height: DevicePixels(64),
        };
        let big = Size {
            width: DevicePixels(700),
            height: DevicePixels(700),
        };

        let keeper_key = make_image_key(1, 0);
        let big_key_a = make_image_key(2, 0);
        let big_key_b = make_image_key(3, 0);

        let keeper_tile = insert_tile(&atlas, keeper_key, small);
        let tile_a = insert_tile(&atlas, big_key_a.clone(), big);
        assert_eq!(keeper_tile.texture_id, tile_a.texture_id);

        atlas.remove(&big_key_a);
        let tile_b = insert_tile(&atlas, big_key_b, big);
        assert_eq!(tile_b.texture_id, keeper_tile.texture_id);
    }

    #[test]
    fn test_remove_nonexistent_key_is_noop() {
        let Some(atlas) = create_atlas() else {
            return;
        };
        let key = make_image_key(999, 0);
        atlas.remove(&key);
    }

    #[test]
    fn retired_tiles_wait_for_all_older_frames_but_not_newer_frames() {
        fn require_send<T: Send>() {}
        require_send::<MetalAtlasFrame>();
        let Some(atlas) = create_atlas().map(Arc::new) else {
            return;
        };
        let keeper = make_image_key(10, 0);
        let old = make_image_key(11, 0);
        let extent = gpui::size(DevicePixels(700), DevicePixels(700));
        insert_tile(&atlas, keeper.clone(), gpui::size(64.into(), 64.into()));
        let tile = insert_tile(&atlas, old.clone(), extent);
        let first = atlas.begin_frame();
        let second = atlas.begin_frame();
        atlas.remove(&old);
        atlas.remove(&old);
        assert_eq!(atlas.0.lock().backend.pending_allocations, 1);
        assert!(!atlas.0.lock().contains(&old));
        let newer = atlas.begin_frame();
        second.complete();
        assert_eq!(atlas.0.lock().backend.pending_allocations, 1);
        let replacement = insert_tile(&atlas, make_image_key(12, 0), extent);
        assert_ne!(replacement.texture_id, tile.texture_id);
        first.complete();
        first.complete();
        assert_eq!(atlas.0.lock().backend.pending_allocations, 0);
        assert_eq!(atlas.0.lock().backend.active_frames.len(), 1);
        let reused = insert_tile(&atlas, make_image_key(13, 0), extent);
        assert_eq!(reused.texture_id, tile.texture_id);
        assert_eq!(reused.bounds, tile.bounds);
        drop(newer);
        assert!(atlas.0.lock().backend.active_frames.is_empty());
    }

    #[test]
    fn retirement_does_not_follow_recycled_texture_indices() {
        let Some(atlas) = create_atlas().map(Arc::new) else {
            return;
        };
        let extent = gpui::size(DevicePixels(700), DevicePixels(700));
        let keeper = make_image_key(20, 0);
        let old = make_image_key(21, 0);
        insert_tile(&atlas, keeper.clone(), gpui::size(64.into(), 64.into()));
        let tile = insert_tile(&atlas, old.clone(), extent);
        let original_texture = atlas.metal_texture(tile.texture_id).unwrap();
        let first = atlas.begin_frame();
        atlas.remove(&old);
        atlas.remove(&keeper);
        assert_eq!(atlas.0.lock().backend.pending_allocations, 0);
        let next_keeper = make_image_key(22, 0);
        let next = make_image_key(23, 0);
        let keeper_tile = insert_tile(
            &atlas,
            next_keeper.clone(),
            gpui::size(64.into(), 64.into()),
        );
        assert_eq!(keeper_tile.texture_id, tile.texture_id);
        assert_ne!(
            atlas
                .metal_texture(keeper_tile.texture_id)
                .unwrap()
                .as_ptr(),
            original_texture.as_ptr()
        );
        let next_tile = insert_tile(&atlas, next.clone(), extent);
        let second = atlas.begin_frame();
        atlas.remove(&next);
        first.complete();
        assert_eq!(atlas.0.lock().backend.pending_allocations, 1);
        assert!(atlas.0.lock().contains(&next_keeper));
        second.complete();
        assert_eq!(atlas.0.lock().backend.pending_allocations, 0);
        let reused = insert_tile(&atlas, make_image_key(24, 0), extent);
        assert_eq!(reused.texture_id, next_tile.texture_id);
        assert_eq!(reused.bounds, next_tile.bounds);
    }

    #[test]
    fn abandoning_an_uncommitted_buffer_retires_its_atlas_frame() {
        let Some(atlas) = create_atlas().map(Arc::new) else {
            return;
        };
        let keeper = make_image_key(30, 0);
        let old = make_image_key(31, 0);
        let extent = gpui::size(DevicePixels(700), DevicePixels(700));
        insert_tile(&atlas, keeper.clone(), gpui::size(64.into(), 64.into()));
        let old_tile = insert_tile(&atlas, old.clone(), extent);
        let device = metal::Device::system_default().unwrap();
        let queue = device.new_command_queue();
        objc::rc::autoreleasepool(|| {
            let commands = queue.new_command_buffer().to_owned();
            let frame = atlas.begin_frame();
            let block = RcBlock::new(move |_: ptr::NonNull<AnyObject>| frame.complete());
            // SAFETY: Both pointee types are opaque views of the same Objective-C block pointer ABI.
            unsafe {
                commands.add_completed_handler(&*RcBlock::as_ptr(&block).cast());
            }
            drop(block);
            atlas.remove(&old);
            assert_eq!(atlas.0.lock().backend.pending_allocations, 1);
            drop(commands);
        });
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while !atlas.0.lock().backend.active_frames.is_empty() {
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
        assert!(atlas.0.lock().backend.active_frames.is_empty());
        assert_eq!(atlas.0.lock().backend.pending_allocations, 0);
        let reused = insert_tile(&atlas, make_image_key(32, 0), extent);
        assert_eq!(reused.texture_id, old_tile.texture_id);
        assert_eq!(reused.bounds, old_tile.bounds);
    }

    struct SignalOnDrop(metal::SharedEvent);

    impl Drop for SignalOnDrop {
        fn drop(&mut self) {
            self.0.set_signaled_value(1);
        }
    }

    #[test]
    fn inflight_replacement_preserves_old_pixels_and_reuses_space_after_completion() {
        let Some(atlas) = create_atlas().map(Arc::new) else {
            return;
        };
        objc::rc::autoreleasepool(|| {
            let device = metal::Device::system_default().unwrap();
            let queue = device.new_command_queue();
            let keeper = make_image_key(40, 0);
            insert_tile(&atlas, keeper.clone(), gpui::size(64.into(), 64.into()));
            let old = make_image_key(41, 0);
            let extent = gpui::size(DevicePixels(700), DevicePixels(700));
            let tile = atlas
                .get_or_insert_with(old.clone(), &mut || {
                    Ok(Some((extent, Cow::Owned(vec![0x11; 700 * 700 * 4]))))
                })
                .unwrap()
                .unwrap();
            let frame = atlas.begin_frame();
            let texture = atlas.metal_texture(tile.texture_id).unwrap();
            let output = device.new_buffer(512, metal::MTLResourceOptions::StorageModeShared);
            let gate = SignalOnDrop(device.new_shared_event());
            let commands = queue.new_command_buffer();
            commands.encode_wait_for_event(&gate.0, 1);
            let blit = commands.new_blit_command_encoder();
            blit.copy_from_texture_to_buffer(
                &texture,
                0,
                0,
                metal::MTLOrigin {
                    x: tile.bounds.origin.x.0 as u64,
                    y: tile.bounds.origin.y.0 as u64,
                    z: 0,
                },
                metal::MTLSize {
                    width: 1,
                    height: 1,
                    depth: 1,
                },
                &output,
                0,
                256,
                256,
                metal::MTLBlitOption::None,
            );
            blit.end_encoding();
            let (complete, completed) = mpsc::channel();
            let block = RcBlock::new(move |_: ptr::NonNull<AnyObject>| {
                frame.complete();
                complete.send(()).unwrap();
            });
            // SAFETY: Both pointee types are opaque views of the same Objective-C block pointer ABI.
            unsafe {
                commands.add_completed_handler(&*RcBlock::as_ptr(&block).cast());
            }
            commands.commit();
            atlas.remove(&old);
            let replacement = atlas
                .get_or_insert_with(make_image_key(42, 0), &mut || {
                    Ok(Some((extent, Cow::Owned(vec![0x99; 700 * 700 * 4]))))
                })
                .unwrap()
                .unwrap();
            assert_ne!(replacement.texture_id, tile.texture_id);
            let replacement_texture = atlas.metal_texture(replacement.texture_id).unwrap();
            let replacement_commands = queue.new_command_buffer();
            let replacement_blit = replacement_commands.new_blit_command_encoder();
            replacement_blit.copy_from_texture_to_buffer(
                &replacement_texture,
                0,
                0,
                metal::MTLOrigin {
                    x: replacement.bounds.origin.x.0 as u64,
                    y: replacement.bounds.origin.y.0 as u64,
                    z: 0,
                },
                metal::MTLSize {
                    width: 1,
                    height: 1,
                    depth: 1,
                },
                &output,
                256,
                256,
                256,
                metal::MTLBlitOption::None,
            );
            replacement_blit.end_encoding();
            replacement_commands.commit();
            gate.0.set_signaled_value(1);
            completed.recv_timeout(Duration::from_secs(5)).unwrap();
            replacement_commands.wait_until_completed();
            assert_eq!(commands.status(), metal::MTLCommandBufferStatus::Completed);
            assert_eq!(
                replacement_commands.status(),
                metal::MTLCommandBufferStatus::Completed
            );
            let bytes = unsafe { std::slice::from_raw_parts(output.contents().cast::<u8>(), 260) };
            assert_eq!(&bytes[..4], &[0x11; 4]);
            assert_eq!(&bytes[256..260], &[0x99; 4]);
            assert_eq!(atlas.0.lock().backend.pending_allocations, 0);
            let reused = insert_tile(&atlas, make_image_key(43, 0), extent);
            assert_eq!(reused.texture_id, tile.texture_id);
            assert_eq!(reused.bounds, tile.bounds);
        });
    }
}
