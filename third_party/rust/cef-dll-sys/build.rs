fn main() -> anyhow::Result<()> {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") && !cfg!(feature = "dox") {
        println!("cargo::rustc-link-lib=dylib=c++");
    }
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("linux")
        || std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() != Ok("x86_64")
        || cfg!(feature = "dox")
    {
        return Ok(());
    }
    use download_cef::OsAndArch;
    use std::{
        env, fs,
        path::{Path, PathBuf},
    };

    println!("cargo::rerun-if-changed=build.rs");

    let target = env::var("TARGET")?;
    let os_arch = OsAndArch::try_from(target.as_str())?;

    println!("cargo::rerun-if-env-changed=FLATPAK");
    println!("cargo::rerun-if-env-changed=NIX_CEF_BINARY");
    println!("cargo::rerun-if-env-changed=CEF_PATH");
    let package_version = env::var("CARGO_PKG_VERSION")?;
    let cef_version = download_cef::default_version(&package_version);

    let check_archive = |path: &Path| -> anyhow::Result<()> {
        download_cef::check_archive_json(&package_version, &path.to_string_lossy())?;
        Ok(())
    };

    let resolve_cef_dir = |location: &Path| -> anyhow::Result<PathBuf> {
        let cef_dir = location.join(os_arch.to_string());

        if !fs::exists(&cef_dir)? {
            if env::var("NIX_CEF_BINARY").is_ok() {
                download_cef::install_nix_cef(&cef_version, &cef_dir, false)?;
            } else {
                use download_cef::CefIndex;

                let download_url = download_cef::default_download_url();
                let index = CefIndex::download_from(&download_url)?;
                let platform = index.platform(&target)?;
                let version = platform.version(&cef_version)?;

                let archive = version.download_archive_from(&download_url, location, false)?;
                let extracted_dir =
                    download_cef::extract_target_archive(&target, &archive, location, false)?;
                let extracted_dir_canonical = fs::canonicalize(&extracted_dir)?;
                let cef_dir_canonical = fs::canonicalize(&cef_dir)?;
                if extracted_dir_canonical != cef_dir_canonical {
                    return Err(anyhow::anyhow!(
                        "extracted dir {extracted_dir_canonical:?} does not match cef_dir {cef_dir_canonical:?}",
                    ));
                }

                version.write_archive_json(extracted_dir)?;
            }
        }

        Ok(cef_dir)
    };

    let resolve_from_versioned = |configured_path: &Path| -> anyhow::Result<PathBuf> {
        let versioned_location = configured_path.join(&cef_version);
        let resolved = resolve_cef_dir(&versioned_location)?;
        println!(
            "Using versioned CEF path from environment: {}",
            resolved.display()
        );
        check_archive(&resolved)?;
        Ok(resolved)
    };

    let download_to_versioned = |configured_path: &Path, reason: &str| -> anyhow::Result<PathBuf> {
        let versioned_location = configured_path.join(&cef_version);
        println!(
            "{reason}, downloading archive to: {}",
            versioned_location.display()
        );
        let resolved = resolve_cef_dir(&versioned_location)?;
        println!("Using downloaded CEF path: {}", resolved.display());
        Ok(resolved)
    };

    let out_dir = PathBuf::from(env::var("OUT_DIR")?);

    let cef_dir = if env::var("FLATPAK").is_ok() {
        let cef_path = String::from("/usr/lib");
        println!("Using CEF path from FLATPAK: {cef_path}");
        let cef_path = PathBuf::from(cef_path);
        check_archive(&cef_path)?;
        cef_path
    } else if let Ok(cef_path) = env::var("CEF_PATH") {
        let configured_path = PathBuf::from(cef_path);
        if fs::exists(&configured_path)? {
            let versioned_location = configured_path.join(&cef_version);
            if fs::exists(&versioned_location)? {
                resolve_from_versioned(&configured_path)?
            } else {
                println!(
                    "Using CEF path from environment: {}",
                    configured_path.display()
                );
                match check_archive(&configured_path) {
                    Ok(()) => configured_path,
                    Err(error) => download_to_versioned(
                        &configured_path,
                        &format!("CEF_PATH is invalid ({error})"),
                    )?,
                }
            }
        } else {
            download_to_versioned(&configured_path, "CEF_PATH does not exist")?
        }
    } else {
        resolve_cef_dir(&out_dir)?
    };

    let target_dir = out_dir
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap();

    let cef_dir_str = cef_dir.to_string_lossy().into_owned();

    println!("cargo::rerun-if-changed={cef_dir_str}");
    println!("cargo::rustc-env=ZZ_CEF_DISTRIBUTION_DIR={cef_dir_str}");

    copy_cef_runtime_files(&cef_dir, target_dir)?;
    Ok(())
}

fn copy_directory(src: &std::path::Path, dest: &std::path::Path) -> Result<(), std::io::Error> {
    std::fs::create_dir_all(dest)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        if entry.path().is_file() {
            let dest = dest.join(entry.file_name());
            if dest.is_file() {
                std::fs::remove_file(&dest)?;
            }
            std::fs::copy(entry.path(), dest)?;
        }
    }
    Ok(())
}

fn copy_cef_runtime_files(
    cef_dir: &std::path::Path,
    target_dir: &std::path::Path,
) -> Result<(), std::io::Error> {
    copy_directory(cef_dir, target_dir)?;

    const LOCALES_DIR: &str = "locales";
    copy_directory(&cef_dir.join(LOCALES_DIR), &target_dir.join(LOCALES_DIR))?;

    Ok(())
}
