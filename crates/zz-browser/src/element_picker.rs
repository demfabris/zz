use std::{fmt::Write as _, sync::Arc};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub(crate) const MAX_ELEMENT_CONTEXT_BYTES: usize = 32 * 1024;
// The slack covers the framing plus the eight-field geometry object.
const MAX_PICKER_MESSAGE_BYTES: usize = MAX_ELEMENT_CONTEXT_BYTES + 1024;
const MAX_PICK_TOKEN_BYTES: usize = 64;
const PICKER_PROTOCOL_VERSION: u8 = 1;

/// Semantic colors and metrics for the in-page element picker UI.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ElementPickerAppearance {
    /// Selection outline color.
    pub highlight_outline: String,
    /// Selection background color.
    pub highlight_fill: String,
    /// Thin contrast ring color around the selection.
    pub highlight_contrast: String,
    /// DOM preview background color.
    pub preview_background: String,
    /// DOM preview text color.
    pub preview_foreground: String,
    /// DOM preview border color.
    pub preview_border: String,
    /// DOM preview shadow color, or `None` to disable its shadow.
    pub shadow: Option<String>,
    /// Requested corner radius in unzoomed UI pixels.
    pub radius: f32,
    /// Native font family name used by the DOM preview.
    pub font_family: String,
    /// Browser page zoom used to keep picker chrome screen-sized.
    pub page_zoom: f64,
}

pub(crate) fn element_picker_start_script(
    token: &str,
    appearance: &ElementPickerAppearance,
) -> Result<String, serde_json::Error> {
    let token = serde_json::to_string(token)?;
    let appearance = serde_json::to_string(appearance)?;
    Ok(format!(
        "globalThis.__zzElementPicker?.start({token},{appearance});"
    ))
}

/// Where the element sat, in CSS pixels: a viewport-relative rect plus the
/// scroll offsets that turn it into page coordinates.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PickGeometry {
    pub(crate) x: f64,
    pub(crate) y: f64,
    pub(crate) width: f64,
    pub(crate) height: f64,
    pub(crate) scroll_x: f64,
    pub(crate) scroll_y: f64,
    pub(crate) viewport_width: f64,
    pub(crate) viewport_height: f64,
}

