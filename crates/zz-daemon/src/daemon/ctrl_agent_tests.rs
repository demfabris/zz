use super::*;

#[test]
fn cold_session_reset_keeps_queued_prompt_text_for_its_single_turn() {
    for restoring in [false, true] {
        let mut lane = PaneLane::new(1, AgentProvider::Codex, None);
        lane.pending_prompts.push_back("review this".to_owned());
        assert!(
            lane.project(&AgentStreamPayload::SessionReset { restoring })
                .is_empty()
        );
        let bytes = lane.project(&AgentStreamPayload::TurnStarted { turn_id: 1 });
        let text = String::from_utf8(bytes).expect("projected prompt");
        assert_eq!(text.matches("review this").count(), 1, "{text:?}");
        assert!(lane.pending_prompts.is_empty());
        let bytes = lane.project(&AgentStreamPayload::TurnStarted { turn_id: 2 });
        assert!(
            !String::from_utf8(bytes)
                .expect("next turn")
                .contains("review this")
        );
    }
}

#[test]
fn an_explicit_reclaim_removes_only_its_queued_projection() {
    let mut lane = PaneLane::new(1, AgentProvider::Codex, None);
    lane.pending_prompts
        .extend(["reclaimed".to_owned(), "retained".to_owned()]);
    lane.project(&AgentStreamPayload::PromptsReclaimed {
        prompts: vec![AgentPrompt {
            owner: ClientInstanceId::default(),
            text: "reclaimed".to_owned(),
            images: Vec::new(),
        }],
    });
    let text = String::from_utf8(lane.project(&AgentStreamPayload::TurnStarted { turn_id: 1 }))
        .expect("remaining projection");
    assert!(!text.contains("reclaimed"), "{text:?}");
    assert_eq!(text.matches("retained").count(), 1, "{text:?}");
    assert!(lane.pending_prompts.is_empty());
}

#[test]
fn runtime_restart_drops_old_projection_text_before_the_new_turn() {
    let mut lane = PaneLane::new(1, AgentProvider::Codex, None);
    lane.pending_prompts.push_back("old generation".to_owned());
    lane.restart(2, AgentProvider::Codex, None);
    lane.pending_prompts.push_back("new generation".to_owned());
    lane.project(&AgentStreamPayload::SessionReset { restoring: false });
    let text = String::from_utf8(lane.project(&AgentStreamPayload::TurnStarted { turn_id: 1 }))
        .expect("new projection");
    assert!(!text.contains("old generation"), "{text:?}");
    assert_eq!(text.matches("new generation").count(), 1, "{text:?}");
    assert!(lane.pending_prompts.is_empty());
}
