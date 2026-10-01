use super::*;

fn submit(worker: &mut SearchWorker, snapshot: &Arc<HistorySearchSnapshot>, text: &str) -> u64 {
    let view_id = TerminalViewId(1);
    let (request_id, latest_request) = worker.next_request(view_id);
    worker.submit(SearchJob {
        request_id,
        view_id,
        screen: Screen::Primary,
        query: SearchQuery::literal(text),
        snapshot: Arc::clone(snapshot),
        selection: SearchSelectionPolicy::Last,
        match_scratch: Vec::new(),
        latest_request,
    });
    request_id
}

fn wait_until(mut ready: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !ready() {
        assert!(
            Instant::now() < deadline,
            "search worker did not make progress"
        );
        thread::sleep(Duration::from_millis(1));
    }
}

#[cfg(target_os = "linux")]
fn search_thread_count() -> usize {
    std::fs::read_dir("/proc/self/task")
        .expect("process threads")
        .filter_map(Result::ok)
        .filter_map(|entry| std::fs::read_to_string(entry.path().join("comm")).ok())
        .filter(|name| name.trim() == "zz-terminal-sea")
        .count()
}

#[test]
fn three_panes_share_one_search_thread_and_drop_stale_requests() {
    let snapshots = ["alpha old alpha", "beta beta beta", "gamma gamma"].map(|text| {
        let mut pane = new_terminal(32, 2, 16).expect("pane terminal");
        pane.vt_write(text.as_bytes());
        Arc::new(HistorySearchSnapshot::capture(&pane).expect("pane history"))
    });
    let mut panes = std::array::from_fn::<_, 3, _>(|_| {
        let ready = Arc::new(AtomicBool::new(false));
        let (worker, results) = SearchWorker::spawn(ActorWake {
            ready: Some(Arc::clone(&ready)),
            ..ActorWake::none()
        });
        (worker, results, ready)
    });
    let gate = snapshots[0].search_gate.lock();
    let stale = submit(&mut panes[0].0, &snapshots[0], "old");
    wait_until(|| panes[0].0.mailbox.jobs.is_empty());
    let beta = submit(&mut panes[1].0, &snapshots[1], "beta");
    let gamma = submit(&mut panes[2].0, &snapshots[2], "gamma");
    let alpha = submit(&mut panes[0].0, &snapshots[0], "alpha");
    assert!(alpha > stale);
    assert!(panes.iter().all(|(_, results, _)| results.is_empty()));
    #[cfg(target_os = "linux")]
    assert_eq!(search_thread_count(), 1);
    drop(gate);

    for ((worker, results, ready), (request_id, query, matches)) in
        panes
            .iter_mut()
            .zip([(alpha, "alpha", 2), (beta, "beta", 3), (gamma, "gamma", 2)])
    {
        let mut completed = results
            .recv_timeout(Duration::from_secs(5))
            .expect("pane search result");
        assert_eq!(completed.by_view.len(), 1);
        let result = completed.by_view.remove(&TerminalViewId(1)).expect("view");
        assert_eq!(result.request_id, request_id);
        assert_eq!(result.state.query.text, query);
        assert_eq!(result.state.matches.len(), matches);
        assert_eq!(result.state.current, Some(matches - 1));
        assert!(worker.is_current(result.view_id, result.request_id));
        wait_until(|| ready.load(Ordering::Acquire));
        assert!(results.is_empty());
    }
    #[cfg(target_os = "linux")]
    assert_eq!(search_thread_count(), 1);
}

#[test]
fn forgotten_and_dropped_panes_cancel_their_search_tokens() {
    let (mut worker, _results) = SearchWorker::spawn(ActorWake::none());
    let view = TerminalViewId(1);
    let (old_request, old_token) = worker.next_request(view);
    worker.forget(view);
    assert_ne!(old_token.load(Ordering::Acquire), old_request);
    assert!(!worker.is_current(view, old_request));
    let (new_request, new_token) = worker.next_request(view);
    assert!(!Arc::ptr_eq(&old_token, &new_token));
    assert!(worker.is_current(view, new_request));
    drop(worker);
    assert_ne!(new_token.load(Ordering::Acquire), new_request);
}

#[test]
fn three_output_panes_receive_their_own_search_results() {
    let panes = ["alpha old alpha", "beta beta beta", "gamma gamma"].map(|text| {
        let pane = TerminalSession::spawn_output_view("search fixture".to_owned(), text.to_owned());
        pane.attach_view(TerminalViewId(1));
        pane.set_view_stream(TerminalViewId(1), ViewStream::Foreground);
        pane
    });
    for pane in &panes {
        wait_until(|| matches!(pane.latest_viewport().mode, TerminalMode::View { .. }));
        pane.view_action(
            TerminalViewId(1),
            TerminalViewAction::SearchBegin(SearchQuery::literal("old")),
        );
    }
    for (pane, query) in panes.iter().zip(["alpha", "beta", "gamma"]) {
        pane.view_action(
            TerminalViewId(1),
            TerminalViewAction::SearchUpdate(SearchQuery::literal(query)),
        );
    }
    for (pane, matches) in panes.iter().zip([2, 3, 2]) {
        wait_until(|| {
            pane.latest_viewport()
                .search
                .is_some_and(|search| !search.pending() && search.total == matches)
        });
    }
    #[cfg(target_os = "linux")]
    assert_eq!(search_thread_count(), 1);
}
