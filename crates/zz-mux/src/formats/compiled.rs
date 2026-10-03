use std::{collections::HashMap, ops::Range};

use super::*;

const CACHE_ENTRIES: usize = 512;
const CACHE_BYTES: usize = 1024 * 1024;
const CACHE_CONTAINER_BYTES: usize = 64 * 1024;

static CACHE: LazyLock<Mutex<Cache>> = LazyLock::new(Mutex::default);

thread_local! {
    #[cfg(test)]
    static ENABLED: Cell<Option<bool>> = const { Cell::new(None) };
}

pub(super) fn enabled() -> bool {
    #[cfg(test)]
    if let Some(enabled) = ENABLED.get() {
        return enabled;
    }
    true
}

#[cfg(test)]
pub(super) fn with_enabled<R>(enabled: bool, body: impl FnOnce() -> R) -> R {
    struct Reset(Option<bool>);
    impl Drop for Reset {
        fn drop(&mut self) {
            ENABLED.set(self.0);
        }
    }
    let _reset = Reset(ENABLED.replace(Some(enabled)));
    body()
}

#[derive(Default)]
struct Cache {
    entries: HashMap<Box<str>, Arc<Template>, foldhash::fast::FixedState>,
    order: VecDeque<Box<str>>,
    bytes: usize,
}

pub(super) fn get(source: &str) -> Arc<Template> {
    if let Some(template) = CACHE.lock().entries.get(source).cloned() {
        return template;
    }
    let template = Arc::new(Template::parse(source, false));
    CACHE.lock().insert(source, template)
}

impl Cache {
    fn insert(&mut self, source: &str, template: Arc<Template>) -> Arc<Template> {
        if let Some(cached) = self.entries.get(source) {
            return Arc::clone(cached);
        }
        let bytes = template
            .bytes
            .saturating_add(source.len().saturating_mul(2));
        if bytes > CACHE_BYTES - CACHE_CONTAINER_BYTES {
            return template;
        }
        while self.entries.len() >= CACHE_ENTRIES
            || self.bytes.saturating_add(bytes) > CACHE_BYTES - CACHE_CONTAINER_BYTES
        {
            let Some(oldest) = self.order.pop_front() else {
                break;
            };
            if let Some(removed) = self.entries.remove(oldest.as_ref()) {
                self.bytes = self
                    .bytes
                    .saturating_sub(removed.bytes.saturating_add(oldest.len().saturating_mul(2)));
            }
        }
        self.bytes = self.bytes.saturating_add(bytes);
        self.order.push_back(source.into());
        self.entries.insert(source.into(), Arc::clone(&template));
        template
    }
}

pub(super) struct Template {
    pub(super) operations: Vec<Operation>,
    pub(super) parts: Arc<[Range<usize>]>,
    pub(super) references: Arc<[String]>,
    bytes: usize,
    pub(super) clock_dependent: bool,
    unconditional_clock: bool,
    pub(super) format_parts: Arc<[FormatPart]>,
}

pub(super) enum Operation {
    Text(Box<str>),
    Variable(&'static str),
    Shell(Box<str>),
    Replacement(Box<Replacement>),
    Style(Box<str>, Template),
}

pub(super) struct Replacement {
    pub(super) body: Box<str>,
    pub(super) copy: Box<str>,
    pub(super) modifiers: Vec<Modifier>,
    pub(super) dynamic: bool,
    pub(super) dynamic_padding: Option<Vec<(usize, bool)>>,
    pub(super) direct: bool,
    pub(super) padding_direct: bool,
    pub(super) flags: Option<Flags>,
    pub(super) discarded: Vec<String>,
}

impl Replacement {
    fn allocation_bytes(&self) -> usize {
        let mut bytes = self
            .body
            .len()
            .saturating_add(self.copy.len())
            .saturating_add(
                self.modifiers
                    .capacity()
                    .saturating_mul(std::mem::size_of::<Modifier>()),
            )
            .saturating_add(
                self.discarded
                    .capacity()
                    .saturating_mul(std::mem::size_of::<String>()),
            )
            .saturating_add(self.dynamic_padding.as_ref().map_or(0, |padding| {
                padding
                    .capacity()
                    .saturating_mul(std::mem::size_of::<(usize, bool)>())
            }))
            .saturating_add(self.flags.as_ref().map_or(0, Flags::allocation_bytes));
        for modifier in &self.modifiers {
            bytes = bytes.saturating_add(
                modifier
                    .args
                    .capacity()
                    .saturating_mul(std::mem::size_of::<String>()),
            );
            for argument in &modifier.args {
                bytes = bytes.saturating_add(argument.capacity());
            }
        }
        for argument in &self.discarded {
            bytes = bytes.saturating_add(argument.capacity());
        }
        bytes
    }

