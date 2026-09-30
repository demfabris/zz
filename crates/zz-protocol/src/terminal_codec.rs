use std::io::{Read, Write};

use serde::ser::SerializeSeq;
use zz_terminal::{TerminalPatchRef, TerminalViewport};

use crate::message::{
    MAX_CLIENT_ENVIRONMENT_BYTES, MAX_CLIENT_ENVIRONMENT_ENTRIES,
    MAX_CLIENT_ENVIRONMENT_ENTRY_BYTES, MAX_CLIENT_WORKING_DIRECTORY_BYTES, MAX_KITTY_IMAGE_BYTES,
    MAX_KITTY_IMAGE_CHUNK_BYTES, MAX_KITTY_IMAGE_REMOVALS, MAX_STARTUP_CONFIG_CAUSE_BYTES,
    MAX_STARTUP_CONFIG_CAUSES, MAX_STARTUP_CONFIG_CAUSES_BYTES, client_environment_is_valid,
};
use crate::pane_frame::{
    COMMAND_OUTPUT_VIEWPORT, FULL_VIEWPORT, HISTORY_CHUNK, HistoryChunkRef, PatchTail,
    VIEWPORT_PATCH, decode_history_frame, decode_patch_frame, decode_viewport_frame,
    encode_history_frame, encode_patch_frame, encode_patch_ref_frame, encode_viewport_frame,
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

const CONTROL_PAYLOAD_RESERVE: usize = 256;

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

pub fn encode_terminal_viewport_event(
    pane: PaneId,
    sequence: u64,
    viewport: &TerminalViewport,
) -> Result<Vec<u8>, ProtocolError> {
    let mut output = Vec::new();
    encode_terminal_viewport_event_into(pane, sequence, viewport, &mut output)?;
    Ok(output)
}

pub fn encode_terminal_viewport_event_into(
    pane: PaneId,
    sequence: u64,
    viewport: &TerminalViewport,
    output: &mut Vec<u8>,
) -> Result<(), ProtocolError> {
    output.clear();
    let result = encode_viewport_frame(output, FULL_VIEWPORT, pane, sequence, None, viewport);
    if result.is_err() {
        output.clear();
    }
    result
}

pub fn encode_terminal_patch_event_into(
    pane: PaneId,
    sequence: u64,
    patch: &TerminalPatchRef<'_>,
    tail: &mut PatchTail,
    output: &mut Vec<u8>,
) -> Result<(), ProtocolError> {
    output.clear();
    let result = encode_patch_ref_frame(output, pane, sequence, patch, tail);
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

struct BorrowedBatchFrames<'a, T>(&'a [T]);

struct BorrowedBatchFrame<'a>(&'a [u8]);

impl serde::Serialize for BorrowedBatchFrame<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_bytes(self.0)
    }
}

impl<T: AsRef<[u8]>> serde::Serialize for BorrowedBatchFrames<'_, T> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut sequence = serializer.serialize_seq(Some(self.0.len()))?;
        for frame in self.0 {
            sequence.serialize_element(&BorrowedBatchFrame(frame.as_ref()))?;
        }
        sequence.end()
    }
}

pub fn batch_frames_encoded_len<T: AsRef<[u8]>>(
    sequence: u64,
    frames: &[T],
) -> Result<usize, ProtocolError> {
    if frames.len() > crate::MAX_BATCH_FRAMES {
        return Err(ProtocolError::InvalidServerHello(
            "batch contains too many frames".to_owned(),
        ));
    }
    frames.iter().try_fold(0_usize, |total, frame| {
        let total = total
            .checked_add(frame.as_ref().len())
            .ok_or(ProtocolError::FrameTooLarge(usize::MAX))?;
        if total > MAX_ENCODED_FRAME_BYTES {
            return Err(ProtocolError::FrameTooLarge(total));
        }
        Ok(total)
    })?;
    let header = postcard::experimental::serialized_size(&ProtocolMessage::Batch(crate::Batch {
        sequence,
        frames: Vec::new(),
    }))
    .map_err(ProtocolError::Encode)?;
    let frames = postcard::experimental::serialized_size(&BorrowedBatchFrames(frames))
        .map_err(ProtocolError::Encode)?;
    let payload = (header - 1)
        .checked_add(frames)
        .ok_or(ProtocolError::FrameTooLarge(usize::MAX))?;
    crate::framing::enveloped_capacity(payload)
}

