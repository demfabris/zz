use zz_client::{ChromeAction, ChromeKeymap, SIDEBAR_TABLE};
use zz_daemon::{
    Endpoint, InteractiveClient, configured_fleet_hosts, validate_fleet_host, write_fleet_host,
};
use zz_protocol::{
    ChooseBufferAction, ChooseTreeAction, CommandInvocation, CommandPromptAction,
    CommandPromptMode, DisplayPanesAction, InputMessage, MAX_COMMAND_PROMPT_BYTES,
};
use zz_terminal::{
    CopyModeAction, KeyAction, KeyCode, KeyInput, Modifiers, PointerCellEvent, TerminalMouseButton,
    TerminalMouseInput, TerminalMousePhase, TerminalViewAction,
};

use crate::{
    browser::BrowserZoomStep,
    browser::{
        BrowserState, ProviderModifiers, ProviderPointerButton, ProviderPointerInput,
        ProviderPointerPhase,
    },
    layout::Rect,
    picker::{self, Action as PickerAction},
    sidebar::{self, EditKind as SidebarEditKind, Target as SidebarTarget},
    state::{ClientMessage, HostSwitch, Model},
    terminal_event::{
        Event as TerminalEvent, KeyCode as TerminalKeyCode, KeyEvent, KeyEventKind, KeyModifiers,
        MouseButton, MouseEvent, MouseEventKind,
    },
};

pub(crate) enum InputOutcome {
    None,
    Repaint,
    RepaintAll,
    Resize(crate::tty::TerminalSize),
    SwitchHost(HostSwitch),
    Detach,
}

pub(crate) fn handle(
    model: &mut Model,
    client: &InteractiveClient,
    browser: &mut BrowserState,
    event: TerminalEvent,
    pixel_mouse: bool,
) -> Result<InputOutcome, String> {
    match event {
        TerminalEvent::CellSize {
            width_px,
            height_px,
        } => Ok(InputOutcome::Resize(
            model.size.with_cell_pixels(width_px, height_px),
        )),
        TerminalEvent::DeviceAttributes | TerminalEvent::KittyGraphicsResponse { .. } => {
            Ok(InputOutcome::None)
        }
        TerminalEvent::FocusGained => {
            send_focus(model, client, true)?;
            Ok(InputOutcome::None)
        }
        TerminalEvent::FocusLost => {
            send_focus(model, client, false)?;
            Ok(InputOutcome::None)
        }
        TerminalEvent::Paste(text) => handle_paste(model, client, text),
        TerminalEvent::Mouse(event) => handle_mouse(model, client, browser, event, pixel_mouse),
        TerminalEvent::Key(event) => handle_key(model, client, browser, event),
    }
}

fn handle_key(
    model: &mut Model,
    client: &InteractiveClient,
    browser: &mut BrowserState,
    event: KeyEvent,
) -> Result<InputOutcome, String> {
    let global_route = global_key_route(&model.chrome, model.sidebar.focused, event);
    if global_route == GlobalKeyRoute::Detach {
        client.detach().map_err(|error| error.to_string())?;
        return Ok(InputOutcome::Detach);
    }
    if model.sidebar_edit.is_some() {
        return handle_sidebar_edit_key(model, client, event);
    }
    match global_route {
        GlobalKeyRoute::Detach => {
            unreachable!("detach handled before the sidebar editor")
        }
        GlobalKeyRoute::ToggleSidebar => {
            return Ok(if model.toggle_sidebar_focus() {
                InputOutcome::RepaintAll
            } else {
                InputOutcome::Repaint
            });
        }
        GlobalKeyRoute::Sidebar => return handle_sidebar_key(model, client, event),
        GlobalKeyRoute::Other => {}
    }

    if model.command_prompt.is_some() {
        return handle_command_prompt(model, client, event);
    }
    if model.choose_tree.is_some() {
        if event.kind != KeyEventKind::Release {
            client
                .send_input(InputMessage::ChooseTree {
                    action: ChooseTreeAction::Key(key_input(event)),
                })
                .map_err(|error| error.to_string())?;
        }
        return Ok(InputOutcome::None);
    }
    if model.choose_buffer.is_some() {
        if event.kind != KeyEventKind::Release {
            client
                .send_input(InputMessage::ChooseBuffer {
                    action: ChooseBufferAction::Key(key_input(event)),
                })
                .map_err(|error| error.to_string())?;
        }
        return Ok(InputOutcome::None);
    }
    if model.display_panes.is_some() {
        if event.kind != KeyEventKind::Release {
            client
                .send_input(InputMessage::DisplayPanes {
                    action: DisplayPanesAction::Key(key_input(event)),
                })
                .map_err(|error| error.to_string())?;
        }
        return Ok(InputOutcome::None);
    }
    if model.command_output.is_some() {
        if event.kind != KeyEventKind::Release && matches!(event.code, TerminalKeyCode::Esc) {
            client
                .send_input(InputMessage::CommandOutputView {
                    action: TerminalViewAction::CopyMode(CopyModeAction::Cancel),
                })
                .map_err(|error| error.to_string())?;
        }
        return Ok(InputOutcome::None);
    }

    if let Some(pane) = model.active_picker() {
        return handle_picker_key(model, client, pane, event);
    }

    let Some(pane) = model.active_pane() else {
        return Ok(InputOutcome::None);
    };
    if event.kind == KeyEventKind::Press
        && let Some(step) = match model.chrome.resolve("browser", &key_input(event)) {
            Some(ChromeAction::BrowserZoomIn) => Some(BrowserZoomStep::In),
            Some(ChromeAction::BrowserZoomOut) => Some(BrowserZoomStep::Out),
            Some(ChromeAction::BrowserZoomReset) => Some(BrowserZoomStep::Reset),
            _ => None,
        }
        && browser.zoom(pane, step)
    {
        return Ok(InputOutcome::None);
    }
    let kitty_keyboard = model
        .viewports
        .get(&pane)
        .is_some_and(|viewport| viewport.kitty_keyboard);
    if !should_forward_key(event.kind, browser.has_surface(pane), kitty_keyboard) {
        return Ok(InputOutcome::None);
    }
    client
        .send_input(InputMessage::Key {
            pane,
            input: key_input(event),
            text_follows: false,
        })
        .map_err(|error| error.to_string())?;
    Ok(InputOutcome::None)
}

