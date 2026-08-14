use std::{rc::Rc, sync::Arc};

use adw::prelude::*;
use zz_client::{ChromeAction, ViewportDamage};
use zz_protocol::{InputMessage, PaneId};
use zz_terminal::{CopyModeAction, TerminalAppearance, TerminalViewAction, TerminalViewport};

use crate::{engine::Engine, ui::terminal::TerminalView};

const PAGER_WIDTH: i32 = 820;
const PAGER_HEIGHT: i32 = 560;

/// The daemon's command-output view — what `C-b ?` and every other command with
/// output land in. It is a real terminal frozen in View mode on the daemon
/// side, so it renders through the same painter as a pane and its keys travel
/// as ordinary presses: while it is open the daemon has swapped this client's
/// key table to copy-mode, which is what makes `q` close it.
pub struct OutputPager {
    dialog: adw::Dialog,
    view: TerminalView,
    pane: PaneId,
}

impl OutputPager {
    pub fn present(
        engine: &Arc<Engine>,
        parent: &impl IsA<gtk::Widget>,
        pane: PaneId,
        appearance: TerminalAppearance,
    ) -> Self {
        let chrome: Rc<dyn Fn(ChromeAction)> = Rc::new(|action| {
            log::debug!(
                "zz-gtk ignores the {} chrome action in the output pager",
                action.name()
            );
        });
        let view = TerminalView::new_command_output(Arc::clone(engine), pane, appearance, chrome);

        let close = gtk::Button::builder().label("Close").build();
        let dismiss = Arc::clone(engine);
        close.connect_clicked(move |_| dismiss_output(&dismiss));

        let header = adw::HeaderBar::builder()
            .show_end_title_buttons(false)
            .build();
        header.set_title_widget(Some(&adw::WindowTitle::new("Command output", "q to close")));
        header.pack_end(&close);

        let toolbar = adw::ToolbarView::new();
        toolbar.add_top_bar(&header);
        toolbar.set_content(Some(&view));

        // The daemon decides when this closes — it owns the frozen view and the
        // key table that cancels it — so the dialog never closes itself.
        let dialog = adw::Dialog::builder()
            .content_width(PAGER_WIDTH)
            .content_height(PAGER_HEIGHT)
            .title("Command output")
            .can_close(false)
            .child(&toolbar)
            .build();
        dialog.present(Some(parent));
        view.grab_focus();

        Self { dialog, view, pane }
    }

    pub const fn pane(&self) -> PaneId {
        self.pane
    }

    pub fn set_appearance(&self, appearance: TerminalAppearance) {
        self.view.set_appearance(appearance);
    }

    /// Pager frames always arrive whole — the daemon never patches this lane.
    pub fn apply(&self, viewport: TerminalViewport) {
        self.view.apply_frame(viewport, &ViewportDamage::All);
    }

    pub fn close(&self) {
        self.dialog.force_close();
    }
}

/// Ask the daemon to retire the view. Cancelling the frozen copy mode is what
/// the `q` binding resolves to, so the button and the key take the same path.
fn dismiss_output(engine: &Arc<Engine>) {
    engine.send(InputMessage::CommandOutputView {
        action: TerminalViewAction::CopyMode(CopyModeAction::Cancel),
    });
}
