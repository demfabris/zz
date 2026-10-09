use crate::{AtlasTile, Bounds, DevicePixels, RenderGlyphParams};

const GLYPH_SLOT_BITS: u32 = 12;

pub(super) struct GlyphLookupCache {
    slots: Box<[Option<GlyphSlot>]>,
    frame: u64,
    font_generation: Option<usize>,
}

#[derive(Clone)]
struct GlyphSlot {
    params: RenderGlyphParams,
    bounds: Bounds<DevicePixels>,
    tile: Option<(u64, AtlasTile)>,
}

impl Default for GlyphLookupCache {
    fn default() -> Self {
        Self {
            slots: vec![None; 1 << GLYPH_SLOT_BITS].into_boxed_slice(),
            frame: 0,
            font_generation: None,
        }
    }
}

impl GlyphLookupCache {
    fn slot(params: &RenderGlyphParams) -> usize {
        const K: u64 = 0x9e37_79b9_7f4a_7c15;
        let words = [
            params.glyph_id.0 as u64 | (params.font_id.0 as u64) << 32,
            params.font_size.0.to_bits() as u64
                | (params.subpixel_variant.x as u64) << 32
                | (params.subpixel_variant.y as u64) << 40
                | (params.font_smoothing_strength as u64) << 48
                | (params.subpixel_rendering as u64) << 56
                | (params.is_emoji as u64) << 57
                | (params.font_smoothing as u64) << 58
                | (params.synthetic_bold as u64) << 59
                | (params.synthetic_italic as u64) << 60,
            params.scale_factor.to_bits() as u64,
        ];
        let hash = words.iter().fold(0u64, |hash, word| {
            (hash.rotate_left(26) ^ word).wrapping_mul(K)
        });
        (hash >> (64 - GLYPH_SLOT_BITS)) as usize
    }

    pub(super) fn sync_font_generation(&mut self, generation: usize) {
        if self.font_generation != Some(generation) {
            if self.font_generation.is_some() {
                self.slots.fill(None);
            }
            self.font_generation = Some(generation);
        }
    }

    pub(super) fn lookup(
        &self,
        params: &RenderGlyphParams,
    ) -> Option<(Bounds<DevicePixels>, Option<AtlasTile>)> {
        match &self.slots[Self::slot(params)] {
            Some(slot) if slot.params == *params => Some((
                slot.bounds,
                slot.tile
                    .and_then(|(frame, tile)| (frame == self.frame).then_some(tile)),
            )),
            _ => None,
        }
    }

    pub(super) fn insert_bounds(
        &mut self,
        params: &RenderGlyphParams,
        bounds: Bounds<DevicePixels>,
    ) {
        self.slots[Self::slot(params)] = Some(GlyphSlot {
            params: params.clone(),
            bounds,
            tile: None,
        });
    }

    pub(super) fn insert_tile(&mut self, params: &RenderGlyphParams, tile: AtlasTile) {
        if let Some(slot) = &mut self.slots[Self::slot(params)]
            && slot.params == *params
        {
            slot.tile = Some((self.frame, tile));
        }
    }

    pub(super) fn finish_frame(&mut self) {
        self.frame = self
            .frame
            .checked_add(1)
            .expect("glyph cache frame overflow");
    }
}

#[cfg(test)]
mod tests {
    use super::GlyphLookupCache;
    use crate::{
        AppContext as _, AtlasKey, AtlasState, AtlasTextureKind, Bounds, DevicePixels, Empty,
        FontId, GlyphId, HeadlessAtlas, HeadlessAtlasBackend, NoopTextSystem, PlatformAtlas,
        RenderGlyphParams, Size, TestAppContext, TextSystem, point, px, size,
    };
    use std::{borrow::Cow, cell::Cell, sync::Arc};

    #[derive(Default)]
    struct CountingAtlas {
        atlas: HeadlessAtlas,
        lookups: Cell<usize>,
    }

