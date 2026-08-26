use std::{
    io::{Read, Write},
    sync::Arc,
};

use zz_terminal::{
    Color, Cursor, CursorStyle, GRAPHEME_TABLE_BIT, KittyLayer, KittyPlacement, NO_COLOR,
    OverlayKind, OverlaySpan, PackedCell, PackedStyle, ScrollbarState, SearchStatus, SessionStatus,
    TerminalDictionary, TerminalDictionaryPatch, TerminalMode, TerminalPatchRowIndices,
    TerminalPatchRows, TerminalPresentation, TerminalViewport, TerminalViewportPatch,
};

use crate::message::{
    MAX_CLIENT_WORKING_DIRECTORY_BYTES, MAX_KITTY_IMAGE_BYTES, MAX_KITTY_IMAGE_CHUNK_BYTES,
    MAX_STARTUP_CONFIG_CAUSES, MAX_STARTUP_CONFIG_CAUSES_BYTES, MAX_STARTUP_CONFIG_CAUSE_BYTES,
};
use crate::{
    AgentSessionOpKind, BrowserCommand, Event, EventPayload, MAX_AGENT_IMAGE_FORMAT_BYTES,
    MAX_AGENT_OPTION_BYTES, MAX_AGENT_PROMPT_BYTES, MAX_AGENT_PROMPT_IMAGES,
    MAX_AGENT_RESULT_BYTES, MAX_AGENT_SEND_BYTES, MAX_AGENT_SESSION_DIRECTORIES,
    MAX_AGENT_SESSION_ID_BYTES, MAX_AGENT_UPDATES_BYTES, MAX_GUI_TEXT_BYTES,
    MAX_PASTE_UPLOAD_BYTES, MAX_PASTE_UPLOAD_CHUNK_BYTES, MAX_PASTE_UPLOAD_EXTENSION_BYTES,
    PROTOCOL_VERSION, PaneId, PasteUploadPurpose, PastedImageFormat, ProtocolMessage,
    agent_update_batch_bytes,
    framing::{
        Lane, MAX_ENCODED_FRAME_BYTES, ProtocolError, begin_enveloped_into, decode_enveloped,
        finish_enveloped_in_place, read_enveloped_into,
    },
    message::{
        MAX_CONFIG_OVERRIDE_ENTRIES, MAX_CONFIG_OVERRIDE_KEY_BYTES,
        MAX_CONFIG_OVERRIDE_VALUE_BYTES, MAX_SERVER_CAPABILITIES, MAX_SERVER_CAPABILITY_BYTES,
    },
    paste_upload_extension_is_valid,
};

const FULL_VIEWPORT: u8 = 0;
const VIEWPORT_PATCH: u8 = 1;
const COMMAND_OUTPUT_VIEWPORT: u8 = 2;
const MAX_TITLE_BYTES: usize = 64 * 1024;
const MAX_WORKING_DIRECTORY_BYTES: usize = 16 * 1024;
const MAX_HOVER_URI_BYTES: usize = 16 * 1024;
const MAX_STATUS_BYTES: usize = 1024 * 1024;
const MAX_STYLE_COUNT: usize = 65_536;
const MAX_GRAPHEME_COUNT: usize = 1024 * 1024;
const MAX_GRAPHEME_BYTES: usize = 16 * 1024 * 1024;
const MAX_OVERLAY_COUNT: usize = 1024 * 1024;
const MAX_KITTY_PLACEMENTS: usize = 512;
const CONTROL_PAYLOAD_RESERVE: usize = 256;
const ROW_RECORD_WIRE_BYTES: usize = 4;
const CELL_WIRE_BYTES: usize = 8;
const STYLE_WIRE_BYTES: usize = 16;
const U32_WIRE_BYTES: usize = 4;
const OVERLAY_WIRE_BYTES: usize = 8;
const KITTY_PLACEMENT_WIRE_BYTES: usize = 72;
// Fixed fields excluding variable mode, search, cursor, status, and section data.
const VIEWPORT_FIXED_BYTES: usize = 115;
const PATCH_FIXED_BYTES: usize = 143;

struct FrameFlavor<'a> {
    output: &'a mut Vec<u8>,
    limit: usize,
}

impl postcard::ser_flavors::Flavor for FrameFlavor<'_> {
    type Output = ();

    fn try_push(&mut self, data: u8) -> postcard::Result<()> {
        if self.output.len() >= self.limit {
            return Err(postcard::Error::SerializeBufferFull);
        }
        self.output.push(data);
        Ok(())
    }

    fn try_extend(&mut self, data: &[u8]) -> postcard::Result<()> {
        if self
            .output
            .len()
            .checked_add(data.len())
            .is_none_or(|len| len > self.limit)
        {
            return Err(postcard::Error::SerializeBufferFull);
        }
        self.output.extend_from_slice(data);
        Ok(())
    }

    fn finalize(self) -> postcard::Result<Self::Output> {
        Ok(())
    }
}

/// Encode a protocol message, selecting the compact terminal lane for viewport events.
pub fn encode_protocol_message(message: &ProtocolMessage) -> Result<Vec<u8>, ProtocolError> {
    let mut output = Vec::new();
    encode_protocol_message_into(message, &mut output)?;
    Ok(output)
}

/// Encode a full terminal viewport event without constructing an owned protocol message.
pub fn encode_terminal_viewport_event(
    pane: PaneId,
    sequence: u64,
    viewport: &TerminalViewport,
) -> Result<Vec<u8>, ProtocolError> {
    let mut output = Vec::new();
    encode_terminal_viewport_event_into(pane, sequence, viewport, &mut output)?;
    Ok(output)
}

/// Encode a borrowed full terminal viewport event into a caller-owned frame buffer.
///
/// Existing capacity is retained and reused. On failure the output is left empty.
pub fn encode_terminal_viewport_event_into(
    pane: PaneId,
    sequence: u64,
    viewport: &TerminalViewport,
    output: &mut Vec<u8>,
) -> Result<(), ProtocolError> {
    output.clear();
    let result = encode_viewport_into(pane, sequence, viewport, FULL_VIEWPORT, None, output);
    if result.is_err() {
        output.clear();
    }
    result
}

/// Encode a protocol message into a caller-owned frame buffer.
///
/// Existing capacity is retained and reused. On failure the output is left empty.
pub fn encode_protocol_message_into(
    message: &ProtocolMessage,
    output: &mut Vec<u8>,
) -> Result<(), ProtocolError> {
    output.clear();
    let result = encode_protocol_message_into_inner(message, output);
    if result.is_err() {
        output.clear();
    }
    result
}

fn encode_protocol_message_into_inner(
    message: &ProtocolMessage,
    output: &mut Vec<u8>,
) -> Result<(), ProtocolError> {
    validate_control_message(message)?;
    if let ProtocolMessage::Event(Event {
        sequence,
        payload: EventPayload::TerminalViewport { pane, viewport },
    }) = message
    {
        return encode_viewport_into(*pane, *sequence, viewport, FULL_VIEWPORT, None, output);
    }
    if let ProtocolMessage::Event(Event {
        sequence,
        payload: EventPayload::TerminalPatch { pane, patch },
    }) = message
    {
        return encode_patch_into(*pane, *sequence, patch, output);
    }
    if let ProtocolMessage::Event(Event {
        sequence,
        payload:
            EventPayload::CommandOutput {
                pane,
                output_id,
                viewport: Some(viewport),
            },
    }) = message
    {
        return encode_viewport_into(
            *pane,
            *sequence,
            viewport,
            COMMAND_OUTPUT_VIEWPORT,
            Some(*output_id),
            output,
        );
    }

    begin_enveloped_into(output, Lane::Control, CONTROL_PAYLOAD_RESERVE)?;
    match postcard::serialize_with_flavor(
        message,
        FrameFlavor {
            output,
            limit: MAX_ENCODED_FRAME_BYTES,
        },
    ) {
        Ok(()) => {}
        Err(postcard::Error::SerializeBufferFull) => {
            return Err(ProtocolError::FrameTooLarge(
                crate::MAX_FRAME_BYTES.saturating_add(1),
            ));
        }
        Err(error) => return Err(ProtocolError::Encode(error)),
    }
    finish_enveloped_in_place(output)
}

/// Decode a complete protocol frame from either the control or terminal lane.
pub fn decode_protocol_frame(frame: &[u8]) -> Result<ProtocolMessage, ProtocolError> {
    let (lane, payload) = decode_enveloped(frame)?;
    decode_protocol_payload(lane, payload.as_ref())
}

/// Write and flush a protocol message using its appropriate lane.
pub fn write_protocol_message(
    writer: &mut impl Write,
    message: &ProtocolMessage,
) -> Result<(), ProtocolError> {
    writer.write_all(&encode_protocol_message(message)?)?;
    writer.flush()?;
    Ok(())
}

/// Read a protocol message from either the control or terminal lane.
pub fn read_protocol_message(reader: &mut impl Read) -> Result<ProtocolMessage, ProtocolError> {
    let mut frame = Vec::new();
    read_protocol_message_into(reader, &mut frame)
}

/// Read a protocol message through a caller-owned transport buffer.
///
/// Existing capacity is retained and the envelope payload is decoded directly from that buffer.
pub fn read_protocol_message_into(
    reader: &mut impl Read,
    frame: &mut Vec<u8>,
) -> Result<ProtocolMessage, ProtocolError> {
    let (lane, payload) = read_enveloped_into(reader, frame)?;
    decode_protocol_payload(lane, payload.as_ref())
}

fn decode_protocol_payload(lane: Lane, payload: &[u8]) -> Result<ProtocolMessage, ProtocolError> {
    match lane {
        Lane::Control => {
            let message = postcard::from_bytes(payload).map_err(ProtocolError::Decode)?;
            validate_control_message(&message)?;
            Ok(message)
        }
        Lane::Terminal => match payload.first().copied() {
            Some(FULL_VIEWPORT) => {
                let (pane, sequence, viewport) = decode_viewport(payload)?;
                Ok(ProtocolMessage::Event(Event {
                    sequence,
                    payload: EventPayload::TerminalViewport { pane, viewport },
                }))
            }
            Some(VIEWPORT_PATCH) => {
                let (pane, sequence, patch) = decode_patch(payload)?;
                Ok(ProtocolMessage::Event(Event {
                    sequence,
                    payload: EventPayload::TerminalPatch { pane, patch },
                }))
            }
            Some(COMMAND_OUTPUT_VIEWPORT) => {
                let (pane, sequence, output_id, viewport) =
                    decode_viewport_kind(payload, COMMAND_OUTPUT_VIEWPORT)?;
                let Some(output_id) = output_id else {
                    return invalid("command output viewport is missing its output ID");
                };
                Ok(ProtocolMessage::Event(Event {
                    sequence,
                    payload: EventPayload::CommandOutput {
                        pane,
                        output_id,
                        viewport: Some(viewport),
                    },
                }))
            }
            _ => invalid("unknown terminal update type"),
        },
    }
}

fn validate_control_message(message: &ProtocolMessage) -> Result<(), ProtocolError> {
    if let ProtocolMessage::Event(Event {
        payload:
            EventPayload::CommandOutput {
                output_id,
                viewport: Some(_),
                ..
            },
        ..
    }) = message
        && *output_id == 0
    {
        return invalid("command output viewport has a zero output ID");
    }
    if let ProtocolMessage::ClientHello(hello) = message {
        if !capabilities_are_bounded(&hello.capabilities) {
            return Err(ProtocolError::InvalidClientHello(format!(
                "capabilities must contain at most {MAX_SERVER_CAPABILITIES} entries of at most \
                 {MAX_SERVER_CAPABILITY_BYTES} bytes"
            )));
        }
        if hello.working_directory.as_ref().is_some_and(|path| {
            path.as_os_str().as_encoded_bytes().len() > MAX_CLIENT_WORKING_DIRECTORY_BYTES
        }) {
            return Err(ProtocolError::InvalidClientHello(format!(
                "working directory must be at most {MAX_CLIENT_WORKING_DIRECTORY_BYTES} bytes"
            )));
        }
    }
    if let ProtocolMessage::ServerHello(hello) = message {
        if hello.protocol_version != PROTOCOL_VERSION {
            return Err(ProtocolError::VersionMismatch {
                expected: PROTOCOL_VERSION,
                received: hello.protocol_version,
            });
        }
        if !capabilities_are_bounded(&hello.capabilities) {
            return Err(ProtocolError::InvalidServerHello(format!(
                "capabilities must contain at most {MAX_SERVER_CAPABILITIES} entries of at most \
                 {MAX_SERVER_CAPABILITY_BYTES} bytes"
            )));
        }
        hello
            .appearance
            .validate()
            .map_err(|error| ProtocolError::InvalidAppearance(error.to_string()))?;
        hello
            .appearance_provenance
            .validate()
            .map_err(|error| ProtocolError::InvalidServerHello(error.to_owned()))?;
        hello
            .mux_options
            .validate()
            .map_err(|error| ProtocolError::InvalidServerHello(error.to_owned()))?;
        hello
            .status
            .validate()
            .map_err(|error| ProtocolError::InvalidServerHello(error.to_owned()))?;
    }
    if let ProtocolMessage::Event(Event {
        payload:
            EventPayload::AppearanceChanged {
                appearance,
                provenance,
            },
        ..
    }) = message
    {
        appearance
            .validate()
            .map_err(|error| ProtocolError::InvalidAppearance(error.to_string()))?;
        provenance
            .validate()
            .map_err(|error| ProtocolError::InvalidAppearance(error.to_owned()))?;
    }
    if let ProtocolMessage::Event(Event {
        payload: EventPayload::MuxOptionsChanged { options },
        ..
    }) = message
    {
        options
            .validate()
            .map_err(|error| ProtocolError::InvalidConfigOverrides(error.to_owned()))?;
    }
    if let ProtocolMessage::Event(Event {
        payload: EventPayload::StatusChanged { status },
        ..
    }) = message
    {
        status
            .validate()
            .map_err(|error| ProtocolError::InvalidStatusLine(error.to_owned()))?;
    }
    if let ProtocolMessage::Event(Event {
        payload: EventPayload::StartupConfigCauses { causes },
        ..
    }) = message
    {
        if causes.len() > MAX_STARTUP_CONFIG_CAUSES {
            return Err(ProtocolError::InvalidStartupConfigCauses(format!(
                "startup configuration causes must contain at most {MAX_STARTUP_CONFIG_CAUSES} entries"
            )));
        }
        if causes
            .iter()
            .any(|cause| cause.len() > MAX_STARTUP_CONFIG_CAUSE_BYTES)
        {
            return Err(ProtocolError::InvalidStartupConfigCauses(format!(
                "startup configuration causes must be at most {MAX_STARTUP_CONFIG_CAUSE_BYTES} bytes each"
            )));
        }
        if causes
            .iter()
            .try_fold(0usize, |total, cause| total.checked_add(cause.len()))
            .is_none_or(|total| total > MAX_STARTUP_CONFIG_CAUSES_BYTES)
        {
            return Err(ProtocolError::InvalidStartupConfigCauses(format!(
                "startup configuration causes must total at most {MAX_STARTUP_CONFIG_CAUSES_BYTES} bytes"
            )));
        }
    }
    if let ProtocolMessage::Event(Event {
        payload: EventPayload::AgentCommand { command, .. },
        ..
    }) = message
        && command.text().len() > MAX_AGENT_SEND_BYTES
    {
        return Err(ProtocolError::InvalidGuiRequest(format!(
            "agent payload must be at most {MAX_AGENT_SEND_BYTES} bytes"
        )));
    }
    if let ProtocolMessage::Event(Event {
        payload:
            EventPayload::BrowserCommand {
                command: BrowserCommand::Screenshot { path, .. },
                ..
            },
        ..
    }) = message
        && path.len() > MAX_GUI_TEXT_BYTES
    {
        return Err(ProtocolError::InvalidGuiRequest(format!(
            "screenshot path must be at most {MAX_GUI_TEXT_BYTES} bytes"
        )));
    }
    if let ProtocolMessage::GuiResponse(response) = message {
        response
            .validate()
            .map_err(|error| ProtocolError::InvalidGuiRequest(error.to_owned()))?;
    }
    if let ProtocolMessage::PasteUploadBegin {
        purpose,
        extension,
        total_bytes,
        ..
    } = message
    {
        if !paste_upload_extension_is_valid(extension) {
            return Err(ProtocolError::InvalidPasteUpload(format!(
                "paste upload extension must be 1 to {MAX_PASTE_UPLOAD_EXTENSION_BYTES} lowercase \
                 ASCII alphanumerics"
            )));
        }
        if *total_bytes == 0 || *total_bytes > MAX_PASTE_UPLOAD_BYTES {
            return Err(ProtocolError::InvalidPasteUpload(format!(
                "paste upload must carry 1 to {MAX_PASTE_UPLOAD_BYTES} bytes"
            )));
        }
        if *purpose == PasteUploadPurpose::RecordPastedImage
            && PastedImageFormat::from_extension(extension).is_none()
        {
            return Err(ProtocolError::InvalidPasteUpload(
                "recorded pasted images must be png, jpeg, gif, or webp".to_owned(),
            ));
        }
    }
    if let ProtocolMessage::PasteUploadChunk { bytes, .. } = message
        && bytes.len() > MAX_PASTE_UPLOAD_CHUNK_BYTES
    {
        return Err(ProtocolError::InvalidPasteUpload(format!(
            "paste upload chunk must be at most {MAX_PASTE_UPLOAD_CHUNK_BYTES} bytes"
        )));
    }
    if let ProtocolMessage::PastedImageBegin { total_bytes, .. } = message
        && (*total_bytes == 0 || *total_bytes > MAX_PASTE_UPLOAD_BYTES)
    {
        return Err(ProtocolError::InvalidPasteUpload(format!(
            "pasted image preview must carry 1 to {MAX_PASTE_UPLOAD_BYTES} bytes"
        )));
    }
    if let ProtocolMessage::PastedImageChunk { bytes, .. } = message
        && bytes.len() > MAX_PASTE_UPLOAD_CHUNK_BYTES
    {
        return Err(ProtocolError::InvalidPasteUpload(format!(
            "pasted image preview chunk must be at most {MAX_PASTE_UPLOAD_CHUNK_BYTES} bytes"
        )));
    }
    if let ProtocolMessage::Event(Event {
        payload:
            EventPayload::KittyImageBegin {
                width,
                height,
                total_bytes,
                ..
            },
        ..
    }) = message
    {
        let expected = width
            .checked_mul(*height)
            .and_then(|pixels| pixels.checked_mul(4));
        if *total_bytes == 0
            || *total_bytes > MAX_KITTY_IMAGE_BYTES
            || expected != Some(*total_bytes)
        {
            return invalid("kitty image dimensions do not match its bounded BGRA byte length");
        }
    }
    if let ProtocolMessage::Event(Event {
        payload: EventPayload::KittyImageChunk { bytes, .. },
        ..
    }) = message
        && bytes.len() > MAX_KITTY_IMAGE_CHUNK_BYTES
    {
        return invalid("kitty image chunk exceeds its wire limit");
    }
    if let ProtocolMessage::Event(Event {
        payload: EventPayload::KittyImagesRemoved { image_ids, .. },
        ..
    }) = message
        && image_ids.len() > MAX_KITTY_PLACEMENTS
    {
        return invalid("kitty image removal list exceeds its wire limit");
    }
    if let ProtocolMessage::AgentPrompt { text, images, .. } = message {
        let total = images.iter().try_fold(text.len(), |total, image| {
            total.checked_add(image.data.len())
        });
        if total.is_none_or(|total| total > MAX_AGENT_PROMPT_BYTES) {
            return Err(ProtocolError::InvalidAgentPayload(format!(
                "agent prompt text and images must total at most {MAX_AGENT_PROMPT_BYTES} bytes"
            )));
        }
        if images
            .iter()
            .any(|image| image.format.len() > MAX_AGENT_IMAGE_FORMAT_BYTES)
        {
            return Err(ProtocolError::InvalidAgentPayload(format!(
                "agent prompt image formats must be at most {MAX_AGENT_IMAGE_FORMAT_BYTES} bytes"
            )));
        }
        if images.len() > MAX_AGENT_PROMPT_IMAGES {
            return Err(ProtocolError::InvalidAgentPayload(format!(
                "agent prompts may attach at most {MAX_AGENT_PROMPT_IMAGES} images"
            )));
        }
    }
    if !agent_option_strings_are_bounded(message) {
        return Err(ProtocolError::InvalidAgentPayload(format!(
            "agent option, mode, and method identifiers must be at most \
             {MAX_AGENT_OPTION_BYTES} bytes"
        )));
    }
    if let ProtocolMessage::AgentSessionOp { op, .. } = message {
        let session_id = match op {
            AgentSessionOpKind::Switch { session_id, .. }
            | AgentSessionOpKind::Delete { session_id } => Some(session_id),
            AgentSessionOpKind::List { .. } | AgentSessionOpKind::New { .. } => None,
        };
        if session_id.is_some_and(|session_id| session_id.len() > MAX_AGENT_SESSION_ID_BYTES) {
            return Err(ProtocolError::InvalidAgentPayload(format!(
                "agent session IDs must be at most {MAX_AGENT_SESSION_ID_BYTES} bytes"
            )));
        }
        if matches!(
            op,
            AgentSessionOpKind::List {
                cursor: Some(cursor),
                ..
            } if cursor.len() > MAX_AGENT_SESSION_ID_BYTES
        ) {
            return Err(ProtocolError::InvalidAgentPayload(format!(
                "agent session cursors must be at most {MAX_AGENT_SESSION_ID_BYTES} bytes"
            )));
        }
        let (cwd, additional_directories) = match op {
            AgentSessionOpKind::List { cwd, .. } => (cwd.as_ref(), &[][..]),
            AgentSessionOpKind::New { cwd } => (Some(cwd), &[][..]),
            AgentSessionOpKind::Switch {
                cwd,
                additional_directories,
                ..
            } => (Some(cwd), additional_directories.as_slice()),
            AgentSessionOpKind::Delete { .. } => (None, &[][..]),
        };
        if cwd.is_some_and(|path| {
            path.as_os_str().is_empty()
                || path.as_os_str().as_encoded_bytes().len() > MAX_GUI_TEXT_BYTES
        }) || additional_directories.len() > MAX_AGENT_SESSION_DIRECTORIES
            || additional_directories.iter().any(|path| {
                path.as_os_str().is_empty()
                    || path.as_os_str().as_encoded_bytes().len() > MAX_GUI_TEXT_BYTES
            })
        {
            return Err(ProtocolError::InvalidAgentPayload(
                "agent session directories must be nonempty and stay inside their wire limits"
                    .to_owned(),
            ));
        }
    }
    if let ProtocolMessage::Event(Event {
        payload: EventPayload::AgentUpdates {
            first_seq, items, ..
        },
        ..
    }) = message
    {
        if items.is_empty() || first_seq.checked_add(items.len() as u64).is_none() {
            return Err(ProtocolError::InvalidAgentPayload(
                "agent update batches must be nonempty and stay inside the sequence space"
                    .to_owned(),
            ));
        }
        if agent_update_batch_bytes(items) > MAX_AGENT_UPDATES_BYTES {
            return Err(ProtocolError::InvalidAgentPayload(format!(
                "agent update batches must total at most {MAX_AGENT_UPDATES_BYTES} bytes"
            )));
        }
    }
    if let ProtocolMessage::Event(Event {
        payload: EventPayload::AgentState { state, .. },
        ..
    }) = message
    {
        state
            .validate()
            .map_err(|error| ProtocolError::InvalidAgentPayload(error.to_owned()))?;
    }
    if let ProtocolMessage::Event(Event {
        payload: EventPayload::AgentSessions { result, .. },
        ..
    }) = message
        && result.len() > MAX_AGENT_RESULT_BYTES
    {
        return Err(ProtocolError::InvalidAgentPayload(format!(
            "agent request results must be at most {MAX_AGENT_RESULT_BYTES} bytes"
        )));
    }
    if let ProtocolMessage::SetConfigOverrides { entries } = message
        && (entries.len() > MAX_CONFIG_OVERRIDE_ENTRIES
            || entries.iter().any(|(key, value)| {
                key.is_empty()
                    || key.len() > MAX_CONFIG_OVERRIDE_KEY_BYTES
                    || value.len() > MAX_CONFIG_OVERRIDE_VALUE_BYTES
                    || key.chars().any(char::is_control)
                    || value
                        .chars()
                        .any(|character| matches!(character, '\r' | '\n'))
            }))
    {
        return Err(ProtocolError::InvalidConfigOverrides(format!(
            "entries must contain at most {MAX_CONFIG_OVERRIDE_ENTRIES} single-line key/value pairs; keys must be 1..={MAX_CONFIG_OVERRIDE_KEY_BYTES} bytes and values at most {MAX_CONFIG_OVERRIDE_VALUE_BYTES} bytes"
        )));
    }
    Ok(())
}

