use super::*;

#[test]
fn a_panicking_peer_scan_completes_and_the_next_scan_can_finish() {
    let (completed, completions) = crossbeam_channel::unbounded();
    for panic in [true, false] {
        let (reply, result) = std::sync::mpsc::sync_channel(1);
        assert!(
            finish_peer_scan(
                || {
                    assert!(!panic, "peer scan failure");
                    Ok(Vec::new())
                },
                Some(reply),
                Some(completed.clone()),
            )
            .is_none()
        );
        assert_eq!(result.try_recv().unwrap().is_err(), panic);
        assert!(matches!(
            completions.try_recv().unwrap(),
            super::super::timers::TimerCompletion::Peer
        ));
    }
    assert!(matches!(
        finish_peer_scan(|| panic!("peer scan failure"), None, Some(completed)),
        Some(Result::Peers(Err(_)))
    ));
    assert!(matches!(
        completions.try_recv().unwrap(),
        super::super::timers::TimerCompletion::Peer
    ));
}
