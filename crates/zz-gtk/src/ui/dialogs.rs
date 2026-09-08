use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::Arc,
};

use adw::prelude::*;
use zz_client::{MenuKeyResult, ViewportDamage, resolve_menu_key};
use zz_protocol::{
    ConfirmAction, ConfirmState, InputMessage, MenuAction, MenuState, PaneId, PopupAction,
};
use zz_terminal::{KeyAction, TerminalViewport};

use crate::{
    engine::Engine,
    ui::{keys, terminal::TerminalView},
};

struct Popup {
    pane: PaneId,
    dialog: adw::Dialog,
    view: TerminalView,
}

pub struct Dialogs {
    engine: Arc<Engine>,
    parent: gtk::Widget,
    confirm: RefCell<Option<(ConfirmState, adw::AlertDialog)>>,
    menu: RefCell<Option<(MenuState, adw::Dialog)>>,
    popup: RefCell<Option<Popup>>,
}

impl Dialogs {
    pub fn new(engine: Arc<Engine>, parent: &impl IsA<gtk::Widget>) -> Self {
        Self {
            engine,
            parent: parent.clone().upcast(),
            confirm: RefCell::new(None),
            menu: RefCell::new(None),
            popup: RefCell::new(None),
        }
    }

    pub fn is_open(&self) -> bool {
        self.confirm.borrow().is_some()
            || self.menu.borrow().is_some()
            || self.popup.borrow().is_some()
    }

    pub fn dismiss(&self) {
        if let Some((_, dialog)) = self.confirm.borrow_mut().take() {
            dialog.force_close();
        }
        if let Some((_, dialog)) = self.menu.borrow_mut().take() {
            dialog.force_close();
        }
        if let Some(popup) = self.popup.borrow_mut().take() {
            popup.dialog.force_close();
        }
    }

    pub fn sync(&self) {
        self.sync_popup();
        self.sync_menu();
        self.sync_confirm();
    }

    pub fn apply_frame(&self, pane: PaneId, viewport: TerminalViewport, damage: &ViewportDamage) {
        if let Some(popup) = self
            .popup
            .borrow()
            .as_ref()
            .filter(|popup| popup.pane == pane)
        {
            popup.view.apply_frame(viewport, damage);
        }
    }

    fn sync_confirm(&self) {
        let desired = self.engine.confirm();
        if self.confirm.borrow().as_ref().map(|(state, _)| state) == desired.as_ref() {
            return;
        }
        if let Some((_, dialog)) = self.confirm.borrow_mut().take() {
            dialog.force_close();
        }
        let Some(state) = desired else {
            return;
        };
        let dialog = adw::AlertDialog::builder().heading(&state.prompt).build();
        dialog.add_response("no", "Cancel");
        dialog.add_response("yes", "Confirm");
        dialog.set_default_response(Some(if state.default_yes { "yes" } else { "no" }));
        dialog.set_close_response("no");
        let answered = Rc::new(Cell::new(false));
        let engine = Arc::clone(&self.engine);
        let replied = Rc::clone(&answered);
        dialog.connect_response(None, move |_, response| {
            if !replied.replace(true) && engine.confirm().is_some() {
                engine.send(InputMessage::Confirm {
                    action: ConfirmAction::Reply(response == "yes"),
                });
            }
        });
        let keyboard = gtk::EventControllerKey::new();
        keyboard.set_propagation_phase(gtk::PropagationPhase::Capture);
        let engine = Arc::clone(&self.engine);
        keyboard.connect_key_pressed(move |_, key, _, modifiers| {
            if keys::is_modifier(key) {
                return gtk::glib::Propagation::Proceed;
            }
            if let Some(pane) = engine.active_pane() {
                engine.send_key(
                    pane,
                    keys::key_input(KeyAction::Press, key, modifiers, None),
                    false,
                );
            }
            gtk::glib::Propagation::Stop
        });
        dialog.add_controller(keyboard);
        dialog.present(Some(&self.parent));
        self.confirm.replace(Some((state, dialog)));
    }

