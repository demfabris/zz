use super::*;

#[test]
fn response_completion_wakes_the_loop_only_after_admissions_freeze() {
    let shared = Arc::new(Shared::new(1));
    let mut event_loop = EventLoop::empty(&shared).unwrap();
    let mut admission = ResponseAdmissionGuard::new(&shared).unwrap();
    admission.finish();
    event_loop
        .poll
        .poll(&mut event_loop.events, Some(Duration::from_millis(20)))
        .unwrap();
    assert!(event_loop.events.is_empty());

    let mut first = ResponseAdmissionGuard::new(&shared).unwrap();
    let mut last = ResponseAdmissionGuard::new(&shared).unwrap();
    event_loop.start_shutdown(&shared);
    assert!(ResponseAdmissionGuard::new(&shared).is_none());
    first.finish();
    event_loop
        .poll
        .poll(&mut event_loop.events, Some(Duration::from_millis(20)))
        .unwrap();
    assert!(event_loop.events.is_empty());

    last.finish();
    event_loop
        .poll
        .poll(&mut event_loop.events, Some(Duration::from_secs(1)))
        .unwrap();
    assert!(event_loop.events.iter().any(|event| event.token() == WAKE));
    assert_eq!(shared.response_admissions.lock().active, 0);
}
