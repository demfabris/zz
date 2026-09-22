use crate::{id, ns_array, ns_string};
use gpui::{ClipboardEntry, ClipboardItem, Image, ImageFormat};
use objc::{
    class,
    declare::ClassDecl,
    msg_send,
    runtime::{BOOL, Object, Protocol, Sel, YES},
    sel, sel_impl,
};
use std::ffi::c_void;

const IMAGE: &str = "public.image";
const TEXT: &str = "public.plain-text";
const URL: &str = "public.url";

pub(crate) unsafe fn register(decl: &mut ClassDecl) {
    if let Some(protocol) = Protocol::get("UIDropInteractionDelegate") {
        decl.add_protocol(protocol);
    }
    decl.add_method(
        sel!(dropInteraction:canHandleSession:),
        can_handle as extern "C" fn(&Object, Sel, id, id) -> BOOL,
    );
    decl.add_method(
        sel!(dropInteraction:sessionDidUpdate:),
        proposal as extern "C" fn(&Object, Sel, id, id) -> id,
    );
    decl.add_method(
        sel!(dropInteraction:performDrop:),
        perform as extern "C" fn(&Object, Sel, id, id),
    );
}

pub(crate) unsafe fn install(view: id) {
    let interaction: id = msg_send![class!(UIDropInteraction), alloc];
    let interaction: id = msg_send![interaction, initWithDelegate: view];
    let _: () = msg_send![view, addInteraction: interaction];
    let _: () = msg_send![interaction, release];
}

extern "C" fn can_handle(_: &Object, _: Sel, _: id, session: id) -> BOOL {
    unsafe {
        let types = ns_array(&[ns_string(IMAGE), ns_string(TEXT), ns_string(URL)]);
        msg_send![session, hasItemsConformingToTypeIdentifiers: types]
    }
}

extern "C" fn proposal(_: &Object, _: Sel, _: id, _: id) -> id {
    unsafe {
        let proposal: id = msg_send![class!(UIDropProposal), alloc];
        let proposal: id = msg_send![proposal, initWithDropOperation: 2usize];
        msg_send![proposal, autorelease]
    }
}

extern "C" fn perform(this: &Object, _: Sel, _: id, session: id) {
    let view = this as *const Object as usize;
    unsafe {
        let items: id = msg_send![session, items];
        let count: usize = msg_send![items, count];
        for index in 0..count {
            let item: id = msg_send![items, objectAtIndex: index];
            let provider: id = msg_send![item, itemProvider];
            let image: BOOL = msg_send![provider, canLoadObjectOfClass: class!(UIImage)];
            if image == YES {
                let png: BOOL =
                    msg_send![provider, hasItemConformingToTypeIdentifier: ns_string("public.png")];
                load(
                    provider,
                    class!(UIImage),
                    move |image| {
                        let data: id = if png == YES {
                            UIImagePNGRepresentation(image)
                        } else {
                            UIImageJPEGRepresentation(image, 0.85)
                        };
                        let format = if png == YES {
                            ImageFormat::Png
                        } else {
                            ImageFormat::Jpeg
                        };
                        let bytes = bytes(data)?;
                        Some(ClipboardItem {
                            entries: vec![ClipboardEntry::Image(Image::from_bytes(format, bytes))],
                        })
                    },
                    view,
                );
                continue;
            }
            for class in [class!(NSString), class!(NSURL)] {
                let text: BOOL = msg_send![provider, canLoadObjectOfClass: class];
                if text == YES {
                    let is_url = std::ptr::eq(class, class!(NSURL));
                    load(
                        provider,
                        class,
                        move |object| {
                            let text: id = if is_url {
                                msg_send![object, absoluteString]
                            } else {
                                object
                            };
                            crate::nsstring_to_string(text).map(ClipboardItem::new_string)
                        },
                        view,
                    );
                    break;
                }
            }
        }
    }
}

unsafe fn load(
    provider: id,
    class: &objc::runtime::Class,
    convert: impl Fn(id) -> Option<ClipboardItem> + 'static,
    view: usize,
) {
    let completion = block2::RcBlock::new(move |object: *mut c_void, _error: *mut c_void| {
        if object.is_null() {
            return;
        }
        let Some(item) = convert(object.cast()) else {
            return;
        };
        dispatch2::DispatchQueue::main().exec_async(move || deliver(view as id, item));
    });
    let _: id = msg_send![
        provider,
        loadObjectOfClass: class
        completionHandler: &*completion as *const block2::Block<dyn Fn(*mut c_void, *mut c_void)> as id
    ];
}

fn deliver(view: id, item: ClipboardItem) {
    if !crate::window::is_live_view(view) {
        return;
    }
    let Some(state) = (unsafe { crate::window::try_window_state(&*view) }) else {
        return;
    };
    let handler = state.borrow_mut().take_input_handler();
    if let Some(mut handler) = handler {
        handler.paste(item);
        state.borrow_mut().restore_input_handler(handler);
    }
}

unsafe fn bytes(data: id) -> Option<Vec<u8>> {
    if data.is_null() {
        return None;
    }
    let pointer: *const u8 = msg_send![data, bytes];
    let length: usize = msg_send![data, length];
    (!pointer.is_null() && length > 0).then(|| std::slice::from_raw_parts(pointer, length).to_vec())
}

#[link(name = "UIKit", kind = "framework")]
unsafe extern "C" {
    fn UIImagePNGRepresentation(image: id) -> id;
    fn UIImageJPEGRepresentation(image: id, quality: f64) -> id;
}
