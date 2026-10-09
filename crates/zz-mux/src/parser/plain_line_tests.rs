use super::*;

fn walk<C: ConfigContext>(input: &str, context: &mut C, assignment_overlay: bool) -> ParsedConfig {
    parse_config_characters(
        "<control>".to_owned(),
        ConfigCharacters::text(input),
        context,
        assignment_overlay,
    )
    .0
}

fn full_parse(input: &str) -> ParsedConfig {
    walk(input, &mut LiteralVariableContext, true)
}

#[test]
fn plain_lines_parse_exactly_like_the_character_walk() {
    let alphabet = (b'!'..=b'~')
        .map(char::from)
        .chain(" \t\n\r\u{b}\u{c}\0\u{85}\u{a0}\u{e9}\u{2028}\u{3000}\u{4e2d}\u{1f600}".chars())
        .collect::<Vec<_>>();
    let homes = BTreeMap::from([(String::new(), "/home/u".to_owned())]);
    let variables = BTreeMap::from([("X".to_owned(), "1".to_owned())]);
    let mut seed = 0x2545_f491_4f6c_dd1d_u64;
    let mut next = || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    let mut covered = BTreeSet::new();
    let mut plain = 0;
    for _ in 0..100_000 {
        let palette = (0..=next() % 6)
            .map(|_| alphabet[(next() % alphabet.len() as u64) as usize])
            .chain([' '])
            .collect::<Vec<_>>();
        let length = (next() % 65) as usize;
        let input = (0..length)
            .map(|_| palette[(next() % palette.len() as u64) as usize])
            .collect::<String>();
        if !is_plain_config_line(&input) {
            continue;
        }
        plain += 1;
        covered.extend(input.chars());
        let fast = parse_plain_config_line("<control>".to_owned(), &input);
        for overlay in [true, false] {
            let walked = [
                walk(&input, &mut LiteralVariableContext, overlay),
                walk(
                    &input,
                    &mut ResolvedExpansionContext {
                        homes: &homes,
                        variables: &variables,
                    },
                    overlay,
                ),
            ];
            for walked in walked {
                assert_eq!(fast, walked, "{input:?} overlay {overlay}");
            }
        }
    }
    assert!(plain > 40_000, "{plain}");
    for character in alphabet {
        assert!(
            covered.contains(&character) || !is_plain_config_line(&format!("x {character}")),
            "{character:?}"
        );
    }
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
