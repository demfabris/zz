use std::collections::BTreeMap;

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
    pub environment: Vec<ConfigEnvironmentAssignment>,
    pub diagnostics: Vec<ConfigDiagnostic>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfigEnvironmentAssignment {
    pub name: String,
    pub value: String,
    pub hidden: bool,
}

pub(crate) trait ConfigContext {
    fn variable(&mut self, name: &str) -> Option<String>;
    fn condition(&mut self, condition: &str) -> bool;

    fn expand_variables(&self) -> bool {
        true
    }
}

impl<V, C> ConfigContext for (V, C)
where
    V: FnMut(&str) -> Option<String>,
    C: FnMut(&str) -> bool,
{
    fn variable(&mut self, name: &str) -> Option<String> {
        self.0(name)
    }

    fn condition(&mut self, condition: &str) -> bool {
        self.1(condition)
    }
}

#[derive(Clone, Copy, Debug)]
struct ConditionalScope {
    parent_active: bool,
    branch_taken: bool,
    active: bool,
    saw_else: bool,
}

struct ConfigBuilder<'a, C> {
    source: String,
    parsed: ParsedConfig,
    overlay: BTreeMap<String, String>,
    conditionals: Vec<ConditionalScope>,
    context: &'a mut C,
    aborted: bool,
}

