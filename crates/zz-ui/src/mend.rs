//! Display-only repair of hanging inline markdown markers in streaming text.
//!
//! While an agent streams, an unclosed `**bold`, `*em`, `` `code ``, `~~gone`
//! or `[label](half-url` renders as literal text; when the closer lands the
//! markers vanish and the run restyles, so wrap points move and the tail of the
//! paragraph visibly reflows. [`mend`] appends synthetic closers to the text the
//! renderer parses, so a run styles from the moment content follows its opener
//! and the settle is a no-op instead of a jump. Only the display copy is mended
//! — the transcript keeps the raw text, so a marker that never closes settles
//! honestly with one flip at the end of the turn instead of jittering
//! throughout.
//!
//! Text in, text out: no parser is involved, so the repair holds for the
//! `markdown` crate the renderer runs, or anything else that speaks
//! `CommonMark`.
//!
//! Scope, and why the scan stays cheap: markers cannot hang across a blank line
//! (`CommonMark` leaves an unclosed marker literal at the block boundary), so
//! only the last top-level block is scanned and per-append work is O(tail), not
//! O(message). Locating that block walks the lines of the whole text to track
//! code fences, but that is a per-line prefix test rather than a per-character
//! scan.
//!
//! Deliberate non-repairs: fenced and indented code (verbatim already),
//! intraword `_`/`*` (`2*3` must not flash italic), and an opener with nothing
//! after it yet (`**` alone stays literal until content arrives).
//!
//! Two repairs are not closers. A half-streamed URL is replaced by
//! [`PENDING_LINK_URL`], so the label styles at once and the settling URL cannot
//! collapse the line. And a trailing line of just `-` or `=` under a paragraph
//! is a setext underline to the parser but almost always a list item still
//! streaming, so it gets a zero-width space until the next characters decide —
//! without it the paragraph above flashes into a heading.
//!
//! The scanner is approximate wherever exact `CommonMark` delimiter resolution
//! would cost more than it is worth, on the grounds that any mid-stream
//! misjudgment is corrected by the next append or by the settle. Known quirks:
//! an intraword `**` closes (`2**3` briefly bolds the `3`), and a block that
//! mixes a paragraph with a list without a blank line between them is mended as
//! one inline run.

use std::cmp::Reverse;

/// Destination for a link whose URL is still streaming. The renderer styles it
/// like any other link but must not make it clickable.
pub const PENDING_LINK_URL: &str = "zz:pending-link";

const ZERO_WIDTH_SPACE: char = '\u{200B}';

/// Repair hanging inline markers in `text`, returning `None` when nothing hangs
/// — the overwhelmingly common case, and the only one that allocates nothing.
pub fn mend(text: &str) -> Option<String> {
    let start = tail_start(text)?;
    let mended = close_hanging(&text[start..])?;
    if start == 0 {
        return Some(mended);
    }
    let mut repaired = String::with_capacity(start + mended.len());
    repaired.push_str(&text[..start]);
    repaired.push_str(&mended);
    Some(repaired)
}

/// Byte offset of the last top-level block, or `None` when that block must be
/// left alone: an unterminated fence, or indented code.
fn tail_start(text: &str) -> Option<usize> {
    let mut fence: Option<(char, usize)> = None;
    let mut start = 0;
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        let next = offset + line.len();
        let body = line.trim_end();
        match fence {
            Some((ch, len)) => {
                if let Some((c, run, info)) = fence_marker(body)
                    && c == ch
                    && run >= len
                    && info.is_empty()
                {
                    fence = None;
                    start = next;
                }
            }
            None => {
                if let Some((ch, len, _)) = fence_marker(body) {
                    fence = Some((ch, len));
                } else if body.is_empty() {
                    start = next;
                }
            }
        }
        offset = next;
    }
    if fence.is_some() {
        return None;
    }
    let first = text[start..].lines().next().unwrap_or_default();
    (!first.starts_with("    ") && !first.starts_with('\t')).then_some(start)
}

