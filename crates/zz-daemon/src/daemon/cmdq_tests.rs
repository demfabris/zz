use super::*;

#[test]
fn interleaved_items_resume_only_their_current_continuation() {
    let first = CommandItem::new(None, String::from("first"));
    let second = CommandItem::new(Some(first.queue), String::from("second"));
    assert_ne!(first.id, second.id);
    assert_eq!(first.queue, second.queue);
    let first_wait = first.wait().unwrap();
    let second_wait = second.wait().unwrap();
    assert!(!first.resume(second_wait));
    assert_eq!(first.state(), State::Waiting(first_wait));
    assert!(second.resume(second_wait));
    assert!(!second.resume(second_wait));
    assert!(first.resume(first_wait));
    let next_wait = first.wait().unwrap();
    assert_ne!(first_wait, next_wait);
    assert!(!first.resume(first_wait));
    assert!(first.resume(next_wait));
    assert_eq!(first.as_str(), "first");
    assert_eq!(second.as_str(), "second");
}

#[test]
fn duplicate_completion_and_cancel_finish_exactly_once() {
    let item = CommandItem::new(None, ());
    let token = item.wait().unwrap();
    let mut finishes = 0;
    for _ in 0..2 {
        if item.finish() {
            finishes += 1;
        }
    }
    assert_eq!(finishes, 1);
    assert_eq!(item.state(), State::Done);
    assert!(!item.resume(token));
    assert!(item.wait().is_none());
}
