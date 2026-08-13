#![allow(clippy::disallowed_methods, reason = "build scripts are exempt")]

use std::{
    env,
    path::{Path, PathBuf},
};

fn main() {
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("ios") {
        return;
    }

    for framework in [
        "UIKit",
        "Foundation",
        "QuartzCore",
        "Metal",
        "CoreFoundation",
        "CoreGraphics",
        "CoreText",
        "CoreVideo",
    ] {
        println!("cargo:rustc-link-lib=framework={framework}");
    }

    let header_path = generate_shader_bindings();
    emit_stitched_shaders(&header_path);
}

fn generate_shader_bindings() -> PathBuf {
    let output_path = PathBuf::from(env::var("OUT_DIR").unwrap()).join("scene.h");

    let gpui_dir: PathBuf = gpui::GPUI_MANIFEST_DIR.into();

    let mut config = cbindgen::Config {
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
        "TransformationMatrix".into(),
    ]);
    config.no_includes = true;
    config.enumeration.prefix_with_name = true;

    let mut builder = cbindgen::Builder::new();

    let crate_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());

    let gpui_src_paths = [
        gpui_dir.join("src/scene.rs"),
        gpui_dir.join("src/geometry.rs"),
        gpui_dir.join("src/color.rs"),
        gpui_dir.join("src/window.rs"),
        gpui_dir.join("src/platform.rs"),
    ];
    let local_src_paths = [crate_dir.join("src/metal_renderer.rs")];

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

fn emit_stitched_shaders(header_path: &Path) {
    let shader_path = PathBuf::from("./src/shaders.metal");
    let header_contents = std::fs::read_to_string(header_path).unwrap();
    let shader_contents = std::fs::read_to_string(&shader_path).unwrap();
    let stitched_contents = format!("{header_contents}\n{shader_contents}");
    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap()).join("stitched_shaders.metal");
    std::fs::write(&out_path, stitched_contents).unwrap();
    println!("cargo:rerun-if-changed={}", shader_path.display());
}
