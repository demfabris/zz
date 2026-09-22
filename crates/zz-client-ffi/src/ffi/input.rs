use std::{
    collections::VecDeque,
    ffi::{CStr, c_char},
};

use zz_client::{
    ChromeAction, Disposition, Effect, InputEvent, InputOwner, InputRouter, PrefixView, SurfaceKind,
};
use zz_protocol::PaneId;
use zz_terminal::{KeyAction, KeyCode, KeyInput, Modifiers};

use super::{ZzBytes, ZzChromeKeymap, key_action, key_code};

pub struct ZzInputRouter {
    router: InputRouter,
    effects: VecDeque<Effect>,
    current: Option<Effect>,
}

#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ZzSurfaceKind {
    Terminal = 0,
    Browser = 1,
    Other = 2,
}

impl From<ZzSurfaceKind> for SurfaceKind {
    fn from(value: ZzSurfaceKind) -> Self {
        match value {
            ZzSurfaceKind::Terminal => Self::Terminal,
            ZzSurfaceKind::Browser => Self::Browser,
            ZzSurfaceKind::Other => Self::Other,
        }
    }
}

impl From<SurfaceKind> for ZzSurfaceKind {
    fn from(value: SurfaceKind) -> Self {
        match value {
            SurfaceKind::Terminal => Self::Terminal,
            SurfaceKind::Browser => Self::Browser,
            SurfaceKind::Other => Self::Other,
        }
    }
}

#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ZzOwnerKind {
    None = 0,
    Pane = 1,
    Sidebar = 2,
    Overlay = 3,
    NativeEditor = 4,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ZzInputOwner {
    pub kind: ZzOwnerKind,
    pub pane: u64,
    pub surface: ZzSurfaceKind,
}

impl From<InputOwner> for ZzInputOwner {
    fn from(value: InputOwner) -> Self {
        let (kind, pane, surface) = match value {
            InputOwner::None => (ZzOwnerKind::None, 0, ZzSurfaceKind::Terminal),
            InputOwner::Pane(pane, surface) => (ZzOwnerKind::Pane, pane.0, surface.into()),
            InputOwner::Sidebar => (ZzOwnerKind::Sidebar, 0, ZzSurfaceKind::Terminal),
            InputOwner::Overlay => (ZzOwnerKind::Overlay, 0, ZzSurfaceKind::Terminal),
            InputOwner::NativeEditor => (ZzOwnerKind::NativeEditor, 0, ZzSurfaceKind::Terminal),
        };
        Self {
            kind,
            pane,
            surface,
        }
    }
}

impl From<ZzInputOwner> for InputOwner {
    fn from(value: ZzInputOwner) -> Self {
        match value.kind {
            ZzOwnerKind::None => Self::None,
            ZzOwnerKind::Pane => Self::Pane(PaneId(value.pane), value.surface.into()),
            ZzOwnerKind::Sidebar => Self::Sidebar,
            ZzOwnerKind::Overlay => Self::Overlay,
            ZzOwnerKind::NativeEditor => Self::NativeEditor,
        }
    }
}

#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ZzInputEventKind {
    ActivatePane = 0,
    SetActivePane = 1,
    FocusSidebar = 2,
    FocusNativeEditor = 3,
    OverlayOpened = 4,
    OverlayClosed = 5,
    NativeFocusObserved = 6,
    PaneRemoved = 7,
    Detached = 8,
    WindowDeactivated = 9,
}

#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ZzInputEffectKind {
    ForwardKey = 0,
    Chrome = 1,
    RequestFocus = 2,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct ZzInputKey {
    pub code: u32,
    pub codepoint: u32,
    pub unshifted_codepoint: u32,
    pub function: u8,
    pub action: u32,
    pub modifiers: u8,
    pub text: ZzBytes,
}

impl ZzInputKey {
    const EMPTY: Self = Self {
        code: 0,
        codepoint: 0,
        unshifted_codepoint: 0,
        function: 0,
        action: 0,
        modifiers: 0,
        text: ZzBytes::EMPTY,
    };

