use super::*;

#[test]
fn listing_chunks_keep_binding_order_padding_and_generation() {
    let mut engine = MuxEngine {
        keys: KeyTables::empty(),
        ..MuxEngine::default()
    };
    for row in 0..150 {
        engine.keys.bind(
            &format!("table{row:03}"),
            "a",
            Binding {
                commands: vec![CommandInvocation::new("display-message", [row.to_string()])],
                repeat: row == 149,
                note: None,
            },
        );
    }
    Arc::make_mut(&mut engine.server_user_options)
        .insert("@frozen".to_owned(), "before".to_owned());
    let context = ExecutionContext::default();
    let args = vec![
        RawText::from("-F"),
        RawText::from(
            "#{key_table} #{key_string} #{key_command} #{key_has_repeat} #{key_table_width} #{@frozen}",
        ),
    ];
    let expected = engine
        .list_keys(&context, &args, &mut CommandHooks::new(0))
        .expect("whole listing")
        .output;
    let mut listing = engine
        .start_key_listing(&context, &args, &mut CommandHooks::new(0))
        .expect("listing");
    assert!(
        engine
            .step_key_listing(&mut listing, &mut CommandHooks::new(0))
            .is_none()
    );
    assert_eq!(listing.next, 64);
    Arc::make_mut(&mut engine.server_user_options).insert("@frozen".to_owned(), "after".to_owned());
    engine.keys.remove_table("table149");
    engine.keys.bind(
        "later",
        "b",
        Binding {
            commands: Vec::new(),
            repeat: false,
            note: None,
        },
    );
    assert!(
        engine
            .step_key_listing(&mut listing, &mut CommandHooks::new(0))
            .is_none()
    );
    assert_eq!(listing.next, 128);
    let actual = engine
        .step_key_listing(&mut listing, &mut CommandHooks::new(0))
        .expect("last step")
        .output;
    assert_eq!(actual, expected);
}
