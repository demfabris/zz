use super::*;

fn full_parse(input: &str) -> ParsedConfig {
    parse_config_characters(
        "<control>".to_owned(),
        ConfigCharacters::text(input),
        &mut LiteralVariableContext,
        true,
    )
}

#[test]
fn plain_lines_parse_exactly_like_the_character_walk() {
    const ALPHABET: [char; 24] = [
        'a', 'z', '-', '_', '=', ' ', ' ', '\t', '%', '#', '{', '}', '$', '~', ';', '"', '\'',
        '\\', '\n', '\r', '|', ':', '\u{e9}', '\u{a0}',
    ];
    let mut seed = 0x2545_f491_4f6c_dd1d_u64;
    let mut plain = 0;
    for _ in 0..40_000 {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        let length = (seed % 11) as usize;
        let mut state = seed;
        let input = (0..length)
            .map(|_| {
                state = state
                    .wrapping_mul(6_364_136_223_846_793_005)
                    .wrapping_add(1);
                ALPHABET[(state >> 33) as usize % ALPHABET.len()]
            })
            .collect::<String>();
        if is_plain_config_line(&input) {
            plain += 1;
            assert_eq!(
                parse_plain_config_line("<control>".to_owned(), &input),
                full_parse(&input),
                "{input:?}"
            );
        }
    }
    assert!(plain > 1_000, "{plain}");
}

#[test]
fn control_query_lines_take_the_plain_path_with_their_source_span() {
    for (line, args, column) in [
        ("display-message -p x", &["-p", "x"][..], 1),
        ("  list-windows\t-a  ", &["-a"][..], 3),
        ("refresh-client -C 80,24", &["-C", "80,24"][..], 1),
        ("set -g @x=1 y", &["-g", "@x=1", "y"][..], 1),
    ] {
        assert!(is_plain_config_line(line), "{line}");
        let parsed = parse_config("<control>", line);
        assert_eq!(parsed, full_parse(line), "{line}");
        let command = &parsed.commands[0];
        assert_eq!(command.args, args, "{line}");
        assert_eq!(
            command.source,
            Some(SourceSpan {
                source: "<control>".to_owned(),
                line: 1,
                column,
            })
        );
    }
    for line in [
        "X=1 display-message",
        "display-message -p '#{pane_id}'",
        "display-message -p $HOME",
        "display-message -p ~",
        "display-message -p %1",
        "a ; b",
        "a\nb",
        "display-message -p caf\u{e9}",
    ] {
        assert!(!is_plain_config_line(line), "{line}");
    }
    assert_eq!(parse_config("<control>", " \t "), ParsedConfig::default());
}
