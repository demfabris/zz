use zz_protocol::{CommandInvocation, SourceSpan};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfigDiagnostic {
    pub source: String,
    pub line: u32,
    pub column: u32,
    pub message: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ParsedConfig {
    pub commands: Vec<CommandInvocation>,
    pub diagnostics: Vec<ConfigDiagnostic>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Quote {
    #[default]
    None,
    Single,
    Double,
}

pub fn parse_config(source: impl Into<String>, input: &str) -> ParsedConfig {
    let source = source.into();
    let mut parsed = ParsedConfig::default();
    let mut words = Vec::new();
    let mut word = String::new();
    let mut word_started = false;
    let mut quote = Quote::None;
    let mut escaped = false;
    let mut in_comment = false;
    let mut line = 1_u32;
    let mut column = 0_u32;
    let mut command_line = 1_u32;
    let mut command_column = 1_u32;

    for character in input.chars() {
        column = column.saturating_add(1);
        if in_comment {
            if character == '\n' {
                finish_command(
                    &source,
                    command_line,
                    command_column,
                    &mut word,
                    &mut word_started,
                    &mut words,
                    &mut parsed.commands,
                );
                in_comment = false;
                line = line.saturating_add(1);
                column = 0;
                command_line = line;
                command_column = 1;
            }
            continue;
        }
        if escaped {
            escaped = false;
            if character == '\n' {
                line = line.saturating_add(1);
                column = 0;
            } else {
                if !word_started && words.is_empty() {
                    command_line = line;
                    command_column = column.saturating_sub(1).max(1);
                }
                word_started = true;
                word.push(match character {
                    'n' if quote == Quote::Double => '\n',
                    'r' if quote == Quote::Double => '\r',
                    't' if quote == Quote::Double => '\t',
                    other => other,
                });
            }
            continue;
        }
        if character == '\\' && quote != Quote::Single {
            escaped = true;
            continue;
        }
        match quote {
            Quote::Single if character == '\'' => quote = Quote::None,
            Quote::Double if character == '"' => quote = Quote::None,
            Quote::Single | Quote::Double => {
                word.push(character);
                if character == '\n' {
                    line = line.saturating_add(1);
                    column = 0;
                }
            }
            Quote::None => match character {
                '\'' => {
                    if !word_started && words.is_empty() {
                        command_line = line;
                        command_column = column;
                    }
                    word_started = true;
                    quote = Quote::Single;
                }
                '"' => {
                    if !word_started && words.is_empty() {
                        command_line = line;
                        command_column = column;
                    }
                    word_started = true;
                    quote = Quote::Double;
                }
                '#' if !word_started => in_comment = true,
                ';' | '\n' => {
                    finish_command(
                        &source,
                        command_line,
                        command_column,
                        &mut word,
                        &mut word_started,
                        &mut words,
                        &mut parsed.commands,
                    );
                    if character == '\n' {
                        line = line.saturating_add(1);
                        column = 0;
                    }
                    command_line = line;
                    command_column = column.saturating_add(1);
                }
                value if value.is_whitespace() => {
                    finish_word(&mut word, &mut word_started, &mut words);
                }
                value => {
                    if words.is_empty() && !word_started {
                        command_line = line;
                        command_column = column;
                    }
                    word_started = true;
                    word.push(value);
                }
            },
        }
    }
    if quote != Quote::None {
        parsed.diagnostics.push(ConfigDiagnostic {
            source,
            line: command_line,
            column: command_column,
            message: "unterminated quote".to_owned(),
        });
    } else if escaped {
        parsed.diagnostics.push(ConfigDiagnostic {
            source,
            line,
            column,
            message: "trailing escape".to_owned(),
        });
    } else {
        finish_command(
            &source,
            command_line,
            command_column,
            &mut word,
            &mut word_started,
            &mut words,
            &mut parsed.commands,
        );
    }
    skip_conditional_blocks(parsed)
}

fn skip_conditional_blocks(parsed: ParsedConfig) -> ParsedConfig {
    if !parsed
        .commands
        .iter()
        .any(|command| command.name.starts_with('%'))
    {
        return parsed;
    }
    let mut result = ParsedConfig {
        commands: Vec::new(),
        diagnostics: parsed.diagnostics,
    };
    let mut depth = 0_u32;
    let mut open_if = None;
    for command in parsed.commands {
        match command.name.as_str() {
            "%if" => {
                if depth == 0 {
                    open_if = Some(conditional_diagnostic(
                        &command,
                        "unsupported %if block skipped",
                    ));
                }
                depth = depth.saturating_add(1);
            }
            "%elif" | "%else" | "%endif" => {
                if depth == 0 {
                    result.diagnostics.push(conditional_diagnostic(
                        &command,
                        format!("{} outside %if", command.name),
                    ));
                } else if command.name == "%endif" {
                    depth -= 1;
                    if depth == 0 {
                        result.diagnostics.extend(open_if.take());
                    }
                }
            }
            _ if depth > 0 => {}
            _ => result.commands.push(command),
        }
    }
    if let Some(mut diagnostic) = open_if {
        "unterminated %if".clone_into(&mut diagnostic.message);
        result.diagnostics.push(diagnostic);
    }
    result
}

fn conditional_diagnostic(
    command: &CommandInvocation,
    message: impl Into<String>,
) -> ConfigDiagnostic {
    let span = command.source.as_ref();
    ConfigDiagnostic {
        source: span.map_or_else(String::new, |span| span.source.clone()),
        line: span.map_or(0, |span| span.line),
        column: span.map_or(0, |span| span.column),
        message: message.into(),
    }
}

fn finish_word(word: &mut String, word_started: &mut bool, words: &mut Vec<String>) {
    if *word_started {
        words.push(std::mem::take(word));
        *word_started = false;
    }
}

fn finish_command(
    source: &str,
    line: u32,
    column: u32,
    word: &mut String,
    word_started: &mut bool,
    words: &mut Vec<String>,
    output: &mut Vec<CommandInvocation>,
) {
    finish_word(word, word_started, words);
    if words.is_empty() {
        return;
    }
    let mut command = std::mem::take(words).into_iter();
    output.push(CommandInvocation {
        name: command.next().expect("command has a name"),
        args: command.collect(),
        source: Some(SourceSpan {
            source: source.to_owned(),
            line,
            column,
        }),
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_tmux_style_words_quotes_comments_and_lists() {
        let parsed = parse_config(
            "test.conf",
            "set -g prefix C-a\n\
             bind c new-window -n 'my window'; bind -n F2 splitw -h # note\n\
             bind x send-keys \"hello world\" Enter\n",
        );
        assert!(parsed.diagnostics.is_empty());
        assert_eq!(parsed.commands.len(), 4);
        assert_eq!(parsed.commands[1].args[3], "my window");
        assert_eq!(parsed.commands[3].args[2], "hello world");
    }

    #[test]
    fn continues_lines_and_reports_unterminated_quotes() {
        let parsed = parse_config("test.conf", "bind c new-\\\nwindow\nset 'oops");
        assert_eq!(parsed.commands[0].args[1], "new-window");
        assert_eq!(parsed.diagnostics.len(), 1);
    }

    #[test]
    fn preserves_quoted_empty_arguments_and_concatenation() {
        let parsed = parse_config(
            "test.conf",
            "set -g word-separators \"\"\nset @joined \"\"suffix\n",
        );
        assert!(parsed.diagnostics.is_empty());
        assert_eq!(parsed.commands[0].args, ["-g", "word-separators", ""]);
        assert_eq!(parsed.commands[1].args, ["@joined", "suffix"]);
    }

    #[test]
    fn flushes_a_final_command_without_a_trailing_newline() {
        let parsed = parse_config("test.conf", "set -g prefix C-a");
        assert!(parsed.diagnostics.is_empty());
        assert_eq!(parsed.commands.len(), 1);
        assert_eq!(parsed.commands[0].name, "set");
        assert_eq!(parsed.commands[0].args, ["-g", "prefix", "C-a"]);
    }

    #[test]
    fn skips_conditional_blocks_instead_of_executing_both_branches() {
        let parsed = parse_config(
            "test.conf",
            "set -g prefix C-a\n\
             %if \"#{==:#{host},work}\"\n\
             bind x kill-pane\n\
             %if nested\n\
             bind y kill-window\n\
             %endif\n\
             %else\n\
             bind z kill-server\n\
             %endif\n\
             bind c new-window\n",
        );
        let names: Vec<&str> = parsed
            .commands
            .iter()
            .map(|command| command.name.as_str())
            .collect();
        assert_eq!(names, ["set", "bind"]);
        assert_eq!(parsed.diagnostics.len(), 1);
        assert_eq!(parsed.diagnostics[0].line, 2);
        assert_eq!(
            parsed.diagnostics[0].message,
            "unsupported %if block skipped"
        );
    }

    #[test]
    fn reports_stray_and_unterminated_conditionals() {
        let stray = parse_config("test.conf", "%endif\nbind c new-window\n");
        assert_eq!(stray.commands.len(), 1);
        assert_eq!(stray.diagnostics[0].message, "%endif outside %if");

        let unterminated = parse_config("test.conf", "%if cond\nbind x kill-pane\n");
        assert!(unterminated.commands.is_empty());
        assert_eq!(unterminated.diagnostics.len(), 1);
        assert_eq!(unterminated.diagnostics[0].message, "unterminated %if");
    }

    #[test]
    fn reports_a_trailing_escape_and_preserves_completed_commands() {
        let parsed = parse_config("test.conf", "set -g prefix C-a\nbind c new-window\\");
        assert_eq!(parsed.commands.len(), 1);
        assert_eq!(parsed.commands[0].name, "set");
        assert_eq!(parsed.diagnostics.len(), 1);
        assert_eq!(parsed.diagnostics[0].line, 2);
        assert_eq!(parsed.diagnostics[0].column, 18);
        assert_eq!(parsed.diagnostics[0].message, "trailing escape");
    }
}
