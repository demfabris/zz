use std::io::Cursor;

use async_channel::Sender;
use image::{ImageFormat, imageops::FilterType};
use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol, Sel};
use objc2::{
    AnyThread as _, DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send,
    rc::Retained, sel,
};
use objc2_app_kit::{
    NSApplication, NSEventMask, NSEventModifierFlags, NSEventType, NSImage, NSMenu, NSMenuItem,
    NSStatusBar, NSStatusItem, NSVariableStatusItemLength,
};
use objc2_foundation::{NSData, NSSize, NSString, ns_string};

use super::TrayEvent;

const TRAY_GLYPH_PNG: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/zz.icon/Assets/layer-z-1024.png"
));

// 18pt is the macOS menu bar's conventional status-item size; 36px is its 2x raster.
const TRAY_IMAGE_POINTS: f64 = 18.0;
const TRAY_IMAGE_PIXELS: u32 = 36;

/// A live status item. Dropping it removes the icon from the menu bar.
pub(super) struct StatusItem {
    item: Retained<NSStatusItem>,
    // The button holds its target unretained.
    _target: Retained<TrayTarget>,
}

impl Drop for StatusItem {
    fn drop(&mut self) {
        NSStatusBar::systemStatusBar().removeStatusItem(&self.item);
    }
}

pub(super) fn spawn(sender: Sender<TrayEvent>) -> Option<StatusItem> {
    let Some(mtm) = MainThreadMarker::new() else {
        log::warn!(target: "zz::tray", "could not create the status item outside the main thread");
        return None;
    };
    let image = tray_image()?;
    let item = NSStatusBar::systemStatusBar().statusItemWithLength(NSVariableStatusItemLength);
    let Some(button) = item.button(mtm) else {
        log::warn!(target: "zz::tray", "AppKit gave the status item no button");
        NSStatusBar::systemStatusBar().removeStatusItem(&item);
        return None;
    };
    button.setImage(Some(&image));

    let target = TrayTarget::new(sender, item.clone(), mtm);
    let any_target: &AnyObject = &target;
    #[allow(
        unsafe_code,
        reason = "target/action wiring has no safe objc2 binding: AppKit cannot type-check either half"
    )]
    // SAFETY: `trayClicked:` is declared on `TrayTarget` below, and `target` is
    // one, so the message AppKit sends on a click is one it implements.
    unsafe {
        button.setTarget(Some(any_target));
        button.setAction(Some(sel!(trayClicked:)));
    }
    // AppKit reports only the primary mouse-up by default.
    button.sendActionOn(NSEventMask::LeftMouseUp | NSEventMask::RightMouseUp);

    Some(StatusItem {
        item,
        _target: target,
    })
}

fn tray_image() -> Option<Retained<NSImage>> {
    let mut glyph = image::load_from_memory_with_format(TRAY_GLYPH_PNG, ImageFormat::Png)
        .expect("the embedded zz glyph must be a valid PNG")
        .into_rgba8();
    for pixel in glyph.chunks_exact_mut(4) {
        pixel[..3].fill(0);
    }
    let scaled = image::imageops::resize(
        &glyph,
        TRAY_IMAGE_PIXELS,
        TRAY_IMAGE_PIXELS,
        FilterType::Lanczos3,
    );

    let mut png = Vec::new();
    if let Err(error) = scaled.write_to(&mut Cursor::new(&mut png), ImageFormat::Png) {
        log::warn!(target: "zz::tray", "could not encode the tray icon: {error}");
        return None;
    }
    let image = NSImage::initWithData(NSImage::alloc(), &NSData::with_bytes(&png))?;
    image.setSize(NSSize::new(TRAY_IMAGE_POINTS, TRAY_IMAGE_POINTS));
    image.setTemplate(true);
    Some(image)
}

struct TrayIvars {
    sender: Sender<TrayEvent>,
    item: Retained<NSStatusItem>,
}

define_class!(
    // SAFETY:
    // - `NSObject` has no subclassing requirements.
    // - `TrayTarget` does not implement `Drop`.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = TrayIvars]
    struct TrayTarget;

    impl TrayTarget {
        #[unsafe(method(trayClicked:))]
        fn tray_clicked(&self, _sender: Option<&AnyObject>) {
            if self.click_was_secondary() {
                self.show_menu();
            } else {
                self.send(TrayEvent::Toggle);
            }
        }

        #[unsafe(method(trayToggle:))]
        fn tray_toggle(&self, _sender: Option<&AnyObject>) {
            self.send(TrayEvent::Toggle);
        }

        #[unsafe(method(trayQuit:))]
        fn tray_quit(&self, _sender: Option<&AnyObject>) {
            self.send(TrayEvent::Quit);
        }
    }

    unsafe impl NSObjectProtocol for TrayTarget {}
);

impl TrayTarget {
    #[allow(
        unsafe_code,
        reason = "`init` on the superclass has no safe objc2 binding"
    )]
    fn new(
        sender: Sender<TrayEvent>,
        item: Retained<NSStatusItem>,
        mtm: MainThreadMarker,
    ) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(TrayIvars { sender, item });
        // SAFETY: `NSObject`'s designated initializer, on a freshly allocated
        // instance whose ivars are already in place.
        unsafe { msg_send![super(this), init] }
    }

    fn click_was_secondary(&self) -> bool {
        NSApplication::sharedApplication(self.mtm())
            .currentEvent()
            .is_some_and(|event| {
                event.r#type() == NSEventType::RightMouseUp
                    || event
                        .modifierFlags()
                        .contains(NSEventModifierFlags::Control)
            })
    }

    #[allow(
        unsafe_code,
        reason = "`performClick:` has no safe objc2 binding: AppKit cannot know what the click will run"
    )]
    fn show_menu(&self) {
        let mtm = self.mtm();
        let menu = NSMenu::initWithTitle(NSMenu::alloc(mtm), ns_string!(""));
        menu.addItem(&self.menu_item(ns_string!("Show/Hide"), sel!(trayToggle:), mtm));
        menu.addItem(&NSMenuItem::separatorItem(mtm));
        menu.addItem(&self.menu_item(ns_string!("Quit zz"), sel!(trayQuit:), mtm));

        let item = &self.ivars().item;
        item.setMenu(Some(&menu));
        if let Some(button) = item.button(mtm) {
            // SAFETY: the click opens the menu just set, whose items target
            // this object and whose actions it implements.
            unsafe { button.performClick(None) };
        }
        // A status item that keeps a menu never fires its action again.
        item.setMenu(None);
    }

    #[allow(
        unsafe_code,
        reason = "target/action wiring has no safe objc2 binding: AppKit cannot type-check either half"
    )]
    fn menu_item(
        &self,
        title: &NSString,
        action: Sel,
        mtm: MainThreadMarker,
    ) -> Retained<NSMenuItem> {
        let target: &AnyObject = self;
        // SAFETY: every `action` passed here is declared on `TrayTarget`, which
        // is what `target` is.
        unsafe {
            let item = NSMenuItem::initWithTitle_action_keyEquivalent(
                NSMenuItem::alloc(mtm),
                title,
                Some(action),
                ns_string!(""),
            );
            item.setTarget(Some(target));
            item
        }
    }

    fn send(&self, event: TrayEvent) {
        if let Err(error) = self.ivars().sender.try_send(event) {
            log::warn!(target: "zz::tray", "dropped a tray event: {error}");
        }
    }
}
