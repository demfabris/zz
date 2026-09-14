use std::{env, error::Error, fs, path::PathBuf, process::ExitCode};

#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::collections::HashMap;
#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
use std::path::Path;
#[cfg(target_os = "macos")]
use std::{collections::BTreeSet, io};
#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
use std::{
    ffi::OsString,
    io::BufReader,
    process::{Command, Stdio},
};

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
use cargo_metadata::Message;

const APP_NAME: &str = "zz";
#[cfg(any(target_os = "linux", target_os = "macos"))]
const CLI_NAME: &str = "zz_cli";
#[cfg(any(target_os = "linux", target_os = "macos"))]
const CLI_BUNDLE_NAME: &str = "cli";
#[cfg(target_os = "macos")]
const BUNDLE_SHORT_VERSION_KEY: &str = "CFBundleShortVersionString";
#[cfg(target_os = "macos")]
const BUNDLE_VERSION_KEY: &str = "CFBundleVersion";
#[cfg(target_os = "macos")]
const PROFILING_PROFILE: &str = "profiling";
#[cfg(target_os = "macos")]
const GHOSTTY_OPTIMIZE_ENV: &str = "LIBGHOSTTY_VT_SYS_OPTIMIZE";
#[cfg(target_os = "macos")]
const GHOSTTY_RELEASE_FAST: &str = "ReleaseFast";
#[cfg(target_os = "macos")]
const MACOS_HELPER_NAME: &str = "zz_helper";
#[cfg(target_os = "macos")]
const MACOS_CEF_FRAMEWORK: &str = "Contents/Frameworks/Chromium Embedded Framework.framework";
#[cfg(target_os = "macos")]
const MACOS_CEF_LOCALE: &str = "en.lproj";
#[cfg(any(target_os = "linux", target_os = "windows"))]
const CEF_LOCALES_DIR: &str = "locales";
#[cfg(any(target_os = "linux", target_os = "windows"))]
const CEF_LOCALE: &str = "en-US.pak";
#[cfg(target_os = "macos")]
const MACOS_HELPER_SUFFIXES: [&str; 5] = [
    "Helper",
    "Helper (GPU)",
    "Helper (Renderer)",
    "Helper (Plugin)",
    "Helper (Alerts)",
];
#[cfg(target_os = "macos")]
const MACOS_FILE_QUARANTINE_KEY: &str = "LSFileQuarantineEnabled";
#[cfg(target_os = "macos")]
const MACOS_APP_DATA_USAGE_DESCRIPTION_KEY: &str = "NSAppDataUsageDescription";
#[cfg(target_os = "macos")]
const MACOS_APP_DATA_USAGE_DESCRIPTION: &str = "Import cookies and browsing history from a Chrome profile you choose. Chrome data is read-only and accessed only when you start an import.";
#[cfg(target_os = "macos")]
const MACOS_ICON_FILE: &str = "zz.icns";
#[cfg(target_os = "macos")]
const MACOS_ICON_KEY: &str = "CFBundleIconFile";
#[cfg(target_os = "macos")]
const MACOS_ICON_NAME_KEY: &str = "CFBundleIconName";
#[cfg(target_os = "macos")]
const MACOS_ICON_NAME: &str = "zz";
#[cfg(target_os = "macos")]
const MACOS_LOCAL_SIGN_IDENTITY_ENV: &str = "MACOS_LOCAL_SIGN_IDENTITY";
#[cfg(target_os = "macos")]
const MACOS_BUILD_VERSION_ENV: &str = "ZZ_MACOS_BUILD_VERSION";
/// CEF's prebuilt windowed launcher. Renamed to `zz.exe`, which is how it finds
/// `zz.dll`: it derives the client library from its own file name.
#[cfg(target_os = "windows")]
const WINDOWS_BOOTSTRAP_NAME: &str = "bootstrap.exe";
#[cfg(target_os = "windows")]
const WINDOWS_ICON_SOURCE: &str = "assets/windows/zz.ico";
/// The icon group id `LoadImage(module, MAKEINTRESOURCE(1), IMAGE_ICON, ..)`
/// resolves, which is how gpui asks the executable for its window icon.
#[cfg(target_os = "windows")]
const WINDOWS_MAIN_ICON_ID: u32 = 1;
/// The name `editpe` files a new icon group under.
#[cfg(target_os = "windows")]
const WINDOWS_EDITPE_ICON_GROUP: &str = "MAINICON";
#[cfg(target_os = "windows")]
const WINDOWS_DPI_AWARENESS: &str = "PerMonitorV2";
/// CEF's manifest plus the per-monitor DPI declaration gpui needs.
#[cfg(target_os = "windows")]
const WINDOWS_APP_MANIFEST: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <compatibility xmlns="urn:schemas-microsoft-com:compatibility.v1">
    <application>
      <!-- Windows Vista, 7, 8, 8.1, and 10/11. -->
      <supportedOS Id="{e2011457-1546-43c5-a5fe-008deee3d3f0}"/>
      <supportedOS Id="{35138b9a-5d96-4fbd-8e2d-a2440225f93a}"/>
      <supportedOS Id="{4a2f28e3-53b9-4441-ba9c-d69d4a4a6e38}"/>
      <supportedOS Id="{1f676c76-80e1-4239-95bb-83d0f6d0da78}"/>
      <supportedOS Id="{8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a}"/>
      <maxversiontested Id="10.0.18362.0"/>
    </application>
  </compatibility>
  <application xmlns="urn:schemas-microsoft-com:asm.v3">
    <windowsSettings>
      <dpiAware xmlns="http://schemas.microsoft.com/SMI/2005/WindowsSettings">true/pm</dpiAware>
      <dpiAwareness xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">PerMonitorV2</dpiAwareness>
    </windowsSettings>
  </application>
  <dependency>
    <dependentAssembly>
      <assemblyIdentity type="Win32" name="Microsoft.Windows.Common-Controls" version="6.0.0.0" processorArchitecture="*" publicKeyToken="6595b64144ccf1df" language="*"/>
    </dependentAssembly>
  </dependency>
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
    <security>
      <requestedPrivileges>
        <requestedExecutionLevel level="asInvoker" uiAccess="false"/>
      </requestedPrivileges>
    </security>
  </trustInfo>
