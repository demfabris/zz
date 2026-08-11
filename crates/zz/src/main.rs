use std::process::ExitCode;

// Windows declares its allocator in the library, where its application runs.
#[cfg(not(windows))]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

fn main() -> ExitCode {
    zz::run()
}
