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

#[derive(Clone, Copy, Debug)]
struct Block {
    depth: u32,
    quote: Quote,
    escaped: bool,
    in_comment: bool,
    word_start: bool,
    line: u32,
    column: u32,
}

impl Block {
    fn open(line: u32, column: u32) -> Self {
        Self {
            depth: 1,
            quote: Quote::None,
            escaped: false,
            in_comment: false,
            word_start: true,
            line,
            column,
        }
    }

    fn feed(&mut self, character: char) -> bool {
        if self.in_comment {
            if character == '\n' {
                self.in_comment = false;
                self.word_start = true;
            }
            return false;
        }
        if self.escaped {
            self.escaped = false;
            self.word_start = false;
            return false;
        }
        if character == '\\' && self.quote != Quote::Single {
            self.escaped = true;
            return false;
        }
        match self.quote {
            Quote::Single if character == '\'' => self.quote = Quote::None,
            Quote::Double if character == '"' => self.quote = Quote::None,
            Quote::Single | Quote::Double => {}
            Quote::None => match character {
                '\'' => self.quote = Quote::Single,
                '"' => self.quote = Quote::Double,
                '#' if self.word_start => self.in_comment = true,
                '{' => self.depth = self.depth.saturating_add(1),
                '}' => {
                    self.depth = self.depth.saturating_sub(1);
                    if self.depth == 0 {
                        return true;
                    }
                }
                _ => {}
            },
        }
        self.word_start = character.is_whitespace();
        false
    }
}