fn agent_option_strings_are_bounded(message: &ProtocolMessage) -> bool {
    let bounded = |text: &str| text.len() <= MAX_AGENT_OPTION_BYTES;
    match message {
        ProtocolMessage::AgentRespondPermission { option_id, .. } => {
            option_id.as_deref().is_none_or(bounded)
        }
        ProtocolMessage::AgentSetConfigOption {
            option_id, value, ..
        } => bounded(option_id) && bounded(value),
        ProtocolMessage::AgentSetMode { mode_id, .. } => bounded(mode_id),
        ProtocolMessage::AgentAuthenticate { method_id, .. } => bounded(method_id),
        _ => true,
    }
}

fn capabilities_are_bounded(capabilities: &[String]) -> bool {
    capabilities.len() <= MAX_SERVER_CAPABILITIES
        && capabilities
            .iter()
            .all(|capability| capability.len() <= MAX_SERVER_CAPABILITY_BYTES)
}

fn encode_viewport_into(
    pane: PaneId,
    sequence: u64,
    viewport: &TerminalViewport,
    kind: u8,
    output_id: Option<u64>,
    output: &mut Vec<u8>,
) -> Result<(), ProtocolError> {
    if kind == COMMAND_OUTPUT_VIEWPORT {
        if output_id == Some(0) {
            return invalid("command output viewport has a zero output ID");
        }
        if output_id.is_none() {
            return invalid("command output viewport is missing its output ID");
        }
    } else if output_id.is_some() {
        return invalid("terminal viewport carries a command output ID");
    }
    validate_viewport(viewport)?;
    let title = viewport.title().as_bytes();
    if title.len() > MAX_TITLE_BYTES {
        return invalid("title exceeds terminal metadata limit");
    }
    let working_directory = viewport.working_directory().unwrap_or_default().as_bytes();
    if working_directory.len() > MAX_WORKING_DIRECTORY_BYTES {
        return invalid("working directory exceeds terminal metadata limit");
    }
    let hovered_uri = viewport.hovered_uri().unwrap_or_default().as_bytes();
    if hovered_uri.len() > MAX_HOVER_URI_BYTES {
        return invalid("hovered URI exceeds terminal metadata limit");
    }

    let capacity = checked_wire_capacity(&[
        viewport_payload_capacity(
            viewport,
            title.len(),
            working_directory.len(),
            hovered_uri.len(),
        )?,
        output_id.map_or(0, |_| 8),
    ])?;
    begin_enveloped_into(output, Lane::Terminal, capacity)?;
    output.push(kind);
    push_u64(output, pane.0);
    push_u64(output, sequence);
    if let Some(output_id) = output_id {
        push_u64(output, output_id);
    }
    push_u64(output, viewport.generation);
    push_u64(output, viewport.view_generation);
    push_u32(output, viewport.dictionary_generation);
    push_u16(output, viewport.columns);
    push_u16(output, viewport.rows);
    push_u32(output, viewport.foreground.packed());
    push_u32(output, viewport.background.packed());
    push_u64(output, u64::from(viewport.scrollbar.total));
    push_u64(output, u64::from(viewport.scrollbar.offset));
    push_u64(output, u64::from(viewport.scrollbar.len));
    output.push(u8::from(viewport.kitty_keyboard));
    output.push(u8::from(viewport.mouse_tracking));
    encode_mode(output, viewport.mode);
    encode_search(output, viewport.search);
    push_u64(output, u64::from(viewport.unseen_output));
    encode_cursor(output, viewport.cursor);
    push_u32(output, checked_count(title.len())?);
    push_u32(output, checked_count(working_directory.len())?);
    push_u32(output, checked_count(hovered_uri.len())?);
    push_u32(output, checked_count(viewport.cells.len())?);
    push_u32(output, checked_count(viewport.styles().len())?);
    push_u32(output, checked_count(viewport.grapheme_offsets().len())?);
    push_u32(output, checked_count(viewport.grapheme_bytes().len())?);
    push_u32(output, checked_count(viewport.overlays.len())?);
    encode_status(output, &viewport.status)?;
    output.extend_from_slice(title);
    output.extend_from_slice(working_directory);
    output.extend_from_slice(hovered_uri);

    for cell in viewport.cells.iter() {
        push_u32(output, cell.glyph());
        push_u16(output, cell.style_id());
        push_u16(output, cell.flags());
    }
    for style in viewport.styles() {
        push_u32(output, style.foreground_raw());
        push_u32(output, style.background_raw());
        push_u32(output, style.underline_color_raw());
        push_u16(output, style.attributes());
        output.push(style.underline_kind_raw());
        output.push(0);
    }
    for offset in viewport.grapheme_offsets() {
        push_u32(output, *offset);
    }
    output.extend_from_slice(viewport.grapheme_bytes());
    for overlay in viewport.overlays.iter() {
        push_u16(output, overlay.row);
        push_u16(output, overlay.start);
        push_u16(output, overlay.end);
        push_u16(output, overlay.kind_and_flags());
    }
    push_u32(output, checked_count(viewport.kitty_placements.len())?);
    encode_kitty_placements(output, &viewport.kitty_placements);
    finish_enveloped_in_place(output)
}

fn encode_patch_into(
    pane: PaneId,
    sequence: u64,
    patch: &TerminalViewportPatch,
    output: &mut Vec<u8>,
) -> Result<(), ProtocolError> {
    validate_patch(patch)?;
    let title = patch.title().as_bytes();
    if title.len() > MAX_TITLE_BYTES {
        return invalid("title exceeds terminal metadata limit");
    }
    let working_directory = patch.working_directory().unwrap_or_default().as_bytes();
    if working_directory.len() > MAX_WORKING_DIRECTORY_BYTES {
        return invalid("working directory exceeds terminal metadata limit");
    }
    let hovered_uri = patch.hovered_uri().unwrap_or_default().as_bytes();
    if hovered_uri.len() > MAX_HOVER_URI_BYTES {
        return invalid("hovered URI exceeds terminal metadata limit");
    }
    let capacity = patch_payload_capacity(
        patch,
        title.len(),
        working_directory.len(),
        hovered_uri.len(),
    )?;
    begin_enveloped_into(output, Lane::Terminal, capacity)?;
    output.push(VIEWPORT_PATCH);
    push_u64(output, pane.0);
    push_u64(output, sequence);
    push_u64(output, patch.base_generation);
    push_u64(output, patch.base_view_generation);
    push_u64(output, patch.generation);
    push_u64(output, patch.view_generation);
    push_u32(output, patch.dictionary_generation);
    push_u16(output, patch.columns);
    push_u16(output, patch.rows);
    output.extend_from_slice(&patch.scroll.to_le_bytes());
    push_u16(output, 0);
    push_u32(output, patch.foreground.packed());
    push_u32(output, patch.background.packed());
    push_u64(output, u64::from(patch.scrollbar.total));
    push_u64(output, u64::from(patch.scrollbar.offset));
    push_u64(output, u64::from(patch.scrollbar.len));
    output.push(u8::from(patch.kitty_keyboard));
    output.push(u8::from(patch.mouse_tracking));
    encode_mode(output, patch.mode);
    encode_search(output, patch.search);
    push_u64(output, u64::from(patch.unseen_output));
    encode_cursor(output, patch.cursor);
    push_u32(output, checked_count(title.len())?);
    push_u32(output, checked_count(working_directory.len())?);
    push_u32(output, checked_count(hovered_uri.len())?);
    push_u32(output, checked_count(patch.changed_rows.len())?);
    push_u32(output, patch.style_base);
    push_u32(
        output,
        checked_count(patch.dictionary.appended_styles().len())?,
    );
    push_u32(output, patch.grapheme_base);
    push_u32(
        output,
        checked_count(patch.dictionary.appended_grapheme_lengths().len())?,
    );
    push_u32(
        output,
        checked_count(patch.dictionary.appended_grapheme_bytes().len())?,
    );
    push_u32(output, checked_count(patch.overlays.len())?);
    encode_status(output, &patch.status)?;
    output.extend_from_slice(title);
    output.extend_from_slice(working_directory);
    output.extend_from_slice(hovered_uri);
    let columns = usize::from(patch.columns);
    for (patch_row, row) in patch.changed_rows.row_indices().iter().copied().enumerate() {
        push_u16(output, row);
        push_u16(output, 0);
        let start = patch_row * columns;
        for cell in &patch.changed_rows.cells()[start..start + columns] {
            push_u32(output, cell.glyph());
            push_u16(output, cell.style_id());
            push_u16(output, cell.flags());
        }
    }
    for style in patch.dictionary.appended_styles() {
        push_u32(output, style.foreground_raw());
        push_u32(output, style.background_raw());
        push_u32(output, style.underline_color_raw());
        push_u16(output, style.attributes());
        output.push(style.underline_kind_raw());
        output.push(0);
    }
    for length in patch.dictionary.appended_grapheme_lengths() {
        push_u32(output, *length);
    }
    output.extend_from_slice(patch.dictionary.appended_grapheme_bytes());
    for overlay in patch.overlays.iter() {
        push_u16(output, overlay.row);
        push_u16(output, overlay.start);
        push_u16(output, overlay.end);
        push_u16(output, overlay.kind_and_flags());
    }
    push_u32(output, checked_count(patch.kitty_placements.len())?);
    encode_kitty_placements(output, &patch.kitty_placements);
    finish_enveloped_in_place(output)
}

fn decode_patch(payload: &[u8]) -> Result<(PaneId, u64, TerminalViewportPatch), ProtocolError> {
    let mut reader = WireReader::new(payload);
    if reader.u8()? != VIEWPORT_PATCH {
        return invalid("unknown terminal patch type");
    }
    let pane = PaneId(reader.u64()?);
    let sequence = reader.u64()?;
    let base_generation = reader.u64()?;
    let base_view_generation = reader.u64()?;
    let generation = reader.u64()?;
    let view_generation = reader.u64()?;
    let dictionary_generation = reader.u32()?;
    let columns = reader.u16()?;
    let rows = reader.u16()?;
    let scroll = reader.i16()?;
    if reader.u16()? != 0 {
        return invalid("terminal patch reserved field is nonzero");
    }
    let foreground = decode_color(reader.u32()?)?;
    let background = decode_color(reader.u32()?)?;
    let scrollbar = ScrollbarState {
        total: compact_u32(&mut reader, "scrollbar total")?,
        offset: compact_u32(&mut reader, "scrollbar offset")?,
        len: compact_u32(&mut reader, "scrollbar length")?,
    };
    let kitty_keyboard = decode_bool(reader.u8()?)?;
    let mouse_tracking = decode_bool(reader.u8()?)?;
    let mode = decode_mode(&mut reader)?;
    let search = decode_search(&mut reader)?;
    let unseen_output = compact_u32(&mut reader, "unseen output count")?;
    let cursor = decode_cursor(&mut reader)?;
    let title_len = reader.count(MAX_TITLE_BYTES, "title")?;
    let working_directory_len = reader.count(MAX_WORKING_DIRECTORY_BYTES, "working directory")?;
    let hovered_uri_len = reader.count(MAX_HOVER_URI_BYTES, "hovered URI")?;
    let changed_count = reader.count(usize::from(rows), "changed row")?;
    let style_base = reader.u32()?;
    let appended_style_count = reader.count(MAX_STYLE_COUNT, "appended style")?;
    if usize::try_from(style_base)
        .ok()
        .and_then(|base| base.checked_add(appended_style_count))
        .is_none_or(|count| count > MAX_STYLE_COUNT)
    {
        return invalid("terminal patch style dictionary exceeds its limit");
    }
    let grapheme_base = reader.u32()?;
    let appended_grapheme_count = reader.count(MAX_GRAPHEME_COUNT, "appended grapheme")?;
    if usize::try_from(grapheme_base)
        .ok()
        .and_then(|base| base.checked_add(appended_grapheme_count))
        .is_none_or(|count| count > MAX_GRAPHEME_COUNT)
    {
        return invalid("terminal patch grapheme dictionary exceeds its limit");
    }
    let appended_grapheme_bytes_len =
        reader.count(MAX_GRAPHEME_BYTES, "appended grapheme arena")?;
    let overlay_count = reader.count(MAX_OVERLAY_COUNT, "overlay")?;
    let changed_cell_count = changed_count
        .checked_mul(usize::from(columns))
        .ok_or_else(|| {
            ProtocolError::InvalidTerminal("terminal patch cell count overflows usize".to_owned())
        })?;
    let mut preflight = reader;
    preflight_status(&mut preflight)?;
    preflight.bytes(title_len)?;
    preflight.bytes(working_directory_len)?;
    preflight.bytes(hovered_uri_len)?;
    let section_bytes = checked_wire_capacity(&[
        checked_wire_section(changed_count, ROW_RECORD_WIRE_BYTES)?,
        checked_wire_section(changed_cell_count, CELL_WIRE_BYTES)?,
        checked_wire_section(appended_style_count, STYLE_WIRE_BYTES)?,
        checked_wire_section(appended_grapheme_count, U32_WIRE_BYTES)?,
        appended_grapheme_bytes_len,
        checked_wire_section(overlay_count, OVERLAY_WIRE_BYTES)?,
    ])?;
    preflight.bytes(section_bytes)?;
    let kitty_placement_count = preflight.count(MAX_KITTY_PLACEMENTS, "kitty placement")?;
    expect_exact_remaining(
        &preflight,
        checked_wire_section(kitty_placement_count, KITTY_PLACEMENT_WIRE_BYTES)?,
        "terminal patch has trailing bytes",
    )?;

    let status = decode_status(&mut reader)?;
    let title = Arc::from(reader.utf8(title_len, "title")?);
    let working_directory = reader.utf8(working_directory_len, "working directory")?;
    let working_directory =
        (!working_directory.is_empty()).then(|| Arc::<str>::from(working_directory));
    let hovered_uri = reader.utf8(hovered_uri_len, "hovered URI")?;
    let hovered_uri = (!hovered_uri.is_empty()).then(|| Arc::<str>::from(hovered_uri));
    let mut changed_row_indices = TerminalPatchRowIndices::with_capacity(changed_count);
    let mut changed_cells = Vec::with_capacity(changed_cell_count);
    for _ in 0..changed_count {
        let row = reader.u16()?;
        if reader.u16()? != 0 {
            return invalid("terminal row patch reserved field is nonzero");
        }
        changed_row_indices.push(row);
        for _ in 0..columns {
            changed_cells.push(PackedCell::from_raw(
                reader.u32()?,
                reader.u16()?,
                reader.u16()?,
            ));
        }
    }
    let changed_rows = TerminalPatchRows::from_flat(changed_row_indices, changed_cells);
    let mut appended_styles = Vec::with_capacity(appended_style_count);
    for _ in 0..appended_style_count {
        appended_styles.push(PackedStyle::from_raw(
            reader.u32()?,
            reader.u32()?,
            reader.u32()?,
            reader.u16()?,
            reader.u8()?,
        ));
        if reader.u8()? != 0 {
            return invalid("packed style reserved byte is nonzero");
        }
    }
    let mut appended_grapheme_lengths = Vec::with_capacity(appended_grapheme_count);
    for _ in 0..appended_grapheme_count {
        appended_grapheme_lengths.push(reader.u32()?);
    }
    let appended_grapheme_bytes = reader.bytes(appended_grapheme_bytes_len)?.to_vec();
    let overlay_bytes = checked_wire_section(overlay_count, OVERLAY_WIRE_BYTES)?;
    let overlays = decode_overlays(reader.bytes(overlay_bytes)?);
    let kitty_placement_count = reader.count(MAX_KITTY_PLACEMENTS, "kitty placement")?;
    let kitty_placements = decode_kitty_placements(&mut reader, kitty_placement_count)?;
    if !reader.is_empty() {
        return invalid("terminal patch has trailing bytes");
    }
    let patch = TerminalViewportPatch {
        base_generation,
        base_view_generation,
        generation,
        view_generation,
        dictionary_generation,
        columns,
        rows,
        scroll,
        changed_rows,
        style_base,
        grapheme_base,
        dictionary: TerminalDictionaryPatch::from_parts(
            appended_styles,
            appended_grapheme_lengths,
            appended_grapheme_bytes,
        ),
        foreground,
        background,
        presentation: Arc::new(TerminalPresentation::new(
            title,
            working_directory,
            hovered_uri,
        )),
        overlays,
        kitty_placements,
        cursor,
        scrollbar,
        mode,
        search,
        unseen_output,
        kitty_keyboard,
        mouse_tracking,
        status,
    };
    validate_patch(&patch)?;
    Ok((pane, sequence, patch))
}

fn decode_viewport(payload: &[u8]) -> Result<(PaneId, u64, TerminalViewport), ProtocolError> {
    let (pane, sequence, output_id, viewport) = decode_viewport_kind(payload, FULL_VIEWPORT)?;
    if output_id.is_some() {
        return invalid("terminal viewport carries a command output ID");
    }
    Ok((pane, sequence, viewport))
}

