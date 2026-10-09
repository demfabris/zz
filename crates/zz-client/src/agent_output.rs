use std::path::Path;

use zz_protocol::PaneId;

pub const OUTPUT_PANE_MAX_BYTES: usize = 256 * 1024;

#[must_use]
pub fn output_pane_args(
    pane: PaneId,
    cwd: &Path,
    command: &str,
    exit_code: Option<i64>,
    output: &str,
) -> Option<Vec<String>> {
    use base64::Engine as _;
    if output.trim().is_empty() {
        return None;
    }
    let mut start = output.len().saturating_sub(OUTPUT_PANE_MAX_BYTES);
    while !output.is_char_boundary(start) {
        start += 1;
    }
    let mut text = format!("$ {command}\n\n");
    if start > 0 {
        text.push_str("[earlier output left out]\n");
    }
    text.push_str(&output[start..]);
    if !text.ends_with('\n') {
        text.push('\n');
    }
    if let Some(code) = exit_code.filter(|code| *code != 0) {
        text.push_str("\n[exit ");
        text.push_str(&code.to_string());
        text.push_str("]\n");
    }
    let title: String = command.chars().take(60).collect();
    Some(vec![
        "-h".to_owned(),
        "-t".to_owned(),
        pane.to_string(),
        "-c".to_owned(),
        cwd.display().to_string(),
        "-T".to_owned(),
        title,
        "sh".to_owned(),
        "-c".to_owned(),
        "printf %s \"$0\" | base64 -d | less -R --tilde +G".to_owned(),
        base64::engine::general_purpose::STANDARD.encode(text),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_commands_output_opens_paged_in_a_split_beside_the_agent() {
        use base64::Engine as _;
        assert!(output_pane_args(PaneId(7), Path::new("/work"), "true", None, "  \n").is_none());

        let args = output_pane_args(
            PaneId(7),
            Path::new("/work"),
            "cargo test",
            Some(101),
            "error: it's `broken` \"here\"",
        )
        .expect("a pane for printed output");
        let (flags, program) = args.split_at(7);
        assert_eq!(flags, ["-h", "-t", "%7", "-c", "/work", "-T", "cargo test"]);
        assert_eq!(
            program[..3],
            [
                "sh",
                "-c",
                "printf %s \"$0\" | base64 -d | less -R --tilde +G"
            ]
        );
        let text = base64::engine::general_purpose::STANDARD
            .decode(&program[3])
            .expect("base64");
        assert_eq!(
            String::from_utf8(text).expect("utf-8"),
            "$ cargo test\n\nerror: it's `broken` \"here\"\n\n[exit 101]\n"
        );

        let long = "é".repeat(OUTPUT_PANE_MAX_BYTES);
        let args = output_pane_args(PaneId(7), Path::new("/work"), "yes", None, &long)
            .expect("a pane for printed output");
        let text = base64::engine::general_purpose::STANDARD
            .decode(&args[10])
            .expect("base64");
        let text = String::from_utf8(text).expect("a cut on a character boundary");
        assert!(text.starts_with("$ yes\n\n[earlier output left out]\n"));
        assert!(text.len() <= OUTPUT_PANE_MAX_BYTES + 64);
    }
}