/// Fence character, run length, and info string of a code-fence line.
fn fence_marker(line: &str) -> Option<(char, usize, &str)> {
    let body = line.trim_start_matches(' ');
    if line.len() - body.len() > 3 {
        return None;
    }
    let ch = body.chars().next()?;
    if ch != '`' && ch != '~' {
        return None;
    }
    let len = body.chars().take_while(|&c| c == ch).count();
    if len < 3 {
        return None;
    }
    // A backtick fence's info string may not hold a backtick, which is what
    // keeps a whole inline code span on one line from opening a fence.
    let info = body[len..].trim();
    (ch != '`' || !info.contains('`')).then_some((ch, len, info))
}

/// One unclosed emphasis-family delimiter run (`*`, `_`, or `~~`).
struct OpenDelim {
    ch: char,
    len: usize,
    /// Char index just past the run: nesting order for closers, and the
    /// content-must-follow guard.
    pos: usize,
}

/// Close whatever hangs in one top-level block.
fn close_hanging(text: &str) -> Option<String> {
    let cs: Vec<(usize, char)> = text.char_indices().collect();
    let n = cs.len();
    let at = |i: usize| cs.get(i).map(|&(_, c)| c);

    let mut delims: Vec<OpenDelim> = Vec::new();
    // Char indices of unmatched `[`.
    let mut brackets: Vec<usize> = Vec::new();
    // Open inline code span: (backtick run length, content char index).
    let mut code: Option<(usize, usize)> = None;
    // Char index of the last substantive character — content that justifies
    // closing an opener, as opposed to whitespace or a bare marker.
    let mut last_content: Option<usize> = None;
    // Char index of the `]` of a `](…` whose URL runs off the end.
    let mut pending_url: Option<usize> = None;

    let mut i = 0;
    while i < n {
        let c = cs[i].1;
        if code.is_none() && c == '\\' {
            if i + 1 < n {
                last_content = Some(i + 1);
            }
            i += 2;
            continue;
        }
        if c == '`' {
            let run = run_len(&cs, i);
            match code {
                Some((open, _)) if run == open => code = None,
                Some(_) => last_content = Some(i + run - 1),
                None => code = Some((run, i + run)),
            }
            i += run;
            continue;
        }
        if code.is_some() {
            last_content = Some(i);
            i += 1;
            continue;
        }
        match c {
            '*' | '_' | '~' => {
                let run = run_len(&cs, i);
                delimiter(&mut delims, &cs, run, i, &mut last_content);
                i += run;
            }
            '[' => {
                brackets.push(i);
                i += 1;
            }
            ']' => {
                if let Some(open) = brackets.pop() {
                    // Emphasis opened inside a completed `[…]` and never closed
                    // there stays literal, as the final parse will decide too.
                    delims.retain(|d| d.pos < open);
                    if at(i + 1) == Some('(') {
                        let mut j = i + 2;
                        let mut depth = 0usize;
                        loop {
                            match at(j) {
                                Some('(') => depth += 1,
                                Some(')') if depth == 0 => break,
                                Some(')') => depth -= 1,
                                Some(_) => {}
                                None => {
                                    pending_url = Some(i);
                                    break;
                                }
                            }
                            j += 1;
                        }
                        if pending_url.is_some() {
                            break;
                        }
                        last_content = Some(j);
                        i = j + 1;
                        continue;
                    }
                }
                last_content = Some(i);
                i += 1;
            }
            c if c.is_whitespace() => i += 1,
            _ => {
                last_content = Some(i);
                i += 1;
            }
        }
    }

    let mut base = text;
    if let Some(close) = pending_url {
        // The URL is still arriving and never renders: drop the partial, keep
        // the label, and point at the sentinel until the real one lands.
        base = &text[..cs[close].0];
    }
    let setext = setext_partial(base);
    // Closers belong to the paragraph, so a setext underline stays below them.
    let head = match setext.then(|| base.rfind('\n')).flatten() {
        Some(nl) => &base[..nl],
        None => base,
    };
    let end = if code.is_some() || pending_url.is_some() {
        head.trim_end().len()
    } else {
        attach_point(head)
    };

    // Closers, innermost first (descending open position). An open `[` splits
    // them naturally: delimiters opened inside the link text close before the
    // `](…)`, ones opened before it close after.
    let mut pending: Vec<(usize, String)> = Vec::new();
    if let Some(close) = pending_url {
        pending.push((close, format!("]({PENDING_LINK_URL})")));
    } else {
        if let Some((ticks, content)) = code
            && last_content.is_some_and(|lc| lc >= content)
        {
            // A span closes on a backtick run of exactly the opening length, so
            // trailing backticks inside the span are part of the closer we
            // append; when they already overrun it, nothing can close the span.
            let trailing = head[..end].chars().rev().take_while(|&c| c == '`').count();
            if trailing < ticks {
                pending.push((content, "`".repeat(ticks - trailing)));
            }
        }
        if let Some(&open) = brackets.last()
            && last_content.is_some_and(|lc| lc > open)
        {
            pending.push((open, format!("]({PENDING_LINK_URL})")));
        }
    }
    for d in &delims {
        if last_content.is_some_and(|lc| lc >= d.pos) {
            pending.push((d.pos, d.ch.to_string().repeat(d.len)));
        }
    }
    pending.sort_by_key(|&(pos, _)| Reverse(pos));
    let closers: String = pending.into_iter().map(|(_, s)| s).collect();

    if closers.is_empty() && !setext {
        return None;
    }
    let mut repaired = String::with_capacity(base.len() + closers.len() + 3);
    repaired.push_str(&base[..end]);
    repaired.push_str(&closers);
    repaired.push_str(&base[end..]);
    if setext {
        repaired.push(ZERO_WIDTH_SPACE);
    }
    Some(repaired)
}