pub fn encode_batch_frames_into<T: AsRef<[u8]>>(
    sequence: u64,
    frames: &[T],
    output: &mut Vec<u8>,
) -> Result<(), ProtocolError> {
    output.clear();
    if frames.len() > crate::MAX_BATCH_FRAMES {
        return Err(ProtocolError::InvalidServerHello(
            "batch contains too many frames".to_owned(),
        ));
    }
    encode_protocol_message_into(
        &ProtocolMessage::Batch(crate::Batch {
            sequence,
            frames: Vec::new(),
        }),
        output,
    )?;
    let _ = output.pop();
    let result = serialize_control_into(&BorrowedBatchFrames(frames), output)
        .and_then(|()| finish_enveloped_in_place(output));
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
    if let ProtocolMessage::Event(Event { sequence, payload }) = message {
        match payload {
            EventPayload::TerminalViewport { pane, viewport } => {
                return encode_viewport_frame(
                    output,
                    FULL_VIEWPORT,
                    *pane,
                    *sequence,
                    None,
                    viewport,
                );
            }
            EventPayload::TerminalPatch { pane, patch } => {
                return encode_patch_frame(output, *pane, *sequence, patch);
            }
            EventPayload::CommandOutput {
                pane,
                output_id,
                viewport: Some(viewport),
            } => {
                return encode_viewport_frame(
                    output,
                    COMMAND_OUTPUT_VIEWPORT,
                    *pane,
                    *sequence,
                    Some(*output_id),
                    viewport,
                );
            }
            EventPayload::HistoryChunk {
                pane,
                start,
                total,
                offset,
                columns,
                rows,
                dictionary,
            } => {
                return encode_history_frame(
                    output,
                    *pane,
                    *sequence,
                    &HistoryChunkRef {
                        start: *start,
                        total: *total,
                        offset: *offset,
                        columns: *columns,
                        rows,
                        dictionary,
                    },
                );
            }
            _ => {}
        }
    }

    begin_enveloped_into(output, Lane::Control, CONTROL_PAYLOAD_RESERVE)?;
    serialize_control_into(message, output)?;
    finish_enveloped_in_place(output)
}

fn serialize_control_into(
    value: &impl serde::Serialize,
    output: &mut Vec<u8>,
) -> Result<(), ProtocolError> {
    match postcard::serialize_with_flavor(
        value,
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
    Ok(())
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
                let (pane, sequence, _, viewport) = decode_viewport_frame(payload)?;
                Ok(ProtocolMessage::Event(Event {
                    sequence,
                    payload: EventPayload::TerminalViewport { pane, viewport },
                }))
            }
            Some(VIEWPORT_PATCH) => {
                let (pane, sequence, patch) = decode_patch_frame(payload)?;
                Ok(ProtocolMessage::Event(Event {
                    sequence,
                    payload: EventPayload::TerminalPatch { pane, patch },
                }))
            }
            Some(COMMAND_OUTPUT_VIEWPORT) => {
                let (pane, sequence, output_id, viewport) = decode_viewport_frame(payload)?;
                let Some(output_id) = output_id else {
                    return Err(ProtocolError::InvalidTerminal(
                        "command output viewport is missing its output ID".to_owned(),
                    ));
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
            Some(HISTORY_CHUNK) => {
                let chunk = decode_history_frame(payload)?;
                Ok(ProtocolMessage::Event(Event {
                    sequence: chunk.sequence,
                    payload: EventPayload::HistoryChunk {
                        pane: chunk.pane,
                        start: chunk.start,
                        total: chunk.total,
                        offset: chunk.offset,
                        columns: chunk.columns,
                        rows: chunk.rows,
                        dictionary: chunk.dictionary,
                    },
                }))
            }
            _ => Err(ProtocolError::InvalidTerminal(
                "unknown terminal update type".to_owned(),
            )),
        },
    }
}

