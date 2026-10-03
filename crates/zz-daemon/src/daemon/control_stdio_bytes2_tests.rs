use super::*;

fn escaped_one_by_one(bytes: &[u8]) -> Vec<u8> {
    let mut line = Vec::new();
    for byte in bytes {
        if *byte < 0x20 || *byte == b'\\' {
            line.extend([
                b'\\',
                b'0' + (byte >> 6),
                b'0' + ((byte >> 3) & 7),
                b'0' + (byte & 7),
            ]);
        } else {
            line.push(*byte);
        }
    }
    line
}

fn escaped(bytes: &[u8]) -> Vec<u8> {
    let mut line = b"%output %1 ".to_vec();
    append_output_bytes(&mut line, bytes);
    line.split_off(b"%output %1 ".len())
}

#[test]
fn bulk_output_escapes_match_the_byte_by_byte_rendering() {
    let every: Vec<u8> = (0..=255).collect();
    assert_eq!(escaped(&every), escaped_one_by_one(&every));
    assert_eq!(escaped(b""), b"");
    assert_eq!(escaped(b"\\"), b"\\134");
    assert_eq!(escaped(b"a\r\nb"), b"a\\015\\012b");
    let mut state = 0x9e37_79b9_u32;
    for length in 0..200 {
        let bytes = (0..length)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                match state % 7 {
                    0 => b'\n',
                    1 => b'\\',
                    2 => (state >> 8) as u8,
                    _ => b'a' + (state >> 8) as u8 % 26,
                }
            })
            .collect::<Vec<_>>();
        assert_eq!(escaped(&bytes), escaped_one_by_one(&bytes), "{bytes:?}");
    }
    for position in 0..48 {
        let mut bytes = vec![b'x'; 48];
        bytes[position] = 0x1b;
        assert_eq!(escaped(&bytes), escaped_one_by_one(&bytes), "{position}");
    }
}
