use base64::{Engine as _, engine::general_purpose::STANDARD};
use zz_protocol::ClipboardProducer;
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
    /// target. Every selection the server writes itself goes out this way:
    /// `window_copy_copy_buffer` for a `copy-selection`, and `cmd-set-buffer`
    /// and `cmd-load-buffer` for a `-w`, all of them
    /// `screen_write_setselection(&ctx, "", buf, len)` or
    /// `tty_set_selection(&tc->tty, "", ...)`.
    TerminalDefault,
    /// A named field, for the write an application asked for. `input_osc_52`
    /// forwards the application's own field; the daemon reduces it to the two
    /// targets the wire carries, so PRIMARY is written as `p` and `c` to keep
    /// a terminal that honours only one of them covered.
    Named(ClipboardTarget),
}

impl Selection {
    /// The field a daemon clipboard write should name, from the producer the
    /// event carries. The pin keeps its two writers apart by construction and
    /// zz now says which one wrote; before v99 both reached a client with
    /// `request_id` 0 and only a client-issued copy could be told apart.
    pub(crate) const fn for_producer(producer: ClipboardProducer, target: ClipboardTarget) -> Self {
        match producer {
            ClipboardProducer::Server => Self::TerminalDefault,
            ClipboardProducer::Application => Self::Named(target),
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
    /// `window_copy_copy_buffer` (window-copy.c), `cmd_set_buffer_exec` and
    /// `cmd_load_buffer_exec` all hand the terminal an empty `clip`, so with
    /// the `clipboard` terminal feature's `Ms=\E]52;%p1%s;%p2%s\a` a
    /// server-written selection reaches the outer terminal as
    /// `ESC ] 52 ; ; <base64> BEL` whichever target the copy named. Measured on
    /// the pin through `compat/scenarios/smoke/copy-selection-clipboard-bytes`:
    /// one write with an empty field under `set-clipboard external` and under
    /// `on`, and no write at all under `off`. `input_osc_52` (input.c) is the
    /// other producer and keeps the field the application named, filtered
    /// through `cpqs01234567`.
    #[test]
    fn a_server_selection_names_no_field_and_an_application_write_keeps_its_own() {
        for target in [ClipboardTarget::Clipboard, ClipboardTarget::Primary] {
            assert_eq!(
                encode(
                    Selection::for_producer(ClipboardProducer::Server, target),
                    "hello"
                ),
                Osc52::Encoded(b"\x1b]52;;aGVsbG8=\x07".to_vec()),
                "the pin writes one empty-field selection whichever target the copy names"
            );
        }
        assert_eq!(
            encode(
                Selection::for_producer(ClipboardProducer::Application, ClipboardTarget::Clipboard),
                "hello"
            ),
            Osc52::Encoded(b"\x1b]52;c;aGVsbG8=\x07".to_vec())
        );
        assert_eq!(
            encode(
                Selection::for_producer(ClipboardProducer::Application, ClipboardTarget::Primary),
                "hello"
            ),
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