    fn parse(
        body: &str,
        references: &mut BTreeSet<String>,
        clock_dependent: &mut bool,
        unconditional_clock: &mut bool,
        (split_safe, required_loops): (&mut bool, &mut FormatNeeds),
        depth: usize,
    ) -> Self {
        let mut arguments = Vec::new();
        let parsed = parse_modifiers(body, |argument| {
            arguments.push(argument.to_owned());
            argument.to_owned()
        });
        let parsed_modifiers = parsed.is_some();
        let (modifiers, copy, discarded) = match parsed {
            Some((modifiers, offset)) => (modifiers, &body[offset..], Vec::new()),
            None => (Vec::new(), body, arguments),
        };
        let flags = ModifierFlags::from_modifiers(&modifiers);
        *clock_dependent |= flags.time.enabled || flags.expand_time;
        *unconditional_clock |= flags.time.enabled;
        *split_safe &= !modifiers
            .iter()
            .any(|modifier| modifier.kind == ModifierKind::NameExists)
            && !flags.repeat
            && (flags.comparison.is_none() || split_once_top(copy, ',').1.is_some())
            && (parsed_modifiers
                || copy.starts_with('?')
                || !body.contains(':') && discarded.is_empty());
        if flags.loop_windows {
            *required_loops |= FormatNeeds::WINDOWS;
        }
        if flags.loop_panes {
            *required_loops |= FormatNeeds::PANES;
        }
        for argument in modifiers
            .iter()
            .flat_map(|modifier| &modifier.args)
            .chain(&discarded)
        {
            let template = Template::parse_depth(argument, false, depth + 1);
            *clock_dependent |= template.clock_dependent;
            *unconditional_clock |= template.unconditional_clock;
            references.extend(template.references.iter().cloned());
        }
        let dynamic_fact_flags = |kind| {
            modifiers.iter().any(|modifier| {
                modifier.kind == kind
                    && modifier
                        .args
                        .iter()
                        .any(|argument| argument.contains(['#', '%']))
            })
        };
        if flags.interrogate.is_some()
            || dynamic_fact_flags(ModifierKind::Interrogate)
            || !flags.literal
                && (flags.loop_clients
                    || flags.content_search.is_some()
                    || flags.loop_environment == Some("c")
                    || dynamic_fact_flags(ModifierKind::Environments))
        {
            references.insert("*".to_owned());
        }
        if flags.expand || flags.expand_time {
            if !copy.contains("#{") && !flags.literal {
                let modifier = if flags.expand { "E" } else { "T" };
                references.insert(format!("{modifier}:{copy}"));
            } else {
                references.insert("*".to_owned());
            }
        }
        if let Some(conditional) = copy.strip_prefix('?') {
            let parts = split_top(conditional, ',');
            for (index, part) in parts.iter().enumerate() {
                if index % 2 == 0 && index + 1 < parts.len() && !part.contains("#{") {
                    references.insert((*part).to_owned());
                }
                let template = Template::parse_depth(part, false, depth + 1);
                *clock_dependent |= template.clock_dependent;
                *unconditional_clock |= template.unconditional_clock;
                references.extend(template.references.iter().cloned());
            }
        } else if !flags.literal {
            if copy.contains("#{")
                || flags.character
                || flags.colour.is_some()
                || flags.loop_sessions
                || flags.loop_windows
                || flags.loop_panes
                || flags.loop_clients
                || flags.loop_options.is_some()
                || flags.loop_environment.is_some()
                || flags.name_window
                || flags.name_session
                || flags.content_search.is_some()
                || flags.repeat
                || flags.not
                || flags.not_not
                || flags.bool_op.is_some()
                || flags.comparison.is_some()
                || flags.expression.is_some()
            {
                let template = Template::parse_depth(copy, false, depth + 1);
                *clock_dependent |= template.clock_dependent;
                *unconditional_clock |= template.unconditional_clock;
                references.extend(template.references.iter().cloned());
            } else {
                references.insert(copy.to_owned());
            }
        }
        let dynamic = modifiers
            .iter()
            .flat_map(|modifier| &modifier.args)
            .any(|argument| argument.contains(['#', '%']));
        let infallible_dynamic_arguments = modifiers.iter().all(|modifier| {
            matches!(
                modifier.kind,
                ModifierKind::Expand
                    | ModifierKind::ExpandTime
                    | ModifierKind::Limit
                    | ModifierKind::Padding
            )
        });
        *clock_dependent |= dynamic;
        *unconditional_clock |=
            dynamic && !infallible_dynamic_arguments || references.contains("*");
        *split_safe &= !dynamic || infallible_dynamic_arguments;
        let last_padding = modifiers
            .iter()
            .rposition(|modifier| modifier.kind == ModifierKind::Padding);
        let mut padding = Vec::new();
        let padding_only = modifiers.iter().enumerate().all(|(index, modifier)| {
            modifier.args.iter().enumerate().all(|(argument, value)| {
                if !value.contains(['#', '%']) {
                    return true;
                }
                if modifier.kind != ModifierKind::Padding
                    || argument != 0
                    || modifier.args.len() != 1
                {
                    return false;
                }
                padding.push((index, Some(index) == last_padding));
                true
            })
        });
        let dynamic_padding = (dynamic && padding_only).then_some(padding);
        let direct = modifiers.is_empty()
            && discarded.is_empty()
            && !copy.starts_with('?')
            && !copy.contains("#{");
        let padding_direct = modifiers
            .iter()
            .all(|modifier| modifier.kind == ModifierKind::Padding)
            && discarded.is_empty()
            && !copy.starts_with('?')
            && !copy.contains("#{");
        let flags =
            ((!dynamic || dynamic_padding.is_some()) && !direct).then(|| Flags::new(&modifiers));
        Self {
            body: body.into(),
            copy: copy.into(),
            modifiers,
            dynamic,
            dynamic_padding,
            direct,
            padding_direct,
            flags,
            discarded,
        }
    }
}

impl Template {
    fn parse(source: &str, style: bool) -> Self {
        Self::parse_depth(source, style, 0)
    }

    fn parse_depth(source: &str, style: bool, depth: usize) -> Self {
        if depth == FORMAT_LOOP_LIMIT {
            let mut template = Self {
                operations: vec![Operation::Text(source.into())],
                parts: std::iter::once(0..source.len()).collect(),
                references: Arc::from(["*".to_owned()]),
                bytes: 0,
                clock_dependent: true,
                unconditional_clock: true,
                format_parts: Arc::from([FormatPart {
                    range: 0..source.len(),
                    unconditional_clock: true,
                    references: Arc::from(["*".to_owned()]),
                    required_loops: FormatNeeds::default(),
                }]),
            };
            template.bytes = template
                .allocation_bytes()
                .saturating_add(std::mem::size_of::<Self>())
                .saturating_add(std::mem::size_of::<usize>().saturating_mul(2));
            return template;
        }
        let mut operations = Vec::new();
        let mut references = BTreeSet::new();
        let mut clock_dependent = source.contains('%');
        let mut unconditional_clock = clock_dependent;
        let mut format_parts = Vec::new();
        let mut part_start = 0;
        let mut previous_raw = false;
        let mut split_safe = true;
        let mut literal = String::new();
        let mut index = 0;
        while index < source.len() {
            let character = source[index..].chars().next().unwrap();
            if character != '#' {
                literal.push(character);
                index += character.len_utf8();
                continue;
            }
            let hashes_end = source.as_bytes()[index..]
                .iter()
                .position(|byte| *byte != b'#')
                .map_or(source.len(), |offset| index + offset);
            if !style && source.as_bytes().get(hashes_end) == Some(&b'[') {
                let Some(end) = find_style_end(source, hashes_end + 1) else {
                    split_safe = false;
                    break;
                };
                Self::flush(&mut operations, &mut literal);
                let body = Self::parse_depth(&source[hashes_end + 1..end], true, depth + 1);
                clock_dependent |= body.clock_dependent;
                unconditional_clock |= body.unconditional_clock;
                Self::push_part(
                    &mut format_parts,
                    &mut part_start,
                    &mut previous_raw,
                    index..end + 1,
                    body.unconditional_clock,
                    (body.references.clone(), FormatNeeds::default()),
                    false,
                );
                references.extend(body.references.iter().cloned());
                operations.push(Operation::Style(source[index..=hashes_end].into(), body));
                index = end + 1;
                continue;
            }
            let next_index = index + 1;
            let Some(next) = source[next_index..].chars().next() else {
                split_safe = false;
                break;
            };
            let after_next = next_index + next.len_utf8();
            match next {
                '#' | '}' | ',' => literal.push(next),
                '(' => {
                    let Some(end) = find_plain_group_end(source, after_next, '(', ')') else {
                        split_safe = false;
                        break;
                    };
                    Self::flush(&mut operations, &mut literal);
                    let command = &source[after_next..end];
                    references.insert("*".to_owned());
                    let template = Self::parse_depth(command, false, depth + 1);
                    clock_dependent |= template.clock_dependent;
                    unconditional_clock = true;
                    references.extend(template.references.iter().cloned());
                    Self::push_part(
                        &mut format_parts,
                        &mut part_start,
                        &mut previous_raw,
                        index..end + 1,
                        true,
                        (
                            std::iter::once("*".to_owned())
                                .chain(template.references.iter().cloned())
                                .collect::<BTreeSet<_>>()
                                .into_iter()
                                .collect(),
                            FormatNeeds::default(),
                        ),
                        true,
                    );
                    operations.push(Operation::Shell(command.into()));
                    index = end + 1;
                    continue;
                }
                '{' => {
                    let Some(end) = find_format_end(source, index) else {
                        split_safe = false;
                        break;
                    };
                    Self::flush(&mut operations, &mut literal);
                    let mut part_references = BTreeSet::new();
                    let mut part_clock = false;
                    let mut part_unconditional_clock = false;
                    let mut required_loops = FormatNeeds::default();
                    let replacement = Replacement::parse(
                        &source[after_next..end],
                        &mut part_references,
                        &mut part_clock,
                        &mut part_unconditional_clock,
                        (&mut split_safe, &mut required_loops),
                        depth,
                    );
                    references.extend(part_references.iter().cloned());
                    clock_dependent |= part_clock;
                    unconditional_clock |= part_unconditional_clock;
                    Self::push_part(
                        &mut format_parts,
                        &mut part_start,
                        &mut previous_raw,
                        index..end + 1,
                        part_unconditional_clock,
                        (part_references.into_iter().collect(), required_loops),
                        true,
                    );
                    if !replacement.modifiers.is_empty()
                        && replacement
                            .modifiers
                            .iter()
                            .all(|modifier| modifier.kind == ModifierKind::Literal)
                    {
                        operations.push(Operation::Text(unescape(&replacement.copy).into()));
                    } else {
                        operations.push(Operation::Replacement(Box::new(replacement)));
                    }
                    index = end + 1;
                    continue;
                }
                _ => {
                    if !style && let Some(name) = shorthand(next) {
                        Self::flush(&mut operations, &mut literal);
                        references.insert(name.to_owned());
                        operations.push(Operation::Variable(name));
                        Self::push_part(
                            &mut format_parts,
                            &mut part_start,
                            &mut previous_raw,
                            index..after_next,
                            false,
                            (Arc::from([name.to_owned()]), FormatNeeds::default()),
                            true,
                        );
                    } else {
                        literal.push('#');
                        literal.push(next);
                    }
                }
            }
            index = after_next;
        }
        Self::flush(&mut operations, &mut literal);
        if part_start < source.len() {
            format_parts.push(FormatPart {
                range: part_start..source.len(),
                unconditional_clock: false,
                references: Arc::from([]),
                required_loops: FormatNeeds::default(),
            });
        }
        if source.contains('%') || !split_safe {
            let mut part_references = references.clone();
            if !split_safe {
                part_references.insert("*".to_owned());
            }
            format_parts = vec![FormatPart {
                range: 0..source.len(),
                unconditional_clock: true,
                references: part_references.into_iter().collect(),
                required_loops: FormatNeeds::default(),
            }];
        }
        let mut parts = Vec::new();
        let mut offset = 0;
        for part in split_top(source, ',') {
            parts.push(offset..offset + part.len());
            offset += part.len() + 1;
        }
        clock_dependent |= references
            .iter()
            .any(|name| name == "*" || name.starts_with("T:"));
        let mut template = Self {
            operations,
            parts: parts.into(),
            references: references.into_iter().collect(),
            bytes: 0,
            clock_dependent,
            unconditional_clock,
            format_parts: format_parts.into(),
        };
        template.bytes = template
            .allocation_bytes()
            .saturating_add(std::mem::size_of::<Self>())
            .saturating_add(std::mem::size_of::<usize>().saturating_mul(2));
        template
    }

