use base64::{Engine as _, engine::general_purpose::STANDARD};
use zz_terminal::ClipboardTarget;

pub(crate) const MAX_OSC52_PAYLOAD_BYTES: usize = 1024 * 1024;

#[derive(Debug, Eq, PartialEq)]
pub(crate) enum Osc52 {
    Empty,
    Encoded(Vec<u8>),
    TooLarge,
}

pub(crate) fn encode(target: ClipboardTarget, text: &str) -> Osc52 {
    if text.is_empty() {
        return Osc52::Empty;
    }
    let encoded_len = text.len().div_ceil(3).saturating_mul(4);
    if encoded_len > MAX_OSC52_PAYLOAD_BYTES {
        return Osc52::TooLarge;
    }

    let payload = STANDARD.encode(text);
    let selections: &[u8] = match target {
        ClipboardTarget::Clipboard => b"c",
        ClipboardTarget::Primary => b"pc",
    };
    let mut output = Vec::with_capacity(
        selections
            .len()
            .saturating_mul(payload.len().saturating_add(8)),
    );
    for selection in selections {
        output.extend_from_slice(b"\x1b]52;");
        output.push(*selection);
        output.push(b';');
        output.extend_from_slice(payload.as_bytes());
        output.push(0x07);
    }
    Osc52::Encoded(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clipboard_and_primary_encode_the_expected_osc52_selections() {
        assert_eq!(
            encode(ClipboardTarget::Clipboard, "hello"),
            Osc52::Encoded(b"\x1b]52;c;aGVsbG8=\x07".to_vec())
        );
        assert_eq!(
            encode(ClipboardTarget::Primary, "hello"),
            Osc52::Encoded(b"\x1b]52;p;aGVsbG8=\x07\x1b]52;c;aGVsbG8=\x07".to_vec())
        );
        assert_eq!(encode(ClipboardTarget::Clipboard, ""), Osc52::Empty);
    }

    #[test]
    fn encoded_payload_is_capped_at_one_mibibyte() {
        let accepted = "a".repeat(786_432);
        let rejected = "a".repeat(786_433);

        let Osc52::Encoded(encoded) = encode(ClipboardTarget::Clipboard, &accepted) else {
            panic!("payload at the cap should be accepted");
        };
        assert_eq!(encoded.len(), MAX_OSC52_PAYLOAD_BYTES + 8);
        assert_eq!(
            encode(ClipboardTarget::Clipboard, &rejected),
            Osc52::TooLarge
        );
    }
}
