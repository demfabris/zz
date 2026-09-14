use super::*;
use zz_terminal::TerminalMode;

fn viewport_text(viewport: &TerminalViewport) -> String {
    (0..viewport.rows)
        .map(|row| {
            let mut text = String::new();
            for cell in viewport.row(row).unwrap_or_default() {
                if matches!(cell.width(), CellWidth::SpacerTail | CellWidth::SpacerHead) {
                    continue;
                }
                match viewport.glyph(*cell) {
                    Glyph::Empty => text.push(' '),
                    Glyph::Scalar(value) => text.push(value),
                    Glyph::Grapheme(value) => text.push_str(value),
                }
            }
            text.trim_end().to_owned()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn tmux_state(core: &ClientCore) -> serde_json::Value {
    let copies: Vec<_> = core
        .snapshot()
        .sessions
        .iter()
        .filter(|session| Some(session.id) == core.attached_session())
        .flat_map(|session| &session.windows)
        .flat_map(|window| window.panes.values())
        .filter_map(|pane| {
            let viewport = core.viewport(pane.id)?;
            let (position, total) = match viewport.mode {
                TerminalMode::Live => return None,
                TerminalMode::Copy {
                    position, total, ..
                }
                | TerminalMode::View { position, total } => (position, total),
            };
            Some(
                serde_json::json!({"pane": pane.id.0, "position": position, "total": total,
                "matches": viewport.search.map(|search| search.total),
                "match_index": viewport.search.map(|search| search.current())}),
            )
        })
        .collect();
    serde_json::json!({
        "prompt": core.command_prompt(),
        "tree": core.choose_tree(),
        "buffers": core.choose_buffer(),
        "display": core.display_panes(),
        "confirm": core.confirm(),
        "menu": core.menu(),
        "output": core.command_output().map(|(pane, viewport)| serde_json::json!({"pane": pane.0, "text": viewport_text(viewport)})),
        "copies": copies,
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_client_tmux_state_json(client: *const ZzClient) -> *mut ZzJson {
    let Some(client) = (unsafe { client.as_ref() }) else {
        return std::ptr::null_mut();
    };
    Box::into_raw(Box::new(ZzJson::new(tmux_state(&lock(&client.core)))))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_client_key_tables_json(client: *const ZzClient) -> *mut ZzJson {
    let Some(client) = (unsafe { client.as_ref() }) else {
        return std::ptr::null_mut();
    };
    let core = lock(&client.core);
    let tables: Vec<_> = core.key_tables().iter().map(|table| serde_json::json!({
        "name": table.name,
        "bindings": table.bindings.iter().map(|binding| serde_json::json!({
            "key": binding.key, "summary": prefix_command_summary(binding), "note": binding.note, "repeats": binding.repeat,
        })).collect::<Vec<_>>()
    })).collect();
    Box::into_raw(Box::new(ZzJson::new(serde_json::json!(tables))))
}

fn tmux_action(json: &str) -> Option<InputMessage> {
    let input: InputMessage = serde_json::from_str(json).ok()?;
    match input {
        InputMessage::ChooseTree { .. }
        | InputMessage::ChooseBuffer { .. }
        | InputMessage::DisplayPanes { .. }
        | InputMessage::CommandPrompt { .. }
        | InputMessage::Confirm { .. }
        | InputMessage::CommandOutputView { .. }
        | InputMessage::TerminalView { .. }
        | InputMessage::Menu { .. }
        | InputMessage::Popup { .. } => Some(input),
        _ => None,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_client_tmux_action_json(
    client: *mut ZzClient,
    json: *const c_char,
) -> bool {
    let Some(client) = (unsafe { client.as_ref() }) else {
        return false;
    };
    let Some(json) = (unsafe { c_string(json) }) else {
        return false;
    };
    tmux_action(&json).is_some_and(|input| client.client.send_input(input).is_ok())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_viewport_overlays(
    viewport: *const ZzViewport,
) -> *const zz_terminal::OverlaySpan {
    unsafe { viewport.as_ref() }.map_or(std::ptr::null(), |viewport| viewport.0.overlays.as_ptr())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_viewport_overlay_count(viewport: *const ZzViewport) -> usize {
    unsafe { viewport.as_ref() }.map_or(0, |viewport| viewport.0.overlays.len())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_client_overlay_color(client: *const ZzClient, kind: u8) -> u32 {
    let Some(client) = (unsafe { client.as_ref() }) else {
        return 0;
    };
    let core = lock(&client.core);
    let Some(appearance) = core.appearance() else {
        return 0;
    };
    let color = match kind {
        0 => appearance.selection_background,
        1 => appearance.search_match_color,
        2 => appearance.search_current_color,
        3 => zz_terminal::AppearanceColor::opaque(appearance.link_color),
        4 => appearance.copy_cursor_color,
        _ => zz_terminal::AppearanceColor::opaque(appearance.selection_foreground),
    };
    u32::from(color.a) << 24
        | u32::from(color.r) << 16
        | u32::from(color.g) << 8
        | u32::from(color.b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlay_actions_preserve_daemon_semantics() {
        assert_eq!(
            tmux_action(r#"{"ChooseTree":{"action":{"ActivateIndex":2}}}"#),
            Some(InputMessage::ChooseTree {
                action: zz_protocol::ChooseTreeAction::ActivateIndex(2)
            })
        );
        assert_eq!(
            tmux_action(r#"{"CommandPrompt":{"action":{"Submit":{"input":"hello world"}}}}"#),
            Some(InputMessage::CommandPrompt {
                action: zz_protocol::CommandPromptAction::Submit {
                    input: "hello world".into()
                }
            })
        );
        assert!(tmux_action(r#"{"Text":{"pane":1,"text":"oops"}}"#).is_none());
    }

    #[test]
    fn absent_overlays_and_null_handles_are_empty() {
        let state = tmux_state(&ClientCore::new());
        assert!(state["prompt"].is_null());
        assert!(state["tree"].is_null());
        assert_eq!(state["copies"], serde_json::json!([]));
        assert!(unsafe { zz_client_tmux_state_json(std::ptr::null()) }.is_null());
        assert!(unsafe { zz_client_key_tables_json(std::ptr::null()) }.is_null());
        assert!(!unsafe { zz_client_tmux_action_json(std::ptr::null_mut(), std::ptr::null()) });
    }
}
