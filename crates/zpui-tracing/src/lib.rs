pub use tracing::{Level, field};

#[cfg(all(target_family = "wasm", feature = "web"))]
pub use tracing::{
    Span, debug_span, error_span, event, info_span, instrument, span, trace_span, warn_span,
};

#[cfg(not(all(target_family = "wasm", feature = "web")))]
pub use ztracing_macro::instrument;

#[cfg(not(all(target_family = "wasm", feature = "web")))]
pub use __consume_all_tokens as trace_span;
#[cfg(not(all(target_family = "wasm", feature = "web")))]
pub use __consume_all_tokens as info_span;
#[cfg(not(all(target_family = "wasm", feature = "web")))]
pub use __consume_all_tokens as debug_span;
#[cfg(not(all(target_family = "wasm", feature = "web")))]
pub use __consume_all_tokens as warn_span;
#[cfg(not(all(target_family = "wasm", feature = "web")))]
pub use __consume_all_tokens as error_span;
#[cfg(not(all(target_family = "wasm", feature = "web")))]
pub use __consume_all_tokens as event;
#[cfg(not(all(target_family = "wasm", feature = "web")))]
pub use __consume_all_tokens as span;

#[cfg(not(all(target_family = "wasm", feature = "web")))]
#[macro_export]
macro_rules! __consume_all_tokens {
    ($($t:tt)*) => {
        $crate::Span
    };
}

#[cfg(not(all(target_family = "wasm", feature = "web")))]
pub struct Span;

#[cfg(not(all(target_family = "wasm", feature = "web")))]
impl Span {
    pub fn current() -> Self {
        Self
    }

    pub fn enter(&self) {}

    pub fn record<T, S>(&self, _t: T, _s: S) {}
}

#[cfg(all(target_family = "wasm", feature = "web"))]
mod web;

#[cfg(all(target_family = "wasm", feature = "web"))]
pub use web::{PerformanceLayer, PerformanceReporter, performance_layer};

#[cfg(all(target_family = "wasm", feature = "web"))]
pub fn init(start_reporter: impl FnOnce(PerformanceReporter)) {
    use tracing_subscriber::prelude::*;

    let (performance_layer, reporter) = performance_layer();
    if tracing::subscriber::set_global_default(
        tracing_subscriber::registry().with(performance_layer),
    )
    .is_ok()
    {
        start_reporter(reporter);
    } else {
        zlog::error!("failed to set browser performance tracing subscriber");
    }
}

#[cfg(not(all(target_family = "wasm", feature = "web")))]
pub fn init() {}
