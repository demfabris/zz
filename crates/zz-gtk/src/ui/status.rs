use std::{cell::RefCell, rc::Rc, sync::Arc};

use adw::prelude::*;
use zz_client::{StatusBarAlignment, StatusBarClock, StatusBarModel};
use zz_protocol::CommandInvocation;

use crate::engine::{Engine, HostId};

pub struct Status {
    root: gtk::Box,
    engine: Arc<Engine>,
    model: RefCell<Option<StatusBarModel>>,
    focus_sidebar: Rc<dyn Fn()>,
    clock: gtk::Label,
}

impl Status {
    pub fn new(engine: Arc<Engine>, focus_sidebar: Rc<dyn Fn()>) -> Self {
        let clock = gtk::Label::new(None);
        clock.add_css_class("dim-label");
        let weak = clock.downgrade();
        gtk::glib::timeout_add_local(std::time::Duration::from_secs(1), move || {
            let Some(clock) = weak.upgrade() else {
                return gtk::glib::ControlFlow::Break;
            };
            refresh_clock(&clock);
            gtk::glib::ControlFlow::Continue
        });
        Self {
            clock,
            root: gtk::Box::new(gtk::Orientation::Horizontal, 4),
            engine,
            model: RefCell::new(None),
            focus_sidebar,
        }
    }

    pub fn widget(&self) -> &gtk::Box {
        &self.root
    }

    pub fn refresh(&self) {
        let host = self.engine.active_host();
        let name = (host != HostId::LOCAL)
            .then(|| self.engine.host_name(host))
            .flatten();
        let model = StatusBarModel::from_snapshot(
            &self.engine.snapshot(),
            self.engine.session_view().map(|view| view.session),
            name.as_deref(),
            crate::config::current().status,
        );
        refresh_clock(&self.clock);
        if self.model.borrow().as_ref() == Some(&model) {
            return;
        }
        while let Some(child) = self.root.first_child() {
            self.root.remove(&child);
        }
        self.root.set_halign(match model.alignment {
            StatusBarAlignment::Left => gtk::Align::Start,
            StatusBarAlignment::Center => gtk::Align::Center,
        });
        if let Some(name) = &model.session_name {
            let button = gtk::Button::builder()
                .label(name)
                .has_frame(false)
                .tooltip_text("Show Sessions")
                .build();
            let focus = Rc::clone(&self.focus_sidebar);
            button.connect_clicked(move |_| focus());
            self.root.append(&button);
        }
        let active = model
            .windows
            .iter()
            .position(|window| window.active)
            .unwrap_or(0);
        let start = active
            .saturating_sub(2)
            .min(model.windows.len().saturating_sub(5));
        for window in model.windows.iter().skip(start).take(5) {
            let badges = format!(
                "{}{}{}",
                if window.bell { " ●" } else { "" },
                if window.activity { " •" } else { "" },
                if window.agent { " ◇" } else { "" }
            );
            let button = gtk::Button::builder()
                .label(format!("{}: {}{badges}", window.index, window.name))
                .has_frame(false)
                .build();
            if window.active {
                button.add_css_class("accent");
            }
            let id = window.id;
            let engine = Arc::clone(&self.engine);
            button.connect_clicked(move |_| {
                engine.execute(CommandInvocation::new(
                    "select-window",
                    ["-t", &id.to_string()],
                ));
            });
            self.root.append(&button);
        }
        if model.windows.len() > 5 {
            let list = gtk::Box::new(gtk::Orientation::Vertical, 2);
            for window in &model.windows {
                let button = gtk::Button::with_label(&format!("{}: {}", window.index, window.name));
                button.set_has_frame(false);
                let engine = Arc::clone(&self.engine);
                let id = window.id;
                button.connect_clicked(move |_| {
                    engine.execute(CommandInvocation::new(
                        "select-window",
                        ["-t", &id.to_string()],
                    ));
                });
                list.append(&button);
            }
            let popover = gtk::Popover::builder().child(&list).build();
            self.root.append(
                &gtk::MenuButton::builder()
                    .icon_name("view-more-symbolic")
                    .tooltip_text("All Windows")
                    .popover(&popover)
                    .has_frame(false)
                    .build(),
            );
        }
        if let Some(count) = model.agent_count {
            let agents = gtk::Label::new(Some(&format!("◇ {count}")));
            agents.set_tooltip_text(Some("Active agents"));
            self.root.append(&agents);
        }
        if let Some(name) = &model.host_name {
            let host = gtk::Label::new(Some(name));
            host.add_css_class("dim-label");
            self.root.append(&host);
        }
        self.root.append(&self.clock);
        self.model.replace(Some(model));
    }
}

fn refresh_clock(label: &gtk::Label) {
    let format = match crate::config::current().status.clock {
        StatusBarClock::TwentyFourHour => "%H:%M",
        StatusBarClock::TwelveHour => "%I:%M %p",
        StatusBarClock::TimeAndDate => "%b %e %H:%M",
        StatusBarClock::Off => "",
    };
    label.set_visible(!format.is_empty());
    if let Ok(now) = gtk::glib::DateTime::now_local()
        && let Ok(text) = now.format(format)
    {
        label.set_text(&text);
    }
}
