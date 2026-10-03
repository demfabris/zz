use serde::ser::{Serialize, SerializeStructVariant, Serializer};
use zz_terminal::KeyInput;

use crate::framing::{Lane, ProtocolError, begin_enveloped_into, finish_enveloped_in_place};
use crate::{MAX_ENCODED_FRAME_BYTES, PaneId};

const INPUT_VARIANT: u32 = 9;
const KEY_VARIANT: u32 = 1;
const KEY_PAYLOAD_RESERVE: usize = 64;

struct KeyMessage<'a> {
    pane: PaneId,
    input: &'a KeyInput,
    text_follows: bool,
}

struct KeyVariant<'a>(&'a KeyMessage<'a>);

impl Serialize for KeyMessage<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_newtype_variant(
            "ProtocolMessage",
            INPUT_VARIANT,
            "Input",
            &KeyVariant(self),
        )
    }
}

impl Serialize for KeyVariant<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut variant =
            serializer.serialize_struct_variant("InputMessage", KEY_VARIANT, "Key", 3)?;
        variant.serialize_field("pane", &self.0.pane)?;
        variant.serialize_field("input", self.0.input)?;
        variant.serialize_field("text_follows", &self.0.text_follows)?;
        variant.end()
    }
}

struct VecFlavor<'a>(&'a mut Vec<u8>);

impl postcard::ser_flavors::Flavor for VecFlavor<'_> {
    type Output = ();

    fn try_push(&mut self, data: u8) -> postcard::Result<()> {
        if self.0.len() >= MAX_ENCODED_FRAME_BYTES {
            return Err(postcard::Error::SerializeBufferFull);
        }
        self.0.push(data);
        Ok(())
    }

    fn try_extend(&mut self, data: &[u8]) -> postcard::Result<()> {
        if self
            .0
            .len()
            .checked_add(data.len())
            .is_none_or(|len| len > MAX_ENCODED_FRAME_BYTES)
        {
            return Err(postcard::Error::SerializeBufferFull);
        }
        self.0.extend_from_slice(data);
        Ok(())
    }

    fn finalize(self) -> postcard::Result<Self::Output> {
        Ok(())
    }
}

pub fn encode_key_input_into(
    pane: PaneId,
    input: &KeyInput,
    text_follows: bool,
    output: &mut Vec<u8>,
) -> Result<(), ProtocolError> {
    let message = KeyMessage {
        pane,
        input,
        text_follows,
    };
    let result = begin_enveloped_into(output, Lane::Control, KEY_PAYLOAD_RESERVE)
        .and_then(|()| {
            postcard::serialize_with_flavor(&message, VecFlavor(output)).map_err(|error| {
                if matches!(error, postcard::Error::SerializeBufferFull) {
                    ProtocolError::FrameTooLarge(crate::MAX_FRAME_BYTES.saturating_add(1))
                } else {
                    ProtocolError::Encode(error)
                }
            })
        })
        .and_then(|()| finish_enveloped_in_place(output));
    if result.is_err() {
        output.clear();
    }
    result
}

#[cfg(test)]
mod tests {
    use zz_terminal::{KeyAction, KeyCode, Modifiers};

    use super::*;
    use crate::{InputMessage, ProtocolMessage, decode_protocol_frame, encode_protocol_message};

    fn input(key: KeyCode, modifiers: Modifiers, text: Option<&str>) -> KeyInput {
        KeyInput {
            action: KeyAction::Press,
            key,
            modifiers,
            text: text.map(Into::into),
            unshifted_codepoint: match key {
                KeyCode::Character(character) => Some(character),
                _ => None,
            },
        }
    }

    #[test]
    fn borrowed_key_frames_match_the_owned_message_encoding() {
        let mut frame = Vec::new();
        for (pane, input, text_follows) in [
            (
                PaneId(1),
                input(KeyCode::Character('a'), Modifiers::default(), Some("a")),
                false,
            ),
            (
                PaneId(u64::MAX),
                input(
                    KeyCode::Character('x'),
                    Modifiers::new(true, true, true, false),
                    None,
                ),
                true,
            ),
            (
                PaneId(300),
                input(KeyCode::Function(12), Modifiers::default(), None),
                false,
            ),
            (
                PaneId(7),
                input(KeyCode::Unidentified, Modifiers::default(), Some("§é界")),
                false,
            ),
        ] {
            let owned = ProtocolMessage::Input(InputMessage::Key {
                pane,
                input: input.clone(),
                text_follows,
            });
            encode_key_input_into(pane, &input, text_follows, &mut frame).unwrap();
            assert_eq!(frame, encode_protocol_message(&owned).unwrap());
            assert_eq!(decode_protocol_frame(&frame).unwrap(), owned);
        }
    }
}
