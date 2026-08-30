use zz_mux::{ParsedConfig, parse_config};

#[test]
fn matches_the_pinned_signed_buffer_ff_placement_matrix() {
    let isolated = ParsedConfig::parse_buffer_bytes("isolated.conf", b"\xff");
    assert!(isolated.commands.is_empty());
    assert!(isolated.environment.is_empty());
    assert!(isolated.diagnostics.is_empty());

    let embedded =
        ParsedConfig::parse_buffer_bytes("embedded.conf", b"display-message -p before\xffafter\n");
    assert!(embedded.diagnostics.is_empty());
    assert_eq!(embedded.commands.len(), 1);
    assert_eq!(embedded.commands[0].name, b"display-message");
    assert_eq!(
        embedded.commands[0]
            .args
            .iter()
            .map(Vec::as_slice)
            .collect::<Vec<_>>(),
        [b"-p".as_slice(), b"before".as_slice(), b"after".as_slice()]
    );

    let boundary = ParsedConfig::parse_buffer_bytes(
        "boundary.conf",
        b"display-message -p before \xffdisplay-message -p after\n",
    );
    assert!(boundary.diagnostics.is_empty());
    assert_eq!(boundary.commands.len(), 2);
    assert_eq!(boundary.commands[0].args[1], b"before");
    assert_eq!(boundary.commands[1].args[1], b"after");
    assert_eq!(boundary.commands[0].source.as_ref().unwrap().line, 1);
    assert_eq!(boundary.commands[1].source.as_ref().unwrap().line, 1);

    let second_boundary = ParsedConfig::parse_buffer_bytes(
        "second-boundary.conf",
        b"\xffdisplay-message -p first\n\xffdisplay-message -p second\ndisplay-message -p third\n",
    );
    assert!(second_boundary.diagnostics.is_empty());
    assert_eq!(second_boundary.commands.len(), 1);
    assert_eq!(second_boundary.commands[0].args[1], b"first");

    let comment = ParsedConfig::parse_buffer_bytes(
        "comment.conf",
        b"# ignored\xffdisplay-message -p escaped\n",
    );
    assert!(comment.diagnostics.is_empty());
    assert_eq!(comment.commands.len(), 1);
    assert_eq!(comment.commands[0].args[1], b"escaped");

    let escaped =
        ParsedConfig::parse_buffer_bytes("escaped.conf", b"display-message -p before\\\xffafter\n");
    assert!(escaped.diagnostics.is_empty());
    assert_eq!(escaped.commands.len(), 1);
    assert_eq!(escaped.commands[0].args[1], b"before\x07fter");

    let block_hard_eof = ParsedConfig::parse_buffer_bytes(
        "block-hard-eof.conf",
        b"display-message -p before\nif-shell true { display-message -p one \xffdisplay-message -p two \xffdisplay-message -p three\n }\ndisplay-message -p after\n",
    );
    assert!(block_hard_eof.commands.is_empty());
    assert_eq!(block_hard_eof.diagnostics[0].line, 2);
    assert_eq!(block_hard_eof.diagnostics[0].message, "syntax error");

    let block_quote = ParsedConfig::parse_buffer_bytes(
        "block-quote.conf",
        b"bind-key x { send-keys 'before\xffafter' }\n",
    );
    assert!(block_quote.commands.is_empty());
    assert_eq!(block_quote.diagnostics[0].line, 2);
    assert_eq!(block_quote.diagnostics[0].message, "syntax error");
}

