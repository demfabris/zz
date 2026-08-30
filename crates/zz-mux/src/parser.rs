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

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ParsedConfigBytes {
    pub commands: Vec<ConfigCommandBytes>,
    pub environment: Vec<ConfigEnvironmentAssignmentBytes>,
    pub diagnostics: Vec<ConfigDiagnostic>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfigCommandBytes {
    pub name: Vec<u8>,
    pub args: Vec<Vec<u8>>,
    pub source: Option<SourceSpan>,
    command_blocks: Vec<usize>,
}

impl ConfigCommandBytes {
    pub fn argument_is_command_block(&self, index: usize) -> bool {
        self.command_blocks.contains(&index)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfigEnvironmentAssignment {
    pub name: String,
    pub value: String,
    pub hidden: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfigEnvironmentAssignmentBytes {
    pub name: Vec<u8>,
    pub value: Vec<u8>,
    pub hidden: bool,
}

impl ParsedConfig {
    pub fn parse_file_bytes(source: impl Into<String>, input: &[u8]) -> ParsedConfigBytes {
        let mut context = (|_: &str| None::<String>, |_: &str| false);
        parse_config_file_bytes_with_assignment_overlay(source, input, &mut context, true)
    }

    pub fn parse_buffer_bytes(source: impl Into<String>, input: &[u8]) -> ParsedConfigBytes {
        let mut context = (|_: &str| None::<String>, |_: &str| false);
        parse_config_buffer_bytes_with_assignment_overlay(source, input, &mut context, true)
    }
}

impl ParsedConfigBytes {
    fn from_encoded(parsed: ParsedConfig) -> Self {
        Self {
            commands: parsed
                .commands
                .into_iter()
                .map(|command| {
                    let command_blocks = (0..command.args.len())
                        .filter(|index| command.argument_is_command_block(*index))
                        .collect();
                    ConfigCommandBytes {
                        name: decode_config_bytes(&command.name),
                        args: command
                            .args
                            .iter()
                            .map(|argument| decode_config_bytes(argument))
                            .collect(),
                        source: command.source,
                        command_blocks,
                    }
                })
                .collect(),
            environment: parsed
                .environment
                .into_iter()
                .map(|assignment| ConfigEnvironmentAssignmentBytes {
                    name: decode_config_bytes(&assignment.name),
                    value: decode_config_bytes(&assignment.value),
                    hidden: assignment.hidden,
                })
                .collect(),
            diagnostics: parsed.diagnostics,
        }
    }
}

const CONFIG_BYTE_BASE: u32 = 0xf0000;
const CONFIG_BYTE_EOF: char = '\u{f0101}';
const CONFIG_BYTE_LITERAL: char = '\u{f0100}';

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ConfigByteInput {
    File,
    SignedBuffer,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ConfigInputKind {
    String,
    Bytes,
}

impl ConfigInputKind {
    fn is_word_whitespace(self, character: char) -> bool {
        match self {
            Self::String => character.is_whitespace(),
            Self::Bytes => matches!(character, ' ' | '\t'),
        }
    }

    fn is_word_start_whitespace(self, character: char) -> bool {
        match self {
            Self::String => character.is_whitespace(),
            Self::Bytes => matches!(character, ' ' | '\t' | '\n'),
        }
    }

    fn is_eof(self, character: char) -> bool {
        self == Self::Bytes && character == CONFIG_BYTE_EOF
    }

    fn character_len(self, character: char) -> usize {
        match self {
            Self::String => character.len_utf8(),
            Self::Bytes => 1,
        }
    }

    fn encoded_len(self, value: &str) -> usize {
        match self {
            Self::String => value.len(),
            Self::Bytes => decode_config_bytes(value).len(),
        }
    }
}

struct ConfigCharacters {
    characters: Vec<char>,
    offset: usize,
    escapes: usize,
    skipped_lines: u32,
    input_kind: ConfigInputKind,
}

impl ConfigCharacters {
    fn new(characters: impl Iterator<Item = char>, input_kind: ConfigInputKind) -> Self {
        Self {
            characters: characters.collect(),
            offset: 0,
            escapes: 0,
            skipped_lines: 0,
            input_kind,
        }
    }

    fn getc(&mut self) -> Option<char> {
        if self.input_kind == ConfigInputKind::String {
            let character = self.characters.get(self.offset).copied()?;
            self.offset += 1;
            return Some(character);
        }
        if self.escapes != 0 {
            self.escapes -= 1;
            return Some('\\');
        }
        loop {
            let character = if let Some(character) = self.characters.get(self.offset).copied() {
                self.offset += 1;
                character
            } else {
                CONFIG_BYTE_EOF
            };
            if character == '\\' {
                self.escapes += 1;
                continue;
            }
            if character == '\n' && self.escapes % 2 == 1 {
                self.skipped_lines = self.skipped_lines.saturating_add(1);
                self.escapes -= 1;
                continue;
            }
            if self.escapes != 0 {
                self.ungetc(character);
                self.escapes -= 1;
                return Some('\\');
            }
            return Some(character);
        }
    }

    fn ungetc(&mut self, character: char) {
        if !self.input_kind.is_eof(character) && self.offset != 0 {
            self.offset -= 1;
        }
    }

    fn take_skipped_lines(&mut self) -> u32 {
        std::mem::take(&mut self.skipped_lines)
    }
}

fn encode_config_byte(byte: u8, input: ConfigByteInput) -> char {
    match (input, byte) {
        (_, 0..=0x7f) => char::from(byte),
        (ConfigByteInput::SignedBuffer, 0xff) => CONFIG_BYTE_EOF,
        (_, byte) => encode_stored_config_byte(byte),
    }
}

fn encode_stored_config_byte(byte: u8) -> char {
    match byte {
        0..=0x7f => char::from(byte),
        _ => char::from_u32(CONFIG_BYTE_BASE + u32::from(byte))
            .expect("stored config byte is a valid scalar"),
    }
}

fn stored_config_byte(character: char) -> Option<u8> {
    let value = character as u32;
    (CONFIG_BYTE_BASE + 0x80..=CONFIG_BYTE_BASE + 0xff)
        .contains(&value)
        .then(|| (value - CONFIG_BYTE_BASE) as u8)
}

fn push_config_text_character(value: &mut String, character: char, input_kind: ConfigInputKind) {
    if input_kind == ConfigInputKind::Bytes
        && (character == CONFIG_BYTE_LITERAL || stored_config_byte(character).is_some())
    {
        value.push(CONFIG_BYTE_LITERAL);
    }
    value.push(character);
}

fn push_config_text(value: &mut String, text: &str, input_kind: ConfigInputKind) {
    for character in text.chars() {
        push_config_text_character(value, character, input_kind);
    }
}

fn decode_config_bytes(value: &str) -> Vec<u8> {
    let mut decoded = Vec::with_capacity(value.len());
    let mut characters = value.chars();
    while let Some(character) = characters.next() {
        if character == CONFIG_BYTE_LITERAL {
            let character = characters.next().unwrap_or(CONFIG_BYTE_LITERAL);
            let mut encoded = [0; 4];
            decoded.extend_from_slice(character.encode_utf8(&mut encoded).as_bytes());
        } else if let Some(byte) = stored_config_byte(character) {
            decoded.push(byte);
        } else {
            let mut encoded = [0; 4];
            decoded.extend_from_slice(character.encode_utf8(&mut encoded).as_bytes());
        }
    }
    decoded
}

pub(crate) trait ConfigContext {
    fn variable(&mut self, name: &str) -> Option<String>;
    fn condition(&mut self, condition: &str) -> bool;

    fn user_home(&mut self, name: Option<&str>) -> Option<String> {
        user_home(name)
    }

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
    assignment_overlay: bool,
    input_kind: ConfigInputKind,
    conditionals: Vec<ConditionalScope>,
    context: &'a mut C,
    aborted: bool,
}

struct ConfigExpansion {
    value: String,
    encoded: bool,
}

impl ConfigExpansion {
    fn encoded(value: String) -> Self {
        Self {
            value,
            encoded: true,
        }
    }

    fn text(value: String) -> Self {
        Self {
            value,
            encoded: false,
        }
    }

    fn push_into(self, output: &mut String, input_kind: ConfigInputKind) {
        if self.encoded {
            output.push_str(&self.value);
        } else {
            push_config_text(output, &self.value, input_kind);
        }
    }
}

impl<C: ConfigContext> ConfigBuilder<'_, C> {
    fn active(&self) -> bool {
        self.conditionals.last().is_none_or(|scope| scope.active)
    }

    fn variable(&mut self, name: &str) -> Option<ConfigExpansion> {
        self.overlay
            .get(name)
            .cloned()
            .map(ConfigExpansion::encoded)
            .or_else(|| self.context.variable(name).map(ConfigExpansion::text))
    }

    fn expand_variables(&self) -> bool {
        self.context.expand_variables()
    }

    fn home_directory(&mut self, name: &str) -> Option<ConfigExpansion> {
        if !name.is_empty() {
            return self
                .context
                .user_home(Some(name))
                .map(ConfigExpansion::text);
        }
        if let Some(home) = self.variable("HOME").filter(|home| !home.value.is_empty()) {
            return Some(home);
        }
        self.context.user_home(None).map(ConfigExpansion::text)
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
        if self.input_kind == ConfigInputKind::Bytes
            && !*word_is_command_block
            && let Some(index) = word.find('\0')
        {
            word.truncate(index);
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
        match parse_assignment(&words[0], self.input_kind) {
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
        let (name, value) = match parse_assignment(&tokens[1], self.input_kind) {
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
        match parse_assignment(&command_name, self.input_kind) {
            Ok(Some((name, value))) => {
                if !assignment_recorded {
                    self.push_assignment(name, value, false);
                }
                if command_args.is_empty() {
                    return;
                }
                command_name = command_args.remove(0);
                argument_start = 2;
                if parse_assignment(&command_name, self.input_kind)
                    .ok()
                    .flatten()
                    .is_some()
                {
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
        if self.assignment_overlay {
            self.overlay.insert(name.clone(), value.clone());
        }
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

#[derive(Debug)]
struct Tilde {
    name: String,
    encoded_len: usize,
    line: u32,
    column: u32,
    quote: Quote,
}

#[derive(Clone, Copy, Debug)]
struct Block {
    depth: u32,
    quote: Quote,
    escaped: bool,
    in_comment: bool,
    comment_start: usize,
    saw_byte_eof: bool,
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
            comment_start: 0,
            saw_byte_eof: false,
            word_start: true,
            line,
            column,
        }
    }

    fn feed(&mut self, character: char, input_kind: ConfigInputKind, word_len: usize) -> bool {
        if self.in_comment {
            if character == '\n' {
                self.in_comment = false;
                self.word_start = true;
            } else {
                self.word_start = false;
            }
            return false;
        }
        if self.escaped {
            self.escaped = false;
            if character != '\n' {
                self.word_start = false;
            }
            return false;
        }
        if character == '\\' && self.quote != Quote::Single {
            self.escaped = true;
            return false;
        }
        match self.quote {
            Quote::Single if character == '\'' => {
                self.quote = Quote::None;
                self.word_start = false;
            }
            Quote::Double if character == '"' => {
                self.quote = Quote::None;
                self.word_start = false;
            }
            Quote::Single | Quote::Double => self.word_start = false,
            Quote::None => match character {
                '\'' => {
                    self.quote = Quote::Single;
                    self.word_start = false;
                }
                '"' => {
                    self.quote = Quote::Double;
                    self.word_start = false;
                }
                '#' if self.word_start => {
                    self.in_comment = true;
                    self.comment_start = word_len.saturating_sub(1);
                    self.word_start = false;
                }
                '{' if self.word_start => {
                    self.depth = self.depth.saturating_add(1);
                    self.word_start = true;
                }
                '}' => {
                    self.depth = self.depth.saturating_sub(1);
                    if self.depth == 0 {
                        return true;
                    }
                    self.word_start = true;
                }
                ';' => self.word_start = true,
                value if input_kind.is_word_start_whitespace(value) => self.word_start = true,
                _ => self.word_start = false,
            },
        }
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
    parse_config_with_assignment_overlay(source, input, context, true)
}

pub(crate) fn parse_config_without_assignment_overlay<C: ConfigContext>(
    source: impl Into<String>,
    input: &str,
    context: &mut C,
) -> ParsedConfig {
    parse_config_with_assignment_overlay(source, input, context, false)
}

fn parse_config_with_assignment_overlay<C: ConfigContext>(
    source: impl Into<String>,
    input: &str,
    context: &mut C,
    assignment_overlay: bool,
) -> ParsedConfig {
    parse_config_characters(
        source,
        input.chars(),
        context,
        assignment_overlay,
        ConfigInputKind::String,
    )
}

pub(crate) fn parse_config_file_bytes_with_assignment_overlay<C: ConfigContext>(
    source: impl Into<String>,
    input: &[u8],
    context: &mut C,
    assignment_overlay: bool,
) -> ParsedConfigBytes {
    parse_config_bytes_with_assignment_overlay(
        source,
        input,
        context,
        assignment_overlay,
        ConfigByteInput::File,
    )
}

pub(crate) fn parse_config_buffer_bytes_with_assignment_overlay<C: ConfigContext>(
    source: impl Into<String>,
    input: &[u8],
    context: &mut C,
    assignment_overlay: bool,
) -> ParsedConfigBytes {
    parse_config_bytes_with_assignment_overlay(
        source,
        input,
        context,
        assignment_overlay,
        ConfigByteInput::SignedBuffer,
    )
}

fn parse_config_bytes_with_assignment_overlay<C: ConfigContext>(
    source: impl Into<String>,
    input: &[u8],
    context: &mut C,
    assignment_overlay: bool,
    byte_input: ConfigByteInput,
) -> ParsedConfigBytes {
    ParsedConfigBytes::from_encoded(parse_config_characters(
        source,
        input
            .iter()
            .copied()
            .map(|byte| encode_config_byte(byte, byte_input)),
        context,
        assignment_overlay,
        ConfigInputKind::Bytes,
    ))
}

fn parse_config_characters<C, I>(
    source: impl Into<String>,
    characters: I,
    context: &mut C,
    assignment_overlay: bool,
    input_kind: ConfigInputKind,
) -> ParsedConfig
where
    C: ConfigContext,
    I: Iterator<Item = char>,
{
    let source = source.into();
    let mut builder = ConfigBuilder {
        source,
        parsed: ParsedConfig::default(),
        overlay: BTreeMap::new(),
        assignment_overlay,
        input_kind,
        conditionals: Vec::new(),
        context,
        aborted: false,
    };
    let mut words = Vec::new();
    let mut command_block_words = Vec::new();
    let mut word = String::new();
    let mut word_started = false;
    let mut percent_word = false;
    let mut word_is_command_block = false;
    let mut quote = Quote::None;
    let mut in_comment = false;
    let mut block: Option<Block> = None;
    let mut eager_assignment = false;
    let mut line = 1_u32;
    let mut column = 0_u32;
    let mut command_line = 1_u32;
    let mut command_column = 1_u32;

    let mut characters = ConfigCharacters::new(characters, input_kind);
    let mut reprocess: Option<char> = None;
    let mut tilde: Option<Tilde> = None;
    let mut last_state: Option<Quote> = None;
    let mut byte_eof_seen = false;
    let mut byte_hard_eof = false;
    loop {
        if builder.aborted() {
            break;
        }
        let character = if let Some(character) = reprocess.take() {
            character
        } else {
            let Some(character) = take_character(&mut characters, &mut line, &mut column) else {
                break;
            };
            character
        };
        if let Some(state) = tilde.as_mut() {
            if input_kind.is_eof(character) {
                let state = tilde.take().expect("tilde state exists");
                expand_tilde(
                    &mut builder,
                    &mut word,
                    &state.name,
                    state.line,
                    state.column,
                );
                last_state = Some(state.quote);
            } else if matches!(character, '/' | ' ' | '\t' | '\n' | '"' | '\'') {
                let state = tilde.take().expect("tilde state exists");
                expand_tilde(
                    &mut builder,
                    &mut word,
                    &state.name,
                    state.line,
                    state.column,
                );
                last_state = Some(state.quote);
                reprocess = Some(character);
            } else if state
                .encoded_len
                .saturating_add(input_kind.character_len(character))
                > 1022
            {
                builder.diagnostic(state.line, state.column, "user name is too long");
            } else {
                state.name.push(character);
                state.encoded_len = state
                    .encoded_len
                    .saturating_add(input_kind.character_len(character));
            }
            continue;
        }
        if let Some(state) = block.as_mut() {
            if input_kind.is_eof(character) {
                state.saw_byte_eof = true;
                if state.in_comment {
                    word.truncate(state.comment_start);
                    state.in_comment = false;
                    state.word_start = true;
                    continue;
                }
                if state.escaped {
                    byte_hard_eof = true;
                    break;
                }
                if !state.word_start || state.quote != Quote::None {
                    word.push(' ');
                    state.quote = Quote::None;
                    state.word_start = true;
                } else if byte_eof_seen {
                    byte_hard_eof = true;
                    break;
                } else {
                    word.push('\n');
                    state.word_start = true;
                    byte_eof_seen = true;
                }
                continue;
            }
            word.push(character);
            if character == '\n' {
                line = line.saturating_add(1);
                column = 0;
            }
            if state.feed(character, input_kind, word.len()) {
                block = None;
                finish_word(
                    &mut word,
                    &mut word_started,
                    &mut word_is_command_block,
                    &mut words,
                    &mut command_block_words,
                );
                last_state = None;
            }
            continue;
        }
        if in_comment {
            if input_kind.is_eof(character) {
                in_comment = false;
                continue;
            }
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
                last_state = None;
            }
            continue;
        }
        if input_kind == ConfigInputKind::Bytes
            && quote == Quote::None
            && !(word_started && percent_word)
            && character == '\r'
            && peek_character(&mut characters, &mut line, &mut column) == Some('\n')
        {
            continue;
        }
        if input_kind.is_eof(character) {
            if word_started {
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
                quote = Quote::None;
                last_state = None;
                continue;
            }
            if byte_eof_seen {
                byte_hard_eof = true;
                break;
            }
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
            byte_eof_seen = true;
            command_line = line;
            command_column = column.saturating_add(1);
            last_state = None;
            continue;
        }
        if character == '\\' && quote != Quote::Single {
            let escape_line = line;
            let escape_column = column;
            match parse_escape(&mut characters, &mut line, &mut column, input_kind) {
                Ok(Some(value)) => {
                    if !word_started && words.is_empty() {
                        command_line = escape_line;
                        command_column = escape_column;
                    }
                    if !word_started {
                        percent_word = false;
                    }
                    word_started = true;
                    match value {
                        ConfigEscape::Text(value) => {
                            push_config_text_character(&mut word, value, input_kind);
                        }
                        ConfigEscape::RawByte(value) => word.push(encode_stored_config_byte(value)),
                    }
                    last_state = Some(quote);
                }
                Ok(None) => {}
                Err(message) => {
                    builder.diagnostic(escape_line, escape_column, message);
                }
            }
            continue;
        }
        if input_kind == ConfigInputKind::String
            && character == '\\'
            && quote == Quote::Single
            && peek_character(&mut characters, &mut line, &mut column) == Some('\n')
        {
            take_character(&mut characters, &mut line, &mut column);
            line = line.saturating_add(1);
            column = 0;
            continue;
        }
        if character == '$' && quote != Quote::Single && builder.expand_variables() {
            if !word_started && words.is_empty() {
                command_line = line;
                command_column = column;
            }
            if !word_started {
                percent_word = false;
            }
            word_started = true;
            match expand_variable(
                &mut characters,
                &mut line,
                &mut column,
                &mut builder,
                input_kind,
            ) {
                Ok(value) => value.push_into(&mut word, input_kind),
                Err(message) => {
                    builder.diagnostic(line, column, message);
                }
            }
            last_state = Some(quote);
            continue;
        }
        if character == '~' && quote != Quote::Single && last_state != Some(quote) {
            if !word_started && words.is_empty() {
                command_line = line;
                command_column = column;
            }
            if !word_started {
                percent_word = false;
            }
            word_started = true;
            tilde = Some(Tilde {
                name: String::new(),
                encoded_len: 0,
                line,
                column,
                quote,
            });
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
                    strip_quoted_line_prefix(
                        &mut characters,
                        &mut reprocess,
                        &mut word,
                        &mut line,
                        &mut column,
                    );
                } else {
                    last_state = Some(quote);
                }
            }
            Quote::None => match character {
                '\'' => {
                    if !word_started && words.is_empty() {
                        command_line = line;
                        command_column = column;
                    }
                    if !word_started {
                        percent_word = false;
                    }
                    word_started = true;
                    quote = Quote::Single;
                }
                '"' => {
                    if !word_started && words.is_empty() {
                        command_line = line;
                        command_column = column;
                    }
                    if !word_started {
                        percent_word = false;
                    }
                    word_started = true;
                    quote = Quote::Double;
                }
                '#' if !word_started
                    && words
                        .last()
                        .is_some_and(|word| matches!(word.as_str(), "%if" | "%elif"))
                    && peek_character(&mut characters, &mut line, &mut column) == Some('{') =>
                {
                    let format_line = line;
                    let format_column = column;
                    percent_word = false;
                    word_started = true;
                    if let Err(message) = scan_condition_format(
                        &mut characters,
                        &mut word,
                        &mut line,
                        &mut column,
                        input_kind,
                    ) {
                        builder.diagnostic(format_line, format_column, message);
                    }
                }
                '#' if !word_started
                    && words
                        .last()
                        .is_some_and(|word| matches!(word.as_str(), "%else" | "%endif"))
                    && peek_character(&mut characters, &mut line, &mut column) == Some('{') =>
                {
                    builder.diagnostic(line, column, "syntax error");
                }
                '#' if !word_started => in_comment = true,
                '{' if !word_started => {
                    if words.is_empty() {
                        command_line = line;
                        command_column = column;
                    }
                    percent_word = false;
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
                    last_state = None;
                }
                value if input_kind.is_word_whitespace(value) => {
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
                    last_state = None;
                }
                value => {
                    if words.is_empty() && !word_started {
                        command_line = line;
                        command_column = column;
                    }
                    if !word_started {
                        percent_word = value == '%';
                    }
                    word_started = true;
                    word.push(value);
                    last_state = Some(quote);
                }
            },
        }
    }
    if !builder.aborted() {
        if byte_hard_eof {
            if let Some(state) = block {
                if state.saw_byte_eof {
                    builder.diagnostic(line, column, "syntax error");
                } else {
                    builder.diagnostic(state.line, state.column, "syntax error");
                }
            } else if word_started || !words.is_empty() || tilde.is_some() {
                builder.diagnostic(line, column, "syntax error");
            }
        } else {
            if let Some(state) = tilde.take() {
                expand_tilde(
                    &mut builder,
                    &mut word,
                    &state.name,
                    state.line,
                    state.column,
                );
            }
            if !builder.aborted() {
                if let Some(state) = block {
                    if input_kind == ConfigInputKind::Bytes && state.saw_byte_eof {
                        builder.diagnostic(line, column, "syntax error");
                    } else {
                        builder.diagnostic(state.line, state.column, "unterminated command block");
                    }
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
        }
    }
    builder.finish(line, column)
}

fn expand_tilde<C: ConfigContext>(
    builder: &mut ConfigBuilder<'_, C>,
    word: &mut String,
    name: &str,
    line: u32,
    column: u32,
) {
    let Some(home) = builder.home_directory(name) else {
        builder.diagnostic(line, column, "syntax error");
        return;
    };
    home.push_into(word, builder.input_kind);
}

#[cfg(unix)]
fn user_home(name: Option<&str>) -> Option<String> {
    use nix::unistd::{Uid, User};

    let user = match name {
        Some(name) => User::from_name(name),
        None => User::from_uid(Uid::current()),
    }
    .ok()
    .flatten()?;
    user.dir.into_os_string().into_string().ok()
}

#[cfg(not(unix))]
fn user_home(name: Option<&str>) -> Option<String> {
    if name.is_some() {
        return None;
    }
    std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .ok()
        .filter(|home| !home.is_empty())
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

fn scan_condition_format(
    characters: &mut ConfigCharacters,
    word: &mut String,
    line: &mut u32,
    column: &mut u32,
    input_kind: ConfigInputKind,
) -> Result<(), String> {
    word.push('#');
    let Some(open) = take_character(characters, line, column) else {
        return Err("syntax error".to_owned());
    };
    if input_kind.is_eof(open) {
        return Err("syntax error".to_owned());
    }
    word.push(open);
    let mut depth = 1_u32;
    loop {
        let Some(character) = take_character(characters, line, column) else {
            return Err("syntax error".to_owned());
        };
        if input_kind.is_eof(character) {
            return Err("syntax error".to_owned());
        }
        if character == '\n' {
            *line = line.saturating_add(1);
            *column = 0;
            return Err("syntax error".to_owned());
        }
        word.push(character);
        if character == '#' {
            let Some(next) = take_character(characters, line, column) else {
                return Err("syntax error".to_owned());
            };
            if input_kind.is_eof(next) {
                return Err("syntax error".to_owned());
            }
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

fn parse_assignment(
    token: &str,
    input_kind: ConfigInputKind,
) -> Result<Option<(String, String)>, ()> {
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
    if input_kind.encoded_len(token) > 16_384 {
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

enum ConfigEscape {
    Text(char),
    RawByte(u8),
}

fn parse_escape(
    characters: &mut ConfigCharacters,
    line: &mut u32,
    column: &mut u32,
    input_kind: ConfigInputKind,
) -> Result<Option<ConfigEscape>, String> {
    let Some(character) = take_character(characters, line, column) else {
        return Err("syntax error".to_owned());
    };
    if input_kind.is_eof(character) {
        return Err("syntax error".to_owned());
    }
    if character == '\n' {
        *line = line.saturating_add(1);
        *column = 0;
        return Ok(None);
    }
    if matches!(character, '4'..='7') {
        return Err("invalid octal escape".to_owned());
    }
    if matches!(character, '0'..='3') {
        let second = take_character(characters, line, column);
        let third = second
            .filter(|value| matches!(value, '0'..='7'))
            .and_then(|_| take_character(characters, line, column));
        let (Some(second), Some(third)) = (second, third) else {
            return Err("invalid octal escape".to_owned());
        };
        if !matches!(third, '0'..='7') {
            return Err("invalid octal escape".to_owned());
        }
        let value = 64 * (character as u32 - '0' as u32)
            + 8 * (second as u32 - '0' as u32)
            + (third as u32 - '0' as u32);
        let value = u8::try_from(value).expect("octal config escape fits in one byte");
        return Ok(Some(match input_kind {
            ConfigInputKind::String => ConfigEscape::Text(char::from(value)),
            ConfigInputKind::Bytes => ConfigEscape::RawByte(value),
        }));
    }
    if input_kind == ConfigInputKind::Bytes
        && let Some(value) = stored_config_byte(character)
    {
        return Ok(Some(ConfigEscape::RawByte(value)));
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
                let Some(digit) = take_character(characters, line, column) else {
                    return Err("syntax error".to_owned());
                };
                if input_kind.is_eof(digit) {
                    return Err("syntax error".to_owned());
                }
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
            return Ok(Some(ConfigEscape::Text(value)));
        }
        other => other,
    };
    Ok(Some(ConfigEscape::Text(value)))
}

fn expand_variable<C>(
    characters: &mut ConfigCharacters,
    line: &mut u32,
    column: &mut u32,
    builder: &mut ConfigBuilder<'_, C>,
    input_kind: ConfigInputKind,
) -> Result<ConfigExpansion, String>
where
    C: ConfigContext,
{
    let Some(next) = peek_character(characters, line, column) else {
        return Err("syntax error".to_owned());
    };
    if input_kind.is_eof(next) {
        return Err("syntax error".to_owned());
    }
    let braced = next == '{';
    if braced {
        take_character(characters, line, column);
    } else if !is_variable_character(next, true) {
        return Ok(ConfigExpansion::text("$".to_owned()));
    }

    let mut name = String::new();
    loop {
        let Some(next) = peek_character(characters, line, column) else {
            if braced {
                return Err("invalid environment variable".to_owned());
            }
            break;
        };
        if input_kind.is_eof(next) {
            if braced {
                return Err("invalid environment variable".to_owned());
            }
            break;
        }
        if braced && next == '}' {
            take_character(characters, line, column);
            break;
        }
        if !is_variable_character(next, false) {
            if braced {
                take_character(characters, line, column);
                return Err("invalid environment variable".to_owned());
            }
            break;
        }
        if name.len() == 1022 {
            return Err("environment variable is too long".to_owned());
        }
        name.push(next);
        take_character(characters, line, column);
    }
    Ok(builder
        .variable(&name)
        .unwrap_or_else(|| ConfigExpansion::text(String::new())))
}

fn take_character(
    characters: &mut ConfigCharacters,
    line: &mut u32,
    column: &mut u32,
) -> Option<char> {
    let character = characters.getc()?;
    let skipped_lines = characters.take_skipped_lines();
    if skipped_lines != 0 {
        *line = line.saturating_add(skipped_lines);
        *column = 0;
    }
    *column = column.saturating_add(1);
    Some(character)
}

fn peek_character(
    characters: &mut ConfigCharacters,
    line: &mut u32,
    column: &mut u32,
) -> Option<char> {
    let character = characters.getc()?;
    let skipped_lines = characters.take_skipped_lines();
    if skipped_lines != 0 {
        *line = line.saturating_add(skipped_lines);
        *column = 0;
    }
    characters.ungetc(character);
    Some(character)
}

fn strip_quoted_line_prefix(
    characters: &mut ConfigCharacters,
    reprocess: &mut Option<char>,
    word: &mut String,
    line: &mut u32,
    column: &mut u32,
) {
    let Some(mut character) = take_character(characters, line, column) else {
        return;
    };
    while matches!(character, ' ' | '\t') {
        let Some(next) = take_character(characters, line, column) else {
            return;
        };
        character = next;
    }
    if character != '#' {
        *reprocess = Some(character);
        return;
    }
    let Some(next) = take_character(characters, line, column) else {
        return;
    };
    if characters.input_kind.is_eof(next) {
        return;
    }
    if matches!(next, ',' | '#' | '{' | '}' | ':') {
        word.push('#');
        *reprocess = Some(next);
        return;
    }
    if next == '\n' {
        *reprocess = Some(next);
        return;
    }
    while let Some(next) = take_character(characters, line, column) {
        if characters.input_kind.is_eof(next) {
            return;
        }
        if next == '\n' {
            *reprocess = Some(next);
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct HomeContext {
        home: Option<String>,
        current_user_home: Option<String>,
        named_user_homes: BTreeMap<String, String>,
    }

    impl ConfigContext for HomeContext {
        fn variable(&mut self, name: &str) -> Option<String> {
            if name == "HOME" {
                self.home.clone()
            } else {
                None
            }
        }

        fn condition(&mut self, _condition: &str) -> bool {
            false
        }

        fn user_home(&mut self, name: Option<&str>) -> Option<String> {
            match name {
                Some(name) => self.named_user_homes.get(name).cloned(),
                None => self.current_user_home.clone(),
            }
        }
    }

    #[test]
    fn expands_leading_tildes_like_the_pin() {
        let mut context = HomeContext {
            home: Some("/server/home".to_owned()),
            current_user_home: Some("/passwd/home".to_owned()),
            named_user_homes: BTreeMap::from([("alice".to_owned(), "/users/alice".to_owned())]),
        };
        let parsed = parse_config_with(
            "<test>",
            r#"run-shell ~/bin/x "~/quoted" '~/literal' \~/escaped prefix~literal ~ 'single'~/after-single "double"~/after-double ''~/empty-single ""~/empty-double prefix''~/not-expanded prefix""~/not-expanded ~alice/bin"#,
            &mut context,
        );
        assert!(parsed.diagnostics.is_empty());
        let command = &parsed.commands[0];
        assert_eq!(command.args[0], "/server/home/bin/x");
        assert_eq!(command.args[1], "/server/home/quoted");
        assert_eq!(command.args[2], "~/literal");
        assert_eq!(command.args[3], "~/escaped");
        assert_eq!(command.args[4], "prefix~literal");
        assert_eq!(command.args[5], "/server/home");
        assert_eq!(command.args[6], "single/server/home/after-single");
        assert_eq!(command.args[7], "double/server/home/after-double");
        assert_eq!(command.args[8], "/server/home/empty-single");
        assert_eq!(command.args[9], "/server/home/empty-double");
        assert_eq!(command.args[10], "prefix~/not-expanded");
        assert_eq!(command.args[11], "prefix~/not-expanded");
        assert_eq!(command.args[12], "/users/alice/bin");
    }

    #[test]
    fn tracks_tilde_state_across_invisible_parser_transitions() {
        let mut context = HomeContext {
            home: Some("/server/home".to_owned()),
            current_user_home: Some("/passwd/home".to_owned()),
            ..HomeContext::default()
        };
        let parsed = parse_config_with(
            "<test>",
            concat!(
                "display-message -p \\\n",
                "~/unquoted\n",
                "display-message -p \"\\\n",
                "~/opening\"\n",
                "display-message -p \"\"\\\n",
                "~/empty-closing\n",
                "display-message -p \"\n",
                "~/raw\"\n",
                "display-message -p \"\n",
                "  # stripped\n",
                "  ~/comment\"\n",
                "if-shell true {}~\n",
                "display-message -p prefix\\\n",
                "~/literal\n",
                "display-message -p $EMPTY~/literal \"$EMPTY~/quoted\"\n",
            ),
            &mut context,
        );

        assert!(parsed.diagnostics.is_empty());
        assert_eq!(parsed.commands.len(), 8);
        assert_eq!(parsed.commands[0].args, ["-p", "/server/home/unquoted"]);
        assert_eq!(parsed.commands[1].args, ["-p", "/server/home/opening"]);
        assert_eq!(
            parsed.commands[2].args,
            ["-p", "/server/home/empty-closing"]
        );
        assert_eq!(parsed.commands[3].args, ["-p", "\n/server/home/raw"]);
        assert_eq!(parsed.commands[4].args, ["-p", "\n\n/server/home/comment"]);
        assert_eq!(parsed.commands[5].args, ["true", "{}", "/server/home"]);
        assert!(parsed.commands[5].argument_is_command_block(1));
        assert_eq!(parsed.commands[6].args, ["-p", "prefix~/literal"]);
        assert_eq!(parsed.commands[7].args, ["-p", "~/literal", "~/quoted"]);
    }

    #[test]
    fn limits_tilde_usernames_to_1022_bytes() {
        let accepted_name = "x".repeat(1022);
        let mut context = HomeContext {
            home: Some("/server/home".to_owned()),
            current_user_home: Some("/passwd/home".to_owned()),
            named_user_homes: BTreeMap::from([(accepted_name.clone(), "/users/long".to_owned())]),
        };
        let accepted = parse_config_with(
            "<test>",
            &format!("display-message -p ~{accepted_name}/ok"),
            &mut context,
        );
        assert!(accepted.diagnostics.is_empty());
        assert_eq!(accepted.commands[0].args, ["-p", "/users/long/ok"]);

        let rejected_name = "x".repeat(1023);
        let rejected = parse_config_with(
            "<test>",
            &format!("display-message -p ~{rejected_name}/bad"),
            &mut context,
        );
        assert!(rejected.commands.is_empty());
        assert_eq!(rejected.diagnostics.len(), 1);
        assert_eq!(rejected.diagnostics[0].message, "user name is too long");
    }

    #[test]
    fn resolves_bare_tildes_from_server_home_then_passwd() {
        let mut server = HomeContext {
            home: Some("/server/home".to_owned()),
            current_user_home: Some("/passwd/home".to_owned()),
            ..HomeContext::default()
        };
        let parsed = parse_config_with("<test>", "display-message -p ~", &mut server);
        assert_eq!(parsed.commands[0].args, ["-p", "/server/home"]);

        for home in [Some(String::new()), None] {
            let mut fallback = HomeContext {
                home,
                current_user_home: Some("/passwd/home".to_owned()),
                ..HomeContext::default()
            };
            let parsed = parse_config_with("<test>", "display-message -p ~", &mut fallback);
            assert!(parsed.diagnostics.is_empty());
            assert_eq!(parsed.commands[0].args, ["-p", "/passwd/home"]);
        }
    }

    #[test]
    fn parse_only_tilde_expansion_uses_the_pre_file_environment() {
        let mut normal = HomeContext {
            home: Some("/server/home".to_owned()),
            current_user_home: Some("/passwd/home".to_owned()),
            ..HomeContext::default()
        };
        let parsed = parse_config_with(
            "<test>",
            "HOME=/file/home\ndisplay-message -p ~/normal\n",
            &mut normal,
        );
        assert!(parsed.diagnostics.is_empty());
        assert_eq!(parsed.commands[0].args, ["-p", "/file/home/normal"]);

        let mut parse_only = HomeContext {
            home: Some("/server/home".to_owned()),
            current_user_home: Some("/passwd/home".to_owned()),
            ..HomeContext::default()
        };
        let parsed = parse_config_without_assignment_overlay(
            "<test>",
            "HOME=/file/home\ndisplay-message -p ~/parse-only\n",
            &mut parse_only,
        );
        assert!(parsed.diagnostics.is_empty());
        assert_eq!(parsed.commands[0].args, ["-p", "/server/home/parse-only"]);
        assert_eq!(parsed.environment[0].name, "HOME");
        assert_eq!(parsed.environment[0].value, "/file/home");
    }

    #[test]
    fn missing_required_tilde_lookups_abort_with_a_location() {
        let mut context = HomeContext {
            home: Some("/server/home".to_owned()),
            current_user_home: Some("/passwd/home".to_owned()),
            ..HomeContext::default()
        };
        let literal = parse_config_with(
            "<test>",
            "display-message -p prefix~missing/path",
            &mut context,
        );
        assert!(literal.diagnostics.is_empty());
        assert_eq!(literal.commands[0].args, ["-p", "prefix~missing/path"]);

        let failed = parse_config_with(
            "tilde.conf",
            "display-message -p before\ndisplay-message -p ~missing/path\n",
            &mut context,
        );
        assert!(failed.commands.is_empty());
        assert_eq!(failed.diagnostics.len(), 1);
        assert_eq!(failed.diagnostics[0].source, "tilde.conf");
        assert_eq!(failed.diagnostics[0].line, 2);
        assert_eq!(failed.diagnostics[0].column, 20);
        assert_eq!(failed.diagnostics[0].message, "syntax error");
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
    fn assignment_overlay_can_be_disabled_without_dropping_records() {
        let mut context = (
            |name: &str| (name == "VALUE").then(|| "before".to_owned()),
            |_: &str| false,
        );
        let parsed = parse_config_without_assignment_overlay(
            "test.conf",
            "VALUE=after\n%hidden SECRET=$VALUE\ndisplay-message -p $VALUE-$SECRET\n",
            &mut context,
        );

        assert!(parsed.diagnostics.is_empty());
        assert_eq!(parsed.environment.len(), 2);
        assert_eq!(parsed.environment[0].name, "VALUE");
        assert_eq!(parsed.environment[0].value, "after");
        assert_eq!(parsed.environment[1].name, "SECRET");
        assert_eq!(parsed.environment[1].value, "before");
        assert!(parsed.environment[1].hidden);
        assert_eq!(parsed.commands[0].args, ["-p", "before-"]);
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
