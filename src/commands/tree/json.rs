//! `tree -J`: the tree as JSON, laid out the way tree 2.x prints it. Each entry is an object
//! with `type` and `name`; directories that list children carry a `contents` array, symlinks a
//! `target`, and `-s` adds `size`. The totals come last as a `report` object.

use super::model::{Class, Counts, Node};

/// Renders the roots as one JSON array. `report` is `None` for `--noreport`.
pub fn render_json(roots: &[Node], with_size: bool, report: Option<(Counts, bool)>) -> String {
    let mut items: Vec<String> = roots
        .iter()
        .map(|root| format!("  {}", entry(root, 2, with_size)))
        .collect();
    let mut out = String::from("[\n");
    match report {
        Some((counts, dirs_only)) => {
            items.push(format!("  {}", report_object(counts, dirs_only)));
            out.push_str(&items.join("\n,\n"));
            out.push('\n');
        }
        None => {
            out.push_str(&items.join("\n,\n"));
            out.push_str("\n\n");
        }
    }
    out.push_str("]\n");
    out
}

fn report_object(counts: Counts, dirs_only: bool) -> String {
    if dirs_only {
        format!("{{\"type\":\"report\",\"directories\":{}}}", counts.dirs)
    } else {
        format!(
            "{{\"type\":\"report\",\"directories\":{},\"files\":{}}}",
            counts.dirs, counts.files
        )
    }
}

/// One entry. `indent` is the column of the opening brace; children sit two columns deeper.
fn entry(node: &Node, indent: usize, with_size: bool) -> String {
    let mut text = format!(
        "{{\"type\":\"{}\",\"name\":{}",
        node.class.json_type(),
        quote(&node.name)
    );
    if with_size {
        text.push_str(&format!(",\"size\":{}", node.stat.size));
    }
    if let Class::Link { target, .. } = &node.class {
        text.push_str(&format!(",\"target\":{}", quote(target)));
    }
    if !node.children.is_empty() {
        let child_indent = indent + 2;
        let pad = " ".repeat(child_indent);
        let children: Vec<String> = node
            .children
            .iter()
            .map(|child| format!("{pad}{}", entry(child, child_indent, with_size)))
            .collect();
        text.push_str(",\"contents\":[\n");
        text.push_str(&children.join(",\n"));
        text.push_str(&format!("\n{}]", " ".repeat(indent)));
    }
    text.push('}');
    text
}

/// A JSON string literal with the mandatory escapes.
fn quote(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::tree::model::Stat;
    use std::path::PathBuf;

    fn node(name: &str, class: Class, children: Vec<Node>) -> Node {
        Node {
            name: name.to_string(),
            path: PathBuf::from(name),
            class,
            stat: Stat::default(),
            children,
            listed: true,
            error: false,
            missing: false,
            over_limit: None,
        }
    }

    #[test]
    fn layout_matches_tree_two() {
        let root = node(
            ".",
            Class::Dir,
            vec![
                node("a", Class::File, Vec::new()),
                node("d", Class::Dir, vec![node("x", Class::File, Vec::new())]),
            ],
        );
        let text = render_json(&[root], false, Some((Counts { dirs: 2, files: 2 }, false)));
        let expected = concat!(
            "[\n",
            "  {\"type\":\"directory\",\"name\":\".\",\"contents\":[\n",
            "    {\"type\":\"file\",\"name\":\"a\"},\n",
            "    {\"type\":\"directory\",\"name\":\"d\",\"contents\":[\n",
            "      {\"type\":\"file\",\"name\":\"x\"}\n",
            "    ]}\n",
            "  ]}\n",
            ",\n",
            "  {\"type\":\"report\",\"directories\":2,\"files\":2}\n",
            "]\n",
        );
        assert_eq!(text, expected);
    }

    #[test]
    fn noreport_leaves_a_blank_line_before_the_bracket() {
        let text = render_json(&[node("x", Class::File, Vec::new())], false, None);
        assert_eq!(text, "[\n  {\"type\":\"file\",\"name\":\"x\"}\n\n]\n");
    }

    #[test]
    fn quotes_special_characters() {
        assert_eq!(quote("a\"b\\c"), "\"a\\\"b\\\\c\"");
        assert_eq!(quote("tab\there"), "\"tab\\there\"");
    }
}
