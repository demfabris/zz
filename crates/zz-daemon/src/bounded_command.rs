use std::{
    io::Read,
    process::{Child, Command, Output, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

const MAX_STDERR_BYTES: usize = 256 * 1024;

pub(crate) fn run_output_until(
    command: Command,
    max_stdout: usize,
    deadline: Instant,
) -> Result<Output, String> {
    run_output_until_cancelled(command, max_stdout, deadline, &|| false)
}

pub(crate) fn run_output_until_cancelled(
    mut command: Command,
    max_stdout: usize,
    deadline: Instant,
    cancelled: &dyn Fn() -> bool,
) -> Result<Output, String> {
    let timeout = deadline.saturating_duration_since(Instant::now());
    if timeout.is_zero() {
        return Err("git timed out".to_owned());
    }
    let (output, truncated) =
        collect_output(spawn_output(&mut command)?, max_stdout, timeout, cancelled)?;
    if truncated {
        return Err(format!("git output exceeded {max_stdout} bytes"));
    }
    Ok(output)
}

fn spawn_output(command: &mut Command) -> Result<Child, String> {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt as _;

        command.process_group(0);
    }
    command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("git could not start: {error}"))
}

fn collect_output(
    mut child: Child,
    max_stdout: usize,
    timeout: Duration,
    cancelled: &dyn Fn() -> bool,
) -> Result<(Output, bool), String> {
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "git offered no output pipe".to_owned())?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| "git offered no error pipe".to_owned())?;
    let (overflow_tx, overflow_rx) = mpsc::sync_channel(1);
    let (stdout_tx, stdout_rx) = mpsc::sync_channel(1);
    let (stderr_tx, stderr_rx) = mpsc::sync_channel(1);
    let stdout_reader = thread::spawn(move || {
        let _ = stdout_tx.send(read_limited(stdout, max_stdout, Some(&overflow_tx)));
    });
    let stderr_reader = thread::spawn(move || {
        let _ = stderr_tx.send(read_limited(stderr, MAX_STDERR_BYTES, None));
    });
    let deadline = Instant::now() + timeout;
    let mut truncated = false;
    let mut status = None;
    let mut stdout = None;
    let mut stderr = None;
    loop {
        if overflow_rx.try_recv().is_ok() {
            truncated = true;
            status = Some(terminate_output(&mut child)?);
        }
        if stdout.is_none() {
            match stdout_rx.try_recv() {
                Ok(value) => stdout = Some(value),
                Err(mpsc::TryRecvError::Disconnected) => {
                    return Err("git output reader stopped".to_owned());
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        if stderr.is_none() {
            match stderr_rx.try_recv() {
                Ok(value) => stderr = Some(value),
                Err(mpsc::TryRecvError::Disconnected) => {
                    return Err("git error reader stopped".to_owned());
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        if status.is_none()
            && child
                .try_wait()
                .map_err(|error| format!("git could not be polled: {error}"))?
                .is_some()
        {
            status = Some(terminate_output(&mut child)?);
        }
        if status.is_some() && stdout.is_some() && stderr.is_some() {
            break;
        }
        if Instant::now() >= deadline {
            let _ = terminate_output(&mut child);
            return Err(format!("git timed out after {} seconds", timeout.as_secs()));
        }
        if status.is_none() && cancelled() {
            let _ = terminate_output(&mut child);
            return Err("git was cancelled".to_owned());
        }
        thread::sleep(Duration::from_millis(10));
    }
    stdout_reader
        .join()
        .map_err(|_| "git output reader panicked".to_owned())?;
    stderr_reader
        .join()
        .map_err(|_| "git error reader panicked".to_owned())?;
    Ok((
        Output {
            status: status.expect("git status set before output completes"),
            stdout: stdout.expect("git output set before completion")?,
            stderr: stderr.expect("git error set before completion")?,
        },
        truncated,
    ))
}

fn terminate_output(child: &mut Child) -> Result<std::process::ExitStatus, String> {
    #[cfg(unix)]
    let _ = rustix::process::kill_process_group(
        rustix::process::Pid::from_child(&*child),
        rustix::process::Signal::KILL,
    );
    #[cfg(windows)]
    {
        let pid = child.id().to_string();
        let _ = Command::new("taskkill")
            .args(["/PID", pid.as_str(), "/T", "/F"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    let _ = child.kill();
    child
        .wait()
        .map_err(|error| format!("git did not exit: {error}"))
}

fn read_limited(
    mut pipe: impl Read,
    limit: usize,
    overflow: Option<&mpsc::SyncSender<()>>,
) -> Result<Vec<u8>, String> {
    let mut output = Vec::new();
    let mut buffer = [0u8; 16 * 1024];
    let mut reported = false;
    loop {
        let filled = pipe
            .read(&mut buffer)
            .map_err(|error| format!("git output could not be read: {error}"))?;
        if filled == 0 {
            break;
        }
        let remaining = limit.saturating_sub(output.len());
        output.extend_from_slice(&buffer[..filled.min(remaining)]);
        if filled > remaining && !reported {
            if let Some(overflow) = overflow {
                let _ = overflow.try_send(());
            }
            reported = true;
        }
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn command_output_drains_stderr_without_blocking_stdout() {
        let mut command = Command::new("sh");
        command.arg("-c").arg(
            "i=0; while [ $i -lt 20000 ]; do echo error-output-line >&2; i=$((i + 1)); done; printf ok",
        );
        let output = run_output_until(command, 1024, Instant::now() + Duration::from_secs(5))
            .expect("command should finish");
        assert_eq!(output.stdout, b"ok");
        assert!(output.stderr.len() <= MAX_STDERR_BYTES);
    }

    #[cfg(unix)]
    #[test]
    fn command_output_has_a_hard_deadline() {
        let mut command = Command::new("sh");
        command
            .arg("-c")
            .arg("(trap '' TERM; while :; do sleep 1; done >&2) & while :; do sleep 1; done");
        let started = Instant::now();
        let error = collect_output(
            spawn_output(&mut command).expect("command should start"),
            1024,
            Duration::from_millis(50),
            &|| false,
        )
        .expect_err("command should time out");
        assert!(error.contains("timed out"), "{error}");
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[cfg(unix)]
    #[test]
    fn command_output_stops_when_cancelled() {
        let mut command = Command::new("sh");
        command.arg("-c").arg("while :; do sleep 1; done");
        let started = Instant::now();
        let error = run_output_until_cancelled(
            command,
            1024,
            Instant::now() + Duration::from_secs(10),
            &|| started.elapsed() >= Duration::from_millis(50),
        )
        .expect_err("command should stop");
        assert!(error.contains("cancelled"), "{error}");
        assert!(started.elapsed() < Duration::from_secs(2));
    }
}