    fn allocation_bytes(&self) -> usize {
        let mut bytes = self
            .operations
            .capacity()
            .saturating_mul(std::mem::size_of::<Operation>())
            .saturating_add(
                self.parts
                    .len()
                    .saturating_mul(std::mem::size_of::<Range<usize>>()),
            )
            .saturating_add(
                self.references
                    .len()
                    .saturating_mul(std::mem::size_of::<String>()),
            )
            .saturating_add(std::mem::size_of::<usize>().saturating_mul(4));
        bytes = bytes
            .saturating_add(
                self.format_parts
                    .len()
                    .saturating_mul(std::mem::size_of::<FormatPart>()),
            )
            .saturating_add(std::mem::size_of::<usize>().saturating_mul(2));
        for part in self.format_parts.iter() {
            bytes = bytes
                .saturating_add(
                    part.references
                        .len()
                        .saturating_mul(std::mem::size_of::<String>()),
                )
                .saturating_add(std::mem::size_of::<usize>().saturating_mul(2));
            for reference in part.references.iter() {
                bytes = bytes.saturating_add(reference.capacity());
            }
        }
        for operation in &self.operations {
            bytes = bytes.saturating_add(match operation {
                Operation::Text(value) | Operation::Shell(value) => value.len(),
                Operation::Variable(_) => 0,
                Operation::Replacement(replacement) => std::mem::size_of::<Replacement>()
                    .saturating_add(replacement.allocation_bytes()),
                Operation::Style(prefix, body) => {
                    prefix.len().saturating_add(body.allocation_bytes())
                }
            });
        }
        for reference in self.references.iter() {
            bytes = bytes.saturating_add(reference.capacity());
        }
        bytes
    }

    fn flush(operations: &mut Vec<Operation>, literal: &mut String) {
        if !literal.is_empty() {
            operations.push(Operation::Text(std::mem::take(literal).into()));
        }
    }

    fn push_part(
        parts: &mut Vec<FormatPart>,
        start: &mut usize,
        previous_raw: &mut bool,
        range: Range<usize>,
        unconditional_clock: bool,
        (references, required_loops): (Arc<[String]>, FormatNeeds),
        raw: bool,
    ) {
        let adjacent = *start == range.start;
        if !adjacent {
            parts.push(FormatPart {
                range: *start..range.start,
                unconditional_clock: false,
                references: Arc::from([]),
                required_loops: FormatNeeds::default(),
            });
        }
        *start = range.end;
        if raw && *previous_raw && adjacent {
            let previous = parts.last_mut().unwrap();
            previous.range.end = range.end;
            previous.unconditional_clock |= unconditional_clock;
            previous.required_loops |= required_loops;
            let mut combined = previous
                .references
                .iter()
                .chain(references.iter())
                .cloned()
                .collect::<Vec<_>>();
            combined.sort_unstable();
            combined.dedup();
            previous.references = combined.into();
        } else {
            parts.push(FormatPart {
                range,
                unconditional_clock,
                references,
                required_loops,
            });
        }
        *previous_raw = raw;
    }
}

pub(super) struct Flags {
    literal: bool,
    basename: bool,
    dirname: bool,
    length: bool,
    width: bool,
    expand: bool,
    expand_time: bool,
    character: bool,
    not: bool,
    not_not: bool,
    name_window: bool,
    name_session: bool,
    loop_sessions: bool,
    loop_windows: bool,
    loop_panes: bool,
    loop_clients: bool,
    loop_reversed: bool,
    repeat: bool,
    quote_shell: bool,
    quote_single: bool,
    quote_style: bool,
    quote_arguments: bool,
    bool_op: Option<bool>,
    padding: isize,
    loop_sort: Option<LoopSort>,
    colour: Option<String>,
    loop_options: Option<String>,
    loop_environment: Option<String>,
    content_search: Option<String>,
    interrogate: Option<String>,
    comparison: Option<Comparison<'static>>,
    match_flags: Option<String>,
    substitutions: Vec<usize>,
    expression: Option<usize>,
    limit: Option<(isize, Option<String>)>,
    time_enabled: bool,
    time_pretty: bool,
    time_relative: bool,
    time_difference: bool,
    time_format: Option<String>,
}

impl Flags {
    fn allocation_bytes(&self) -> usize {
        [
            &self.colour,
            &self.loop_options,
            &self.loop_environment,
            &self.content_search,
            &self.interrogate,
            &self.match_flags,
            &self.time_format,
        ]
        .into_iter()
        .flatten()
        .chain(self.limit.iter().filter_map(|(_, marker)| marker.as_ref()))
        .fold(
            self.substitutions
                .capacity()
                .saturating_mul(std::mem::size_of::<usize>()),
            |bytes, value| bytes.saturating_add(value.capacity()),
        )
    }