    fn sync_menu(&self) {
        let desired = self.engine.menu();
        if self.menu.borrow().as_ref().map(|(state, _)| state) == desired.as_ref() {
            return;
        }
        if let Some((_, dialog)) = self.menu.borrow_mut().take() {
            dialog.force_close();
        }
        let Some(state) = desired else {
            return;
        };
        let list = gtk::ListBox::new();
        list.add_css_class("boxed-list");
        for item in &state.items {
            if let Some(item) = item {
                let row = adw::ActionRow::builder()
                    .title(gtk::glib::markup_escape_text(&item.name))
                    .activatable(item.enabled)
                    .sensitive(item.enabled)
                    .build();
                if let Some(key) = &item.annotation {
                    row.add_suffix(&gtk::Label::new(Some(key)));
                }
                list.append(&row);
            } else {
                let row = gtk::ListBoxRow::new();
                row.set_selectable(false);
                row.set_activatable(false);
                row.set_child(Some(&gtk::Separator::new(gtk::Orientation::Horizontal)));
                list.append(&row);
            }
        }
        let selected = Rc::new(Cell::new(state.selected.map(|index| index as usize)));
        list.select_row(
            selected
                .get()
                .and_then(|index| list.row_at_index(index as i32))
                .as_ref(),
        );
        let engine = Arc::clone(&self.engine);
        list.connect_row_activated(move |_, row| {
            engine.send(InputMessage::Menu {
                action: MenuAction::Choose(row.index() as u32),
            });
        });
        let close = gtk::Button::with_label("Close");
        let engine = Arc::clone(&self.engine);
        close.connect_clicked(move |_| {
            engine.send(InputMessage::Menu {
                action: MenuAction::Cancel,
            });
        });
        let header = adw::HeaderBar::builder()
            .show_end_title_buttons(false)
            .build();
        header.set_title_widget(Some(&gtk::Label::new(Some(&state.title))));
        header.pack_end(&close);
        let toolbar = adw::ToolbarView::new();
        toolbar.add_top_bar(&header);
        toolbar.set_content(Some(
            &gtk::ScrolledWindow::builder()
                .child(&list)
                .min_content_height(80)
                .max_content_height(480)
                .propagate_natural_height(true)
                .build(),
        ));
        let dialog = adw::Dialog::builder()
            .title(&state.title)
            .child(&toolbar)
            .content_width(420)
            .can_close(false)
            .build();
        let keyboard = gtk::EventControllerKey::new();
        keyboard.set_propagation_phase(gtk::PropagationPhase::Capture);
        let engine = Arc::clone(&self.engine);
        let menu = state.clone();
        keyboard.connect_key_pressed(move |_, key, _, modifiers| {
            let input = keys::key_input(KeyAction::Press, key, modifiers, None);
            match resolve_menu_key(&menu, selected.get(), &input) {
                MenuKeyResult::Action(action) => engine.send(InputMessage::Menu { action }),
                MenuKeyResult::Select(index) => {
                    selected.set(index);
                    list.select_row(
                        index
                            .and_then(|index| list.row_at_index(index as i32))
                            .as_ref(),
                    );
                }
                MenuKeyResult::Consumed => {}
            }
            gtk::glib::Propagation::Stop
        });
        dialog.add_controller(keyboard);
        dialog.present(Some(&self.parent));
        self.menu.replace(Some((state, dialog)));
    }

    fn sync_popup(&self) {
        let state = self.engine.popup();
        if self.popup.borrow().as_ref().map(|popup| popup.pane)
            == state.as_ref().map(|state| state.pane)
        {
            return;
        }
        if let Some(popup) = self.popup.borrow_mut().take() {
            popup.dialog.force_close();
        }
        let Some(state) = state else {
            return;
        };
        let view = TerminalView::new_popup(
            Arc::clone(&self.engine),
            state.pane,
            self.engine.appearance(),
            Rc::new(|_| {}),
        );
        let close = gtk::Button::with_label("Close");
        let engine = Arc::clone(&self.engine);
        close.connect_clicked(move |_| {
            engine.send(InputMessage::Popup {
                action: PopupAction::Close,
            });
        });
        let header = adw::HeaderBar::builder()
            .show_end_title_buttons(false)
            .build();
        header.set_title_widget(Some(&gtk::Label::new(Some(&state.title))));
        header.pack_end(&close);
        let toolbar = adw::ToolbarView::new();
        toolbar.add_top_bar(&header);
        toolbar.set_content(Some(&view));
        let metrics = view.cell_metrics();
        let header_height = header.measure(gtk::Orientation::Vertical, -1).1;
        let dialog = adw::Dialog::builder()
            .title(&state.title)
            .child(&toolbar)
            .content_width((f32::from(state.width) * metrics.width).ceil() as i32)
            .content_height(
                (f32::from(state.height) * metrics.height).ceil() as i32 + header_height,
            )
            .can_close(false)
            .build();
        if let Some(viewport) = self.engine.viewport(state.pane) {
            view.apply_frame(viewport, &ViewportDamage::All);
        }
        dialog.present(Some(&self.parent));
        view.grab_focus();
        self.popup.replace(Some(Popup {
            pane: state.pane,
            dialog,
            view,
        }));
    }
}
