#![cfg(all(
    feature = "cef-runtime",
    any(target_os = "macos", all(target_os = "linux", target_arch = "x86_64"))
))]

use zz_browser::{
    BrowserBootstrap, BrowserProfilePaths, RuntimePhase, bootstrap_with_profile_paths,
};

#[test]
fn preparing_and_closing_a_cold_runtime_does_not_load_chromium() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("browser");
    let paths = BrowserProfilePaths {
        profile: root.join("default"),
        root,
    };
    assert!(!chromium_is_loaded());
    let BrowserBootstrap::Runtime(mut runtime) = bootstrap_with_profile_paths(paths).unwrap()
    else {
        panic!("main process was dispatched as a subprocess");
    };
    assert_eq!(runtime.phase(), RuntimePhase::Uninitialized);
    assert_eq!(runtime.active_session_count(), 0);
    assert!(!chromium_is_loaded());
    runtime.message_pump().do_message_loop_work();
    assert!(!chromium_is_loaded());
    runtime.shutdown().unwrap();
    drop(runtime);
    assert!(!chromium_is_loaded());
    #[cfg(target_os = "linux")]
    assert!(
        !std::fs::read_to_string("/proc/self/maps")
            .unwrap()
            .contains("/libcef.so")
    );
}

#[cfg(target_os = "macos")]
#[test]
fn starting_without_a_framework_returns_an_error() {
    const CHILD: &str = "ZZ_CEF_MISSING_FRAMEWORK_TEST";
    if std::env::var_os(CHILD).is_none() {
        let directory = tempfile::tempdir().unwrap();
        let executable_directory = directory.path().join("bin");
        std::fs::create_dir(&executable_directory).unwrap();
        let executable = executable_directory.join("unbundled-test");
        std::fs::copy(std::env::current_exe().unwrap(), &executable).unwrap();
        let output = std::process::Command::new(executable)
            .args([
                "--exact",
                "starting_without_a_framework_returns_an_error",
                "--nocapture",
            ])
            .env(CHILD, "1")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "child exited with {}\nstdout: {}\nstderr: {}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
        return;
    }

    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("browser");
    let paths = BrowserProfilePaths {
        profile: root.join("default"),
        root,
    };
    assert!(!chromium_is_loaded());
    let BrowserBootstrap::Runtime(mut runtime) = bootstrap_with_profile_paths(paths).unwrap()
    else {
        panic!("main process was dispatched as a subprocess");
    };
    assert!(matches!(
        runtime.start(),
        Err(zz_browser::BrowserError::FrameworkLoad)
    ));
    assert_eq!(runtime.phase(), RuntimePhase::Failed);
    runtime.message_pump().do_message_loop_work();
    runtime.shutdown().unwrap();
    drop(runtime);
    assert!(!chromium_is_loaded());
}

#[cfg(target_os = "linux")]
fn chromium_is_loaded() -> bool {
    cef::sys::is_library_loaded()
}

#[cfg(target_os = "macos")]
#[allow(
    unsafe_code,
    reason = "read-only dyld framework lookup in an isolated test process"
)]
fn chromium_is_loaded() -> bool {
    unsafe extern "C" {
        fn NSVersionOfRunTimeLibrary(library: *const std::ffi::c_char) -> i32;
    }
    unsafe { NSVersionOfRunTimeLibrary(c"Chromium Embedded Framework".as_ptr()) != -1 }
}
