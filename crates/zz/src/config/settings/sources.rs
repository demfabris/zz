use super::*;

impl SettingsView {
    pub(super) fn reload_mux_configuration(&mut self, cx: &mut Context<Self>) {
        self.cancel_pending_mux_split();
        self.pending_file_command = None;
        config::request_daemon_reload(cx);
        cx.notify();
    }
}