const fn should_forward_key(
    kind: KeyEventKind,
    browser_surface: bool,
    kitty_keyboard: bool,
) -> bool {
    !matches!(kind, KeyEventKind::Release) || browser_surface || kitty_keyboard
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum GlobalKeyRoute {
    Detach,
    ToggleSidebar,
    Sidebar,
    Other,
}

fn global_key_route(
    chrome: &ChromeKeymap,
    sidebar_focused: bool,
    event: KeyEvent,
) -> GlobalKeyRoute {
    if event.kind == KeyEventKind::Press {
        match chrome.resolve("ui", &key_input(event)) {
            Some(ChromeAction::Detach) => return GlobalKeyRoute::Detach,
            Some(ChromeAction::ToggleSidebar) => return GlobalKeyRoute::ToggleSidebar,
            _ => {}
        }
    }
    if sidebar_focused {
        GlobalKeyRoute::Sidebar
    } else {
        GlobalKeyRoute::Other
    }
}

fn handle_sidebar_key(
    model: &mut Model,
    client: &InteractiveClient,
    event: KeyEvent,
) -> Result<InputOutcome, String> {
    if event.kind == KeyEventKind::Release {
        return Ok(InputOutcome::None);
    }
    let Some(action) = model.chrome.resolve(SIDEBAR_TABLE, &key_input(event)) else {
        return Ok(InputOutcome::None);
    };
    match action {
        ChromeAction::SidebarSelectUp => model.move_sidebar_selection(-1),
        ChromeAction::SidebarSelectDown => model.move_sidebar_selection(1),
        ChromeAction::SidebarConfirm => {
            if let Some(target) = model.selected_sidebar_target()
                && let Some(host) = activate_sidebar_target(model, client, target)?
            {
                return Ok(InputOutcome::SwitchHost(host));
            }
        }
        ChromeAction::SidebarRename => model.begin_sidebar_rename(),
        ChromeAction::SidebarCancel => model.sidebar.focused = false,
        ChromeAction::ToggleSidebar => {
            return Ok(if model.hide_sidebar() {
                InputOutcome::RepaintAll
            } else {
                InputOutcome::Repaint
            });
        }
        _ => return Ok(InputOutcome::None),
    }
    Ok(InputOutcome::Repaint)
}

fn activate_sidebar_target(
    model: &mut Model,
    client: &InteractiveClient,
    target: SidebarTarget,
) -> Result<Option<HostSwitch>, String> {
    match target {
        SidebarTarget::Session(session) => {
            client
                .attach(session.to_string())
                .map_err(|error| error.to_string())?;
            Ok(None)
        }
        SidebarTarget::Window(window) => {
            execute_target(client, "select-window", window.to_string())?;
            Ok(None)
        }
        SidebarTarget::Pane(pane) => {
            focus_pane(client, pane)?;
            model.sidebar.focused = false;
            Ok(None)
        }
        SidebarTarget::NewPane(target) => {
            execute_target(client, "split-picker", target.to_string())?;
            model.sidebar.focused = false;
            Ok(None)
        }
        SidebarTarget::LocalHost | SidebarTarget::FleetHost(_) => Ok(model.host_switch(target)),
        SidebarTarget::AddHost => {
            model.begin_add_host();
            Ok(None)
        }
    }
}

fn execute_target(
    client: &InteractiveClient,
    command: &'static str,
    target: String,
) -> Result<(), String> {
    client
        .execute(CommandInvocation::new(command, ["-t".to_owned(), target]))
        .map(drop)
        .map_err(|error| error.to_string())
}

fn handle_sidebar_edit_key(
    model: &mut Model,
    client: &InteractiveClient,
    event: KeyEvent,
) -> Result<InputOutcome, String> {
    if event.kind == KeyEventKind::Release {
        return Ok(InputOutcome::None);
    }
    match event.code {
        TerminalKeyCode::Esc => model.sidebar_edit = None,
        TerminalKeyCode::Enter => commit_sidebar_edit(model, client)?,
        TerminalKeyCode::Left => model
            .sidebar_edit
            .as_mut()
            .expect("sidebar editor checked above")
            .move_left(),
        TerminalKeyCode::Right => model
            .sidebar_edit
            .as_mut()
            .expect("sidebar editor checked above")
            .move_right(),
        TerminalKeyCode::Home => model
            .sidebar_edit
            .as_mut()
            .expect("sidebar editor checked above")
            .move_home(),
        TerminalKeyCode::End => model
            .sidebar_edit
            .as_mut()
            .expect("sidebar editor checked above")
            .move_end(),
        TerminalKeyCode::Backspace => model
            .sidebar_edit
            .as_mut()
            .expect("sidebar editor checked above")
            .backspace(),
        TerminalKeyCode::Delete => model
            .sidebar_edit
            .as_mut()
            .expect("sidebar editor checked above")
            .delete(),
        TerminalKeyCode::Char(character)
            if !event
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER) =>
        {
            model
                .sidebar_edit
                .as_mut()
                .expect("sidebar editor checked above")
                .insert_char(character);
        }
        _ => {}
    }
    Ok(InputOutcome::Repaint)
}

