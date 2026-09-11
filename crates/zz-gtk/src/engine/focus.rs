#[derive(Default)]
pub(super) struct ClientFocus {
    desired: Option<bool>,
    sent: Option<bool>,
    ready: bool,
    pending: Option<u64>,
    recoverable: bool,
}

impl ClientFocus {
    pub fn begin(&mut self, request_id: u64) {
        if self.pending.is_none() {
            self.recoverable = self.ready;
        }
        self.ready = false;
        self.pending = Some(request_id);
    }

    pub fn reset(&mut self) {
        self.ready = false;
        self.pending = None;
        self.recoverable = false;
        self.sent = None;
    }

    pub fn attached(&mut self) {
        self.ready = true;
        self.pending = None;
        self.recoverable = false;
        self.sent = None;
    }

    pub fn failed(&mut self, request_id: u64) {
        if self.pending != Some(request_id) {
            return;
        }
        self.pending = None;
        if std::mem::take(&mut self.recoverable) {
            self.ready = true;
        } else {
            self.reset();
        }
    }

    pub fn set(&mut self, focused: bool) {
        self.desired = Some(focused);
    }

    pub fn flush(&mut self, send: impl FnOnce(bool) -> bool) {
        if let Some(focused) = self.desired
            && self.ready
            && self.sent != Some(focused)
            && send(focused)
        {
            self.sent = Some(focused);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ClientFocus;

    fn drain(focus: &mut ClientFocus) -> Vec<bool> {
        let mut sent = Vec::new();
        focus.flush(|focused| {
            sent.push(focused);
            true
        });
        sent
    }

    #[test]
    fn pending_focus_replays_once_per_attachment() {
        let mut focus = ClientFocus::default();
        focus.begin(0);
        focus.set(true);
        focus.set(false);
        assert!(drain(&mut focus).is_empty());
        focus.attached();
        assert_eq!(drain(&mut focus), [false]);
        assert!(drain(&mut focus).is_empty());
        focus.begin(0);
        focus.attached();
        assert_eq!(drain(&mut focus), [false]);
    }

    #[test]
    fn failed_attach_restores_ready_and_preserves_unrelated_pending_requests() {
        let mut focus = ClientFocus::default();
        focus.set(true);
        focus.attached();
        assert_eq!(drain(&mut focus), [true]);
        focus.begin(7);
        focus.set(false);
        focus.failed(0);
        assert!(drain(&mut focus).is_empty());
        focus.failed(7);
        assert_eq!(drain(&mut focus), [false]);
        assert!(drain(&mut focus).is_empty());
    }

    #[test]
    fn reconnect_cannot_restore_a_dead_attachment() {
        let mut focus = ClientFocus::default();
        focus.set(false);
        focus.attached();
        assert_eq!(drain(&mut focus), [false]);
        focus.reset();
        focus.begin(0);
        focus.failed(0);
        assert!(drain(&mut focus).is_empty());
        focus.attached();
        assert_eq!(drain(&mut focus), [false]);
    }

    #[test]
    fn failed_send_is_retried_without_marking_focus_delivered() {
        let mut focus = ClientFocus::default();
        focus.set(true);
        focus.attached();
        focus.flush(|_| false);
        assert_eq!(drain(&mut focus), [true]);
    }
}
