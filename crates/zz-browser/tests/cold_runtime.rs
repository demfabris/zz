#![cfg(all(feature = "cef-runtime", target_os = "linux", target_arch = "x86_64"))]

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
    assert!(!cef::sys::is_library_loaded());
    let BrowserBootstrap::Runtime(mut runtime) = bootstrap_with_profile_paths(paths).unwrap()
    else {
        panic!("main process was dispatched as a subprocess");
    };
    assert_eq!(runtime.phase(), RuntimePhase::Uninitialized);
    assert_eq!(runtime.active_session_count(), 0);
    assert!(!cef::sys::is_library_loaded());
    runtime.shutdown().unwrap();
    drop(runtime);
    assert!(!cef::sys::is_library_loaded());
    assert!(
        !std::fs::read_to_string("/proc/self/maps")
            .unwrap()
            .contains("/libcef.so")
    );
}