    fn new(modifiers: &[Modifier]) -> Self {
        let flags = ModifierFlags::from_modifiers(modifiers);
        let (comparison, match_flags) = match flags.comparison {
            Some(Comparison::Match(value)) => (None, Some(value.to_owned())),
            value => (
                value.map(|comparison| match comparison {
                    Comparison::Equal => Comparison::Equal,
                    Comparison::NotEqual => Comparison::NotEqual,
                    Comparison::Less => Comparison::Less,
                    Comparison::Greater => Comparison::Greater,
                    Comparison::LessEqual => Comparison::LessEqual,
                    Comparison::GreaterEqual => Comparison::GreaterEqual,
                    Comparison::Match(_) => unreachable!(),
                }),
                None,
            ),
        };
        Self {
            literal: flags.literal,
            basename: flags.basename,
            dirname: flags.dirname,
            length: flags.length,
            width: flags.width,
            expand: flags.expand,
            expand_time: flags.expand_time,
            character: flags.character,
            not: flags.not,
            not_not: flags.not_not,
            name_window: flags.name_window,
            name_session: flags.name_session,
            loop_sessions: flags.loop_sessions,
            loop_windows: flags.loop_windows,
            loop_panes: flags.loop_panes,
            loop_clients: flags.loop_clients,
            loop_reversed: flags.loop_reversed,
            repeat: flags.repeat,
            quote_shell: flags.quote_shell,
            quote_single: flags.quote_single,
            quote_style: flags.quote_style,
            quote_arguments: flags.quote_arguments,
            bool_op: flags.bool_op,
            padding: flags.padding,
            loop_sort: flags.loop_sort,
            colour: flags.colour.map(str::to_owned),
            loop_options: flags.loop_options.map(str::to_owned),
            loop_environment: flags.loop_environment.map(str::to_owned),
            content_search: flags.content_search.map(str::to_owned),
            interrogate: flags.interrogate.map(str::to_owned),
            comparison,
            match_flags,
            substitutions: flags
                .substitutions
                .iter()
                .map(|modifier| {
                    modifiers
                        .iter()
                        .position(|candidate| std::ptr::eq(candidate, *modifier))
                        .unwrap()
                })
                .collect(),
            expression: flags.expression.map(|modifier| {
                modifiers
                    .iter()
                    .position(|candidate| std::ptr::eq(candidate, modifier))
                    .unwrap()
            }),
            limit: flags
                .limit
                .map(|(limit, marker)| (limit, marker.map(str::to_owned))),
            time_enabled: flags.time.enabled,
            time_pretty: flags.time.pretty,
            time_relative: flags.time.relative,
            time_difference: flags.time.difference,
            time_format: flags.time.format.map(str::to_owned),
        }
    }

