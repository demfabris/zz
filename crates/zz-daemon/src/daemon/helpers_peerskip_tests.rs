use super::*;
use std::io::Write;

fn backdate(path: &std::path::Path) {
    std::fs::File::open(path)
        .unwrap()
        .set_modified(std::time::SystemTime::now() - Duration::from_secs(30))
        .unwrap();
}

#[test]
fn a_peer_key_holds_until_a_record_file_or_a_pane_pid_beside_records_changes() {
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("sessions");
    let panes = vec![(PaneId(0), "%0".to_owned(), Some(10))];
    let moved = [(PaneId(0), "%0".to_owned(), Some(11))];
    let key = PeerKey::take(&directory, &panes).unwrap();
    assert!(key.holds(&directory, &panes));
    assert!(key.holds(&directory, &moved));
    assert!(key.holds(&directory, &[]));
    assert!(!key.holds(&root.path().join("other"), &panes));

    std::fs::create_dir(&directory).unwrap();
    assert!(!key.holds(&directory, &panes));
    assert!(PeerKey::take(&directory, &panes).is_none());
    backdate(&directory);
    let key = PeerKey::take(&directory, &panes).unwrap();
    assert!(key.holds(&directory, &panes));
    assert!(key.holds(&directory, &moved));

    let record = directory.join("42.json");
    std::fs::write(&record, b"{\"status\":\"idle\"}").unwrap();
    assert!(!key.holds(&directory, &panes));
    assert!(PeerKey::take(&directory, &panes).is_none());
    backdate(&record);
    backdate(&directory);
    let key = PeerKey::take(&directory, &panes).unwrap();
    assert!(key.holds(&directory, &panes));
    assert!(!key.holds(&directory, &moved));
    assert!(!key.holds(&directory, &[(PaneId(0), "%0".to_owned(), None)]));
    assert!(!key.holds(&directory, &[]));

    std::fs::OpenOptions::new()
        .write(true)
        .open(&record)
        .unwrap()
        .write_all(b"{\"status\":\"busy\"}")
        .unwrap();
    assert_eq!(stamp(&directory), key.stamps[0].1);
    assert!(!key.holds(&directory, &panes));
    backdate(&record);
    let key = PeerKey::take(&directory, &panes).unwrap();
    assert!(key.holds(&directory, &panes));

    std::fs::remove_file(&record).unwrap();
    assert!(!key.holds(&directory, &panes));
}
