#[cfg(all(feature = "hotreload", debug_assertions, not(target_family = "wasm")))]
macro_rules! render {
    ($body:block) => {
        dioxus_devtools::subsecond::call(|| zpui::IntoElement::into_any_element($body))
    };
}

#[cfg(not(all(feature = "hotreload", debug_assertions, not(target_family = "wasm"))))]
macro_rules! render {
    ($body:block) => {
        $body
    };
}

pub(crate) use render;

#[cfg(all(feature = "hotreload", debug_assertions, not(target_family = "wasm")))]
pub(crate) fn init(cx: &mut zpui::App) {
    if !zz_protocol::app_identity::DEVELOPMENT {
        return;
    }
    let (patch_applied, patches) = async_channel::bounded(1);
    dioxus_devtools::subsecond::register_handler(std::sync::Arc::new(move || {
        let _ = patch_applied.try_send(());
    }));
    cx.spawn(async move |cx| {
        while patches.recv().await.is_ok() {
            log::info!(target: "zz::hotreload", "patch applied; refreshing windows");
            cx.update(zpui::App::refresh_windows);
        }
    })
    .detach();
    dioxus_devtools::connect_subsecond();
}