#[test]
fn matches_the_pinned_signed_buffer_getc_boundaries() {
    let even_backslash = ParsedConfig::parse_buffer_bytes(
        "even-backslash.conf",
        b"display-message -p before\\\\\xffafter\n",
    );
    assert!(even_backslash.diagnostics.is_empty());
    assert_eq!(even_backslash.commands.len(), 1);
    assert_eq!(even_backslash.commands[0].args[0], b"-p");
    assert_eq!(even_backslash.commands[0].args[1], b"before\\after");

    let cr_lookahead = ParsedConfig::parse_buffer_bytes(
        "cr-lookahead.conf",
        b"display-message -p before\r\xffafter\n",
    );
    assert!(cr_lookahead.diagnostics.is_empty());
    assert_eq!(cr_lookahead.commands.len(), 1);
    assert_eq!(cr_lookahead.commands[0].args[0], b"-p");
    assert_eq!(cr_lookahead.commands[0].args[1], b"before\rafter");

    let cr_even_backslash = ParsedConfig::parse_buffer_bytes(
        "cr-even-backslash.conf",
        b"display-message -p before\r\\\\\xffafter\n",
    );
    assert!(cr_even_backslash.commands.is_empty());
    assert_eq!(cr_even_backslash.diagnostics.len(), 1);
    assert_eq!(cr_even_backslash.diagnostics[0].line, 1);
    assert_eq!(cr_even_backslash.diagnostics[0].message, "syntax error");

    let unicode_eof =
        ParsedConfig::parse_buffer_bytes("unicode-eof.conf", b"display-message -p \\u12\xff34\n");
    assert!(unicode_eof.commands.is_empty());
    assert_eq!(unicode_eof.diagnostics.len(), 1);
    assert_eq!(unicode_eof.diagnostics[0].line, 1);
    assert_eq!(unicode_eof.diagnostics[0].message, "syntax error");

    let second_eof =
        ParsedConfig::parse_buffer_bytes("second-eof.conf", b"\xffdisplay-message -p after");
    assert!(second_eof.commands.is_empty());
    assert_eq!(second_eof.diagnostics.len(), 1);
    assert_eq!(second_eof.diagnostics[0].line, 1);
    assert_eq!(second_eof.diagnostics[0].message, "syntax error");

    let block_trailing_backslash = ParsedConfig::parse_buffer_bytes(
        "block-trailing-backslash.conf",
        b"bind x { display-message -p tail \\",
    );
    assert!(block_trailing_backslash.commands.is_empty());
    assert_eq!(block_trailing_backslash.diagnostics.len(), 1);
    assert_eq!(block_trailing_backslash.diagnostics[0].line, 1);
    assert_eq!(
        block_trailing_backslash.diagnostics[0].message,
        "syntax error"
    );

    let quoted_comment_eof = ParsedConfig::parse_buffer_bytes(
        "quoted-comment-eof.conf",
        b"display-message -p \"a\n#comment",
    );
    assert!(quoted_comment_eof.diagnostics.is_empty());
    assert_eq!(quoted_comment_eof.commands.len(), 1);
    assert_eq!(quoted_comment_eof.commands[0].args[0], b"-p");
    assert_eq!(quoted_comment_eof.commands[0].args[1], b"a\n");
    assert_eq!(
        quoted_comment_eof.commands[0].source.as_ref().unwrap().line,
        2
    );
}

#[test]
fn truncates_signed_buffer_tokens_at_nul() {
    let parsed = ParsedConfig::parse_buffer_bytes(
        "nul.conf",
        b"display-message -p before\0after\nVALUE=kept\0discarded\n",
    );
    assert!(parsed.diagnostics.is_empty());
    assert_eq!(parsed.commands.len(), 1);
    assert_eq!(parsed.commands[0].args[0], b"-p");
    assert_eq!(parsed.commands[0].args[1], b"before");
    assert_eq!(parsed.environment.len(), 1);
    assert_eq!(parsed.environment[0].name, b"VALUE");
    assert_eq!(parsed.environment[0].value, b"kept");

    let escaped = ParsedConfig::parse_buffer_bytes(
        "nul-escape.conf",
        b"display-message -p before\\000after\n",
    );
    assert!(escaped.diagnostics.is_empty());
    assert_eq!(escaped.commands[0].args[0], b"-p");
    assert_eq!(escaped.commands[0].args[1], b"before");

    let command_block = ParsedConfig::parse_buffer_bytes(
        "nul-command-block.conf",
        b"if-shell -F 1 { display-message -p before\0{after }\n",
    );
    assert!(command_block.diagnostics.is_empty());
    assert_eq!(command_block.commands.len(), 1);
    assert_eq!(command_block.commands[0].name, b"if-shell");
    assert_eq!(
        command_block.commands[0]
            .args
            .iter()
            .map(Vec::as_slice)
            .collect::<Vec<_>>(),
        [
            b"-F".as_slice(),
            b"1".as_slice(),
            b"{ display-message -p before\0{after }".as_slice(),
        ]
    );
    assert!(command_block.commands[0].argument_is_command_block(2));
}

