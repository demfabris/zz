use super::*;

fn backdate(path: &Path) {
    fs::File::open(path)
        .unwrap()
        .set_modified(SystemTime::now() - Duration::from_secs(30))
        .unwrap();
}

#[test]
fn a_registry_key_holds_until_the_directory_or_a_record_file_changes() {
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("sessions");
    let mut cache = RegistryCache::default();
    let mut settle = || {
        cache.refresh_in(&directory).unwrap();
        cache.settled()
    };
    let key = settle().unwrap();
    assert!(key.holds(&directory));
    assert!(!key.has_files());
    assert!(!key.holds(&root.path().join("other")));

    fs::create_dir(&directory).unwrap();
    assert!(!key.holds(&directory));
    assert!(settle().is_none());
    backdate(&directory);
    let key = settle().unwrap();
    assert!(key.holds(&directory));
    assert!(!key.has_files());

    let record = directory.join("42.json");
    fs::write(&record, b"{\"status\":\"idle\"}").unwrap();
    fs::write(directory.join("notes.json"), b"{}").unwrap();
    assert!(!key.holds(&directory));
    assert!(settle().is_none());
    backdate(&record);
    backdate(&directory);
    let key = settle().unwrap();
    assert!(key.holds(&directory));
    assert_eq!(key.stamps.len(), 2);

    fs::OpenOptions::new()
        .write(true)
        .open(&record)
        .unwrap()
        .write_all(b"{\"status\":\"busy\"}")
        .unwrap();
    assert!(
        fs::metadata(&directory)
            .ok()
            .map(|metadata| FileStamp::of(&metadata))
            == key.stamps[0].1
    );
    assert!(!key.holds(&directory));
    assert!(settle().is_none());
    backdate(&record);
    let key = settle().unwrap();
    assert!(key.holds(&directory));

    fs::remove_file(&record).unwrap();
    assert!(!key.holds(&directory));
}