fn decode_viewport_kind(
    payload: &[u8],
    expected_kind: u8,
) -> Result<(PaneId, u64, Option<u64>, TerminalViewport), ProtocolError> {
    let mut reader = WireReader::new(payload);
    if reader.u8()? != expected_kind {
        return invalid("unknown terminal update type");
    }
    let pane = PaneId(reader.u64()?);
    let sequence = reader.u64()?;
    let output_id = if expected_kind == COMMAND_OUTPUT_VIEWPORT {
        let output_id = reader.u64()?;
        if output_id == 0 {
            return invalid("command output viewport has a zero output ID");
        }
        Some(output_id)
    } else {
        None
    };
    let generation = reader.u64()?;
    let view_generation = reader.u64()?;
    let dictionary_generation = reader.u32()?;
    let columns = reader.u16()?;
    let rows = reader.u16()?;
    let foreground = decode_color(reader.u32()?)?;
    let background = decode_color(reader.u32()?)?;
    let scrollbar = ScrollbarState {
        total: compact_u32(&mut reader, "scrollbar total")?,
        offset: compact_u32(&mut reader, "scrollbar offset")?,
        len: compact_u32(&mut reader, "scrollbar length")?,
    };
    let kitty_keyboard = decode_bool(reader.u8()?)?;
    let mouse_tracking = decode_bool(reader.u8()?)?;
    let mode = decode_mode(&mut reader)?;
    let search = decode_search(&mut reader)?;
    let unseen_output = compact_u32(&mut reader, "unseen output count")?;
    let cursor = decode_cursor(&mut reader)?;
    let title_len = reader.count(MAX_TITLE_BYTES, "title")?;
    let working_directory_len = reader.count(MAX_WORKING_DIRECTORY_BYTES, "working directory")?;
    let hovered_uri_len = reader.count(MAX_HOVER_URI_BYTES, "hovered URI")?;
    let expected_cells = usize::from(columns)
        .checked_mul(usize::from(rows))
        .ok_or_else(|| ProtocolError::InvalidTerminal("grid dimensions overflow".to_owned()))?;
    let cell_count = reader.count(expected_cells, "cell")?;
    if cell_count != expected_cells {
        return invalid("cell count does not match grid dimensions");
    }
    let style_count = reader.count(MAX_STYLE_COUNT, "style")?;
    if style_count == 0 {
        return invalid("style dictionary is empty");
    }
    let offset_count = reader.count(MAX_GRAPHEME_COUNT.saturating_add(1), "grapheme offset")?;
    if offset_count == 0 {
        return invalid("grapheme offset table is empty");
    }
    let grapheme_len = reader.count(MAX_GRAPHEME_BYTES, "grapheme arena")?;
    let overlay_count = reader.count(MAX_OVERLAY_COUNT, "overlay")?;
    let mut preflight = reader;
    preflight_status(&mut preflight)?;
    preflight.bytes(title_len)?;
    preflight.bytes(working_directory_len)?;
    preflight.bytes(hovered_uri_len)?;
    let section_bytes = checked_wire_capacity(&[
        checked_wire_section(cell_count, CELL_WIRE_BYTES)?,
        checked_wire_section(style_count, STYLE_WIRE_BYTES)?,
        checked_wire_section(offset_count, U32_WIRE_BYTES)?,
        grapheme_len,
        checked_wire_section(overlay_count, OVERLAY_WIRE_BYTES)?,
    ])?;
    preflight.bytes(section_bytes)?;
    let kitty_placement_count = preflight.count(MAX_KITTY_PLACEMENTS, "kitty placement")?;
    expect_exact_remaining(
        &preflight,
        checked_wire_section(kitty_placement_count, KITTY_PLACEMENT_WIRE_BYTES)?,
        "terminal update has trailing bytes",
    )?;

    let status = decode_status(&mut reader)?;
    let title = reader.utf8(title_len, "title")?;
    let working_directory = reader.utf8(working_directory_len, "working directory")?;
    let working_directory =
        (!working_directory.is_empty()).then(|| Arc::<str>::from(working_directory));
    let hovered_uri = reader.utf8(hovered_uri_len, "hovered URI")?;
    let hovered_uri = (!hovered_uri.is_empty()).then(|| Arc::<str>::from(hovered_uri));
    let cell_bytes = checked_wire_section(cell_count, CELL_WIRE_BYTES)?;
    let cells = decode_cells(reader.bytes(cell_bytes)?);
    let style_bytes = checked_wire_section(style_count, STYLE_WIRE_BYTES)?;
    let styles = decode_styles(reader.bytes(style_bytes)?)?;
    let offset_bytes = checked_wire_section(offset_count, U32_WIRE_BYTES)?;
    let grapheme_offsets = decode_u32s(reader.bytes(offset_bytes)?);
    let grapheme_bytes = Arc::from(reader.bytes(grapheme_len)?);
    let overlay_bytes = checked_wire_section(overlay_count, OVERLAY_WIRE_BYTES)?;
    let overlays = decode_overlays(reader.bytes(overlay_bytes)?);
    let kitty_placement_count = reader.count(MAX_KITTY_PLACEMENTS, "kitty placement")?;
    let kitty_placements = decode_kitty_placements(&mut reader, kitty_placement_count)?;
    if !reader.is_empty() {
        return invalid("terminal update has trailing bytes");
    }

    let viewport = TerminalViewport {
        generation,
        view_generation,
        dictionary_generation,
        columns,
        rows,
        foreground,
        background,
        presentation: Arc::new(TerminalPresentation::new(
            Arc::from(title),
            working_directory,
            hovered_uri,
        )),
        cells,
        dictionary: Arc::new(TerminalDictionary::from_shared(
            styles,
            grapheme_offsets,
            grapheme_bytes,
        )),
        overlays,
        kitty_placements,
        cursor,
        scrollbar,
        mode,
        search,
        unseen_output,
        kitty_keyboard,
        mouse_tracking,
        status,
    };
    validate_viewport(&viewport)?;
    Ok((pane, sequence, output_id, viewport))
}

fn validate_viewport(viewport: &TerminalViewport) -> Result<(), ProtocolError> {
    validate_working_directory(viewport.working_directory())?;
    validate_hovered_uri(viewport.hovered_uri())?;
    let expected = usize::from(viewport.columns)
        .checked_mul(usize::from(viewport.rows))
        .ok_or_else(|| ProtocolError::InvalidTerminal("grid dimensions overflow".to_owned()))?;
    if viewport.cells.len() != expected {
        return invalid("cell count does not match grid dimensions");
    }
    if viewport.styles().is_empty() || viewport.styles().len() > MAX_STYLE_COUNT {
        return invalid("invalid style dictionary length");
    }
    if viewport.grapheme_bytes().len() > MAX_GRAPHEME_BYTES {
        return invalid("grapheme arena exceeds limit");
    }
    if viewport.grapheme_offsets().len() > MAX_GRAPHEME_COUNT.saturating_add(1) {
        return invalid("grapheme dictionary exceeds limit");
    }
    if viewport.grapheme_offsets().first() != Some(&0)
        || viewport.grapheme_offsets().last().copied()
            != u32::try_from(viewport.grapheme_bytes().len()).ok()
    {
        return invalid("grapheme offsets do not cover the byte arena");
    }
    let mut previous = 0_usize;
    for offset in &viewport.grapheme_offsets()[1..] {
        let end = usize::try_from(*offset)
            .map_err(|_| ProtocolError::InvalidTerminal("invalid grapheme offset".to_owned()))?;
        if end < previous || end > viewport.grapheme_bytes().len() {
            return invalid("grapheme offsets are not monotonic");
        }
        std::str::from_utf8(&viewport.grapheme_bytes()[previous..end]).map_err(|_| {
            ProtocolError::InvalidTerminal("grapheme is not valid UTF-8".to_owned())
        })?;
        previous = end;
    }
    let grapheme_count = viewport.grapheme_offsets().len().saturating_sub(1);
    for cell in viewport.cells.iter() {
        if usize::from(cell.style_id()) >= viewport.styles().len() {
            return invalid("cell references a missing style");
        }
        let glyph = cell.glyph();
        if glyph & GRAPHEME_TABLE_BIT != 0 {
            if usize::try_from(glyph & !GRAPHEME_TABLE_BIT).map_or(true, |id| id >= grapheme_count)
            {
                return invalid("cell references a missing grapheme");
            }
        } else if glyph != 0 && char::from_u32(glyph).is_none() {
            return invalid("cell contains an invalid Unicode scalar");
        }
    }
    for style in viewport.styles() {
        if !valid_style(*style) {
            return invalid("style contains an invalid packed value");
        }
    }
    validate_kitty_placements(
        &viewport.kitty_placements,
        viewport.columns,
        viewport.rows,
        viewport.scrollbar,
    )?;
    validate_view_metadata(
        viewport.columns,
        viewport.rows,
        &viewport.overlays,
        viewport.cursor,
        viewport.scrollbar,
        viewport.mode,
        viewport.search,
    )
}

fn validate_patch(patch: &TerminalViewportPatch) -> Result<(), ProtocolError> {
    if patch.title().len() > MAX_TITLE_BYTES {
        return invalid("title exceeds terminal metadata limit");
    }
    validate_working_directory(patch.working_directory())?;
    validate_hovered_uri(patch.hovered_uri())?;
    if patch.changed_rows.len() > usize::from(patch.rows) {
        return invalid("changed row count exceeds viewport height");
    }
    let appended_styles = patch.dictionary.appended_styles();
    let appended_grapheme_lengths = patch.dictionary.appended_grapheme_lengths();
    let appended_grapheme_bytes = patch.dictionary.appended_grapheme_bytes();
    let style_count = usize::try_from(patch.style_base)
        .ok()
        .and_then(|base| base.checked_add(appended_styles.len()))
        .filter(|count| *count <= MAX_STYLE_COUNT)
        .ok_or_else(|| {
            ProtocolError::InvalidTerminal(
                "terminal patch style dictionary exceeds limit".to_owned(),
            )
        })?;
    if appended_styles
        .iter()
        .copied()
        .any(|style| !valid_style(style))
    {
        return invalid("terminal patch style contains an invalid packed value");
    }
    let grapheme_count = usize::try_from(patch.grapheme_base)
        .ok()
        .and_then(|base| base.checked_add(appended_grapheme_lengths.len()))
        .filter(|count| *count <= MAX_GRAPHEME_COUNT)
        .ok_or_else(|| {
            ProtocolError::InvalidTerminal(
                "terminal patch grapheme dictionary exceeds limit".to_owned(),
            )
        })?;
    if appended_grapheme_bytes.len() > MAX_GRAPHEME_BYTES {
        return invalid("terminal patch grapheme arena exceeds limit");
    }
    let mut grapheme_cursor = 0_usize;
    for length in appended_grapheme_lengths {
        let end = grapheme_cursor
            .checked_add(usize::try_from(*length).map_err(|_| {
                ProtocolError::InvalidTerminal("grapheme length overflows usize".to_owned())
            })?)
            .ok_or_else(|| {
                ProtocolError::InvalidTerminal("grapheme lengths overflow usize".to_owned())
            })?;
        let bytes = appended_grapheme_bytes
            .get(grapheme_cursor..end)
            .ok_or_else(|| {
                ProtocolError::InvalidTerminal("grapheme lengths exceed appended arena".to_owned())
            })?;
        std::str::from_utf8(bytes).map_err(|_| {
            ProtocolError::InvalidTerminal("grapheme is not valid UTF-8".to_owned())
        })?;
        grapheme_cursor = end;
    }
    if grapheme_cursor != appended_grapheme_bytes.len() {
        return invalid("grapheme lengths do not cover appended arena");
    }

    let rows = usize::from(patch.rows);
    let shift = isize::from(patch.scroll);
    if shift != 0 && (rows == 0 || shift.unsigned_abs() >= rows) {
        return invalid("terminal row shift is outside the viewport");
    }

    let columns = usize::from(patch.columns);
    let changed_row_indices = patch.changed_rows.row_indices();
    let changed_cells = patch.changed_rows.cells();
    if changed_row_indices
        .len()
        .checked_mul(columns)
        .is_none_or(|expected| expected != changed_cells.len())
    {
        return invalid("terminal patch flat cell plane does not match its rows");
    }
    let mut previous_row = None;
    for row in changed_row_indices.iter().copied() {
        let index = usize::from(row);
        if index >= rows || previous_row.is_some_and(|previous| previous >= row) {
            return invalid("terminal patch contains an invalid, duplicate, or out-of-order row");
        }
        previous_row = Some(row);
    }
    for cell in changed_cells {
        if usize::from(cell.style_id()) >= style_count {
            return invalid("terminal patch cell references a missing style");
        }
        let glyph = cell.glyph();
        if glyph & GRAPHEME_TABLE_BIT != 0 {
            if usize::try_from(glyph & !GRAPHEME_TABLE_BIT).map_or(true, |id| id >= grapheme_count)
            {
                return invalid("terminal patch cell references a missing grapheme");
            }
        } else if glyph != 0 && char::from_u32(glyph).is_none() {
            return invalid("terminal patch contains an invalid Unicode scalar");
        }
    }
    if shift > 0 {
        let exposed = usize::try_from(shift).map_err(|_| {
            ProtocolError::InvalidTerminal("terminal row shift overflows usize".to_owned())
        })?;
        if changed_row_indices.len() < exposed
            || changed_row_indices[..exposed]
                .iter()
                .enumerate()
                .any(|(row, changed)| usize::from(*changed) != row)
        {
            return invalid("terminal patch does not replace newly exposed rows");
        }
    } else if shift < 0 {
        let exposed = shift.unsigned_abs();
        if changed_row_indices.len() < exposed
            || changed_row_indices[changed_row_indices.len() - exposed..]
                .iter()
                .enumerate()
                .any(|(offset, changed)| usize::from(*changed) != rows - exposed + offset)
        {
            return invalid("terminal patch does not replace newly exposed rows");
        }
    }

    validate_kitty_placements(
        &patch.kitty_placements,
        patch.columns,
        patch.rows,
        patch.scrollbar,
    )?;

    validate_view_metadata(
        patch.columns,
        patch.rows,
        &patch.overlays,
        patch.cursor,
        patch.scrollbar,
        patch.mode,
        patch.search,
    )
}

fn validate_hovered_uri(uri: Option<&str>) -> Result<(), ProtocolError> {
    if uri.is_some_and(|uri| {
        uri.len() > MAX_HOVER_URI_BYTES
            || uri
                .chars()
                .any(|character| character.is_control() || character.is_whitespace())
    }) {
        return invalid("hovered URI is invalid");
    }
    Ok(())
}

fn validate_working_directory(working_directory: Option<&str>) -> Result<(), ProtocolError> {
    if working_directory.is_some_and(|working_directory| {
        working_directory.len() > MAX_WORKING_DIRECTORY_BYTES
            || working_directory.chars().any(char::is_control)
    }) {
        return invalid("working directory is invalid");
    }
    Ok(())
}

fn valid_style(style: PackedStyle) -> bool {
    style.foreground_raw() <= 0x00ff_ffff
        && style.background_raw() <= 0x00ff_ffff
        && (style.underline_color_raw() <= 0x00ff_ffff || style.underline_color_raw() == NO_COLOR)
        && style.underline_kind_raw() <= 5
}

fn validate_kitty_placements(
    placements: &[KittyPlacement],
    columns: u16,
    rows: u16,
    scrollbar: ScrollbarState,
) -> Result<(), ProtocolError> {
    if placements.len() > MAX_KITTY_PLACEMENTS {
        return invalid("kitty placement count exceeds its wire limit");
    }
    for placement in placements {
        if placement.image_id == 0
            || placement.image_generation == 0
            || placement.grid_cols == 0
            || placement.grid_rows == 0
            || placement.pixel_width == 0
            || placement.pixel_height == 0
        {
            return invalid("kitty placement has empty image or geometry metadata");
        }
        let right = i64::from(placement.viewport_col)
            .checked_add(i64::from(placement.grid_cols))
            .ok_or_else(|| {
                ProtocolError::InvalidTerminal("kitty placement column range overflows".to_owned())
            })?;
        let bottom = i64::from(placement.viewport_row)
            .checked_add(i64::from(placement.grid_rows))
            .ok_or_else(|| {
                ProtocolError::InvalidTerminal("kitty placement row range overflows".to_owned())
            })?;
        if right <= 0
            || i64::from(placement.viewport_col) >= i64::from(columns)
            || bottom <= 0
            || i64::from(placement.viewport_row) >= i64::from(rows)
        {
            return invalid("kitty placement is outside the viewport");
        }
        let expected_absolute = if placement.viewport_row < 0 {
            u64::from(scrollbar.offset)
                .saturating_sub(u64::from(placement.viewport_row.unsigned_abs()))
        } else {
            u64::from(scrollbar.offset).saturating_add(
                u64::try_from(placement.viewport_row).map_err(|_| {
                    ProtocolError::InvalidTerminal(
                        "kitty placement row is not representable".to_owned(),
                    )
                })?,
            )
        };
        if placement.absolute_row != expected_absolute {
            return invalid("kitty placement absolute row is inconsistent");
        }
        if let Some((x, y, width, height)) = placement.source_rect
            && (width == 0
                || height == 0
                || x.checked_add(width).is_none()
                || y.checked_add(height).is_none())
        {
            return invalid("kitty placement source rectangle is invalid");
        }
    }
    Ok(())
}

fn validate_view_metadata(
    columns: u16,
    rows: u16,
    overlays: &[OverlaySpan],
    cursor: Option<Cursor>,
    scrollbar: ScrollbarState,
    mode: TerminalMode,
    search: Option<SearchStatus>,
) -> Result<(), ProtocolError> {
    for overlay in overlays {
        if overlay.row >= rows || overlay.start > overlay.end || overlay.end > columns {
            return invalid("overlay span is outside the viewport");
        }
        if overlay.kind_and_flags() & 0xff > OverlayKind::CopyCursor as u16 {
            return invalid("overlay span has an unknown kind");
        }
    }
    if cursor.is_some_and(|cursor| cursor.row() >= rows || cursor.column() >= columns) {
        return invalid("cursor is outside the viewport");
    }
    if scrollbar.offset > scrollbar.total
        || scrollbar.len > scrollbar.total
        || scrollbar.offset.saturating_add(scrollbar.len) > scrollbar.total
    {
        return invalid("scrollbar range is inconsistent");
    }
    if let TerminalMode::Copy {
        position, total, ..
    }
    | TerminalMode::View { position, total } = mode
        && (total == 0 || position == 0 || position > total)
    {
        return invalid("copy-mode position is inconsistent");
    }
    if search.is_some_and(|search| search.current() > search.total) {
        return invalid("search status is inconsistent");
    }
    Ok(())
}

fn viewport_payload_capacity(
    viewport: &TerminalViewport,
    title_len: usize,
    working_directory_len: usize,
    hovered_uri_len: usize,
) -> Result<usize, ProtocolError> {
    checked_wire_capacity(&[
        VIEWPORT_FIXED_BYTES,
        encoded_mode_len(viewport.mode),
        encoded_search_len(viewport.search),
        encoded_cursor_len(viewport.cursor),
        encoded_status_len(&viewport.status)?,
        checked_wire_section(viewport.cells.len(), CELL_WIRE_BYTES)?,
        checked_wire_section(viewport.styles().len(), STYLE_WIRE_BYTES)?,
        checked_wire_section(viewport.grapheme_offsets().len(), U32_WIRE_BYTES)?,
        viewport.grapheme_bytes().len(),
        checked_wire_section(viewport.overlays.len(), OVERLAY_WIRE_BYTES)?,
        U32_WIRE_BYTES,
        checked_wire_section(viewport.kitty_placements.len(), KITTY_PLACEMENT_WIRE_BYTES)?,
        title_len,
        working_directory_len,
        hovered_uri_len,
    ])
}

fn patch_payload_capacity(
    patch: &TerminalViewportPatch,
    title_len: usize,
    working_directory_len: usize,
    hovered_uri_len: usize,
) -> Result<usize, ProtocolError> {
    checked_wire_capacity(&[
        PATCH_FIXED_BYTES,
        encoded_mode_len(patch.mode),
        encoded_search_len(patch.search),
        encoded_cursor_len(patch.cursor),
        encoded_status_len(&patch.status)?,
        checked_wire_section(patch.changed_rows.len(), ROW_RECORD_WIRE_BYTES)?,
        checked_wire_section(patch.changed_rows.cells().len(), CELL_WIRE_BYTES)?,
        checked_wire_section(patch.dictionary.appended_styles().len(), STYLE_WIRE_BYTES)?,
        checked_wire_section(
            patch.dictionary.appended_grapheme_lengths().len(),
            U32_WIRE_BYTES,
        )?,
        patch.dictionary.appended_grapheme_bytes().len(),
        checked_wire_section(patch.overlays.len(), OVERLAY_WIRE_BYTES)?,
        U32_WIRE_BYTES,
        checked_wire_section(patch.kitty_placements.len(), KITTY_PLACEMENT_WIRE_BYTES)?,
        title_len,
        working_directory_len,
        hovered_uri_len,
    ])
}

const fn encoded_mode_len(mode: TerminalMode) -> usize {
    match mode {
        TerminalMode::Live => 1,
        TerminalMode::Copy { .. } => 18,
        TerminalMode::View { .. } => 17,
    }
}

const fn encoded_search_len(search: Option<SearchStatus>) -> usize {
    if search.is_some() { 10 } else { 1 }
}

const fn encoded_cursor_len(cursor: Option<Cursor>) -> usize {
    if cursor.is_some() { 11 } else { 1 }
}

fn encoded_status_len(status: &SessionStatus) -> Result<usize, ProtocolError> {
    match status {
        SessionStatus::Starting | SessionStatus::Running => Ok(1),
        SessionStatus::Exited(exit) => match exit.signal.as_deref() {
            Some(signal) => checked_wire_capacity(&[10, checked_status_string_len(signal)?]),
            None => Ok(6),
        },
        SessionStatus::Failed(error) => {
            checked_wire_capacity(&[5, checked_status_string_len(error)?])
        }
    }
}

fn checked_status_string_len(value: &str) -> Result<usize, ProtocolError> {
    if value.len() > MAX_STATUS_BYTES {
        return invalid("terminal status string exceeds limit");
    }
    Ok(value.len())
}

fn checked_wire_section(count: usize, width: usize) -> Result<usize, ProtocolError> {
    count
        .checked_mul(width)
        .ok_or(ProtocolError::FrameTooLarge(usize::MAX))
}

fn checked_wire_capacity(parts: &[usize]) -> Result<usize, ProtocolError> {
    parts.iter().try_fold(0_usize, |total, part| {
        total
            .checked_add(*part)
            .ok_or(ProtocolError::FrameTooLarge(usize::MAX))
    })
}

fn expect_exact_remaining(
    reader: &WireReader<'_>,
    expected: usize,
    trailing_message: &str,
) -> Result<(), ProtocolError> {
    if reader.remaining_len() < expected {
        return Err(ProtocolError::Truncated);
    }
    if reader.remaining_len() > expected {
        return invalid(trailing_message);
    }
    Ok(())
}