fn commit_sidebar_edit(model: &mut Model, client: &InteractiveClient) -> Result<(), String> {
    let Some(edit) = model.sidebar_edit.as_ref() else {
        return Ok(());
    };
    let kind = edit.kind;
    let buffer = edit.buffer.clone();
    match kind {
        SidebarEditKind::RenameSession(_) | SidebarEditKind::RenameWindow(_) => {
            let Some(command) = rename_command(kind, &buffer) else {
                model.sidebar_edit = None;
                return Ok(());
            };
            client
                .execute(command)
                .map(drop)
                .map_err(|error| error.to_string())?;
            model.sidebar_edit = None;
        }
        SidebarEditKind::AddHost => {
            let (name, endpoint) = match parse_add_host_input(&buffer) {
                Ok(parsed) => parsed,
                Err(error) => {
                    model.client_message = Some(ClientMessage::local(error));
                    return Ok(());
                }
            };
            if let Err(error) = write_fleet_host(&name, &endpoint) {
                model.client_message = Some(ClientMessage::local(format!(
                    "could not write zz/config: {error}"
                )));
                return Ok(());
            }
            let (hosts, _) = match configured_fleet_hosts() {
                Ok(configured) => configured,
                Err(error) => {
                    model.client_message = Some(ClientMessage::local(format!(
                        "could not read zz/config: {error}"
                    )));
                    return Ok(());
                }
            };
            model.refresh_fleet_hosts(hosts);
            model.sidebar_edit = None;
            model.client_message = Some(ClientMessage::local(format!("host {name} added")));
        }
    }
    Ok(())
}

fn rename_command(kind: SidebarEditKind, name: &str) -> Option<CommandInvocation> {
    if name.is_empty() {
        return None;
    }
    let (command, target) = match kind {
        SidebarEditKind::RenameSession(session) => ("rename-session", session.to_string()),
        SidebarEditKind::RenameWindow(window) => ("rename-window", window.to_string()),
        SidebarEditKind::AddHost => return None,
    };
    Some(CommandInvocation::new(
        command,
        ["-t".to_owned(), target, name.to_owned()],
    ))
}

fn parse_add_host_input(input: &str) -> Result<(String, String), String> {
    let input = input.trim();
    let Some((name, destination)) = input.split_once(' ') else {
        return Err("add host expects <name> <ssh-destination>".to_owned());
    };
    let destination = destination.trim();
    if name.is_empty() || destination.is_empty() {
        return Err("add host expects <name> <ssh-destination>".to_owned());
    }
    let destination = destination.strip_prefix("ssh://").unwrap_or(destination);
    if destination.starts_with('-') {
        return Err("ssh destination must not start with `-`".to_owned());
    }
    let endpoint = format!("ssh://{destination}");
    Endpoint::parse(&endpoint).map_err(|error| error.to_string())?;
    validate_fleet_host(name, &endpoint)?;
    Ok((name.to_owned(), endpoint))
}

fn handle_picker_key(
    model: &mut Model,
    client: &InteractiveClient,
    pane: zz_protocol::PaneId,
    event: KeyEvent,
) -> Result<InputOutcome, String> {
    let Some(action) = picker::key_action(event, model.picker_selection) else {
        return Ok(InputOutcome::None);
    };
    match action {
        PickerAction::Previous => model.move_picker_selection(-1),
        PickerAction::Next => model.move_picker_selection(1),
        PickerAction::Materialize(choice) => {
            client
                .execute(CommandInvocation::new(
                    "select-pane-kind",
                    [
                        "-t".to_owned(),
                        pane.to_string(),
                        choice.argument().to_owned(),
                    ],
                ))
                .map(drop)
                .map_err(|error| error.to_string())?;
        }
        PickerAction::Cancel => {
            execute_target(client, "kill-pane", pane.to_string())?;
        }
    }
    Ok(InputOutcome::Repaint)
}

/// `-1`, `-N` and `-k` are decided key by key inside the daemon, so the TUI
/// stops editing and relays the press on the pane-targeted key path instead.
const fn prompt_relays_keys(mode: CommandPromptMode) -> bool {
    matches!(
        mode,
        CommandPromptMode::Single | CommandPromptMode::Numeric | CommandPromptMode::Key
    )
}

