pub(crate) mod add_host;
mod new_session;
pub(crate) mod sidebar;
mod ssh_prompt;
pub(crate) mod tree;
mod view;
mod which_key;

pub use view::AppView;
/// Re-exported for the desktop-only binders (`macos_app`, the real browser view).
pub(crate) use view::ClosePane;
pub(crate) use view::maybe_prompt_stale_daemon;

pub fn init(cx: &mut zpui::App) {
    sidebar::init(cx);
}