    fn new(input: &KeyInput) -> Self {
        let (code, codepoint, function) = match input.key {
            KeyCode::Character(value) => (0, u32::from(value), 0),
            KeyCode::Backspace => (1, 0, 0),
            KeyCode::Enter => (2, 0, 0),
            KeyCode::Tab => (3, 0, 0),
            KeyCode::Escape => (4, 0, 0),
            KeyCode::Delete => (5, 0, 0),
            KeyCode::Insert => (6, 0, 0),
            KeyCode::Home => (7, 0, 0),
            KeyCode::End => (8, 0, 0),
            KeyCode::PageUp => (9, 0, 0),
            KeyCode::PageDown => (10, 0, 0),
            KeyCode::ArrowUp => (11, 0, 0),
            KeyCode::ArrowDown => (12, 0, 0),
            KeyCode::ArrowLeft => (13, 0, 0),
            KeyCode::ArrowRight => (14, 0, 0),
            KeyCode::Function(value) => (15, 0, value),
            KeyCode::Unidentified => (16, 0, 0),
        };
        Self {
            code,
            codepoint,
            unshifted_codepoint: input.unshifted_codepoint.map_or(0, u32::from),
            function,
            action: match input.action {
                KeyAction::Press => 0,
                KeyAction::Repeat => 1,
                KeyAction::Release => 2,
            },
            modifiers: input.modifiers.bits(),
            text: input.text.as_deref().map_or(ZzBytes::EMPTY, ZzBytes::new),
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct ZzInputEffect {
    pub kind: ZzInputEffectKind,
    pub pane: u64,
    pub key: ZzInputKey,
    pub action: ZzBytes,
    pub owner: ZzInputOwner,
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_input_router_new(keymap: *mut ZzChromeKeymap) -> *mut ZzInputRouter {
    if keymap.is_null() {
        return std::ptr::null_mut();
    }
    let keymap = unsafe { Box::from_raw(keymap) };
    Box::into_raw(Box::new(ZzInputRouter {
        router: InputRouter::new(keymap.0),
        effects: VecDeque::new(),
        current: None,
    }))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_input_router_free(router: *mut ZzInputRouter) {
    if !router.is_null() {
        drop(unsafe { Box::from_raw(router) });
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_input_router_unbind_action(
    router: *mut ZzInputRouter,
    action: *const c_char,
) -> usize {
    let Some(router) = (unsafe { router.as_mut() }) else {
        return 0;
    };
    if action.is_null() {
        return 0;
    }
    let Some(action) = unsafe { CStr::from_ptr(action) }
        .to_str()
        .ok()
        .and_then(ChromeAction::from_name)
    else {
        return 0;
    };
    router.router.unbind_action(action)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_input_router_owner(router: *const ZzInputRouter) -> ZzInputOwner {
    unsafe { router.as_ref() }
        .map_or(InputOwner::None, |router| router.router.owner())
        .into()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_input_router_event(
    router: *mut ZzInputRouter,
    kind: ZzInputEventKind,
    owner: ZzInputOwner,
) {
    let Some(router) = (unsafe { router.as_mut() }) else {
        return;
    };
    let event = match kind {
        ZzInputEventKind::ActivatePane => {
            InputEvent::ActivatePane(PaneId(owner.pane), owner.surface.into())
        }
        ZzInputEventKind::SetActivePane => {
            InputEvent::SetActivePane(PaneId(owner.pane), owner.surface.into())
        }
        ZzInputEventKind::FocusSidebar => InputEvent::FocusSidebar,
        ZzInputEventKind::FocusNativeEditor => InputEvent::FocusNativeEditor,
        ZzInputEventKind::OverlayOpened => InputEvent::OverlayOpened,
        ZzInputEventKind::OverlayClosed => InputEvent::OverlayClosed,
        ZzInputEventKind::NativeFocusObserved => InputEvent::NativeFocusObserved(owner.into()),
        ZzInputEventKind::PaneRemoved => InputEvent::PaneRemoved(PaneId(owner.pane)),
        ZzInputEventKind::Detached => InputEvent::Detached,
        ZzInputEventKind::WindowDeactivated => InputEvent::WindowDeactivated,
    };
    router.router.event(event);
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_input_router_key(
    router: *mut ZzInputRouter,
    code: u32,
    codepoint: u32,
    unshifted_codepoint: u32,
    function: u8,
    action: u32,
    modifiers: u8,
    text: *const c_char,
    prefix_armed: bool,
    prefix_claimed: bool,
) -> u32 {
    let (Some(router), Some(key), Some(action), Some(modifiers)) = (
        unsafe { router.as_mut() },
        key_code(code, codepoint, function),
        key_action(action),
        Modifiers::from_bits(modifiers),
    ) else {
        return 0;
    };
    let text = if text.is_null() {
        None
    } else {
        let Ok(text) = unsafe { CStr::from_ptr(text) }.to_str() else {
            return 0;
        };
        Some(text.into())
    };
    let input = KeyInput {
        key,
        action,
        modifiers,
        text,
        unshifted_codepoint: char::from_u32(unshifted_codepoint)
            .filter(|_| unshifted_codepoint != 0),
    };
    u32::from(
        router.router.key(
            &input,
            PrefixView {
                armed: prefix_armed,
                claimed: prefix_claimed,
            },
        ) == Disposition::Consumed,
    )
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn zz_input_router_next_effect(
    router: *mut ZzInputRouter,
    out: *mut ZzInputEffect,
) -> bool {
    let (Some(router), Some(out)) = (unsafe { (router.as_mut(), out.as_mut()) }) else {
        return false;
    };
    router.effects.extend(router.router.drain_effects());
    router.current = router.effects.pop_front();
    let Some(effect) = router.current.as_ref() else {
        return false;
    };
    let mut result = ZzInputEffect {
        kind: ZzInputEffectKind::ForwardKey,
        pane: 0,
        key: ZzInputKey::EMPTY,
        action: ZzBytes::EMPTY,
        owner: InputOwner::None.into(),
    };
    match effect {
        Effect::ForwardKey { pane, input } => {
            result.pane = pane.0;
            result.key = ZzInputKey::new(input);
        }
        Effect::Chrome(action) => {
            result.kind = ZzInputEffectKind::Chrome;
            result.action = ZzBytes::new(action.name());
        }
        Effect::RequestFocus(owner) => {
            result.kind = ZzInputEffectKind::RequestFocus;
            result.owner = (*owner).into();
        }
    }
    *out = result;
    true
}