fn handle_command_prompt(
    model: &mut Model,
    client: &InteractiveClient,
    event: KeyEvent,
) -> Result<InputOutcome, String> {
    if event.kind == KeyEventKind::Release {
        return Ok(InputOutcome::None);
    }
    let mode = model
        .command_prompt
        .as_ref()
        .expect("command prompt checked above")
        .mode;
    if prompt_relays_keys(mode) {
        if let Some(pane) = model.active_pane() {
            client
                .send_input(InputMessage::Key {
                    pane,
                    input: key_input(event),
                    text_follows: false,
                })
                .map_err(|error| error.to_string())?;
        }
        return Ok(InputOutcome::None);
    }
    let state = model
        .command_prompt
        .as_mut()
        .expect("command prompt checked above");
    let mut changed = false;
    let action = match event.code {
        TerminalKeyCode::Esc => Some(CommandPromptAction::Close),
        TerminalKeyCode::Enter => Some(CommandPromptAction::Submit {
            input: state.input.clone(),
        }),
        TerminalKeyCode::Left => {
            state.cursor = state.cursor.saturating_sub(1);
            changed = true;
            None
        }
        TerminalKeyCode::Right => {
            state.cursor = state
                .cursor
                .saturating_add(1)
                .min(u32::try_from(state.input.chars().count()).unwrap_or(u32::MAX));
            changed = true;
            None
        }
        TerminalKeyCode::Home => {
            state.cursor = 0;
            changed = true;
            None
        }
        TerminalKeyCode::End => {
            state.cursor = u32::try_from(state.input.chars().count()).unwrap_or(u32::MAX);
            changed = true;
            None
        }
        TerminalKeyCode::Backspace => {
            if state.cursor > 0 {
                let end = scalar_byte_index(&state.input, state.cursor);
                let start = scalar_byte_index(&state.input, state.cursor - 1);
                state.input.replace_range(start..end, "");
                state.cursor -= 1;
                changed = true;
                None
            } else if mode == CommandPromptMode::BackspaceExit && state.input.is_empty() {
                Some(CommandPromptAction::Close)
            } else {
                None
            }
        }
        TerminalKeyCode::Delete => {
            let start = scalar_byte_index(&state.input, state.cursor);
            let end = scalar_byte_index(&state.input, state.cursor.saturating_add(1));
            if start < end {
                state.input.replace_range(start..end, "");
                changed = true;
            }
            None
        }
        TerminalKeyCode::Char(character)
            if !event
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER) =>
        {
            let index = scalar_byte_index(&state.input, state.cursor);
            if state.input.len().saturating_add(character.len_utf8()) <= MAX_COMMAND_PROMPT_BYTES {
                state.input.insert(index, character);
                state.cursor = state.cursor.saturating_add(1);
                changed = true;
            }
            None
        }
        _ => None,
    };
    if let Some(action) = action {
        client
            .send_input(InputMessage::CommandPrompt { action })
            .map_err(|error| error.to_string())?;
    } else if changed {
        client
            .send_input(InputMessage::CommandPrompt {
                action: CommandPromptAction::Update {
                    input: state.input.clone(),
                    cursor: state.cursor,
                },
            })
            .map_err(|error| error.to_string())?;
    }
    Ok(InputOutcome::Repaint)
}

fn scalar_byte_index(text: &str, scalar: u32) -> usize {
    text.char_indices()
        .nth(usize::try_from(scalar).unwrap_or(usize::MAX))
        .map_or(text.len(), |(index, _)| index)
}

fn handle_paste(
    model: &mut Model,
    client: &InteractiveClient,
    text: String,
) -> Result<InputOutcome, String> {
    if let Some(edit) = model.sidebar_edit.as_mut() {
        edit.insert_text(&text);
        return Ok(InputOutcome::Repaint);
    }
    if model.sidebar.focused || model.active_picker().is_some() {
        return Ok(InputOutcome::None);
    }
    if let Some(state) = model.command_prompt.as_mut() {
        if prompt_relays_keys(state.mode) {
            return Ok(InputOutcome::None);
        }
        let available = MAX_COMMAND_PROMPT_BYTES.saturating_sub(state.input.len());
        let mut end = text.len().min(available);
        while !text.is_char_boundary(end) {
            end = end.saturating_sub(1);
        }
        let index = scalar_byte_index(&state.input, state.cursor);
        state.input.insert_str(index, &text[..end]);
        state.cursor = state
            .cursor
            .saturating_add(u32::try_from(text[..end].chars().count()).unwrap_or(u32::MAX));
        client
            .send_input(InputMessage::CommandPrompt {
                action: CommandPromptAction::Update {
                    input: state.input.clone(),
                    cursor: state.cursor,
                },
            })
            .map_err(|error| error.to_string())?;
        return Ok(InputOutcome::Repaint);
    }
    let Some(pane) = model.active_pane() else {
        return Ok(InputOutcome::None);
    };
    client
        .send_input(InputMessage::TerminalView {
            pane,
            action: TerminalViewAction::Paste(text),
        })
        .map_err(|error| error.to_string())?;
    Ok(InputOutcome::None)
}

