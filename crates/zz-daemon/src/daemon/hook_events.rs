use std::sync::LazyLock;

use super::*;

pub(super) static READONLY_SKIP: LazyLock<bool> =
    LazyLock::new(|| std::env::var_os("ZZ_PERF_READONLY_SKIP").is_none_or(|value| value != "0"));

pub(super) static EAGER_FACTS: LazyLock<bool> =
    LazyLock::new(|| std::env::var_os("ZZ_PERF_EAGER_FACTS").is_some_and(|value| value == "1"));

pub(super) fn log_knobs() {
    log::info!(
        target: "zz_daemon::perf",
        "hook knobs: ZZ_PERF_READONLY_SKIP={} ZZ_PERF_EAGER_FACTS={}",
        u8::from(*READONLY_SKIP),
        u8::from(*EAGER_FACTS),
    );
}

pub(super) fn command_is_read_only(command: &str, args: &[RawText]) -> bool {
    *READONLY_SKIP
        && zz_protocol::catalog_command_spec(command).is_some_and(|spec| !spec.mutates(args))
}

pub(super) fn format_facts_unread(command: &str, args: &[RawText]) -> bool {
    if *EAGER_FACTS {
        return false;
    }
    match command {
        "bind-key" | "unbind-key" | "has-session" => true,
        "set-option" | "set-window-option" => zz_protocol::catalog_command_spec(command)
            .and_then(|spec| zz_protocol::parse_tmux_options(spec, args).ok())
            .is_some_and(|parsed| {
                let expanded = parsed
                    .options
                    .contains(&zz_protocol::TmuxOption::Flag("-F"));
                let mut positionals = parsed.positionals.iter();
                let name = positionals.next().map_or("", |name| &**name);
                !name.is_empty()
                    && !name.contains('#')
                    && !"automatic-rename".starts_with(name)
                    && !(expanded && positionals.any(|value| value.contains('#')))
            }),
        "show-options" | "show-window-options" => {
            args.iter().all(|argument| !argument.contains('#'))
        }
        _ => false,
    }
}

pub(super) fn unread_format_facts() -> &'static FormatHookFacts {
    static UNREAD: LazyLock<FormatHookFacts> = LazyLock::new(FormatHookFacts::default);
    &UNREAD
}
