use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Pinned ghostty commit. Update this to pull a newer version.
const GHOSTTY_REPO: &str = "https://github.com/ghostty-org/ghostty.git";
const GHOSTTY_COMMIT: &str = "20c3eae04dee606349eb21e2dd0293b203d47179";

#[derive(Clone, Copy)]
enum LinkMode {
    Dynamic,
    Static,
}

impl LinkMode {
    fn current() -> Self {
        if cfg!(feature = "link-dynamic") {
            Self::Dynamic
        } else {
            Self::Static
        }
    }

    fn artifact_kind(self) -> &'static str {
        match self {
            Self::Dynamic => "shared library",
            Self::Static => "static library",
        }
    }

    fn matches_library(self, target: &str, file_name: &str) -> bool {
        match self {
            Self::Dynamic => {
                if target.contains("darwin") {
                    file_name.starts_with("libghostty-vt") && file_name.ends_with(".dylib")
                } else if target.contains("windows") {
                    file_name == "ghostty-vt.lib"
                        || file_name == "ghostty-vt.dll"
                        || file_name == "libghostty-vt.dll.lib"
                        || file_name == "libghostty-vt.dll.a"
                } else {
                    file_name == "libghostty-vt.so" || file_name.starts_with("libghostty-vt.so.")
                }
            }
            Self::Static => {
                if target.contains("windows") {
                    file_name == "ghostty-vt-static.lib"
                } else {
                    file_name == "libghostty-vt.a"
                }
            }
        }
    }

    #[cfg(feature = "pkg-config")]
    fn pkg_config_name(self) -> &'static str {
        match self {
            Self::Dynamic => "libghostty-vt",
            Self::Static => "libghostty-vt-static",
        }
    }
}

fn main() {
    // docs.rs has no Zig toolchain. The checked-in bindings in src/bindings.rs
    // are enough for generating documentation, so skip the entire native
    // build when running under docs.rs.
    if env::var("DOCS_RS").is_ok() {
        return;
    }

    let link_mode = LinkMode::current();

    println!("cargo:rerun-if-env-changed=LIBGHOSTTY_VT_SYS_OPTIMIZE");
    println!("cargo:rerun-if-env-changed=GHOSTTY_SOURCE_DIR");
    println!("cargo:rerun-if-env-changed=GHOSTTY_ZIG_SYSTEM_DIR");
    println!("cargo:rerun-if-env-changed=TARGET");
    println!("cargo:rerun-if-env-changed=HOST");
    println!("cargo:rerun-if-env-changed=DEBUG");
    println!("cargo:rerun-if-env-changed=OPT_LEVEL");
    println!("cargo:rerun-if-changed=build.rs");

    // An explicit source override should stay authoritative even when the
    // pkg-config feature is enabled, so local Ghostty checkouts remain easy to
    // test against.
    if env::var_os("GHOSTTY_SOURCE_DIR").is_some() {
        build_vendored(link_mode);
        return;
    }

    // When the pkg-config feature is enabled, prefer an installed library over
    // fetching Ghostty. libghostty is pre-1.0, so this crate intentionally does
    // not promise compatibility with every installed C API revision.
    #[cfg(feature = "pkg-config")]
    if try_pkg_config(link_mode) {
        return;
    }

    build_vendored(link_mode);
}

