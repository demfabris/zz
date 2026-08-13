pub(crate) mod add_host;
mod new_session;
pub(crate) mod sidebar;
mod ssh_prompt;
pub(crate) mod tree;
mod view;

pub use view::AppView;
pub(crate) use view::ClosePane;
#[cfg(not(target_os = "ios"))]
pub(crate) use view::maybe_prompt_stale_daemon;

pub fn init(cx: &mut gpui::App) {
    sidebar::init(cx);
}
