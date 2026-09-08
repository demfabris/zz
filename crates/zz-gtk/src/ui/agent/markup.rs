use std::fmt::Write as _;

use gtk::glib;
use markdown::{ParseOptions, mdast::Node};

pub(super) fn render(text: &str) -> String {
    let Ok(root) = markdown::to_mdast(text, &ParseOptions::gfm()) else {
        return escape(text);
    };
    let mut output = String::new();
    append(&root, &mut output, 0);
    output.trim_end().to_owned()
}

fn escape(text: &str) -> String {
    glib::markup_escape_text(text).to_string()
}

fn append(node: &Node, output: &mut String, depth: usize) {
    if depth > 64 {
        return;
    }
    match node {
        Node::Text(value) => output.push_str(&escape(&value.value)),
        Node::Html(value) => output.push_str(&escape(&value.value)),
        Node::InlineCode(value) => {
            let _ = write!(output, "<tt>{}</tt>", escape(&value.value));
        }
        Node::Code(value) => {
            let _ = write!(output, "<tt>{}</tt>\n\n", escape(&value.value));
        }
        Node::InlineMath(value) => output.push_str(&escape(&value.value)),
        Node::Math(value) => {
            output.push_str(&escape(&value.value));
            output.push_str("\n\n");
        }
        Node::Strong(_) => wrapped(node, output, "b", depth),
        Node::Emphasis(_) => wrapped(node, output, "i", depth),
        Node::Delete(_) => wrapped(node, output, "s", depth),
        Node::Heading(heading) => {
            let size = if heading.depth <= 2 {
                "large"
            } else {
                "medium"
            };
            let _ = write!(output, "<span size=\"{size}\" weight=\"bold\">");
            children(node, output, depth);
            output.push_str("</span>\n\n");
        }
        Node::Paragraph(_) => {
            children(node, output, depth);
            output.push_str("\n\n");
        }
        Node::Break(_) => output.push('\n'),
        Node::ThematicBreak(_) => output.push_str("────────\n\n"),
        Node::Link(link) => {
            let clickable = ["https://", "http://", "mailto:", "file://"]
                .iter()
                .any(|prefix| link.url.starts_with(prefix));
            if clickable {
                let _ = write!(output, "<a href=\"{}\">", escape(&link.url));
            }
            children(node, output, depth);
            if clickable {
                output.push_str("</a>");
            }
        }
        Node::Image(image) => {
            let _ = write!(output, "[Image: {}]", escape(&image.alt));
        }
        Node::ImageReference(image) => {
            let _ = write!(output, "[Image: {}]", escape(&image.alt));
        }
        Node::List(list) => {
            for (index, item) in list.children.iter().enumerate() {
                if list.ordered {
                    let _ = write!(output, "{}. ", list.start.unwrap_or(1) + index as u32);
                } else {
                    output.push_str("• ");
                }
                if let Node::ListItem(item) = item
                    && let Some(checked) = item.checked
                {
                    output.push_str(if checked { "☑ " } else { "☐ " });
                }
                let mut item_text = String::new();
                append(item, &mut item_text, depth + 1);
                output.push_str(item_text.trim_end());
                output.push('\n');
            }
            output.push('\n');
        }
        Node::Blockquote(_) => {
            output.push_str("<i>❯ ");
            children(node, output, depth);
            output.push_str("</i>\n");
        }
        Node::TableRow(row) => {
            for (index, cell) in row.children.iter().enumerate() {
                if index > 0 {
                    output.push_str("  │  ");
                }
                append(cell, output, depth + 1);
            }
            output.push('\n');
        }
        Node::Definition(_) => {}
        _ => children(node, output, depth),
    }
}

fn children(node: &Node, output: &mut String, depth: usize) {
    if let Some(children) = node.children() {
        for child in children {
            append(child, output, depth + 1);
        }
    }
}

fn wrapped(node: &Node, output: &mut String, tag: &str, depth: usize) {
    let _ = write!(output, "<{tag}>");
    children(node, output, depth);
    let _ = write!(output, "</{tag}>");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_renders_code_headings_and_emphasis_with_safe_text() {
        let rendered =
            render("## Result\n\n**Good** and `a < b`\n\n```rust\nlet value = \"<&>\";\n```\n");
        assert!(rendered.contains("weight=\"bold\">Result"));
        assert!(rendered.contains("<b>Good</b>"));
        assert!(rendered.contains("<tt>a &lt; b</tt>"));
        assert!(rendered.contains("&lt;&amp;&gt;"));
        assert!(gtk::pango::parse_markup(&rendered, '\0').is_ok());
    }

    #[test]
    fn html_and_link_attributes_cannot_inject_markup() {
        let rendered = render(
            "<span size=\"999999\">bad</span>\n\n[site](https://example.com/?a=1&b=2)\n\n[script](javascript:alert)",
        );
        assert!(!rendered.contains("<span size=\"999999\">"));
        assert!(rendered.contains("href=\"https://example.com/?a=1&amp;b=2\""));
        assert!(!rendered.contains("href=\"javascript:"));
    }
}