fn encode_kitty_placements(output: &mut Vec<u8>, placements: &[KittyPlacement]) {
    for placement in placements {
        push_u32(output, placement.image_id);
        push_u64(output, placement.image_generation);
        output.push(placement.layer as u8);
        output.push(u8::from(placement.source_rect.is_some()));
        push_u16(output, 0);
        push_i32(output, placement.viewport_col);
        push_i32(output, placement.viewport_row);
        push_u64(output, placement.absolute_row);
        push_u32(output, placement.cell_offset_x);
        push_u32(output, placement.cell_offset_y);
        push_u32(output, placement.grid_cols);
        push_u32(output, placement.grid_rows);
        push_u32(output, placement.pixel_width);
        push_u32(output, placement.pixel_height);
        let (x, y, width, height) = placement.source_rect.unwrap_or_default();
        push_u32(output, x);
        push_u32(output, y);
        push_u32(output, width);
        push_u32(output, height);
    }
}

fn decode_kitty_placements(
    reader: &mut WireReader<'_>,
    count: usize,
) -> Result<Arc<[KittyPlacement]>, ProtocolError> {
    let mut placements = Vec::with_capacity(count);
    for _ in 0..count {
        let image_id = reader.u32()?;
        let image_generation = reader.u64()?;
        let layer = match reader.u8()? {
            0 => KittyLayer::BelowBg,
            1 => KittyLayer::BelowText,
            2 => KittyLayer::AboveText,
            _ => return invalid("kitty placement has an unknown paint layer"),
        };
        let has_source_rect = decode_bool(reader.u8()?)?;
        if reader.u16()? != 0 {
            return invalid("kitty placement reserved field is nonzero");
        }
        let viewport_col = reader.i32()?;
        let viewport_row = reader.i32()?;
        let absolute_row = reader.u64()?;
        let cell_offset_x = reader.u32()?;
        let cell_offset_y = reader.u32()?;
        let grid_cols = reader.u32()?;
        let grid_rows = reader.u32()?;
        let pixel_width = reader.u32()?;
        let pixel_height = reader.u32()?;
        let source = (reader.u32()?, reader.u32()?, reader.u32()?, reader.u32()?);
        if !has_source_rect && source != (0, 0, 0, 0) {
            return invalid("kitty placement empty source rectangle is nonzero");
        }
        placements.push(KittyPlacement {
            image_id,
            image_generation,
            layer,
            viewport_col,
            viewport_row,
            absolute_row,
            cell_offset_x,
            cell_offset_y,
            grid_cols,
            grid_rows,
            pixel_width,
            pixel_height,
            source_rect: has_source_rect.then_some(source),
        });
    }
    Ok(placements.into())
}

fn decode_cells(bytes: &[u8]) -> Arc<[PackedCell]> {
    bytes
        .chunks_exact(CELL_WIRE_BYTES)
        .map(|chunk| {
            PackedCell::from_raw(
                wire_u32_at(chunk, 0),
                wire_u16_at(chunk, 4),
                wire_u16_at(chunk, 6),
            )
        })
        .collect()
}

fn decode_styles(bytes: &[u8]) -> Result<Arc<[PackedStyle]>, ProtocolError> {
    if bytes
        .chunks_exact(STYLE_WIRE_BYTES)
        .any(|chunk| chunk[15] != 0)
    {
        return invalid("packed style reserved byte is nonzero");
    }
    Ok(bytes
        .chunks_exact(STYLE_WIRE_BYTES)
        .map(|chunk| {
            PackedStyle::from_raw(
                wire_u32_at(chunk, 0),
                wire_u32_at(chunk, 4),
                wire_u32_at(chunk, 8),
                wire_u16_at(chunk, 12),
                chunk[14],
            )
        })
        .collect())
}

fn decode_u32s(bytes: &[u8]) -> Arc<[u32]> {
    bytes
        .chunks_exact(U32_WIRE_BYTES)
        .map(|chunk| wire_u32_at(chunk, 0))
        .collect()
}

fn decode_overlays(bytes: &[u8]) -> Arc<[OverlaySpan]> {
    bytes
        .chunks_exact(OVERLAY_WIRE_BYTES)
        .map(|chunk| {
            OverlaySpan::from_raw(
                wire_u16_at(chunk, 0),
                wire_u16_at(chunk, 2),
                wire_u16_at(chunk, 4),
                wire_u16_at(chunk, 6),
            )
        })
        .collect()
}

#[inline]
fn wire_u16_at(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes(
        bytes[offset..offset + 2]
            .try_into()
            .expect("fixed-width wire chunk"),
    )
}

#[inline]
fn wire_u32_at(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(
        bytes[offset..offset + 4]
            .try_into()
            .expect("fixed-width wire chunk"),
    )
}

fn encode_mode(output: &mut Vec<u8>, mode: TerminalMode) {
    match mode {
        TerminalMode::Live => output.push(0),
        TerminalMode::Copy {
            position,
            total,
            hide_position,
        } => {
            output.push(1);
            push_u64(output, u64::from(position));
            push_u64(output, u64::from(total));
            output.push(u8::from(hide_position));
        }
        TerminalMode::View { position, total } => {
            output.push(2);
            push_u64(output, u64::from(position));
            push_u64(output, u64::from(total));
        }
    }
}

fn decode_mode(reader: &mut WireReader<'_>) -> Result<TerminalMode, ProtocolError> {
    match reader.u8()? {
        0 => Ok(TerminalMode::Live),
        1 => Ok(TerminalMode::Copy {
            position: compact_u32(reader, "copy-mode position")?,
            total: compact_u32(reader, "copy-mode total")?,
            hide_position: decode_bool(reader.u8()?)?,
        }),
        2 => Ok(TerminalMode::View {
            position: compact_u32(reader, "view-mode position")?,
            total: compact_u32(reader, "view-mode total")?,
        }),
        _ => invalid("unknown terminal interaction mode"),
    }
}

fn encode_search(output: &mut Vec<u8>, search: Option<SearchStatus>) {
    let Some(search) = search else {
        output.push(0);
        return;
    };
    output.push(1);
    push_u32(output, search.current());
    push_u32(output, search.total);
    output.push(u8::from(search.pending()) | (u8::from(search.invalid_pattern()) << 1));
}

fn decode_search(reader: &mut WireReader<'_>) -> Result<Option<SearchStatus>, ProtocolError> {
    match reader.u8()? {
        0 => Ok(None),
        1 => {
            let current = reader.u32()?;
            let total = reader.u32()?;
            let flags = reader.u8()?;
            if flags & !0b11 != 0 {
                return invalid("unknown search status flags");
            }
            Ok(Some(
                SearchStatus::new(current, total)
                    .with_pending(flags & 1 != 0)
                    .with_invalid_pattern(flags & 2 != 0),
            ))
        }
        _ => invalid("invalid search status presence flag"),
    }
}

fn encode_cursor(output: &mut Vec<u8>, cursor: Option<Cursor>) {
    let Some(cursor) = cursor else {
        output.push(0);
        return;
    };
    output.push(1);
    push_u16(output, cursor.column());
    push_u16(output, cursor.row());
    let flags = u8::from(cursor.visible())
        | u8::from(cursor.blinking()) << 1
        | u8::from(cursor.at_wide_tail()) << 2;
    output.push(flags);
    output.push(match cursor.style() {
        CursorStyle::Bar => 0,
        CursorStyle::Block => 1,
        CursorStyle::Underline => 2,
        CursorStyle::BlockHollow => 3,
    });
    push_u32(output, cursor.color().packed());
}

fn decode_cursor(reader: &mut WireReader<'_>) -> Result<Option<Cursor>, ProtocolError> {
    match reader.u8()? {
        0 => Ok(None),
        1 => {
            let column = reader.u16()?;
            let row = reader.u16()?;
            let flags = reader.u8()?;
            if flags & !0b111 != 0 {
                return invalid("cursor flags contain reserved bits");
            }
            let style = match reader.u8()? {
                0 => CursorStyle::Bar,
                1 => CursorStyle::Block,
                2 => CursorStyle::Underline,
                3 => CursorStyle::BlockHollow,
                _ => return invalid("unknown cursor style"),
            };
            Ok(Some(Cursor::new(
                column,
                row,
                flags & 1 != 0,
                flags & 2 != 0,
                flags & 4 != 0,
                style,
                decode_color(reader.u32()?)?,
            )))
        }
        _ => invalid("invalid cursor presence flag"),
    }
}

fn encode_status(output: &mut Vec<u8>, status: &SessionStatus) -> Result<(), ProtocolError> {
    match status {
        SessionStatus::Starting => output.push(0),
        SessionStatus::Running => output.push(1),
        SessionStatus::Exited(exit) => {
            output.push(2);
            push_u32(output, exit.code);
            encode_optional_string(output, exit.signal.as_deref())?;
        }
        SessionStatus::Failed(error) => {
            output.push(3);
            encode_string(output, error)?;
        }
    }
    Ok(())
}

fn decode_status(reader: &mut WireReader<'_>) -> Result<SessionStatus, ProtocolError> {
    match reader.u8()? {
        0 => Ok(SessionStatus::Starting),
        1 => Ok(SessionStatus::Running),
        2 => {
            let code = reader.u32()?;
            let signal = decode_optional_string(reader)?;
            Ok(SessionStatus::exited(code, signal))
        }
        3 => Ok(SessionStatus::failed(decode_string(reader)?)),
        _ => invalid("unknown terminal status"),
    }
}

fn preflight_status(reader: &mut WireReader<'_>) -> Result<(), ProtocolError> {
    match reader.u8()? {
        0 | 1 => Ok(()),
        2 => {
            reader.u32()?;
            match reader.u8()? {
                0 => Ok(()),
                1 => preflight_string(reader),
                _ => invalid("invalid optional string flag"),
            }
        }
        3 => preflight_string(reader),
        _ => invalid("unknown terminal status"),
    }
}

fn preflight_string(reader: &mut WireReader<'_>) -> Result<(), ProtocolError> {
    let len = reader.count(MAX_STATUS_BYTES, "status string")?;
    reader.bytes(len)?;
    Ok(())
}

fn encode_optional_string(output: &mut Vec<u8>, value: Option<&str>) -> Result<(), ProtocolError> {
    if let Some(value) = value {
        output.push(1);
        encode_string(output, value)
    } else {
        output.push(0);
        Ok(())
    }
}

fn decode_optional_string(reader: &mut WireReader<'_>) -> Result<Option<String>, ProtocolError> {
    match reader.u8()? {
        0 => Ok(None),
        1 => decode_string(reader).map(Some),
        _ => invalid("invalid optional string flag"),
    }
}

fn encode_string(output: &mut Vec<u8>, value: &str) -> Result<(), ProtocolError> {
    let len = checked_status_string_len(value)?;
    push_u32(output, checked_count(len)?);
    output.extend_from_slice(value.as_bytes());
    Ok(())
}

fn decode_string(reader: &mut WireReader<'_>) -> Result<String, ProtocolError> {
    let len = reader.count(MAX_STATUS_BYTES, "status string")?;
    Ok(reader.utf8(len, "status string")?.to_owned())
}

fn decode_color(value: u32) -> Result<Color, ProtocolError> {
    if value > 0x00ff_ffff {
        return invalid("packed color exceeds 24 bits");
    }
    Ok(Color::from_packed(value))
}

fn decode_bool(value: u8) -> Result<bool, ProtocolError> {
    match value {
        0 => Ok(false),
        1 => Ok(true),
        _ => invalid("invalid boolean value"),
    }
}

fn compact_u32(reader: &mut WireReader<'_>, name: &str) -> Result<u32, ProtocolError> {
    u32::try_from(reader.u64()?)
        .map_err(|_| ProtocolError::InvalidTerminal(format!("{name} exceeds u32")))
}

fn checked_count(value: usize) -> Result<u32, ProtocolError> {
    u32::try_from(value)
        .map_err(|_| ProtocolError::InvalidTerminal("section count exceeds u32".to_owned()))
}

fn invalid<T>(message: &str) -> Result<T, ProtocolError> {
    Err(ProtocolError::InvalidTerminal(message.to_owned()))
}

fn push_u16(output: &mut Vec<u8>, value: u16) {
    output.extend_from_slice(&value.to_le_bytes());
}

fn push_u32(output: &mut Vec<u8>, value: u32) {
    output.extend_from_slice(&value.to_le_bytes());
}

fn push_i32(output: &mut Vec<u8>, value: i32) {
    output.extend_from_slice(&value.to_le_bytes());
}

fn push_u64(output: &mut Vec<u8>, value: u64) {
    output.extend_from_slice(&value.to_le_bytes());
}

#[derive(Clone, Copy)]
struct WireReader<'a> {
    remaining: &'a [u8],
}

impl<'a> WireReader<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { remaining: bytes }
    }

    const fn is_empty(&self) -> bool {
        self.remaining.is_empty()
    }

    const fn remaining_len(&self) -> usize {
        self.remaining.len()
    }

    fn bytes(&mut self, len: usize) -> Result<&'a [u8], ProtocolError> {
        let Some((head, tail)) = self.remaining.split_at_checked(len) else {
            return Err(ProtocolError::Truncated);
        };
        self.remaining = tail;
        Ok(head)
    }

    fn u8(&mut self) -> Result<u8, ProtocolError> {
        Ok(self.bytes(1)?[0])
    }

    fn u16(&mut self) -> Result<u16, ProtocolError> {
        Ok(u16::from_le_bytes(
            self.bytes(2)?
                .try_into()
                .map_err(|_| ProtocolError::Truncated)?,
        ))
    }

    fn i16(&mut self) -> Result<i16, ProtocolError> {
        Ok(i16::from_le_bytes(
            self.bytes(2)?
                .try_into()
                .map_err(|_| ProtocolError::Truncated)?,
        ))
    }

    fn i32(&mut self) -> Result<i32, ProtocolError> {
        Ok(i32::from_le_bytes(
            self.bytes(4)?
                .try_into()
                .map_err(|_| ProtocolError::Truncated)?,
        ))
    }

    fn u32(&mut self) -> Result<u32, ProtocolError> {
        Ok(u32::from_le_bytes(
            self.bytes(4)?
                .try_into()
                .map_err(|_| ProtocolError::Truncated)?,
        ))
    }

    fn u64(&mut self) -> Result<u64, ProtocolError> {
        Ok(u64::from_le_bytes(
            self.bytes(8)?
                .try_into()
                .map_err(|_| ProtocolError::Truncated)?,
        ))
    }

    fn count(&mut self, max: usize, name: &str) -> Result<usize, ProtocolError> {
        let value = usize::try_from(self.u32()?)
            .map_err(|_| ProtocolError::InvalidTerminal(format!("{name} count overflows usize")))?;
        if value > max {
            return Err(ProtocolError::InvalidTerminal(format!(
                "{name} count {value} exceeds limit {max}"
            )));
        }
        Ok(value)
    }

    fn utf8(&mut self, len: usize, name: &str) -> Result<&'a str, ProtocolError> {
        std::str::from_utf8(self.bytes(len)?)
            .map_err(|_| ProtocolError::InvalidTerminal(format!("{name} is not valid UTF-8")))
    }
}

#[cfg(test)]
mod tests {
    use crate::message::MAX_MUX_OPTION_VALUE_BYTES;
    use crate::{
        MAX_AGENT_PERMISSION_BYTES, MAX_AGENT_STATE_BLOB_BYTES, MuxOptionKey, MuxOptionSource,
        MuxOptions,
    };
    use zz_terminal::{
        AppearanceConfigKey, AppearanceProvenance, AppearanceSource, CellWidth, CursorStyle,
        KittyLayer, KittyPlacement, OverlayKind, SearchDirection, TerminalAppearance,
        UnderlineStyle,
    };

    use super::*;

    #[test]
    fn control_frame_flavor_stops_before_exceeding_its_limit() {
        let mut output = vec![1, 2];
        let mut flavor = FrameFlavor {
            output: &mut output,
            limit: 4,
        };
        postcard::ser_flavors::Flavor::try_extend(&mut flavor, &[3, 4])
            .expect("bytes through the limit");
        assert_eq!(
            postcard::ser_flavors::Flavor::try_push(&mut flavor, 5),
            Err(postcard::Error::SerializeBufferFull)
        );
        assert_eq!(output, [1, 2, 3, 4]);
    }

    #[test]
    fn server_hello_round_trips_the_validated_terminal_appearance() {
        let mut appearance = TerminalAppearance {
            font_families: vec!["Fixture Mono".to_owned(), "Fixture Emoji".to_owned()],
            font_size_points: 12.5,
            padding_left: 7.0,
            padding_right: 8.0,
            cursor_style: CursorStyle::Underline,
            ..TerminalAppearance::default()
        };
        appearance.palette[42] = Color::rgb(0x12, 0x34, 0x56);
        let mut appearance_provenance = AppearanceProvenance::default();
        appearance_provenance
            .set_source(AppearanceConfigKey::Background, AppearanceSource::Ghostty);
        let message = ProtocolMessage::ServerHello(crate::ServerHello {
            protocol_version: crate::PROTOCOL_VERSION,
            server_id: 7,
            client_id: crate::ClientId(11),
            client_instance_id: crate::ClientInstanceId(13),
            capabilities: vec!["terminal-appearance-v1".to_owned()],
            appearance,
            appearance_provenance,
            mux_options: MuxOptions::default(),
            status: crate::StatusLine::default(),
            key_tables: Vec::new(),
        });

        let frame = encode_protocol_message(&message).expect("encode ServerHello");
        assert_eq!(frame[4], Lane::Control as u8);
        assert_eq!(
            decode_protocol_frame(&frame).expect("decode ServerHello"),
            message
        );
    }

    #[test]
    fn appearance_change_round_trips_and_rejects_invalid_values() {
        let mut appearance = TerminalAppearance {
            color_scheme: zz_terminal::TerminalColorScheme::Light,
            background: Color::rgb(0xf0, 0xf1, 0xf2),
            ..TerminalAppearance::default()
        };
        let message = ProtocolMessage::Event(Event {
            sequence: 91,
            payload: EventPayload::AppearanceChanged {
                appearance: Box::new(appearance.clone()),
                provenance: AppearanceProvenance::default(),
            },
        });
        let frame = encode_protocol_message(&message).expect("encode appearance update");
        assert_eq!(frame[4], Lane::Control as u8);
        assert_eq!(decode_protocol_frame(&frame).unwrap(), message);

        appearance.font_size_points = f32::NAN;
        let invalid = ProtocolMessage::Event(Event {
            sequence: 92,
            payload: EventPayload::AppearanceChanged {
                appearance: Box::new(appearance),
                provenance: AppearanceProvenance::default(),
            },
        });
        assert!(matches!(
            encode_protocol_message(&invalid),
            Err(ProtocolError::InvalidAppearance(_))
        ));
    }

    #[test]
    fn config_overrides_round_trip_in_order_with_repeated_keys() {
        let message = ProtocolMessage::SetConfigOverrides {
            entries: vec![
                ("theme".to_owned(), "Fixture".to_owned()),
                ("palette".to_owned(), "1=#112233".to_owned()),
                ("palette".to_owned(), "2=#445566".to_owned()),
                ("font-family".to_owned(), "Fixture Mono".to_owned()),
            ],
        };

        let frame = encode_protocol_message(&message).expect("encode configuration overrides");
        assert_eq!(frame[4], Lane::Control as u8);
        assert_eq!(decode_protocol_frame(&frame).unwrap(), message);
    }

    #[test]
    fn startup_config_causes_enforce_count_item_and_aggregate_bounds() {
        assert_eq!(MAX_STARTUP_CONFIG_CAUSES, 1024);
        assert_eq!(MAX_STARTUP_CONFIG_CAUSE_BYTES, 64 * 1024);
        assert_eq!(MAX_STARTUP_CONFIG_CAUSES_BYTES, 1024 * 1024);

        let message = |causes| {
            ProtocolMessage::Event(Event {
                sequence: 1,
                payload: EventPayload::StartupConfigCauses { causes },
            })
        };
        let rejected = |causes| {
            let invalid = message(causes);
            assert!(matches!(
                encode_protocol_message(&invalid),
                Err(ProtocolError::InvalidStartupConfigCauses(_))
            ));
            let payload = postcard::to_stdvec(&invalid).expect("serialize invalid fixture");
            let frame = crate::framing::encode_enveloped(Lane::Control, &payload)
                .expect("envelope invalid fixture");
            assert!(matches!(
                decode_protocol_frame(&frame),
                Err(ProtocolError::Decode(_))
            ));
        };

        rejected(vec![String::new(); MAX_STARTUP_CONFIG_CAUSES + 1]);
        rejected(vec!["x".repeat(MAX_STARTUP_CONFIG_CAUSE_BYTES + 1)]);
        rejected(vec![
            "x".repeat(MAX_STARTUP_CONFIG_CAUSE_BYTES);
            MAX_STARTUP_CONFIG_CAUSES_BYTES / MAX_STARTUP_CONFIG_CAUSE_BYTES + 1
        ]);

        let count_boundary = message(vec![String::new(); MAX_STARTUP_CONFIG_CAUSES]);
        let frame = encode_protocol_message(&count_boundary).expect("encode count boundary");
        assert_eq!(
            decode_protocol_frame(&frame).expect("decode count boundary"),
            count_boundary
        );

        let aggregate_boundary = message(vec![
            "x".repeat(MAX_STARTUP_CONFIG_CAUSE_BYTES);
            MAX_STARTUP_CONFIG_CAUSES_BYTES / MAX_STARTUP_CONFIG_CAUSE_BYTES
        ]);
        let frame = encode_protocol_message(&aggregate_boundary).expect("encode aggregate boundary");
        assert_eq!(
            decode_protocol_frame(&frame).expect("decode aggregate boundary"),
            aggregate_boundary
        );
    }