    impl PlatformAtlas for CountingAtlas {
        fn get_or_insert_with<'a>(
            &self,
            key: AtlasKey,
            build: &mut dyn FnMut() -> anyhow::Result<Option<(Size<DevicePixels>, Cow<'a, [u8]>)>>,
        ) -> anyhow::Result<Option<crate::AtlasTile>> {
            self.lookups.set(self.lookups.get() + 1);
            self.atlas.get_or_insert_with(key, build)
        }

        fn remove(&self, key: &AtlasKey) {
            self.atlas.remove(key);
        }
    }

    fn glyph(glyph_id: u32) -> RenderGlyphParams {
        RenderGlyphParams {
            font_id: FontId(1),
            glyph_id: GlyphId(glyph_id),
            font_size: px(14.),
            subpixel_variant: point(0, 0),
            scale_factor: 2.,
            is_emoji: false,
            subpixel_rendering: false,
            font_smoothing: false,
            font_smoothing_strength: 0,
            synthetic_bold: false,
            synthetic_italic: false,
        }
    }

    fn bounds(value: i32) -> Bounds<DevicePixels> {
        Bounds::new(
            point(DevicePixels(value), DevicePixels(-1)),
            size(DevicePixels(10), DevicePixels(14)),
        )
    }

    #[test]
    fn glyph_lookup_requires_the_complete_carried_render_key() {
        let mut cache = GlyphLookupCache::default();
        let params = glyph(7);
        assert_eq!(cache.lookup(&params), None);
        cache.insert_bounds(&params, bounds(5));
        assert_eq!(cache.lookup(&params), Some((bounds(5), None)));
        for changed in [
            RenderGlyphParams {
                font_id: FontId(2),
                ..params.clone()
            },
            RenderGlyphParams {
                glyph_id: GlyphId(8),
                ..params.clone()
            },
            RenderGlyphParams {
                font_size: px(15.),
                ..params.clone()
            },
            RenderGlyphParams {
                subpixel_variant: point(1, 0),
                ..params.clone()
            },
            RenderGlyphParams {
                subpixel_variant: point(0, 1),
                ..params.clone()
            },
            RenderGlyphParams {
                scale_factor: 1.,
                ..params.clone()
            },
            RenderGlyphParams {
                is_emoji: true,
                ..params.clone()
            },
            RenderGlyphParams {
                subpixel_rendering: true,
                ..params.clone()
            },
            RenderGlyphParams {
                font_smoothing: true,
                ..params.clone()
            },
            RenderGlyphParams {
                font_smoothing_strength: 64,
                ..params.clone()
            },
            RenderGlyphParams {
                synthetic_bold: true,
                ..params.clone()
            },
            RenderGlyphParams {
                synthetic_italic: true,
                ..params.clone()
            },
        ] {
            assert_eq!(cache.lookup(&changed), None);
        }
        for glyph_id in 0..10_000 {
            cache.insert_bounds(&glyph(glyph_id), bounds(glyph_id as i32));
        }
        for glyph_id in 0..10_000 {
            let found = cache.lookup(&glyph(glyph_id));
            assert!(found.is_none() || found == Some((bounds(glyph_id as i32), None)));
        }
        assert_eq!(cache.lookup(&glyph(9_999)), Some((bounds(9_999), None)));
    }

    #[test]
    fn atlas_clear_between_frames_requires_a_fresh_tile() {
        let params = glyph(7);
        let mut atlas = AtlasState::new(HeadlessAtlasBackend::default());
        let mut build = || Ok(Some((bounds(5).size, Cow::Owned(vec![0; 140]))));
        let tile = atlas
            .get_or_insert_with(AtlasKey::Glyph(params.clone()), &mut build)
            .unwrap()
            .unwrap();
        assert_eq!(tile.texture_id.kind, AtlasTextureKind::Monochrome);
        let mut cache = GlyphLookupCache::default();
        cache.insert_tile(&params, tile);
        assert_eq!(cache.lookup(&params), None);
        cache.insert_bounds(&params, bounds(5));
        cache.insert_tile(&params, tile);
        assert_eq!(cache.lookup(&params), Some((bounds(5), Some(tile))));
        cache.insert_tile(&glyph(8), tile);
        assert_eq!(cache.lookup(&glyph(8)), None);
        cache.finish_frame();
        atlas.clear(|_| {});
        assert_eq!(cache.lookup(&params), Some((bounds(5), None)));
        let new_tile = atlas
            .get_or_insert_with(AtlasKey::Glyph(params.clone()), &mut build)
            .unwrap()
            .unwrap();
        assert_ne!(new_tile, tile);
        cache.insert_tile(&params, new_tile);
        assert_eq!(cache.lookup(&params), Some((bounds(5), Some(new_tile))));
    }

