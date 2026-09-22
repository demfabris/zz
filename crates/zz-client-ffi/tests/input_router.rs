#![cfg(unix)]

use std::{
    path::{Path, PathBuf},
    process::Command,
};

fn static_library() -> PathBuf {
    let executable = std::env::current_exe().expect("test executable path");
    let deps = executable.parent().expect("deps directory");
    let debug = deps.parent().expect("target profile directory");
    let uplifted = debug.join("libzz_client_ffi.a");
    let mut candidates: Vec<PathBuf> = std::fs::read_dir(deps)
        .expect("read deps directory")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("libzz_client_ffi") && name.ends_with(".a"))
        })
        .collect();
    candidates.sort();
    candidates
        .pop()
        .or_else(|| uplifted.exists().then_some(uplifted))
        .expect("libzz_client_ffi.a was built")
}

fn compile_input_router_client(scratch: &Path) -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let binary = scratch.join("zz-input-router-client");
    let mut compiler = Command::new("cc");
    compiler
        .arg(manifest.join("tests/input_router.c"))
        .arg("-I")
        .arg(manifest.join("include"))
        .arg("-o")
        .arg(&binary)
        .arg(static_library());
    #[cfg(target_os = "macos")]
    compiler.args([
        "-framework",
        "CoreFoundation",
        "-framework",
        "Foundation",
        "-framework",
        "Security",
        "-framework",
        "CoreGraphics",
        "-framework",
        "Metal",
        "-framework",
        "QuartzCore",
        "-Wl,-dead_strip",
        "-lc++",
        "-lobjc",
        "-framework",
        "IOKit",
        "-liconv",
        "-lSystem",
        "-lc",
        "-lm",
    ]);
    #[cfg(not(target_os = "macos"))]
    compiler.args(["-lpthread", "-ldl", "-lm"]);
    let output = compiler.output().expect("run the C compiler");
    assert!(
        output.status.success(),
        "compiling the input router client failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    binary
}

#[test]
fn a_c_client_routes_input_sequences_through_the_abi() {
    let scratch = std::env::temp_dir().join(format!("zz-input-router-{}", std::process::id()));
    std::fs::create_dir_all(&scratch).expect("create scratch directory");
    let binary = compile_input_router_client(&scratch);
    let run = Command::new(&binary)
        .output()
        .expect("run the input router client");
    let _ = std::fs::remove_dir_all(&scratch);
    assert!(
        run.status.success(),
        "the C input router client failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(run.stdout, b"input router scenarios 1, 2, 5, 7, 8 passed\n");
}
