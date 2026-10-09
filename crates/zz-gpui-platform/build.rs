#![allow(clippy::disallowed_methods, reason = "build scripts are exempt")]

fn main() {
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();

    #[cfg(target_os = "macos")]
    if target_os == "macos" {
        macos_build::run();
    }

    #[cfg(all(target_os = "windows", not(debug_assertions)))]
    if target_os == "windows" {
        compile_shaders();
    }

    let _ = target_os;
}

#[cfg(target_os = "macos")]
mod macos_build {
    use std::{
        env,
        path::{Path, PathBuf},
    };

    use cbindgen::Config;

    pub fn run() {
        let header_path = generate_shader_bindings();

        #[cfg(feature = "runtime_shaders")]
        emit_stitched_shaders(&header_path);
        #[cfg(not(feature = "runtime_shaders"))]
        compile_metal_shaders(&header_path);
    }

    fn generate_shader_bindings() -> PathBuf {
        let output_path = PathBuf::from(env::var("OUT_DIR").unwrap()).join("scene.h");

        let gpui_dir = find_zz_gpui_crate_dir();

        let mut config = Config {
            include_guard: Some("SCENE_H".into()),
            language: cbindgen::Language::C,
            no_includes: true,
            ..Default::default()
        };
        config.export.include.extend([
            "Bounds".into(),
            "Corners".into(),
            "Edges".into(),
            "Size".into(),
            "Pixels".into(),
            "PointF".into(),
            "Hsla".into(),
            "ContentMask".into(),
            "Uniforms".into(),
            "AtlasTile".into(),
            "PathRasterizationInputIndex".into(),
            "PathVertex_ScaledPixels".into(),
            "PathRasterizationVertex".into(),
            "ShadowInputIndex".into(),
            "Shadow".into(),
            "QuadInputIndex".into(),
            "Underline".into(),
            "UnderlineInputIndex".into(),
            "Quad".into(),
            "BorderStyle".into(),
            "SpriteInputIndex".into(),
            "MonochromeSprite".into(),
            "PolychromeSprite".into(),
            "PathSprite".into(),
            "SurfaceInputIndex".into(),
            "SurfaceBounds".into(),
            "ShaderLayerInputIndex".into(),
            "ShaderLayerBounds".into(),
            "TransformationMatrix".into(),
        ]);
        config.no_includes = true;
        config.enumeration.prefix_with_name = true;

        let mut builder = cbindgen::Builder::new();

        let crate_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());

        // Source files from zz-gpui that define types used in shaders
        let gpui_src_paths = [
            gpui_dir.join("src/scene.rs"),
            gpui_dir.join("src/geometry.rs"),
            gpui_dir.join("src/color.rs"),
            gpui_dir.join("src/window.rs"),
            gpui_dir.join("src/platform.rs"),
        ];

        // Source files from this crate
        let local_src_paths = [crate_dir.join("src/apple/metal_renderer.rs")];

        for src_path in gpui_src_paths.iter().chain(local_src_paths.iter()) {
            println!("cargo:rerun-if-changed={}", src_path.display());
            builder = builder.with_src(src_path);
        }

        builder
            .with_config(config)
            .generate()
            .expect("Unable to generate bindings")
            .write_to_file(&output_path);