impl<C: ConfigContext> ConfigBuilder<'_, C> {
    fn active(&self) -> bool {
        self.conditionals.last().is_none_or(|scope| scope.active)
    }

    fn variable(&mut self, name: &str) -> Option<String> {
        self.overlay
            .get(name)
            .cloned()
            .or_else(|| self.context.variable(name))
    }

    fn expand_variables(&self) -> bool {
        self.context.expand_variables()
    }

    fn aborted(&self) -> bool {
        self.aborted
    }

    fn diagnostic(&mut self, line: u32, column: u32, message: impl Into<String>) {
        if self.aborted {
            return;
        }
        self.parsed.commands.clear();
        self.parsed.diagnostics.push(ConfigDiagnostic {
            source: self.source.clone(),
            line,
            column,
            message: message.into(),
        });
        self.aborted = true;
    }

    fn finish_word(
        &mut self,
        line: u32,
        column: u32,
        word: &mut String,
        word_started: &mut bool,
        word_is_command_block: &mut bool,
        words: &mut Vec<String>,
        command_block_words: &mut Vec<usize>,
        eager_assignment: &mut bool,
    ) {
        if !*word_started {
            return;
        }
        if *word_is_command_block {
            command_block_words.push(words.len());
        }
        words.push(std::mem::take(word));
        *word_started = false;
        *word_is_command_block = false;
        if words.len() != 1 {
            return;
        }
        match parse_assignment(&words[0]) {
            Ok(Some((name, value))) => {
                self.push_assignment(name, value, false);
                *eager_assignment = true;
            }
            Ok(None) => {}
            Err(()) => self.diagnostic(line, column, "environment variable is too long"),
        }
    }

    fn finish_statement(
        &mut self,
        line: u32,
        column: u32,
        completion_line: u32,
        word: &mut String,
        word_started: &mut bool,
        word_is_command_block: &mut bool,
        words: &mut Vec<String>,
        command_block_words: &mut Vec<usize>,
        eager_assignment: &mut bool,
    ) {
        self.finish_word(
            line,
            column,
            word,
            word_started,
            word_is_command_block,
            words,
            command_block_words,
            eager_assignment,
        );
        if self.aborted {
            return;
        }
        if words.is_empty() {
            command_block_words.clear();
            return;
        }
        let tokens = std::mem::take(words);
        let command_block_tokens = std::mem::take(command_block_words);
        if tokens.first().is_some_and(|token| token == "%hidden") {
            self.finish_hidden(line, column, &tokens);
            return;
        }

        let mut start = 0;
        while start < tokens.len() {
            match tokens[start].as_str() {
                "%if" | "%elif" => {
                    let Some(condition) = tokens.get(start + 1) else {
                        self.diagnostic(line, column, "syntax error");
                        return;
                    };
                    self.finish_conditional(&tokens[start], Some(condition.as_str()), line, column);
                    start += 2;
                }
                "%else" | "%endif" => {
                    self.finish_conditional(&tokens[start], None, line, column);
                    start += 1;
                }
                token if is_invalid_percent_token(token) => {
                    self.diagnostic(line, column, "syntax error");
                    return;
                }
                _ => {
                    let end = tokens[start + 1..]
                        .iter()
                        .position(|token| is_conditional_token(token))
                        .map_or(tokens.len(), |offset| start + 1 + offset);
                    let command_blocks = command_block_tokens
                        .iter()
                        .copied()
                        .filter(|index| *index >= start && *index < end)
                        .map(|index| index - start)
                        .collect::<Vec<_>>();
                    self.finish_command_tokens(
                        line,
                        column,
                        completion_line,
                        &tokens[start..end],
                        &command_blocks,
                        start == 0 && *eager_assignment,
                    );
                    start = end;
                }
            }
            if self.aborted {
                break;
            }
        }
        *eager_assignment = false;
    }

    fn finish_hidden(&mut self, line: u32, column: u32, tokens: &[String]) {
        if tokens.len() != 2 {
            self.diagnostic(line, column, "syntax error");
            return;
        }
        let (name, value) = match parse_assignment(&tokens[1]) {
            Ok(Some(assignment)) => assignment,
            Ok(None) => {
                self.diagnostic(line, column, "syntax error");
                return;
            }
            Err(()) => {
                self.diagnostic(line, column, "environment variable is too long");
                return;
            }
        };
        self.push_assignment(name, value, true);
    }

    fn finish_conditional(
        &mut self,
        directive: &str,
        condition: Option<&str>,
        line: u32,
        column: u32,
    ) {
        match directive {
            "%if" => {
                let condition = self
                    .context
                    .condition(condition.expect("if condition was checked"));
                let parent_active = self.active();
                self.conditionals.push(ConditionalScope {
                    parent_active,
                    branch_taken: condition,
                    active: parent_active && condition,
                    saw_else: false,
                });
            }
            "%elif" => {
                let condition = self
                    .context
                    .condition(condition.expect("elif condition was checked"));
                let Some(scope) = self.conditionals.last_mut() else {
                    self.diagnostic(line, column, "syntax error");
                    return;
                };
                if scope.saw_else {
                    self.diagnostic(line, column, "syntax error");
                    return;
                }
                scope.active = scope.parent_active && !scope.branch_taken && condition;
                scope.branch_taken |= condition;
            }
            "%else" => {
                let Some(scope) = self.conditionals.last_mut() else {
                    self.diagnostic(line, column, "syntax error");
                    return;
                };
                if scope.saw_else {
                    self.diagnostic(line, column, "syntax error");
                    return;
                }
                scope.active = scope.parent_active && !scope.branch_taken;
                scope.branch_taken = true;
                scope.saw_else = true;
            }
            "%endif" => {
                if self.conditionals.pop().is_none() {
                    self.diagnostic(line, column, "syntax error");
                }
            }
            _ => unreachable!("conditional token was checked"),
        }
    }

    fn finish_command_tokens(
        &mut self,
        line: u32,
        column: u32,
        completion_line: u32,
        tokens: &[String],
        command_block_tokens: &[usize],
        assignment_recorded: bool,
    ) {
        let mut tokens = tokens.iter().cloned();
        let mut command_name = tokens.next().expect("command has a name");
        let mut command_args = tokens.collect::<Vec<_>>();
        let mut argument_start = 1;
        match parse_assignment(&command_name) {
            Ok(Some((name, value))) => {
                if !assignment_recorded {
                    self.push_assignment(name, value, false);
                }
                if command_args.is_empty() {
                    return;
                }
                command_name = command_args.remove(0);
                argument_start = 2;
                if parse_assignment(&command_name).ok().flatten().is_some() {
                    self.diagnostic(line, column, "syntax error");
                    return;
                }
            }
            Ok(None) => {}
            Err(()) => {
                self.diagnostic(line, column, "environment variable is too long");
                return;
            }
        }
        if command_block_tokens.contains(&(argument_start - 1)) {
            self.diagnostic(line, column, "syntax error");
            return;
        }
        if !self.active() {
            return;
        }
        let command_blocks = command_block_tokens
            .iter()
            .filter_map(|index| index.checked_sub(argument_start));
        self.parsed.commands.push(
            CommandInvocation::new(command_name, command_args)
                .with_command_blocks(command_blocks)
                .with_source(SourceSpan {
                    source: self.source.clone(),
                    line: completion_line,
                    column,
                }),
        );
    }

    fn push_assignment(&mut self, name: String, value: String, hidden: bool) {
        if !self.active() {
            return;
        }
        self.overlay.insert(name.clone(), value.clone());
        self.parsed.environment.push(ConfigEnvironmentAssignment {
            name,
            value,
            hidden,
        });
    }

    fn finish(mut self, line: u32, column: u32) -> ParsedConfig {
        if !self.aborted && !self.conditionals.is_empty() {
            self.diagnostic(line, column.saturating_add(1), "syntax error");
        }
        self.parsed
    }
}

struct LiteralVariableContext;

impl ConfigContext for LiteralVariableContext {
    fn variable(&mut self, _name: &str) -> Option<String> {
        None
    }

    fn condition(&mut self, _condition: &str) -> bool {
        false
    }