/// Build libghostty-vt from source via zig. The zig build itself generates
/// shared and static artifacts plus pkg-config files in `share/pkgconfig/`.
fn build_vendored(link_mode: LinkMode) {
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR must be set"));
    let target = env::var("TARGET").expect("TARGET must be set");
    let host = env::var("HOST").expect("HOST must be set");

    // Locate ghostty source: env override > fetch into OUT_DIR.
    let source_dir = match env::var("GHOSTTY_SOURCE_DIR") {
        Ok(dir) => {
            let p = PathBuf::from(dir);
            assert!(
                p.join("build.zig").exists(),
                "GHOSTTY_SOURCE_DIR does not contain build.zig: {}",
                p.display()
            );
            p
        }
        Err(_) => fetch_ghostty(&out_dir),
    };

    let (ghostty_dir, install_prefix) = provenance_tree(&source_dir, &out_dir);

    // Build libghostty-vt via zig.
    let zig_cache_dir = out_dir.join("zig-cache");
    let zig_global_cache_dir = out_dir.join("zig-global-cache");

    let optimize = zig_optimize_mode();

    let mut build = Command::new("zig");
    build
        .arg("build")
        .arg("-Demit-lib-vt=true")
        .arg(format!("-Doptimize={optimize}"))
        .arg("-Dcpu=baseline")
        .arg("-Demit-xcframework=false")
        .arg("-Dapp-runtime=none")
        .arg("--prefix")
        .arg(&install_prefix)
        .arg("--cache-dir")
        .arg(&zig_cache_dir)
        .env("GIT_CEILING_DIRECTORIES", &out_dir)
        .current_dir(&ghostty_dir);

    // Package managers can provide Ghostty's Zig package cache ahead of time
    // and ask Zig to resolve packages from that immutable store path instead
    // of fetching during this Cargo build script.
    if let Ok(dir) = env::var("GHOSTTY_ZIG_SYSTEM_DIR") {
        assert!(
            !dir.is_empty(),
            "GHOSTTY_ZIG_SYSTEM_DIR must not be empty when set"
        );
        let zig_system_dir = PathBuf::from(dir);
        assert!(
            zig_system_dir.exists(),
            "GHOSTTY_ZIG_SYSTEM_DIR does not exist: {}",
            zig_system_dir.display()
        );
        build
            .arg("--system")
            .arg(&zig_system_dir)
            .arg("--global-cache-dir")
            .arg(&zig_global_cache_dir);
    }

    // Only pass -Dtarget when cross-compiling. For native builds, let zig
    // auto-detect the host (matches how ghostty's own CMakeLists.txt works).
    if target != host {
        let zig_target = zig_target(&target);
        build.arg(format!("-Dtarget={zig_target}"));
    }

    run(build, "zig build");
    for name in ["libghostty-vt.pc", "libghostty-vt-static.pc"] {
        let path = install_prefix.join("share/pkgconfig").join(name);
        if let Ok(contents) = std::fs::read_to_string(&path) {
            std::fs::write(path, format!("zz_capture_provenance=1\n{contents}"))
                .expect("mark terminal provenance package");
        }
    }

    let lib_dir = install_prefix.join("lib");
    let include_dir = install_prefix.join("include");
    let search_dirs = library_search_dirs(&target, &install_prefix);
    warn_unused_xcframework(&lib_dir);

    let has_requested_library = search_dirs.iter().any(|dir| {
        std::fs::read_dir(dir)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", dir.display()))
            .any(|entry| {
                let entry = entry.unwrap_or_else(|error| {
                    panic!("failed to read entry from {}: {error}", dir.display())
                });
                let file_name = entry.file_name();
                let Some(file_name) = file_name.to_str() else {
                    return false;
                };

                link_mode.matches_library(&target, file_name)
            })
    });
    assert!(
        has_requested_library,
        "expected libghostty-vt {} in one of {:?}",
        link_mode.artifact_kind(),
        search_dirs
    );
    assert!(
        include_dir.join("ghostty").join("vt.h").exists(),
        "expected header at {}",
        include_dir.join("ghostty").join("vt.h").display()
    );

    for dir in &search_dirs {
        println!("cargo:rustc-link-search=native={}", dir.display());
    }
    match link_mode {
        LinkMode::Dynamic => println!("cargo:rustc-link-lib=dylib=ghostty-vt"),
        // On MSVC the name `ghostty-vt` resolves to `ghostty-vt.lib`, the
        // import library for ghostty-vt.dll that zig installs next to the
        // static archive. rustc would bundle its short-import pseudo-objects
        // into the rlib, and link.exe stops honoring default-library
        // directives from anything loaded after them . every CRT symbol in
        // the final link goes unresolved. The static archive carries the
        // `-static` suffix there, so name it exactly.
        LinkMode::Static if target.contains("windows") => {
            println!("cargo:rustc-link-lib=static=ghostty-vt-static");
        }
        LinkMode::Static => println!("cargo:rustc-link-lib=static=ghostty-vt"),
    }
    emit_include_metadata(&[include_dir]);
}

