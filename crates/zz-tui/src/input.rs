use zz_client::{ChromeAction, ChromeKeymap, MenuKeyResult, SIDEBAR_TABLE, resolve_menu_key};
use zz_daemon::{
    Endpoint, InteractiveClient, configured_fleet_hosts, validate_fleet_host, write_fleet_host,
};
use zz_protocol::{
    ChooseBufferAction, ChooseTreeAction, CommandInvocation, CommandPromptAction,
    CommandPromptMode, ConfirmAction, ConfirmState, DisplayPanesAction, InputMessage,
    MAX_COMMAND_PROMPT_BYTES, MenuAction, MenuState, PaneKindSnapshot,
};
use zz_terminal::{
    KeyAction, KeyCode, KeyInput, Modifiers, PointerCellEvent, SearchQuery, TerminalMouseButton,
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
    AttachRequested,
    SwitchHost(HostSwitch),
    Detach,
}

const MAX_COMMAND_OUTPUT_SEARCH_BYTES: usize = 4096;

pub(crate) fn handle(
    model: &mut Model,
    client: &InteractiveClient,
    browser: &mut BrowserState,
    event: TerminalEvent,
    pixel_mouse: bool,
) -> Result<InputOutcome, String> {
    let event = match menu_input_route(
        model.menu.as_ref(),
        model.menu_selection,
        model.menu_action_pending,
        &mut model.menu_swallowed_key,
        event,
    ) {
        MenuInputRoute::Forward(event) => event,
        MenuInputRoute::Consume => return Ok(InputOutcome::None),
        MenuInputRoute::Select(selection) => {
            model.menu_selection = selection;
            return Ok(InputOutcome::Repaint);
        }
        MenuInputRoute::Action {
            action,
            swallowed_key,
        } => {
            client
                .send_input(InputMessage::Menu { action })
                .map_err(|error| error.to_string())?;
            model.menu_action_pending = true;
            model.menu_swallowed_key = swallowed_key;
            return Ok(InputOutcome::None);
        }
    };
    let event = match confirm_input_route(
        model.confirm.as_ref(),
        model.confirm_reply_pending,
        &mut model.confirm_swallowed_key,
        event,
    ) {
        ConfirmInputRoute::Forward(event) => event,
        ConfirmInputRoute::Consume => return Ok(InputOutcome::None),
        ConfirmInputRoute::Reply {
            accepted,
            swallowed_key,
        } => {
            client
                .send_input(InputMessage::Confirm {
                    action: ConfirmAction::Reply(accepted),
                })
                .map_err(|error| error.to_string())?;
            model.confirm_reply_pending = true;
            model.confirm_swallowed_key = swallowed_key;
            return Ok(InputOutcome::None);
        }
    };
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

#[derive(Debug, Eq, PartialEq)]
enum MenuInputRoute {
    Forward(TerminalEvent),
    Consume,
    Select(Option<usize>),
    Action {
        action: MenuAction,
        swallowed_key: Option<KeyCode>,
    },
}

fn menu_input_route(
    state: Option<&MenuState>,
    selection: Option<usize>,
    action_pending: bool,
    swallowed_key: &mut Option<KeyCode>,
    event: TerminalEvent,
) -> MenuInputRoute {
    if let TerminalEvent::Key(key) = &event
        && swallow_overlay_key(swallowed_key, *key)
    {
        return MenuInputRoute::Consume;
    }
    let Some(state) = state else {
        return MenuInputRoute::Forward(event);
    };
    if action_pending
        && matches!(
            event,
            TerminalEvent::Key(_) | TerminalEvent::Paste(_) | TerminalEvent::Mouse(_)
        )
    {
        return MenuInputRoute::Consume;
    }
    match event {
        TerminalEvent::Key(event) => match resolve_menu_key(state, selection, &key_input(event)) {
            MenuKeyResult::Action(action) => MenuInputRoute::Action {
                action,
                swallowed_key: Some(key_code(event.code)),
            },
            MenuKeyResult::Select(selection) => MenuInputRoute::Select(selection),
            MenuKeyResult::Consumed => MenuInputRoute::Consume,
        },
        TerminalEvent::Paste(_) | TerminalEvent::Mouse(_) => MenuInputRoute::Consume,
        event => MenuInputRoute::Forward(event),
    }
}

#[derive(Debug, Eq, PartialEq)]
enum ConfirmInputRoute {
    Forward(TerminalEvent),
    Consume,
    Reply {
        accepted: bool,
        swallowed_key: Option<KeyCode>,
    },
}

fn confirm_input_route(
    state: Option<&ConfirmState>,
    reply_pending: bool,
    swallowed_key: &mut Option<KeyCode>,
    event: TerminalEvent,
) -> ConfirmInputRoute {
    if let TerminalEvent::Key(key) = &event
        && swallow_overlay_key(swallowed_key, *key)
    {
        return ConfirmInputRoute::Consume;
    }
    let Some(state) = state else {
        return ConfirmInputRoute::Forward(event);
    };
    if reply_pending
        && matches!(
            event,
            TerminalEvent::Key(_) | TerminalEvent::Paste(_) | TerminalEvent::Mouse(_)
        )
    {
        return ConfirmInputRoute::Consume;
    }
    match event {
        TerminalEvent::Key(event) if event.kind == KeyEventKind::Release => {
            ConfirmInputRoute::Consume
        }
        TerminalEvent::Key(event) => match confirm_reply(state, event) {
            Some(accepted) => ConfirmInputRoute::Reply {
                accepted,
                swallowed_key: Some(key_code(event.code)),
            },
            None => ConfirmInputRoute::Consume,
        },
        TerminalEvent::Paste(_) | TerminalEvent::Mouse(_) => ConfirmInputRoute::Consume,
        event => ConfirmInputRoute::Forward(event),
    }
}

fn swallow_overlay_key(swallowed_key: &mut Option<KeyCode>, event: KeyEvent) -> bool {
    let Some(swallowed) = *swallowed_key else {
        return false;
    };
    if key_code(event.code) != swallowed {
        return false;
    }
    match event.kind {
        KeyEventKind::Repeat => true,
        KeyEventKind::Release => {
            *swallowed_key = None;
            true
        }
        KeyEventKind::Press => {
            *swallowed_key = None;
            false
        }
    }
}

fn confirm_reply(state: &ConfirmState, event: KeyEvent) -> Option<bool> {
    match event.code {
        TerminalKeyCode::Enter => Some(state.default_yes),
        TerminalKeyCode::Char(character) => Some(
            !event.modifiers.contains(KeyModifiers::CONTROL)
                && character.is_ascii()
                && character as u8 == state.confirm_key,
        ),
        TerminalKeyCode::Backspace | TerminalKeyCode::Tab | TerminalKeyCode::Esc => Some(false),
        _ => None,
    }
}

fn handle_key(
    model: &mut Model,
    client: &InteractiveClient,
    browser: &mut BrowserState,
    event: KeyEvent,
) -> Result<InputOutcome, String> {
    let global_route = global_key_route(
        &model.chrome,
        model.sidebar.focused && model.command_output.is_none(),
        event,
    );
    if global_route == GlobalKeyRoute::Detach {
        client.detach().map_err(|error| error.to_string())?;
        return Ok(InputOutcome::Detach);
    }
    if model.sidebar_edit.is_some() && model.command_output.is_none() {
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
    if let Some(pane) = model.command_output.as_ref().map(|(pane, _)| *pane) {
        match command_output_key_route(
            &mut model.command_output_search,
            &mut model.command_output_swallowed_key,
            pane,
            event,
        ) {
            CommandOutputKeyRoute::Search(edit) => {
                if let Some(action) = edit.action {
                    client
                        .send_input(InputMessage::CommandOutputView { action })
                        .map_err(|error| error.to_string())?;
                }
                return Ok(if edit.repaint {
                    if model.command_output_search.is_some() {
                        InputOutcome::Repaint
                    } else {
                        InputOutcome::RepaintAll
                    }
                } else {
                    InputOutcome::None
                });
            }
            CommandOutputKeyRoute::Swallowed => return Ok(InputOutcome::None),
            CommandOutputKeyRoute::Forward(input) => {
                if let Some(input) = input {
                    client
                        .send_input(input)
                        .map_err(|error| error.to_string())?;
                }
            }
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

fn command_output_key_input(pane: zz_protocol::PaneId, event: KeyEvent) -> Option<InputMessage> {
    (event.kind != KeyEventKind::Release).then(|| InputMessage::Key {
        pane,
        input: key_input(event),
        text_follows: false,
    })
}

enum CommandOutputKeyRoute {
    Search(CommandOutputSearchEdit),
    Swallowed,
    Forward(Option<InputMessage>),
}

fn command_output_key_route(
    search: &mut Option<SearchQuery>,
    swallowed_key: &mut Option<KeyCode>,
    pane: zz_protocol::PaneId,
    event: KeyEvent,
) -> CommandOutputKeyRoute {
    if search.is_some() {
        let edit = command_output_search_key(search, event);
        if let Some(key) = edit.swallowed_key {
            *swallowed_key = Some(key);
        }
        return CommandOutputKeyRoute::Search(edit);
    }
    if swallow_command_output_key(swallowed_key, event) {
        CommandOutputKeyRoute::Swallowed
    } else {
        CommandOutputKeyRoute::Forward(command_output_key_input(pane, event))
    }
}

fn swallow_command_output_key(swallowed_key: &mut Option<KeyCode>, event: KeyEvent) -> bool {
    let Some(swallowed) = *swallowed_key else {
        return false;
    };
    if key_input(event).key != swallowed {
        return false;
    }
    match event.kind {
        KeyEventKind::Repeat => true,
        KeyEventKind::Release => {
            *swallowed_key = None;
            true
        }
        KeyEventKind::Press => {
            *swallowed_key = None;
            false
        }
    }
}

#[derive(Debug, Default, Eq, PartialEq)]
struct CommandOutputSearchEdit {
    action: Option<TerminalViewAction>,
    repaint: bool,
    swallowed_key: Option<KeyCode>,
}

fn command_output_search_key(
    search: &mut Option<SearchQuery>,
    event: KeyEvent,
) -> CommandOutputSearchEdit {
    if event.kind == KeyEventKind::Release {
        return CommandOutputSearchEdit::default();
    }
    match event.code {
        TerminalKeyCode::Esc if event.kind == KeyEventKind::Press => {
            *search = None;
            CommandOutputSearchEdit {
                action: Some(TerminalViewAction::SearchClose),
                repaint: true,
                swallowed_key: Some(KeyCode::Escape),
            }
        }
        TerminalKeyCode::Enter if event.kind == KeyEventKind::Press => {
            *search = None;
            CommandOutputSearchEdit {
                action: None,
                repaint: true,
                swallowed_key: Some(KeyCode::Enter),
            }
        }
        TerminalKeyCode::Backspace => {
            let query = search.as_mut().expect("search is active");
            query.text.pop();
            CommandOutputSearchEdit {
                action: Some(TerminalViewAction::SearchUpdate(query.clone())),
                repaint: true,
                swallowed_key: None,
            }
        }
        TerminalKeyCode::Char(character)
            if !character.is_control()
                && !event.modifiers.intersects(
                    KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER,
                ) =>
        {
            let mut encoded = [0; 4];
            let Some(query) =
                append_command_output_search(search, character.encode_utf8(&mut encoded))
            else {
                return CommandOutputSearchEdit::default();
            };
            CommandOutputSearchEdit {
                action: Some(TerminalViewAction::SearchUpdate(query)),
                repaint: true,
                swallowed_key: None,
            }
        }
        _ => CommandOutputSearchEdit::default(),
    }
}

fn command_output_search_paste(
    search: &mut Option<SearchQuery>,
    text: &str,
) -> Option<TerminalViewAction> {
    append_command_output_search(search, text).map(TerminalViewAction::SearchUpdate)
}

fn append_command_output_search(
    search: &mut Option<SearchQuery>,
    text: &str,
) -> Option<SearchQuery> {
    let query = search.as_mut()?;
    let mut changed = false;
    for character in text.chars().filter(|character| !character.is_control()) {
        if query.text.len().saturating_add(character.len_utf8()) > MAX_COMMAND_OUTPUT_SEARCH_BYTES {
            break;
        }
        query.text.push(character);
        changed = true;
    }
    changed.then(|| query.clone())
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
            if let Some(target) = model.selected_sidebar_target() {
                return activate_sidebar_target(model, client, target);
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
) -> Result<InputOutcome, String> {
    match target {
        SidebarTarget::Session(session) => {
            client
                .attach(session.to_string())
                .map_err(|error| error.to_string())?;
            model.begin_client_focus_attach();
            Ok(InputOutcome::AttachRequested)
        }
        SidebarTarget::Window(window) => {
            execute_target(client, "select-window", window.to_string())?;
            Ok(InputOutcome::Repaint)
        }
        SidebarTarget::Pane(pane) => {
            focus_pane(client, pane)?;
            model.sidebar.focused = false;
            Ok(InputOutcome::Repaint)
        }
        SidebarTarget::NewPane(target) => {
            execute_target(client, "split-picker", target.to_string())?;
            model.sidebar.focused = false;
            Ok(InputOutcome::Repaint)
        }
        SidebarTarget::LocalHost | SidebarTarget::FleetHost(_) => Ok(model
            .host_switch(target)
            .map_or(InputOutcome::Repaint, InputOutcome::SwitchHost)),
        SidebarTarget::AddHost => {
            model.begin_add_host();
            Ok(InputOutcome::Repaint)
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
    if let Some(action) = command_output_search_paste(&mut model.command_output_search, &text) {
        client
            .send_input(InputMessage::CommandOutputView { action })
            .map_err(|error| error.to_string())?;
        return Ok(InputOutcome::Repaint);
    }
    if model.command_output.is_some() {
        return Ok(InputOutcome::None);
    }
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
    if model.command_output.is_some() {
        return Ok(InputOutcome::None);
    }
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
    match mouse_route_owner(
        model.command_output.is_some(),
        model.mouse_option,
        model.sidebar_edit.is_some() || model.sidebar_visible() && global_column <= sidebar::WIDTH,
    ) {
        MouseRouteOwner::CommandOutput => unreachable!("command output returned before routing"),
        MouseRouteOwner::Application => {
            if let Some((pane, action)) = app_mouse_forward_action(
                model,
                event,
                global_column,
                global_row,
                global_x,
                global_y,
            ) {
                client
                    .send_input(InputMessage::TerminalView { pane, action })
                    .map_err(|error| error.to_string())?;
            }
            return Ok(InputOutcome::None);
        }
        MouseRouteOwner::Sidebar => {
            if model.sidebar_edit.is_some() || global_column == sidebar::WIDTH {
                return Ok(InputOutcome::None);
            }
            match event.kind {
                MouseEventKind::ScrollUp => model.scroll_sidebar(-3),
                MouseEventKind::ScrollDown => model.scroll_sidebar(3),
                MouseEventKind::Down(MouseButton::Left)
                    if global_row < model.sidebar_tree_height() =>
                {
                    if let Some(target) = model.select_sidebar_row(global_row) {
                        return activate_sidebar_target(model, client, target);
                    }
                }
                _ => return Ok(InputOutcome::None),
            }
            return Ok(InputOutcome::Repaint);
        }
        MouseRouteOwner::Workspace => {}
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MouseRouteOwner {
    CommandOutput,
    Application,
    Sidebar,
    Workspace,
}

const fn mouse_route_owner(
    command_output: bool,
    mouse_option: bool,
    sidebar_hit: bool,
) -> MouseRouteOwner {
    if command_output {
        MouseRouteOwner::CommandOutput
    } else if !mouse_option {
        MouseRouteOwner::Application
    } else if sidebar_hit {
        MouseRouteOwner::Sidebar
    } else {
        MouseRouteOwner::Workspace
    }
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

fn send_focus(model: &mut Model, client: &InteractiveClient, focused: bool) -> Result<(), String> {
    if let Some(input) = model.client_focus_changed(focused) {
        client
            .send_input(input)
            .map_err(|error| error.to_string())?;
    }
    let active_pane = model.active_pane().and_then(|pane| {
        model
            .pane_snapshot(pane)
            .map(|snapshot| (pane, &snapshot.kind))
    });
    if let Some(input) = pane_focus_input(active_pane, focused) {
        client
            .send_input(input)
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn pane_focus_input(
    active_pane: Option<(zz_protocol::PaneId, &PaneKindSnapshot)>,
    focused: bool,
) -> Option<InputMessage> {
    if let Some((pane, PaneKindSnapshot::Terminal)) = active_pane {
        Some(InputMessage::TerminalView {
            pane,
            action: TerminalViewAction::Focus(focused),
        })
    } else {
        None
    }
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
    let event_modifiers = if matches!(event.code, TerminalKeyCode::BackTab) {
        event.modifiers | KeyModifiers::SHIFT
    } else {
        event.modifiers
    };
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
        modifiers: modifiers(event_modifiers),
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
    use zz_protocol::{MenuItem, PopupBorderLines};

    fn menu_state(selected: Option<u32>, stay_open: bool) -> MenuState {
        MenuState {
            left: 0,
            top: 0,
            width: 20,
            height: 6,
            client_columns: 80,
            client_rows: 24,
            cell_width_px: 8,
            cell_height_px: 16,
            title: "Menu".to_owned(),
            style: "default".to_owned(),
            selected_style: "reverse".to_owned(),
            border_style: "default".to_owned(),
            border_lines: PopupBorderLines::Single,
            items: vec![
                Some(MenuItem {
                    name: "Quit item".to_owned(),
                    key: Some("q".to_owned()),
                    enabled: true,
                }),
                None,
                Some(MenuItem {
                    name: "Disabled".to_owned(),
                    key: None,
                    enabled: false,
                }),
                Some(MenuItem {
                    name: "Last".to_owned(),
                    key: None,
                    enabled: true,
                }),
            ],
            selected,
            stay_open,
        }
    }

    fn menu_route(
        state: &MenuState,
        selection: Option<usize>,
        event: TerminalEvent,
    ) -> MenuInputRoute {
        menu_input_route(Some(state), selection, false, &mut None, event)
    }

    #[test]
    fn menu_shortcuts_win_and_navigation_changes_only_local_selection() {
        let state = menu_state(Some(0), false);
        assert_eq!(
            menu_route(
                &state,
                Some(0),
                TerminalEvent::Key(KeyEvent::new(
                    TerminalKeyCode::Char('q'),
                    KeyModifiers::NONE,
                )),
            ),
            MenuInputRoute::Action {
                action: MenuAction::Choose(0),
                swallowed_key: Some(KeyCode::Character('q')),
            }
        );
        assert_eq!(
            menu_route(
                &state,
                Some(0),
                TerminalEvent::Key(KeyEvent::new(TerminalKeyCode::Down, KeyModifiers::NONE,)),
            ),
            MenuInputRoute::Select(Some(3))
        );
        assert_eq!(state.selected, Some(0));
    }

    #[test]
    fn menu_cancel_enter_and_disabled_stay_open_use_the_shared_resolver() {
        let state = menu_state(Some(3), false);
        assert!(matches!(
            menu_route(
                &state,
                Some(3),
                TerminalEvent::Key(KeyEvent::new(TerminalKeyCode::Esc, KeyModifiers::NONE))
            ),
            MenuInputRoute::Action {
                action: MenuAction::Cancel,
                ..
            }
        ));
        assert!(matches!(
            menu_route(
                &state,
                Some(3),
                TerminalEvent::Key(KeyEvent::new(TerminalKeyCode::Enter, KeyModifiers::NONE))
            ),
            MenuInputRoute::Action {
                action: MenuAction::Choose(3),
                ..
            }
        ));
        assert!(matches!(
            menu_route(
                &state,
                Some(2),
                TerminalEvent::Key(KeyEvent::new(TerminalKeyCode::Enter, KeyModifiers::NONE))
            ),
            MenuInputRoute::Action {
                action: MenuAction::Cancel,
                ..
            }
        ));
        let stay_open = menu_state(Some(2), true);
        assert_eq!(
            menu_route(
                &stay_open,
                Some(2),
                TerminalEvent::Key(KeyEvent::new(TerminalKeyCode::Enter, KeyModifiers::NONE,)),
            ),
            MenuInputRoute::Consume
        );
    }

    #[test]
    fn menu_backtab_uses_real_parser_bytes() {
        let mut parser = crate::terminal_event::EventParser::default();
        let mut events = Vec::new();
        parser.push(b"\x1b[Z", &mut events);
        assert_eq!(events.len(), 1);
        let TerminalEvent::Key(event) = &events[0] else {
            panic!("backtab did not parse as a key");
        };
        let input = key_input(*event);
        assert_eq!(input.key, KeyCode::Tab);
        assert!(input.modifiers.shift());
        assert_eq!(
            menu_route(&menu_state(Some(3), false), Some(3), events.remove(0)),
            MenuInputRoute::Select(Some(0))
        );
    }

    #[test]
    fn menu_pending_action_owns_input_until_close() {
        let state = menu_state(Some(0), false);
        let press = KeyEvent::new(TerminalKeyCode::Char('q'), KeyModifiers::NONE);
        assert!(matches!(
            menu_route(&state, Some(0), TerminalEvent::Key(press)),
            MenuInputRoute::Action {
                action: MenuAction::Choose(0),
                ..
            }
        ));
        assert_eq!(
            menu_input_route(
                Some(&state),
                Some(0),
                true,
                &mut None,
                TerminalEvent::Key(KeyEvent {
                    kind: KeyEventKind::Repeat,
                    ..press
                }),
            ),
            MenuInputRoute::Consume
        );
        assert_eq!(
            menu_input_route(
                Some(&state),
                Some(0),
                true,
                &mut None,
                TerminalEvent::Paste("q".to_owned()),
            ),
            MenuInputRoute::Consume
        );
        assert_eq!(
            menu_input_route(
                Some(&state),
                Some(0),
                true,
                &mut None,
                TerminalEvent::Mouse(MouseEvent {
                    kind: MouseEventKind::Down(MouseButton::Left),
                    column: 2,
                    row: 1,
                    modifiers: KeyModifiers::NONE,
                }),
            ),
            MenuInputRoute::Consume
        );
        assert_eq!(
            menu_input_route(
                Some(&state),
                Some(0),
                true,
                &mut None,
                TerminalEvent::CellSize {
                    width_px: 8,
                    height_px: 16,
                },
            ),
            MenuInputRoute::Forward(TerminalEvent::CellSize {
                width_px: 8,
                height_px: 16,
            })
        );
    }

    #[test]
    fn menu_consumes_pointer_paste_releases_and_the_action_key_lifecycle() {
        let state = menu_state(Some(0), false);
        assert_eq!(
            menu_route(&state, Some(0), TerminalEvent::Paste("q".to_owned())),
            MenuInputRoute::Consume
        );
        assert_eq!(
            menu_route(
                &state,
                Some(0),
                TerminalEvent::Mouse(MouseEvent {
                    kind: MouseEventKind::Down(MouseButton::Left),
                    column: 2,
                    row: 1,
                    modifiers: KeyModifiers::NONE,
                }),
            ),
            MenuInputRoute::Consume
        );
        let press = KeyEvent::new(TerminalKeyCode::Char('q'), KeyModifiers::NONE);
        assert_eq!(
            menu_route(
                &state,
                Some(0),
                TerminalEvent::Key(KeyEvent {
                    kind: KeyEventKind::Release,
                    ..press
                }),
            ),
            MenuInputRoute::Consume
        );
        let MenuInputRoute::Action {
            swallowed_key: Some(key),
            ..
        } = menu_route(&state, Some(0), TerminalEvent::Key(press))
        else {
            panic!("menu shortcut was not consumed");
        };
        let mut swallowed = Some(key);
        assert_eq!(
            menu_input_route(
                None,
                Some(0),
                false,
                &mut swallowed,
                TerminalEvent::Key(KeyEvent {
                    kind: KeyEventKind::Repeat,
                    ..press
                }),
            ),
            MenuInputRoute::Consume
        );
        assert_eq!(
            menu_input_route(
                None,
                Some(0),
                false,
                &mut swallowed,
                TerminalEvent::Key(KeyEvent {
                    kind: KeyEventKind::Release,
                    ..press
                }),
            ),
            MenuInputRoute::Consume
        );
        assert_eq!(swallowed, None);
        assert_eq!(
            menu_input_route(
                None,
                Some(0),
                false,
                &mut swallowed,
                TerminalEvent::Key(press),
            ),
            MenuInputRoute::Forward(TerminalEvent::Key(press))
        );
    }

    fn confirm_state(confirm_key: u8, default_yes: bool) -> ConfirmState {
        ConfirmState {
            prompt: "Confirm? ".to_owned(),
            confirm_key,
            default_yes,
        }
    }

    fn confirm_route(state: &ConfirmState, event: TerminalEvent) -> ConfirmInputRoute {
        confirm_input_route(Some(state), false, &mut None, event)
    }

    fn assert_confirm_reply(state: &ConfirmState, event: KeyEvent, accepted: bool) {
        assert!(matches!(
            confirm_route(state, TerminalEvent::Key(event)),
            ConfirmInputRoute::Reply {
                accepted: actual,
                swallowed_key: Some(_),
            } if actual == accepted
        ));
    }

    #[test]
    fn confirm_keys_follow_the_published_case_and_enter_rules() {
        let lower = confirm_state(b'y', false);
        assert_confirm_reply(
            &lower,
            KeyEvent::new(TerminalKeyCode::Char('y'), KeyModifiers::NONE),
            true,
        );
        assert_confirm_reply(
            &lower,
            KeyEvent::new(TerminalKeyCode::Char('n'), KeyModifiers::NONE),
            false,
        );
        assert_confirm_reply(
            &lower,
            KeyEvent::new(TerminalKeyCode::Char('Y'), KeyModifiers::SHIFT),
            false,
        );
        assert_confirm_reply(
            &lower,
            KeyEvent::new(TerminalKeyCode::Char('y'), KeyModifiers::CONTROL),
            false,
        );
        assert_confirm_reply(
            &lower,
            KeyEvent::new(TerminalKeyCode::Char('y'), KeyModifiers::ALT),
            true,
        );
        assert_confirm_reply(
            &lower,
            KeyEvent::new(TerminalKeyCode::Char('y'), KeyModifiers::SUPER),
            true,
        );
        assert_confirm_reply(
            &lower,
            KeyEvent::new(TerminalKeyCode::Enter, KeyModifiers::NONE),
            false,
        );

        let upper = confirm_state(b'Y', false);
        assert_confirm_reply(
            &upper,
            KeyEvent::new(TerminalKeyCode::Char('Y'), KeyModifiers::SHIFT),
            true,
        );
        assert_confirm_reply(
            &upper,
            KeyEvent::new(TerminalKeyCode::Char('y'), KeyModifiers::NONE),
            false,
        );

        let enter = confirm_state(b'y', true);
        assert_confirm_reply(
            &enter,
            KeyEvent::new(TerminalKeyCode::Enter, KeyModifiers::NONE),
            true,
        );
        assert_confirm_reply(
            &enter,
            KeyEvent::new(TerminalKeyCode::Enter, KeyModifiers::SHIFT),
            true,
        );
        assert_confirm_reply(
            &enter,
            KeyEvent::new(TerminalKeyCode::Enter, KeyModifiers::ALT),
            true,
        );
        assert_confirm_reply(
            &enter,
            KeyEvent::new(TerminalKeyCode::Enter, KeyModifiers::CONTROL),
            true,
        );
        assert_confirm_reply(
            &enter,
            KeyEvent::new(TerminalKeyCode::Enter, KeyModifiers::SUPER),
            true,
        );
        assert_eq!(
            confirm_route(
                &lower,
                TerminalEvent::Key(KeyEvent::new(TerminalKeyCode::F(2), KeyModifiers::NONE))
            ),
            ConfirmInputRoute::Consume
        );
        assert_confirm_reply(
            &lower,
            KeyEvent::new(TerminalKeyCode::Esc, KeyModifiers::NONE),
            false,
        );
    }

    #[test]
    fn confirm_routes_extended_modifier_bit_eight_like_tmux_meta() {
        let mut parser = crate::terminal_event::EventParser::default();
        let mut events = Vec::new();
        parser.push(b"\x1b[121;9u\x1b[13;9u", &mut events);
        assert_eq!(events.len(), 2);

        let lower = confirm_state(b'y', false);
        assert!(matches!(
            confirm_route(&lower, events[0].clone()),
            ConfirmInputRoute::Reply { accepted: true, .. }
        ));

        let enter = confirm_state(b'y', true);
        assert!(matches!(
            confirm_route(&enter, events[1].clone()),
            ConfirmInputRoute::Reply { accepted: true, .. }
        ));
    }

    #[test]
    fn confirm_consumes_paste_pointer_and_the_reply_key_lifecycle() {
        let lower = confirm_state(b'y', false);
        assert_eq!(
            confirm_route(&lower, TerminalEvent::Paste("y trailing".to_owned())),
            ConfirmInputRoute::Consume
        );
        assert_eq!(
            confirm_route(
                &lower,
                TerminalEvent::Mouse(MouseEvent {
                    kind: MouseEventKind::Down(MouseButton::Left),
                    column: 4,
                    row: 3,
                    modifiers: KeyModifiers::NONE,
                })
            ),
            ConfirmInputRoute::Consume
        );
        let press = KeyEvent::new(TerminalKeyCode::Char('y'), KeyModifiers::NONE);
        let ConfirmInputRoute::Reply {
            swallowed_key: Some(key),
            ..
        } = confirm_route(&lower, TerminalEvent::Key(press))
        else {
            panic!("confirm key was not consumed");
        };
        let mut swallowed = Some(key);
        assert_eq!(
            confirm_input_route(
                None,
                false,
                &mut swallowed,
                TerminalEvent::Key(KeyEvent {
                    kind: KeyEventKind::Repeat,
                    ..press
                })
            ),
            ConfirmInputRoute::Consume
        );
        assert_eq!(
            confirm_input_route(
                None,
                false,
                &mut swallowed,
                TerminalEvent::Key(KeyEvent {
                    kind: KeyEventKind::Release,
                    ..press
                })
            ),
            ConfirmInputRoute::Consume
        );
        assert_eq!(swallowed, None);
        assert_eq!(
            confirm_input_route(None, false, &mut swallowed, TerminalEvent::Key(press)),
            ConfirmInputRoute::Forward(TerminalEvent::Key(press))
        );
        assert_eq!(
            confirm_input_route(
                Some(&lower),
                true,
                &mut None,
                TerminalEvent::Key(KeyEvent::new(
                    TerminalKeyCode::Char('n'),
                    KeyModifiers::NONE,
                ))
            ),
            ConfirmInputRoute::Consume
        );
        assert_eq!(
            confirm_route(
                &lower,
                TerminalEvent::CellSize {
                    width_px: 8,
                    height_px: 16,
                }
            ),
            ConfirmInputRoute::Forward(TerminalEvent::CellSize {
                width_px: 8,
                height_px: 16,
            })
        );
    }

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
    fn command_output_routes_press_and_repeat_keys_but_not_releases() {
        let pane = zz_protocol::PaneId(9);
        for (event, expected_action, expected_key) in [
            (
                KeyEvent::new(TerminalKeyCode::Char('q'), KeyModifiers::NONE),
                KeyAction::Press,
                KeyCode::Character('q'),
            ),
            (
                KeyEvent {
                    code: TerminalKeyCode::Esc,
                    modifiers: KeyModifiers::NONE,
                    kind: KeyEventKind::Repeat,
                },
                KeyAction::Repeat,
                KeyCode::Escape,
            ),
        ] {
            let Some(InputMessage::Key {
                pane: target,
                input,
                text_follows,
            }) = command_output_key_input(pane, event)
            else {
                panic!("command output key was not routed through the daemon");
            };
            assert_eq!(target, pane);
            assert_eq!(input.action, expected_action);
            assert_eq!(input.key, expected_key);
            assert!(!text_follows);
        }
        assert!(
            command_output_key_input(
                pane,
                KeyEvent {
                    code: TerminalKeyCode::Char('q'),
                    modifiers: KeyModifiers::NONE,
                    kind: KeyEventKind::Release,
                },
            )
            .is_none()
        );
    }

    fn assert_search_exit_swallow(
        terminal_code: TerminalKeyCode,
        key: KeyCode,
        closes_search: bool,
    ) {
        let pane = zz_protocol::PaneId(9);
        let mut search = Some(SearchQuery::default());
        let mut swallowed = None;

        let press = command_output_key_route(
            &mut search,
            &mut swallowed,
            pane,
            KeyEvent::new(terminal_code, KeyModifiers::NONE),
        );
        let CommandOutputKeyRoute::Search(edit) = press else {
            panic!("search exit press escaped the local prompt");
        };
        assert_eq!(
            edit.action,
            closes_search.then_some(TerminalViewAction::SearchClose)
        );
        assert!(edit.repaint);
        assert!(search.is_none());
        assert_eq!(swallowed, Some(key));

        let repeat = command_output_key_route(
            &mut search,
            &mut swallowed,
            pane,
            KeyEvent {
                code: terminal_code,
                modifiers: KeyModifiers::NONE,
                kind: KeyEventKind::Repeat,
            },
        );
        assert!(matches!(repeat, CommandOutputKeyRoute::Swallowed));
        assert_eq!(swallowed, Some(key));

        let release = command_output_key_route(
            &mut search,
            &mut swallowed,
            pane,
            KeyEvent {
                code: terminal_code,
                modifiers: KeyModifiers::NONE,
                kind: KeyEventKind::Release,
            },
        );
        assert!(matches!(release, CommandOutputKeyRoute::Swallowed));
        assert_eq!(swallowed, None);

        let fresh = command_output_key_route(
            &mut search,
            &mut swallowed,
            pane,
            KeyEvent::new(terminal_code, KeyModifiers::NONE),
        );
        assert!(matches!(
            fresh,
            CommandOutputKeyRoute::Forward(Some(InputMessage::Key {
                pane: target,
                input,
                text_follows: false,
            })) if target == pane && input.key == key && input.action == KeyAction::Press
        ));
    }

    #[test]
    fn command_output_search_enter_swallows_its_repeat_and_release() {
        assert_search_exit_swallow(TerminalKeyCode::Enter, KeyCode::Enter, false);
    }

    #[test]
    fn command_output_search_escape_swallows_its_repeat_and_release() {
        assert_search_exit_swallow(TerminalKeyCode::Esc, KeyCode::Escape, true);
    }

    #[test]
    fn command_output_search_exit_does_not_swallow_unrelated_keys() {
        let pane = zz_protocol::PaneId(9);
        let mut search = Some(SearchQuery::default());
        let mut swallowed = None;
        let _ = command_output_key_route(
            &mut search,
            &mut swallowed,
            pane,
            KeyEvent::new(TerminalKeyCode::Enter, KeyModifiers::NONE),
        );

        let unrelated = command_output_key_route(
            &mut search,
            &mut swallowed,
            pane,
            KeyEvent::new(TerminalKeyCode::Char('q'), KeyModifiers::NONE),
        );
        assert!(matches!(
            unrelated,
            CommandOutputKeyRoute::Forward(Some(InputMessage::Key { input, .. }))
                if input.key == KeyCode::Character('q')
        ));
        assert_eq!(swallowed, Some(KeyCode::Enter));

        let fresh_same_key = command_output_key_route(
            &mut search,
            &mut swallowed,
            pane,
            KeyEvent::new(TerminalKeyCode::Enter, KeyModifiers::NONE),
        );
        assert!(matches!(
            fresh_same_key,
            CommandOutputKeyRoute::Forward(Some(InputMessage::Key { input, .. }))
                if input.key == KeyCode::Enter && input.action == KeyAction::Press
        ));
        assert_eq!(swallowed, None);
    }

    #[test]
    fn command_output_search_edits_accepts_and_closes_locally() {
        let mut search = Some(SearchQuery::default());
        let appended = command_output_search_key(
            &mut search,
            KeyEvent::new(TerminalKeyCode::Char('é'), KeyModifiers::NONE),
        );
        assert_eq!(search.as_ref().unwrap().text, "é");
        assert!(matches!(
            appended.action,
            Some(TerminalViewAction::SearchUpdate(ref query)) if query.text == "é"
        ));

        let erased = command_output_search_key(
            &mut search,
            KeyEvent::new(TerminalKeyCode::Backspace, KeyModifiers::NONE),
        );
        assert_eq!(search.as_ref().unwrap().text, "");
        assert!(matches!(
            erased.action,
            Some(TerminalViewAction::SearchUpdate(ref query)) if query.text.is_empty()
        ));

        let accepted = command_output_search_key(
            &mut search,
            KeyEvent::new(TerminalKeyCode::Enter, KeyModifiers::NONE),
        );
        assert_eq!(accepted.action, None);
        assert!(accepted.repaint);
        assert!(search.is_none());

        search = Some(SearchQuery::default());
        let closed = command_output_search_key(
            &mut search,
            KeyEvent::new(TerminalKeyCode::Esc, KeyModifiers::NONE),
        );
        assert_eq!(closed.action, Some(TerminalViewAction::SearchClose));
        assert!(search.is_none());
    }

    #[test]
    fn command_output_search_releases_and_modified_text_are_inert() {
        let mut search = Some(SearchQuery::default());
        let released = command_output_search_key(
            &mut search,
            KeyEvent {
                code: TerminalKeyCode::Char('x'),
                modifiers: KeyModifiers::NONE,
                kind: KeyEventKind::Release,
            },
        );
        assert_eq!(released, CommandOutputSearchEdit::default());
        assert_eq!(search.as_ref().unwrap().text, "");

        let modified = command_output_search_key(
            &mut search,
            KeyEvent::new(TerminalKeyCode::Char('x'), KeyModifiers::CONTROL),
        );
        assert_eq!(modified, CommandOutputSearchEdit::default());
        assert_eq!(search.as_ref().unwrap().text, "");
    }

    #[test]
    fn command_output_search_paste_updates_the_query() {
        let mut search = Some(SearchQuery::literal("start"));
        let action = command_output_search_paste(&mut search, " + pasted");
        assert_eq!(search.as_ref().unwrap().text, "start + pasted");
        assert!(matches!(
            action,
            Some(TerminalViewAction::SearchUpdate(ref query))
                if query.text == "start + pasted"
        ));

        let mut inactive = None;
        assert_eq!(command_output_search_paste(&mut inactive, "ignored"), None);
    }

    #[test]
    fn command_output_search_drops_control_characters_from_paste() {
        let mut search = Some(SearchQuery::default());
        let action = command_output_search_paste(&mut search, "a\n\t\u{1b}\u{7f}界");
        assert_eq!(search.as_ref().unwrap().text, "a界");
        assert!(matches!(
            action,
            Some(TerminalViewAction::SearchUpdate(ref query)) if query.text == "a界"
        ));

        assert_eq!(command_output_search_paste(&mut search, "\n\t\u{1b}"), None);
        assert_eq!(search.as_ref().unwrap().text, "a界");
    }

    #[test]
    fn command_output_search_respects_the_utf8_byte_limit() {
        let mut paste = Some(SearchQuery::literal("a".repeat(4094)));
        let action = command_output_search_paste(&mut paste, "éx");
        assert_eq!(
            paste.as_ref().unwrap().text.len(),
            MAX_COMMAND_OUTPUT_SEARCH_BYTES
        );
        assert!(paste.as_ref().unwrap().text.ends_with('é'));
        assert!(matches!(
            action,
            Some(TerminalViewAction::SearchUpdate(ref query))
                if query.text.len() == MAX_COMMAND_OUTPUT_SEARCH_BYTES
        ));

        let mut typed = Some(SearchQuery::literal("a".repeat(4095)));
        let rejected = command_output_search_key(
            &mut typed,
            KeyEvent::new(TerminalKeyCode::Char('é'), KeyModifiers::NONE),
        );
        assert_eq!(rejected, CommandOutputSearchEdit::default());
        assert_eq!(typed.as_ref().unwrap().text.len(), 4095);

        let accepted = command_output_search_key(
            &mut typed,
            KeyEvent::new(TerminalKeyCode::Char('x'), KeyModifiers::NONE),
        );
        assert!(accepted.repaint);
        assert_eq!(
            typed.as_ref().unwrap().text.len(),
            MAX_COMMAND_OUTPUT_SEARCH_BYTES
        );

        let full = command_output_search_key(
            &mut typed,
            KeyEvent::new(TerminalKeyCode::Char('y'), KeyModifiers::NONE),
        );
        assert_eq!(full, CommandOutputSearchEdit::default());
        assert_eq!(
            typed.as_ref().unwrap().text.len(),
            MAX_COMMAND_OUTPUT_SEARCH_BYTES
        );
    }

    #[test]
    fn command_output_mouse_owns_every_outer_route() {
        for mouse_option in [false, true] {
            for sidebar_hit in [false, true] {
                assert_eq!(
                    mouse_route_owner(true, mouse_option, sidebar_hit),
                    MouseRouteOwner::CommandOutput
                );
            }
        }
        assert_eq!(
            mouse_route_owner(false, false, true),
            MouseRouteOwner::Application
        );
        assert_eq!(
            mouse_route_owner(false, true, true),
            MouseRouteOwner::Sidebar
        );
        assert_eq!(
            mouse_route_owner(false, true, false),
            MouseRouteOwner::Workspace
        );
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
    fn focus_events_forward_pane_focus_only_for_terminals() {
        assert_eq!(pane_focus_input(None, true), None);
        assert_eq!(
            pane_focus_input(
                Some((zz_protocol::PaneId(7), &PaneKindSnapshot::Terminal)),
                false,
            ),
            Some(InputMessage::TerminalView {
                pane: zz_protocol::PaneId(7),
                action: TerminalViewAction::Focus(false),
            })
        );
        assert_eq!(
            pane_focus_input(
                Some((zz_protocol::PaneId(8), &PaneKindSnapshot::Picker)),
                true,
            ),
            None
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