/// Where a closer can attach and still close. A delimiter run is right-flanking
/// only when a non-whitespace character precedes it, so appending onto trailing
/// whitespace — or onto a marker run that is itself preceded by whitespace, which
/// the closer would merge into — leaves the opener hanging anyway.
fn attach_point(head: &str) -> usize {
    let mut end = head.trim_end().len();
    loop {
        let markers = head[..end]
            .chars()
            .rev()
            .take_while(|&c| c == '*' || c == '_' || c == '~')
            .count();
        if markers == 0 {
            return end;
        }
        let before = end - markers;
        if head[..before]
            .chars()
            .next_back()
            .is_some_and(|c| !c.is_whitespace())
        {
            return end;
        }
        end = head[..before].trim_end().len();
    }
}

fn run_len(cs: &[(usize, char)], i: usize) -> usize {
    let c = cs[i].1;
    cs[i..].iter().take_while(|&&(_, x)| x == c).count()
}

/// Match or open one delimiter run. Closes against the innermost same-character
/// openers outwards (partially, for half-streamed closers like `**a*`);
/// delimiters opened after a consumed opener were inside the closed span and
/// stay literal, exactly as the final parse treats them.
fn delimiter(
    delims: &mut Vec<OpenDelim>,
    cs: &[(usize, char)],
    run: usize,
    i: usize,
    last_content: &mut Option<usize>,
) {
    let c = cs[i].1;
    let end = i + run;
    // Strikethrough is `~~` only; longer tilde runs are literal. A run of one
    // may still be the half-streamed first tilde of a closing `~~`, so it
    // reaches the matcher, but it never opens.
    if c == '~' && run > 2 {
        *last_content = Some(end - 1);
        return;
    }
    let prev = i.checked_sub(1).map(|p| cs[p].1);
    let next = cs.get(end).map(|&(_, c)| c);
    let word = |c: Option<char>| c.is_some_and(char::is_alphanumeric);
    // Intraword `_` never delimits; intraword single `*` is treated the same,
    // conservatively — `2*3` must not flash italic.
    if word(prev) && word(next) && (c == '_' || (c == '*' && run == 1)) {
        *last_content = Some(end - 1);
        return;
    }
    let can_close = prev.is_some_and(|c| !c.is_whitespace());
    let can_open = next.is_some_and(|c| !c.is_whitespace());
    let mut rest = run;
    while can_close
        && rest > 0
        && let Some(k) = delims.iter().rposition(|d| d.ch == c)
    {
        let take = rest.min(delims[k].len);
        delims[k].len -= take;
        rest -= take;
        let keep = if delims[k].len == 0 { k } else { k + 1 };
        delims.truncate(keep);
    }
    if rest == 0 {
        return;
    }
    if can_open && (c != '~' || rest == 2) {
        delims.push(OpenDelim {
            ch: c,
            len: rest,
            pos: end,
        });
    } else {
        *last_content = Some(end - 1);
    }
}