#[test]
fn tracks_command_block_token_boundaries() {
    let attached_open = parse_config(
        "attached-open.conf",
        "if-shell -F 1 { display-message -p a{b}c }\n",
    );
    assert!(attached_open.diagnostics.is_empty());
    assert_eq!(attached_open.commands.len(), 1);
    assert_eq!(
        attached_open.commands[0].args,
        ["-F", "1", "{ display-message -p a{b}", "c", "}"]
    );
    assert!(attached_open.commands[0].argument_is_command_block(2));

    let literal_open = ParsedConfig::parse_buffer_bytes(
        "literal-open.conf",
        b"if-shell -F 1 { display-message -p before{after }\n",
    );
    assert!(literal_open.diagnostics.is_empty());
    assert_eq!(literal_open.commands.len(), 1);
    assert_eq!(
        literal_open.commands[0]
            .args
            .iter()
            .map(Vec::as_slice)
            .collect::<Vec<_>>(),
        [
            b"-F".as_slice(),
            b"1".as_slice(),
            b"{ display-message -p before{after }".as_slice(),
        ]
    );
    assert!(literal_open.commands[0].argument_is_command_block(2));

    for (source, input, string_block, byte_block) in [
        (
            "continued-adjacent-blocks.conf",
            "if-shell -F 1 { if-shell -F 1 \\\n{}{} }\n",
            "{ if-shell -F 1 \\\n{}{} }",
            "{ if-shell -F 1 {}{} }",
        ),
        (
            "continued-semicolon-comment.conf",
            "if-shell -F 1 { display-message -p before;\\\n# ignored {\ndisplay-message -p after }\n",
            "{ display-message -p before;\\\n# ignored {\ndisplay-message -p after }",
            "{ display-message -p before;# ignored {\ndisplay-message -p after }",
        ),
        (
            "continued-brace-comment.conf",
            "if-shell -F 1 { if-shell -F 1 { display-message -p nested }\\\n# ignored {\ndisplay-message -p after }\n",
            "{ if-shell -F 1 { display-message -p nested }\\\n# ignored {\ndisplay-message -p after }",
            "{ if-shell -F 1 { display-message -p nested }# ignored {\ndisplay-message -p after }",
        ),
    ] {
        let parsed = parse_config(source, input);
        assert!(parsed.diagnostics.is_empty(), "{source}");
        assert_eq!(parsed.commands.len(), 1, "{source}");
        assert_eq!(parsed.commands[0].name, "if-shell", "{source}");
        assert_eq!(
            parsed.commands[0].args,
            ["-F", "1", string_block],
            "{source}"
        );
        assert!(parsed.commands[0].argument_is_command_block(2), "{source}");

        let parsed = ParsedConfig::parse_buffer_bytes(source, input.as_bytes());
        assert!(parsed.diagnostics.is_empty(), "{source}");
        assert_eq!(parsed.commands.len(), 1, "{source}");
        assert_eq!(parsed.commands[0].name, b"if-shell", "{source}");
        assert_eq!(
            parsed.commands[0]
                .args
                .iter()
                .map(Vec::as_slice)
                .collect::<Vec<_>>(),
            [b"-F".as_slice(), b"1".as_slice(), byte_block.as_bytes()],
            "{source}"
        );
        assert!(parsed.commands[0].argument_is_command_block(2), "{source}");
    }

    let continued_blocks = parse_config(
        "continued-top-level-blocks.conf",
        "if-shell -F 1 \\\n{}{}\n",
    );
    assert!(continued_blocks.diagnostics.is_empty());
    assert_eq!(continued_blocks.commands.len(), 1);
    assert_eq!(continued_blocks.commands[0].args, ["-F", "1", "{}", "{}"]);
    assert!(continued_blocks.commands[0].argument_is_command_block(2));
    assert!(continued_blocks.commands[0].argument_is_command_block(3));

    let continued_semicolon = parse_config(
        "continued-top-level-semicolon-comment.conf",
        "display-message -p before;\\\n# ignored {\ndisplay-message -p after\n",
    );
    assert!(continued_semicolon.diagnostics.is_empty());
    assert_eq!(continued_semicolon.commands.len(), 2);
    assert_eq!(continued_semicolon.commands[0].args, ["-p", "before"]);
    assert_eq!(continued_semicolon.commands[1].args, ["-p", "after"]);

    let continued_close = parse_config(
        "continued-top-level-brace-comment.conf",
        "if-shell -F 1 {}\\\n# ignored {\ndisplay-message -p after\n",
    );
    assert!(continued_close.diagnostics.is_empty());
    assert_eq!(continued_close.commands.len(), 2);
    assert_eq!(continued_close.commands[0].args, ["-F", "1", "{}"]);
    assert!(continued_close.commands[0].argument_is_command_block(2));
    assert_eq!(continued_close.commands[1].args, ["-p", "after"]);

    for (source, input, block) in [
        (
            "adjacent-blocks.conf",
            b"if-shell -F 1 { if-shell -F 1 {}{} }\n".as_slice(),
            "{ if-shell -F 1 {}{} }",
        ),
        (
            "adjacent-open.conf",
            b"if-shell -F 1 {{ display-message -p nested }}\n".as_slice(),
            "{{ display-message -p nested }}",
        ),
        (
            "semicolon-block.conf",
            b"if-shell -F 1 { display-message -p before;{ display-message -p nested } }\n"
                .as_slice(),
            "{ display-message -p before;{ display-message -p nested } }",
        ),
        (
            "semicolon-comment.conf",
            b"if-shell -F 1 { display-message -p before;# ignored {\ndisplay-message -p after }\n"
                .as_slice(),
            "{ display-message -p before;# ignored {\ndisplay-message -p after }",
        ),
        (
            "brace-comment.conf",
            b"if-shell -F 1 { if-shell -F 1 { display-message -p nested }# ignored {\ndisplay-message -p after }\n"
                .as_slice(),
            "{ if-shell -F 1 { display-message -p nested }# ignored {\ndisplay-message -p after }",
        ),
    ] {
        let parsed = ParsedConfig::parse_buffer_bytes(source, input);
        assert!(parsed.diagnostics.is_empty(), "{source}");
        assert_eq!(parsed.commands.len(), 1, "{source}");
        assert_eq!(parsed.commands[0].name, b"if-shell", "{source}");
        assert_eq!(
            parsed.commands[0]
                .args
                .iter()
                .map(Vec::as_slice)
                .collect::<Vec<_>>(),
            [b"-F".as_slice(), b"1".as_slice(), block.as_bytes()],
            "{source}"
        );
        assert!(parsed.commands[0].argument_is_command_block(2), "{source}");
    }
}