fn warn_unused_xcframework(lib_dir: &Path) {
    let xcframework = lib_dir.join("ghostty-vt.xcframework");
    if xcframework.exists() {
        println!(
            "cargo:warning=unused libghostty-vt XCFramework emitted at {}; Cargo links the dylib or archive directly",
            xcframework.display()
        );
    }
}

#[cfg(feature = "pkg-config")]
fn try_pkg_config(link_mode: LinkMode) -> bool {
    if !matches!(
        pkg_config::get_variable(link_mode.pkg_config_name(), "zz_capture_provenance").as_deref(),
        Ok("1")
    ) {
        return false;
    }
    let mut config = pkg_config::Config::new();
    let lib = match link_mode {
        LinkMode::Dynamic => config.probe(link_mode.pkg_config_name()),
        LinkMode::Static => config
            .statik(true)
            .cargo_metadata(false)
            .probe(link_mode.pkg_config_name()),
    };
    let lib = match lib {
        Ok(lib) => lib,
        Err(_) => return false,
    };

    if let LinkMode::Static = link_mode {
        emit_static_pkg_config_metadata(&lib);
    }
    emit_include_metadata(&lib.include_paths);
    true
}

#[cfg(feature = "pkg-config")]
fn emit_static_pkg_config_metadata(lib: &pkg_config::Library) {
    for path in &lib.link_paths {
        println!("cargo:rustc-link-search=native={}", path.display());
    }
    for path in &lib.link_files {
        if let Some(parent) = path.parent() {
            println!("cargo:rustc-link-search=native={}", parent.display());
        }
    }
    for path in &lib.framework_paths {
        println!("cargo:rustc-link-search=framework={}", path.display());
    }
    for framework in &lib.frameworks {
        println!("cargo:rustc-link-lib=framework={framework}");
    }

    println!("cargo:rustc-link-lib=static=ghostty-vt");
    for library in &lib.libs {
        if library != "ghostty-vt" {
            println!("cargo:rustc-link-lib={library}");
        }
    }
    for args in &lib.ld_args {
        if !args.is_empty() {
            println!("cargo:rustc-link-arg=-Wl,{}", args.join(","));
        }
    }
}

fn emit_include_metadata(include_paths: &[PathBuf]) {
    if include_paths.is_empty() {
        return;
    }

    let joined = env::join_paths(include_paths)
        .unwrap_or_else(|error| panic!("failed to join include paths for cargo metadata: {error}"));
    println!("cargo:include={}", joined.to_string_lossy());
}

/// Decide which Zig `OptimizeMode` to pass to `zig build`.
///
/// The `LIBGHOSTTY_VT_SYS_OPTIMIZE` environment variable overrides this unconditionally; accepted
/// values are the four Zig `OptimizeMode` names (`Debug`, `ReleaseSafe`, `ReleaseFast`,
/// `ReleaseSmall`).
///
/// Defaults to `ReleaseFast` for optimized builds. If `DEBUG` is `true` (as cargo sets for the
/// `dev` profile), `ReleaseSafe` is used: it keeps Zig's runtime safety checks (the terminal
/// integrity assertions) while avoiding an unoptimized per-byte VT state machine, which parses
/// roughly 6x slower and blows the daemon's 2s command budgets under test load. Anyone actually
/// debugging the VT engine can still get an unoptimized build with
/// `LIBGHOSTTY_VT_SYS_OPTIMIZE=Debug`. Otherwise, if `OPT_LEVEL` is `s` or `z`, `ReleaseSmall`
/// is used.
fn zig_optimize_mode() -> &'static str {
    if let Ok(override_mode) = env::var("LIBGHOSTTY_VT_SYS_OPTIMIZE") {
        return match override_mode.as_str() {
            "Debug" => "Debug",
            "ReleaseSafe" => "ReleaseSafe",
            "ReleaseFast" => "ReleaseFast",
            "ReleaseSmall" => "ReleaseSmall",
            other => panic!(
                "LIBGHOSTTY_VT_SYS_OPTIMIZE must be one of Debug, ReleaseSafe, ReleaseFast, ReleaseSmall (got '{other}')"
            ),
        };
    }

    if env::var("DEBUG").as_deref() == Ok("true") {
        return "ReleaseSafe";
    }

    match env::var("OPT_LEVEL").as_deref() {
        Ok("s") | Ok("z") => "ReleaseSmall",
        _ => "ReleaseFast",
    }
}