impl PickGeometry {
    fn is_usable(self) -> bool {
        [
            self.x,
            self.y,
            self.width,
            self.height,
            self.scroll_x,
            self.scroll_y,
            self.viewport_width,
            self.viewport_height,
        ]
        .into_iter()
        .all(f64::is_finite)
            && self.width > 0.0
            && self.height > 0.0
            && self.viewport_width > 0.0
            && self.viewport_height > 0.0
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum ElementPickOutcome {
    Picked(Arc<str>, Option<PickGeometry>),
    Cancelled,
    Failed,
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub(crate) enum ElementPickMessageError {
    #[error("element picker message exceeds its size limit")]
    OversizedMessage,
    #[error("element picker message is malformed")]
    MalformedMessage,
    #[error("element picker protocol version is unsupported")]
    UnsupportedVersion,
    #[error("element picker message has an invalid token")]
    InvalidToken,
    #[error("no element pick is active")]
    NoActivePick,
    #[error("element picker token is stale")]
    StaleToken,
    #[error("element picker context is invalid")]
    InvalidContext,
}

#[derive(Clone, Default)]
pub(crate) struct ElementPickState {
    inner: Arc<Mutex<ElementPickStateInner>>,
}

#[derive(Default)]
struct ElementPickStateInner {
    active_token: Option<Arc<str>>,
}

#[derive(Deserialize)]
struct WireMessage {
    version: u8,
    kind: String,
    token: String,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    geometry: Option<serde_json::Value>,
}

impl ElementPickState {
    pub(crate) fn begin(&self) -> Option<Arc<str>> {
        let mut random = [0_u8; 16];
        getrandom::fill(&mut random).ok()?;
        let mut token = String::with_capacity(random.len() * 2);
        for byte in random {
            write!(&mut token, "{byte:02x}").expect("writing to a String cannot fail");
        }
        let token: Arc<str> = Arc::from(token);
        self.inner.lock().active_token = Some(token.clone());
        Some(token)
    }

    pub(crate) fn cancel(&self) -> bool {
        self.inner.lock().active_token.take().is_some()
    }

    pub(crate) fn consume(
        &self,
        request: &str,
    ) -> Result<ElementPickOutcome, ElementPickMessageError> {
        if request.len() > MAX_PICKER_MESSAGE_BYTES {
            return Err(ElementPickMessageError::OversizedMessage);
        }
        let message: WireMessage =
            serde_json::from_str(request).map_err(|_| ElementPickMessageError::MalformedMessage)?;
        if message.version != PICKER_PROTOCOL_VERSION {
            return Err(ElementPickMessageError::UnsupportedVersion);
        }
        if message.token.is_empty() || message.token.len() > MAX_PICK_TOKEN_BYTES {
            return Err(ElementPickMessageError::InvalidToken);
        }

        let outcome = match message.kind.as_str() {
            "picked" => {
                let text = message
                    .text
                    .ok_or(ElementPickMessageError::InvalidContext)?;
                if text.is_empty()
                    || text.len() > MAX_ELEMENT_CONTEXT_BYTES
                    || text.chars().any(char::is_control)
                    || !text.starts_with('[')
                    || !text.ends_with(']')
                {
                    return Err(ElementPickMessageError::InvalidContext);
                }
                let geometry = message
                    .geometry
                    .and_then(|value| serde_json::from_value::<PickGeometry>(value).ok())
                    .filter(|geometry| geometry.is_usable());
                ElementPickOutcome::Picked(Arc::from(text), geometry)
            }
            "cancelled" => ElementPickOutcome::Cancelled,
            "failed" => ElementPickOutcome::Failed,
            _ => return Err(ElementPickMessageError::MalformedMessage),
        };

        let mut state = self.inner.lock();
        let Some(active_token) = state.active_token.as_deref() else {
            return Err(ElementPickMessageError::NoActivePick);
        };
        if active_token != message.token {
            return Err(ElementPickMessageError::StaleToken);
        }
        state.active_token = None;
        Ok(outcome)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_script_json_serializes_token_and_appearance() {
        let appearance = ElementPickerAppearance {
            highlight_outline: "#88c0d0".to_owned(),
            highlight_fill: "rgb(136 192 208 / 12%)".to_owned(),
            highlight_contrast: "rgb(0 0 0 / 30%)".to_owned(),
            preview_background: "#2e3440".to_owned(),
            preview_foreground: "#eceff4".to_owned(),
            preview_border: "#4c566a".to_owned(),
            shadow: Some("#01020366".to_owned()),
            radius: 7.5,
            font_family: "0xProto \"Nerd Font\"".to_owned(),
            page_zoom: 1.25,
        };

        let script = element_picker_start_script("token-\"quoted\"", &appearance)
            .expect("appearance serializes");
        let arguments = script
            .strip_prefix("globalThis.__zzElementPicker?.start(")
            .and_then(|script| script.strip_suffix(");"))
            .expect("start call wraps JSON arguments");
        let arguments: serde_json::Value =
            serde_json::from_str(&format!("[{arguments}]")).expect("arguments remain valid JSON");

        assert_eq!(
            arguments,
            serde_json::json!([
                "token-\"quoted\"",
                {
                    "highlightOutline": "#88c0d0",
                    "highlightFill": "rgb(136 192 208 / 12%)",
                    "highlightContrast": "rgb(0 0 0 / 30%)",
                    "previewBackground": "#2e3440",
                    "previewForeground": "#eceff4",
                    "previewBorder": "#4c566a",
                    "shadow": "#01020366",
                    "radius": 7.5,
                    "fontFamily": "0xProto \"Nerd Font\"",
                    "pageZoom": 1.25,
                }
            ])
        );
    }

    fn picked(token: &str, text: &str) -> String {
        serde_json::json!({
            "version": 1,
            "kind": "picked",
            "token": token,
            "text": text,
        })
        .to_string()
    }

    fn picked_with_geometry(token: &str, text: &str, geometry: &serde_json::Value) -> String {
        serde_json::json!({
            "version": 1,
            "kind": "picked",
            "token": token,
            "text": text,
            "geometry": geometry,
        })
        .to_string()
    }

    fn sample_geometry() -> serde_json::Value {
        serde_json::json!({
            "x": 100.0,
            "y": 200.0,
            "width": 300.0,
            "height": 150.0,
            "scrollX": 0.0,
            "scrollY": 50.0,
            "viewportWidth": 1280.0,
            "viewportHeight": 800.0,
        })
    }

    #[test]
    fn accepts_one_result_for_the_active_token() {
        let state = ElementPickState::default();
        let token = state.begin().expect("OS random source");
        let request = picked(
            &token,
            "[<button>Save</button> in SaveButton (at src/save.tsx:3:2)]",
        );

        assert_eq!(
            state.consume(&request),
            Ok(ElementPickOutcome::Picked(
                Arc::from("[<button>Save</button> in SaveButton (at src/save.tsx:3:2)]"),
                None
            ))
        );
        assert_eq!(
            state.consume(&request),
            Err(ElementPickMessageError::NoActivePick)
        );
    }

    #[test]
    fn rejects_stale_tokens_without_consuming_the_active_pick() {
        let state = ElementPickState::default();
        let stale_token = state.begin().expect("OS random source");
        let active = state.begin().expect("OS random source");

        assert_eq!(
            state.consume(&picked(&stale_token, "[<div />]")),
            Err(ElementPickMessageError::StaleToken)
        );
        assert_eq!(
            state.consume(&picked(&active, "[<div />]")),
            Ok(ElementPickOutcome::Picked(Arc::from("[<div />]"), None))
        );
    }

    #[test]
    fn accepts_cancel_and_failure_outcomes() {
        let state = ElementPickState::default();
        let cancelled = state.begin().expect("OS random source");
        let cancel_request = serde_json::json!({
            "version": 1,
            "kind": "cancelled",
            "token": cancelled,
        })
        .to_string();
        assert_eq!(
            state.consume(&cancel_request),
            Ok(ElementPickOutcome::Cancelled)
        );

        let failed = state.begin().expect("OS random source");
        let failure_request = serde_json::json!({
            "version": 1,
            "kind": "failed",
            "token": failed,
        })
        .to_string();
        assert_eq!(
            state.consume(&failure_request),
            Ok(ElementPickOutcome::Failed)
        );
    }

    #[test]
    fn rejects_multiline_unbracketed_and_oversized_context() {
        let state = ElementPickState::default();
        let token = state.begin().expect("OS random source");

        assert_eq!(
            state.consume(&picked(&token, "[<div />]\nextra")),
            Err(ElementPickMessageError::InvalidContext)
        );
        assert_eq!(
            state.consume(&picked(&token, "<div />")),
            Err(ElementPickMessageError::InvalidContext)
        );
        assert_eq!(
            state.consume(&picked(&token, "[<div\t/>]")),
            Err(ElementPickMessageError::InvalidContext)
        );
        assert_eq!(
            state.consume(&picked(
                &token,
                &format!("[{}]", "x".repeat(MAX_ELEMENT_CONTEXT_BYTES))
            )),
            Err(ElementPickMessageError::InvalidContext)
        );
    }

    #[test]
    fn rejects_bad_protocol_messages_without_consuming_the_pick() {
        let state = ElementPickState::default();
        let token = state.begin().expect("OS random source");

        assert_eq!(
            state.consume("{"),
            Err(ElementPickMessageError::MalformedMessage)
        );
        assert_eq!(
            state.consume(
                &serde_json::json!({
                    "version": 2,
                    "kind": "cancelled",
                    "token": token,
                })
                .to_string()
            ),
            Err(ElementPickMessageError::UnsupportedVersion)
        );
        assert_eq!(
            state.consume(&"x".repeat(MAX_PICKER_MESSAGE_BYTES + 1)),
            Err(ElementPickMessageError::OversizedMessage)
        );
        assert_eq!(
            state.consume(&picked(&token, "[<div />]")),
            Ok(ElementPickOutcome::Picked(Arc::from("[<div />]"), None))
        );
    }

    #[test]
    fn accepts_geometry_alongside_the_context() {
        let state = ElementPickState::default();
        let token = state.begin().expect("OS random source");

        assert_eq!(
            state.consume(&picked_with_geometry(
                &token,
                "[<div />]",
                &sample_geometry()
            )),
            Ok(ElementPickOutcome::Picked(
                Arc::from("[<div />]"),
                Some(PickGeometry {
                    x: 100.0,
                    y: 200.0,
                    width: 300.0,
                    height: 150.0,
                    scroll_x: 0.0,
                    scroll_y: 50.0,
                    viewport_width: 1280.0,
                    viewport_height: 800.0,
                })
            ))
        );
    }

    #[test]
    fn drops_unusable_geometry_without_failing_the_pick() {
        let unusable = [
            serde_json::json!(null),
            serde_json::json!("not an object"),
            serde_json::json!({ "x": 1.0 }),
            {
                let mut value = sample_geometry();
                value["width"] = serde_json::json!(0.0);
                value
            },
            {
                let mut value = sample_geometry();
                value["scrollY"] = serde_json::json!(f64::INFINITY);
                value
            },
            {
                let mut value = sample_geometry();
                value["viewportHeight"] = serde_json::json!(-800.0);
                value
            },
        ];

        for geometry in unusable {
            let state = ElementPickState::default();
            let token = state.begin().expect("OS random source");
            assert_eq!(
                state.consume(&picked_with_geometry(&token, "[<div />]", &geometry)),
                Ok(ElementPickOutcome::Picked(Arc::from("[<div />]"), None)),
                "geometry={geometry}"
            );
        }
    }

    #[test]
    fn native_cancel_invalidates_the_token() {
        let state = ElementPickState::default();
        let token = state.begin().expect("OS random source");
        assert!(state.cancel());
        assert!(!state.cancel());
        assert_eq!(
            state.consume(&picked(&token, "[<div />]")),
            Err(ElementPickMessageError::NoActivePick)
        );
    }
}