fn validate_control_message(message: &ProtocolMessage) -> Result<(), ProtocolError> {
    if let ProtocolMessage::Event(Event {
        payload: EventPayload::ControlCommandStarted { canonical_name, .. },
        ..
    }) = message
        && canonical_name
            .as_ref()
            .is_some_and(|name| name.len() > MAX_GUI_TEXT_BYTES)
    {
        return Err(ProtocolError::InvalidServerHello(
            "control command name is too large".to_owned(),
        ));
    }
    if let ProtocolMessage::Hello(hello) = message {
        validate_control_message(&ProtocolMessage::ClientHello(hello.client.clone()))?;
        if !hello.environment.is_valid() {
            return Err(ProtocolError::InvalidClientHello(
                "invalid hello environment blob".to_owned(),
            ));
        }
    }
    if let ProtocolMessage::Welcome(welcome) = message {
        if welcome.protocol_version != PROTOCOL_VERSION {
            return Err(ProtocolError::VersionMismatch {
                expected: PROTOCOL_VERSION,
                received: welcome.protocol_version,
            });
        }
        for capability in [crate::CONTROL_CAPABILITY, crate::PANE_FRAME_CAPABILITY] {
            if !welcome.has_capability(capability) {
                return Err(ProtocolError::InvalidServerHello(format!(
                    "daemon does not support {capability}"
                )));
            }
        }
    }
    if let ProtocolMessage::Batch(batch) = message
        && batch.frames.len() > crate::MAX_BATCH_FRAMES
    {
        return Err(ProtocolError::InvalidServerHello(
            "batch contains too many frames".to_owned(),
        ));
    }
    if let ProtocolMessage::Event(Event {
        payload: EventPayload::MuxOptionsPatched { options },
        ..
    }) = message
    {
        options
            .validate_partial()
            .map_err(|error| ProtocolError::InvalidServerHello(error.to_owned()))?;
    }
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
        if hello
            .working_directory
            .as_ref()
            .is_some_and(|path| path.len() > MAX_CLIENT_WORKING_DIRECTORY_BYTES)
        {
            return Err(ProtocolError::InvalidClientHello(format!(
                "working directory must be at most {MAX_CLIENT_WORKING_DIRECTORY_BYTES} bytes"
            )));
        }
        if !client_environment_is_valid(&hello.environment) {
            return Err(ProtocolError::InvalidClientHello(format!(
                "environment must contain at most {MAX_CLIENT_ENVIRONMENT_ENTRIES} valid \
                 NAME=VALUE entries of at most {MAX_CLIENT_ENVIRONMENT_ENTRY_BYTES} bytes each \
                 and {MAX_CLIENT_ENVIRONMENT_BYTES} bytes total"
            )));
        }
    }
    if let ProtocolMessage::Exec(request) = message
        && !request.is_valid()
    {
        return Err(ProtocolError::InvalidClientHello(format!(
            "exec request facts must stay within the working directory, tty and environment \
             limits ({MAX_CLIENT_ENVIRONMENT_ENTRIES} entries of at most \
             {MAX_CLIENT_ENVIRONMENT_ENTRY_BYTES} bytes, {MAX_CLIENT_ENVIRONMENT_BYTES} bytes \
             total)"
        )));
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
        && image_ids.len() > MAX_KITTY_IMAGE_REMOVALS
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

fn invalid<T>(message: &str) -> Result<T, ProtocolError> {
    Err(ProtocolError::InvalidTerminal(message.to_owned()))
}

#[cfg(test)]
mod tests {
    use crate::message::MAX_MUX_OPTION_VALUE_BYTES;
    use crate::{
        MAX_AGENT_PERMISSION_BYTES, MAX_AGENT_STATE_BLOB_BYTES, MuxOptionKey, MuxOptionSource,
        MuxOptions, RawText,
    };
    use std::sync::Arc;

    use zz_terminal::{
        AppearanceConfigKey, AppearanceProvenance, AppearanceSource, CellWidth, Color, CursorStyle,
        PackedCell, SearchDirection, SessionStatus, TerminalAppearance, TerminalMode,
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
    fn borrowed_batch_encoder_matches_owned_frames_and_reuses_output() {
        let control = encode_protocol_message(&ProtocolMessage::TreeSync).expect("control");
        let terminal = encode_terminal_viewport_event(
            PaneId(7),
            128,
            &TerminalViewport::blank(80, 24, SessionStatus::Running),
        )
        .expect("terminal");
        let shared: Arc<[u8]> = Arc::from((0_u8..=255).collect::<Vec<_>>());
        let mixed = [
            std::borrow::Cow::Owned(control.clone()),
            std::borrow::Cow::Borrowed(shared.as_ref()),
            std::borrow::Cow::Owned(terminal),
        ];
        let mut output = Vec::with_capacity(128 * 1024);
        let allocation = output.as_ptr();
        for sequence in [0, 127, 128, u64::MAX] {
            let empty = encode_protocol_message(&ProtocolMessage::Batch(crate::Batch {
                sequence,
                frames: Vec::new(),
            }))
            .expect("empty batch");
            assert_eq!(empty.last(), Some(&0));
            encode_batch_frames_into::<Vec<u8>>(sequence, &[], &mut output)
                .expect("borrowed empty");
            assert_eq!(output, empty);
            assert_eq!(
                batch_frames_encoded_len::<Vec<u8>>(sequence, &[]).expect("empty length"),
                output.len()
            );
            encode_batch_frames_into(sequence, &mixed, &mut output).expect("mixed batch");
            assert_eq!(
                batch_frames_encoded_len(sequence, &mixed).expect("mixed length"),
                output.len()
            );
            let owned = ProtocolMessage::Batch(crate::Batch {
                sequence,
                frames: mixed.iter().map(|frame| frame.as_ref().to_vec()).collect(),
            });
            assert_eq!(
                output,
                encode_protocol_message(&owned).expect("owned batch")
            );
            assert_eq!(decode_protocol_frame(&output).expect("decode batch"), owned);
            for count in [1, 127, 128, crate::MAX_BATCH_FRAMES] {
                let frames = vec![control.clone(); count];
                encode_batch_frames_into(sequence, &frames, &mut output).expect("many frames");
                assert_eq!(
                    batch_frames_encoded_len(sequence, &frames).expect("many length"),
                    output.len()
                );
                assert_eq!(
                    output,
                    encode_protocol_message(&ProtocolMessage::Batch(crate::Batch {
                        sequence,
                        frames,
                    }))
                    .expect("owned many frames")
                );
            }
            assert_eq!(output.as_ptr(), allocation);
        }
    }

    #[test]
    fn borrowed_batch_encoder_keeps_frame_and_count_bounds() {
        let mut output = Vec::with_capacity(256);
        output.push(42);
        let allocation = output.as_ptr();
        let too_many = vec![&[][..]; crate::MAX_BATCH_FRAMES + 1];
        assert!(matches!(
            encode_batch_frames_into(0, &too_many, &mut output),
            Err(ProtocolError::InvalidServerHello(_))
        ));
        assert!(output.is_empty());
        assert_eq!(output.as_ptr(), allocation);
        assert!(matches!(
            batch_frames_encoded_len(0, &too_many),
            Err(ProtocolError::InvalidServerHello(_))
        ));
        let body = vec![0_u8; 64 * 1024];
        let frames = vec![body.as_slice(); crate::MAX_FRAME_BYTES / body.len()];
        assert!(matches!(
            batch_frames_encoded_len(0, &frames),
            Err(ProtocolError::FrameTooLarge(_))
        ));
        let repeated = vec![body.as_slice(); crate::MAX_BATCH_FRAMES];
        assert!(matches!(
            batch_frames_encoded_len(u64::MAX, &repeated),
            Err(ProtocolError::FrameTooLarge(_))
        ));
        assert!(matches!(
            encode_batch_frames_into(0, &frames, &mut output),
            Err(ProtocolError::FrameTooLarge(_))
        ));
        assert!(output.is_empty());
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
            MAX_STARTUP_CONFIG_CAUSES_BYTES
                / MAX_STARTUP_CONFIG_CAUSE_BYTES
                + 1
        ]);

        let count_boundary = message(vec![String::new(); MAX_STARTUP_CONFIG_CAUSES]);
        let frame = encode_protocol_message(&count_boundary).expect("encode count boundary");
        assert_eq!(
            decode_protocol_frame(&frame).expect("decode count boundary"),
            count_boundary
        );

        let aggregate_boundary = message(vec![
            "x".repeat(MAX_STARTUP_CONFIG_CAUSE_BYTES);
            MAX_STARTUP_CONFIG_CAUSES_BYTES
                / MAX_STARTUP_CONFIG_CAUSE_BYTES
        ]);
        let frame =
            encode_protocol_message(&aggregate_boundary).expect("encode aggregate boundary");
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
                environment: Vec::new(),
                process_id: 13,
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
                working_directory: crate::ClientPath::from_path(&std::path::PathBuf::from(
                    "x".repeat(length),
                )),
                environment: Vec::new(),
                process_id: 13,
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
    fn client_hello_environment_enforces_count_item_aggregate_and_syntax_bounds() {
        assert_eq!(MAX_CLIENT_ENVIRONMENT_ENTRIES, 4096);
        assert_eq!(MAX_CLIENT_ENVIRONMENT_ENTRY_BYTES, 16_367);
        assert_eq!(MAX_CLIENT_ENVIRONMENT_BYTES, 4 * 1024 * 1024);

        let entry = |length: usize| {
            assert!(length >= 2);
            format!("N={}", "x".repeat(length - 2))
        };
        let message = |environment| {
            ProtocolMessage::ClientHello(crate::ClientHello {
                protocol_version: PROTOCOL_VERSION,
                client_instance_id: crate::ClientInstanceId(13),
                kind: crate::ClientKind::Command,
                device_name: None,
                capabilities: Vec::new(),
                color_scheme: None,
                origin: None,
                working_directory: None,
                environment,
                process_id: 13,
            })
        };
        let rejected = |environment| {
            let invalid = message(environment);
            assert!(matches!(
                encode_protocol_message(&invalid),
                Err(ProtocolError::InvalidClientHello(_))
            ));
            let payload = postcard::to_stdvec(&invalid).expect("serialize invalid environment");
            let frame = crate::framing::encode_enveloped(Lane::Control, &payload)
                .expect("envelope invalid environment");
            assert!(matches!(
                decode_protocol_frame(&frame),
                Err(ProtocolError::Decode(_))
            ));
        };

        rejected(vec![
            RawText::from("N=");
            MAX_CLIENT_ENVIRONMENT_ENTRIES + 1
        ]);
        rejected(vec![entry(MAX_CLIENT_ENVIRONMENT_ENTRY_BYTES + 1).into()]);
        for malformed in ["", "NAME", "=value", "NA\0ME=value", "NAME=va\0lue"] {
            rejected(vec![malformed.into()]);
        }

        let count_boundary = message(vec![RawText::from("N="); MAX_CLIENT_ENVIRONMENT_ENTRIES]);
        let frame = encode_protocol_message(&count_boundary).expect("encode count boundary");
        assert_eq!(
            decode_protocol_frame(&frame).expect("decode count boundary"),
            count_boundary
        );

        let full_entries = MAX_CLIENT_ENVIRONMENT_BYTES / MAX_CLIENT_ENVIRONMENT_ENTRY_BYTES;
        let remainder = MAX_CLIENT_ENVIRONMENT_BYTES % MAX_CLIENT_ENVIRONMENT_ENTRY_BYTES;
        let mut environment: Vec<RawText> =
            vec![entry(MAX_CLIENT_ENVIRONMENT_ENTRY_BYTES).into(); full_entries];
        environment.push(entry(remainder).into());
        let aggregate_boundary = message(environment.clone());
        let frame =
            encode_protocol_message(&aggregate_boundary).expect("encode aggregate boundary");
        assert_eq!(
            decode_protocol_frame(&frame).expect("decode aggregate boundary"),
            aggregate_boundary
        );
        environment.push("N=x".into());
        rejected(environment);

        let equals_in_value = message(vec!["TOKEN=left=right".into()]);
        let frame = encode_protocol_message(&equals_in_value).expect("encode equals in value");
        assert_eq!(
            decode_protocol_frame(&frame).expect("decode equals in value"),
            equals_in_value
        );
    }

    #[test]
    fn client_hello_debug_omits_environment_contents() {
        let hello = crate::ClientHello {
            protocol_version: PROTOCOL_VERSION,
            client_instance_id: crate::ClientInstanceId(13),
            kind: crate::ClientKind::Command,
            device_name: None,
            capabilities: Vec::new(),
            color_scheme: None,
            origin: None,
            working_directory: None,
            environment: vec!["ZZ_SECRET=do-not-print".into()],
            process_id: 13,
        };
        let debug = format!("{hello:?}");
        assert!(debug.contains("environment_entries: 1"));
        assert!(debug.contains("process_id: 13"));
        assert!(!debug.contains("ZZ_SECRET"));
        assert!(!debug.contains("do-not-print"));
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
    fn terminal_preview_toggle_round_trips_on_the_control_lane() {
        for enabled in [false, true] {
            let message = ProtocolMessage::SetTerminalPreview { enabled };
            let frame = encode_protocol_message(&message).expect("encode terminal preview toggle");
            assert_eq!(frame[4], Lane::Control as u8);
            assert_eq!(&frame[6..8], &PROTOCOL_VERSION.to_le_bytes());
            assert_eq!(
                decode_protocol_frame(&frame).expect("decode terminal preview toggle"),
                message
            );
        }
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
            working_directory: crate::ClientPath::from_path(std::path::Path::new(
                "/tmp/client-fixture",
            )),
            environment: Vec::new(),
            process_id: 13,
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
                    pane: None,
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
        assert_eq!(encoded[8], COMMAND_OUTPUT_VIEWPORT);
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
        assert_eq!(&encoded[9..12], &[12, 108, 9]);
        encoded[11] = 0;
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
                        text: "<dev>".to_owned(),
                    }],
                    search: None,
                    selected: 0,
                    kind: crate::ChooseTreeKind::Windows,
                    filter_no_matches: true,
                    prompt: String::new(),
                    help: false,
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
                        text: String::new(),
                        tagged: false,
                    }],
                    search: None,
                    selected: 0,
                    filter_no_matches: true,
                    help: false,
                    prompt: String::new(),
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
                    colour: None,
                    active_colour: None,
                }),
            },
        });
        let encoded = encode_protocol_message(&message).expect("encode");
        assert_eq!(encoded[4], Lane::Control as u8);
        assert_eq!(decode_protocol_frame(&encoded).expect("decode"), message);
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
        for count in [MAX_KITTY_IMAGE_REMOVALS, MAX_KITTY_IMAGE_REMOVALS + 1] {
            let message = ProtocolMessage::Event(Event {
                sequence: 6,
                payload: EventPayload::KittyImagesRemoved {
                    pane,
                    image_ids: (1..=count).map(|id| u32::try_from(id).unwrap()).collect(),
                },
            });
            if count == MAX_KITTY_IMAGE_REMOVALS {
                let frame = encode_protocol_message(&message).expect("encode removal limit");
                assert_eq!(
                    decode_protocol_frame(&frame).expect("decode removals"),
                    message
                );
            } else {
                assert!(matches!(
                    encode_protocol_message(&message),
                    Err(ProtocolError::InvalidTerminal(error))
                        if error == "kitty image removal list exceeds its wire limit"
                ));
            }
        }
    }
}