</assembly>
"#;

#[derive(Clone, Debug, Eq, PartialEq)]
enum BuildProfile {
    Development,
    Release,
    Named(String),
}

impl BuildProfile {
    #[cfg(any(target_os = "linux", target_os = "windows"))]
    fn release_flag(&self) -> Option<bool> {
        match self {
            Self::Development => Some(false),
            Self::Release => Some(true),
            Self::Named(_) => None,
        }
    }

    #[cfg(target_os = "macos")]
    fn configure_cargo(&self, command: &mut Command) {
        match self {
            Self::Development => {}
            Self::Release => {
                command.arg("--release");
            }
            Self::Named(profile) => {
                command.arg("--profile").arg(profile);
            }
        }
    }

    #[cfg(target_os = "macos")]
    fn is_named(&self, expected: &str) -> bool {
        matches!(self, Self::Named(profile) if profile == expected)
    }

    #[cfg(target_os = "macos")]
    fn ghostty_optimize_override(&self) -> Option<&'static str> {
        self.is_named(PROFILING_PROFILE)
            .then_some(GHOSTTY_RELEASE_FAST)
    }
}

#[derive(Debug, Eq, PartialEq)]
struct BundleOptions {
    profile: BuildProfile,
    output: PathBuf,
    features: Option<String>,
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("xtask failed: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let mut args = env::args().skip(1);
    match args.next().as_deref() {
        Some("bundle-cef") => bundle_cef(&args.collect::<Vec<_>>()),
        Some("verify-cef-bundle") => {
            let path = args
                .next()
                .ok_or("usage: cargo xtask verify-cef-bundle <bundle path>")?;
            if args.next().is_some() {
                return Err("verify-cef-bundle accepts exactly one path".into());
            }
            verify_bundle(&PathBuf::from(path))
        }
        _ => Err(
            "usage: cargo xtask <bundle-cef [--release | --profile NAME] [--output DIR] \
             [--features LIST] | verify-cef-bundle PATH>"
                .into(),
        ),
    }
}

#[cfg(target_os = "macos")]
fn product_version(version: &str) -> &str {
    version.split(['-', '+']).next().unwrap_or(version)
}

fn bundle_cef(args: &[String]) -> Result<(), Box<dyn Error>> {
    let options = parse_bundle_options(args)?;
    fs::create_dir_all(&options.output)?;

    #[cfg(target_os = "linux")]
    let executable = {
        let release = options
            .profile
            .release_flag()
            .ok_or("named Cargo profiles are currently supported only for macOS CEF bundles")?;
        let target_path = build_linux_binaries(release, options.features.as_deref())?;
        let previous = options.output.join(APP_NAME);
        if previous.exists() {
            fs::remove_file(&previous)?;
        }
        let executable = cef::build_util::linux::bundle(&options.output, &target_path, APP_NAME)?;
        prune_locales(&options.output.join(CEF_LOCALES_DIR), "pak", CEF_LOCALE)?;
        if release {
            strip_library(&options.output.join("libcef.so"))?;
        }
        fs::copy(
            target_path.join(CLI_NAME),
            options.output.join(CLI_BUNDLE_NAME),
        )?;
        executable
    };

    #[cfg(target_os = "windows")]
    let executable = {
        let release = options
            .profile
            .release_flag()
            .ok_or("named Cargo profiles are currently supported only for macOS CEF bundles")?;
        let target_path = build_windows_library(release, options.features.as_deref())?;
        bundle_windows(&options.output, &target_path)?
    };

    #[cfg(target_os = "macos")]
    let executable = build_macos_bundle(
        &options.output,
        &options.profile,
        options.features.as_deref(),
    )?;

    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    compile_error!("CEF bundling is supported only on Linux, macOS, and Windows");

    #[cfg(target_os = "macos")]
    configure_macos_main_app(&executable)?;
    #[cfg(target_os = "macos")]
    configure_macos_bundle_versions(&executable)?;
    #[cfg(target_os = "windows")]
    configure_windows_executable(&executable)?;
    install_cef_notices(&executable)?;
    #[cfg(target_os = "macos")]
    sign_macos_bundle(&executable)?;
    verify_bundle(&executable)?;
    println!("CEF bundle ready: {}", executable.display());
    Ok(())
}

fn parse_bundle_options(args: &[String]) -> Result<BundleOptions, String> {
    let mut profile = BuildProfile::Development;
    let mut profile_was_set = false;
    let mut output = PathBuf::from("dist/zz");
    let mut features: Option<String> = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--release" => {
                if profile_was_set {
                    return Err("--release and --profile are mutually exclusive".to_owned());
                }
                profile = BuildProfile::Release;
                profile_was_set = true;
            }
            "--profile" => {
                if profile_was_set {
                    return Err("--release and --profile are mutually exclusive".to_owned());
                }
                index += 1;
                let name = args.get(index).ok_or("--profile requires a name")?;
                if name.is_empty() || name.starts_with('-') {
                    return Err("--profile requires a non-empty Cargo profile name".to_owned());
                }
                profile = BuildProfile::Named(name.clone());
                profile_was_set = true;
            }
            "--output" => {
                index += 1;
                output = PathBuf::from(args.get(index).ok_or("--output requires a directory")?);
            }
            "--features" => {
                index += 1;
                let list = args
                    .get(index)
                    .ok_or("--features requires a feature list")?;
                if list.is_empty() || list.starts_with('-') {
                    return Err("--features requires a non-empty feature list".to_owned());
                }
                features = Some(match features {
                    None => list.clone(),
                    Some(existing) => format!("{existing},{list}"),
                });
            }
            "--" => {}
            argument => return Err(format!("unknown bundle-cef argument: {argument}")),
        }
        index += 1;
    }
    Ok(BundleOptions {
        profile,
        output,
        features,
    })
}

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
fn merged_features(cli: Option<&str>) -> Option<OsString> {
    let env = env::var_os("ZZ_CARGO_FEATURES").filter(|value| !value.is_empty());
    match (cli, env) {
        (None, None) => None,
        (Some(cli), None) => Some(cli.into()),
        (None, Some(env)) => Some(env),
        (Some(cli), Some(env)) => {
            let mut merged = env;
            merged.push(",");
            merged.push(cli);
            Some(merged)
        }
    }
}

