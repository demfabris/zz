use zz_protocol::{InsertStyle, PathKind, ShellKind};

struct SplitPath<'a> {
    anchor: String,
    parts: Vec<&'a str>,
}

impl<'a> SplitPath<'a> {
    fn new(path: &'a str) -> Self {
        if let Some(drive) = drive_letter(path) {
            return Self {
                anchor: format!("{drive}:/"),
                parts: components(&path[2..], &['/', '\\']),
            };
        }
        let anchor = if path.starts_with('/') { "/" } else { "" };
        Self {
            anchor: anchor.to_owned(),
            parts: components(path, &['/']),
        }
    }

    fn windows(&self) -> bool {
        self.anchor.len() == 3
    }

    fn push_rel(&mut self, rel: &'a str) {
        let separators: &[char] = if self.windows() { &['/', '\\'] } else { &['/'] };
        self.parts.extend(components(rel, separators));
    }

    fn absolute(&self) -> String {
        let mut text = self.anchor.clone();
        text.push_str(&self.parts.join("/"));
        if text.is_empty() {
            text.push('.');
        }
        text
    }

    fn relative_to(&self, base: &SplitPath<'_>) -> Option<String> {
        if !self.anchor.eq_ignore_ascii_case(&base.anchor) || !self.parts.starts_with(&base.parts) {
            return None;
        }
        let rest = &self.parts[base.parts.len()..];
        Some(if rest.is_empty() {
            ".".to_owned()
        } else {
            rest.join("/")
        })
    }
}

fn drive_letter(path: &str) -> Option<char> {
    let mut chars = path.chars();
    let letter = chars.next().filter(char::is_ascii_alphabetic)?;
    (chars.next() == Some(':') && matches!(chars.next(), None | Some('/' | '\\'))).then_some(letter)
}

fn components<'a>(path: &'a str, separators: &[char]) -> Vec<&'a str> {
    path.split(separators)
        .filter(|part| !part.is_empty() && *part != ".")
        .collect()
}

#[must_use]
pub fn insert_text(
    root: &str,
    rel: &str,
    kind: PathKind,
    cwd: Option<&str>,
    style: InsertStyle,
    absolute: bool,
) -> Option<String> {
    let mut path = SplitPath::new(root);
    path.push_rel(rel);
    let mut full = path.absolute();
    if full.chars().any(char::is_control) {
        return None;
    }
    let relative = cwd
        .filter(|_| !absolute)
        .and_then(|cwd| path.relative_to(&SplitPath::new(cwd)));
    let is_relative = relative.is_some();
    let mut text = relative.unwrap_or_else(|| full.clone());
    if kind == PathKind::Dir {
        for text in [&mut text, &mut full] {
            if !text.ends_with('/') {
                text.push('/');
            }
        }
    }
    let mut out = match style {
        InsertStyle::Shell(shell) => {
            if is_relative && text.starts_with(['-', '=', '+']) {
                text.insert_str(0, "./");
            }
            quote_shell(shell, &text)
        }
        InsertStyle::Claude => claude_mention(&text, full),
        InsertStyle::Codex => {
            if text.chars().any(char::is_whitespace) && !text.contains('"') {
                format!("\"{text}\"")
            } else {
                text
            }
        }
    };
    out.push(' ');
    Some(out)
}

fn quote_shell(shell: ShellKind, text: &str) -> String {
    match shell {
        ShellKind::Posix => {
            if text.chars().all(posix_bare) {
                text.to_owned()
            } else {
                format!("'{}'", text.replace('\'', r"'\''"))
            }
        }
        ShellKind::Fish => {
            if !text.starts_with('%') && text.chars().all(posix_bare) {
                text.to_owned()
            } else {
                format!("'{}'", text.replace('\\', r"\\").replace('\'', r"\'"))
            }
        }
        ShellKind::Pwsh => {
            if !text.starts_with('@')
                && !pwsh_number_like(text)
                && text
                    .chars()
                    .all(|c| c == '\\' || (c != ',' && posix_bare(c)))
            {
                return text.to_owned();
            }
            let mut quoted = String::with_capacity(text.len() + 2);
            quoted.push('\'');
            for c in text.chars() {
                if matches!(c, '\'' | '\u{2018}' | '\u{2019}' | '\u{201a}' | '\u{201b}') {
                    quoted.push(c);
                }
                quoted.push(c);
            }
            quoted.push('\'');
            quoted
        }
        ShellKind::Nu => {
            if text.starts_with(|c: char| c.is_ascii_alphabetic() || matches!(c, '_' | '.' | '/'))
                && text.chars().all(mention_bare)
            {
                return text.to_owned();
            }
            let mut longest = 0;
            for (index, _) in text.match_indices('\'') {
                let run = text[index + 1..].chars().take_while(|&c| c == '#').count();
                longest = longest.max(run);
            }
            let hashes = "#".repeat(longest + 1);
            format!("r{hashes}'{text}'{hashes}")
        }
    }
}

