use super::*;

#[test]
fn selected_buffer_created_uses_the_row_override() {
    let mut engine = MuxEngine::default();
    let (session, window, pane) = engine.state.create_session("buffers").unwrap();
    let context = engine.format_status_context(Some(session), Some(window), Some(pane));
    let facts = FormatHookFacts::default();
    let buffer = BufferFormatFacts {
        name: "named".to_owned(),
        data: Arc::from(b"contents".as_slice()),
        created: UNIX_EPOCH + Duration::from_secs(1000),
    };
    let mut hooks = DaemonFormatHooks::command(&facts).with_buffer(buffer);
    let output = expand_format_values("#{buffer_name}:#{buffer_created}", &context, &mut hooks);
    assert_eq!(output, "named:1000");
}

#[test]
fn choose_buffer_filters_each_row_by_its_own_created_time() {
    let mut engine = MuxEngine::default();
    let (session, _, pane) = engine.state.create_session("buffers").unwrap();
    let buffers = [
        PasteBuffer {
            name: "older".to_owned(),
            data: Arc::from(b"old".as_slice()),
            created: UNIX_EPOCH + Duration::from_secs(1000),
            automatic: false,
            utf8: true,
        },
        PasteBuffer {
            name: "newer".to_owned(),
            data: Arc::from(b"new".as_slice()),
            created: UNIX_EPOCH + Duration::from_secs(2000),
            automatic: false,
            utf8: true,
        },
    ];
    let facts = FormatHookFacts::default();
    let chooser = ChooseBufferSession::new(
        pane,
        &engine,
        &buffers,
        Some(session),
        &facts,
        Some("#{==:#{buffer_created},2000}".to_owned()),
        Some("#{buffer_name}:#{buffer_created}".to_owned()),
        false,
        TmuxSort::default(),
        None,
    )
    .unwrap()
    .unwrap();
    let output = chooser
        .rendered
        .items
        .iter()
        .map(|item| item.text.clone())
        .collect::<Vec<_>>();
    assert_eq!(output, vec!["newer:2000"]);
}