/// Clone ghostty at the pinned commit into OUT_DIR/ghostty-src.
/// Reuses an existing clone if the commit matches.
fn fetch_ghostty(out_dir: &Path) -> PathBuf {
    let src_dir = out_dir.join("ghostty-src");
    let stamp = src_dir.join(".ghostty-commit");

    // Skip fetch if we already have the right commit.
    if stamp.exists()
        && let Ok(existing) = std::fs::read_to_string(&stamp)
        && existing.trim() == GHOSTTY_COMMIT
        && restore_in_place_patch(&src_dir)
    {
        return src_dir;
    }

    // Clean and clone fresh.
    if src_dir.exists() {
        std::fs::remove_dir_all(&src_dir)
            .unwrap_or_else(|e| panic!("failed to remove {}: {e}", src_dir.display()));
    }

    eprintln!("Fetching ghostty {GHOSTTY_COMMIT} ...");

    let mut clone = Command::new("git");
    clone
        .arg("clone")
        .arg("--filter=blob:none")
        .arg("--no-checkout")
        .arg(GHOSTTY_REPO)
        .arg(&src_dir);
    run(clone, "git clone ghostty");

    let mut checkout = Command::new("git");
    checkout
        .arg("checkout")
        .arg(GHOSTTY_COMMIT)
        .current_dir(&src_dir);
    run(checkout, "git checkout ghostty commit");

    std::fs::write(&stamp, GHOSTTY_COMMIT).unwrap_or_else(|e| panic!("failed to write stamp: {e}"));

    src_dir
}

const IN_PLACE_STAMP: &str = ".zz-provenance.patch";

fn restore_in_place_patch(source: &Path) -> bool {
    let stamp = source.join(IN_PLACE_STAMP);
    if !stamp.exists() {
        return true;
    }
    let restored = Command::new("git")
        .args(["apply", "--reverse"])
        .arg(&stamp)
        .current_dir(source)
        .status()
        .is_ok_and(|status| status.success());
    restored && std::fs::remove_file(&stamp).is_ok()
}

fn provenance_tree(source: &Path, out_dir: &Path) -> (PathBuf, PathBuf) {
    let patch = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap()).join("provenance.patch");
    println!("cargo:rerun-if-changed={}", patch.display());
    assert!(
        !source.join(IN_PLACE_STAMP).exists(),
        "{} still carries a provenance patch an older build applied in place; restore its sources",
        source.display()
    );
    let contents = std::fs::read(&patch).expect("read terminal provenance patch");
    let key = contents
        .iter()
        .chain(GHOSTTY_COMMIT.as_bytes())
        .fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
            (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3)
        });
    let name = format!("ghostty-provenance-{key:016x}");
    for entry in std::fs::read_dir(out_dir).expect("read OUT_DIR").flatten() {
        let stale = entry.file_name().to_string_lossy().into_owned();
        if stale.starts_with("ghostty-provenance-") && !stale.starts_with(&name) {
            std::fs::remove_dir_all(entry.path()).expect("remove stale provenance tree");
        }
    }
    let tree = out_dir.join(&name);
    if tree.exists() {
        std::fs::remove_dir_all(&tree).expect("remove previous provenance tree");
    }
    let patched: Vec<PathBuf> = String::from_utf8_lossy(&contents)
        .lines()
        .filter_map(|line| line.strip_prefix("+++ b/"))
        .map(PathBuf::from)
        .collect();
    mirror_source(source, &tree, Path::new(""), &patched);
    let mut apply = Command::new("git");
    apply
        .arg("apply")
        .arg(&patch)
        .env("GIT_CEILING_DIRECTORIES", out_dir)
        .current_dir(&tree);
    run(apply, "apply terminal provenance patch");
    (tree, out_dir.join(format!("{name}-install")))
}

