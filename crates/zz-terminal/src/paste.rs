use thiserror::Error;

const MAX_SEPARATOR_BYTES: usize = 4 * 1024;
const MAX_PREPARED_BYTES: usize = 128 * 1024 * 1024;

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum PastePreparationError {
    #[error("paste-buffer separator exceeds {MAX_SEPARATOR_BYTES} bytes")]
    SeparatorTooLarge,
    #[error("prepared paste-buffer output exceeds {MAX_PREPARED_BYTES} bytes")]
    OutputTooLarge,
}

/// Prepares buffer bytes under tmux's `paste-buffer` rules: newlines become
/// `separator`, and unless `literal`, unsafe bytes are visibly encoded the way
/// tmux's `VIS_SAFE | VIS_NOSLASH` does.
pub fn prepare_paste_buffer(
    data: &[u8],
    separator: &[u8],
    literal: bool,
) -> Result<Vec<u8>, PastePreparationError> {
    if separator.len() > MAX_SEPARATOR_BYTES {
        return Err(PastePreparationError::SeparatorTooLarge);
    }

    let output_len = prepared_output_len(data, separator.len(), literal)?;
    let mut output = Vec::with_capacity(output_len);
    let mut start = 0;
    for (index, byte) in data.iter().enumerate() {
        if *byte != b'\n' {
            continue;
        }
        append_buffer_fragment(&mut output, &data[start..index], literal);
        output.extend_from_slice(separator);
        start = index + 1;
    }
    if start < data.len() {
        append_buffer_fragment(&mut output, &data[start..], literal);
    }
    debug_assert_eq!(output.len(), output_len);
    Ok(output)
}

fn prepared_output_len(
    data: &[u8],
    separator_len: usize,
    literal: bool,
) -> Result<usize, PastePreparationError> {
    let mut output_len = 0_usize;
    let mut start = 0;
    for (index, byte) in data.iter().enumerate() {
        if *byte != b'\n' {
            continue;
        }
        let fragment = &data[start..index];
        let fragment_len = if literal {
            fragment.len()
        } else {
            safe_fragment_len(fragment)
        };
        output_len = output_len
            .checked_add(fragment_len)
            .and_then(|size| size.checked_add(separator_len))
            .filter(|size| *size <= MAX_PREPARED_BYTES)
            .ok_or(PastePreparationError::OutputTooLarge)?;
        start = index + 1;
    }
    if start < data.len() {
        let fragment = &data[start..];
        let fragment_len = if literal {
            fragment.len()
        } else {
            safe_fragment_len(fragment)
        };
        output_len = output_len
            .checked_add(fragment_len)
            .filter(|size| *size <= MAX_PREPARED_BYTES)
            .ok_or(PastePreparationError::OutputTooLarge)?;
    }
    Ok(output_len)
}

fn safe_fragment_len(fragment: &[u8]) -> usize {
    let mut output_len = 0;
    let mut index = 0;
    while index < fragment.len() {
        let byte = fragment[index];
        if byte.is_ascii() {
            output_len += if safe_ascii(byte) { 1 } else { 2 };
            index += 1;
            continue;
        }

        let width = utf8_sequence_width(byte);
        if width > 1
            && index + width <= fragment.len()
            && std::str::from_utf8(&fragment[index..index + width]).is_ok()
        {
            output_len += width;
            index += width;
        } else {
            output_len += visible_byte_len(byte);
            index += 1;
        }
    }
    output_len
}

fn append_buffer_fragment(output: &mut Vec<u8>, fragment: &[u8], literal: bool) {
    if literal {
        output.extend_from_slice(fragment);
        return;
    }

    let mut index = 0;
    while index < fragment.len() {
        let byte = fragment[index];
        if byte.is_ascii() {
            append_safe_ascii(output, byte);
            index += 1;
            continue;
        }

        let width = utf8_sequence_width(byte);
        if width > 1
            && index + width <= fragment.len()
            && std::str::from_utf8(&fragment[index..index + width]).is_ok()
        {
            output.extend_from_slice(&fragment[index..index + width]);
            index += width;
        } else {
            append_visible_byte(output, byte);
            index += 1;
        }
    }
}

fn append_safe_ascii(output: &mut Vec<u8>, byte: u8) {
    if safe_ascii(byte) {
        output.push(byte);
    } else {
        append_visible_byte(output, byte);
    }
}

fn safe_ascii(byte: u8) -> bool {
    byte.is_ascii_graphic() || matches!(byte, b' ' | b'\t' | b'\n' | b'\r' | 0x07 | 0x08)
}

fn visible_byte_len(byte: u8) -> usize {
    if byte & 0x7f == b' ' {
        4
    } else if byte.is_ascii() {
        2
    } else {
        3
    }
}

fn append_visible_byte(output: &mut Vec<u8>, byte: u8) {
    if byte & 0x7f == b' ' {
        output.extend_from_slice(&[
            b'\\',
            b'0' + (byte >> 6 & 0x07),
            b'0' + (byte >> 3 & 0x07),
            b'0' + (byte & 0x07),
        ]);
        return;
    }

    let mut value = byte;
    if !byte.is_ascii() {
        value &= 0x7f;
        output.push(b'M');
    }
    if value.is_ascii_control() {
        output.push(b'^');
        output.push(if value == 0x7f { b'?' } else { value + b'@' });
    } else {
        output.extend_from_slice(&[b'-', value]);
    }
}

const fn utf8_sequence_width(byte: u8) -> usize {
    match byte {
        0xc2..=0xdf => 2,
        0xe0..=0xef => 3,
        0xf0..=0xf4 => 4,
        _ => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_paste_preserves_utf8_and_vis_encodes_unsafe_bytes() {
        let prepared = prepare_paste_buffer(b"A\0\x1b\x7f\xc3\xa9\xff\xa0\nB", b"\r", false)
            .expect("safe paste");
        assert_eq!(prepared, b"A^@^[^?\xc3\xa9M^?\\240\rB");
    }

    #[test]
    fn literal_paste_only_replaces_newline_separators() {
        let prepared = prepare_paste_buffer(b"a\0\xff\n\nb", b"::", true).expect("literal paste");
        assert_eq!(prepared, b"a\0\xff::::b");
        assert_eq!(
            prepare_paste_buffer(b"one\ntwo\n", b"", true).expect("joined paste"),
            b"onetwo"
        );
    }

    #[test]
    fn paste_expansion_and_separator_are_bounded_before_allocation() {
        assert_eq!(
            prepare_paste_buffer(b"data", &vec![b'x'; MAX_SEPARATOR_BYTES + 1], false),
            Err(PastePreparationError::SeparatorTooLarge)
        );
        assert_eq!(
            prepare_paste_buffer(
                &vec![b'\n'; MAX_PREPARED_BYTES / MAX_SEPARATOR_BYTES + 1],
                &vec![b'x'; MAX_SEPARATOR_BYTES],
                false,
            ),
            Err(PastePreparationError::OutputTooLarge)
        );
    }
}
