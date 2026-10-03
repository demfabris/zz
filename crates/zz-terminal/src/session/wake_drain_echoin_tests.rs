use super::*;

fn reads_for(queued: usize, interrupt_first: bool) -> usize {
    let mut remaining = queued;
    let mut reads = 0;
    let mut interrupted = !interrupt_first;
    drain_wake_reads(|buffer| {
        reads += 1;
        if !interrupted {
            interrupted = true;
            return Err(rustix::io::Errno::INTR);
        }
        if remaining == 0 {
            return Err(rustix::io::Errno::AGAIN);
        }
        let count = remaining.min(buffer.len());
        remaining -= count;
        Ok(count)
    })
    .expect("drain");
    assert_eq!(remaining, 0);
    reads
}

#[test]
fn a_short_read_ends_the_drain() {
    assert_eq!(reads_for(1, false), 1);
    assert_eq!(reads_for(63, false), 1);
    assert_eq!(reads_for(65, false), 2);
    assert_eq!(reads_for(129, false), 3);
}

#[test]
fn a_full_read_reads_again_until_empty() {
    assert_eq!(reads_for(64, false), 2);
    assert_eq!(reads_for(128, false), 3);
}

#[test]
fn an_interrupted_read_is_retried() {
    assert_eq!(reads_for(1, true), 2);
}

#[test]
fn a_real_pipe_drains_in_one_read_and_stays_readable_for_later_bytes() {
    let (read, write) = configured_actor_wake_pipe().expect("configured wake pipe");
    assert_eq!(rustix::io::write(&write, &[1_u8]).expect("wake"), 1);
    drain_wake_pipe(&read).expect("drain");
    let mut byte = [0_u8; 1];
    assert!(matches!(
        rustix::io::read(&read, &mut byte),
        Err(rustix::io::Errno::AGAIN)
    ));
    assert_eq!(rustix::io::write(&write, &[1_u8; 3]).expect("wake"), 3);
    drain_wake_pipe(&read).expect("drain");
    assert!(matches!(
        rustix::io::read(&read, &mut byte),
        Err(rustix::io::Errno::AGAIN)
    ));
}