fn mirror_source(source: &Path, tree: &Path, relative: &Path, patched: &[PathBuf]) {
    std::fs::create_dir_all(tree.join(relative)).expect("create provenance tree");
    let entries = std::fs::read_dir(source.join(relative))
        .unwrap_or_else(|error| panic!("read {}: {error}", source.join(relative).display()));
    for entry in entries {
        let entry = entry.expect("read Ghostty source entry");
        let name = entry.file_name();
        if relative.as_os_str().is_empty()
            && [".git", ".zig-cache", "zig-out", ".ghostty-commit"]
                .iter()
                .any(|skip| name == *skip)
        {
            continue;
        }
        let path = relative.join(&name);
        let (from, to) = (source.join(&path), tree.join(&path));
        let kind = entry.file_type().expect("read Ghostty source entry type");
        if kind.is_dir() {
            mirror_source(source, tree, &path, patched);
        } else if kind.is_symlink() {
            mirror_symlink(&from, &to);
        } else if patched.contains(&path) || std::fs::hard_link(&from, &to).is_err() {
            std::fs::copy(&from, &to)
                .unwrap_or_else(|error| panic!("copy {}: {error}", from.display()));
        }
    }
}

#[cfg(unix)]
fn mirror_symlink(from: &Path, to: &Path) {
    let target = std::fs::read_link(from).expect("read Ghostty source symlink");
    std::os::unix::fs::symlink(target, to).expect("mirror Ghostty source symlink");
}

#[cfg(not(unix))]
fn mirror_symlink(from: &Path, to: &Path) {
    if from.is_file() {
        std::fs::copy(from, to).expect("copy Ghostty source symlink target");
    }
}

fn run(mut command: Command, context: &str) {
    let status = command
        .status()
        .unwrap_or_else(|error| panic!("failed to execute {context}: {error}"));
    assert!(status.success(), "{context} failed with status {status}");
}

/// Returns directories to search for the built library artifact.
/// On Windows, Zig may place the DLL in `bin/` and the import lib in `lib/`,
/// so both are included.
fn library_search_dirs(target: &str, install_prefix: &Path) -> Vec<PathBuf> {
    let mut dirs = vec![install_prefix.join("lib")];
    if target.contains("windows") {
        dirs.push(install_prefix.join("bin"));
    }
    dirs
}

fn zig_target(target: &str) -> String {
    let value = match target {
        "x86_64-unknown-linux-gnu" => "x86_64-linux-gnu",
        "x86_64-unknown-linux-musl" => "x86_64-linux-musl",
        "aarch64-unknown-linux-gnu" => "aarch64-linux-gnu",
        "aarch64-unknown-linux-musl" => "aarch64-linux-musl",
        "aarch64-apple-darwin" => "aarch64-macos-none",
        "x86_64-apple-darwin" => "x86_64-macos-none",
        "x86_64-pc-windows-gnu" => "x86_64-windows-gnu",
        "aarch64-pc-windows-gnullvm" => "aarch64-windows-gnu",
        "x86_64-pc-windows-msvc" => "x86_64-windows-msvc",
        "aarch64-pc-windows-msvc" => "aarch64-windows-msvc",
        "aarch64-linux-android" => "aarch64-linux-android",
        "x86_64-linux-android" => "x86_64-linux-android",
        other => panic!("unsupported Rust target for vendored build: {other}"),
    };
    value.to_owned()
}