fn handle_mouse(
    model: &mut Model,
    client: &InteractiveClient,
    browser: &mut BrowserState,
    event: MouseEvent,
    pixel_mouse: bool,
) -> Result<InputOutcome, String> {
    let (global_column, global_row, global_x, global_y) = if pixel_mouse {
        (
            u16::try_from(u32::from(event.column) / model.size.cell_width_px).unwrap_or(u16::MAX),
            u16::try_from(u32::from(event.row) / model.size.cell_height_px).unwrap_or(u16::MAX),
            u32::from(event.column),
            u32::from(event.row),
        )
    } else {
        (
            event.column,
            event.row,
            u32::from(event.column).saturating_mul(model.size.cell_width_px),
            u32::from(event.row).saturating_mul(model.size.cell_height_px),
        )
    };
    if !model.mouse_option {
        if let Some((pane, action)) =
            app_mouse_forward_action(model, event, global_column, global_row, global_x, global_y)
        {
            client
                .send_input(InputMessage::TerminalView { pane, action })
                .map_err(|error| error.to_string())?;
        }
        return Ok(InputOutcome::None);
    }
    if model.sidebar_edit.is_some() {
        return Ok(InputOutcome::None);
    }
    if model.sidebar_visible() && global_column < sidebar::WIDTH {
        match event.kind {
            MouseEventKind::ScrollUp => model.scroll_sidebar(-3),
            MouseEventKind::ScrollDown => model.scroll_sidebar(3),
            MouseEventKind::Down(MouseButton::Left) if global_row < model.sidebar_tree_height() => {
                if let Some(target) = model.select_sidebar_row(global_row)
                    && let Some(host) = activate_sidebar_target(model, client, target)?
                {
                    return Ok(InputOutcome::SwitchHost(host));
                }
            }
            _ => return Ok(InputOutcome::None),
        }
        return Ok(InputOutcome::Repaint);
    }
    if model.sidebar_visible() && global_column == sidebar::WIDTH {
        return Ok(InputOutcome::None);
    }
    let sidebar_focus_changed =
        model.sidebar.focused && matches!(event.kind, MouseEventKind::Down(MouseButton::Left));
    if sidebar_focus_changed {
        model.sidebar.focused = false;
    }
    if let Some(index) = model.status_row_at(global_row) {
        let (status_x, _) = model.status_area();
        if matches!(event.kind, MouseEventKind::Down(MouseButton::Left))
            && global_column >= status_x
            && let Some(zz_protocol::TmuxRange::Window(window)) =
                model.status_hit_target(index, global_column - status_x)
        {
            execute_target(client, "select-window", format!(":{window}"))?;
        }
        return Ok(if sidebar_focus_changed {
            InputOutcome::Repaint
        } else {
            InputOutcome::None
        });
    }
    let Some(entry) = model.pane_at(global_column, global_row) else {
        return Ok(if sidebar_focus_changed {
            InputOutcome::Repaint
        } else {
            InputOutcome::None
        });
    };
    let content = entry.rect.content();
    if !content.contains(global_column, global_row) {
        if matches!(event.kind, MouseEventKind::Down(_)) {
            focus_pane(client, entry.pane)?;
        }
        return Ok(if sidebar_focus_changed {
            InputOutcome::Repaint
        } else {
            InputOutcome::None
        });
    }
    if browser.has_surface(entry.pane) {
        if matches!(event.kind, MouseEventKind::Down(_)) {
            focus_pane(client, entry.pane)?;
        }
        let input = browser_pointer_input(
            event,
            content,
            global_x,
            global_y,
            model.size.cell_width_px,
            model.size.cell_height_px,
        );
        browser.pointer(entry.pane, input);
        return Ok(if sidebar_focus_changed {
            InputOutcome::Repaint
        } else {
            InputOutcome::None
        });
    }
    let Some(viewport) = model.viewports.get(&entry.pane) else {
        if matches!(event.kind, MouseEventKind::Down(_)) {
            focus_pane(client, entry.pane)?;
        }
        return Ok(if sidebar_focus_changed {
            InputOutcome::Repaint
        } else {
            InputOutcome::None
        });
    };
    if matches!(event.kind, MouseEventKind::Moved) && !viewport.mouse_tracking {
        return Ok(InputOutcome::None);
    }
    if matches!(event.kind, MouseEventKind::Down(_)) {
        focus_pane(client, entry.pane)?;
    }
    let force_selection = event.modifiers.contains(KeyModifiers::SHIFT) || !viewport.mouse_tracking;
    if let Some(action) = pane_mouse_action(
        &model.size,
        event,
        content,
        global_column,
        global_row,
        global_x,
        global_y,
        force_selection,
    ) {
        client
            .send_input(InputMessage::TerminalView {
                pane: entry.pane,
                action,
            })
            .map_err(|error| error.to_string())?;
    }
    Ok(if sidebar_focus_changed {
        InputOutcome::Repaint
    } else {
        InputOutcome::None
    })
}

