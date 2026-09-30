use std::{cell::RefCell, collections::HashMap, ops::Range};

use super::*;

const CACHE_ENTRIES: usize = 512;
const CACHE_BYTES: usize = 1024 * 1024;

static COMPILED_FORMATS: LazyLock<bool> =
    LazyLock::new(|| std::env::var("ZZ_PERF_COMPILED_FORMATS").as_deref() != Ok("0"));

thread_local! {
    static CACHE: RefCell<Cache> = RefCell::new(Cache::default());
    #[cfg(test)]
    static ENABLED: Cell<Option<bool>> = const { Cell::new(None) };
}

pub(super) fn enabled() -> bool {
    #[cfg(test)]
    if let Some(enabled) = ENABLED.get() {
        return enabled;
    }
    *COMPILED_FORMATS
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
    if let Some(template) = CACHE.with_borrow(|cache| cache.entries.get(source).cloned()) {
        return template;
    }
    let template = Arc::new(Template::parse(source, false));
    let bytes = template.bytes + source.len();
    if bytes > CACHE_BYTES {
        return template;
    }
    CACHE.with_borrow_mut(|cache| {
        while cache.entries.len() >= CACHE_ENTRIES || cache.bytes + bytes > CACHE_BYTES {
            let Some(oldest) = cache.order.pop_front() else {
                break;
            };
            if let Some(removed) = cache.entries.remove(oldest.as_ref()) {
                cache.bytes -= removed.bytes + oldest.len();
            }
        }
        cache.bytes += bytes;
        cache.order.push_back(source.into());
        cache.entries.insert(source.into(), template.clone());
    });
    template
}

pub(super) struct Template {
    pub(super) operations: Vec<Operation>,
    pub(super) parts: Arc<[Range<usize>]>,
    pub(super) references: Arc<[String]>,
    bytes: usize,
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
    fn parse(body: &str, references: &mut BTreeSet<String>, depth: usize) -> Self {
        let mut arguments = Vec::new();
        let parsed = parse_modifiers(body, |argument| {
            arguments.push(argument.to_owned());
            argument.to_owned()
        });
        let (modifiers, copy, discarded) = match parsed {
            Some((modifiers, offset)) => (modifiers, &body[offset..], Vec::new()),
            None => (Vec::new(), body, arguments),
        };
        let flags = ModifierFlags::from_modifiers(&modifiers);
        for argument in modifiers
            .iter()
            .flat_map(|modifier| &modifier.args)
            .chain(&discarded)
        {
            references.extend(
                Template::parse_depth(argument, false, depth + 1)
                    .references
                    .iter()
                    .cloned(),
            );
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
                references.extend(
                    Template::parse_depth(part, false, depth + 1)
                        .references
                        .iter()
                        .cloned(),
                );
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
                references.extend(
                    Template::parse_depth(copy, false, depth + 1)
                        .references
                        .iter()
                        .cloned(),
                );
            } else {
                references.insert(copy.to_owned());
            }
        }
        let dynamic = modifiers
            .iter()
            .flat_map(|modifier| &modifier.args)
            .any(|argument| argument.contains(['#', '%']));
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
            return Self {
                operations: vec![Operation::Text(source.into())],
                parts: std::iter::once(0..source.len()).collect(),
                references: Arc::from(["*".to_owned()]),
                bytes: source.len(),
            };
        }
        let mut operations = Vec::new();
        let mut references = BTreeSet::new();
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
                    break;
                };
                Self::flush(&mut operations, &mut literal);
                let body = Self::parse_depth(&source[hashes_end + 1..end], true, depth + 1);
                references.extend(body.references.iter().cloned());
                operations.push(Operation::Style(source[index..=hashes_end].into(), body));
                index = end + 1;
                continue;
            }
            let next_index = index + 1;
            let Some(next) = source[next_index..].chars().next() else {
                break;
            };
            let after_next = next_index + next.len_utf8();
            match next {
                '#' | '}' | ',' => literal.push(next),
                '(' => {
                    let Some(end) = find_plain_group_end(source, after_next, '(', ')') else {
                        break;
                    };
                    Self::flush(&mut operations, &mut literal);
                    let command = &source[after_next..end];
                    references.insert("*".to_owned());
                    references.extend(
                        Self::parse_depth(command, false, depth + 1)
                            .references
                            .iter()
                            .cloned(),
                    );
                    operations.push(Operation::Shell(command.into()));
                    index = end + 1;
                    continue;
                }
                '{' => {
                    let Some(end) = find_format_end(source, index) else {
                        break;
                    };
                    Self::flush(&mut operations, &mut literal);
                    let replacement =
                        Replacement::parse(&source[after_next..end], &mut references, depth);
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
                    } else {
                        literal.push('#');
                        literal.push(next);
                    }
                }
            }
            index = after_next;
        }
        Self::flush(&mut operations, &mut literal);
        let mut parts = Vec::new();
        let mut offset = 0;
        for part in split_top(source, ',') {
            parts.push(offset..offset + part.len());
            offset += part.len() + 1;
        }
        let bytes = source.len() * 3
            + operations.len() * std::mem::size_of::<Operation>()
            + operations
                .iter()
                .map(|operation| match operation {
                    Operation::Replacement(replacement) => {
                        std::mem::size_of::<Replacement>()
                            + replacement
                                .modifiers
                                .iter()
                                .map(|modifier| {
                                    std::mem::size_of::<Modifier>()
                                        + modifier
                                            .args
                                            .iter()
                                            .map(|argument| {
                                                argument.len() + std::mem::size_of::<String>()
                                            })
                                            .sum::<usize>()
                                })
                                .sum::<usize>()
                    }
                    Operation::Style(_, style) => style.bytes,
                    _ => 0,
                })
                .sum::<usize>()
            + parts.len() * std::mem::size_of::<Range<usize>>()
            + references.iter().map(String::len).sum::<usize>();
        Self {
            operations,
            parts: parts.into(),
            references: references.into_iter().collect(),
            bytes,
        }
    }

    fn flush(operations: &mut Vec<Operation>, literal: &mut String) {
        if !literal.is_empty() {
            operations.push(Operation::Text(std::mem::take(literal).into()));
        }
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
    fn compiled_cache_bounds_and_references_follow_operations() {
        CACHE.with_borrow_mut(|cache| *cache = Cache::default());
        for index in 0..CACHE_ENTRIES * 2 {
            get(&format!("{index}:#{{pane_id}}"));
        }
        CACHE.with_borrow(|cache| {
            assert!(cache.entries.len() <= CACHE_ENTRIES);
            assert!(cache.bytes <= CACHE_BYTES);
        });
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
        get(&oversized);
        CACHE.with_borrow(|cache| assert!(!cache.entries.contains_key(oversized.as_str())));
    }
}