fn claude_mention(text: &str, full: String) -> String {
    let last_ok = text
        .strip_suffix('/')
        .unwrap_or(text)
        .chars()
        .last()
        .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_');
    if last_ok && text.chars().all(mention_bare) {
        format!("@{text}")
    } else if !text.contains(['"', '#']) {
        format!("@\"{text}\"")
    } else {
        full
    }
}

fn posix_bare(c: char) -> bool {
    c.is_ascii_alphanumeric() || "_./:@%+,=-".contains(c)
}

fn pwsh_number_like(text: &str) -> bool {
    let mut chars = text.chars();
    match chars.next() {
        Some(c) if c.is_ascii_digit() => true,
        Some('.' | '+') => chars.next().is_some_and(|c| c.is_ascii_digit()),
        _ => false,
    }
}

fn mention_bare(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '/' | '-')
}

#[cfg(test)]
mod tests {
    use super::*;

    const POSIX: InsertStyle = InsertStyle::Shell(ShellKind::Posix);
    const FISH: InsertStyle = InsertStyle::Shell(ShellKind::Fish);
    const PWSH: InsertStyle = InsertStyle::Shell(ShellKind::Pwsh);
    const NU: InsertStyle = InsertStyle::Shell(ShellKind::Nu);
    const FILE: PathKind = PathKind::File;
    const DIR: PathKind = PathKind::Dir;