/// The pin's `forward_key` route for a disabled `mouse` option: the event goes
/// to the pane under the cursor when that pane's application asked for mouse
/// reporting, and every chrome branch is skipped.
pub(crate) fn app_mouse_forward_action(
    model: &Model,
    event: MouseEvent,
    global_column: u16,
    global_row: u16,
    global_x: u32,
    global_y: u32,
) -> Option<(zz_protocol::PaneId, TerminalViewAction)> {
    let entry = model.pane_at(global_column, global_row)?;
    let content = entry.rect.content();
    if !content.contains(global_column, global_row) {
        return None;
    }
    let viewport = model.viewports.get(&entry.pane)?;
    if !viewport.mouse_tracking {
        return None;
    }
    let action = pane_mouse_action(
        &model.size,
        event,
        content,
        global_column,
        global_row,
        global_x,
        global_y,
        event.modifiers.contains(KeyModifiers::SHIFT),
    )?;
    Some((entry.pane, action))
}

#[expect(clippy::too_many_arguments)]
fn pane_mouse_action(
    size: &crate::tty::TerminalSize,
    event: MouseEvent,
    content: Rect,
    global_column: u16,
    global_row: u16,
    global_x: u32,
    global_y: u32,
    force_selection: bool,
) -> Option<TerminalViewAction> {
    let column = global_column.saturating_sub(content.x);
    let row = global_row.saturating_sub(content.y);
    let x = global_x.saturating_sub(u32::from(content.x).saturating_mul(size.cell_width_px));
    let y = global_y.saturating_sub(u32::from(content.y).saturating_mul(size.cell_height_px));
    let (phase, button) = mouse_routing(event.kind);
    let input = TerminalMouseInput::new(
        phase,
        button,
        PointerCellEvent {
            column,
            row,
            click_count: u8::from(matches!(event.kind, MouseEventKind::Down(_))),
            rectangle: false,
        },
        x,
        y,
        u32::from(content.width).saturating_mul(size.cell_width_px),
        u32::from(content.height).saturating_mul(size.cell_height_px),
        size.cell_width_px,
        size.cell_height_px,
        modifiers(event.modifiers),
        force_selection,
    );
    match event.kind {
        MouseEventKind::ScrollUp => Some(TerminalViewAction::ScrollWheel { lines: -3, input }),
        MouseEventKind::ScrollDown => Some(TerminalViewAction::ScrollWheel { lines: 3, input }),
        MouseEventKind::ScrollLeft | MouseEventKind::ScrollRight => None,
        _ => Some(TerminalViewAction::Mouse(input)),
    }
}

fn focus_pane(client: &InteractiveClient, pane: zz_protocol::PaneId) -> Result<(), String> {
    client
        .execute(CommandInvocation::new(
            "select-pane",
            ["-t".to_owned(), pane.to_string()],
        ))
        .map(drop)
        .map_err(|error| error.to_string())
}

fn send_focus(model: &Model, client: &InteractiveClient, focused: bool) -> Result<(), String> {
    let Some(pane) = model.active_pane() else {
        return Ok(());
    };
    client
        .send_input(InputMessage::TerminalView {
            pane,
            action: TerminalViewAction::Focus(focused),
        })
        .map_err(|error| error.to_string())
}

fn browser_pointer_input(
    event: MouseEvent,
    content: Rect,
    global_x: u32,
    global_y: u32,
    cell_width_px: u32,
    cell_height_px: u32,
) -> ProviderPointerInput {
    const WHEEL_STEP: i32 = 120;

    let (phase, button, wheel_delta_x, wheel_delta_y) = match event.kind {
        MouseEventKind::Down(button) => (
            ProviderPointerPhase::Down,
            Some(provider_button(button)),
            0,
            0,
        ),
        MouseEventKind::Up(button) => (
            ProviderPointerPhase::Up,
            Some(provider_button(button)),
            0,
            0,
        ),
        MouseEventKind::Drag(button) => (
            ProviderPointerPhase::Move,
            Some(provider_button(button)),
            0,
            0,
        ),
        MouseEventKind::Moved => (ProviderPointerPhase::Move, None, 0, 0),
        MouseEventKind::ScrollUp => (ProviderPointerPhase::Wheel, None, 0, WHEEL_STEP),
        MouseEventKind::ScrollDown => (ProviderPointerPhase::Wheel, None, 0, -WHEEL_STEP),
        MouseEventKind::ScrollLeft => (ProviderPointerPhase::Wheel, None, -WHEEL_STEP, 0),
        MouseEventKind::ScrollRight => (ProviderPointerPhase::Wheel, None, WHEEL_STEP, 0),
    };
    let x = global_x.saturating_sub(u32::from(content.x).saturating_mul(cell_width_px));
    let y = global_y.saturating_sub(u32::from(content.y).saturating_mul(cell_height_px));
    ProviderPointerInput {
        x: i32::try_from(x).unwrap_or(i32::MAX),
        y: i32::try_from(y).unwrap_or(i32::MAX),
        phase,
        button,
        click_count: i32::from(matches!(
            event.kind,
            MouseEventKind::Down(_) | MouseEventKind::Up(_)
        )),
        wheel_delta_x,
        wheel_delta_y,
        modifiers: ProviderModifiers::new(
            event.modifiers.contains(KeyModifiers::SHIFT),
            event.modifiers.contains(KeyModifiers::CONTROL),
            event.modifiers.contains(KeyModifiers::ALT),
            event.modifiers.contains(KeyModifiers::SUPER),
        ),
    }
}