    #[test]
    fn mux_options_round_trip_in_current_hello_and_change_event() {
        let mut options = MuxOptions::default();
        for (index, key) in MuxOptionKey::ALL.into_iter().enumerate() {
            let source = [
                MuxOptionSource::Default,
                MuxOptionSource::TmuxConfig,
                MuxOptionSource::Override,
                MuxOptionSource::RuntimeCommand,
            ][index % 4];
            options.set(key, format!("fixture-{index}"), source);
        }
        let hello = ProtocolMessage::ServerHello(crate::ServerHello {
            protocol_version: PROTOCOL_VERSION,
            server_id: 7,
            client_id: crate::ClientId(11),
            client_instance_id: crate::ClientInstanceId(13),
            capabilities: vec!["config-overrides-v1".to_owned()],
            appearance: TerminalAppearance::default(),
            appearance_provenance: AppearanceProvenance::default(),
            mux_options: options.clone(),
            status: crate::StatusLine::default(),
            key_tables: Vec::new(),
        });
        let hello_frame = encode_protocol_message(&hello).expect("encode mux options in hello");
        assert_eq!(decode_protocol_frame(&hello_frame).unwrap(), hello);

        let changed = ProtocolMessage::Event(Event {
            sequence: 93,
            payload: EventPayload::MuxOptionsChanged { options },
        });
        let changed_frame = encode_protocol_message(&changed).expect("encode mux options event");
        assert_eq!(decode_protocol_frame(&changed_frame).unwrap(), changed);
    }

    #[test]
    fn status_payloads_round_trip_and_reject_oversized_text() {
        let status = crate::StatusLine {
            left: "[work] 1:frontend".to_owned(),
            right: "batt 82% · 09:41".to_owned(),
            ..crate::StatusLine::default()
        };
        let changed = ProtocolMessage::Event(Event {
            sequence: 97,
            payload: EventPayload::StatusChanged {
                status: status.clone(),
            },
        });
        let frame = encode_protocol_message(&changed).expect("encode status event");
        assert_eq!(decode_protocol_frame(&frame).unwrap(), changed);

        let oversized = crate::ServerHello {
            protocol_version: PROTOCOL_VERSION,
            server_id: 7,
            client_id: crate::ClientId(11),
            client_instance_id: crate::ClientInstanceId(13),
            capabilities: Vec::new(),
            appearance: TerminalAppearance::default(),
            appearance_provenance: AppearanceProvenance::default(),
            mux_options: MuxOptions::default(),
            status: crate::StatusLine {
                left: "x".repeat(crate::MAX_STATUS_TEXT_BYTES + 1),
                ..crate::StatusLine::default()
            },
            key_tables: Vec::new(),
        };
        assert!(matches!(
            encode_protocol_message(&ProtocolMessage::ServerHello(oversized)),
            Err(ProtocolError::InvalidServerHello(_))
        ));
    }

    #[test]
    fn mux_option_payloads_reject_missing_keys_and_oversized_values() {
        let incomplete =
            MuxOptions::from_entries(MuxOptionKey::ALL[..MuxOptionKey::ALL.len() - 1].iter().map(
                |key| {
                    let value = MuxOptions::default().get(*key).unwrap().clone();
                    (*key, value)
                },
            ));
        let incomplete_event = ProtocolMessage::Event(Event {
            sequence: 94,
            payload: EventPayload::MuxOptionsChanged {
                options: incomplete,
            },
        });
        assert!(matches!(
            encode_protocol_message(&incomplete_event),
            Err(ProtocolError::InvalidConfigOverrides(_))
        ));

        let mut oversized = MuxOptions::default();
        oversized.set(
            MuxOptionKey::Prefix,
            "x".repeat(MAX_MUX_OPTION_VALUE_BYTES + 1),
            MuxOptionSource::RuntimeCommand,
        );
        let oversized_hello = ProtocolMessage::ServerHello(crate::ServerHello {
            protocol_version: PROTOCOL_VERSION,
            server_id: 7,
            client_id: crate::ClientId(11),
            client_instance_id: crate::ClientInstanceId(13),
            capabilities: Vec::new(),
            appearance: TerminalAppearance::default(),
            appearance_provenance: AppearanceProvenance::default(),
            mux_options: oversized,
            status: crate::StatusLine::default(),
            key_tables: Vec::new(),
        });
        assert!(matches!(
            encode_protocol_message(&oversized_hello),
            Err(ProtocolError::InvalidServerHello(_))
        ));
        let payload = postcard::to_stdvec(&oversized_hello).expect("serialize oversized fixture");
        let frame = crate::framing::encode_enveloped(Lane::Control, &payload)
            .expect("envelope oversized fixture");
        assert!(matches!(
            decode_protocol_frame(&frame),
            Err(ProtocolError::Decode(_))
        ));
    }

    #[test]
    fn malformed_server_hello_appearance_is_rejected_on_encode_and_decode() {
        let appearance = TerminalAppearance {
            font_size_points: f32::NAN,
            ..TerminalAppearance::default()
        };
        let message = ProtocolMessage::ServerHello(crate::ServerHello {
            protocol_version: crate::PROTOCOL_VERSION,
            server_id: 7,
            client_id: crate::ClientId(11),
            client_instance_id: crate::ClientInstanceId(13),
            capabilities: Vec::new(),
            appearance,
            appearance_provenance: AppearanceProvenance::default(),
            mux_options: MuxOptions::default(),
            status: crate::StatusLine::default(),
            key_tables: Vec::new(),
        });

        assert!(matches!(
            encode_protocol_message(&message),
            Err(ProtocolError::InvalidAppearance(_))
        ));
        let payload = postcard::to_stdvec(&message).expect("serialize malformed fixture");
        let frame = crate::framing::encode_enveloped(Lane::Control, &payload)
            .expect("envelope malformed fixture");
        assert!(matches!(
            decode_protocol_frame(&frame),
            Err(ProtocolError::InvalidAppearance(_))
        ));
    }

    #[test]
    fn server_hello_rejects_invalid_font_feature_tags() {
        let appearance = TerminalAppearance {
            font_features: vec![zz_terminal::FontFeature::new(*b"\0bad", 1)],
            ..TerminalAppearance::default()
        };
        let message = ProtocolMessage::ServerHello(crate::ServerHello {
            protocol_version: PROTOCOL_VERSION,
            server_id: 7,
            client_id: crate::ClientId(11),
            client_instance_id: crate::ClientInstanceId(13),
            capabilities: Vec::new(),
            appearance,
            appearance_provenance: AppearanceProvenance::default(),
            mux_options: MuxOptions::default(),
            status: crate::StatusLine::default(),
            key_tables: Vec::new(),
        });

        assert!(matches!(
            encode_protocol_message(&message),
            Err(ProtocolError::InvalidAppearance(_))
        ));
        let payload = postcard::to_stdvec(&message).expect("serialize malformed fixture");
        let frame = crate::framing::encode_enveloped(Lane::Control, &payload)
            .expect("envelope malformed fixture");
        assert!(matches!(
            decode_protocol_frame(&frame),
            Err(ProtocolError::InvalidAppearance(_))
        ));
    }

    #[test]
    fn server_hello_rejects_inner_version_mismatch() {
        let message = ProtocolMessage::ServerHello(crate::ServerHello {
            protocol_version: PROTOCOL_VERSION - 1,
            server_id: 7,
            client_id: crate::ClientId(11),
            client_instance_id: crate::ClientInstanceId(13),
            capabilities: Vec::new(),
            appearance: TerminalAppearance::default(),
            appearance_provenance: AppearanceProvenance::default(),
            mux_options: MuxOptions::default(),
            status: crate::StatusLine::default(),
            key_tables: Vec::new(),
        });

        assert!(matches!(
            encode_protocol_message(&message),
            Err(ProtocolError::VersionMismatch {
                expected: PROTOCOL_VERSION,
                received,
            }) if received == PROTOCOL_VERSION - 1
        ));
        let payload = postcard::to_stdvec(&message).expect("serialize malformed fixture");
        let frame = crate::framing::encode_enveloped(Lane::Control, &payload)
            .expect("envelope malformed fixture");
        assert!(matches!(
            decode_protocol_frame(&frame),
            Err(ProtocolError::VersionMismatch {
                expected: PROTOCOL_VERSION,
                received,
            }) if received == PROTOCOL_VERSION - 1
        ));
    }

    #[test]
    fn server_hello_rejects_appearance_vectors_before_materializing_them() {
        for appearance in [
            TerminalAppearance {
                font_families: vec![String::new(); 33],
                ..TerminalAppearance::default()
            },
            TerminalAppearance {
                font_families: vec!["x".repeat(257)],
                ..TerminalAppearance::default()
            },
            TerminalAppearance {
                font_features: vec![zz_terminal::FontFeature::new(*b"ss01", 1); 65],
                ..TerminalAppearance::default()
            },
        ] {
            let message = ProtocolMessage::ServerHello(crate::ServerHello {
                protocol_version: PROTOCOL_VERSION,
                server_id: 7,
                client_id: crate::ClientId(11),
                client_instance_id: crate::ClientInstanceId(13),
                capabilities: Vec::new(),
                appearance,
                appearance_provenance: AppearanceProvenance::default(),
                mux_options: MuxOptions::default(),
                status: crate::StatusLine::default(),
                key_tables: Vec::new(),
            });
            let payload = postcard::to_stdvec(&message).expect("serialize malformed fixture");
            let frame = crate::framing::encode_enveloped(Lane::Control, &payload)
                .expect("envelope malformed fixture");
            assert!(matches!(
                decode_protocol_frame(&frame),
                Err(ProtocolError::Decode(_))
            ));
        }
    }

    #[test]
    fn server_hello_rejects_capabilities_before_materializing_them() {
        for capabilities in [
            vec![String::new(); MAX_SERVER_CAPABILITIES + 1],
            vec!["x".repeat(MAX_SERVER_CAPABILITY_BYTES + 1)],
        ] {
            let message = ProtocolMessage::ServerHello(crate::ServerHello {
                protocol_version: PROTOCOL_VERSION,
                server_id: 7,
                client_id: crate::ClientId(11),
                client_instance_id: crate::ClientInstanceId(13),
                capabilities,
                appearance: TerminalAppearance::default(),
                appearance_provenance: AppearanceProvenance::default(),
                mux_options: MuxOptions::default(),
                status: crate::StatusLine::default(),
                key_tables: Vec::new(),
            });
            assert!(matches!(
                encode_protocol_message(&message),
                Err(ProtocolError::InvalidServerHello(_))
            ));

            let payload = postcard::to_stdvec(&message).expect("serialize malformed fixture");
            let frame = crate::framing::encode_enveloped(Lane::Control, &payload)
                .expect("envelope malformed fixture");
            assert!(matches!(
                decode_protocol_frame(&frame),
                Err(ProtocolError::Decode(_))
            ));
        }
    }

    #[test]
    fn client_hello_rejects_capabilities_before_materializing_them() {
        for capabilities in [
            vec![String::new(); MAX_SERVER_CAPABILITIES + 1],
            vec!["x".repeat(MAX_SERVER_CAPABILITY_BYTES + 1)],
        ] {
            let message = ProtocolMessage::ClientHello(crate::ClientHello {
                protocol_version: PROTOCOL_VERSION,
                client_instance_id: crate::ClientInstanceId(13),
                kind: crate::ClientKind::Interactive,
                device_name: Some("fixture".to_owned()),
                capabilities,
                color_scheme: Some(zz_terminal::TerminalColorScheme::Dark),
                origin: None,
                working_directory: None,
            });
            assert!(matches!(
                encode_protocol_message(&message),
                Err(ProtocolError::InvalidClientHello(_))
            ));

            let payload = postcard::to_stdvec(&message).expect("serialize malformed fixture");
            let frame = crate::framing::encode_enveloped(Lane::Control, &payload)
                .expect("envelope malformed fixture");
            assert!(matches!(
                decode_protocol_frame(&frame),
                Err(ProtocolError::Decode(_))
            ));
        }
    }

    #[test]
    fn client_hello_bounds_working_directories_on_encode_and_decode() {
        let hello_with_working_directory = |length| {
            ProtocolMessage::ClientHello(crate::ClientHello {
                protocol_version: PROTOCOL_VERSION,
                client_instance_id: crate::ClientInstanceId(13),
                kind: crate::ClientKind::Command,
                device_name: None,
                capabilities: Vec::new(),
                color_scheme: None,
                origin: None,
                working_directory: Some(std::path::PathBuf::from("x".repeat(length))),
            })
        };
        let boundary = hello_with_working_directory(MAX_CLIENT_WORKING_DIRECTORY_BYTES);
        let frame = encode_protocol_message(&boundary).expect("encode boundary fixture");
        assert_eq!(
            decode_protocol_frame(&frame).expect("decode boundary fixture"),
            boundary
        );

        let oversized = hello_with_working_directory(MAX_CLIENT_WORKING_DIRECTORY_BYTES + 1);
        assert!(matches!(
            encode_protocol_message(&oversized),
            Err(ProtocolError::InvalidClientHello(_))
        ));

        let payload = postcard::to_stdvec(&oversized).expect("serialize malformed fixture");
        let frame = crate::framing::encode_enveloped(Lane::Control, &payload)
            .expect("envelope malformed fixture");
        assert!(matches!(
            decode_protocol_frame(&frame),
            Err(ProtocolError::Decode(_))
        ));
    }

    #[test]
    fn truncated_server_hello_palette_is_rejected() {
        let message = ProtocolMessage::ServerHello(crate::ServerHello {
            protocol_version: PROTOCOL_VERSION,
            server_id: 7,
            client_id: crate::ClientId(11),
            client_instance_id: crate::ClientInstanceId(13),
            capabilities: Vec::new(),
            appearance: TerminalAppearance::default(),
            appearance_provenance: AppearanceProvenance::default(),
            mux_options: MuxOptions::default(),
            status: crate::StatusLine::default(),
            key_tables: Vec::new(),
        });
        let mut changed = message.clone();
        let ProtocolMessage::ServerHello(hello) = &mut changed else {
            unreachable!("fixture is a ServerHello");
        };
        hello.appearance.palette[200] = Color::rgb(0x12, 0x34, 0x56);

        let payload = postcard::to_stdvec(&message).expect("serialize valid fixture");
        let changed_payload = postcard::to_stdvec(&changed).expect("serialize changed fixture");
        let palette_offset = payload
            .iter()
            .zip(&changed_payload)
            .position(|(left, right)| left != right)
            .expect("changed palette byte");
        let frame = crate::framing::encode_enveloped(Lane::Control, &payload[..palette_offset])
            .expect("envelope truncated fixture");
        assert!(matches!(
            decode_protocol_frame(&frame),
            Err(ProtocolError::Decode(_))
        ));
    }

    #[test]
    fn caller_owned_encoder_reuses_frame_capacity_across_lanes_and_errors() {
        let viewport = TerminalViewport::blank(80, 24, SessionStatus::Running);
        let terminal = ProtocolMessage::Event(Event {
            sequence: 1,
            payload: EventPayload::TerminalViewport {
                pane: PaneId(7),
                viewport,
            },
        });
        let mut frame = Vec::new();
        encode_protocol_message_into(&terminal, &mut frame).expect("initial terminal frame");
        let allocation = frame.as_ptr();
        let capacity = frame.capacity();

        encode_protocol_message_into(&terminal, &mut frame).expect("reused terminal frame");
        assert_eq!(frame.as_ptr(), allocation);
        assert_eq!(frame.capacity(), capacity);
        assert_eq!(
            decode_protocol_frame(&frame).expect("decode terminal"),
            terminal
        );

        let control = ProtocolMessage::Resync;
        encode_protocol_message_into(&control, &mut frame).expect("reused control frame");
        assert_eq!(frame.as_ptr(), allocation);
        assert_eq!(frame.capacity(), capacity);
        let postcard = postcard::to_stdvec(&control).expect("legacy control payload");
        let expected = crate::framing::encode_enveloped(Lane::Control, &postcard)
            .expect("legacy control frame");
        assert_eq!(frame, expected);
        assert_eq!(
            decode_protocol_frame(&frame).expect("decode control"),
            control
        );

        let mut invalid = TerminalViewport::blank(1, 1, SessionStatus::Running);
        Arc::make_mut(&mut invalid.cells)[0] =
            PackedCell::new(u32::from('x'), 8, CellWidth::Narrow);
        let invalid = ProtocolMessage::Event(Event {
            sequence: 2,
            payload: EventPayload::TerminalViewport {
                pane: PaneId(7),
                viewport: invalid,
            },
        });
        assert!(matches!(
            encode_protocol_message_into(&invalid, &mut frame),
            Err(ProtocolError::InvalidTerminal(_))
        ));
        assert!(frame.is_empty());
        assert_eq!(frame.as_ptr(), allocation);
        assert_eq!(frame.capacity(), capacity);
    }

    #[test]
    fn request_full_round_trips_on_the_control_lane() {
        let message = ProtocolMessage::RequestFull { pane: PaneId(17) };
        let frame = encode_protocol_message(&message).expect("encode RequestFull");
        assert_eq!(frame[4], Lane::Control as u8);
        assert_eq!(&frame[6..8], &PROTOCOL_VERSION.to_le_bytes());
        assert_eq!(
            decode_protocol_frame(&frame).expect("decode RequestFull"),
            message
        );
    }

    #[test]
    fn rejects_truncated_frames() {
        assert!(matches!(
            decode_protocol_frame(&[1, 0, 0]),
            Err(ProtocolError::Truncated)
        ));
        assert!(matches!(
            decode_protocol_frame(&[4, 0, 0, 0, 1]),
            Err(ProtocolError::LengthMismatch | ProtocolError::Truncated)
        ));
    }

    #[test]
    fn rejects_reserved_envelope_flags() {
        let mut frame = encode_protocol_message(&ProtocolMessage::Resync).expect("encode frame");
        frame[5] = 0x02;
        assert!(matches!(
            decode_protocol_frame(&frame),
            Err(ProtocolError::UnsupportedFlags(0x02))
        ));
    }

    #[test]
    fn stream_messages_round_trip() {
        let message = ProtocolMessage::ClientHello(crate::ClientHello {
            protocol_version: PROTOCOL_VERSION,
            client_instance_id: crate::ClientInstanceId(13),
            kind: crate::ClientKind::Command,
            device_name: None,
            capabilities: Vec::new(),
            color_scheme: None,
            origin: None,
            working_directory: Some(std::path::PathBuf::from("/tmp/client-fixture")),
        });
        let mut bytes = Vec::new();
        write_protocol_message(&mut bytes, &message).expect("write message");
        assert_eq!(
            read_protocol_message(&mut bytes.as_slice()).expect("read message"),
            message
        );
    }

    #[test]
    fn native_split_resize_round_trips_as_fixed_point_control_input() {
        let message = ProtocolMessage::Input(crate::InputMessage::ResizeSplit {
            window: crate::WindowId(7),
            split: crate::SplitId(12),
            ratio_basis_points: 6_250,
        });
        let frame = encode_protocol_message(&message).expect("encode resize");
        assert_eq!(
            decode_protocol_frame(&frame).expect("decode resize"),
            message
        );
    }

    #[test]
    fn cancel_prefix_round_trips_as_control_input() {
        let message = ProtocolMessage::Input(crate::InputMessage::CancelPrefix { request_id: 17 });
        let frame = encode_protocol_message(&message).expect("encode prefix cancellation");
        assert_eq!(
            decode_protocol_frame(&frame).expect("decode prefix cancellation"),
            message
        );

        let message = ProtocolMessage::Event(crate::Event {
            sequence: 9,
            payload: crate::EventPayload::PrefixCancelled { request_id: 17 },
        });
        let frame = encode_protocol_message(&message).expect("encode prefix cancellation ack");
        assert_eq!(
            decode_protocol_frame(&frame).expect("decode prefix cancellation ack"),
            message
        );
    }

    #[test]
    fn key_input_and_prefix_state_round_trip() {
        let input = zz_terminal::KeyInput {
            action: zz_terminal::KeyAction::Press,
            key: zz_terminal::KeyCode::Character('x'),
            modifiers: zz_terminal::Modifiers::new(false, true, false, false),
            text: Some("x".into()),
            unshifted_codepoint: Some('x'),
        };
        let messages = [
            ProtocolMessage::Input(crate::InputMessage::Key {
                pane: PaneId(3),
                input: input.clone(),
                text_follows: true,
            }),
            ProtocolMessage::Input(crate::InputMessage::Text {
                pane: PaneId(3),
                text: "hello".to_owned(),
            }),
            ProtocolMessage::Input(crate::InputMessage::BrowserSurfaceKey {
                pane: PaneId(4),
                input,
                text_follows: false,
            }),
            ProtocolMessage::Input(crate::InputMessage::BrowserSurfaceText {
                pane: PaneId(4),
                text: "browser text".to_owned(),
            }),
            ProtocolMessage::Input(crate::InputMessage::ClientFocus { focused: true }),
            ProtocolMessage::Event(Event {
                sequence: 7,
                payload: EventPayload::PrefixArmed { armed: true },
            }),
        ];
        for message in messages {
            let frame = encode_protocol_message(&message).expect("encode input message");
            assert_eq!(
                decode_protocol_frame(&frame).expect("decode input message"),
                message
            );
        }
    }

