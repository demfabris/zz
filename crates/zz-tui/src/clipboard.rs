use base64::{Engine as _, engine::general_purpose::STANDARD};
use zz_terminal::ClipboardTarget;

pub(crate) const MAX_OSC52_PAYLOAD_BYTES: usize = 1024 * 1024;

#[derive(Debug, Eq, PartialEq)]
pub(crate) enum Osc52 {
    Empty,
    Encoded(Vec<u8>),
    TooLarge,
}

/// Which OSC 52 selection field a write names.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Selection {
    /// The empty field, which asks the outer terminal for its own default
    /// target. `window_copy_copy_buffer` writes every `copy-selection` this
    /// way — `screen_write_setselection(&ctx, "", buf, len)`.
    TerminalDefault,
    /// A named field, for the write an application asked for. `input_osc_52`
    /// forwards the application's own field; the daemon reduces it to the two
    /// targets the wire carries, so PRIMARY is written as `p` and `c` to keep
    /// a terminal that honours only one of them covered.
    Named(ClipboardTarget),
}

impl Selection {
    /// The field a daemon clipboard write should name. `request_id` is the
    /// pin's own split: zero is an application's OSC 52, non-zero answers a
    /// client's `copy-selection`.
    pub(crate) const fn for_request(request_id: u64, target: ClipboardTarget) -> Self {
        if request_id == 0 {
            Self::Named(target)
        } else {
            Self::TerminalDefault
        }
    }
}

pub(crate) fn encode(selection: Selection, text: &str) -> Osc52 {
    if text.is_empty() {
        return Osc52::Empty;
    }
    let encoded_len = text.len().div_ceil(3).saturating_mul(4);
    if encoded_len > MAX_OSC52_PAYLOAD_BYTES {
        return Osc52::TooLarge;
    }

    let payload = STANDARD.encode(text);
    let selections: &[u8] = match selection {
        Selection::TerminalDefault => b"",
        Selection::Named(ClipboardTarget::Clipboard) => b"c",
        Selection::Named(ClipboardTarget::Primary) => b"pc",
    };
    let writes = selections.len().max(1);
    let mut output = Vec::with_capacity(writes.saturating_mul(payload.len().saturating_add(8)));
    for index in 0..writes {
        output.extend_from_slice(b"\x1b]52;");
        if let Some(field) = selections.get(index) {
            output.push(*field);
        }
        output.push(b';');
        output.extend_from_slice(payload.as_bytes());
        output.push(0x07);
    }
    Osc52::Encoded(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Derived from pinned tmux d77c9dc6.
    ///
    /// `window_copy_copy_buffer` (window-copy.c) hands `tty_set_selection` an
    /// empty `clip`, so with the `clipboard` terminal feature's
    /// `Ms=\E]52;%p1%s;%p2%s\a` a `copy-selection` reaches the outer terminal
    /// as `ESC ] 52 ; ; <base64> BEL`. Measured on the pin through
    /// `compat/scenarios/smoke/copy-selection-clipboard-bytes`: one write with
    /// an empty field under `set-clipboard external` and under `on`, and no
    /// write at all under `off`. `input_osc_52` (input.c) is the other
    /// producer and keeps the field the application named.
    #[test]
    fn a_copy_selection_names_no_field_and_an_application_write_keeps_its_own() {
        assert_eq!(
            encode(
                Selection::for_request(1, ClipboardTarget::Clipboard),
                "hello"
            ),
            Osc52::Encoded(b"\x1b]52;;aGVsbG8=\x07".to_vec())
        );
        assert_eq!(
            encode(Selection::for_request(1, ClipboardTarget::Primary), "hello"),
            Osc52::Encoded(b"\x1b]52;;aGVsbG8=\x07".to_vec()),
            "the pin writes one empty-field selection whichever target the copy names"
        );
        assert_eq!(
            encode(
                Selection::for_request(0, ClipboardTarget::Clipboard),
                "hello"
            ),
            Osc52::Encoded(b"\x1b]52;c;aGVsbG8=\x07".to_vec())
        );
        assert_eq!(
            encode(Selection::for_request(0, ClipboardTarget::Primary), "hello"),
            Osc52::Encoded(b"\x1b]52;p;aGVsbG8=\x07\x1b]52;c;aGVsbG8=\x07".to_vec())
        );
        assert_eq!(
            encode(Selection::Named(ClipboardTarget::Clipboard), ""),
            Osc52::Empty
        );
    }

    #[test]
    fn encoded_payload_is_capped_at_one_mibibyte() {
        let accepted = "a".repeat(786_432);
        let rejected = "a".repeat(786_433);

        let Osc52::Encoded(encoded) = encode(Selection::Named(ClipboardTarget::Clipboard), &accepted)
        else {
            panic!("payload at the cap should be accepted");
        };
        assert_eq!(encoded.len(), MAX_OSC52_PAYLOAD_BYTES + 8);
        assert_eq!(
            encode(Selection::Named(ClipboardTarget::Clipboard), &rejected),
            Osc52::TooLarge
        );
    }
}
