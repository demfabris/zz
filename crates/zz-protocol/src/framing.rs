use std::{borrow::Cow, io::Read};

use thiserror::Error;

use crate::PROTOCOL_VERSION;

pub const MAX_FRAME_BYTES: usize = 64 * 1024 * 1024;
const ENVELOPE_BYTES: usize = 4;
const LENGTH_PREFIX_BYTES: usize = 4;
const FRAME_HEADER_BYTES: usize = LENGTH_PREFIX_BYTES + ENVELOPE_BYTES;
pub const MAX_ENCODED_FRAME_BYTES: usize = LENGTH_PREFIX_BYTES + MAX_FRAME_BYTES;

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Lane {
    Control = 0,
    Terminal = 1,
}

impl TryFrom<u8> for Lane {
    type Error = ProtocolError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Control),
            1 => Ok(Self::Terminal),
            _ => Err(ProtocolError::UnsupportedLane(value)),
        }
    }
}

#[derive(Debug, Error)]
pub enum ProtocolError {
    #[error("protocol frame is truncated")]
    Truncated,
    #[error("protocol frame is too large: {0} bytes")]
    FrameTooLarge(usize),
    #[error("protocol frame length does not match payload")]
    LengthMismatch,
    #[error("unsupported protocol lane: {0}")]
    UnsupportedLane(u8),
    #[error("unsupported protocol envelope flags: {0:#04x}")]
    UnsupportedFlags(u8),
    #[error("protocol version mismatch: expected {expected}, received {received}")]
    VersionMismatch { expected: u16, received: u16 },
    #[error("protocol encoding failed: {0}")]
    Encode(#[source] postcard::Error),
    #[error("protocol decoding failed: {0}")]
    Decode(#[source] postcard::Error),
    #[error("invalid terminal viewport payload: {0}")]
    InvalidTerminal(String),
    #[error("invalid terminal appearance payload: {0}")]
    InvalidAppearance(String),
    #[error("invalid server hello payload: {0}")]
    InvalidServerHello(String),
    #[error("invalid client hello payload: {0}")]
    InvalidClientHello(String),
    #[error("invalid configuration override payload: {0}")]
    InvalidConfigOverrides(String),
    #[error("invalid GUI request payload: {0}")]
    InvalidGuiRequest(String),
    #[error("invalid paste upload payload: {0}")]
    InvalidPasteUpload(String),
    #[error("protocol I/O failed: {0}")]
    Io(#[from] std::io::Error),
}

#[cfg(test)]
pub(crate) fn encode_enveloped(lane: Lane, payload: &[u8]) -> Result<Vec<u8>, ProtocolError> {
    let mut frame = Vec::new();
    begin_enveloped_into(&mut frame, lane, payload.len())?;
    frame.extend_from_slice(payload);
    finish_enveloped_in_place(&mut frame)?;
    Ok(frame)
}

pub(crate) fn begin_enveloped_into(
    frame: &mut Vec<u8>,
    lane: Lane,
    payload_capacity: usize,
) -> Result<(), ProtocolError> {
    let capacity = enveloped_capacity(payload_capacity)?;
    frame.clear();
    if frame.capacity() < capacity {
        frame.reserve_exact(capacity);
    }
    write_envelope_header(frame, lane);
    Ok(())
}

pub(crate) fn enveloped_capacity(payload_capacity: usize) -> Result<usize, ProtocolError> {
    let following_capacity = ENVELOPE_BYTES
        .checked_add(payload_capacity)
        .ok_or(ProtocolError::FrameTooLarge(usize::MAX))?;
    if following_capacity > MAX_FRAME_BYTES {
        return Err(ProtocolError::FrameTooLarge(following_capacity));
    }
    FRAME_HEADER_BYTES
        .checked_add(payload_capacity)
        .ok_or(ProtocolError::FrameTooLarge(usize::MAX))
}

fn write_envelope_header(frame: &mut Vec<u8>, lane: Lane) {
    frame.extend_from_slice(&[0; LENGTH_PREFIX_BYTES]);
    frame.push(lane as u8);
    frame.push(0); // flags, reserved for future envelope extensions
    frame.extend_from_slice(&PROTOCOL_VERSION.to_le_bytes());
}

pub(crate) fn finish_enveloped_in_place(frame: &mut [u8]) -> Result<(), ProtocolError> {
    let following = frame
        .len()
        .checked_sub(LENGTH_PREFIX_BYTES)
        .ok_or(ProtocolError::Truncated)?;
    if following < ENVELOPE_BYTES {
        return Err(ProtocolError::Truncated);
    }
    if following > MAX_FRAME_BYTES {
        return Err(ProtocolError::FrameTooLarge(following));
    }
    let length = u32::try_from(following).map_err(|_| ProtocolError::FrameTooLarge(following))?;
    frame[..LENGTH_PREFIX_BYTES].copy_from_slice(&length.to_le_bytes());
    Ok(())
}

pub(crate) fn decode_enveloped(frame: &[u8]) -> Result<(Lane, Cow<'_, [u8]>), ProtocolError> {
    let prefix: [u8; 4] = frame
        .get(..4)
        .ok_or(ProtocolError::Truncated)?
        .try_into()
        .map_err(|_| ProtocolError::Truncated)?;
    let length = usize::try_from(u32::from_le_bytes(prefix))
        .map_err(|_| ProtocolError::FrameTooLarge(usize::MAX))?;
    if length > MAX_FRAME_BYTES {
        return Err(ProtocolError::FrameTooLarge(length));
    }
    let following = frame.get(4..).ok_or(ProtocolError::Truncated)?;
    if following.len() != length {
        return Err(ProtocolError::LengthMismatch);
    }
    decode_following(following)
}

pub(crate) fn read_enveloped_into<'a>(
    reader: &mut impl Read,
    frame: &'a mut Vec<u8>,
) -> Result<(Lane, Cow<'a, [u8]>), ProtocolError> {
    frame.clear();
    let mut prefix = [0_u8; 4];
    reader.read_exact(&mut prefix)?;
    let length = usize::try_from(u32::from_le_bytes(prefix))
        .map_err(|_| ProtocolError::FrameTooLarge(usize::MAX))?;
    if length > MAX_FRAME_BYTES {
        return Err(ProtocolError::FrameTooLarge(length));
    }
    if length < ENVELOPE_BYTES {
        return Err(ProtocolError::Truncated);
    }
    if frame.capacity() < length {
        frame.reserve_exact(length);
    }
    let limit = u64::try_from(length).map_err(|_| ProtocolError::FrameTooLarge(length))?;
    let read = reader.take(limit).read_to_end(frame)?;
    if read != length {
        return Err(std::io::Error::new(
            std::io::ErrorKind::UnexpectedEof,
            "protocol frame ended before its declared length",
        )
        .into());
    }
    decode_following(frame)
}

fn decode_following(following: &[u8]) -> Result<(Lane, Cow<'_, [u8]>), ProtocolError> {
    let header = following
        .get(..ENVELOPE_BYTES)
        .ok_or(ProtocolError::Truncated)?;
    let lane = Lane::try_from(header[0])?;
    let version = u16::from_le_bytes([header[2], header[3]]);
    if version != PROTOCOL_VERSION {
        return Err(ProtocolError::VersionMismatch {
            expected: PROTOCOL_VERSION,
            received: version,
        });
    }
    let payload = &following[ENVELOPE_BYTES..];
    match header[1] {
        0 => Ok((lane, Cow::Borrowed(payload))),
        flags => Err(ProtocolError::UnsupportedFlags(flags)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn in_place_envelope_builder_reuses_its_payload_allocation() {
        let mut frame = Vec::new();
        begin_enveloped_into(&mut frame, Lane::Terminal, 64).expect("begin frame");
        let allocation = frame.as_ptr();
        frame.extend_from_slice(b"terminal-payload");
        finish_enveloped_in_place(&mut frame).expect("finish frame");

        assert_eq!(frame.as_ptr(), allocation);
        let (lane, payload) = decode_enveloped(&frame).expect("decode frame");
        assert_eq!(lane, Lane::Terminal);
        assert_eq!(payload.as_ref(), b"terminal-payload");
    }

    #[test]
    fn in_place_envelope_builder_rejects_oversized_capacity_before_allocating() {
        assert!(matches!(
            begin_enveloped_into(&mut Vec::new(), Lane::Terminal, MAX_FRAME_BYTES),
            Err(ProtocolError::FrameTooLarge(_))
        ));
    }

    #[test]
    fn stream_reader_borrows_and_reuses_its_following_buffer() {
        let first = encode_enveloped(Lane::Terminal, &[7; 64]).expect("first frame");
        let second = encode_enveloped(Lane::Control, b"b").expect("second frame");
        let stream = [first, second].concat();
        let mut bytes = stream.as_slice();
        let mut frame = Vec::with_capacity(68);
        let allocation = frame.as_ptr();

        {
            let (lane, payload) =
                read_enveloped_into(&mut bytes, &mut frame).expect("read first frame");
            assert_eq!(lane, Lane::Terminal);
            assert_eq!(payload.as_ref(), &[7; 64]);
            assert_eq!(
                payload.as_ptr() as usize,
                allocation as usize + ENVELOPE_BYTES
            );
        }
        let capacity = frame.capacity();
        assert_eq!(frame.as_ptr(), allocation);

        let (lane, payload) =
            read_enveloped_into(&mut bytes, &mut frame).expect("read second frame");
        assert_eq!(lane, Lane::Control);
        assert_eq!(payload.as_ref(), b"b");
        assert_eq!(frame.as_ptr(), allocation);
        assert_eq!(frame.capacity(), capacity);
    }
}