    fn expand_variables(&self) -> bool {
        false
    }
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
    let mut context = (|_: &str| None::<String>, |_: &str| false);
    parse_config_with(source, input, &mut context)
}

pub(crate) fn parse_config_without_variable_expansion(
    source: impl Into<String>,
    input: &str,
) -> ParsedConfig {
    parse_config_with(source, input, &mut LiteralVariableContext)
}

pub(crate) fn parse_config_with<C: ConfigContext>(
    source: impl Into<String>,
    input: &str,
    context: &mut C,
) -> ParsedConfig {
    let source = source.into();
    let mut builder = ConfigBuilder {
        source,
        parsed: ParsedConfig::default(),
        overlay: BTreeMap::new(),
        conditionals: Vec::new(),
        context,
        aborted: false,
    };
    let mut words = Vec::new();
    let mut command_block_words = Vec::new();
    let mut word = String::new();
    let mut word_started = false;
    let mut word_is_command_block = false;
    let mut quote = Quote::None;
    let mut in_comment = false;
    let mut block: Option<Block> = None;
    let mut eager_assignment = false;
    let mut line = 1_u32;
    let mut column = 0_u32;
    let mut command_line = 1_u32;
    let mut command_column = 1_u32;

    let mut characters = input.chars().peekable();
    let mut reprocess: Option<char> = None;
    let mut tilde: Option<String> = None;
    let mut tilde_after_quote = false;
    loop {
        if builder.aborted() {
            break;
        }
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
                finish_word(
                    &mut word,
                    &mut word_started,
                    &mut word_is_command_block,
                    &mut words,
                    &mut command_block_words,
                );
            }
            continue;
        }
        if in_comment {
            if character == '\n' {
                builder.finish_statement(
                    command_line,
                    command_column,
                    line,
                    &mut word,
                    &mut word_started,
                    &mut word_is_command_block,
                    &mut words,
                    &mut command_block_words,
                    &mut eager_assignment,
                );
                in_comment = false;
                line = line.saturating_add(1);
                column = 0;
                command_line = line;
                command_column = 1;
            }
            continue;
        }
        if character == '\\' && quote != Quote::Single {
            let escape_line = line;
            let escape_column = column;
            tilde_after_quote = false;
            if !word_started && words.is_empty() {
                command_line = line;
                command_column = column;
            }
            word_started = true;
            match parse_escape(&mut characters, &mut line, &mut column) {
                Ok(Some(value)) => word.push(value),
                Ok(None) => {}
                Err(message) => {
                    builder.diagnostic(escape_line, escape_column, message);
                }
            }
            continue;
        }
        if character == '\\' && quote == Quote::Single && characters.peek() == Some(&'\n') {
            take_character(&mut characters, &mut column);
            line = line.saturating_add(1);
            column = 0;
            continue;
        }
        if character == '$' && quote != Quote::Single && builder.expand_variables() {
            tilde_after_quote = false;
            if !word_started && words.is_empty() {
                command_line = line;
                command_column = column;
            }
            word_started = true;
            match expand_variable(&mut characters, &mut column, &mut builder) {
                Ok(value) => word.push_str(&value),
                Err(message) => {
                    builder.diagnostic(line, column, message);
                }
            }
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
                    strip_quoted_line_prefix(
                        &mut characters,
                        &mut reprocess,
                        &mut word,
                        &mut column,
                    );
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
                '#' if !word_started
                    && words
                        .last()
                        .is_some_and(|word| matches!(word.as_str(), "%if" | "%elif"))
                    && characters.peek() == Some(&'{') =>
                {
                    let format_line = line;
                    let format_column = column;
                    word_started = true;
                    if let Err(message) =
                        scan_condition_format(&mut characters, &mut word, &mut line, &mut column)
                    {
                        builder.diagnostic(format_line, format_column, message);
                    }
                }
                '#' if !word_started
                    && words
                        .last()
                        .is_some_and(|word| matches!(word.as_str(), "%else" | "%endif"))
                    && characters.peek() == Some(&'{') =>
                {
                    builder.diagnostic(line, column, "syntax error");
                }
                '#' if !word_started => in_comment = true,
                '{' if !word_started => {
                    if words.is_empty() {
                        command_line = line;
                        command_column = column;
                    }
                    word_started = true;
                    word_is_command_block = true;
                    word.push('{');
                    block = Some(Block::open(line, column));
                }
                ';' | '\n' => {
                    builder.finish_statement(
                        command_line,
                        command_column,
                        line,
                        &mut word,
                        &mut word_started,
                        &mut word_is_command_block,
                        &mut words,
                        &mut command_block_words,
                        &mut eager_assignment,
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
                    builder.finish_word(
                        command_line,
                        command_column,
                        &mut word,
                        &mut word_started,
                        &mut word_is_command_block,
                        &mut words,
                        &mut command_block_words,
                        &mut eager_assignment,
                    );
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
    if !builder.aborted() {
        if let Some(name) = tilde.take() {
            expand_tilde(&mut word, &name);
        }
        if let Some(state) = block {
            builder.diagnostic(state.line, state.column, "unterminated command block");
        } else {
            builder.finish_statement(
                command_line,
                command_column,
                line,
                &mut word,
                &mut word_started,
                &mut word_is_command_block,
                &mut words,
                &mut command_block_words,
                &mut eager_assignment,
            );
        }
    }
    builder.finish(line, column)
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

fn finish_word(
    word: &mut String,
    word_started: &mut bool,
    word_is_command_block: &mut bool,
    words: &mut Vec<String>,
    command_block_words: &mut Vec<usize>,
) {
    if *word_started {
        if *word_is_command_block {
            command_block_words.push(words.len());
        }
        words.push(std::mem::take(word));
        *word_started = false;
        *word_is_command_block = false;
    }
}

fn scan_condition_format<I>(
    characters: &mut std::iter::Peekable<I>,
    word: &mut String,
    line: &mut u32,
    column: &mut u32,
) -> Result<(), String>
where
    I: Iterator<Item = char>,
{
    word.push('#');
    let Some(open) = take_character(characters, column) else {
        return Err("syntax error".to_owned());
    };
    word.push(open);
    let mut depth = 1_u32;
    loop {
        let Some(character) = take_character(characters, column) else {
            return Err("syntax error".to_owned());
        };
        if character == '\n' {
            *line = line.saturating_add(1);
            *column = 0;
            return Err("syntax error".to_owned());
        }
        word.push(character);
        if character == '#' {
            let Some(next) = take_character(characters, column) else {
                return Err("syntax error".to_owned());
            };
            if next == '\n' {
                *line = line.saturating_add(1);
                *column = 0;
                return Err("syntax error".to_owned());
            }
            word.push(next);
            if next == '{' {
                depth = depth.saturating_add(1);
            }
        } else if character == '}' {
            depth = depth.saturating_sub(1);
            if depth == 0 {
                return Ok(());
            }
        }
    }
}

fn parse_assignment(token: &str) -> Result<Option<(String, String)>, ()> {
    let Some((name, value)) = token.split_once('=') else {
        return Ok(None);
    };
    let mut characters = name.chars();
    let Some(first) = characters.next() else {
        return Ok(None);
    };
    if !is_variable_character(first, true)
        || !characters.all(|character| is_variable_character(character, false))
    {
        return Ok(None);
    }
    if token.len() > 16_384 {
        return Err(());
    }
    Ok(Some((name.to_owned(), value.to_owned())))
}

fn is_conditional_token(token: &str) -> bool {
    matches!(token, "%if" | "%elif" | "%else" | "%endif")
}

fn is_invalid_percent_token(token: &str) -> bool {
    token.starts_with('%')
        && !token
            .chars()
            .all(|character| character == '%' || character.is_ascii_digit())
}

fn is_variable_character(character: char, first: bool) -> bool {
    character != '='
        && (!first || !character.is_ascii_digit())
        && (character.is_ascii_alphanumeric() || character == '_')
}

fn parse_escape<I>(
    characters: &mut std::iter::Peekable<I>,
    line: &mut u32,
    column: &mut u32,
) -> Result<Option<char>, String>
where
    I: Iterator<Item = char>,
{
    let Some(character) = take_character(characters, column) else {
        return Err("syntax error".to_owned());
    };
    if character == '\n' {
        *line = line.saturating_add(1);
        *column = 0;
        return Ok(None);
    }
    if matches!(character, '4'..='7') {
        return Err("invalid octal escape".to_owned());
    }
    if matches!(character, '0'..='3') {
        let second = take_character(characters, column);
        let third = second
            .filter(|value| matches!(value, '0'..='7'))
            .and_then(|_| take_character(characters, column));
        let (Some(second), Some(third)) = (second, third) else {
            return Err("invalid octal escape".to_owned());
        };
        if !matches!(third, '0'..='7') {
            return Err("invalid octal escape".to_owned());
        }
        let value = 64 * (character as u32 - '0' as u32)
            + 8 * (second as u32 - '0' as u32)
            + (third as u32 - '0' as u32);
        return Ok(char::from_u32(value));
    }
    let value = match character {
        'a' => '\u{7}',
        'b' => '\u{8}',
        'e' => '\u{1b}',
        'f' => '\u{c}',
        's' => ' ',
        'v' => '\u{b}',
        'r' => '\r',
        'n' => '\n',
        't' => '\t',
        'u' | 'U' => {
            let digits = if character == 'u' { 4 } else { 8 };
            let mut encoded = String::with_capacity(digits);
            for _ in 0..digits {
                let Some(digit) = take_character(characters, column) else {
                    return Err("syntax error".to_owned());
                };
                if digit == '\n' {
                    *line = line.saturating_add(1);
                    *column = 0;
                    return Err("syntax error".to_owned());
                }
                if !digit.is_ascii_hexdigit() {
                    return Err(format!("invalid \\{character} argument"));
                }
                encoded.push(digit);
            }
            let value = u32::from_str_radix(&encoded, 16)
                .ok()
                .and_then(char::from_u32)
                .ok_or_else(|| format!("invalid \\{character} argument"))?;
            return Ok(Some(value));
        }
        other => other,
    };
    Ok(Some(value))
}

fn expand_variable<I, C>(
    characters: &mut std::iter::Peekable<I>,
    column: &mut u32,
    builder: &mut ConfigBuilder<'_, C>,
) -> Result<String, String>
where
    I: Iterator<Item = char>,
    C: ConfigContext,
{
    let Some(&next) = characters.peek() else {
        return Err("syntax error".to_owned());
    };
    let braced = next == '{';
    if braced {
        take_character(characters, column);
    } else if !is_variable_character(next, true) {
        return Ok("$".to_owned());
    }

    let mut name = String::new();
    loop {
        let Some(&next) = characters.peek() else {
            if braced {
                return Err("invalid environment variable".to_owned());
            }
            break;
        };
        if braced && next == '}' {
            take_character(characters, column);
            break;
        }
        if !is_variable_character(next, false) {
            if braced {
                take_character(characters, column);
                return Err("invalid environment variable".to_owned());
            }
            break;
        }
        if name.len() == 1022 {
            return Err("environment variable is too long".to_owned());
        }
        name.push(next);
        take_character(characters, column);
    }
    Ok(builder.variable(&name).unwrap_or_default())
}

fn take_character<I>(characters: &mut std::iter::Peekable<I>, column: &mut u32) -> Option<char>
where
    I: Iterator<Item = char>,
{
    let character = characters.next()?;
    *column = column.saturating_add(1);
    Some(character)
}

fn strip_quoted_line_prefix<I>(
    characters: &mut std::iter::Peekable<I>,
    reprocess: &mut Option<char>,
    word: &mut String,
    column: &mut u32,
) where
    I: Iterator<Item = char>,
{
    let Some(mut character) = take_character(characters, column) else {
        return;
    };
    while matches!(character, ' ' | '\t') {
        let Some(next) = take_character(characters, column) else {
            return;
        };
        character = next;
    }
    if character != '#' {
        *reprocess = Some(character);
        return;
    }
    let Some(next) = take_character(characters, column) else {
        return;
    };
    if matches!(next, ',' | '#' | '{' | '}' | ':') {
        word.push('#');
        *reprocess = Some(next);
        return;
    }
    if next == '\n' {
        *reprocess = Some(next);
        return;
    }
    while let Some(next) = take_character(characters, column) {
        if next == '\n' {
            *reprocess = Some(next);
            return;
        }
    }
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
    fn continues_lines_and_finishes_open_quotes_at_eof() {
        let parsed = parse_config("test.conf", "bind c new-\\\nwindow\nset 'oops");
        assert!(parsed.diagnostics.is_empty());
        assert_eq!(parsed.commands.len(), 2);
        assert_eq!(parsed.commands[0].name, "bind");
        assert_eq!(parsed.commands[0].args, ["c", "new-window"]);
        assert_eq!(parsed.commands[1].name, "set");
        assert_eq!(parsed.commands[1].args, ["oops"]);
    }

    #[test]
    fn finishes_a_trailing_open_quote_before_command_validation() {
        let parsed = parse_config("<test>", "display-message -p \"\" ; wibble\"");
        assert!(parsed.diagnostics.is_empty());
        assert_eq!(parsed.commands.len(), 2);
        assert_eq!(parsed.commands[0].args, ["-p", ""]);
        assert_eq!(parsed.commands[1].name, "wibble");
    }

    #[test]
    fn physical_multiline_command_source_uses_completion_line() {
        let parsed = parse_config(
            "test.conf",
            "display-message -p \"MULTI_FIRST\nMULTI_SECOND\"\n",
        );
        assert!(parsed.diagnostics.is_empty());
        assert_eq!(
            parsed.commands[0].source,
            Some(SourceSpan {
                source: "test.conf".to_owned(),
                line: 2,
                column: 1,
            })
        );
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
    fn evaluates_conditionals_with_elif_else_and_nesting() {
        let mut context = (
            |_: &str| None::<String>,
            |condition: &str| matches!(condition, "yes" | "also-yes"),
        );
        let parsed = parse_config_with(
            "test.conf",
            "set -g prefix C-a\n\
             %if yes\n\
             set @first kept\n\
             %if no\n\
             set @nested dropped\n\
             %else\n\
             set @nested kept\n\
             %endif\n\
             %elif also-yes\n\
             set @elif dropped\n\
             %else\n\
             set @else dropped\n\
             %endif\n\
             %if no\n\
             set @false dropped\n\
             %elif also-yes\n\
             set @elif kept\n\
             %else\n\
             set @fallback dropped\n\
             %endif\n\
             %if no\n\
             set @false2 dropped\n\
             %else\n\
             set @fallback kept\n\
             %endif\n",
            &mut context,
        );
        let names: Vec<&str> = parsed
            .commands
            .iter()
            .filter_map(|command| command.args.first().map(String::as_str))
            .collect();
        assert_eq!(names, ["-g", "@first", "@nested", "@elif", "@fallback"]);
        assert!(parsed.diagnostics.is_empty());
    }

    #[test]
    fn evaluates_same_line_and_semicolon_conditionals() {
        let mut context = (
            |_: &str| None::<String>,
            |condition: &str| condition == "yes",
        );
        let parsed = parse_config_with(
            "test.conf",
            "%if yes set @inline kept %else set @inline wrong %endif\n\
             set @before kept; %if yes set @mixed kept %endif; set @after kept\n",
            &mut context,
        );
        let options = parsed
            .commands
            .iter()
            .map(|command| command.args[0].as_str())
            .collect::<Vec<_>>();
        assert_eq!(options, ["@inline", "@before", "@mixed", "@after"]);
        assert!(parsed.diagnostics.is_empty());
    }

    #[test]
    fn reports_stray_and_unterminated_conditionals() {
        for directive in ["%endif", "%else", "%elif 1"] {
            let stray = parse_config("test.conf", &format!("{directive}\nbind c new-window\n"));
            assert!(stray.commands.is_empty());
            assert!(stray.environment.is_empty());
            assert_eq!(stray.diagnostics.len(), 1);
            assert_eq!(stray.diagnostics[0].message, "syntax error");
        }

        let unterminated = parse_config("test.conf", "%if cond\nbind x kill-pane\n");
        assert!(unterminated.commands.is_empty());
        assert_eq!(unterminated.diagnostics.len(), 1);
        assert_eq!(unterminated.diagnostics[0].line, 3);
        assert_eq!(unterminated.diagnostics[0].message, "syntax error");
    }

    #[test]
    fn balanced_condition_formats_include_whitespace_and_nesting() {
        let expected = "#{==:#{l:a b},a b}";
        let mut context = (
            |_: &str| None::<String>,
            |condition: &str| condition == expected,
        );
        let parsed = parse_config_with(
            "test.conf",
            "%if #{==:#{l:a b},a b}\nset @branch selected\n%endif\n",
            &mut context,
        );
        assert!(parsed.diagnostics.is_empty());
        assert_eq!(parsed.commands.len(), 1);
        assert_eq!(parsed.commands[0].args, ["@branch", "selected"]);

        let mut dead_context = (|_: &str| None::<String>, |_: &str| false);
        let malformed = parse_config_with(
            "test.conf",
            "%if false\n%if #{unterminated\nset @branch wrong\n%endif\n%endif\n",
            &mut dead_context,
        );
        assert!(malformed.commands.is_empty());
        assert!(malformed.environment.is_empty());
        assert_eq!(malformed.diagnostics.len(), 1);
        assert_eq!(malformed.diagnostics[0].message, "syntax error");
    }

    #[test]
    fn same_line_nested_condition_formats_follow_each_if_token() {
        let mut taken_context = (
            |_: &str| None::<String>,
            |condition: &str| matches!(condition, "1" | "#{==:a b,a b}"),
        );
        let taken = parse_config_with(
            "test.conf",
            "%if 1 %if #{==:a b,a b} set-environment -g NEST taken %endif %endif",
            &mut taken_context,
        );
        assert!(taken.diagnostics.is_empty());
        assert_eq!(taken.commands.len(), 1);
        assert_eq!(taken.commands[0].args, ["-g", "NEST", "taken"]);

        let mut skipped_context = (
            |_: &str| None::<String>,
            |condition: &str| matches!(condition, "1" | "#{==:a b,a b}"),
        );
        let skipped = parse_config_with(
            "test.conf",
            "%if 1 %if #{==:a b,c d} set-environment -g NEST wrong %endif %endif",
            &mut skipped_context,
        );
        assert!(skipped.diagnostics.is_empty());
        assert!(skipped.commands.is_empty());
    }

    #[test]
    fn condition_formats_after_else_or_endif_are_syntax_errors() {
        for input in [
            "%if 0\n%else #{==:a,b}\nset @after wrong\n%endif\n",
            "%if 0\n%endif #{==:a,b}\nset @after wrong\n",
        ] {
            let parsed = parse_config("test.conf", input);
            assert!(parsed.commands.is_empty(), "{input}");
            assert_eq!(parsed.diagnostics.len(), 1, "{input}");
            assert_eq!(parsed.diagnostics[0].line, 2, "{input}");
            assert_eq!(parsed.diagnostics[0].message, "syntax error", "{input}");
        }
    }

    #[test]
    fn expands_variables_in_bare_and_double_quoted_words_only() {
        let variables = BTreeMap::from([
            ("VAR".to_owned(), "value".to_owned()),
            ("A1_".to_owned(), "named".to_owned()),
            ("9".to_owned(), "braced-digit".to_owned()),
        ]);
        let mut context = (|name: &str| variables.get(name).cloned(), |_: &str| false);
        let parsed = parse_config_with(
            "test.conf",
            r#"set @vars pre$VAR-${VAR} "$VAR/${VAR}" '$VAR/${VAR}' $MISSING \$VAR $A1_ $9 ${9}"#,
            &mut context,
        );
        assert!(parsed.diagnostics.is_empty());
        assert_eq!(
            parsed.commands[0].args,
            [
                "@vars",
                "prevalue-value",
                "value/value",
                "$VAR/${VAR}",
                "",
                "$VAR",
                "named",
                "$9",
                "braced-digit",
            ]
        );
    }

    #[test]
    fn expands_every_pin_escape_in_bare_and_double_quoted_words() {
        let parsed = parse_config(
            "test.conf",
            r#"set @bare \141\a\b\e\f\s\v\r\n\t\u03bb\U0001F980
set @double "\141\a\b\e\f\s\v\r\n\t\u03bb\U0001F980"
set @single '\141\a\b\e\f\s\v\r\n\t\u03bb\U0001F980'"#,
        );
        let expected = "a\u{7}\u{8}\u{1b}\u{c} \u{b}\r\n\tλ🦀";
        assert!(parsed.diagnostics.is_empty());
        assert_eq!(parsed.commands[0].args[1], expected);
        assert_eq!(parsed.commands[1].args[1], expected);
        assert_eq!(
            parsed.commands[2].args[1],
            r"\141\a\b\e\f\s\v\r\n\t\u03bb\U0001F980"
        );
    }

    #[test]
    fn represents_high_octal_and_nul_escapes_as_rust_string_characters() {
        let parsed = parse_config("test.conf", r"set @bytes \377 \000");
        assert!(parsed.diagnostics.is_empty());
        assert_eq!(parsed.commands[0].args[1], "\u{ff}");
        assert_eq!(parsed.commands[0].args[2], "\0");
        // The pin stores raw bytes (0xff and NUL truncation); Rust String requires UTF-8 and retains NUL.
    }

    #[test]
    fn rejects_invalid_octal_and_unicode_escapes_like_the_pin() {
        for (input, message) in [
            (r"set @x \400", "invalid octal escape"),
            (r"set @x \12x", "invalid octal escape"),
            (r"set @x \u12xz", "invalid \\u argument"),
            (r"set @x \U00110000", "invalid \\U argument"),
            (r"set @x \u12", "syntax error"),
        ] {
            let parsed = parse_config("test.conf", input);
            assert_eq!(parsed.diagnostics[0].message, message, "{input}");
            assert!(parsed.commands.is_empty(), "{input}");
        }
    }

    #[test]
    fn assignments_expand_in_order_and_preserve_hidden_state() {
        let mut context = (
            |name: &str| (name == "SEED").then(|| "seeded".to_owned()),
            |_: &str| false,
        );
        let parsed = parse_config_with(
            "test.conf",
            "FIRST=$SEED\n\
             SECOND=$FIRST-${FIRST}\n\
             %hidden SECRET=$SECOND\n\
             THIRD=$SECRET command $THIRD\n\
             show-environment -g THIRD\n",
            &mut context,
        );
        assert!(parsed.diagnostics.is_empty());
        assert_eq!(parsed.environment.len(), 4);
        assert_eq!(parsed.environment[0].name, "FIRST");
        assert_eq!(parsed.environment[0].value, "seeded");
        assert_eq!(parsed.environment[1].value, "seeded-seeded");
        assert!(parsed.environment[2].hidden);
        assert_eq!(parsed.environment[3].value, "seeded-seeded");
        assert_eq!(parsed.commands[0].name, "command");
        assert_eq!(parsed.commands[0].args, ["seeded-seeded"]);
        assert_eq!(parsed.commands[1].name, "show-environment");

        let multiple = parse_config("test.conf", "A=one B=$A command\n");
        assert!(multiple.commands.is_empty());
        assert_eq!(multiple.environment.len(), 1);
        assert_eq!(multiple.environment[0].name, "A");
        assert_eq!(multiple.environment[0].value, "one");
        assert_eq!(multiple.diagnostics.len(), 1);
        assert_eq!(multiple.diagnostics[0].message, "syntax error");
    }

    #[test]
    fn first_diagnostic_aborts_the_whole_file_without_a_cascade() {
        let parsed = parse_config(
            "test.conf",
            "BEFORE=recorded\nset @before kept\nset @bad \"${BROKEN\nset @after wrong\n",
        );
        assert!(parsed.commands.is_empty());
        assert_eq!(parsed.environment.len(), 1);
        assert_eq!(parsed.environment[0].name, "BEFORE");
        assert_eq!(parsed.environment[0].value, "recorded");
        assert_eq!(parsed.diagnostics.len(), 1);
        assert_eq!(
            parsed.diagnostics[0].message,
            "invalid environment variable"
        );
    }

    #[test]
    fn assignments_reduced_before_pin_parse_errors_survive() {
        let invalid_escape = parse_config("test.conf", "KEEP=yes\nset @bad \\400\n");
        assert!(invalid_escape.commands.is_empty());
        assert_eq!(invalid_escape.environment.len(), 1);
        assert_eq!(invalid_escape.environment[0].name, "KEEP");
        assert_eq!(invalid_escape.environment[0].value, "yes");
        assert_eq!(invalid_escape.diagnostics.len(), 1);
        assert_eq!(
            invalid_escape.diagnostics[0].message,
            "invalid octal escape"
        );

        let invalid_second_assignment = parse_config("test.conf", "A=1 B=$A command\n");
        assert!(invalid_second_assignment.commands.is_empty());
        assert_eq!(invalid_second_assignment.environment.len(), 1);
        assert_eq!(invalid_second_assignment.environment[0].name, "A");
        assert_eq!(invalid_second_assignment.environment[0].value, "1");
        assert_eq!(invalid_second_assignment.diagnostics.len(), 1);
        assert_eq!(
            invalid_second_assignment.diagnostics[0].message,
            "syntax error"
        );
    }

    #[test]
    fn quoted_newlines_strip_indentation_and_comments() {
        let parsed = parse_config(
            "test.conf",
            "set @double \"first\n    # stripped\n      second\"\n\
             set @single 'first\n    #:kept\n      second'\n",
        );
        assert!(parsed.diagnostics.is_empty());
        assert_eq!(parsed.commands[0].args[1], "first\n\nsecond");
        assert_eq!(parsed.commands[1].args[1], "first\n#:kept\nsecond");
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
        assert!(!parsed.commands[0].argument_is_command_block(0));
        assert!(parsed.commands[0].argument_is_command_block(1));
    }

    #[test]
    fn command_block_types_follow_argument_positions() {
        let parsed = parse_config(
            "test.conf",
            "SEED=value display-menu {} \"{ quoted }\" key { display-message second }\n",
        );
        assert!(parsed.diagnostics.is_empty());
        assert_eq!(
            parsed.commands[0].args,
            ["{}", "{ quoted }", "key", "{ display-message second }"]
        );
        assert!(parsed.commands[0].argument_is_command_block(0));
        assert!(!parsed.commands[0].argument_is_command_block(1));
        assert!(!parsed.commands[0].argument_is_command_block(2));
        assert!(parsed.commands[0].argument_is_command_block(3));
        assert!(!parsed.commands[0].argument_is_command_block(4));

        let parsed = parse_config("test.conf", "{ display-message top-level }\n");
        assert!(parsed.commands.is_empty());
        assert_eq!(parsed.diagnostics[0].message, "syntax error");
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
        assert!(parsed.commands.iter().all(|command| {
            (0..command.args.len()).all(|index| !command.argument_is_command_block(index))
        }));
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
        assert!(unterminated.commands.is_empty());
        assert_eq!(unterminated.diagnostics.len(), 1);
        assert_eq!(unterminated.diagnostics[0].line, 2);
        assert_eq!(unterminated.diagnostics[0].column, 8);
        assert_eq!(
            unterminated.diagnostics[0].message,
            "unterminated command block"
        );
    }

    #[test]
    fn reports_a_pin_syntax_error_for_a_trailing_escape() {
        let parsed = parse_config("test.conf", "set -g prefix C-a\nbind c new-window\\");
        assert!(parsed.commands.is_empty());
        assert_eq!(parsed.diagnostics.len(), 1);
        assert_eq!(parsed.diagnostics[0].line, 2);
        assert_eq!(parsed.diagnostics[0].column, 18);
        assert_eq!(parsed.diagnostics[0].message, "syntax error");
    }
}