        output_path
    }

    /// Locate the zz-gpui crate directory relative to this crate. Resolved at
    /// build-script runtime against this crate's manifest dir, so no checkout
    /// path is baked into a compiled artifact (which corgi rejects).
    fn find_zz_gpui_crate_dir() -> PathBuf {
        PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("../zz-gpui")
    }

    /// To enable runtime compilation, we need to "stitch" the shaders file with the generated header
    /// so that it is self-contained.
    #[cfg(feature = "runtime_shaders")]
    fn emit_stitched_shaders(header_path: &Path) {
        fn stitch_header(header: &Path, shader_path: &Path) -> std::io::Result<PathBuf> {
            let header_contents = std::fs::read_to_string(header)?;
            let shader_contents = std::fs::read_to_string(shader_path)?;
            let stitched_contents = format!("{header_contents}\n{shader_contents}");
            let out_path =
                PathBuf::from(env::var("OUT_DIR").unwrap()).join("stitched_shaders.metal");
            std::fs::write(&out_path, stitched_contents)?;
            Ok(out_path)
        }
        let shader_source_path = "./src/apple/shaders.metal";
        let shader_path = PathBuf::from(shader_source_path);
        stitch_header(header_path, &shader_path).unwrap();
        println!("cargo:rerun-if-changed={shader_source_path}");
    }

    #[cfg(not(feature = "runtime_shaders"))]
    fn compile_metal_shaders(header_path: &Path) {
        use std::process::{self, Command};
        let shader_path = "./src/apple/shaders.metal";
        let air_output_path = PathBuf::from(env::var("OUT_DIR").unwrap()).join("shaders.air");
        let metallib_output_path =
            PathBuf::from(env::var("OUT_DIR").unwrap()).join("shaders.metallib");
        println!("cargo:rerun-if-changed={}", shader_path);

        // The metal compiler records the resolved absolute path of its input
        // unconditionally. Compile a copy staged in OUT_DIR so the recorded
        // location is the build's canonical output directory, never the
        // checkout (corgi rejects artifacts that embed the build path).
        let staged_shader_path = PathBuf::from(env::var("OUT_DIR").unwrap()).join("shaders.metal");
        std::fs::copy(shader_path, &staged_shader_path).unwrap();

        let output = Command::new("xcrun")
            .args([
                "-sdk",
                "macosx",
                "metal",
                "-gline-tables-only",
                "-mmacosx-version-min=10.15.7",
                "-MO",
                "-c",
            ])
            .arg(&staged_shader_path)
            .args(["-include", header_path.to_str().unwrap(), "-o"])
            .arg(&air_output_path)
            .output()
            .unwrap();

        if !output.status.success() {
            println!(
                "cargo::error=metal shader compilation failed:\n{}",
                String::from_utf8_lossy(&output.stderr)
            );
            process::exit(1);
        }

        let output = Command::new("xcrun")
            .args(["-sdk", "macosx", "metallib"])
            .arg(&air_output_path)
            .arg("-o")
            .arg(metallib_output_path)
            .output()
            .unwrap();

        if !output.status.success() {
            println!(
                "cargo::error=metallib compilation failed:\n{}",
                String::from_utf8_lossy(&output.stderr)
            );
            process::exit(1);
        }

        // The .air intermediate records the compiler's working directory in
        // its debug info; the metallib built from it does not. Nothing reads
        // the .air after this point, so drop it rather than leave a
        // checkout-path-bearing file in OUT_DIR.
        std::fs::remove_file(&air_output_path).unwrap();
    }
}

#[cfg(all(target_os = "windows", not(debug_assertions)))]
mod shader_compilation {
    use std::{
        fs,
        io::Write,
        path::{Path, PathBuf},
        process::{self, Command},
    };

    pub fn compile_shaders() {
        let shader_path = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap())
            .join("src/windows/shaders.hlsl");
        let out_dir = std::env::var("OUT_DIR").unwrap();

        println!("cargo:rerun-if-changed={}", shader_path.display());

        // Check if fxc.exe is available
        let fxc_path = find_fxc_compiler();

        // Define all modules
        let modules = [
            "quad",
            "shadow",
            "path_rasterization",
            "path_sprite",
            "underline",
            "monochrome_sprite",
            "subpixel_sprite",
            "polychrome_sprite",
            "surface",
        ];

        let rust_binding_path = format!("{}/shaders_bytes.rs", out_dir);
        if Path::new(&rust_binding_path).exists() {
            fs::remove_file(&rust_binding_path)
                .expect("Failed to remove existing Rust binding file");
        }
        for module in modules {
            compile_shader_for_module(
                module,
                &out_dir,
                &fxc_path,
                shader_path.to_str().unwrap(),
                &rust_binding_path,
            );
        }

