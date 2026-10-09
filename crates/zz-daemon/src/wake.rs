#[cfg(unix)]
use std::io;

pub(crate) struct AcceptWake {
    #[cfg(unix)]
    waker: parking_lot::Mutex<Option<std::sync::Arc<mio::Waker>>>,
}

impl AcceptWake {
    pub(crate) fn new() -> Self {
        Self {
            #[cfg(unix)]
            waker: parking_lot::Mutex::new(None),
        }
    }

    #[cfg(unix)]
    pub(crate) fn install(&self, waker: std::sync::Arc<mio::Waker>) {
        *self.waker.lock() = Some(waker);
    }

    pub(crate) fn wake(&self) {
        #[cfg(unix)]
        {
            let waker = self.waker.lock().clone();
            if let Some(waker) = waker
                && let Err(error) = wake_loop(&waker)
            {
                log::warn!("could not wake the mux loop: {error}");
            }
        }
    }
}

#[cfg(unix)]
thread_local! {
    static LOOP_WAKER: std::cell::Cell<*const mio::Waker> = const { std::cell::Cell::new(std::ptr::null()) };
    static LOOP_AGAIN: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[cfg(unix)]
pub(crate) fn wake_loop(waker: &mio::Waker) -> io::Result<()> {
    let own = std::ptr::eq(LOOP_WAKER.get(), waker);
    if own && LOOP_AGAIN.replace(true) {
        return Ok(());
    }
    let woken = waker.wake();
    if own && woken.is_err() {
        LOOP_AGAIN.set(false);
    }
    woken
}

#[cfg(unix)]
pub(crate) fn clear_loop_again() {
    LOOP_AGAIN.set(false);
}

#[cfg(unix)]
pub(crate) struct LoopThread {
    waker: *const mio::Waker,
    again: bool,
}

#[cfg(unix)]
impl LoopThread {
    pub(crate) fn enter(waker: &std::sync::Arc<mio::Waker>) -> Self {
        Self {
            waker: LOOP_WAKER.replace(std::sync::Arc::as_ptr(waker)),
            again: LOOP_AGAIN.replace(false),
        }
    }
}

#[cfg(unix)]
impl Drop for LoopThread {
    fn drop(&mut self) {
        LOOP_WAKER.set(self.waker);
        LOOP_AGAIN.set(self.again);
    }
}
