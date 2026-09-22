use async_task::Runnable;
use dispatch2::{DispatchQueue, DispatchQueueGlobalPriority, DispatchTime, GlobalQueueIdentifier};
use gpui::{PlatformDispatcher, Priority, RunnableMeta, RunnableVariant};
use objc::{
    class, msg_send,
    runtime::{BOOL, YES},
    sel, sel_impl,
};
use std::{ffi::c_void, ptr::NonNull, time::Duration};

pub(crate) struct IosDispatcher;

impl IosDispatcher {
    pub fn new() -> Self {
        Self
    }
}

impl PlatformDispatcher for IosDispatcher {
    fn is_main_thread(&self) -> bool {
        let is_main: BOOL = unsafe { msg_send![class!(NSThread), isMainThread] };
        is_main == YES
    }

    fn dispatch(&self, runnable: RunnableVariant, priority: Priority) {
        let priority = match priority {
            Priority::RealtimeAudio | Priority::High => DispatchQueueGlobalPriority::High,
            Priority::Medium => DispatchQueueGlobalPriority::Default,
            Priority::Low => DispatchQueueGlobalPriority::Low,
        };
        unsafe {
            DispatchQueue::global_queue(GlobalQueueIdentifier::Priority(priority))
                .exec_async_f(runnable.into_raw().as_ptr().cast(), trampoline);
        }
    }

    fn dispatch_on_main_thread(&self, runnable: RunnableVariant, _priority: Priority) {
        unsafe {
            DispatchQueue::main().exec_async_f(runnable.into_raw().as_ptr().cast(), trampoline);
        }
    }

    fn dispatch_after(&self, duration: Duration, runnable: RunnableVariant) {
        let queue = DispatchQueue::global_queue(GlobalQueueIdentifier::Priority(
            DispatchQueueGlobalPriority::Default,
        ));
        unsafe {
            DispatchQueue::exec_after_f(
                DispatchTime::NOW.time(duration.as_nanos().min(i64::MAX as u128) as i64),
                &queue,
                runnable.into_raw().as_ptr().cast(),
                trampoline,
            );
        }
    }

    fn spawn_realtime(&self, f: Box<dyn FnOnce() + Send>) {
        std::thread::spawn(f);
    }
}

extern "C" fn trampoline(context: *mut c_void) {
    unsafe { Runnable::<RunnableMeta>::from_raw(NonNull::new_unchecked(context.cast())) }.run();
}
