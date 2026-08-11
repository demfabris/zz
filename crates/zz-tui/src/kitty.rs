use std::{
    collections::{HashMap, HashSet},
    fs,
    io::Write as _,
    path::PathBuf,
};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use miniz_oxide::deflate::compress_to_vec_zlib;
use zz_protocol::PaneId;
use zz_terminal::{KittyLayer, KittyPlacement, MAX_KITTY_IMAGE_BYTES};

use crate::layout::Rect;

pub(crate) const PROBE_IMAGE_ID: u32 = u32::MAX;
pub(crate) const FILE_PROBE_IMAGE_ID: u32 = u32::MAX - 1;

const BASE64_CHUNK_BYTES: usize = 4096;
const FRAME_SLOT_COUNT: u64 = 8;
const Z_BELOW_BACKGROUND: i32 = -1_073_741_827;
const Z_BELOW_TEXT: i32 = -1;
const Z_ABOVE_TEXT: i32 = 1;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum FrameTransport {
    File,
    #[default]
    Inline,
}

#[derive(Default)]
struct FrameSlots {
    sequence: u64,
    written: HashSet<PathBuf>,
}

impl FrameSlots {
    fn write(&mut self, rgba: &[u8]) -> std::io::Result<PathBuf> {
        let slot = self.sequence % FRAME_SLOT_COUNT;
        self.sequence = self.sequence.wrapping_add(1);
        let path = frame_slot_path(slot);
        remove_file_if_present(&path)?;
        self.written.insert(path.clone());
        fs::write(&path, rgba)?;
        Ok(path)
    }

    fn cleanup(&mut self) {
        for path in self.written.drain() {
            let _ = fs::remove_file(path);
        }
    }
}

fn frame_slot_path(slot: u64) -> PathBuf {
    std::env::temp_dir().join(format!("zz-tui-{}-{slot}.rgba", std::process::id()))
}

fn remove_file_if_present(path: &std::path::Path) -> std::io::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