#[cfg(target_os = "linux")]
fn build_linux_binaries(release: bool, features: Option<&str>) -> Result<PathBuf, Box<dyn Error>> {
    println!("Building {APP_NAME} and {CLI_NAME}...");
    let mut command = Command::new(env::var_os("CARGO").unwrap_or_else(|| "cargo".into()));
    command.arg("build");
    if release {
        command.arg("--release");
    }
    if let Some(features) = merged_features(features) {
        command.arg("--features").arg(features);
    }
    command
        .args([
            "--message-format=json-render-diagnostics",
            "--bin",
            APP_NAME,
            "--bin",
            CLI_NAME,
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());

    let mut child = command.spawn()?;
    let stdout = child.stdout.take().ok_or("cargo stdout is unavailable")?;
    let mut executables = HashMap::new();
    for message in Message::parse_stream(BufReader::new(stdout)) {
        if let Message::CompilerArtifact(artifact) = message?
            && matches!(artifact.target.name.as_str(), APP_NAME | CLI_NAME)
            && let Some(path) = artifact.executable
        {
            executables.insert(artifact.target.name, path.into_std_path_buf());
        }
    }
    let status = child.wait()?;
    if !status.success() {
        return Err(format!("cargo build failed with {status}").into());
    }
    let executable = executables
        .remove(APP_NAME)
        .ok_or("cargo did not emit the zz executable")?;
    let target_path = executable
        .parent()
        .ok_or("zz executable has no parent directory")?
        .to_owned();
    let cli = executables
        .remove(CLI_NAME)
        .ok_or("cargo did not emit the zz CLI launcher")?;
    if cli.parent() != Some(target_path.as_path()) {
        return Err("zz and its CLI launcher were emitted into different directories".into());
    }
    Ok(target_path)
}

#[cfg(target_os = "windows")]
fn build_windows_library(release: bool, features: Option<&str>) -> Result<PathBuf, Box<dyn Error>> {
    println!("Building {APP_NAME}.dll...");
    let mut command = Command::new(env::var_os("CARGO").unwrap_or_else(|| "cargo".into()));
    command.arg("build");
    if release {
        command.arg("--release");
    }
    if let Some(features) = merged_features(features) {
        command.arg("--features").arg(features);
    }
    command
        .args([
            "--message-format=json-render-diagnostics",
            "--package",
            APP_NAME,
            "--lib",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());

    let mut child = command.spawn()?;
    let stdout = child.stdout.take().ok_or("cargo stdout is unavailable")?;
    let mut library = None;
    for message in Message::parse_stream(BufReader::new(stdout)) {
        if let Message::CompilerArtifact(artifact) = message?
            && artifact.target.name == APP_NAME
            && let Some(path) = artifact
                .filenames
                .iter()
                .find(|path| path.extension() == Some("dll"))
        {
            library = Some(path.clone().into_std_path_buf());
        }
    }
    let status = child.wait()?;
    if !status.success() {
        return Err(format!("cargo build failed with {status}").into());
    }
    let library = library.ok_or("cargo did not emit the zz library")?;
    Ok(library
        .parent()
        .ok_or("the zz library has no parent directory")?
        .to_owned())
}

#[cfg(target_os = "windows")]
fn bundle_windows(output: &Path, target_path: &Path) -> Result<PathBuf, Box<dyn Error>> {
    let cef_dir = cef::sys::get_cef_dir().ok_or("CEF distribution path is unavailable")?;
    copy_directory_files(&cef_dir, output)?;
    copy_directory_files(
        &cef_dir.join(CEF_LOCALES_DIR),
        &output.join(CEF_LOCALES_DIR),
    )?;
    prune_locales(&output.join(CEF_LOCALES_DIR), "pak", CEF_LOCALE)?;

    let executable = output.join(format!("{APP_NAME}.exe"));
    fs::copy(cef_dir.join(WINDOWS_BOOTSTRAP_NAME), &executable)?;

    let library = format!("{APP_NAME}.dll");
    fs::copy(target_path.join(&library), output.join(&library))?;
    let symbols = format!("{APP_NAME}.pdb");
    if target_path.join(&symbols).is_file() {
        fs::copy(target_path.join(&symbols), output.join(&symbols))?;
    }
    Ok(executable)
}

#[cfg(target_os = "windows")]
fn copy_directory_files(source: &Path, destination: &Path) -> Result<(), Box<dyn Error>> {
    fs::create_dir_all(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let path = entry.path();
        let build_only = path
            .extension()
            .is_some_and(|extension| extension == "exe" || extension == "lib");
        if entry.file_type()?.is_file() && !build_only {
            fs::copy(&path, destination.join(entry.file_name()))?;
        }
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn configure_windows_executable(executable: &Path) -> Result<(), Box<dyn Error>> {
    let icon = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(WINDOWS_ICON_SOURCE);
    let icon = icon.to_str().ok_or("the Windows icon path is not UTF-8")?;

    let mut image = editpe::Image::parse_file(executable)?;
    let mut resources = image.resource_directory().cloned().unwrap_or_default();
    resources.set_main_icon_file(icon)?;
    number_windows_icon_group(&mut resources)?;
    resources.set_manifest(WINDOWS_APP_MANIFEST)?;
    image.set_resource_directory(resources)?;
    image.write_file(executable)?;
    Ok(())
}

#[cfg(target_os = "windows")]
fn number_windows_icon_group(
    resources: &mut editpe::ResourceDirectory,
) -> Result<(), Box<dyn Error>> {
    use editpe::{ResourceEntry, ResourceEntryName, constants::RT_GROUP_ICON};

    let groups = resources
        .root_mut()
        .get_mut(ResourceEntryName::ID(u32::from(RT_GROUP_ICON)))
        .and_then(ResourceEntry::as_table_mut)
        .ok_or("the bundled executable has no icon group table")?;
    let group = groups
        .remove(ResourceEntryName::from_string(WINDOWS_EDITPE_ICON_GROUP))
        .ok_or("the icon group was not written to the bundled executable")?;
    let _replaced = groups.insert_at(ResourceEntryName::ID(WINDOWS_MAIN_ICON_ID), group, 0);
    Ok(())
}

#[cfg(target_os = "macos")]
fn configure_macos_main_app(app: &Path) -> Result<(), Box<dyn Error>> {
    let info_path = app.join("Contents/Info.plist");
    let mut info = plist::Value::from_file(&info_path)?;
    let dictionary = info
        .as_dictionary_mut()
        .ok_or("macOS main app Info.plist is not a dictionary")?;

    dictionary.insert(
        MACOS_FILE_QUARANTINE_KEY.to_owned(),
        plist::Value::Boolean(false),
    );
    dictionary.insert(
        MACOS_APP_DATA_USAGE_DESCRIPTION_KEY.to_owned(),
        plist::Value::String(MACOS_APP_DATA_USAGE_DESCRIPTION.to_owned()),
    );
    dictionary.insert(
        MACOS_ICON_NAME_KEY.to_owned(),
        plist::Value::String(MACOS_ICON_NAME.to_owned()),
    );
    info.to_file_xml(&info_path)?;
    // Assets.car reaches Resources/ through the packaging/mac resource copy.
    // It is compiled from assets/zz.icon by scripts/compile-macos-icon.sh and
    // committed: actool renders the layered icon through the GPU and does so
    // only unreliably on virtualized CI runners. The bundle validation below
    // still requires it, so a missing artifact fails loudly.
    Ok(())
}

#[cfg(target_os = "macos")]
fn build_macos_bundle(
    output: &Path,
    profile: &BuildProfile,
    features: Option<&str>,
) -> Result<PathBuf, Box<dyn Error>> {
    let target_path = build_macos_binaries(profile, features)?;
    let resources_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("packaging");
    let app = cef::build_util::mac::bundle(
        output,
        &target_path,
        APP_NAME,
        MACOS_HELPER_NAME,
        Some(resources_path),
        cef::build_util::mac::BundleInfo {
            name: APP_NAME.to_owned(),
            identifier: "dev.zz.app".to_owned(),
            display_name: APP_NAME.to_owned(),
            development_region: "English".to_owned(),
            version: product_version(env!("CARGO_PKG_VERSION")).parse()?,
        },
    )?;
    prune_locales(
        &app.join(MACOS_CEF_FRAMEWORK).join("Resources"),
        "lproj",
        MACOS_CEF_LOCALE,
    )?;
    fs::copy(target_path.join(CLI_NAME), macos_cli_launcher(&app))?;
    if profile.is_named(PROFILING_PROFILE) {
        install_macos_debug_symbols(output, &target_path)?;
    }
    Ok(app)
}

#[cfg(target_os = "macos")]
fn configure_macos_bundle_versions(app: &Path) -> Result<(), Box<dyn Error>> {
    let short_version = product_version(env!("CARGO_PKG_VERSION"));
    let build_version =
        env::var(MACOS_BUILD_VERSION_ENV).unwrap_or_else(|_| short_version.to_owned());
    if !is_valid_apple_build_version(&build_version) {
        return Err(format!(
            "{MACOS_BUILD_VERSION_ENV} must be one to three numeric components: {build_version:?}"
        )
        .into());
    }

    let mut apps = vec![app.to_owned()];
    apps.extend(macos_helper_apps(app));
    for bundle in apps {
        let info_path = bundle.join("Contents/Info.plist");
        let mut info = plist::Value::from_file(&info_path)?;
        let dictionary = info
            .as_dictionary_mut()
            .ok_or_else(|| format!("{} is not a dictionary", info_path.display()))?;
        dictionary.insert(
            BUNDLE_SHORT_VERSION_KEY.to_owned(),
            plist::Value::String(short_version.to_owned()),
        );
        dictionary.insert(
            BUNDLE_VERSION_KEY.to_owned(),
            plist::Value::String(build_version.clone()),
        );
        info.to_file_xml(&info_path)?;
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn is_valid_apple_build_version(version: &str) -> bool {
    let parts = version.split('.').collect::<Vec<_>>();
    (1..=3).contains(&parts.len())
        && parts.iter().zip([4, 2, 2]).all(|(part, max_digits)| {
            !part.is_empty()
                && part.len() <= max_digits
                && part.bytes().all(|byte| byte.is_ascii_digit())
        })
}

#[cfg(target_os = "macos")]
fn macos_cli_launcher(app: &Path) -> PathBuf {
    app.join("Contents/MacOS").join(CLI_BUNDLE_NAME)
}

#[cfg(target_os = "macos")]
fn build_macos_binaries(
    profile: &BuildProfile,
    features: Option<&str>,
) -> Result<PathBuf, Box<dyn Error>> {
    println!("Building {APP_NAME}, {MACOS_HELPER_NAME}, and {CLI_NAME}...");
    let mut command = Command::new(env::var_os("CARGO").unwrap_or_else(|| "cargo".into()));
    command.arg("build");
    profile.configure_cargo(&mut command);
    if let Some(features) = merged_features(features) {
        command.arg("--features").arg(features);
    }
    if let Some(optimize_mode) = profile.ghostty_optimize_override() {
        command.env(GHOSTTY_OPTIMIZE_ENV, optimize_mode);
        println!("Building libghostty-vt with Zig {optimize_mode} for profiling parity...");
    }
    command
        .args([
            "--message-format=json-render-diagnostics",
            "--bin",
            APP_NAME,
            "--bin",
            MACOS_HELPER_NAME,
            "--bin",
            CLI_NAME,
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());

    let mut child = command.spawn()?;
    let stdout = child.stdout.take().ok_or("cargo stdout is unavailable")?;
    let mut executables = HashMap::new();
    for message in Message::parse_stream(BufReader::new(stdout)) {
        if let Message::CompilerArtifact(artifact) = message?
            && matches!(
                artifact.target.name.as_str(),
                APP_NAME | MACOS_HELPER_NAME | CLI_NAME
            )
            && let Some(executable) = artifact.executable
        {
            executables.insert(artifact.target.name, executable.into_std_path_buf());
        }
    }

    let status = child.wait()?;
    if !status.success() {
        return Err(format!("cargo build failed with {status}").into());
    }

    let app = executables
        .remove(APP_NAME)
        .ok_or("cargo did not emit the zz executable")?;
    let target_path = app
        .parent()
        .ok_or("zz executable has no parent directory")?;
    for name in [MACOS_HELPER_NAME, CLI_NAME] {
        let executable = executables
            .remove(name)
            .ok_or_else(|| format!("cargo did not emit the {name} executable"))?;
        if executable.parent() != Some(target_path) {
            return Err(format!("zz and {name} were emitted into different directories").into());
        }
    }
    Ok(target_path.to_owned())
}

#[cfg(target_os = "macos")]
fn install_macos_debug_symbols(output: &Path, target_path: &Path) -> Result<(), Box<dyn Error>> {
    let symbols_path = output.join("symbols");
    fs::create_dir_all(&symbols_path)?;

    for binary_name in [APP_NAME, MACOS_HELPER_NAME] {
        let binary = target_path.join(binary_name);
        let mut source = binary.clone();
        source.set_extension("dSYM");
        if !source.is_dir() {
            return Err(format!(
                "profiling build did not emit debug symbols at {}",
                source.display()
            )
            .into());
        }

        let destination = symbols_path.join(
            source
                .file_name()
                .ok_or("profiling debug-symbol path has no file name")?,
        );
        if let Ok(metadata) = fs::symlink_metadata(&destination) {
            if metadata.is_dir() {
                fs::remove_dir_all(&destination)?;
            } else {
                fs::remove_file(&destination)?;
            }
        }
        copy_directory_recursively(&source, &destination)?;

        let binary_uuids = macho_uuids(&binary)?;
        let symbol_uuids = macho_uuids(&destination)?;
        if binary_uuids.is_empty() || binary_uuids != symbol_uuids {
            return Err(format!(
                "debug-symbol UUID mismatch for {}: binary={binary_uuids:?} symbols={symbol_uuids:?}",
                binary.display()
            )
            .into());
        }
        println!(
            "Debug symbols ready: {} ({})",
            destination.display(),
            binary_uuids.into_iter().collect::<Vec<_>>().join(", ")
        );
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn copy_directory_recursively(source: &Path, destination: &Path) -> io::Result<()> {
    fs::create_dir_all(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let destination_entry = destination.join(entry.file_name());
        if file_type.is_dir() {
            copy_directory_recursively(&entry.path(), &destination_entry)?;
        } else if file_type.is_file() {
            fs::copy(entry.path(), destination_entry)?;
        } else {
            return Err(io::Error::other(format!(
                "unsupported entry in debug-symbol bundle: {}",
                entry.path().display()
            )));
        }
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn macho_uuids(path: &Path) -> Result<BTreeSet<String>, Box<dyn Error>> {
    let output = Command::new("/usr/bin/dwarfdump")
        .arg("--uuid")
        .arg(path)
        .output()?;
    if !output.status.success() {
        return Err(format!(
            "dwarfdump failed for {} with {}: {}",
            path.display(),
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        )
        .into());
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            line.strip_prefix("UUID: ")
                .and_then(|line| line.split_whitespace().next())
                .map(str::to_owned)
        })
        .collect())
}

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
fn prune_locales(directory: &Path, extension: &str, keep: &str) -> Result<(), Box<dyn Error>> {
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().is_none_or(|value| value != extension) || entry.file_name() == keep {
            continue;
        }
        if entry.file_type()?.is_dir() {
            fs::remove_dir_all(&path)?;
        } else {
            fs::remove_file(&path)?;
        }
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn strip_library(path: &Path) -> Result<(), Box<dyn Error>> {
    let before = path.metadata()?.len();
    let status = Command::new("strip").arg(path).status()?;
    if !status.success() {
        return Err(format!("strip failed for {} with {status}", path.display()).into());
    }
    let after = path.metadata()?.len();
    println!(
        "Stripped {}: {} MiB -> {} MiB",
        path.display(),
        before >> 20,
        after >> 20
    );
    Ok(())
}

fn install_cef_notices(executable: &std::path::Path) -> Result<(), Box<dyn Error>> {
    let resources = bundle_root(executable);
    fs::create_dir_all(&resources)?;

    let license = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("third_party/cef/LICENSE.txt");
    fs::copy(license, resources.join("CEF_LICENSE.txt"))?;

    let cef_dir = cef::sys::get_cef_dir().ok_or("CEF distribution path is unavailable")?;
    fs::copy(cef_dir.join("CREDITS.html"), resources.join("CREDITS.html"))?;
    Ok(())
}

#[cfg(target_os = "macos")]
fn sign_macos_bundle(app: &Path) -> Result<(), Box<dyn Error>> {
    let identity = macos_local_signing_identity();
    let framework = app.join(MACOS_CEF_FRAMEWORK);
    sign_macos_code(&framework, &identity)?;
    for helper in macos_helper_apps(app) {
        sign_macos_code(&helper, &identity)?;
    }
    sign_macos_code(&macos_cli_launcher(app), &identity)?;
    sign_macos_code(app, &identity)
}

#[cfg(target_os = "macos")]
fn macos_local_signing_identity() -> OsString {
    if let Some(identity) = env::var_os(MACOS_LOCAL_SIGN_IDENTITY_ENV)
        && !identity.is_empty()
    {
        println!("Signing macOS bundle with the configured local identity");
        return identity;
    }

    let identity = Command::new("/usr/bin/security")
        .args(["find-identity", "-v", "-p", "codesigning"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| {
            let output = String::from_utf8_lossy(&output.stdout);
            let identities = apple_development_identities(&output);
            (identities.len() == 1).then(|| identities[0].clone())
        });

    if let Some(identity) = identity {
        println!("Signing macOS bundle with the installed Apple Development identity");
        identity.into()
    } else {
        println!(
            "No unique Apple Development identity found; ad-hoc signing the macOS bundle \
             (set {MACOS_LOCAL_SIGN_IDENTITY_ENV} to override)"
        );
        "-".into()
    }
}

#[cfg(target_os = "macos")]
fn apple_development_identities(output: &str) -> Vec<String> {
    output
        .lines()
        .filter_map(|line| {
            let (_, identity) = line.split_once(')')?;
            let identity = identity.trim_start();
            let (fingerprint, label) = identity.split_once(char::is_whitespace)?;
            let fingerprint_is_sha1 =
                fingerprint.len() == 40 && fingerprint.bytes().all(|byte| byte.is_ascii_hexdigit());
            (fingerprint_is_sha1 && label.trim_start().starts_with("\"Apple Development:"))
                .then(|| fingerprint.to_owned())
        })
        .collect()
}

#[cfg(target_os = "macos")]
fn sign_macos_code(path: &Path, identity: &std::ffi::OsStr) -> Result<(), Box<dyn Error>> {
    let status = Command::new("/usr/bin/codesign")
        .args(["--force", "--sign"])
        .arg(identity)
        .arg(path)
        .status()?;
    if !status.success() {
        return Err(format!("codesign failed for {} with {status}", path.display()).into());
    }
    Ok(())
}

fn verify_bundle(executable: &std::path::Path) -> Result<(), Box<dyn Error>> {
    let root = bundle_root(executable);
    let required = platform_bundle_files(executable);
    let invalid: Vec<_> = required
        .into_iter()
        .filter(|path| {
            path.metadata()
                .map_or(true, |metadata| !metadata.is_file() || metadata.len() == 0)
        })
        .collect();
    if !invalid.is_empty() {
        let invalid = invalid
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>()
            .join(", ");
        return Err(format!("CEF bundle files are missing or empty: {invalid}").into());
    }
    #[cfg(target_os = "macos")]
    verify_macos_main_app_configuration(executable)?;
    #[cfg(target_os = "macos")]
    verify_macos_bundle_versions(executable)?;
    #[cfg(target_os = "macos")]
    verify_macos_signature(executable)?;
    #[cfg(target_os = "windows")]
    verify_windows_executable(executable)?;
    println!("verified CEF bundle resources below {}", root.display());
    Ok(())
}

#[cfg(target_os = "macos")]
fn verify_macos_main_app_configuration(app: &Path) -> Result<(), Box<dyn Error>> {
    let info_path = app.join("Contents/Info.plist");
    let info = plist::Value::from_file(&info_path)?;
    let dictionary = info
        .as_dictionary()
        .ok_or("macOS main app Info.plist is not a dictionary")?;
    let quarantine_enabled = dictionary
        .get(MACOS_FILE_QUARANTINE_KEY)
        .and_then(plist::Value::as_boolean);

    match quarantine_enabled {
        Some(false) => {}
        Some(true) => {
            return Err(format!(
                "macOS bundle enables automatic file quarantine in {}",
                info_path.display()
            )
            .into());
        }
        None => {
            return Err(format!(
                "macOS bundle has no boolean {MACOS_FILE_QUARANTINE_KEY} in {}",
                info_path.display()
            )
            .into());
        }
    }

    match dictionary
        .get(MACOS_APP_DATA_USAGE_DESCRIPTION_KEY)
        .and_then(plist::Value::as_string)
    {
        Some(MACOS_APP_DATA_USAGE_DESCRIPTION) => {}
        Some(description) => {
            return Err(format!(
                "macOS bundle has unexpected {MACOS_APP_DATA_USAGE_DESCRIPTION_KEY} value {description:?} in {}",
                info_path.display()
            )
            .into());
        }
        None => {
            return Err(format!(
                "macOS bundle has no string {MACOS_APP_DATA_USAGE_DESCRIPTION_KEY} in {}",
                info_path.display()
            )
            .into());
        }
    }

    match dictionary
        .get(MACOS_ICON_KEY)
        .and_then(plist::Value::as_string)
    {
        Some(MACOS_ICON_FILE) => {}
        Some(icon) => {
            return Err(format!(
                "macOS bundle references unexpected icon {icon:?} in {}",
                info_path.display()
            )
            .into());
        }
        None => {
            return Err(format!(
                "macOS bundle has no string {MACOS_ICON_KEY} in {}",
                info_path.display()
            )
            .into());
        }
    }

    match dictionary
        .get(MACOS_ICON_NAME_KEY)
        .and_then(plist::Value::as_string)
    {
        Some(MACOS_ICON_NAME) => Ok(()),
        Some(name) => Err(format!(
            "macOS bundle references unexpected icon name {name:?} in {}",
            info_path.display()
        )
        .into()),
        None => Err(format!(
            "macOS bundle has no string {MACOS_ICON_NAME_KEY} in {}",
            info_path.display()
        )
        .into()),
    }
}

#[cfg(target_os = "macos")]
fn verify_macos_bundle_versions(app: &Path) -> Result<(), Box<dyn Error>> {
    let expected_short_version = product_version(env!("CARGO_PKG_VERSION"));
    let expected_build_version = env::var(MACOS_BUILD_VERSION_ENV).ok();
    let mut apps = vec![app.to_owned()];
    apps.extend(macos_helper_apps(app));

    for bundle in apps {
        let info_path = bundle.join("Contents/Info.plist");
        let info = plist::Value::from_file(&info_path)?;
        let dictionary = info
            .as_dictionary()
            .ok_or_else(|| format!("{} is not a dictionary", info_path.display()))?;
        let short_version = dictionary
            .get(BUNDLE_SHORT_VERSION_KEY)
            .and_then(plist::Value::as_string)
            .ok_or_else(|| {
                format!(
                    "macOS bundle has no string {BUNDLE_SHORT_VERSION_KEY} in {}",
                    info_path.display()
                )
            })?;
        if short_version != expected_short_version {
            return Err(format!(
                "macOS bundle has {BUNDLE_SHORT_VERSION_KEY}={short_version:?}, expected {expected_short_version:?} in {}",
                info_path.display()
            )
            .into());
        }

        let build_version = dictionary
            .get(BUNDLE_VERSION_KEY)
            .and_then(plist::Value::as_string)
            .ok_or_else(|| {
                format!(
                    "macOS bundle has no string {BUNDLE_VERSION_KEY} in {}",
                    info_path.display()
                )
            })?;
        if !is_valid_apple_build_version(build_version) {
            return Err(format!(
                "macOS bundle has invalid {BUNDLE_VERSION_KEY}={build_version:?} in {}",
                info_path.display()
            )
            .into());
        }
        if let Some(expected) = expected_build_version.as_deref()
            && build_version != expected
        {
            return Err(format!(
                "macOS bundle has {BUNDLE_VERSION_KEY}={build_version:?}, expected {expected:?} in {}",
                info_path.display()
            )
            .into());
        }
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn verify_windows_executable(executable: &Path) -> Result<(), Box<dyn Error>> {
    use editpe::{ResourceEntry, ResourceEntryName, constants::RT_GROUP_ICON};

    let image = editpe::Image::parse_file(executable)?;
    let resources = image
        .resource_directory()
        .ok_or_else(|| format!("{} has no resources", executable.display()))?;

    match resources.get_manifest()? {
        Some(manifest) if manifest.contains(WINDOWS_DPI_AWARENESS) => {}
        Some(_) => {
            return Err(format!(
                "{} has an application manifest without {WINDOWS_DPI_AWARENESS} DPI awareness",
                executable.display()
            )
            .into());
        }
        None => {
            return Err(format!("{} has no application manifest", executable.display()).into());
        }
    }

    let icon_group = resources
        .root()
        .get(ResourceEntryName::ID(u32::from(RT_GROUP_ICON)))
        .and_then(ResourceEntry::as_table)
        .and_then(|groups| groups.get(ResourceEntryName::ID(WINDOWS_MAIN_ICON_ID)));
    if icon_group.is_none() || resources.get_main_icon()?.is_none() {
        return Err(format!(
            "{} has no icon group at resource id {WINDOWS_MAIN_ICON_ID}",
            executable.display()
        )
        .into());
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn verify_macos_signature(app: &Path) -> Result<(), Box<dyn Error>> {
    let status = Command::new("/usr/bin/codesign")
        .args(["--verify", "--deep", "--strict", "--verbose=2"])
        .arg(app)
        .status()?;
    if !status.success() {
        return Err(format!(
            "macOS bundle signature verification failed for {} with {status}",
            app.display()
        )
        .into());
    }
    Ok(())
}

#[cfg(any(target_os = "linux", target_os = "windows"))]
fn bundle_root(executable: &std::path::Path) -> PathBuf {
    executable
        .parent()
        .expect("bundled executable has a parent directory")
        .to_owned()
}

#[cfg(target_os = "macos")]
fn bundle_root(executable: &std::path::Path) -> PathBuf {
    executable.join("Contents/Resources")
}

#[cfg(target_os = "linux")]
fn platform_bundle_files(executable: &std::path::Path) -> Vec<PathBuf> {
    let root = bundle_root(executable);
    vec![
        executable.to_owned(),
        root.join("libcef.so"),
        root.join("icudtl.dat"),
        root.join("resources.pak"),
        root.join("locales/en-US.pak"),
        root.join("chrome-sandbox"),
        root.join("CREDITS.html"),
        root.join("CEF_LICENSE.txt"),
        root.join(CLI_BUNDLE_NAME),
    ]
}

#[cfg(target_os = "windows")]
fn platform_bundle_files(executable: &std::path::Path) -> Vec<PathBuf> {
    let root = bundle_root(executable);
    vec![
        executable.to_owned(),
        root.join("zz.dll"),
        root.join("libcef.dll"),
        root.join("chrome_elf.dll"),
        root.join("v8_context_snapshot.bin"),
        root.join("icudtl.dat"),
        root.join("resources.pak"),
        root.join("chrome_100_percent.pak"),
        root.join("chrome_200_percent.pak"),
        root.join("locales/en-US.pak"),
        root.join("CREDITS.html"),
        root.join("CEF_LICENSE.txt"),
    ]
}

#[cfg(target_os = "macos")]
fn platform_bundle_files(app: &std::path::Path) -> Vec<PathBuf> {
    let resources = bundle_root(app);
    let framework = app.join(MACOS_CEF_FRAMEWORK);
    let mut required = vec![
        app.join("Contents/Info.plist"),
        app.join("Contents/MacOS/zz"),
        macos_cli_launcher(app),
        framework.join("Chromium Embedded Framework"),
        framework.join("Resources/Info.plist"),
        framework.join("Resources/icudtl.dat"),
        framework.join("Resources/chrome_100_percent.pak"),
        framework.join("Resources/chrome_200_percent.pak"),
        framework.join("Resources/resources.pak"),
        framework.join("Resources/en.lproj/locale.pak"),
        resources.join("CREDITS.html"),
        resources.join("CEF_LICENSE.txt"),
        resources.join(MACOS_ICON_FILE),
        resources.join("Assets.car"),
    ];
    for helper in macos_helper_apps(app) {
        let name = helper
            .file_stem()
            .expect("helper app has a file stem")
            .to_string_lossy();
        required.push(helper.join("Contents/Info.plist"));
        required.push(helper.join("Contents/MacOS").join(name.as_ref()));
    }
    required
}

#[cfg(target_os = "macos")]
fn macos_helper_apps(app: &Path) -> Vec<PathBuf> {
    MACOS_HELPER_SUFFIXES
        .into_iter()
        .map(|suffix| {
            app.join("Contents/Frameworks")
                .join(format!("{APP_NAME} {suffix}.app"))
        })
        .collect()
}

#[cfg(test)]
mod option_tests {
    use std::path::PathBuf;

    #[cfg(target_os = "macos")]
    use super::product_version;
    use super::{BuildProfile, BundleOptions, parse_bundle_options};

    fn arguments(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn strips_prerelease_and_build_metadata_from_product_versions() {
        assert_eq!(product_version("0.2.0"), "0.2.0");
        assert_eq!(product_version("0.2.0-beta.1"), "0.2.0");
        assert_eq!(product_version("0.2.0+build.7"), "0.2.0");
        assert_eq!(product_version("0.2.0-beta.1+build.7"), "0.2.0");
    }

    #[test]
    fn parses_named_bundle_profile_and_output() {
        assert_eq!(
            parse_bundle_options(&arguments(&[
                "--profile",
                "profiling",
                "--output",
                "dist/zz-profile",
            ])),
            Ok(BundleOptions {
                profile: BuildProfile::Named("profiling".to_owned()),
                output: PathBuf::from("dist/zz-profile"),
                features: None,
            })
        );
    }

    #[test]
    fn preserves_release_bundle_mode() {
        assert_eq!(
            parse_bundle_options(&arguments(&["--release"])),
            Ok(BundleOptions {
                profile: BuildProfile::Release,
                output: PathBuf::from("dist/zz"),
                features: None,
            })
        );
    }

    #[test]
    fn features_accumulate_across_flags_and_reject_empty_or_flaglike_lists() {
        assert_eq!(
            parse_bundle_options(&arguments(&["--release", "--", "--features", "agent-pane"])),
            Ok(BundleOptions {
                profile: BuildProfile::Release,
                output: PathBuf::from("dist/zz"),
                features: Some("agent-pane".to_owned()),
            })
        );
        assert_eq!(
            parse_bundle_options(&arguments(&[
                "--release",
                "--features",
                "agent-pane",
                "--features",
                "editor-pane",
            ])),
            Ok(BundleOptions {
                profile: BuildProfile::Release,
                output: PathBuf::from("dist/zz"),
                features: Some("agent-pane,editor-pane".to_owned()),
            })
        );
        assert_eq!(
            parse_bundle_options(&arguments(&["--features"])),
            Err("--features requires a feature list".to_owned())
        );
        assert_eq!(
            parse_bundle_options(&arguments(&["--features", ""])),
            Err("--features requires a non-empty feature list".to_owned())
        );
        assert_eq!(
            parse_bundle_options(&arguments(&["--features", "--release"])),
            Err("--features requires a non-empty feature list".to_owned())
        );
    }

    #[test]
    fn rejects_conflicting_bundle_profiles() {
        assert_eq!(
            parse_bundle_options(&arguments(&["--release", "--profile", "profiling"])),
            Err("--release and --profile are mutually exclusive".to_owned())
        );
    }

    #[test]
    fn rejects_missing_named_profile() {
        assert_eq!(
            parse_bundle_options(&arguments(&["--profile"])),
            Err("--profile requires a name".to_owned())
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn profiling_profile_keeps_release_fast_ghostty() {
        assert_eq!(
            BuildProfile::Named("profiling".to_owned()).ghostty_optimize_override(),
            Some("ReleaseFast")
        );
        assert_eq!(BuildProfile::Release.ghostty_optimize_override(), None);
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::{apple_development_identities, is_valid_apple_build_version};

    #[test]
    fn accepts_only_apple_numeric_build_versions() {
        assert!(is_valid_apple_build_version("42"));
        assert!(is_valid_apple_build_version("0.2.0"));
        assert!(is_valid_apple_build_version("1234.12.31"));
        assert!(!is_valid_apple_build_version("0.2.0-beta.1"));
        assert!(!is_valid_apple_build_version("12345"));
        assert!(!is_valid_apple_build_version("1.2.345"));
    }

    #[test]
    fn finds_only_apple_development_signing_identities() {
        let output = r#"
  1) 5A0FD75C88FC92C548FD70B64BAE89B593E9EDC1 "Apple Development: Developer One (TEAMONE)"
  2) 41146C777A0DB8DB2AFBD1F61B73D9EBBA872CB4 "Developer ID Application: Developer One (TEAMTWO)"
     2 valid identities found
"#;

        assert_eq!(
            apple_development_identities(output),
            ["5A0FD75C88FC92C548FD70B64BAE89B593E9EDC1"]
        );
    }

    #[test]
    fn ignores_malformed_identity_rows() {
        let output = r#"
  1) NOT-A-SHA "Apple Development: Broken (TEAMONE)"
  2) 5A0FD75C88FC92C548FD70B64BAE89B593E9EDCZ "Apple Development: Broken (TEAMONE)"
     0 valid identities found
"#;

        assert!(apple_development_identities(output).is_empty());
    }
}