const fn provider_button(button: MouseButton) -> ProviderPointerButton {
    match button {
        MouseButton::Left => ProviderPointerButton::Left,
        MouseButton::Middle => ProviderPointerButton::Middle,
        MouseButton::Right => ProviderPointerButton::Right,
    }
}

fn mouse_routing(kind: MouseEventKind) -> (TerminalMousePhase, Option<TerminalMouseButton>) {
    match kind {
        MouseEventKind::Down(button) => (TerminalMousePhase::Press, Some(mouse_button(button))),
        MouseEventKind::Up(button) => (TerminalMousePhase::Release, Some(mouse_button(button))),
        MouseEventKind::Drag(button) => (TerminalMousePhase::Motion, Some(mouse_button(button))),
        MouseEventKind::Moved => (TerminalMousePhase::Motion, None),
        MouseEventKind::ScrollUp => (
            TerminalMousePhase::Press,
            Some(TerminalMouseButton::ScrollUp),
        ),
        MouseEventKind::ScrollDown => (
            TerminalMousePhase::Press,
            Some(TerminalMouseButton::ScrollDown),
        ),
        MouseEventKind::ScrollLeft | MouseEventKind::ScrollRight => {
            (TerminalMousePhase::Press, None)
        }
    }
}

const fn mouse_button(button: MouseButton) -> TerminalMouseButton {
    match button {
        MouseButton::Left => TerminalMouseButton::Left,
        MouseButton::Middle => TerminalMouseButton::Middle,
        MouseButton::Right => TerminalMouseButton::Right,
    }
}

fn key_input(event: KeyEvent) -> KeyInput {
    let key = key_code(event.code);
    let text = match event.code {
        TerminalKeyCode::Char(character) if !character.is_control() => {
            Some(character.to_string().into_boxed_str())
        }
        _ => None,
    };
    let unshifted_codepoint = match key {
        KeyCode::Character(character) => Some(character),
        _ => None,
    };
    KeyInput {
        action: match event.kind {
            KeyEventKind::Press => KeyAction::Press,
            KeyEventKind::Repeat => KeyAction::Repeat,
            KeyEventKind::Release => KeyAction::Release,
        },
        key,
        modifiers: modifiers(event.modifiers),
        text,
        unshifted_codepoint,
    }
}

fn key_code(code: TerminalKeyCode) -> KeyCode {
    match code {
        TerminalKeyCode::Char(character) => KeyCode::Character(if character.is_ascii_uppercase() {
            character.to_ascii_lowercase()
        } else {
            character
        }),
        TerminalKeyCode::Backspace => KeyCode::Backspace,
        TerminalKeyCode::Enter => KeyCode::Enter,
        TerminalKeyCode::Tab | TerminalKeyCode::BackTab => KeyCode::Tab,
        TerminalKeyCode::Esc => KeyCode::Escape,
        TerminalKeyCode::Delete => KeyCode::Delete,
        TerminalKeyCode::Insert => KeyCode::Insert,
        TerminalKeyCode::Home => KeyCode::Home,
        TerminalKeyCode::End => KeyCode::End,
        TerminalKeyCode::PageUp => KeyCode::PageUp,
        TerminalKeyCode::PageDown => KeyCode::PageDown,
        TerminalKeyCode::Up => KeyCode::ArrowUp,
        TerminalKeyCode::Down => KeyCode::ArrowDown,
        TerminalKeyCode::Left => KeyCode::ArrowLeft,
        TerminalKeyCode::Right => KeyCode::ArrowRight,
        TerminalKeyCode::F(number) => KeyCode::Function(number),
        TerminalKeyCode::Unidentified => KeyCode::Unidentified,
    }
}