/// Last line is only one or two `-` or `=` under a paragraph line — the
/// incomplete-setext ambiguity, where a streaming list item reads as an
/// underline and flashes the paragraph above it into a heading.
fn setext_partial(text: &str) -> bool {
    let Some(nl) = text.rfind('\n') else {
        return false;
    };
    let last = text[nl + 1..].trim_start();
    let underline = |c: char| !last.is_empty() && last.len() <= 2 && last.chars().all(|x| x == c);
    if !(underline('-') || underline('=')) {
        return false;
    }
    // Both ends of the block: an underline only binds to a paragraph, and a
    // paragraph that has already grown a list keeps growing one.
    let head = &text[..nl];
    head.lines().next().is_some_and(paragraph_line)
        && head
            .lines()
            .last()
            .is_some_and(|l| !l.trim().is_empty() && paragraph_line(l))
}

/// A line that starts no block of its own, so it is paragraph text.
fn paragraph_line(line: &str) -> bool {
    let body = line.trim_start();
    if body.starts_with('>') || body.starts_with('#') {
        return false;
    }
    let bullet = body.starts_with("- ") || body.starts_with("* ") || body.starts_with("+ ");
    let ordered = body.find(['.', ')']).is_some_and(|i| {
        i > 0 && body[..i].bytes().all(|b| b.is_ascii_digit()) && body[i + 1..].starts_with(' ')
    });
    !bullet && !ordered
}

#[cfg(test)]
mod tests {
    use super::*;

    const CORPUS: &[&str] = &[
        "Here is **bold** and *em* and `code` and ~~gone~~ text.\n",
        "See [the docs](https://zzmux.sh/docs) for **more**, or `zz ls`.\n",
        "- one **two**\n- three `four`\n\nAfter the *list*.\n",
        "Intro:\n\n```rust\nfn main() { let a = **b; }\n```\n\nOutro **done**.\n",
        "# Title\n\nA ~~struck~~ word and an ![image](https://zzmux.sh/i.png).\n",
        "Steps\n\n1. first\n2. **second**\n\nDone _at last_.\n",
        "A paragraph\n- and a bullet that interrupts it\n- with `code`\n",
        "**outer *inner* rest** and a snake_case_name, 2*3=6.\n",
        "Glob **`*.rs`** and a literal * star, then ~~`a * b`~~.\n",
        "Line one\n\nLine two ends on [a link](https://zzmux.sh/a(b)c)\n",
    ];

    #[track_caller]
    fn mends(input: &str, expected: &str) {
        assert_eq!(mend(input).as_deref(), Some(expected), "{input:?}");
    }

    #[track_caller]
    fn stays(input: &str) {
        assert_eq!(mend(input), None, "{input:?}");
    }