    #[test]
    fn insert_text_table() {
        let cwd = Some("/home/me/src");
        let cases: &[(&str, &str, PathKind, Option<&str>, InsertStyle, bool, &str)] = &[
            (
                "/home/me/src",
                "main.rs",
                FILE,
                cwd,
                POSIX,
                false,
                "main.rs ",
            ),
            (
                "/home/me/src",
                "lib/a.rs",
                FILE,
                cwd,
                POSIX,
                false,
                "lib/a.rs ",
            ),
            ("/home/me/src", "lib", DIR, cwd, POSIX, false, "lib/ "),
            (
                "/home/me/src",
                "lib/a.rs",
                FILE,
                cwd,
                POSIX,
                true,
                "/home/me/src/lib/a.rs ",
            ),
            (
                "/home/me/src",
                "lib",
                DIR,
                cwd,
                POSIX,
                true,
                "/home/me/src/lib/ ",
            ),
            (
                "/home/me/src",
                "a.rs",
                FILE,
                None,
                POSIX,
                false,
                "/home/me/src/a.rs ",
            ),
            (
                "/home/me",
                "notes.md",
                FILE,
                cwd,
                POSIX,
                false,
                "/home/me/notes.md ",
            ),
            ("/home/me", "src", DIR, cwd, POSIX, false, "./ "),
            (
                "/home/me",
                "src",
                DIR,
                cwd,
                InsertStyle::Claude,
                false,
                "@\"./\" ",
            ),
            (
                "/home/me/src",
                "lib",
                DIR,
                cwd,
                InsertStyle::Claude,
                false,
                "@lib/ ",
            ),
            (
                "/home/me/src",
                "build-",
                DIR,
                cwd,
                InsertStyle::Claude,
                false,
                "@\"build-/\" ",
            ),
            (
                "/home/me/src",
                "v1.",
                DIR,
                cwd,
                InsertStyle::Claude,
                false,
                "@\"v1./\" ",
            ),
            ("/", "", DIR, None, InsertStyle::Claude, false, "@\"/\" "),
            ("/home/me/src", "+q", FILE, cwd, POSIX, false, "./+q "),
            (
                "/home/me",
                "src",
                DIR,
                cwd,
                InsertStyle::Codex,
                false,
                "./ ",
            ),
            ("/home/me/src", "", DIR, cwd, POSIX, false, "./ "),
            ("/home/me/src", "", DIR, cwd, POSIX, true, "/home/me/src/ "),
            (
                "/home/me/src",
                "-rf.txt",
                FILE,
                cwd,
                POSIX,
                false,
                "./-rf.txt ",
            ),
            ("/home/me/src", "=foo", FILE, cwd, POSIX, false, "./=foo "),
            (
                "/home/me/src",
                "=foo",
                FILE,
                cwd,
                POSIX,
                true,
                "/home/me/src/=foo ",
            ),
            ("/home/me/src", "-d", DIR, cwd, FISH, false, "./-d/ "),
            (
                "/home/me/src",
                "-rf.txt",
                FILE,
                cwd,
                PWSH,
                false,
                "./-rf.txt ",
            ),
            (
                "/home/me/src",
                "-rf.txt",
                FILE,
                cwd,
                NU,
                false,
                "./-rf.txt ",
            ),
            (
                "/home/me/src",
                "-rf.txt",
                FILE,
                cwd,
                InsertStyle::Claude,
                false,
                "@-rf.txt ",
            ),
            (
                "/home/me/src",
                "-rf.txt",
                FILE,
                cwd,
                InsertStyle::Codex,
                false,
                "-rf.txt ",
            ),
            (
                "/home/me/src",
                "a b.txt",
                FILE,
                cwd,
                POSIX,
                false,
                "'a b.txt' ",
            ),
            (
                "/home/me/src",
                "it's",
                FILE,
                cwd,
                POSIX,
                false,
                r"'it'\''s' ",
            ),
            ("/home/me/src", "~x", FILE, cwd, POSIX, false, "'~x' "),
            ("/home/me/src", "$HOME", FILE, cwd, POSIX, false, "'$HOME' "),
            (
                "/home/me/src",
                "a@b:c%d+e,f=g",
                FILE,
                cwd,
                POSIX,
                false,
                "a@b:c%d+e,f=g ",
            ),
            (
                "/home/me/src",
                r"back\",
                FILE,
                cwd,
                FISH,
                false,
                r"'back\\' ",
            ),
            ("/home/me/src", "it's", FILE, cwd, FISH, false, r"'it\'s' "),
            (
                "/home/me/src",
                "plain.txt",
                FILE,
                cwd,
                FISH,
                false,
                "plain.txt ",
            ),
            ("/home/me/src", "it's", FILE, cwd, PWSH, false, "'it''s' "),
            (
                "/home/me/src",
                "it\u{2019}s",
                FILE,
                cwd,
                PWSH,
                false,
                "'it\u{2019}\u{2019}s' ",
            ),
            ("/home/me/src", "a,b", FILE, cwd, PWSH, false, "'a,b' "),
            ("/home/me/src", "@args", FILE, cwd, PWSH, false, "'@args' "),
            ("/home/me/src", "x@y", FILE, cwd, PWSH, false, "x@y "),
            ("/home/me/src", "1kb", FILE, cwd, PWSH, false, "'1kb' "),
            ("/home/me/src", "0x10", FILE, cwd, PWSH, false, "'0x10' "),
            ("/home/me/src", ".5", FILE, cwd, PWSH, false, "'.5' "),
            ("/home/me/src", "a1", FILE, cwd, PWSH, false, "a1 "),
            ("/home/me/src", "%self", FILE, cwd, FISH, false, "'%self' "),
            ("/home/me/src", "a%b", FILE, cwd, FISH, false, "a%b "),
            ("/home/me/src", "it's", FILE, cwd, NU, false, "r#'it's'# "),
            ("/home/me/src", "a'#b", FILE, cwd, NU, false, "r##'a'#b'## "),
            (
                "/home/me/src",
                "a'##b'#c",
                FILE,
                cwd,
                NU,
                false,
                "r###'a'##b'#c'### ",
            ),
            ("/home/me/src", "a b", FILE, cwd, NU, false, "r#'a b'# "),
            (
                "/home/me/src",
                "2024.md",
                FILE,
                cwd,
                NU,
                false,
                "r#'2024.md'# ",
            ),
            (
                "/home/me/src",
                "lib/a.rs",
                FILE,
                cwd,
                NU,
                false,
                "lib/a.rs ",
            ),
            (
                "/home/me/src",
                "lib/a.rs",
                FILE,
                cwd,
                InsertStyle::Claude,
                false,
                "@lib/a.rs ",
            ),
            (
                "/home/me/src",
                "a b.rs",
                FILE,
                cwd,
                InsertStyle::Claude,
                false,
                "@\"a b.rs\" ",
            ),
            (
                "/home/me/src",
                "trail-",
                FILE,
                cwd,
                InsertStyle::Claude,
                false,
                "@\"trail-\" ",
            ),
            (
                "/home/me/src",
                "is#1.md",
                FILE,
                cwd,
                InsertStyle::Claude,
                false,
                "/home/me/src/is#1.md ",
            ),
            (
                "/home/me/src",
                "q\"t",
                DIR,
                cwd,
                InsertStyle::Claude,
                false,
                "/home/me/src/q\"t/ ",
            ),
            (
                "/home/me/src",
                "a.rs",
                FILE,
                cwd,
                InsertStyle::Claude,
                true,
                "@/home/me/src/a.rs ",
            ),
            (
                "/home/me/src",
                "a b.png",
                FILE,
                cwd,
                InsertStyle::Codex,
                false,
                "\"a b.png\" ",
            ),
            (
                "/home/me/src",
                "shot.png",
                FILE,
                cwd,
                InsertStyle::Codex,
                false,
                "shot.png ",
            ),
            (
                "/home/me/src",
                "a \"b",
                FILE,
                cwd,
                InsertStyle::Codex,
                false,
                "a \"b ",
            ),
            ("/", "etc", DIR, None, POSIX, false, "/etc/ "),
            ("/", "", DIR, None, POSIX, false, "/ "),
            ("/", "", DIR, Some("/"), POSIX, false, "./ "),
            (
                "/home/me/src/",
                "lib//a.rs",
                FILE,
                Some("/home/me/src/"),
                POSIX,
                false,
                "lib/a.rs ",
            ),
            (
                "/home/me/srcx",
                "a.rs",
                FILE,
                cwd,
                POSIX,
                false,
                "/home/me/srcx/a.rs ",
            ),
            (
                "C:/src",
                "app/main.rs",
                FILE,
                None,
                PWSH,
                false,
                "C:/src/app/main.rs ",
            ),
            (
                "C:/src",
                "app/main.rs",
                FILE,
                Some("C:/src"),
                PWSH,
                false,
                "app/main.rs ",
            ),
            ("C:\\src", "app", DIR, Some("c:\\src"), PWSH, false, "app/ "),
            (
                "C:\\src",
                "a b.rs",
                FILE,
                None,
                PWSH,
                false,
                "'C:/src/a b.rs' ",
            ),
            ("C:/", "Users", DIR, None, POSIX, false, "C:/Users/ "),
        ];
        for &(root, rel, kind, cwd, style, absolute, expected) in cases {
            assert_eq!(
                insert_text(root, rel, kind, cwd, style, absolute).as_deref(),
                Some(expected),
                "{root:?} {rel:?} {kind:?} {cwd:?} {style:?} absolute={absolute}"
            );
        }
    }

    #[test]
    fn control_characters_refuse() {
        for style in [
            POSIX,
            FISH,
            PWSH,
            NU,
            InsertStyle::Claude,
            InsertStyle::Codex,
        ] {
            assert_eq!(insert_text("/tmp", "a\nb", FILE, None, style, false), None);
            assert_eq!(
                insert_text("/tmp", "a\u{1b}b", DIR, Some("/tmp"), style, false),
                None
            );
            assert_eq!(
                insert_text("/t\u{7f}", "a", FILE, Some("/t\u{7f}"), style, false),
                None
            );
        }
    }
}