    #[test]
    fn copy_mode_vi_actions_round_trip_on_the_control_lane() {
        for action in [
            zz_terminal::CopyModeAction::NextSpaceEnd,
            zz_terminal::CopyModeAction::NextMatchingBracket,
            zz_terminal::CopyModeAction::SearchCursorWord {
                direction: SearchDirection::Backward,
            },
            zz_terminal::CopyModeAction::GotoLine(42),
        ] {
            let message = ProtocolMessage::Input(crate::InputMessage::TerminalView {
                pane: PaneId(3),
                action: zz_terminal::TerminalViewAction::CopyMode(action),
            });
            let frame = encode_protocol_message(&message).expect("encode copy-mode action");
            assert_eq!(frame[4], Lane::Control as u8);
            assert_eq!(
                decode_protocol_frame(&frame).expect("decode copy-mode action"),
                message
            );
        }

        let message = ProtocolMessage::Input(crate::InputMessage::TerminalView {
            pane: PaneId(3),
            action: zz_terminal::TerminalViewAction::CopyModeCounted {
                action: zz_terminal::CopyModeAction::NextMatchingBracket,
                count: u32::MAX,
            },
        });
        let frame = encode_protocol_message(&message).expect("encode counted copy-mode action");
        assert_eq!(frame[4], Lane::Control as u8);
        assert_eq!(
            decode_protocol_frame(&frame).expect("decode counted copy-mode action"),
            message
        );
    }

    #[test]
    fn borrowed_viewport_encode_matches_owned_event_encode() {
        let pane = PaneId(7);
        let sequence = 42;
        let viewport = TerminalViewport::blank(80, 24, SessionStatus::Running);
        let owned = ProtocolMessage::Event(Event {
            sequence,
            payload: EventPayload::TerminalViewport {
                pane,
                viewport: viewport.clone(),
            },
        });

        let owned_frame = encode_protocol_message(&owned).expect("owned viewport frame");
        let borrowed_frame = encode_terminal_viewport_event(pane, sequence, &viewport)
            .expect("borrowed viewport frame");

        assert_eq!(borrowed_frame, owned_frame);
        assert_eq!(
            decode_protocol_frame(&borrowed_frame).expect("decode borrowed viewport frame"),
            owned
        );
    }

    #[test]
    fn caller_owned_decoder_reuses_transport_capacity_across_lanes() {
        let terminal = ProtocolMessage::Event(Event {
            sequence: 1,
            payload: EventPayload::TerminalViewport {
                pane: PaneId(7),
                viewport: TerminalViewport::blank(80, 24, SessionStatus::Running),
            },
        });
        let control = ProtocolMessage::Resync;
        let terminal_frame = encode_protocol_message(&terminal).expect("terminal frame");
        let control_frame = encode_protocol_message(&control).expect("control frame");
        let stream = [terminal_frame, control_frame].concat();
        let mut bytes = stream.as_slice();
        let mut frame = Vec::new();

        assert_eq!(
            read_protocol_message_into(&mut bytes, &mut frame).expect("terminal message"),
            terminal
        );
        let allocation = frame.as_ptr();
        let capacity = frame.capacity();
        assert_eq!(
            read_protocol_message_into(&mut bytes, &mut frame).expect("control message"),
            control
        );
        assert_eq!(frame.as_ptr(), allocation);
        assert_eq!(frame.capacity(), capacity);
    }

    #[test]
    fn compact_terminal_metadata_rejects_oversized_wire_values() {
        let bytes = (u64::from(u32::MAX) + 1).to_le_bytes();
        let mut reader = WireReader::new(&bytes);
        assert!(matches!(
            compact_u32(&mut reader, "test value"),
            Err(ProtocolError::InvalidTerminal(_))
        ));

        let bytes = u64::from(u32::MAX).to_le_bytes();
        let mut reader = WireReader::new(&bytes);
        assert_eq!(
            compact_u32(&mut reader, "test value").expect("maximum compact value"),
            u32::MAX
        );

        let mut encoded_mode = Vec::new();
        encode_mode(
            &mut encoded_mode,
            TerminalMode::Copy {
                position: u32::MAX,
                total: u32::MAX,
                hide_position: false,
            },
        );
        assert_eq!(encoded_mode.len(), 18);
        assert_eq!(&encoded_mode[1..9], &u64::from(u32::MAX).to_le_bytes());
        assert_eq!(encoded_mode.last(), Some(&0));

        let mut encoded_view_mode = Vec::new();
        encode_mode(
            &mut encoded_view_mode,
            TerminalMode::View {
                position: u32::MAX,
                total: u32::MAX,
            },
        );
        assert_eq!(encoded_view_mode.len(), 17);

        let mut hidden = Vec::new();
        encode_mode(
            &mut hidden,
            TerminalMode::Copy {
                position: 1,
                total: 2,
                hide_position: true,
            },
        );
        assert_eq!(hidden.len(), 18);
        assert_eq!(hidden.last(), Some(&1));
        let mut reader = WireReader::new(&hidden);
        assert_eq!(
            decode_mode(&mut reader).expect("copy mode decodes"),
            TerminalMode::Copy {
                position: 1,
                total: 2,
                hide_position: true,
            }
        );
        let last = hidden.len() - 1;
        hidden[last] = 2;
        let mut reader = WireReader::new(&hidden);
        assert!(decode_mode(&mut reader).is_err());

        let viewport = TerminalViewport::blank(2, 1, SessionStatus::Running);
        let mut frame = Vec::new();
        encode_viewport_into(PaneId(1), 1, &viewport, FULL_VIEWPORT, None, &mut frame)
            .expect("viewport frame");
        let scrollbar_total_offset = 8 + 1 + 8 * 4 + 4 + 2 + 2 + 4 + 4;
        frame[scrollbar_total_offset..scrollbar_total_offset + 8]
            .copy_from_slice(&(u64::from(u32::MAX) + 1).to_le_bytes());
        assert!(matches!(
            decode_protocol_frame(&frame),
            Err(ProtocolError::InvalidTerminal(_))
        ));
    }

    #[test]
    fn terminal_decoder_preflights_declared_sections_before_allocating() {
        let viewport = TerminalViewport::blank(1, 1, SessionStatus::Running);
        let message = ProtocolMessage::Event(Event {
            sequence: 1,
            payload: EventPayload::TerminalViewport {
                pane: PaneId(1),
                viewport,
            },
        });
        let mut frame = encode_protocol_message(&message).expect("viewport frame");
        let payload_offset = 8;
        let columns_offset = payload_offset + 1 + 8 * 4 + 4;
        let cell_count_offset = payload_offset + 98;
        assert_eq!(
            u32::from_le_bytes(
                frame[cell_count_offset..cell_count_offset + 4]
                    .try_into()
                    .expect("cell count"),
            ),
            1
        );
        frame[columns_offset..columns_offset + 2].copy_from_slice(&u16::MAX.to_le_bytes());
        frame[columns_offset + 2..columns_offset + 4].copy_from_slice(&u16::MAX.to_le_bytes());
        let declared_cells = u32::from(u16::MAX) * u32::from(u16::MAX);
        frame[cell_count_offset..cell_count_offset + 4]
            .copy_from_slice(&declared_cells.to_le_bytes());
        assert!(matches!(
            decode_protocol_frame(&frame),
            Err(ProtocolError::Truncated)
        ));

        let previous = TerminalViewport::blank(1, 1, SessionStatus::Running);
        let mut current = previous.clone();
        current.generation = 1;
        current.view_generation = 1;
        let patch = TerminalViewport::diff(&previous, &current).expect("compatible viewport");
        let message = ProtocolMessage::Event(Event {
            sequence: 2,
            payload: EventPayload::TerminalPatch {
                pane: PaneId(1),
                patch,
            },
        });
        let mut frame = encode_protocol_message(&message).expect("patch frame");
        let overlay_count_offset = payload_offset + 142;
        assert_eq!(
            u32::from_le_bytes(
                frame[overlay_count_offset..overlay_count_offset + 4]
                    .try_into()
                    .expect("overlay count"),
            ),
            0
        );
        frame[overlay_count_offset..overlay_count_offset + 4].copy_from_slice(
            &u32::try_from(MAX_OVERLAY_COUNT)
                .expect("overlay limit fits the wire count")
                .to_le_bytes(),
        );
        assert!(matches!(
            decode_protocol_frame(&frame),
            Err(ProtocolError::Truncated)
        ));
    }

    #[test]
    fn terminal_decoder_rejects_every_truncated_packed_frame() {
        let mut viewport = TerminalViewport::blank(3, 2, SessionStatus::failed("cold status"));
        viewport.set_title(Arc::from("truncation fixture"));
        viewport.overlays = Arc::from([OverlaySpan::new(0, 0, 2, OverlayKind::Selection)]);
        let full = encode_protocol_message(&ProtocolMessage::Event(Event {
            sequence: 11,
            payload: EventPayload::TerminalViewport {
                pane: PaneId(7),
                viewport,
            },
        }))
        .expect("full viewport frame");

        for cut in 0..full.len() {
            assert!(
                decode_protocol_frame(&full[..cut]).is_err(),
                "full viewport prefix of {cut} bytes was accepted"
            );
        }

        let previous = TerminalViewport::blank(2, 2, SessionStatus::Running);
        let mut current = previous.clone();
        current.generation = 1;
        current.view_generation = 1;
        current.set_title(Arc::from("patch fixture"));
        current.status = SessionStatus::failed("patch status");
        let mut styles = current.styles().to_vec();
        styles.push(PackedStyle::new(
            Color::rgb(1, 2, 3),
            current.background,
            None,
            zz_terminal::ATTR_BOLD,
            UnderlineStyle::None,
        ));
        let dictionary = Arc::make_mut(&mut current.dictionary);
        dictionary.styles = styles.into();
        dictionary.grapheme_bytes = Arc::from("e\u{301}".as_bytes());
        dictionary.grapheme_offsets = Arc::from([
            0,
            u32::try_from(dictionary.grapheme_bytes.len()).expect("small fixture"),
        ]);
        Arc::make_mut(&mut current.cells)[0] =
            PackedCell::new(GRAPHEME_TABLE_BIT, 1, CellWidth::Narrow);
        current.overlays = Arc::from([OverlaySpan::new(0, 0, 1, OverlayKind::SearchCurrent)]);
        let patch = TerminalViewport::diff(&previous, &current).expect("compatible viewport");
        let patch = encode_protocol_message(&ProtocolMessage::Event(Event {
            sequence: 12,
            payload: EventPayload::TerminalPatch {
                pane: PaneId(7),
                patch,
            },
        }))
        .expect("viewport patch frame");

        for cut in 0..patch.len() {
            assert!(
                decode_protocol_frame(&patch[..cut]).is_err(),
                "viewport patch prefix of {cut} bytes was accepted"
            );
        }
    }

    #[test]
    fn terminal_decoder_validates_text_utf8_only_after_structural_preflight() {
        let invalid_string = [1, 0, 0, 0, 0xff];
        let mut preflight = WireReader::new(&invalid_string);
        preflight_string(&mut preflight).expect("preflight only checks the bounded byte range");
        assert!(preflight.is_empty());

        let mut decoder = WireReader::new(&invalid_string);
        assert!(matches!(
            decode_string(&mut decoder),
            Err(ProtocolError::InvalidTerminal(_))
        ));

        let title = "title-utf8-unique-41";
        let working_directory = "file://localhost/tmp/utf8-unique-42";
        let uri = "https://utf8.example/unique-43";
        let status = "status-utf8-unique-44";
        let mut viewport = TerminalViewport::blank(1, 1, SessionStatus::failed(status));
        viewport.set_title(Arc::from(title));
        viewport.set_working_directory(Some(Arc::from(working_directory)));
        viewport.set_hovered_uri(Some(Arc::from(uri)));
        let full = encode_protocol_message(&ProtocolMessage::Event(Event {
            sequence: 21,
            payload: EventPayload::TerminalViewport {
                pane: PaneId(3),
                viewport,
            },
        }))
        .expect("full viewport fixture");

        let previous = TerminalViewport::blank(1, 1, SessionStatus::Running);
        let mut current = previous.clone();
        current.generation = 1;
        current.view_generation = 1;
        current.set_title(Arc::from(title));
        current.set_working_directory(Some(Arc::from(working_directory)));
        current.set_hovered_uri(Some(Arc::from(uri)));
        current.status = SessionStatus::failed(status);
        let patch = TerminalViewport::diff(&previous, &current).expect("compatible viewport");
        let patch = encode_protocol_message(&ProtocolMessage::Event(Event {
            sequence: 22,
            payload: EventPayload::TerminalPatch {
                pane: PaneId(3),
                patch,
            },
        }))
        .expect("viewport patch fixture");

        let markers: [(&str, &[u8]); 4] = [
            ("title", title.as_bytes()),
            ("working directory", working_directory.as_bytes()),
            ("hovered URI", uri.as_bytes()),
            ("status", status.as_bytes()),
        ];
        for (kind, frame) in [("full viewport", full), ("viewport patch", patch)] {
            for (field, marker) in markers {
                let mut malformed = frame.clone();
                let offset = malformed
                    .windows(marker.len())
                    .position(|window| window == marker)
                    .unwrap_or_else(|| panic!("missing {field} marker in {kind}"));
                malformed[offset] = 0xff;
                assert!(
                    matches!(
                        decode_protocol_frame(&malformed),
                        Err(ProtocolError::InvalidTerminal(_))
                    ),
                    "{kind} accepted invalid UTF-8 in {field}"
                );
            }
        }
    }

    #[test]
    fn cold_terminal_statuses_preserve_the_wire_format() {
        let exited = SessionStatus::exited(7, Some("TERM".to_owned()));
        let mut encoded = Vec::new();
        encode_status(&mut encoded, &exited).expect("encode exited status");
        assert_eq!(encoded, b"\x02\x07\0\0\0\x01\x04\0\0\0TERM");
        let mut reader = WireReader::new(&encoded);
        assert_eq!(decode_status(&mut reader).expect("decode exit"), exited);
        assert!(reader.is_empty());

        let failed = SessionStatus::failed("boom");
        encoded.clear();
        encode_status(&mut encoded, &failed).expect("encode failed status");
        assert_eq!(encoded, b"\x03\x04\0\0\0boom");
        let mut reader = WireReader::new(&encoded);
        assert_eq!(decode_status(&mut reader).expect("decode failure"), failed);
        assert!(reader.is_empty());
    }

    #[test]
    fn niche_encoded_terminal_metadata_preserves_the_wire_format() {
        let search = SearchStatus::new(7, 19)
            .with_pending(true)
            .with_invalid_pattern(true);
        let mut encoded = Vec::new();
        encode_search(&mut encoded, Some(search));
        assert_eq!(encoded, [1, 7, 0, 0, 0, 19, 0, 0, 0, 3]);
        let mut reader = WireReader::new(&encoded);
        assert_eq!(
            decode_search(&mut reader).expect("decode search"),
            Some(search)
        );
        assert!(reader.is_empty());

        let cursor = Cursor::new(
            0x1234,
            0x5678,
            true,
            false,
            true,
            CursorStyle::Underline,
            Color::rgb(1, 2, 3),
        );
        encoded.clear();
        encode_cursor(&mut encoded, Some(cursor));
        assert_eq!(encoded, [1, 0x34, 0x12, 0x78, 0x56, 5, 2, 3, 2, 1, 0]);
        let mut reader = WireReader::new(&encoded);
        assert_eq!(
            decode_cursor(&mut reader).expect("decode cursor"),
            Some(cursor)
        );
        assert!(reader.is_empty());
    }

    #[test]
    fn terminal_lane_round_trips_fixed_width_arrays() {
        let mut viewport = TerminalViewport::blank(2, 1, SessionStatus::Running);
        viewport.generation = 42;
        viewport.view_generation = 7;
        viewport.dictionary_generation = 3;
        viewport.scrollbar = ScrollbarState {
            total: u32::MAX,
            offset: u32::MAX - 1,
            len: 1,
        };
        viewport.mode = TerminalMode::Copy {
            position: u32::MAX,
            total: u32::MAX,
            hide_position: true,
        };
        viewport.unseen_output = u32::MAX;
        viewport.set_working_directory(Some(Arc::from("file://localhost/tmp/terminal-fixture")));
        viewport.set_hovered_uri(Some(Arc::from("https://example.com/terminal")));
        let mut styles = viewport.styles().to_vec();
        styles.push(PackedStyle::new(
            Color::rgb(1, 2, 3),
            viewport.background,
            Some(Color::rgb(4, 5, 6)),
            zz_terminal::ATTR_BOLD,
            UnderlineStyle::Curly,
        ));
        Arc::make_mut(&mut viewport.dictionary).styles = styles.into();
        Arc::make_mut(&mut viewport.cells)[0] =
            PackedCell::new(u32::from('A'), 1, CellWidth::Narrow);
        viewport.search = Some(
            SearchStatus::new(2, 5)
                .with_pending(true)
                .with_invalid_pattern(true),
        );
        viewport.cursor = Some(Cursor::new(
            1,
            0,
            true,
            true,
            false,
            CursorStyle::Block,
            Color::rgb(7, 8, 9),
        ));
        viewport.overlays = Arc::from([OverlaySpan::new(0, 0, 1, OverlayKind::Selection)]);
        viewport.kitty_placements = Arc::from([KittyPlacement {
            image_id: 17,
            image_generation: 9,
            layer: KittyLayer::BelowText,
            viewport_col: 0,
            viewport_row: 0,
            absolute_row: u64::from(u32::MAX - 1),
            cell_offset_x: 2,
            cell_offset_y: 3,
            grid_cols: 1,
            grid_rows: 1,
            pixel_width: 20,
            pixel_height: 10,
            source_rect: Some((1, 2, 8, 6)),
        }]);
        viewport.status = SessionStatus::failed("renderer disconnected");
        let expected_payload_len = viewport_payload_capacity(
            &viewport,
            viewport.title().len(),
            viewport.working_directory().map_or(0, str::len),
            viewport.hovered_uri().map_or(0, str::len),
        )
        .expect("payload capacity");
        let message = ProtocolMessage::Event(Event {
            sequence: 99,
            payload: EventPayload::TerminalViewport {
                pane: PaneId(12),
                viewport,
            },
        });

        let encoded = encode_protocol_message(&message).expect("encode");
        assert_eq!(encoded[4], Lane::Terminal as u8);
        let (_, payload) = decode_enveloped(&encoded).expect("decode envelope");
        assert_eq!(payload.len(), expected_payload_len);
        assert_eq!(decode_protocol_frame(&encoded).expect("decode"), message);
    }

    #[test]
    fn terminal_lane_rejects_invalid_hovered_uri_metadata() {
        let mut viewport = TerminalViewport::blank(1, 1, SessionStatus::Running);
        viewport.set_hovered_uri(Some(Arc::from("https://example.com/not allowed")));
        let message = ProtocolMessage::Event(Event {
            sequence: 1,
            payload: EventPayload::TerminalViewport {
                pane: PaneId(1),
                viewport,
            },
        });
        assert!(matches!(
            encode_protocol_message(&message),
            Err(ProtocolError::InvalidTerminal(_))
        ));
    }

    #[test]
    fn terminal_lane_rejects_invalid_working_directory_metadata() {
        let mut viewport = TerminalViewport::blank(1, 1, SessionStatus::Running);
        viewport.set_working_directory(Some(Arc::from("file://localhost/tmp/not\nallowed")));
        let message = ProtocolMessage::Event(Event {
            sequence: 1,
            payload: EventPayload::TerminalViewport {
                pane: PaneId(1),
                viewport,
            },
        });
        assert!(matches!(
            encode_protocol_message(&message),
            Err(ProtocolError::InvalidTerminal(_))
        ));
    }

    #[test]
    fn osc8_open_uri_uses_the_reliable_control_lane() {
        let message = ProtocolMessage::Event(Event {
            sequence: 100,
            payload: EventPayload::OpenUri {
                pane: PaneId(12),
                uri: "https://example.com/docs".to_owned(),
            },
        });
        let encoded = encode_protocol_message(&message).expect("encode");
        assert_eq!(encoded[4], Lane::Control as u8);
        assert_eq!(decode_protocol_frame(&encoded).expect("decode"), message);
    }

    #[test]
    fn agent_commands_round_trip_and_reject_oversized_payloads() {
        let message = ProtocolMessage::Event(Event {
            sequence: 102,
            payload: EventPayload::AgentCommand {
                pane: PaneId(3),
                request_id: 77,
                command: crate::AgentCommand::ComposerAppend {
                    text: "review this diff".to_owned(),
                },
            },
        });
        let encoded = encode_protocol_message(&message).expect("encode");
        assert_eq!(encoded[4], Lane::Control as u8);
        assert_eq!(decode_protocol_frame(&encoded).expect("decode"), message);

        let oversized = ProtocolMessage::Event(Event {
            sequence: 103,
            payload: EventPayload::AgentCommand {
                pane: PaneId(3),
                request_id: 78,
                command: crate::AgentCommand::Prompt {
                    text: "x".repeat(MAX_AGENT_SEND_BYTES + 1),
                },
            },
        });
        assert!(matches!(
            encode_protocol_message(&oversized),
            Err(ProtocolError::InvalidGuiRequest(_))
        ));
    }

    fn agent_state_fixture() -> crate::AgentPaneWire {
        crate::AgentPaneWire {
            phase: crate::AgentConnectionPhase::Running,
            queued_prompts: 2,
            session_id: Some("sess-7".to_owned()),
            title: Some("port the runtime".to_owned()),
            error: Some("setting failed".to_owned()),
            auth_methods: r#"[{"id":"oauth"}]"#.to_owned(),
            config_options: r#"[{"id":"model","value":"opus"}]"#.to_owned(),
            modes: r#"{"current":"plan"}"#.to_owned(),
            pending_permission: Some(crate::AgentPermissionWire {
                request_id: 4,
                payload: r#"{"tool":"edit"}"#.to_owned(),
            }),
            git: Some(crate::AgentGitSummary {
                branch: Some("main".to_owned()),
                changed_files: 3,
                additions: 21,
                deletions: 8,
            }),
        }
    }