pub fn command_block_body(argument: &str) -> Option<&str> {
    argument
        .strip_prefix('{')
        .and_then(|rest| rest.strip_suffix('}'))
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
    let mut block: Option<Block> = None;
    let mut line = 1_u32;
    let mut column = 0_u32;
    let mut command_line = 1_u32;
    let mut command_column = 1_u32;

    let mut characters = input.chars();
    let mut reprocess: Option<char> = None;
    let mut tilde: Option<String> = None;
    let mut tilde_after_quote = false;
    loop {
        let character = if let Some(character) = reprocess.take() {
            character
        } else {
            let Some(character) = characters.next() else {
                break;
            };
            column = column.saturating_add(1);
            character
        };
        if let Some(name) = tilde.as_mut() {
            if matches!(character, '/' | ' ' | '\t' | '\n' | '"' | '\'') {
                let name = tilde.take().unwrap_or_default();
                expand_tilde(&mut word, &name);
                reprocess = Some(character);
            } else {
                name.push(character);
            }
            continue;
        }
        if let Some(state) = block.as_mut() {
            word.push(character);
            if character == '\n' {
                line = line.saturating_add(1);
                column = 0;
            }
            if state.feed(character) {
                block = None;
                finish_word(&mut word, &mut word_started, &mut words);
            }
            continue;
        }
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
            tilde_after_quote = false;
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
            Quote::Double if character == '~' && tilde_after_quote => {
                tilde_after_quote = false;
                tilde = Some(String::new());
            }
            Quote::Single | Quote::Double => {
                tilde_after_quote = false;
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
                    tilde_after_quote = true;
                }
                '#' if !word_started => in_comment = true,
                '{' if !word_started => {
                    if words.is_empty() {
                        command_line = line;
                        command_column = column;
                    }
                    word_started = true;
                    word.push('{');
                    block = Some(Block::open(line, column));
                }
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
                '~' if !word_started => {
                    if words.is_empty() {
                        command_line = line;
                        command_column = column;
                    }
                    word_started = true;
                    tilde = Some(String::new());
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
    if let Some(name) = tilde.take() {
        expand_tilde(&mut word, &name);
    }
    if let Some(state) = block {
        parsed.diagnostics.push(ConfigDiagnostic {
            source,
            line: state.line,
            column: state.column,
            message: "unterminated command block".to_owned(),
        });
    } else if quote != Quote::None {
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

fn expand_tilde(word: &mut String, name: &str) {
    if name.is_empty()
        && let Ok(home) = std::env::var("HOME")
        && !home.is_empty()
    {
        word.push_str(&home);
        return;
    }
    word.push('~');
    word.push_str(name);
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
    fn expands_leading_tildes_like_the_pin() {
        let home = std::env::var("HOME").expect("test environment has HOME");
        let parsed = parse_config(
            "<test>",
            "run-shell ~/bin/x \"~/quoted\" '~/literal' \\~/escaped ~name/x ~",
        );
        assert!(parsed.diagnostics.is_empty());
        let command = &parsed.commands[0];
        assert_eq!(command.args[0], format!("{home}/bin/x"));
        assert_eq!(command.args[1], format!("{home}/quoted"));
        assert_eq!(command.args[2], "~/literal");
        assert_eq!(command.args[3], "~/escaped");
        assert_eq!(command.args[4], "~name/x");
        assert_eq!(command.args[5], home);
    }

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
    fn groups_a_command_block_into_one_argument() {
        let parsed = parse_config("test.conf", "bind c { new-window ; split-window }\n");
        assert!(parsed.diagnostics.is_empty());
        assert_eq!(parsed.commands.len(), 1);
        assert_eq!(parsed.commands[0].name, "bind");
        assert_eq!(
            parsed.commands[0].args,
            ["c", "{ new-window ; split-window }"]
        );
    }

    #[test]
    fn accepts_empty_blocks_and_final_escaped_separators() {
        let parsed = parse_config("test.conf", r"bind x {}");
        assert!(parsed.diagnostics.is_empty());
        assert_eq!(parsed.commands[0].args, ["x", "{}"]);

        let parsed = parse_config("test.conf", r"bind y new-window \;");
        assert!(parsed.diagnostics.is_empty());
        assert_eq!(parsed.commands[0].args, ["y", "new-window", ";"]);
    }

    #[test]
    fn command_blocks_nest_and_span_lines_and_comments() {
        let parsed = parse_config(
            "test.conf",
            "bind x {\n\
             \x20 if-shell true { new-window ; kill-pane }\n\
             \x20 # a } comment\n\
             \x20 split-window\n\
             }\nbind y kill-pane\n",
        );
        assert!(parsed.diagnostics.is_empty());
        assert_eq!(parsed.commands.len(), 2);
        assert_eq!(parsed.commands[0].args[0], "x");
        assert_eq!(
            parsed.commands[0].args[1],
            "{\n  if-shell true { new-window ; kill-pane }\n  # a } comment\n  split-window\n}"
        );
        assert_eq!(parsed.commands[1].args, ["y", "kill-pane"]);
    }

    #[test]
    fn braces_stay_literal_inside_quotes_and_words() {
        let parsed = parse_config(
            "test.conf",
            "bind z send-keys \"{ literal ; text }\"\nbind w new-window -n a{b}c\n",
        );
        assert!(parsed.diagnostics.is_empty());
        assert_eq!(parsed.commands.len(), 2);
        assert_eq!(
            parsed.commands[0].args,
            ["z", "send-keys", "{ literal ; text }"]
        );
        assert_eq!(parsed.commands[1].args, ["w", "new-window", "-n", "a{b}c"]);
    }

    #[test]
    fn command_blocks_keep_quoted_separators_and_report_unterminated_blocks() {
        let quoted = parse_config("test.conf", "bind w { send-keys 'a ; b' ; new-window }");
        assert!(quoted.diagnostics.is_empty());
        assert_eq!(
            quoted.commands[0].args[1],
            "{ send-keys 'a ; b' ; new-window }"
        );

        let unterminated = parse_config("test.conf", "set -g prefix C-a\nbind c { new-window\n");
        assert_eq!(unterminated.commands.len(), 1);
        assert_eq!(unterminated.commands[0].name, "set");
        assert_eq!(unterminated.diagnostics.len(), 1);
        assert_eq!(unterminated.diagnostics[0].line, 2);
        assert_eq!(unterminated.diagnostics[0].column, 8);
        assert_eq!(
            unterminated.diagnostics[0].message,
            "unterminated command block"
        );
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
