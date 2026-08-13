fn main() -> std::process::ExitCode {
    let code = u8::try_from(zz_browser::run_subprocess().clamp(0, 255)).unwrap_or_default();
    std::process::ExitCode::from(code)
}