    pub(super) fn view<'a>(&'a self, modifiers: &'a [Modifier]) -> ModifierFlags<'a> {
        ModifierFlags {
            literal: self.literal,
            basename: self.basename,
            dirname: self.dirname,
            length: self.length,
            width: self.width,
            expand: self.expand,
            expand_time: self.expand_time,
            character: self.character,
            not: self.not,
            not_not: self.not_not,
            name_window: self.name_window,
            name_session: self.name_session,
            loop_sessions: self.loop_sessions,
            loop_windows: self.loop_windows,
            loop_panes: self.loop_panes,
            loop_clients: self.loop_clients,
            loop_reversed: self.loop_reversed,
            repeat: self.repeat,
            quote_shell: self.quote_shell,
            quote_single: self.quote_single,
            quote_style: self.quote_style,
            quote_arguments: self.quote_arguments,
            bool_op: self.bool_op,
            padding: self.padding,
            loop_sort: self.loop_sort,
            colour: self.colour.as_deref(),
            loop_options: self.loop_options.as_deref(),
            loop_environment: self.loop_environment.as_deref(),
            content_search: self.content_search.as_deref(),
            interrogate: self.interrogate.as_deref(),
            comparison: self
                .match_flags
                .as_deref()
                .map(Comparison::Match)
                .or(self.comparison),
            substitutions: self
                .substitutions
                .iter()
                .map(|index| &modifiers[*index])
                .collect(),
            expression: self.expression.map(|index| &modifiers[index]),
            limit: self
                .limit
                .as_ref()
                .map(|(limit, marker)| (*limit, marker.as_deref())),
            time: TimeFlags {
                enabled: self.time_enabled,
                pretty: self.time_pretty,
                relative: self.time_relative,
                difference: self.time_difference,
                format: self.time_format.as_deref(),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ExecutionContext;
    use zz_protocol::{Axis, CommandInvocation};

    #[derive(Default)]
    struct Hooks {
        calls: Vec<String>,
    }

    impl StatusHooks for Hooks {
        fn strftime(&mut self, literal: &str) -> String {
            literal.replace("%H:%M", "09:41").replace("%Y", "2026")
        }

        fn shell(&mut self, command: &str, _tag: &FormatJobTag) -> String {
            self.calls.push(command.to_owned());
            format!("<{command}>")
        }

        fn variable(&mut self, name: &str, _context: &StatusContext) -> Option<String> {
            match name {
                "@dynamic" => Some("#{pane_id}-%Y".to_owned()),
                "@padding" => Some("7".to_owned()),
                _ => None,
            }
        }
    }

    fn compare(format: &str, engine: &MuxEngine, context: FormatContext) {
        let mut interpreted_hooks = Hooks::default();
        let interpreted = tree::with_borrowed_formats(false, || {
            with_enabled(false, || {
                expand_format_time_with_hooks(format, engine, context, &mut interpreted_hooks)
            })
        });
        let mut compiled_hooks = Hooks::default();
        let compiled = tree::with_borrowed_formats(true, || {
            with_enabled(true, || {
                expand_format_time_with_hooks(format, engine, context, &mut compiled_hooks)
            })
        });
        assert_eq!(compiled.as_bytes(), interpreted.as_bytes(), "{format}");
        assert_eq!(compiled_hooks.calls, interpreted_hooks.calls, "{format}");
    }

    fn scene() -> (MuxEngine, FormatContext) {
        let mut engine = MuxEngine::default();
        engine.set_format_server_context("tower.local", "tower", "/tmp/zz-fmt.sock", 946_728_000);
        let (session, window, pane) = engine.state.create_session("work").unwrap();
        engine
            .state
            .split_pane(pane, Axis::Horizontal, PaneKind::Terminal)
            .unwrap();
        engine
            .state
            .create_window(session, Some("logs".to_owned()), PaneKind::Terminal)
            .unwrap();
        engine.state.create_session("other").unwrap();
        let mut execution = ExecutionContext::default();
        for (name, args) in [
            (
                "set-option",
                vec!["-g", "@fmt", "#{pane_id}/#{session_name}"],
            ),
            ("set-option", vec!["-g", "@padding", "7"]),
            ("set-environment", vec!["-g", "FMT", "value"]),
        ] {
            engine
                .execute(&mut execution, &CommandInvocation::new(name, args))
                .unwrap();
        }
        let context = FormatContext {
            session: Some(session),
            window: Some(window),
            pane: Some(pane),
            active_session: Some(session),
            format_client: FormatClient::Attached(session),
            format_type: FormatType::Pane,
        };
        (engine, context)
    }

    #[test]
    fn compiled_matches_lazy_interpreter_for_every_pinned_name() {
        let (engine, context) = scene();
        for spec in &FORMAT_VARIABLES {
            let name = spec.name;
            for format in [
                format!("before#{{{name}}}after"),
                format!("#[fg=#{{{name}}}]#{{{name}}}"),
                format!("#{{?{name},yes,no}}"),
                format!("#{{?#{{{name}}},yes,no}}"),
                format!("#{{p12;=8/...;q;w:{name}}}"),
                format!("#{{s/a/b/;n:{name}}}"),
                format!("#{{==:#{{{name}}},#{{{name}}}}}"),
                format!("#{{&&:#{{{name}}},1,0}}"),
                format!("#{{S:#{{session_name}}=#{{{name}}};}}"),
                format!("#{{W:#{{window_name}}=#{{{name}}};}}"),
                format!("#{{P:#{{pane_id}}=#{{{name}}};}}"),
            ] {
                let session = context.session.unwrap();
                let other = *engine
                    .state
                    .sessions
                    .keys()
                    .find(|candidate| **candidate != session)
                    .unwrap();
                for client in [
                    FormatClient::NoClient,
                    FormatClient::Unattached,
                    FormatClient::Attached(session),
                    FormatClient::Attached(other),
                ] {
                    for format_type in [
                        FormatType::None,
                        FormatType::Session,
                        FormatType::Window,
                        FormatType::Pane,
                    ] {
                        compare(
                            &format,
                            &engine,
                            FormatContext {
                                format_client: client,
                                format_type,
                                ..context
                            },
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn compiled_matches_w1_for_marked_zoomed_dead_and_null_fixture_contexts() {
        for (_, engine) in crate::format_universe_tests::fixtures() {
            for client in crate::format_universe_tests::clients(&engine) {
                for (session, window, pane) in crate::format_universe_tests::targets(&engine) {
                    let context = FormatContext {
                        session,
                        window,
                        pane,
                        active_session: session,
                        format_client: client,
                        format_type: FormatType::Pane,
                    };
                    for spec in &FORMAT_VARIABLES {
                        let name = spec.name;
                        compare(
                            &format!("#{{{name}}}|#{{p10:{name}}}|#{{?{name},yes,no}}"),
                            &engine,
                            context,
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn compiled_matches_interpreter_for_nested_dynamic_and_malformed_templates() {
        let (engine, context) = scene();
        let atoms = [
            "text",
            "#S",
            "##S",
            "###",
            "#}",
            "#,",
            "#{",
            "#(",
            "#[",
            "#{pane_id}",
            "#{E:@fmt}",
            "#{T:@dynamic}",
            "#{p/#{@padding}/:pane_id}",
            "#{T;=/#{status-left-length}:status-left}",
            "#{T;=/#{status-right-length}:status-right}",
            "#{p/#{@padding}/;p3:pane_id}",
            "#{p12;p/#{@padding}/:pane_id}",
            "#{p/#(first)/;p3:pane_id}",
            "#{l:literal-##-#,-#{pane_id}}",
            "#{s/#(first)/x/;bad:pane_id}",
            "#{?pane_id,#{W:#{P:#{pane_id}}},none}",
            "#{?missing,a,pane_id,b,c}",
            "#{||:0,#{pane_id},#(unused)}",
            "#{e|+|f|3:2.5,#{@padding}}",
            "#{R:ab,3}",
            "#{=3/#(marker)/:pane_id}",
            "###[fg=#{?pane_id,red,blue}]#(job)",
            "#{O:#{option_name}=#{option_value};}",
            "#{Vg:#{environ_name}=#{environ_value};}",
            "界#{pane_id}🌻",
            "%H:%M#{pane_id}",
        ];
        for atom in atoms {
            compare(atom, &engine, context);
        }
        let mut state = 0x9e37_79b9u64;
        for _ in 0..1024 {
            let mut format = String::new();
            for _ in 0..6 {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                format.push_str(atoms[state as usize % atoms.len()]);
            }
            compare(&format, &engine, context);
        }
        let mut deep = "#{p1:pane_id}".to_owned();
        for _ in 0..FORMAT_LOOP_LIMIT + 2 {
            deep = format!("#{{?1,{deep},no}}");
        }
        compare(&deep, &engine, context);
    }

    #[test]
    fn option_table_command_tree_and_environment_keep_the_pinned_lookup_order() {
        struct Phases;
        impl StatusHooks for Phases {
            fn strftime(&mut self, literal: &str) -> String {
                literal.to_owned()
            }
            fn shell(&mut self, _command: &str, _tag: &FormatJobTag) -> String {
                String::new()
            }
            fn option_variable(&mut self, name: &str, _context: &StatusContext) -> Option<String> {
                (name == "pane_id").then(|| "option".to_owned())
            }
            fn variable(&mut self, name: &str, _context: &StatusContext) -> Option<String> {
                (name == "pane_in_mode").then(|| "callback".to_owned())
            }
            fn tree_variable(
                &mut self,
                name: &str,
                _context: &StatusContext,
            ) -> Option<Cow<'_, str>> {
                match name {
                    "pane_id" | "pane_title" | "pane_in_mode" => Some(Cow::Borrowed("shadow")),
                    "CMD" | "ENV" => Some(Cow::Borrowed("command")),
                    _ => None,
                }
            }
        }
        let mut context = StatusContext::from(StatusValues {
            pane_id: "%7".to_owned(),
            pane_title: "table".to_owned(),
            ..StatusValues::default()
        });
        context.format_universe = FormatUniverseRef {
            parts: Arc::new(FormatUniverse::with_global_environment(vec![
                FormatEnvironRow {
                    name: "ENV".to_owned(),
                    value: RawText::from("environment"),
                    ..FormatEnvironRow::default()
                },
                FormatEnvironRow {
                    name: "ONLY_ENV".to_owned(),
                    value: RawText::from("environment"),
                    ..FormatEnvironRow::default()
                },
                FormatEnvironRow {
                    name: "FIRST".to_owned(),
                    value: RawText::from_bytes(vec![0xc3]),
                    ..FormatEnvironRow::default()
                },
                FormatEnvironRow {
                    name: "SECOND".to_owned(),
                    value: RawText::from_bytes(vec![0xa9]),
                    ..FormatEnvironRow::default()
                },
            ])),
            engine: None,
        };
        for enabled in [false, true] {
            with_enabled(enabled, || {
                assert_eq!(
                    expand_status(
                        "#{pane_id}|#{pane_title}|#{pane_in_mode}|#{CMD}|#{ENV}|#{ONLY_ENV}",
                        &context,
                        &mut Phases
                    ),
                    "option|table|callback|command|command|environment"
                );
                assert_eq!(
                    expand_status("#[#{FIRST}#{SECOND}]", &context, &mut Phases),
                    "#[\u{fffd}\u{fffd}]"
                );
            });
        }
    }

    #[test]
    fn compiled_clock_dependencies_include_nested_time_and_dynamic_formats() {
        for source in [
            "%S",
            "literal 100%",
            "#{t:start_time}",
            "#{t/r:start_time}",
            "#{t/d:start_time}",
            "#{t/f/%Y:start_time}",
            "#{T:status-right}",
            "#{?pane_active,#{t/r:start_time},plain}",
            "#{?#{==:#{t/d:start_time},0},fresh,old}",
            "#{p/#{t/d:start_time}/:pane_title}",
            "#{s/foo/#{t/r:start_time}/:pane_title}",
            "#{P:#{t/d:pane_dead_time}}",
            "#[fg=#{?pane_active,#{t/d:start_time},green}]",
            "#(printf constant)",
            "#[fg=#(printf red)]",
            "#{E:#{@format}}",
            "#{p/#{pane_width}/:pane_title}",
            "#{L:constant}",
            "#{I/c:RGB}",
        ] {
            assert!(format_clock_dependent(source), "{source}");
        }
        let deep = format!(
            "{}plain{}",
            "#{?pane_active,".repeat(FORMAT_LOOP_LIMIT),
            ",plain}".repeat(FORMAT_LOOP_LIMIT)
        );
        assert!(format_clock_dependent(&deep));
    }

    #[test]
    fn compiled_clock_dependencies_keep_static_native_and_literal_formats_independent() {
        for source in [
            "",
            "arbitrary literal fg=cyan,bg=black",
            "#S #{pane_title}",
            "#{session_created}:#{start_time}",
            "#{?pane_active,#{session_name},plain}",
            "#{==:#{window_index},2}",
            "#{p/8/:session_name}",
            "#[fg=#{?pane_active,red,green}]",
            "#{E:pane-border-style}",
            "##{t/r:start_time}",
            "##(printf constant)",
            "#{l:#{t/r:start_time}}",
        ] {
            assert!(!format_clock_dependent(source), "{source}");
        }
    }

    #[test]
    fn compiled_format_parts_preserve_ranges_and_conditional_time_dependencies() {
        let source = "界#S|#{T:status-right}|#[fg=#{?pane_active,red,green}]🌻";
        let parts = format_parts(source);
        assert_eq!(parts.first().unwrap().range.start, 0);
        assert_eq!(parts.last().unwrap().range.end, source.len());
        assert!(
            parts
                .windows(2)
                .all(|pair| pair[0].range.end == pair[1].range.start)
        );
        assert_eq!(
            parts
                .iter()
                .map(|part| &source[part.range.clone()])
                .collect::<String>(),
            source
        );
        let time = parts
            .iter()
            .find(|part| part.references.iter().any(|name| name == "T:status-right"))
            .unwrap();
        assert!(!time.unconditional_clock);
        assert!(format_clock_dependent(source));
        assert!(parts.iter().all(|part| !part.unconditional_clock));
        for source in [
            "#{?pane_active,#{T:status-right},#{E:@static}}",
            "#[fg=#{T:@colour}]",
            "##{t/r:start_time}",
            "#{l:#{t/r:start_time}}",
        ] {
            assert!(
                format_parts(source)
                    .iter()
                    .all(|part| !part.unconditional_clock),
                "{source}"
            );
        }
        for source in [
            "#{t:start_time}",
            "#{t/r:start_time}",
            "#{t/d:start_time}",
            "#{?pane_active,#{t/r:start_time},plain}",
            "#[fg=#{?pane_active,#{t/d:start_time},green}]",
            "#(printf constant)",
            "#{E:#{@format}}",
            "#{I/c:RGB}",
        ] {
            assert!(
                format_parts(source)
                    .iter()
                    .any(|part| part.unconditional_clock),
                "{source}"
            );
        }
        for source in ["a%Sb#{pane_title}", "##{t/f/%Y:start_time}"] {
            let parts = format_parts(source);
            assert_eq!(parts.len(), 1);
            assert_eq!(parts[0].range, 0..source.len());
            assert!(parts[0].unconditional_clock);
        }
        let source = "#{W:#{window_name}}|#{P:#{pane_id}}";
        let parts = format_parts(source);
        assert_eq!(parts.len(), 3);
        assert_eq!(parts[0].required_loops, FormatNeeds::WINDOWS);
        assert_eq!(parts[2].required_loops, FormatNeeds::PANES);
        assert!(parts.iter().all(|part| !part.unconditional_clock));
        assert_eq!(
            format_parts("#{?pane_active,#{W:#{window_name}},plain}")[0].required_loops,
            FormatNeeds::default()
        );
    }

    #[test]
    fn compiled_format_parts_match_whole_expansion_and_early_stops() {
        let (engine, context) = scene();
        let atoms = [
            "arbitrary literal",
            "界#S🌻",
            "##S",
            "###",
            "#}",
            "#,",
            "#{",
            "#(",
            "#[",
            "#{pane_id}#{pane_title}",
            "#{E:@fmt}",
            "#{T:@dynamic}",
            "#{p/#{@padding}/:pane_id}",
            "#{l:literal-##-#,-#{pane_id}}",
            "#{s/#(first)/x/;bad:pane_id}",
            "#{?pane_id,#{W:#{P:#{pane_id}}},none}",
            "#{?missing,a,pane_id,b,c}",
            "#{||:0,#{pane_id},#(unused)}",
            "#{==:#{window_index},2}",
            "#{==:missing}",
            "#{R:ab,3}",
            "#{R:missing}",
            "#{N:work}",
            "#{W:#{window_name}}",
            "#{P:#{pane_id}}",
            "#{e|+|f|3:2.5,#{@padding}}",
            "###[fg=#{?pane_id,red,blue}]#(job)",
            "%H:%M#{pane_id}",
        ];
        let compare_parts = |source: &str| {
            let parts = format_parts(source);
            assert_eq!(parts.first().unwrap().range.start, 0, "{source}");
            assert_eq!(parts.last().unwrap().range.end, source.len(), "{source}");
            assert!(
                parts
                    .windows(2)
                    .all(|pair| pair[0].range.end == pair[1].range.start),
                "{source}"
            );
            for enabled in [false, true] {
                with_enabled(enabled, || {
                    let mut whole_hooks = Hooks::default();
                    let whole =
                        expand_format_time_with_hooks(source, &engine, context, &mut whole_hooks);
                    let mut part_hooks = Hooks::default();
                    let segmented = parts
                        .iter()
                        .map(|part| {
                            expand_format_time_with_hooks(
                                &source[part.range.clone()],
                                &engine,
                                context,
                                &mut part_hooks,
                            )
                            .to_string()
                        })
                        .collect::<String>();
                    assert_eq!(segmented, whole.to_string(), "{source}, compiled={enabled}");
                    assert_eq!(part_hooks.calls, whole_hooks.calls, "{source}");
                });
            }
        };
        for atom in atoms {
            compare_parts(&format!("before|{atom}|after"));
        }
        for source in [
            "before|#{==:missing}|after",
            "before|#{R:missing}|after",
            "before|#{N:work}|after",
            "before|#{N/#{@scope}:work}|after",
            "before|#{s/foo/bar/;bad:pane_title}|after",
            "before|#{unterminated",
        ] {
            let parts = format_parts(source);
            assert_eq!(parts.len(), 1, "{source}");
            assert_eq!(parts[0].range, 0..source.len());
            assert!(parts[0].unconditional_clock, "{source}");
        }
        let mut state = 0x9e37_79b9u64;
        for _ in 0..256 {
            let mut source = String::new();
            for _ in 0..4 {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                source.push_str(atoms[state as usize % atoms.len()]);
            }
            compare_parts(&source);
        }
        assert!(format_parts("").is_empty());
    }

    #[test]
    fn compiled_format_parts_keep_infallible_dynamic_arguments_conditionally_clocked() {
        let source = "#{T;=/#{status-left-length}:status-left}|#{W:#{window_name}}|#{T;=/#{status-right-length}:status-right}";
        let parts = format_parts(source);
        assert_eq!(parts.len(), 5);
        assert!(!parts[0].unconditional_clock);
        assert!(!parts[2].unconditional_clock);
        assert!(!parts[4].unconditional_clock);
        assert_eq!(parts[2].required_loops, FormatNeeds::WINDOWS);
        assert_eq!(&source[parts[2].range.clone()], "#{W:#{window_name}}");
        assert!(
            parts[0]
                .references
                .iter()
                .any(|name| name == "status-left-length")
        );
        assert!(
            parts[4]
                .references
                .iter()
                .any(|name| name == "status-right-length")
        );
        assert!(
            parts[0]
                .references
                .iter()
                .any(|name| name == "T:status-left")
        );
        assert!(
            parts[4]
                .references
                .iter()
                .any(|name| name == "T:status-right")
        );
        let context = StatusContext::from(StatusValues {
            session_name: "abcdefgh".to_owned(),
            ..StatusValues::default()
        });
        for enabled in [false, true] {
            with_enabled(enabled, || {
                for width in ["1", "4", "-3", "invalid", "100000000", "x:#(not-run)", "%S"] {
                    struct Width<'a>(&'a str);
                    impl StatusHooks for Width<'_> {
                        fn strftime(&mut self, _source: &str) -> String {
                            panic!("argument data must not be interpreted as a time format")
                        }
                        fn shell(&mut self, _command: &str, _tag: &FormatJobTag) -> String {
                            panic!("expanded modifier text must not be parsed as another command")
                        }
                        fn variable(
                            &mut self,
                            name: &str,
                            _context: &StatusContext,
                        ) -> Option<String> {
                            (name == "WIDTH").then(|| self.0.to_owned())
                        }
                    }
                    for source in [
                        "before|#{=/#{WIDTH}:session_name}|after",
                        "before|#{p/#{WIDTH}/:session_name}|after",
                        "before|#{=/4/#{WIDTH}:session_name}|after",
                    ] {
                        let parts = format_parts(source);
                        assert_eq!(parts.len(), 3);
                        assert!(!parts[1].unconditional_clock);
                        assert!(parts[1].references.iter().any(|name| name == "WIDTH"));
                        let mut hooks = Width(width);
                        let whole = expand_status(source, &context, &mut hooks);
                        if source == "before|#{=/4/#{WIDTH}:session_name}|after" && width == "%S" {
                            assert_eq!(whole, "before|abcd%S|after");
                        }
                        let segmented = parts
                            .iter()
                            .map(|part| {
                                expand_status(&source[part.range.clone()], &context, &mut hooks)
                            })
                            .collect::<String>();
                        assert_eq!(segmented, whole, "{source}: {width}, compiled={enabled}");
                    }
                }
            });
        }
    }

    #[test]
    fn compiled_format_parts_preserve_actual_clock_dependencies_in_dynamic_arguments() {
        struct Clock(&'static str);
        impl StatusHooks for Clock {
            fn strftime(&mut self, source: &str) -> String {
                source.replace("%S", self.0)
            }
            fn shell(&mut self, _command: &str, _tag: &FormatJobTag) -> String {
                panic!("clock argument test must not invoke a shell")
            }
            fn option_variable(&mut self, name: &str, _context: &StatusContext) -> Option<String> {
                (name == "@clock_width").then(|| "%S".to_owned())
            }
        }
        for enabled in [false, true] {
            with_enabled(enabled, || {
                for (source, unconditional) in [
                    ("before|#{p/#{T:@clock_width}/:session_name}|after", false),
                    ("before|#{=/#{T:@clock_width}:session_name}|after", false),
                    ("before|#{=/4/#{T:@clock_width}:session_name}|after", false),
                    ("before|#{p/#{t/d:start_time}/:session_name}|after", true),
                    ("before|#{p/%S/:session_name}|after", true),
                ] {
                    let parts = format_parts(source);
                    assert_eq!(
                        parts.iter().any(|part| part.unconditional_clock),
                        unconditional,
                        "{source}"
                    );
                    if !unconditional {
                        assert!(
                            parts.iter().any(|part| part
                                .references
                                .iter()
                                .any(|name| name == "T:@clock_width")),
                            "{source}"
                        );
                    }
                    let mut results = Vec::new();
                    for (seconds, now) in [("12", 112), ("15", 115)] {
                        let context = StatusContext::from(StatusValues {
                            session_name: if source.contains("#{p/") {
                                "x"
                            } else {
                                "abcdefghijklmnopqrst"
                            }
                            .to_owned(),
                            start_time: Some(100),
                            format_now: Some(now),
                            ..StatusValues::default()
                        });
                        let mut clock = Clock(seconds);
                        let whole = expand_status(source, &context, &mut clock);
                        let segmented = parts
                            .iter()
                            .map(|part| {
                                expand_status(&source[part.range.clone()], &context, &mut clock)
                            })
                            .collect::<String>();
                        assert_eq!(segmented, whole, "{source}: {seconds}, compiled={enabled}");
                        results.push(whole);
                    }
                    assert_ne!(results[0], results[1], "{source}, compiled={enabled}");
                }
            });
        }
        for source in [
            "#{p/#(date)/:session_name}",
            "#{=/#{I/c:RGB}:session_name}",
            "#{N/#{WIDTH}:name}",
        ] {
            assert!(
                format_parts(source)
                    .iter()
                    .all(|part| part.unconditional_clock),
                "{source}"
            );
        }
    }

    #[test]
    fn compiled_format_parts_keep_adjacent_raw_values_in_one_expansion() {
        let context = StatusContext {
            format_universe: FormatUniverseRef {
                parts: Arc::new(FormatUniverse::with_global_environment(vec![
                    FormatEnvironRow {
                        name: "FIRST".to_owned(),
                        value: RawText::from_bytes(vec![0xc3]),
                        ..FormatEnvironRow::default()
                    },
                    FormatEnvironRow {
                        name: "SECOND".to_owned(),
                        value: RawText::from_bytes(vec![0xa9]),
                        ..FormatEnvironRow::default()
                    },
                ])),
                engine: None,
            },
            ..StatusContext::default()
        };
        for enabled in [false, true] {
            with_enabled(enabled, || {
                for source in [
                    "#{FIRST}#{SECOND}",
                    "#{FIRST}#{l:}#{SECOND}",
                    "#{FIRST}#{?missing,,}#{SECOND}",
                    "x#{FIRST}#{SECOND}|#[#{FIRST}#{SECOND}]",
                ] {
                    let parts = format_parts(source);
                    let mut hooks = Hooks::default();
                    let whole = expand_status(source, &context, &mut hooks);
                    let segmented = parts
                        .iter()
                        .map(|part| {
                            expand_status(&source[part.range.clone()], &context, &mut hooks)
                        })
                        .collect::<String>();
                    assert_eq!(segmented, whole, "{source}");
                    assert!(parts.iter().any(|part| {
                        let source = &source[part.range.clone()];
                        source.contains("#{FIRST}") && source.contains("#{SECOND}")
                    }));
                }
            });
        }
    }

    #[test]
    fn compiled_cache_retention_respects_format_cache_rollback() {
        let source = "clock-retention:#{t/d:start_time}";
        let first = get(source);
        let second = get(source);
        assert!(Arc::ptr_eq(&first, &second));
        assert!(Arc::ptr_eq(&first.format_parts, &second.format_parts));
        assert!(first.clock_dependent);
        assert_eq!(first.references, second.references);
        let cache = CACHE.lock();
        assert!(cache.entries.contains_key(source));
        assert_eq!(
            cache
                .order
                .iter()
                .filter(|entry| entry.as_ref() == source)
                .count(),
            1,
        );
    }

    #[test]
    fn implicit_fact_modifiers_capture_unknown_dependencies() {
        for source in [
            "#{L:constant}",
            "#{L:#{client_name}}",
            "#{Vc:constant}",
            "#{V/c:constant}",
            "#{V/#{@scope}:constant}",
            "#{C:pattern}",
            "#{I/c:RGB}",
            "#{I/f:RGB}",
            "#{I/e:SHELL}",
            "#{I/#{@flags}:RGB}",
            "#{l;I/c:RGB}",
            "#[fg=#{C:pattern}]",
            "#[fg=#{I/c:RGB}]",
            "#{p/#{L:constant}/:pane_title}",
            "#{?pane_active,#{Vc:constant},plain}",
        ] {
            let references = get(source).references.clone();
            assert!(
                references.iter().any(|name| name == "*"),
                "{source}: {references:?}"
            );
        }
    }

    #[test]
    fn escaped_literal_and_nonclient_environment_templates_remain_selective() {
        for source in [
            "##{L:constant}",
            "##{C:pattern}",
            "##{I/c:RGB}",
            "#{l:#{L:constant}}",
            "#{l:#{Vc:constant}}",
            "#{l:#{I/c:RGB}}",
            "#{l;L:constant}",
            "#{l;C:pattern}",
            "#{l;Vc:constant}",
            "#{Vg:#{environ_name}}",
            "#{Vs:#{environ_name}}",
            "#S #{pane_title}",
        ] {
            let references = get(source).references.clone();
            assert!(
                !references.iter().any(|name| name == "*"),
                "{source}: {references:?}"
            );
        }
        assert_eq!(
            get("#S #{pane_title}").references.as_ref(),
            ["pane_title", "session_name"]
        );
    }

    #[test]
    fn cached_reference_access_respects_cache_rollback_and_option_changes() {
        let (mut engine, _) = scene();
        for source in ["#{pane_title}", "#{L:constant}", "#{E:@fmt}"] {
            let first = engine.cached_format_references(source);
            let second = engine.cached_format_references(source);
            assert_eq!(first, second, "{source}");
            assert!(Arc::ptr_eq(&first, &second), "{source}");
        }
        let before = engine.cached_format_references("#{E:@fmt}");
        assert!(before.contains("pane_id"));
        assert!(!before.contains("*"));
        engine
            .execute(
                &mut ExecutionContext::default(),
                &CommandInvocation::new("set-option", ["-g", "@fmt", "#{I/c:RGB}"]),
            )
            .unwrap();
        let after = engine.cached_format_references("#{E:@fmt}");
        assert!(after.contains("*"));
        assert!(!after.contains("pane_id"));
        assert!(!Arc::ptr_eq(&before, &after));
    }

    #[test]
    fn compiled_templates_reuse_syntax_across_connection_threads() {
        let source = "shared-connection-syntax:#{pane_id}";
        let first = std::thread::spawn(move || get(source)).join().unwrap();
        let second = std::thread::spawn(move || get(source)).join().unwrap();
        assert!(Arc::ptr_eq(&first, &second));
    }

    #[test]
    fn compiled_cache_rejects_templates_with_oversized_spare_operation_capacity() {
        let mut cache = Cache::default();
        let count = (CACHE_BYTES / (std::mem::size_of::<Operation>() * 2)).next_power_of_two() + 1;
        let source = "#S".repeat(count);
        let template = Template::parse(&source, false);
        assert!(template.operations.capacity() > template.operations.len());
        let bytes = template
            .bytes
            .saturating_add(source.len().saturating_mul(2));
        let spare = template
            .operations
            .capacity()
            .saturating_sub(template.operations.len())
            .saturating_mul(std::mem::size_of::<Operation>());
        assert!(bytes > CACHE_BYTES - CACHE_CONTAINER_BYTES);
        assert!(bytes.saturating_sub(spare) <= CACHE_BYTES - CACHE_CONTAINER_BYTES);
        cache.insert(&source, Arc::new(template));
        assert!(!cache.entries.contains_key(source.as_str()));
        assert_eq!(cache.bytes, 0);
    }

    #[test]
    fn compiled_cache_bounds_and_references_follow_operations() {
        let mut cache = Cache::default();
        for index in 0..CACHE_ENTRIES * 2 {
            let source = format!("{index}:#{{pane_id}}");
            cache.insert(&source, Arc::new(Template::parse(&source, false)));
        }
        {
            assert!(cache.entries.len() <= CACHE_ENTRIES);
            assert!(cache.bytes.saturating_add(CACHE_CONTAINER_BYTES) <= CACHE_BYTES);
            assert_eq!(
                cache.bytes,
                cache
                    .entries
                    .iter()
                    .fold(0_usize, |bytes, (source, template)| {
                        bytes.saturating_add(
                            template
                                .bytes
                                .saturating_add(source.len().saturating_mul(2)),
                        )
                    }),
            );
        }
        let template = get("#S #{?pane_active,#{p/#{@width}/:pane_id},#{E:status-left}}");
        for name in [
            "session_name",
            "pane_active",
            "@width",
            "pane_id",
            "status-left",
            "E:status-left",
        ] {
            assert!(
                template
                    .references
                    .iter()
                    .any(|reference| reference == name),
                "{name}"
            );
        }
        for source in [
            "#(printf value)",
            "#[fg=#(printf red)]",
            "#{p/#(width)/:pane_id}",
        ] {
            assert!(
                get(source)
                    .references
                    .iter()
                    .any(|reference| reference == "*")
            );
        }
        let oversized = "x".repeat(CACHE_BYTES);
        cache.insert(&oversized, Arc::new(Template::parse(&oversized, false)));
        assert!(!cache.entries.contains_key(oversized.as_str()));
    }
}