    fn prefixes(doc: &str) -> impl Iterator<Item = &str> {
        (0..=doc.len())
            .filter(|&i| doc.is_char_boundary(i))
            .map(|i| &doc[..i])
    }

    fn keeps_every_char(mended: &str, source: &str) -> bool {
        let mut chars = mended.chars();
        source.chars().all(|c| chars.any(|m| m == c))
    }

    #[test]
    fn complete_text_needs_no_repair() {
        stays("");
        stays("plain words, no markers");
        stays("a **b** and *c* and `d` and ~~e~~");
        stays("[docs](https://zzmux.sh) done");
        for doc in CORPUS {
            stays(doc);
        }
    }

    #[test]
    fn emphasis_closes() {
        mends("**bold", "**bold**");
        mends("some *em", "some *em*");
        mends("a __b", "a __b__");
        mends("a _b", "a _b_");
        mends("***both", "***both***");
        mends("~~gone", "~~gone~~");
    }

    #[test]
    fn half_streamed_closers_complete() {
        mends("**bold*", "**bold**");
        mends("__b_", "__b__");
        mends("~~gone~", "~~gone~~");
    }

    #[test]
    fn nested_closers_come_innermost_first() {
        mends("**a *b", "**a *b***");
        mends("*a **b", "*a **b***");
        mends("_a **b", "_a **b**_");
    }

    #[test]
    fn bare_openers_stay_literal_until_content() {
        stays("**");
        stays("text **");
        stays("text ** ");
        stays("*");
        stays("~~");
        stays("`");
    }

    #[test]
    fn closers_insert_before_trailing_whitespace() {
        mends("**bold ", "**bold** ");
        mends("*em\n", "*em*\n");
    }

    #[test]
    fn isolated_marker_runs_do_not_swallow_the_closer() {
        // A closer merged onto a space-preceded run is not right-flanking, so it
        // has to attach further back or the opener still hangs.
        mends("**outer *", "**outer** *");
        mends("**a **", "**a** **");
        mends("**a * *", "**a** * *");
    }

    #[test]
    fn intraword_markers_and_escapes_stay_literal() {
        stays("2*3 equals 6");
        stays("snake_case_name");
        stays("20~25 degrees");
        stays(r"\*not emphasis");
        stays(r"a \** b");
    }

    #[test]
    fn list_markers_are_not_openers() {
        stays("* item one");
        stays("- a\n* b");
    }

    #[test]
    fn inline_code_closes_and_shields_markers() {
        mends("`code", "`code`");
        mends("call `a ** b", "call `a ** b`");
        // The trailing tick is span content until the run reaches the opening
        // length, so one more tick closes it.
        mends("``a`", "``a``");
        // An overrun trailing run can never equal the opening length again.
        stays("`a``");
        stays("`done` after");
    }

    #[test]
    fn links_mend_to_pending_sentinel() {
        mends("[docs](https://zzmux.sh/lo", "[docs](zz:pending-link)");
        mends("[docs](", "[docs](zz:pending-link)");
        mends("see [do", "see [do](zz:pending-link)");
        mends("![alt](https://zzmux.sh/i.p", "![alt](zz:pending-link)");
        mends("[a](https://zzmux.sh/(y", "[a](zz:pending-link)");
        stays("see [");
        stays("[x] task-like");
        stays("[a](https://zzmux.sh/(y)) done");
    }

    #[test]
    fn emphasis_and_links_nest() {
        mends("[**a", "[**a**](zz:pending-link)");
        mends("**a [b", "**a [b](zz:pending-link)**");
        mends("**a [b](htt", "**a [b](zz:pending-link)**");
        stays("[**a] done");
    }