    fn assert_control_round_trip(message: &ProtocolMessage) {
        let encoded = encode_protocol_message(message).expect("encode agent message");
        assert_eq!(encoded[4], Lane::Control as u8);
        assert_eq!(
            &decode_protocol_frame(&encoded).expect("decode agent message"),
            message
        );
    }

    #[test]
    fn agent_runtime_commands_round_trip_on_the_control_lane() {
        let pane = PaneId(12);
        let messages = vec![
            ProtocolMessage::AgentPrompt {
                pane,
                text: "port the runtime".to_owned(),
                images: vec![crate::AgentImage {
                    format: "png".to_owned(),
                    data: vec![0x89, 0x50, 0x4e, 0x47],
                }],
            },
            ProtocolMessage::AgentCancel { pane },
            ProtocolMessage::AgentUnqueue { pane },
            ProtocolMessage::AgentRespondPermission {
                pane,
                request_id: 9,
                option_id: Some("allow-once".to_owned()),
            },
            ProtocolMessage::AgentRespondPermission {
                pane,
                request_id: 10,
                option_id: None,
            },
            ProtocolMessage::AgentSetConfigOption {
                pane,
                option_id: "model".to_owned(),
                value: "opus".to_owned(),
            },
            ProtocolMessage::AgentSetMode {
                pane,
                mode_id: "plan".to_owned(),
            },
            ProtocolMessage::AgentAuthenticate {
                pane,
                method_id: "oauth".to_owned(),
            },
            ProtocolMessage::AgentSessionOp {
                pane,
                op: AgentSessionOpKind::List {
                    cwd: Some("/work".into()),
                    cursor: Some("page-2".to_owned()),
                    replace: false,
                },
            },
            ProtocolMessage::AgentSessionOp {
                pane,
                op: AgentSessionOpKind::New {
                    cwd: "/next".into(),
                },
            },
            ProtocolMessage::AgentSessionOp {
                pane,
                op: AgentSessionOpKind::Switch {
                    session_id: "sess-7".to_owned(),
                    cwd: "/restored".into(),
                    additional_directories: vec!["/shared".into()],
                },
            },
            ProtocolMessage::AgentSessionOp {
                pane,
                op: AgentSessionOpKind::Delete {
                    session_id: "sess-8".to_owned(),
                },
            },
            ProtocolMessage::AgentReplay { pane, from_seq: 41 },
            ProtocolMessage::AgentAcknowledgePromptRestore {
                pane,
                reclaim_id: 3,
            },
        ];
        for message in &messages {
            assert_control_round_trip(message);
        }
    }

    #[test]
    fn agent_stream_events_round_trip_on_the_control_lane() {
        let pane = PaneId(12);
        for payload in [
            EventPayload::AgentUpdates {
                pane,
                first_seq: 17,
                items: vec![
                    br#"{"kind":"assistant"}"#.to_vec(),
                    br#"{"kind":"tool"}"#.to_vec(),
                ],
            },
            EventPayload::AgentState {
                pane,
                state: agent_state_fixture(),
            },
            EventPayload::AgentState {
                pane,
                state: crate::AgentPaneWire {
                    phase: crate::AgentConnectionPhase::Failed {
                        message: "adapter exited".to_owned(),
                    },
                    ..crate::AgentPaneWire::default()
                },
            },
            EventPayload::AgentLagged { pane, next_seq: 88 },
            EventPayload::AgentSessions {
                pane,
                request_id: 5,
                result: r#"{"sessions":[]}"#.to_owned(),
            },
        ] {
            assert_control_round_trip(&ProtocolMessage::Event(Event {
                sequence: 4,
                payload,
            }));
        }
    }

    #[test]
    fn agent_prompts_bound_their_text_images_and_formats() {
        let prompt = |text: String, images: Vec<crate::AgentImage>| ProtocolMessage::AgentPrompt {
            pane: PaneId(1),
            text,
            images,
        };
        assert!(
            encode_protocol_message(&prompt("x".repeat(MAX_AGENT_PROMPT_BYTES), Vec::new()))
                .is_ok()
        );
        assert!(matches!(
            encode_protocol_message(&prompt("x".repeat(MAX_AGENT_PROMPT_BYTES + 1), Vec::new())),
            Err(ProtocolError::InvalidAgentPayload(_))
        ));

        let image = |bytes: usize| crate::AgentImage {
            format: "png".to_owned(),
            data: vec![0; bytes],
        };
        assert!(
            encode_protocol_message(&prompt(
                "x".to_owned(),
                vec![
                    image(MAX_AGENT_PROMPT_BYTES / 2),
                    image(MAX_AGENT_PROMPT_BYTES / 2 - 1)
                ]
            ))
            .is_ok()
        );
        assert!(matches!(
            encode_protocol_message(&prompt(
                "x".to_owned(),
                vec![
                    image(MAX_AGENT_PROMPT_BYTES / 2),
                    image(MAX_AGENT_PROMPT_BYTES / 2)
                ]
            )),
            Err(ProtocolError::InvalidAgentPayload(_))
        ));

        let format = |length: usize| {
            prompt(
                String::new(),
                vec![crate::AgentImage {
                    format: "f".repeat(length),
                    data: Vec::new(),
                }],
            )
        };
        assert!(encode_protocol_message(&format(MAX_AGENT_IMAGE_FORMAT_BYTES)).is_ok());
        assert!(matches!(
            encode_protocol_message(&format(MAX_AGENT_IMAGE_FORMAT_BYTES + 1)),
            Err(ProtocolError::InvalidAgentPayload(_))
        ));
        assert!(matches!(
            encode_protocol_message(&prompt(
                String::new(),
                (0..=MAX_AGENT_PROMPT_IMAGES).map(|_| image(0)).collect(),
            )),
            Err(ProtocolError::InvalidAgentPayload(_))
        ));
    }

    #[test]
    fn agent_identifiers_bound_options_modes_methods_and_sessions() {
        let pane = PaneId(2);
        let at_limit = "o".repeat(MAX_AGENT_OPTION_BYTES);
        let over_limit = "o".repeat(MAX_AGENT_OPTION_BYTES + 1);
        for (accepted, rejected) in [
            (
                ProtocolMessage::AgentSetConfigOption {
                    pane,
                    option_id: at_limit.clone(),
                    value: at_limit.clone(),
                },
                ProtocolMessage::AgentSetConfigOption {
                    pane,
                    option_id: at_limit.clone(),
                    value: over_limit.clone(),
                },
            ),
            (
                ProtocolMessage::AgentSetMode {
                    pane,
                    mode_id: at_limit.clone(),
                },
                ProtocolMessage::AgentSetMode {
                    pane,
                    mode_id: over_limit.clone(),
                },
            ),
            (
                ProtocolMessage::AgentAuthenticate {
                    pane,
                    method_id: at_limit.clone(),
                },
                ProtocolMessage::AgentAuthenticate {
                    pane,
                    method_id: over_limit.clone(),
                },
            ),
            (
                ProtocolMessage::AgentRespondPermission {
                    pane,
                    request_id: 1,
                    option_id: Some(at_limit.clone()),
                },
                ProtocolMessage::AgentRespondPermission {
                    pane,
                    request_id: 1,
                    option_id: Some(over_limit.clone()),
                },
            ),
        ] {
            assert!(encode_protocol_message(&accepted).is_ok());
            assert!(matches!(
                encode_protocol_message(&rejected),
                Err(ProtocolError::InvalidAgentPayload(_))
            ));
        }

        let session_op = |session_id: String| ProtocolMessage::AgentSessionOp {
            pane,
            op: AgentSessionOpKind::Switch {
                session_id,
                cwd: "/work".into(),
                additional_directories: Vec::new(),
            },
        };
        assert!(
            encode_protocol_message(&session_op("s".repeat(MAX_AGENT_SESSION_ID_BYTES))).is_ok()
        );
        assert!(matches!(
            encode_protocol_message(&session_op("s".repeat(MAX_AGENT_SESSION_ID_BYTES + 1))),
            Err(ProtocolError::InvalidAgentPayload(_))
        ));
        assert!(
            encode_protocol_message(&ProtocolMessage::AgentSessionOp {
                pane,
                op: AgentSessionOpKind::New {
                    cwd: "relative".into(),
                },
            })
            .is_ok()
        );
        assert!(matches!(
            encode_protocol_message(&ProtocolMessage::AgentSessionOp {
                pane,
                op: AgentSessionOpKind::New {
                    cwd: "x".repeat(MAX_GUI_TEXT_BYTES + 1).into(),
                },
            }),
            Err(ProtocolError::InvalidAgentPayload(_))
        ));
        assert!(matches!(
            encode_protocol_message(&ProtocolMessage::AgentSessionOp {
                pane,
                op: AgentSessionOpKind::Switch {
                    session_id: "session".to_owned(),
                    cwd: "/work".into(),
                    additional_directories: vec![
                        "/shared".into();
                        MAX_AGENT_SESSION_DIRECTORIES + 1
                    ],
                },
            }),
            Err(ProtocolError::InvalidAgentPayload(_))
        ));
        assert!(matches!(
            encode_protocol_message(&ProtocolMessage::AgentSessionOp {
                pane,
                op: AgentSessionOpKind::List {
                    cwd: None,
                    cursor: Some("c".repeat(MAX_AGENT_SESSION_ID_BYTES + 1)),
                    replace: false,
                },
            }),
            Err(ProtocolError::InvalidAgentPayload(_))
        ));
    }

    #[test]
    fn agent_update_batches_bound_their_bytes_and_sequence_space() {
        let batch = |first_seq: u64, items: Vec<Vec<u8>>| {
            ProtocolMessage::Event(Event {
                sequence: 7,
                payload: EventPayload::AgentUpdates {
                    pane: PaneId(3),
                    first_seq,
                    items,
                },
            })
        };
        assert!(encode_protocol_message(&batch(0, vec![vec![0; MAX_AGENT_UPDATES_BYTES]])).is_ok());
        assert!(matches!(
            encode_protocol_message(&batch(0, vec![vec![0; MAX_AGENT_UPDATES_BYTES], vec![0]])),
            Err(ProtocolError::InvalidAgentPayload(_))
        ));
        assert!(matches!(
            encode_protocol_message(&batch(0, Vec::new())),
            Err(ProtocolError::InvalidAgentPayload(_))
        ));
        assert!(matches!(
            encode_protocol_message(&batch(u64::MAX, vec![vec![1]])),
            Err(ProtocolError::InvalidAgentPayload(_))
        ));
    }

    #[test]
    fn agent_state_bounds_blobs_permissions_and_results() {
        let state = |state: crate::AgentPaneWire| {
            ProtocolMessage::Event(Event {
                sequence: 8,
                payload: EventPayload::AgentState {
                    pane: PaneId(4),
                    state,
                },
            })
        };
        let with_modes = |length: usize| crate::AgentPaneWire {
            modes: "m".repeat(length),
            ..agent_state_fixture()
        };
        assert!(encode_protocol_message(&state(with_modes(MAX_AGENT_STATE_BLOB_BYTES))).is_ok());
        assert!(matches!(
            encode_protocol_message(&state(with_modes(MAX_AGENT_STATE_BLOB_BYTES + 1))),
            Err(ProtocolError::InvalidAgentPayload(_))
        ));

        let with_permission = |length: usize| crate::AgentPaneWire {
            pending_permission: Some(crate::AgentPermissionWire {
                request_id: 2,
                payload: "p".repeat(length),
            }),
            ..agent_state_fixture()
        };
        assert!(
            encode_protocol_message(&state(with_permission(MAX_AGENT_PERMISSION_BYTES))).is_ok()
        );
        assert!(matches!(
            encode_protocol_message(&state(with_permission(MAX_AGENT_PERMISSION_BYTES + 1))),
            Err(ProtocolError::InvalidAgentPayload(_))
        ));
        let with_error = |length: usize| crate::AgentPaneWire {
            error: Some("e".repeat(length)),
            ..agent_state_fixture()
        };
        assert!(encode_protocol_message(&state(with_error(MAX_AGENT_STATE_BLOB_BYTES))).is_ok());
        assert!(matches!(
            encode_protocol_message(&state(with_error(MAX_AGENT_STATE_BLOB_BYTES + 1))),
            Err(ProtocolError::InvalidAgentPayload(_))
        ));

        let sessions = |length: usize| {
            ProtocolMessage::Event(Event {
                sequence: 9,
                payload: EventPayload::AgentSessions {
                    pane: PaneId(4),
                    request_id: 1,
                    result: "r".repeat(length),
                },
            })
        };
        assert!(encode_protocol_message(&sessions(MAX_AGENT_RESULT_BYTES)).is_ok());
        assert!(matches!(
            encode_protocol_message(&sessions(MAX_AGENT_RESULT_BYTES + 1)),
            Err(ProtocolError::InvalidAgentPayload(_))
        ));

        let with_branch = |length: usize| crate::AgentPaneWire {
            git: Some(crate::AgentGitSummary {
                branch: Some("b".repeat(length)),
                ..crate::AgentGitSummary::default()
            }),
            ..agent_state_fixture()
        };
        assert!(encode_protocol_message(&state(with_branch(MAX_AGENT_OPTION_BYTES))).is_ok());
        assert!(matches!(
            encode_protocol_message(&state(with_branch(MAX_AGENT_OPTION_BYTES + 1))),
            Err(ProtocolError::InvalidAgentPayload(_))
        ));
    }

    #[test]
    fn agent_bounds_also_reject_hand_built_frames() {
        let tag_of = |message: &ProtocolMessage| {
            postcard::to_stdvec(message).expect("a valid message encodes")[0]
        };
        let forge = |tag: u8, fields: Vec<u8>| {
            let mut payload = vec![tag];
            payload.extend(fields);
            crate::framing::encode_enveloped(Lane::Control, &payload).expect("envelope")
        };
        let mode_tag = tag_of(&ProtocolMessage::AgentSetMode {
            pane: PaneId(0),
            mode_id: String::new(),
        });
        let oversized_mode = forge(
            mode_tag,
            postcard::to_stdvec(&(0_u64, "m".repeat(MAX_AGENT_OPTION_BYTES + 1))).expect("fields"),
        );
        assert!(decode_protocol_frame(&oversized_mode).is_err());

        let prompt_tag = tag_of(&ProtocolMessage::AgentPrompt {
            pane: PaneId(0),
            text: String::new(),
            images: Vec::new(),
        });
        let oversized_format = forge(
            prompt_tag,
            postcard::to_stdvec(&(
                0_u64,
                "",
                vec![(
                    "f".repeat(MAX_AGENT_IMAGE_FORMAT_BYTES + 1),
                    Vec::<u8>::new(),
                )],
            ))
            .expect("fields"),
        );
        assert!(decode_protocol_frame(&oversized_format).is_err());

        let updates = ProtocolMessage::Event(Event {
            sequence: 1,
            payload: EventPayload::AgentUpdates {
                pane: PaneId(0),
                first_seq: 0,
                items: vec![vec![0; MAX_AGENT_UPDATES_BYTES], vec![0]],
            },
        });
        let oversized_batch = crate::framing::encode_enveloped(
            Lane::Control,
            &postcard::to_stdvec(&updates).expect("oversized batch fixture"),
        )
        .expect("envelope");
        assert!(decode_protocol_frame(&oversized_batch).is_err());
    }

    #[test]
    fn gui_responses_round_trip_and_reject_oversized_text() {
        let message = ProtocolMessage::GuiResponse(crate::GuiResponse::Success {
            request_id: 5,
            output: "/tmp/frame.png".to_owned(),
        });
        let encoded = encode_protocol_message(&message).expect("encode");
        assert_eq!(encoded[4], Lane::Control as u8);
        assert_eq!(decode_protocol_frame(&encoded).expect("decode"), message);

        let oversized = ProtocolMessage::GuiResponse(crate::GuiResponse::Error {
            request_id: 6,
            message: "x".repeat(MAX_GUI_TEXT_BYTES + 1),
        });
        assert!(matches!(
            encode_protocol_message(&oversized),
            Err(ProtocolError::InvalidGuiRequest(_))
        ));
    }

    #[test]
    fn browser_screenshot_requests_bound_their_path() {
        let message = ProtocolMessage::Event(Event {
            sequence: 104,
            payload: EventPayload::BrowserCommand {
                pane: PaneId(8),
                command: BrowserCommand::Screenshot {
                    request_id: 9,
                    path: "/tmp/zz/frame.png".to_owned(),
                },
            },
        });
        let encoded = encode_protocol_message(&message).expect("encode");
        assert_eq!(decode_protocol_frame(&encoded).expect("decode"), message);

        let oversized = ProtocolMessage::Event(Event {
            sequence: 105,
            payload: EventPayload::BrowserCommand {
                pane: PaneId(8),
                command: BrowserCommand::Screenshot {
                    request_id: 10,
                    path: "x".repeat(MAX_GUI_TEXT_BYTES + 1),
                },
            },
        });
        assert!(matches!(
            encode_protocol_message(&oversized),
            Err(ProtocolError::InvalidGuiRequest(_))
        ));
    }

    #[test]
    fn repeated_browser_keys_round_trip_without_expansion() {
        let message = ProtocolMessage::Event(Event {
            sequence: 106,
            payload: EventPayload::BrowserCommand {
                pane: PaneId(8),
                command: BrowserCommand::SendKeysRepeated {
                    keys: vec![crate::KeyToken::Literal("x".to_owned())],
                    count: u32::MAX,
                },
            },
        });
        let encoded = encode_protocol_message(&message).expect("encode");
        assert_eq!(decode_protocol_frame(&encoded).expect("decode"), message);
    }

    #[test]
    fn paste_uploads_round_trip_on_the_control_lane() {
        let begin = ProtocolMessage::PasteUploadBegin {
            upload_id: 17,
            pane: PaneId(4),
            purpose: PasteUploadPurpose::PastePath,
            extension: "png".to_owned(),
            total_bytes: 3,
        };
        let encoded = encode_protocol_message(&begin).expect("encode");
        assert_eq!(encoded[4], Lane::Control as u8);
        assert_eq!(decode_protocol_frame(&encoded).expect("decode"), begin);

        let chunk = ProtocolMessage::PasteUploadChunk {
            upload_id: 17,
            bytes: vec![0x89, 0x50, 0x4e],
        };
        let encoded = encode_protocol_message(&chunk).expect("encode");
        assert_eq!(encoded[4], Lane::Control as u8);
        assert_eq!(decode_protocol_frame(&encoded).expect("decode"), chunk);

        let record = ProtocolMessage::PasteUploadBegin {
            upload_id: 18,
            pane: PaneId(4),
            purpose: PasteUploadPurpose::RecordPastedImage,
            extension: "webp".to_owned(),
            total_bytes: 3,
        };
        let encoded = encode_protocol_message(&record).expect("encode record upload");
        assert_eq!(encoded[4], Lane::Control as u8);
        assert_eq!(decode_protocol_frame(&encoded).expect("decode"), record);

        let unsupported_record = ProtocolMessage::PasteUploadBegin {
            upload_id: 19,
            pane: PaneId(4),
            purpose: PasteUploadPurpose::RecordPastedImage,
            extension: "tiff".to_owned(),
            total_bytes: 3,
        };
        assert!(matches!(
            encode_protocol_message(&unsupported_record),
            Err(ProtocolError::InvalidPasteUpload(_))
        ));
    }

    #[test]
    fn pasted_image_fetch_controls_round_trip_and_enforce_byte_budgets() {
        let pane = PaneId(7);
        let messages = [
            ProtocolMessage::FetchPastedImage { pane, number: 12 },
            ProtocolMessage::PastedImageBegin {
                pane,
                number: 12,
                format: PastedImageFormat::Jpeg,
                total_bytes: 3,
            },
            ProtocolMessage::PastedImageChunk {
                pane,
                number: 12,
                bytes: vec![1, 2, 3],
            },
            ProtocolMessage::PastedImageUnavailable { pane, number: 13 },
        ];
        for message in messages {
            let encoded = encode_protocol_message(&message).expect("encode pasted-image control");
            assert_eq!(encoded[4], Lane::Control as u8);
            assert_eq!(decode_protocol_frame(&encoded).expect("decode"), message);
        }

        let oversized = ProtocolMessage::PastedImageChunk {
            pane,
            number: 12,
            bytes: vec![0; MAX_PASTE_UPLOAD_CHUNK_BYTES + 1],
        };
        assert!(matches!(
            encode_protocol_message(&oversized),
            Err(ProtocolError::InvalidPasteUpload(_))
        ));
        let empty = ProtocolMessage::PastedImageBegin {
            pane,
            number: 12,
            format: PastedImageFormat::Png,
            total_bytes: 0,
        };
        assert!(matches!(
            encode_protocol_message(&empty),
            Err(ProtocolError::InvalidPasteUpload(_))
        ));
    }

