use std::process::Command;

const TMUX_USAGE: &str = concat!(
    "usage: zz [-2CDhlNuVv] [-c shell-command] [-f file] [-L socket-name]\n",
    "            [-S socket-path] [-T features] [command [flags]]\n"
);

#[test]
fn tmux_version_is_exact() {
    let output = Command::new(env!("CARGO_BIN_EXE_zz"))
        .args(["-2uV", "ignored"])
        .output()
        .expect("run zz -V");
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(output.stdout, b"tmux 3.8-zz\n");
    assert!(output.stderr.is_empty());
}

#[test]
fn unknown_tmux_flag_uses_tmux_usage_shape() {
    let output = Command::new(env!("CARGO_BIN_EXE_zz"))
        .arg("-8")
        .output()
        .expect("run zz with an unknown tmux flag");
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert_eq!(output.stderr, TMUX_USAGE.as_bytes());
}

#[test]
fn unsupported_control_mode_fails_on_one_line() {
    let output = Command::new(env!("CARGO_BIN_EXE_zz"))
        .arg("-CC")
        .output()
        .expect("run zz control mode rejection");
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert_eq!(
        output.stderr,
        b"zz: -C and -CC control mode are not supported\n"
    );
}