#[test]
fn distinguishes_file_and_signed_buffer_ff_input() {
    let input = b"\xffset-environment -g CONFIG_NON_UTF8_MODE set\n";
    let file = ParsedConfig::parse_file_bytes("file.conf", input);
    let buffer = ParsedConfig::parse_buffer_bytes("buffer.conf", input);

    assert!(file.diagnostics.is_empty());
    assert_eq!(file.commands.len(), 1);
    assert_eq!(file.commands[0].name, b"\xffset-environment");
    assert!(buffer.diagnostics.is_empty());
    assert_eq!(buffer.commands.len(), 1);
    assert_eq!(buffer.commands[0].name, b"set-environment");

    let isolated_file = ParsedConfig::parse_file_bytes("file.conf", b"\xff");
    let isolated_buffer = ParsedConfig::parse_buffer_bytes("buffer.conf", b"\xff");
    assert_eq!(isolated_file.commands[0].name, b"\xff");
    assert!(isolated_buffer.commands.is_empty());

    let embedded_file =
        ParsedConfig::parse_file_bytes("file.conf", b"display-message -p before\xffafter\n");
    let embedded_buffer =
        ParsedConfig::parse_buffer_bytes("buffer.conf", b"display-message -p before\xffafter\n");
    assert_eq!(embedded_file.commands[0].args[1], b"before\xffafter");
    assert_eq!(embedded_buffer.commands[0].args[1], b"before");
    assert_eq!(embedded_buffer.commands[0].args[2], b"after");

    let trailing_file =
        ParsedConfig::parse_file_bytes("file.conf", b"display-message -p before\xff\n");
    let trailing_buffer =
        ParsedConfig::parse_buffer_bytes("buffer.conf", b"display-message -p before\xff\n");
    assert_eq!(trailing_file.commands[0].args[1], b"before\xff");
    assert_eq!(trailing_buffer.commands[0].args[1], b"before");
}