    #[test]
    fn paste_uploads_reject_oversized_totals_and_chunks() {
        let oversized_total = ProtocolMessage::PasteUploadBegin {
            upload_id: 1,
            pane: PaneId(4),
            purpose: PasteUploadPurpose::PastePath,
            extension: "png".to_owned(),
            total_bytes: MAX_PASTE_UPLOAD_BYTES + 1,
        };
        assert!(matches!(
            encode_protocol_message(&oversized_total),
            Err(ProtocolError::InvalidPasteUpload(_))
        ));

        let empty_total = ProtocolMessage::PasteUploadBegin {
            upload_id: 2,
            pane: PaneId(4),
            purpose: PasteUploadPurpose::PastePath,
            extension: "png".to_owned(),
            total_bytes: 0,
        };
        assert!(matches!(
            encode_protocol_message(&empty_total),
            Err(ProtocolError::InvalidPasteUpload(_))
        ));

        let oversized_chunk = ProtocolMessage::PasteUploadChunk {
            upload_id: 3,
            bytes: vec![0; MAX_PASTE_UPLOAD_CHUNK_BYTES + 1],
        };
        assert!(matches!(
            encode_protocol_message(&oversized_chunk),
            Err(ProtocolError::InvalidPasteUpload(_))
        ));
    }

    #[test]
    fn paste_upload_extensions_stay_inside_one_path_segment() {
        for extension in ["", "PNG", "p/g", "p.g", "../x", "toolongext", "png "] {
            assert!(
                !paste_upload_extension_is_valid(extension),
                "{extension:?} should not be a usable file extension"
            );
            let rejected = ProtocolMessage::PasteUploadBegin {
                upload_id: 4,
                pane: PaneId(4),
                purpose: PasteUploadPurpose::PastePath,
                extension: extension.to_owned(),
                total_bytes: 16,
            };
            assert!(matches!(
                encode_protocol_message(&rejected),
                Err(ProtocolError::InvalidPasteUpload(_))
            ));
        }
        for extension in ["png", "jpg", "webp", "gif", "tiff", "pnm", "a1"] {
            assert!(paste_upload_extension_is_valid(extension));
        }
    }

    #[test]
    fn paste_upload_bounds_also_reject_hand_built_frames() {
        let tag_of = |message: &ProtocolMessage| {
            postcard::to_stdvec(message).expect("a valid message encodes")[0]
        };
        let begin_tag = tag_of(&ProtocolMessage::PasteUploadBegin {
            upload_id: 0,
            pane: PaneId(0),
            purpose: PasteUploadPurpose::PastePath,
            extension: "png".to_owned(),
            total_bytes: 1,
        });
        let chunk_tag = tag_of(&ProtocolMessage::PasteUploadChunk {
            upload_id: 0,
            bytes: Vec::new(),
        });
        let forge = |tag: u8, fields: Vec<u8>| {
            let mut payload = vec![tag];
            payload.extend(fields);
            crate::framing::encode_enveloped(Lane::Control, &payload).expect("envelope")
        };
        let bad_extension = forge(
            begin_tag,
            postcard::to_stdvec(&(
                1_u64,
                4_u64,
                PasteUploadPurpose::PastePath,
                "../etc",
                16_u32,
            ))
            .expect("fields"),
        );
        assert!(decode_protocol_frame(&bad_extension).is_err());

        let bad_total = forge(
            begin_tag,
            postcard::to_stdvec(&(
                1_u64,
                4_u64,
                PasteUploadPurpose::PastePath,
                "png",
                MAX_PASTE_UPLOAD_BYTES + 1,
            ))
            .expect("fields"),
        );
        assert!(decode_protocol_frame(&bad_total).is_err());

        let bad_chunk = forge(
            chunk_tag,
            postcard::to_stdvec(&(1_u64, vec![0_u8; MAX_PASTE_UPLOAD_CHUNK_BYTES + 1]))
                .expect("fields"),
        );
        assert!(decode_protocol_frame(&bad_chunk).is_err());
    }

    #[test]
    fn focus_sidebar_uses_the_reliable_control_lane() {
        let message = ProtocolMessage::Event(Event {
            sequence: 101,
            payload: EventPayload::FocusSidebar,
        });
        let encoded = encode_protocol_message(&message).expect("encode");
        assert_eq!(encoded[4], Lane::Control as u8);
        assert_eq!(decode_protocol_frame(&encoded).expect("decode"), message);
    }

    #[test]
    fn terminal_ui_commands_use_the_reliable_control_lane() {
        let message = ProtocolMessage::Event(Event {
            sequence: 101,
            payload: EventPayload::TerminalUiCommand {
                pane: PaneId(12),
                command: crate::TerminalUiCommand::BeginSearch {
                    direction: SearchDirection::Backward,
                },
            },
        });
        let encoded = encode_protocol_message(&message).expect("encode");
        assert_eq!(encoded[4], Lane::Control as u8);
        assert_eq!(decode_protocol_frame(&encoded).expect("decode"), message);
    }

    #[test]
    fn client_messages_use_the_reliable_control_lane() {
        let message = ProtocolMessage::Event(Event {
            sequence: 102,
            payload: EventPayload::ClientMessage {
                pane: Some(PaneId(12)),
                kind: crate::ClientMessageKind::Error,
                text: "copy-pipe exited unsuccessfully".to_owned(),
            },
        });
        let encoded = encode_protocol_message(&message).expect("encode");
        assert_eq!(encoded[4], Lane::Control as u8);
        assert_eq!(decode_protocol_frame(&encoded).expect("decode"), message);
    }

    #[test]
    fn command_prompt_updates_use_the_reliable_control_lane() {
        let message = ProtocolMessage::Event(Event {
            sequence: 103,
            payload: EventPayload::CommandPrompt {
                state: Some(crate::CommandPromptState {
                    prompt: ":".to_owned(),
                    input: "list-panes".to_owned(),
                    cursor: 10,
                    kind: crate::CommandPromptKind::Command,
                    history: vec!["list-sessions".to_owned(), "list-panes".to_owned()],
                    prompt_type: crate::CommandPromptType::Command,
                    mode: crate::CommandPromptMode::Text,
                    no_freeze: false,
                }),
            },
        });
        let encoded = encode_protocol_message(&message).expect("encode");
        assert_eq!(encoded[4], Lane::Control as u8);
        assert_eq!(decode_protocol_frame(&encoded).expect("decode"), message);
    }

    #[test]
    fn command_prompt_actions_round_trip_on_the_control_lane() {
        let actions = [
            crate::CommandPromptAction::Update {
                input: "rename-window café".to_owned(),
                cursor: 18,
            },
            crate::CommandPromptAction::Submit {
                input: "list-panes -a".to_owned(),
            },
            crate::CommandPromptAction::Close,
        ];
        for action in actions {
            let message = ProtocolMessage::Input(crate::InputMessage::CommandPrompt { action });
            let encoded = encode_protocol_message(&message).expect("encode");
            assert_eq!(encoded[4], Lane::Control as u8);
            assert_eq!(decode_protocol_frame(&encoded).expect("decode"), message);
        }
    }

    #[test]
    fn command_output_viewports_use_the_terminal_lane() {
        let mut viewport = TerminalViewport::blank(4, 2, SessionStatus::Running);
        viewport.mode = TerminalMode::View {
            position: 1,
            total: 8,
        };
        let message = ProtocolMessage::Event(Event {
            sequence: 104,
            payload: EventPayload::CommandOutput {
                pane: PaneId(12),
                output_id: 0x0123_4567_89ab_cdef,
                viewport: Some(viewport),
            },
        });
        let encoded = encode_protocol_message(&message).expect("encode");
        assert_eq!(encoded[4], Lane::Terminal as u8);
        assert_eq!(
            u64::from_le_bytes(encoded[25..33].try_into().expect("output ID")),
            0x0123_4567_89ab_cdef
        );
        assert_eq!(decode_protocol_frame(&encoded).expect("decode"), message);
    }

    #[test]
    fn command_output_close_uses_the_reliable_control_lane() {
        let message = ProtocolMessage::Event(Event {
            sequence: 105,
            payload: EventPayload::CommandOutput {
                pane: PaneId(12),
                output_id: 0x0123_4567_89ab_cdef,
                viewport: None,
            },
        });
        let encoded = encode_protocol_message(&message).expect("encode");
        assert_eq!(encoded[4], Lane::Control as u8);
        assert_eq!(decode_protocol_frame(&encoded).expect("decode"), message);
    }

    #[test]
    fn command_output_zero_id_is_reserved_for_empty_resync() {
        let empty = ProtocolMessage::Event(Event {
            sequence: 106,
            payload: EventPayload::CommandOutput {
                pane: PaneId(12),
                output_id: 0,
                viewport: None,
            },
        });
        let encoded = encode_protocol_message(&empty).expect("encode empty resync");
        assert_eq!(encoded[4], Lane::Control as u8);
        assert_eq!(decode_protocol_frame(&encoded).expect("decode"), empty);

        let populated = ProtocolMessage::Event(Event {
            sequence: 107,
            payload: EventPayload::CommandOutput {
                pane: PaneId(12),
                output_id: 0,
                viewport: Some(TerminalViewport::blank(4, 2, SessionStatus::Running)),
            },
        });
        assert!(matches!(
            encode_protocol_message(&populated),
            Err(ProtocolError::InvalidTerminal(_))
        ));

        let valid = ProtocolMessage::Event(Event {
            sequence: 108,
            payload: EventPayload::CommandOutput {
                pane: PaneId(12),
                output_id: 9,
                viewport: Some(TerminalViewport::blank(4, 2, SessionStatus::Running)),
            },
        });
        let mut encoded = encode_protocol_message(&valid).expect("encode populated output");
        encoded[25..33].fill(0);
        assert!(matches!(
            decode_protocol_frame(&encoded),
            Err(ProtocolError::InvalidTerminal(_))
        ));
    }

    #[test]
    fn choose_tree_updates_use_the_reliable_control_lane() {
        let message = ProtocolMessage::Event(Event {
            sequence: 106,
            payload: EventPayload::ChooseTree {
                state: Some(crate::ChooseTreeState {
                    items: vec![crate::ChooseTreeItem {
                        label: "dev".to_owned(),
                        detail: "2 windows".to_owned(),
                        target: crate::ChooseTreeTarget::Session(crate::SessionId(2)),
                        depth: 0,
                        flags: crate::ChooseTreeItem::ACTIVE,
                        pane_kind: None,
                        key: "0".to_owned(),
                    }],
                    search: None,
                    selected: 0,
                    kind: crate::ChooseTreeKind::Windows,
                    filter_no_matches: true,
                }),
            },
        });
        let encoded = encode_protocol_message(&message).expect("encode");
        assert_eq!(encoded[4], Lane::Control as u8);
        assert_eq!(decode_protocol_frame(&encoded).expect("decode"), message);

        let delta = ProtocolMessage::Event(Event {
            sequence: 107,
            payload: EventPayload::ChooseTreeUpdate {
                search: Some(crate::ChooseTreeSearchState {
                    query: "dev".to_owned(),
                    reverse: false,
                }),
                selected: 3,
            },
        });
        let encoded = encode_protocol_message(&delta).expect("encode delta");
        assert_eq!(encoded[4], Lane::Control as u8);
        assert_eq!(
            decode_protocol_frame(&encoded).expect("decode delta"),
            delta
        );
    }

    #[test]
    fn choose_buffer_updates_use_the_reliable_control_lane() {
        let message = ProtocolMessage::Event(Event {
            sequence: 108,
            payload: EventPayload::ChooseBuffer {
                state: Some(crate::ChooseBufferState {
                    items: vec![crate::ChooseBufferItem {
                        name: "buffer0001".to_owned(),
                        preview: "hello".to_owned(),
                        size_bytes: 5,
                        created_unix_seconds: 42,
                        key: String::new(),
                    }],
                    search: None,
                    selected: 0,
                    filter_no_matches: true,
                }),
            },
        });
        let encoded = encode_protocol_message(&message).expect("encode");
        assert_eq!(encoded[4], Lane::Control as u8);
        assert_eq!(decode_protocol_frame(&encoded).expect("decode"), message);

        let delta = ProtocolMessage::Event(Event {
            sequence: 109,
            payload: EventPayload::ChooseBufferUpdate {
                search: Some(crate::ChooseBufferSearchState {
                    query: "hello".to_owned(),
                    reverse: false,
                }),
                selected: 0,
            },
        });
        let encoded = encode_protocol_message(&delta).expect("encode delta");
        assert_eq!(encoded[4], Lane::Control as u8);
        assert_eq!(
            decode_protocol_frame(&encoded).expect("decode delta"),
            delta
        );
    }

    #[test]
    fn display_panes_updates_use_the_reliable_control_lane() {
        let message = ProtocolMessage::Event(Event {
            sequence: 108,
            payload: EventPayload::DisplayPanes {
                state: Some(crate::DisplayPanesState {
                    window: crate::WindowId(4),
                    duration_ms: 1_000,
                    indicators: vec![crate::PaneIndicator {
                        pane: PaneId(12),
                        index: 0,
                        select_key: b'0',
                        flags: crate::PaneIndicator::ACTIVE,
                        label: String::new(),
                    }],
                }),
            },
        });
        let encoded = encode_protocol_message(&message).expect("encode");
        assert_eq!(encoded[4], Lane::Control as u8);
        assert_eq!(decode_protocol_frame(&encoded).expect("decode"), message);
    }

    #[test]
    fn terminal_lane_rejects_bad_cell_style_ids() {
        let mut viewport = TerminalViewport::blank(1, 1, SessionStatus::Running);
        Arc::make_mut(&mut viewport.cells)[0] =
            PackedCell::new(u32::from('x'), 8, CellWidth::Narrow);
        let message = ProtocolMessage::Event(Event {
            sequence: 1,
            payload: EventPayload::TerminalViewport {
                pane: PaneId(1),
                viewport,
            },
        });
        assert!(matches!(
            encode_protocol_message(&message),
            Err(ProtocolError::InvalidTerminal(_))
        ));
    }

    #[test]
    fn terminal_lane_rejects_noncanonical_patch_rows() {
        let previous = TerminalViewport::blank(1, 3, SessionStatus::Running);
        let mut current = previous.clone();
        current.generation = 1;
        current.view_generation = 1;
        let cells = Arc::make_mut(&mut current.cells);
        cells[0] = PackedCell::new(u32::from('a'), 0, CellWidth::Narrow);
        cells[2] = PackedCell::new(u32::from('c'), 0, CellWidth::Narrow);
        let mut patch = TerminalViewport::diff(&previous, &current).expect("compatible viewport");
        let reversed = patch
            .changed_rows
            .row_indices()
            .iter()
            .rev()
            .copied()
            .collect::<TerminalPatchRowIndices>();
        let cells = patch.changed_rows.cells().to_vec();
        patch.changed_rows = TerminalPatchRows::from_flat(reversed, cells);
        let message = ProtocolMessage::Event(Event {
            sequence: 2,
            payload: EventPayload::TerminalPatch {
                pane: PaneId(1),
                patch,
            },
        });

        assert!(matches!(
            encode_protocol_message(&message),
            Err(ProtocolError::InvalidTerminal(_))
        ));
    }

    #[test]
    fn terminal_lane_round_trips_and_applies_row_patches() {
        let mut previous = TerminalViewport::blank(2, 3, SessionStatus::Running);
        previous.generation = 4;
        let cells = Arc::make_mut(&mut previous.cells);
        for (row, glyph) in ['a', 'b', 'c'].into_iter().enumerate() {
            cells[row * 2..row * 2 + 2].fill(PackedCell::new(
                u32::from(glyph),
                0,
                CellWidth::Narrow,
            ));
        }
        let mut current = previous.clone();
        current.generation = 5;
        current.view_generation = 5;
        current.set_working_directory(Some(Arc::from("file://localhost/tmp/patch-fixture")));
        current.set_hovered_uri(Some(Arc::from("ssh://example.com")));
        let cells = Arc::make_mut(&mut current.cells);
        cells.copy_within(2..6, 0);
        cells[4..6].fill(PackedCell::new(u32::from('d'), 0, CellWidth::Narrow));
        let mut styles = current.styles().to_vec();
        styles.push(PackedStyle::new(
            Color::rgb(0x33, 0x88, 0xcc),
            current.background,
            None,
            zz_terminal::ATTR_ITALIC,
            UnderlineStyle::None,
        ));
        let grapheme = "e\u{301}";
        let dictionary = Arc::make_mut(&mut current.dictionary);
        dictionary.styles = styles.into();
        dictionary.grapheme_bytes = grapheme.as_bytes().into();
        dictionary.grapheme_offsets =
            Arc::from([0, u32::try_from(grapheme.len()).expect("small fixture")]);
        Arc::make_mut(&mut current.cells)[4] =
            PackedCell::new(GRAPHEME_TABLE_BIT, 1, CellWidth::Narrow);
        current.kitty_placements = Arc::from([KittyPlacement {
            image_id: 42,
            image_generation: 3,
            layer: KittyLayer::AboveText,
            viewport_col: 1,
            viewport_row: 1,
            absolute_row: 1,
            cell_offset_x: 0,
            cell_offset_y: 1,
            grid_cols: 1,
            grid_rows: 2,
            pixel_width: 8,
            pixel_height: 16,
            source_rect: None,
        }]);
        let patch = TerminalViewport::diff(&previous, &current).expect("compatible viewport");
        let expected_payload_len = patch_payload_capacity(
            &patch,
            patch.title().len(),
            patch.working_directory().map_or(0, str::len),
            patch.hovered_uri().map_or(0, str::len),
        )
        .expect("payload capacity");
        let message = ProtocolMessage::Event(Event {
            sequence: 101,
            payload: EventPayload::TerminalPatch {
                pane: PaneId(7),
                patch,
            },
        });

        let encoded = encode_protocol_message(&message).expect("encode");
        assert_eq!(encoded[4], Lane::Terminal as u8);
        let (_, payload) = decode_enveloped(&encoded).expect("decode envelope");
        assert_eq!(payload.len(), expected_payload_len);
        let decoded = decode_protocol_frame(&encoded).expect("decode");
        assert_eq!(decoded, message);
        let ProtocolMessage::Event(Event {
            payload: EventPayload::TerminalPatch { patch, .. },
            ..
        }) = decoded
        else {
            panic!("expected a terminal patch");
        };
        previous.apply_patch(patch).expect("apply decoded patch");
        assert_eq!(previous, current);
    }

    #[test]
    fn terminal_lane_rejects_kitty_placement_caps_and_malformed_records() {
        let viewport = TerminalViewport::blank(1, 1, SessionStatus::Running);
        let message = ProtocolMessage::Event(Event {
            sequence: 1,
            payload: EventPayload::TerminalViewport {
                pane: PaneId(1),
                viewport,
            },
        });
        let mut capped = encode_protocol_message(&message).expect("encode empty placements");
        let count = u32::try_from(MAX_KITTY_PLACEMENTS + 1).expect("cap fits u32");
        let count_offset = capped.len() - size_of::<u32>();
        capped[count_offset..].copy_from_slice(&count.to_le_bytes());
        assert!(matches!(
            decode_protocol_frame(&capped),
            Err(ProtocolError::InvalidTerminal(_))
        ));

        let mut viewport = TerminalViewport::blank(1, 1, SessionStatus::Running);
        viewport.kitty_placements = Arc::from([KittyPlacement {
            image_id: 1,
            image_generation: 1,
            layer: KittyLayer::AboveText,
            viewport_col: 0,
            viewport_row: 0,
            absolute_row: 0,
            cell_offset_x: 0,
            cell_offset_y: 0,
            grid_cols: 1,
            grid_rows: 1,
            pixel_width: 1,
            pixel_height: 1,
            source_rect: None,
        }]);
        let mut malformed = encode_protocol_message(&ProtocolMessage::Event(Event {
            sequence: 2,
            payload: EventPayload::TerminalViewport {
                pane: PaneId(1),
                viewport,
            },
        }))
        .expect("encode placement");
        let record_offset = malformed.len() - KITTY_PLACEMENT_WIRE_BYTES;
        malformed[record_offset + 12] = 9;
        assert!(matches!(
            decode_protocol_frame(&malformed),
            Err(ProtocolError::InvalidTerminal(_))
        ));
    }

    #[test]
    fn kitty_image_controls_round_trip_and_enforce_byte_budgets() {
        let pane = PaneId(7);
        let messages = [
            ProtocolMessage::Event(Event {
                sequence: 1,
                payload: EventPayload::KittyImageBegin {
                    pane,
                    image_id: 4,
                    generation: 2,
                    width: 2,
                    height: 1,
                    total_bytes: 8,
                },
            }),
            ProtocolMessage::Event(Event {
                sequence: 2,
                payload: EventPayload::KittyImageChunk {
                    pane,
                    image_id: 4,
                    generation: 2,
                    bytes: vec![0; 8],
                },
            }),
            ProtocolMessage::Event(Event {
                sequence: 3,
                payload: EventPayload::KittyImagesRemoved {
                    pane,
                    image_ids: vec![4],
                },
            }),
        ];
        for message in messages {
            let frame = encode_protocol_message(&message).expect("encode Kitty control");
            assert_eq!(frame[4], Lane::Control as u8);
            assert_eq!(
                decode_protocol_frame(&frame).expect("decode Kitty control"),
                message
            );
        }

        let malformed = ProtocolMessage::Event(Event {
            sequence: 4,
            payload: EventPayload::KittyImageBegin {
                pane,
                image_id: 4,
                generation: 2,
                width: 2,
                height: 2,
                total_bytes: 8,
            },
        });
        assert!(matches!(
            encode_protocol_message(&malformed),
            Err(ProtocolError::InvalidTerminal(_))
        ));
        let oversized = ProtocolMessage::Event(Event {
            sequence: 5,
            payload: EventPayload::KittyImageChunk {
                pane,
                image_id: 4,
                generation: 2,
                bytes: vec![0; MAX_KITTY_IMAGE_CHUNK_BYTES + 1],
            },
        });
        assert!(matches!(
            encode_protocol_message(&oversized),
            Err(ProtocolError::InvalidTerminal(_))
        ));
    }
}