const fn modifiers(value: KeyModifiers) -> Modifiers {
    Modifiers::new(
        value.contains(KeyModifiers::SHIFT),
        value.contains(KeyModifiers::CONTROL),
        value.contains(KeyModifiers::ALT),
        value.contains(KeyModifiers::SUPER),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scalar_index_clamps_and_tracks_multibyte_text() {
        assert_eq!(scalar_byte_index("aéz", 0), 0);
        assert_eq!(scalar_byte_index("aéz", 1), 1);
        assert_eq!(scalar_byte_index("aéz", 2), 3);
        assert_eq!(scalar_byte_index("aéz", 99), 4);
    }

    #[test]
    fn key_mapping_keeps_typed_text_and_normalizes_letter_identity() {
        let input = key_input(KeyEvent::new(
            TerminalKeyCode::Char('A'),
            KeyModifiers::SHIFT,
        ));
        assert_eq!(input.key, KeyCode::Character('a'));
        assert_eq!(input.text.as_deref(), Some("A"));
        assert!(input.modifiers.shift());
    }

    /// `-1`, `-N` and `-k` are decided key by key inside the daemon, so the
    /// TUI must stop editing and relay their presses; text modes keep the
    /// local editor.
    #[test]
    fn key_reading_prompt_modes_relay_instead_of_editing() {
        for mode in [
            CommandPromptMode::Single,
            CommandPromptMode::Numeric,
            CommandPromptMode::Key,
        ] {
            assert!(prompt_relays_keys(mode), "{mode:?}");
        }
        for mode in [
            CommandPromptMode::Text,
            CommandPromptMode::Incremental,
            CommandPromptMode::BackspaceExit,
        ] {
            assert!(!prompt_relays_keys(mode), "{mode:?}");
        }
    }

    #[test]
    fn browser_surfaces_forward_key_releases_through_the_daemon() {
        assert!(should_forward_key(KeyEventKind::Release, true, false));
        assert!(should_forward_key(KeyEventKind::Release, false, true));
        assert!(!should_forward_key(KeyEventKind::Release, false, false));
        assert!(should_forward_key(KeyEventKind::Press, false, false));
    }

    #[test]
    fn sidebar_focus_routes_regular_keys_locally_before_daemon_input() {
        let ordinary = KeyEvent::new(TerminalKeyCode::Char('x'), KeyModifiers::NONE);
        let alt_s = KeyEvent::new(TerminalKeyCode::Char('s'), KeyModifiers::ALT);
        let detach = KeyEvent::new(TerminalKeyCode::Char('\\'), KeyModifiers::CONTROL);

        let chrome = ChromeKeymap::new();
        assert_eq!(
            global_key_route(&chrome, true, ordinary),
            GlobalKeyRoute::Sidebar
        );
        assert_eq!(
            global_key_route(&chrome, true, alt_s),
            GlobalKeyRoute::ToggleSidebar
        );
        assert_eq!(
            global_key_route(&chrome, true, detach),
            GlobalKeyRoute::Detach
        );
        assert_eq!(
            global_key_route(&chrome, false, ordinary),
            GlobalKeyRoute::Other
        );
    }

    #[test]
    fn sidebar_keys_come_from_the_chrome_table() {
        let mut chrome = ChromeKeymap::new();
        let key = KeyEvent::new(TerminalKeyCode::Char('k'), KeyModifiers::NONE);
        assert_eq!(
            chrome.resolve(SIDEBAR_TABLE, &key_input(key)),
            Some(ChromeAction::SidebarSelectUp)
        );
        assert!(chrome.unbind(SIDEBAR_TABLE, "k"));
        assert_eq!(chrome.resolve(SIDEBAR_TABLE, &key_input(key)), None);
        chrome
            .bind(SIDEBAR_TABLE, "x", "sidebar-select-up")
            .unwrap();
        let rebound = KeyEvent::new(TerminalKeyCode::Char('x'), KeyModifiers::NONE);
        assert_eq!(
            chrome.resolve(SIDEBAR_TABLE, &key_input(rebound)),
            Some(ChromeAction::SidebarSelectUp)
        );
    }

    #[test]
    fn rename_commands_use_stable_session_and_window_targets() {
        let session = rename_command(
            SidebarEditKind::RenameSession(zz_protocol::SessionId(7)),
            "work tree",
        )
        .unwrap();
        assert_eq!(session.name, "rename-session");
        assert_eq!(session.args, ["-t", "$7", "work tree"]);

        let window = rename_command(
            SidebarEditKind::RenameWindow(zz_protocol::WindowId(9)),
            "logs",
        )
        .unwrap();
        assert_eq!(window.name, "rename-window");
        assert_eq!(window.args, ["-t", "@9", "logs"]);
        assert!(
            rename_command(SidebarEditKind::RenameWindow(zz_protocol::WindowId(9)), "").is_none()
        );
        assert!(rename_command(SidebarEditKind::AddHost, "ignored").is_none());
    }

    #[test]
    fn add_host_input_matches_fleet_add_endpoint_and_validation_rules() {
        assert_eq!(
            parse_add_host_input("box user@box:2222").unwrap(),
            ("box".to_owned(), "ssh://user@box:2222".to_owned())
        );
        assert_eq!(
            parse_add_host_input("box ssh://box").unwrap(),
            ("box".to_owned(), "ssh://box".to_owned())
        );
        assert_eq!(
            parse_add_host_input("local box").unwrap_err(),
            "invalid `host-local`: host name `local` is reserved"
        );
        assert_eq!(
            parse_add_host_input("box -oProxyCommand=nope").unwrap_err(),
            "ssh destination must not start with `-`"
        );
        assert!(parse_add_host_input("box").is_err());
        assert!(parse_add_host_input("box user@@box").is_err());
    }

    #[test]
    fn browser_mouse_uses_content_relative_pixels_and_wheel_direction() {
        let input = browser_pointer_input(
            MouseEvent {
                kind: MouseEventKind::ScrollDown,
                column: 0,
                row: 0,
                modifiers: KeyModifiers::CONTROL | KeyModifiers::ALT,
            },
            Rect {
                x: 10,
                y: 4,
                width: 20,
                height: 10,
            },
            93,
            71,
            8,
            16,
        );

        assert_eq!(input.x, 13);
        assert_eq!(input.y, 7);
        assert_eq!(input.phase, ProviderPointerPhase::Wheel);
        assert_eq!((input.wheel_delta_x, input.wheel_delta_y), (0, -120));
        assert!(input.modifiers.control());
        assert!(input.modifiers.alt());
        assert!(!input.modifiers.shift());
        assert_eq!(input.button, None);
    }
}
