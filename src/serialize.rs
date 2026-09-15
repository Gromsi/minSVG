use crate::ast::{Document, Element, Node};

/// Elements that are valid as self-closing in SVG/XML serialization.
const VOIDISH: &[&str] = &[
    "area", "base", "br", "circle", "ellipse", "hr", "img", "image", "line", "meta", "path",
    "polygon", "polyline", "rect", "stop", "use",
];

pub fn serialize(doc: &Document) -> String {
    let mut out = String::new();
    for node in &doc.nodes {
        write_node(&mut out, node);
    }
    out
}

fn write_node(out: &mut String, node: &Node) {
    match node {
        Node::Element(el) => write_element(out, el),
        Node::Text(t) => escape_text(out, t),
        Node::Comment(c) => {
            out.push_str("<!--");
            out.push_str(c);
            out.push_str("-->");
        }
        Node::Pi { target, value } => {
            out.push_str("<?");
            out.push_str(target);
            if !value.is_empty() {
                out.push(' ');
                out.push_str(value);
            }
            out.push_str("?>");
        }
        Node::Doctype(d) => {
            out.push_str("<!DOCTYPE ");
            out.push_str(d.trim());
            out.push('>');
        }
        Node::Decl {
            version,
            encoding,
            standalone,
        } => {
            out.push_str("<?xml version=\"");
            out.push_str(version);
            out.push('"');
            if let Some(enc) = encoding {
                out.push_str(" encoding=\"");
                out.push_str(enc);
                out.push('"');
            }
            if let Some(st) = standalone {
                out.push_str(" standalone=\"");
                out.push_str(st);
                out.push('"');
            }
            out.push_str("?>");
        }
    }
}

fn write_element(out: &mut String, el: &Element) {
    out.push('<');
    out.push_str(&el.name);
    for (k, v) in &el.attrs {
        out.push(' ');
        out.push_str(k);
        out.push_str("=\"");
        escape_attr(out, v);
        out.push('"');
    }

    let has_real_children = el.children.iter().any(|c| !c.is_ignorable_whitespace());
    if !has_real_children && (el.self_closing || VOIDISH.contains(&el.local_name())) {
        out.push_str("/>");
        return;
    }

    out.push('>');
    if is_text_container(el.local_name()) {
        for child in &el.children {
            write_node(out, child);
        }
    } else {
        for child in &el.children {
            if child.is_ignorable_whitespace() {
                continue;
            }
            write_node(out, child);
        }
    }
    out.push_str("</");
    out.push_str(&el.name);
    out.push('>');
}

fn is_text_container(name: &str) -> bool {
    matches!(
        name,
        "text" | "tspan" | "textPath" | "title" | "desc" | "style" | "script"
    )
}

fn escape_text(out: &mut String, s: &str) {
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            other => out.push(other),
        }
    }
}

fn escape_attr(out: &mut String, s: &str) {
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            other => out.push(other),
        }
    }
}