    #[test]
    fn font_installation_expires_local_bounds_and_tiles() {
        let text_system = TextSystem::new(Arc::new(NoopTextSystem));
        let generation = text_system.font_cache_generation();
        let mut cache = GlyphLookupCache::default();
        cache.sync_font_generation(generation);
        cache.insert_bounds(&glyph(7), bounds(5));
        cache.sync_font_generation(generation);
        assert_eq!(cache.lookup(&glyph(7)), Some((bounds(5), None)));
        text_system.add_fonts(Vec::new()).unwrap();
        let next_generation = text_system.font_cache_generation();
        assert_ne!(generation, next_generation);
        cache.sync_font_generation(next_generation);
        assert_eq!(cache.lookup(&glyph(7)), None);
    }

    #[test]
    fn zero_raster_bounds_stay_cached_without_an_atlas_tile() {
        let mut cache = GlyphLookupCache::default();
        cache.sync_font_generation(0);
        cache.insert_bounds(&glyph(7), Bounds::default());
        cache.finish_frame();
        assert_eq!(cache.lookup(&glyph(7)), Some((Bounds::default(), None)));
        cache.sync_font_generation(1);
        assert_eq!(cache.lookup(&glyph(7)), None);
    }

    #[gpui::test]
    fn completed_window_draw_expires_the_tile_epoch(cx: &mut TestAppContext) {
        let window = cx.add_window(|_, _| Empty);
        cx.update_window(window.into(), |_, window, cx| {
            let params = glyph(7);
            let mut atlas = AtlasState::new(HeadlessAtlasBackend::default());
            let tile = atlas
                .get_or_insert_with(AtlasKey::Glyph(params.clone()), &mut || {
                    Ok(Some((bounds(5).size, Cow::Owned(vec![0; 140]))))
                })
                .unwrap()
                .unwrap();
            window
                .glyph_lookup_cache
                .get_mut()
                .insert_bounds(&params, bounds(5));
            window
                .glyph_lookup_cache
                .get_mut()
                .insert_tile(&params, tile);
            window.draw(cx).clear(cx);
            assert_eq!(
                window.glyph_lookup_cache.borrow().lookup(&params),
                Some((bounds(5), None))
            );
        })
        .unwrap();
    }

    #[gpui::test]
    fn shared_tile_lookup_reuses_within_a_frame_and_observes_retirement(cx: &mut TestAppContext) {
        let window = cx.add_window(|_, _| Empty);
        cx.update_window(window.into(), |_, window, cx| {
            let atlas = Arc::new(CountingAtlas::default());
            window.sprite_atlas = atlas.clone();
            let params = glyph(7);
            let tile = window.get_or_insert_glyph_tile(&params).unwrap();
            assert_eq!(window.get_or_insert_glyph_tile(&params).unwrap(), tile);
            assert_eq!(atlas.lookups.get(), 1);
            window.draw(cx).clear(cx);
            atlas.remove(&AtlasKey::Glyph(params.clone()));
            let next_tile = window.get_or_insert_glyph_tile(&params).unwrap();
            assert_ne!(next_tile, tile);
            assert_eq!(atlas.lookups.get(), 2);
            assert_eq!(window.get_or_insert_glyph_tile(&params).unwrap(), next_tile);
            assert_eq!(atlas.lookups.get(), 2);
            window.text_system().add_fonts(Vec::new()).unwrap();
            assert_eq!(window.get_or_insert_glyph_tile(&params).unwrap(), next_tile);
            assert_eq!(atlas.lookups.get(), 3);
        })
        .unwrap();
    }
}