pub(crate) fn cleanup_frame_slot_files() {
    for slot in 0..FRAME_SLOT_COUNT {
        let _ = fs::remove_file(frame_slot_path(slot));
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct ImageKey {
    pane: PaneId,
    image_id: u32,
}

impl ImageKey {
    const fn new(pane: PaneId, image_id: u32) -> Self {
        Self { pane, image_id }
    }
}

#[derive(Debug)]
pub(crate) struct KittyImageData {
    pub pane: PaneId,
    pub image_id: u32,
    pub generation: u64,
    pub width: u32,
    pub height: u32,
    pub bytes: Vec<u8>,
}

struct ImageAssembly {
    generation: u64,
    width: u32,
    height: u32,
    total_bytes: usize,
    bytes: Vec<u8>,
}

#[derive(Default)]
pub(crate) struct KittyImageAssembler {
    assemblies: HashMap<ImageKey, ImageAssembly>,
    completed: HashMap<ImageKey, u64>,
}

impl KittyImageAssembler {
    pub fn begin(
        &mut self,
        pane: PaneId,
        image_id: u32,
        generation: u64,
        width: u32,
        height: u32,
        total_bytes: u32,
    ) {
        let key = ImageKey::new(pane, image_id);
        self.assemblies.remove(&key);
        if self.completed.get(&key) == Some(&generation) {
            return;
        }
        self.completed.remove(&key);

        let total_bytes = usize::try_from(total_bytes).unwrap_or(usize::MAX);
        let expected = usize::try_from(width)
            .ok()
            .and_then(|width| {
                usize::try_from(height)
                    .ok()
                    .and_then(|height| width.checked_mul(height))
            })
            .and_then(|pixels| pixels.checked_mul(4));
        if total_bytes == 0 || total_bytes > MAX_KITTY_IMAGE_BYTES || expected != Some(total_bytes)
        {
            log::warn!(
                "discarding malformed Kitty image {image_id} generation {generation} for {pane}"
            );
            return;
        }

        self.assemblies.insert(
            key,
            ImageAssembly {
                generation,
                width,
                height,
                total_bytes,
                bytes: Vec::with_capacity(total_bytes),
            },
        );
    }

    pub fn push_chunk(
        &mut self,
        pane: PaneId,
        image_id: u32,
        generation: u64,
        bytes: Vec<u8>,
    ) -> Option<KittyImageData> {
        let key = ImageKey::new(pane, image_id);
        let complete = {
            let assembly = self.assemblies.get_mut(&key)?;
            if assembly.generation != generation {
                return None;
            }
            if assembly
                .bytes
                .len()
                .checked_add(bytes.len())
                .is_none_or(|next| next > assembly.total_bytes)
            {
                log::warn!(
                    "discarding over-declared Kitty image {image_id} generation {generation} for {pane}"
                );
                self.assemblies.remove(&key);
                return None;
            }
            assembly.bytes.extend(bytes);
            assembly.bytes.len() == assembly.total_bytes
        };
        if !complete {
            return None;
        }

        let assembly = self
            .assemblies
            .remove(&key)
            .expect("completed Kitty assembly remains present");
        self.completed.insert(key, generation);
        Some(KittyImageData {
            pane,
            image_id,
            generation,
            width: assembly.width,
            height: assembly.height,
            bytes: assembly.bytes,
        })
    }

    pub fn remove(&mut self, pane: PaneId, image_ids: &[u32]) {
        self.assemblies
            .retain(|key, _| key.pane != pane || !image_ids.contains(&key.image_id));
        self.completed
            .retain(|key, _| key.pane != pane || !image_ids.contains(&key.image_id));
    }

    pub fn remove_pane(&mut self, pane: PaneId) {
        self.assemblies.retain(|key, _| key.pane != pane);
        self.completed.retain(|key, _| key.pane != pane);
    }

    pub fn clear(&mut self) {
        self.assemblies.clear();
        self.completed.clear();
    }
}

struct CachedImage {
    generation: u64,
    width: u32,
    height: u32,
    premultiplied_bgra: Vec<u8>,
}

#[derive(Clone, Copy)]
struct PlacedImage {
    outer_image_id: u32,
    placement_id: u32,
}

struct AppliedPane {
    rect: Rect,
    source: Vec<KittyPlacement>,
    placed: Vec<PlacedImage>,
}

pub(crate) struct KittyBridge {
    enabled: bool,
    transport: FrameTransport,
    frame_slots: FrameSlots,
    images: HashMap<ImageKey, CachedImage>,
    outer_image_ids: HashMap<ImageKey, u32>,
    next_outer_image_id: u32,
    transmitted: HashSet<(u32, u64)>,
    placement_ids: HashMap<(PaneId, usize), u32>,
    next_placement_id: u32,
    applied: HashMap<PaneId, AppliedPane>,
    visible_panes: Vec<PaneId>,
    invalidated: bool,
}

impl Default for KittyBridge {
    fn default() -> Self {
        Self {
            enabled: false,
            transport: FrameTransport::Inline,
            frame_slots: FrameSlots::default(),
            images: HashMap::new(),
            outer_image_ids: HashMap::new(),
            next_outer_image_id: 1,
            transmitted: HashSet::new(),
            placement_ids: HashMap::new(),
            next_placement_id: 1,
            applied: HashMap::new(),
            visible_panes: Vec::new(),
            invalidated: false,
        }
    }
}

impl KittyBridge {
    pub fn set_transport(&mut self, transport: FrameTransport, control: &mut Vec<u8>) {
        if self.transport == transport {
            return;
        }
        self.transport = transport;
        self.frame_slots.cleanup();
        self.transmitted.clear();
        if self.enabled {
            let keys = self.images.keys().copied().collect::<Vec<_>>();
            for key in keys {
                let _ = self.transmit(key, control);
            }
        }
    }

    pub fn enable(&mut self, control: &mut Vec<u8>) {
        if self.enabled {
            return;
        }
        self.enabled = true;
        let keys = self.images.keys().copied().collect::<Vec<_>>();
        for key in keys {
            let _ = self.transmit(key, control);
        }
    }

    pub fn disable(&mut self) {
        self.enabled = false;
        self.frame_slots.cleanup();
        self.images.clear();
        self.outer_image_ids.clear();
        self.transmitted.clear();
        self.placement_ids.clear();
        self.applied.clear();
        self.visible_panes.clear();
        self.invalidated = false;
    }

    pub fn install(&mut self, image: KittyImageData, control: &mut Vec<u8>) -> usize {
        let key = ImageKey::new(image.pane, image.image_id);
        if self
            .images
            .get(&key)
            .is_some_and(|cached| cached.generation == image.generation)
        {
            return 0;
        }
        if self.enabled {
            self.invalidate_image_placements(key, control);
        }
        if let Some(outer_image_id) = self.outer_image_ids.get(&key).copied() {
            self.transmitted
                .retain(|(transmitted_id, _)| *transmitted_id != outer_image_id);
        }
        self.images.insert(
            key,
            CachedImage {
                generation: image.generation,
                width: image.width,
                height: image.height,
                premultiplied_bgra: image.bytes,
            },
        );
        if self.enabled {
            self.transmit(key, control)
        } else {
            0
        }
    }

    pub fn remove_images(&mut self, pane: PaneId, image_ids: &[u32], control: &mut Vec<u8>) {
        for image_id in image_ids {
            let key = ImageKey::new(pane, *image_id);
            if self.enabled {
                self.invalidate_image_placements(key, control);
            }
            self.images.remove(&key);
            if let Some(outer_image_id) = self.outer_image_ids.get(&key).copied() {
                self.transmitted
                    .retain(|(transmitted_id, _)| *transmitted_id != outer_image_id);
                if self.enabled {
                    write_delete_image(control, outer_image_id);
                }
            }
        }
    }

    pub fn remove_pane(&mut self, pane: PaneId, control: &mut Vec<u8>) {
        if self.enabled {
            self.delete_applied_pane(pane, control);
        } else {
            self.applied.remove(&pane);
        }
        let keys = self
            .outer_image_ids
            .keys()
            .filter(|key| key.pane == pane)
            .copied()
            .collect::<Vec<_>>();
        for key in keys {
            self.images.remove(&key);
            if let Some(outer_image_id) = self.outer_image_ids.remove(&key) {
                self.transmitted
                    .retain(|(transmitted_id, _)| *transmitted_id != outer_image_id);
                if self.enabled {
                    write_delete_image(control, outer_image_id);
                }
            }
        }
        self.images.retain(|key, _| key.pane != pane);
        self.placement_ids.retain(|(target, _), _| *target != pane);
        self.visible_panes.retain(|target| *target != pane);
    }

    pub fn reset(&mut self, control: &mut Vec<u8>) {
        if self.enabled {
            let image_ids = self
                .outer_image_ids
                .values()
                .copied()
                .collect::<HashSet<_>>();
            for image_id in image_ids {
                write_delete_image(control, image_id);
            }
        }
        self.frame_slots.cleanup();
        self.images.clear();
        self.outer_image_ids.clear();
        self.transmitted.clear();
        self.placement_ids.clear();
        self.applied.clear();
        self.visible_panes.clear();
        self.invalidated = false;
    }

    pub const fn invalidate(&mut self) {
        self.invalidated = true;
    }

    pub fn suspend(&mut self, output: &mut Vec<u8>) {
        if !self.enabled {
            return;
        }
        self.delete_all_applied(output);
        self.invalidated = false;
    }

    pub fn reconcile<'a, I>(&mut self, panes: I, output: &mut Vec<u8>)
    where
        I: Clone + IntoIterator<Item = (PaneId, Rect, &'a [KittyPlacement])>,
    {
        if !self.enabled {
            return;
        }
        if self.invalidated {
            self.delete_all_applied(output);
            self.invalidated = false;
        }

        if !self
            .visible_panes
            .iter()
            .copied()
            .eq(panes.clone().into_iter().map(|(pane, _, _)| pane))
        {
            while let Some(stale) = self
                .applied
                .keys()
                .find(|applied| {
                    !panes
                        .clone()
                        .into_iter()
                        .any(|(visible, _, _)| visible == **applied)
                })
                .copied()
            {
                self.delete_applied_pane(stale, output);
            }
            self.visible_panes.clear();
            self.visible_panes
                .extend(panes.clone().into_iter().map(|(pane, _, _)| pane));
        }

        for (pane, rect, source) in panes {
            if self
                .applied
                .get(&pane)
                .is_some_and(|applied| applied.rect == rect && applied.source == source)
            {
                continue;
            }
            self.delete_applied_pane(pane, output);

            let mut placed = Vec::new();
            for (slot, placement) in source.iter().enumerate() {
                let key = ImageKey::new(pane, placement.image_id);
                let Some((image_width, image_height)) = self
                    .images
                    .get(&key)
                    .filter(|image| image.generation == placement.image_generation)
                    .map(|image| (image.width, image.height))
                else {
                    continue;
                };
                let Some(clipped) = clip_placement(placement, image_width, image_height, rect)
                else {
                    continue;
                };
                let outer_image_id = self.outer_image_id(key);
                let placement_id = self.placement_id(pane, slot);
                write_cursor_position(
                    output,
                    rect.x.saturating_add(clipped.column),
                    rect.y.saturating_add(clipped.row),
                );
                write!(
                    output,
                    "\x1b_Ga=p,i={outer_image_id},p={placement_id},c={},r={},x={},y={},w={},h={},X={},Y={},z={},C=1,q=2\x1b\\",
                    clipped.columns,
                    clipped.rows,
                    clipped.source_x,
                    clipped.source_y,
                    clipped.source_width,
                    clipped.source_height,
                    clipped.cell_offset_x,
                    clipped.cell_offset_y,
                    layer_z(placement.layer),
                )
                .expect("writing to Vec cannot fail");
                placed.push(PlacedImage {
                    outer_image_id,
                    placement_id,
                });
            }
            self.applied.insert(
                pane,
                AppliedPane {
                    rect,
                    source: source.to_vec(),
                    placed,
                },
            );
        }
    }

    fn transmit(&mut self, key: ImageKey, control: &mut Vec<u8>) -> usize {
        let Some((generation, width, height)) = self
            .images
            .get(&key)
            .map(|image| (image.generation, image.width, image.height))
        else {
            return 0;
        };
        let outer_image_id = self.outer_image_id(key);
        if self.transmitted.contains(&(outer_image_id, generation)) {
            return 0;
        }
        let Some(image) = self.images.get(&key) else {
            return 0;
        };
        let rgba = premultiplied_bgra_to_rgba(&image.premultiplied_bgra);
        let start = control.len();
        match self.transport {
            FrameTransport::File => {
                let path = match self.frame_slots.write(&rgba) {
                    Ok(path) => path,
                    Err(error) => {
                        log::warn!("failed to write Kitty frame slot for {key:?}: {error}");
                        return 0;
                    }
                };
                let encoded_path = STANDARD.encode(path.as_os_str().as_encoded_bytes());
                write!(
                    control,
                    "\x1b_Ga=t,f=32,t=f,i={outer_image_id},s={width},v={height},q=2;{encoded_path}\x1b\\"
                )
                .expect("writing to Vec cannot fail");
            }
            FrameTransport::Inline => {
                let compressed = compress_to_vec_zlib(&rgba, 1);
                let encoded = STANDARD.encode(compressed);
                let mut chunks = encoded.as_bytes().chunks(BASE64_CHUNK_BYTES).peekable();
                let mut first = true;
                while let Some(chunk) = chunks.next() {
                    let more = u8::from(chunks.peek().is_some());
                    if first {
                        write!(
                            control,
                            "\x1b_Ga=t,f=32,t=d,o=z,i={outer_image_id},s={width},v={height},q=2,m={more};"
                        )
                        .expect("writing to Vec cannot fail");
                        first = false;
                    } else {
                        write!(control, "\x1b_Gm={more},q=2;").expect("writing to Vec cannot fail");
                    }
                    control.extend_from_slice(chunk);
                    control.extend_from_slice(b"\x1b\\");
                }
            }
        }
        self.transmitted.insert((outer_image_id, generation));
        control.len().saturating_sub(start)
    }

    fn invalidate_image_placements(&mut self, key: ImageKey, output: &mut Vec<u8>) {
        let panes = self
            .applied
            .iter()
            .filter(|(pane, applied)| {
                **pane == key.pane
                    && applied
                        .source
                        .iter()
                        .any(|placement| placement.image_id == key.image_id)
            })
            .map(|(pane, _)| *pane)
            .collect::<Vec<_>>();
        for pane in panes {
            self.delete_applied_pane(pane, output);
        }
    }

    fn delete_all_applied(&mut self, output: &mut Vec<u8>) {
        let panes = self.applied.keys().copied().collect::<Vec<_>>();
        for pane in panes {
            self.delete_applied_pane(pane, output);
        }
    }

    fn delete_applied_pane(&mut self, pane: PaneId, output: &mut Vec<u8>) {
        let Some(applied) = self.applied.remove(&pane) else {
            return;
        };
        for placed in applied.placed {
            write!(
                output,
                "\x1b_Ga=d,d=i,i={},p={},q=2\x1b\\",
                placed.outer_image_id, placed.placement_id
            )
            .expect("writing to Vec cannot fail");
        }
    }

    fn outer_image_id(&mut self, key: ImageKey) -> u32 {
        if let Some(image_id) = self.outer_image_ids.get(&key) {
            return *image_id;
        }
        loop {
            let candidate = self.next_outer_image_id;
            self.next_outer_image_id = self.next_outer_image_id.wrapping_add(1).max(1);
            if candidate != 0
                && candidate != PROBE_IMAGE_ID
                && candidate != FILE_PROBE_IMAGE_ID
                && !self
                    .outer_image_ids
                    .values()
                    .any(|image_id| *image_id == candidate)
            {
                self.outer_image_ids.insert(key, candidate);
                return candidate;
            }
        }
    }

    fn placement_id(&mut self, pane: PaneId, slot: usize) -> u32 {
        if let Some(placement_id) = self.placement_ids.get(&(pane, slot)) {
            return *placement_id;
        }
        loop {
            let candidate = self.next_placement_id;
            self.next_placement_id = self.next_placement_id.wrapping_add(1).max(1);
            if candidate != 0
                && !self
                    .placement_ids
                    .values()
                    .any(|placement_id| *placement_id == candidate)
            {
                self.placement_ids.insert((pane, slot), candidate);
                return candidate;
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ClippedPlacement {
    column: u16,
    row: u16,
    columns: u32,
    rows: u32,
    source_x: u32,
    source_y: u32,
    source_width: u32,
    source_height: u32,
    cell_offset_x: u32,
    cell_offset_y: u32,
}

fn clip_placement(
    placement: &KittyPlacement,
    image_width: u32,
    image_height: u32,
    rect: Rect,
) -> Option<ClippedPlacement> {
    if placement.pixel_width == 0
        || placement.pixel_height == 0
        || placement.grid_cols == 0
        || placement.grid_rows == 0
    {
        return None;
    }
    let (source_x, source_y, source_width, source_height) =
        placement
            .source_rect
            .unwrap_or((0, 0, image_width, image_height));
    if source_width == 0
        || source_height == 0
        || source_x
            .checked_add(source_width)
            .is_none_or(|right| right > image_width)
        || source_y
            .checked_add(source_height)
            .is_none_or(|bottom| bottom > image_height)
    {
        return None;
    }

    let horizontal = clip_axis(placement.viewport_col, placement.grid_cols, rect.width)?;
    let vertical = clip_axis(placement.viewport_row, placement.grid_rows, rect.height)?;
    let (source_x, source_width) = crop_source_axis(
        source_x,
        source_width,
        placement.grid_cols,
        horizontal.clipped_before,
        horizontal.visible,
    )?;
    let (source_y, source_height) = crop_source_axis(
        source_y,
        source_height,
        placement.grid_rows,
        vertical.clipped_before,
        vertical.visible,
    )?;

    Some(ClippedPlacement {
        column: horizontal.start,
        row: vertical.start,
        columns: horizontal.visible,
        rows: vertical.visible,
        source_x,
        source_y,
        source_width,
        source_height,
        cell_offset_x: if horizontal.clipped_before == 0 {
            placement.cell_offset_x
        } else {
            0
        },
        cell_offset_y: if vertical.clipped_before == 0 {
            placement.cell_offset_y
        } else {
            0
        },
    })
}

struct ClippedAxis {
    start: u16,
    visible: u32,
    clipped_before: u32,
}

fn clip_axis(anchor: i32, cells: u32, extent: u16) -> Option<ClippedAxis> {
    let start = i64::from(anchor);
    let end = start.checked_add(i64::from(cells))?;
    let visible_start = start.max(0);
    let visible_end = end.min(i64::from(extent));
    if visible_start >= visible_end {
        return None;
    }
    Some(ClippedAxis {
        start: u16::try_from(visible_start).ok()?,
        visible: u32::try_from(visible_end.checked_sub(visible_start)?).ok()?,
        clipped_before: u32::try_from(visible_start.checked_sub(start)?).ok()?,
    })
}

fn crop_source_axis(
    source_start: u32,
    source_extent: u32,
    total_cells: u32,
    clipped_before: u32,
    visible_cells: u32,
) -> Option<(u32, u32)> {
    let start_offset = proportional_boundary(source_extent, clipped_before, total_cells)?;
    let visible_end = clipped_before.checked_add(visible_cells)?;
    let end_offset = proportional_boundary(source_extent, visible_end, total_cells)?;
    let cropped_extent = end_offset.checked_sub(start_offset)?;
    (cropped_extent > 0).then(|| (source_start.saturating_add(start_offset), cropped_extent))
}

fn proportional_boundary(extent: u32, cells: u32, total_cells: u32) -> Option<u32> {
    if total_cells == 0 || cells > total_cells {
        return None;
    }
    let scaled = u128::from(extent) * u128::from(cells) / u128::from(total_cells);
    u32::try_from(scaled).ok()
}

fn premultiplied_bgra_to_rgba(bytes: &[u8]) -> Vec<u8> {
    let mut rgba = Vec::with_capacity(bytes.len());
    for pixel in bytes.chunks_exact(4) {
        let [blue, green, red, alpha] = [pixel[0], pixel[1], pixel[2], pixel[3]];
        if alpha == 0 {
            rgba.extend_from_slice(&[0, 0, 0, 0]);
            continue;
        }
        rgba.extend_from_slice(&[
            unpremultiply(red, alpha),
            unpremultiply(green, alpha),
            unpremultiply(blue, alpha),
            alpha,
        ]);
    }
    rgba
}

fn unpremultiply(channel: u8, alpha: u8) -> u8 {
    let numerator = u32::from(channel) * 255 + u32::from(alpha) / 2;
    u8::try_from((numerator / u32::from(alpha)).min(255)).unwrap_or(255)
}

const fn layer_z(layer: KittyLayer) -> i32 {
    match layer {
        KittyLayer::BelowBg => Z_BELOW_BACKGROUND,
        KittyLayer::BelowText => Z_BELOW_TEXT,
        KittyLayer::AboveText => Z_ABOVE_TEXT,
    }
}

fn write_cursor_position(output: &mut Vec<u8>, column: u16, row: u16) {
    write!(
        output,
        "\x1b[{};{}H",
        row.saturating_add(1),
        column.saturating_add(1)
    )
    .expect("writing to Vec cannot fail");
}

fn write_delete_image(output: &mut Vec<u8>, image_id: u32) {
    write!(output, "\x1b_Ga=d,d=I,i={image_id},q=2\x1b\\").expect("writing to Vec cannot fail");
}

#[cfg(test)]
mod tests {
    use super::*;
    use miniz_oxide::inflate::decompress_to_vec_zlib;

    fn payload(output: &[u8]) -> &[u8] {
        let start = output.iter().position(|byte| *byte == b';').unwrap() + 1;
        let end = output[start..]
            .windows(2)
            .position(|bytes| bytes == b"\x1b\\")
            .unwrap()
            + start;
        &output[start..end]
    }

    fn placement(column: i32, row: i32, columns: u32, rows: u32) -> KittyPlacement {
        KittyPlacement {
            image_id: 9,
            image_generation: 3,
            layer: KittyLayer::AboveText,
            viewport_col: column,
            viewport_row: row,
            absolute_row: 0,
            cell_offset_x: 2,
            cell_offset_y: 3,
            grid_cols: columns,
            grid_rows: rows,
            pixel_width: 80,
            pixel_height: 40,
            source_rect: None,
        }
    }

    fn rect(width: u16, height: u16) -> Rect {
        Rect {
            width,
            height,
            ..Rect::default()
        }
    }

    #[test]
    fn premultiplied_bgra_converts_to_straight_rgba() {
        let input = [30, 20, 10, 85, 7, 11, 19, 255, 9, 8, 7, 0];
        assert_eq!(
            premultiplied_bgra_to_rgba(&input),
            [30, 60, 90, 85, 19, 11, 7, 255, 0, 0, 0, 0]
        );
    }

    #[test]
    fn clipping_a_negative_row_advances_the_source_rectangle() {
        let clipped = clip_placement(&placement(0, -1, 4, 4), 80, 40, rect(4, 3)).unwrap();
        assert_eq!(clipped.row, 0);
        assert_eq!(clipped.rows, 3);
        assert_eq!((clipped.source_y, clipped.source_height), (10, 30));
        assert_eq!(clipped.cell_offset_y, 0);
    }

    #[test]
    fn clipping_at_the_right_edge_reduces_cells_and_source_width() {
        let clipped = clip_placement(&placement(3, 0, 4, 2), 80, 40, rect(5, 2)).unwrap();
        assert_eq!((clipped.column, clipped.columns), (3, 2));
        assert_eq!((clipped.source_x, clipped.source_width), (0, 40));
    }

    #[test]
    fn a_fully_clipped_placement_is_omitted() {
        assert!(clip_placement(&placement(-4, 0, 2, 2), 80, 40, rect(5, 5)).is_none());
    }

    #[test]
    fn clipping_tightens_an_existing_source_rectangle_on_every_edge() {
        let mut source = placement(-1, -1, 4, 4);
        source.source_rect = Some((10, 20, 80, 40));
        let clipped = clip_placement(&source, 100, 80, rect(2, 2)).unwrap();
        assert_eq!((clipped.columns, clipped.rows), (2, 2));
        assert_eq!(
            (
                clipped.source_x,
                clipped.source_y,
                clipped.source_width,
                clipped.source_height,
            ),
            (30, 30, 40, 20)
        );
    }

    #[test]
    fn kitty_layers_map_to_the_settled_z_indices() {
        assert_eq!(layer_z(KittyLayer::BelowBg), -1_073_741_827);
        assert_eq!(layer_z(KittyLayer::BelowText), -1);
        assert_eq!(layer_z(KittyLayer::AboveText), 1);
    }

    #[test]
    fn inline_transmit_compresses_rgba_and_sends_each_generation_once() {
        let mut bridge = KittyBridge::default();
        let mut output = Vec::new();
        bridge.enable(&mut output);
        let image = || KittyImageData {
            pane: PaneId(1),
            image_id: 7,
            generation: 4,
            width: 1,
            height: 1,
            bytes: vec![0, 0, 255, 255],
        };
        let transmitted = bridge.install(image(), &mut output);
        assert_eq!(transmitted, output.len());
        let compressed = STANDARD.decode(payload(&output)).unwrap();
        assert_eq!(
            decompress_to_vec_zlib(&compressed).unwrap(),
            [255, 0, 0, 255]
        );
        let _ = bridge.install(image(), &mut output);

        let text = String::from_utf8(output).unwrap();
        assert_eq!(text.matches("\x1b_Ga=t").count(), 1);
        assert!(text.contains("t=d,o=z"));
    }

    #[test]
    fn file_transmit_rotates_eight_slots_and_cleanup_removes_them() {
        cleanup_frame_slot_files();
        let mut bridge = KittyBridge::default();
        let mut output = Vec::new();
        bridge.set_transport(FrameTransport::File, &mut output);
        bridge.enable(&mut output);
        let mut paths = Vec::new();

        for generation in 1..=9 {
            output.clear();
            let blue = u8::try_from(generation).unwrap();
            let transmitted = bridge.install(
                KittyImageData {
                    pane: PaneId(1),
                    image_id: 7,
                    generation,
                    width: 1,
                    height: 1,
                    bytes: vec![blue, 10, 20, 255],
                },
                &mut output,
            );
            assert_eq!(transmitted, output.len());
            let text = String::from_utf8_lossy(&output);
            assert_eq!(text.matches("\x1b_G").count(), 1);
            assert!(text.contains("t=f"));

            let decoded_path = STANDARD.decode(payload(&output)).unwrap();
            let path = PathBuf::from(String::from_utf8(decoded_path).unwrap());
            assert!(path.exists());
            assert_eq!(fs::read(&path).unwrap(), [20, 10, blue, 255]);
            paths.push(path);
        }

        assert_eq!(paths[0], paths[8]);
        assert_eq!(paths[..8].iter().collect::<HashSet<_>>().len(), 8);
        bridge.disable();
        assert!(paths.iter().all(|path| !path.exists()));
    }

    #[test]
    fn an_image_arriving_after_its_viewport_places_once() {
        let mut bridge = KittyBridge::default();
        let mut control = Vec::new();
        let mut output = Vec::new();
        let source = [placement(0, 0, 1, 1)];
        bridge.enable(&mut control);
        bridge.reconcile([(PaneId(1), rect(4, 4), source.as_slice())], &mut output);
        assert!(output.is_empty());

        bridge.install(
            KittyImageData {
                pane: PaneId(1),
                image_id: 9,
                generation: 3,
                width: 1,
                height: 1,
                bytes: vec![0, 0, 255, 255],
            },
            &mut control,
        );
        bridge.reconcile([(PaneId(1), rect(4, 4), source.as_slice())], &mut output);
        assert!(String::from_utf8_lossy(&output).contains("\x1b_Ga=p"));
        output.clear();

        bridge.reconcile([(PaneId(1), rect(4, 4), source.as_slice())], &mut output);
        assert!(output.is_empty());
    }

    #[test]
    fn leaving_the_layout_deletes_the_panes_placements() {
        let mut bridge = KittyBridge::default();
        let mut control = Vec::new();
        let mut output = Vec::new();
        let source = [placement(0, 0, 1, 1)];
        bridge.enable(&mut control);
        bridge.install(
            KittyImageData {
                pane: PaneId(1),
                image_id: 9,
                generation: 3,
                width: 1,
                height: 1,
                bytes: vec![0, 0, 255, 255],
            },
            &mut control,
        );
        bridge.reconcile([(PaneId(1), rect(4, 4), source.as_slice())], &mut output);
        output.clear();

        bridge.reconcile(
            std::iter::empty::<(PaneId, Rect, &[KittyPlacement])>(),
            &mut output,
        );
        assert!(String::from_utf8_lossy(&output).contains("\x1b_Ga=d,d=i"));
    }

    #[test]
    fn daemon_image_ids_are_namespaced_across_panes() {
        let mut bridge = KittyBridge::default();
        let mut output = Vec::new();
        bridge.enable(&mut output);
        for pane in [PaneId(1), PaneId(2)] {
            bridge.install(
                KittyImageData {
                    pane,
                    image_id: 7,
                    generation: 1,
                    width: 1,
                    height: 1,
                    bytes: vec![0, 0, 0, 255],
                },
                &mut output,
            );
        }
        assert_ne!(
            bridge.outer_image_ids[&ImageKey::new(PaneId(1), 7)],
            bridge.outer_image_ids[&ImageKey::new(PaneId(2), 7)]
        );
    }

    #[test]
    fn image_assembly_replaces_an_incomplete_generation() {
        let mut assembler = KittyImageAssembler::default();
        assembler.begin(PaneId(2), 5, 1, 1, 1, 4);
        assert!(assembler.push_chunk(PaneId(2), 5, 1, vec![1, 2]).is_none());
        assembler.begin(PaneId(2), 5, 2, 1, 1, 4);
        assert!(assembler.push_chunk(PaneId(2), 5, 1, vec![3, 4]).is_none());
        let complete = assembler
            .push_chunk(PaneId(2), 5, 2, vec![9, 8, 7, 6])
            .unwrap();
        assert_eq!(complete.generation, 2);
        assert_eq!(complete.bytes, [9, 8, 7, 6]);
    }
}