#[test]
fn retains_isolated_valid_and_malformed_high_bytes() {
    for input in [
        b"display-message -p before\x80after\n".as_slice(),
        b"display-message -p before\xc2\x80after\n".as_slice(),
        b"display-message -p before\xc3(after\n".as_slice(),
    ] {
        let parsed = ParsedConfig::parse_buffer_bytes("bytes.conf", input);
        assert!(parsed.diagnostics.is_empty(), "{input:02x?}");
        assert_eq!(parsed.commands.len(), 1, "{input:02x?}");
        assert_eq!(&parsed.commands[0].args[1], &input[19..input.len() - 1]);
    }

    let assignment =
        ParsedConfig::parse_buffer_bytes("environment.conf", b"VALUE=before\x80after\n");
    assert!(assignment.diagnostics.is_empty());
    assert!(assignment.commands.is_empty());
    assert_eq!(assignment.environment[0].name, b"VALUE");
    assert_eq!(assignment.environment[0].value, b"before\x80after");

    let crlf = ParsedConfig::parse_buffer_bytes("crlf.conf", b"display-message -p value\r\n");
    assert!(crlf.diagnostics.is_empty());
    assert_eq!(crlf.commands[0].args[1], b"value");

    let escaped_crlf =
        ParsedConfig::parse_buffer_bytes("escaped-crlf.conf", b"display-message -p value\\\r\n");
    assert!(escaped_crlf.diagnostics.is_empty());
    assert_eq!(escaped_crlf.commands[0].args[1], b"value\r");

    let unicode = ParsedConfig::parse_buffer_bytes(
        "unicode.conf",
        b"display-message -p \\U000F0080\\U000F00FF\\U000F0100\\U000F0101\n",
    );
    assert!(unicode.diagnostics.is_empty());
    assert_eq!(
        unicode.commands[0].args[1],
        "\u{f0080}\u{f00ff}\u{f0100}\u{f0101}".as_bytes()
    );

    for parsed in [
        ParsedConfig::parse_file_bytes("octal-file.conf", b"display-message -p \\377\n"),
        ParsedConfig::parse_buffer_bytes("octal-buffer.conf", b"display-message -p \\377\n"),
    ] {
        assert!(parsed.diagnostics.is_empty());
        assert_eq!(parsed.commands[0].args[1], b"\xff");
    }

    let overlay = ParsedConfig::parse_buffer_bytes(
        "overlay.conf",
        b"VALUE=before\x80after\ndisplay-message -p $VALUE\n",
    );
    assert!(overlay.diagnostics.is_empty());
    assert_eq!(overlay.commands[0].args[1], b"before\x80after");

    let unicode_overlay = ParsedConfig::parse_buffer_bytes(
        "unicode-overlay.conf",
        b"VALUE=\\U000F0080\ndisplay-message -p $VALUE\n",
    );
    assert!(unicode_overlay.diagnostics.is_empty());
    assert_eq!(unicode_overlay.commands[0].args[1], "\u{f0080}".as_bytes());

    let home = ParsedConfig::parse_buffer_bytes("home.conf", b"HOME=/tmp/\x80\nset @home ~\n");
    assert!(home.diagnostics.is_empty());
    assert_eq!(home.commands[0].args[1], b"/tmp/\x80");

    let percent_crlf = ParsedConfig::parse_buffer_bytes(
        "percent-crlf.conf",
        b"%if 0\r\ndisplay-message -p wrong\r\n%else\r\ndisplay-message -p kept\r\n%endif\r\n",
    );
    assert!(percent_crlf.commands.is_empty());
    assert_eq!(percent_crlf.diagnostics[0].line, 3);
    assert_eq!(percent_crlf.diagnostics[0].message, "syntax error");

    let percent_space_crlf = ParsedConfig::parse_buffer_bytes(
        "percent-space-crlf.conf",
        b"%if 0\r\ndisplay-message -p wrong\n%else \r\ndisplay-message -p kept\r\n%endif \r\n",
    );
    assert!(percent_space_crlf.diagnostics.is_empty());
    assert_eq!(percent_space_crlf.commands[0].args[1], b"kept");

    let block_crlf = ParsedConfig::parse_buffer_bytes("block-crlf.conf", b"bind c { %else\r\n }\n");
    assert!(block_crlf.diagnostics.is_empty());
    assert_eq!(block_crlf.commands[0].args[1], b"{ %else\r\n }");
}

#[test]
fn keeps_the_string_entrypoint_on_unicode_characters() {
    let bytes =
        ParsedConfig::parse_buffer_bytes("bytes.conf", b"display-message -p before\xa0after\n");
    assert!(bytes.diagnostics.is_empty());
    assert_eq!(bytes.commands[0].args[1], b"before\xa0after");

    let string = parse_config(
        "string.conf",
        "display-message -p before\u{a0}after\nset @value \u{e9}\n",
    );
    assert!(string.diagnostics.is_empty());
    assert_eq!(string.commands[0].args, ["-p", "before", "after"]);
    assert_eq!(string.commands[1].args, ["@value", "é"]);

    let string_nul = parse_config("string-nul.conf", "display-message -p before\0after\n");
    assert!(string_nul.diagnostics.is_empty());
    assert_eq!(string_nul.commands[0].args, ["-p", "before\0after"]);
}