    #[test]
    fn setext_partials_get_zero_width_space() {
        mends("para\n-", "para\n-\u{200B}");
        mends("para\n--", "para\n--\u{200B}");
        mends("para\n=", "para\n=\u{200B}");
        mends("**b\n-", "**b**\n-\u{200B}");
        stays("para\n---");
        stays("-");
        stays("\n-");
        // Inside a list `-` is the next item, never an underline.
        stays("- one **two**\n-");
        stays("# Title\n-");
    }

    #[test]
    fn fenced_code_is_skipped() {
        stays("```\n**not bold\n");
        stays("```rust\nlet a = *b;\n");
        stays("~~~\n**x\n");
        stays("intro\n\n```\n**x\n");
        stays("```\ncode\n```");
        mends(
            "```\ncode\n```\nthen **bold",
            "```\ncode\n```\nthen **bold**",
        );
        mends(
            "```\ncode\n```\n\nthen **bold",
            "```\ncode\n```\n\nthen **bold**",
        );
        // A whole span on one line is a code span, not a fence.
        mends("```x``` and **b", "```x``` and **b**");
    }

    #[test]
    fn indented_code_is_skipped() {
        stays("intro\n\n    let a = **b;");
        stays("intro\n\n\tlet a = **b;");
    }

    #[test]
    fn only_the_last_block_is_mended() {
        stays("**settled\n\ndone.");
        mends("**settled\n\nnow **bold", "**settled\n\nnow **bold**");
        mends("*a\n \n*b", "*a\n \n*b*");
    }

    /// The repair has to survive the renderer's parser, not merely look closed:
    /// every marker the mend accounts for must be consumed as syntax. Inline
    /// code is its own node, so its content never reaches the plain text.
    #[test]
    fn mended_text_parses_as_markup() {
        for (source, plain) in [
            ("**bold", "bold"),
            ("some *em", "some em"),
            ("a __b", "a b"),
            ("call `a ** b", "call "),
            ("~~gone", "gone"),
            ("**a *b", "a b"),
            ("**outer *inner", "outer inner"),
            ("**bold ", "bold"),
            ("see [do", "see do"),
            ("[docs](https://zzmux.sh/lo", "docs"),
            ("**a [b](htt", "a b"),
            // An opener with nothing after it is deliberately left literal.
            ("**outer *", "outer *"),
        ] {
            let mended = mend(source).unwrap_or_else(|| panic!("{source:?} needs a repair"));
            let ast = markdown::to_mdast(&mended, &markdown::ParseOptions::gfm())
                .unwrap_or_else(|e| panic!("{mended:?}: {e}"));
            let mut text = String::new();
            plain_text(&ast, &mut text);
            assert_eq!(text, plain, "{source:?} -> {mended:?}");
        }
    }

    fn plain_text(node: &markdown::mdast::Node, out: &mut String) {
        if let markdown::mdast::Node::Text(text) = node {
            out.push_str(&text.value);
        }
        for child in node.children().into_iter().flatten() {
            plain_text(child, out);
        }
    }

    #[test]
    fn streaming_prefixes_are_idempotent() {
        for doc in CORPUS {
            for prefix in prefixes(doc) {
                let Some(mended) = mend(prefix) else { continue };
                assert_eq!(
                    mend(&mended),
                    None,
                    "not idempotent: {prefix:?} -> {mended:?}"
                );
            }
        }
    }

    #[test]
    fn streaming_prefixes_keep_their_text() {
        for doc in CORPUS {
            for prefix in prefixes(doc) {
                let Some(mended) = mend(prefix) else { continue };
                assert!(
                    mended.contains(PENDING_LINK_URL) || keeps_every_char(&mended, prefix),
                    "dropped text: {prefix:?} -> {mended:?}"
                );
            }
        }
    }

    #[test]
    fn streaming_prefixes_never_repair_to_themselves() {
        for doc in CORPUS {
            for prefix in prefixes(doc) {
                let mended = mend(prefix);
                assert!(
                    mended.as_deref() != Some(prefix),
                    "no-op repair allocates: {prefix:?}"
                );
            }
        }
    }
}
