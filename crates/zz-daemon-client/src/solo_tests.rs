const SOLO_TEST: &str = "ZZ_SOLO_TEST";

pub(crate) fn rerun_alone(test_path: &str) -> bool {
    if std::env::var_os(SOLO_TEST).is_some_and(|path| path == test_path) {
        return true;
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([test_path, "--exact", "--test-threads=1"])
        .env(SOLO_TEST, test_path)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success() && stdout.contains("running 1 test"),
        "{stdout}{}",
        String::from_utf8_lossy(&output.stderr)
    );
    false
}