        {
            let shader_path = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap())
                .join("src/windows/color_text_raster.hlsl");
            compile_shader_for_module(
                "emoji_rasterization",
                &out_dir,
                &fxc_path,
                shader_path.to_str().unwrap(),
                &rust_binding_path,
            );
        }
    }

    /// Locate `binary` in the newest installed Windows SDK.
    pub fn find_latest_windows_sdk_binary(
        binary: &str,
    ) -> Result<Option<PathBuf>, Box<dyn std::error::Error>> {
        let key = windows_registry::LOCAL_MACHINE
            .open("SOFTWARE\\WOW6432Node\\Microsoft\\Microsoft SDKs\\Windows\\v10.0")?;

        let install_folder: String = key.get_string("InstallationFolder")?; // "C:\Program Files (x86)\Windows Kits\10\"
        let install_folder_bin = Path::new(&install_folder).join("bin");

        let mut versions: Vec<_> = std::fs::read_dir(&install_folder_bin)?
            .flatten()
            .filter(|entry| entry.path().is_dir())
            .filter_map(|entry| entry.file_name().into_string().ok())
            .collect();

        versions.sort_by_key(|s| {
            s.split('.')
                .filter_map(|p| p.parse().ok())
                .collect::<Vec<u32>>()
        });

        let arch = match std::env::consts::ARCH {
            "x86_64" => "x64",
            "aarch64" => "arm64",
            _ => Err(format!(
                "Unsupported architecture: {}",
                std::env::consts::ARCH
            ))?,
        };

        if let Some(highest_version) = versions.last() {
            return Ok(Some(
                install_folder_bin
                    .join(highest_version)
                    .join(arch)
                    .join(binary),
            ));
        }

        Ok(None)
    }

    /// You can set the `GPUI_FXC_PATH` environment variable to specify the path to the fxc.exe compiler.
    fn find_fxc_compiler() -> String {
        // Check environment variable
        if let Ok(path) = std::env::var("GPUI_FXC_PATH")
            && Path::new(&path).exists()
        {
            return path;
        }

        // Try to find in PATH
        // NOTE: This has to be `where.exe` on Windows, not `where`, it must be ended with `.exe`
        if let Ok(output) = std::process::Command::new("where.exe")
            .arg("fxc.exe")
            .output()
            && output.status.success()
        {
            let path = String::from_utf8_lossy(&output.stdout);
            return path.trim().to_string();
        }

        if let Ok(Some(path)) = find_latest_windows_sdk_binary("fxc.exe") {
            return path.to_string_lossy().into_owned();
        }

        panic!("Failed to find fxc.exe");
    }

    fn compile_shader_for_module(
        module: &str,
        out_dir: &str,
        fxc_path: &str,
        shader_path: &str,
        rust_binding_path: &str,
    ) {
        // Compile vertex shader
        let output_file = format!("{}/{}_vs.h", out_dir, module);
        let const_name = format!("{}_VERTEX_BYTES", module.to_uppercase());
        compile_shader_impl(
            fxc_path,
            &format!("{module}_vertex"),
            &output_file,
            &const_name,
            shader_path,
            "vs_4_1",
        );
        generate_rust_binding(&const_name, &output_file, rust_binding_path);

        // Compile fragment shader
        let output_file = format!("{}/{}_ps.h", out_dir, module);
        let const_name = format!("{}_FRAGMENT_BYTES", module.to_uppercase());
        compile_shader_impl(
            fxc_path,
            &format!("{module}_fragment"),
            &output_file,
            &const_name,
            shader_path,
            "ps_4_1",
        );
        generate_rust_binding(&const_name, &output_file, rust_binding_path);
    }

    fn compile_shader_impl(
        fxc_path: &str,
        entry_point: &str,
        output_path: &str,
        var_name: &str,
        shader_path: &str,
        target: &str,
    ) {
        let output = Command::new(fxc_path)
            .args([
                "/T",
                target,
                "/E",
                entry_point,
                "/Fh",
                output_path,
                "/Vn",
                var_name,
                "/O3",
                shader_path,
            ])
            .output();

        match output {
            Ok(result) => {
                if result.status.success() {
                    return;
                }
                println!(
                    "cargo::error=Shader compilation failed for {}:\n{}",
                    entry_point,
                    String::from_utf8_lossy(&result.stderr)
                );
                process::exit(1);
            }
            Err(e) => {
                println!("cargo::error=Failed to run fxc for {}: {}", entry_point, e);
                process::exit(1);
            }
        }
    }

    fn generate_rust_binding(const_name: &str, head_file: &str, output_path: &str) {
        let header_content = fs::read_to_string(head_file).expect("Failed to read header file");
        let const_definition = {
            let global_var_start = header_content.find("const BYTE").unwrap();
            let global_var = &header_content[global_var_start..];
            let equal = global_var.find('=').unwrap();
            global_var[equal + 1..].trim()
        };
        let rust_binding = format!(
            "const {}: &[u8] = &{}\n",
            const_name,
            const_definition.replace('{', "[").replace('}', "]")
        );
        let mut options = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(output_path)
            .expect("Failed to open Rust binding file");
        options
            .write_all(rust_binding.as_bytes())
            .expect("Failed to write Rust binding file");
    }
}

#[cfg(all(target_os = "windows", not(debug_assertions)))]
use shader_compilation::compile_shaders;
