use std::{cell::RefCell, rc::Rc, sync::Arc};

use gpui::{
    App, Bounds, Element, ElementId, ElementInputHandler, Entity, GlobalElementId,
    InspectorElementId, IntoElement, LayoutId, Pixels, RenderImage, Window,
};
use parking_lot::RwLock;
use zz_terminal::TerminalAppearance;
use zz_ui::terminal::{
    PaintState, TerminalHistoryRow, TerminalHistorySource, TerminalImageSource, TerminalRenderInput,
};

pub(crate) use zz_ui::terminal::RowRenderCache;

use crate::{
    mux::client::{HistoryRing, KittyImageCache, RetainedTerminalViewport},
    pane,
    terminal::view::TerminalView,
};

pub(crate) struct TerminalElement {
    view: Entity<TerminalView>,
    retained: Arc<RwLock<RetainedTerminalViewport>>,
    kitty_images: Arc<RwLock<KittyImageCache>>,
    row_cache: Rc<RefCell<RowRenderCache>>,
    appearance: Arc<TerminalAppearance>,
    appearance_hash: u64,
    text_opacity: f32,
    cursor_blink_visible: bool,
    marked_text: Option<String>,
}

impl TerminalElement {
    pub(crate) fn new(
        view: Entity<TerminalView>,
        retained: Arc<RwLock<RetainedTerminalViewport>>,
        kitty_images: Arc<RwLock<KittyImageCache>>,
        row_cache: Rc<RefCell<RowRenderCache>>,
        appearance: Arc<TerminalAppearance>,
        appearance_hash: u64,
        text_opacity: f32,
        cursor_blink_visible: bool,
        marked_text: Option<String>,
    ) -> Self {
        Self {
            view,
            retained,
            kitty_images,
            row_cache,
            appearance,
            appearance_hash,
            text_opacity,
            cursor_blink_visible,
            marked_text,
        }
    }
}

impl IntoElement for TerminalElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl TerminalHistorySource for HistoryRing {
    fn row_count(&self) -> usize {
        self.rows.len()
    }

    fn row(&self, index: usize) -> Option<TerminalHistoryRow<'_>> {
        self.rows.get(index).map(|row| TerminalHistoryRow {
            cells: &row.cells,
            dictionary: &row.dictionary,
            revision: row.revision,
        })
    }
}

impl TerminalImageSource for KittyImageCache {
    fn image(&self, image_id: u32, generation: u64) -> Option<Arc<RenderImage>> {
        self.image(image_id, generation)
    }
}

impl Element for TerminalElement {
    type RequestLayoutState = ();
    type PrepaintState = PaintState;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        (pane::fill_parent(window, cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        (): &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let retained = self.retained.read();
        let images = self.kitty_images.read();
        let view = self.view.read(cx);
        let focused = view.focus().is_focused(window);
        let command_output = view.is_command_output();
        let local_scroll_target = view.local_scroll_target();
        let paint = self.row_cache.borrow_mut().prepaint(
            TerminalRenderInput {
                viewport: &retained.viewport,
                row_revisions: &retained.row_revisions,
                revision_epoch: retained.row_revision_epoch,
                history: Some(&retained.history),
                images: Some(&*images),
                local_scroll_target,
                command_output,
                appearance: &self.appearance,
                appearance_hash: self.appearance_hash,
                text_opacity: self.text_opacity,
                focused,
                cursor_blink_visible: self.cursor_blink_visible,
                marked_text: self.marked_text.as_deref(),
            },
            bounds,
            window,
            cx,
        );
        let geometry = paint.geometry;
        self.view.update(cx, |view, cx| {
            view.update_geometry(
                geometry.grid,
                geometry.grid_bounds,
                geometry.surface_bounds,
                geometry.cell_width,
                geometry.line_height,
                geometry.input_bounds,
                geometry.link_hover_bounds,
                cx,
            );
        });
        paint
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        (): &mut Self::RequestLayoutState,
        paint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let focus = self.view.read(cx).focus();
        window.handle_input(
            &focus,
            ElementInputHandler::new(bounds, self.view.clone()),
            cx,
        );
        self.row_cache.borrow_mut().paint(paint, bounds, window, cx);
    }
}
