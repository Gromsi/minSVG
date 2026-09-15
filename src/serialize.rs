use crate::ast::{Document, Element, Node};
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;

/// Elements that are valid as self-closing in SVG/XML serialization.
const VOIDISH: &[&str] = &[
    "area", "base", "br", "circle", "ellipse", "hr", "img", "image", "line", "meta", "path",
    "polygon", "polyline", "rect", "stop", "use",
];

/// SVGO `js2svg` emit knobs. Compact (`pretty: false`) is the default.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SerializeOptions {
    pub pretty: bool,
    /// Spaces per indent level. SVGO default is 4. Ignored unless `pretty`.
    pub indent: u8,
}

impl Default for SerializeOptions {
    fn default() -> Self {
        Self {
            pretty: false,
            indent: 4,
        }
    }
}

/// SVGO `--datauri` / `config.datauri`: `base64`, `enc`, or `unenc`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataUri {
    Base64,
    Enc,
    Unenc,
}

impl DataUri {
    pub fn parse_cli(s: &str) -> Result<Self, String> {
        match s {
            "base64" => Ok(Self::Base64),
            "enc" => Ok(Self::Enc),
            "unenc" => Ok(Self::Unenc),
            _ => Err(
                "option '--datauri' must have one of the following values: 'base64', 'enc' or 'unenc'"
                    .into(),
            ),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Base64 => "base64",
            Self::Enc => "enc",
            Self::Unenc => "unenc",
        }
    }
}

impl std::str::FromStr for DataUri {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse_cli(s)
    }
}

/// Compact serialize (plugin loop / default CLI).
pub fn serialize(doc: &Document) -> String {
    serialize_with(doc, SerializeOptions::default())
}

/// Indented emit (SVGO `--pretty`, default indent 4).
pub fn serialize_pretty(doc: &Document) -> String {
    serialize_with(
        doc,
        SerializeOptions {
            pretty: true,
            indent: 4,
        },
    )
}

pub fn serialize_with(doc: &Document, opts: SerializeOptions) -> String {
    let mut out = String::new();
    for node in &doc.nodes {
        write_node(&mut out, node, &opts, 0, false);
    }
    out
}

/// Wrap optimized SVG as `data:image/svg+xml…` (SVGO `encodeSVGDatauri`).
pub fn encode_svg_datauri(svg: &str, kind: DataUri) -> String {
    match kind {
        DataUri::Base64 => {
            let mut out = String::from("data:image/svg+xml;base64,");
            out.push_str(&B64.encode(svg.as_bytes()));
            out
        }
        DataUri::Enc => {
            let mut out = String::from("data:image/svg+xml,");
            encode_uri_component_into(&mut out, svg);
            out
        }
        DataUri::Unenc => {
            let mut out = String::from("data:image/svg+xml,");
            out.push_str(svg);
            out
        }
    }
}

/// `encodeURIComponent` (RFC 3986 unreserved + `! ' ( ) *`). Hex is uppercase.
fn encode_uri_component_into(out: &mut String, s: &str) {
    for &b in s.as_bytes() {
        match b {
            b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'-'
            | b'_'
            | b'.'
            | b'!'
            | b'~'
            | b'*'
            | b'\''
            | b'('
            | b')' => out.push(b as char),
            _ => {
                out.push('%');
                out.push(hex_digit(b >> 4));
                out.push(hex_digit(b & 0x0f));
            }
        }
    }
}

fn hex_digit(n: u8) -> char {
    b"0123456789ABCDEF"[n as usize] as char
}

fn write_node(
    out: &mut String,
    node: &Node,
    opts: &SerializeOptions,
    depth: usize,
    text_ctx: bool,
) {
    let pretty = opts.pretty && !text_ctx;
    if pretty {
        write_indent(out, opts.indent, depth);
    }
    match node {
        Node::Element(el) => write_element(out, el, opts, depth, text_ctx),
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
    if pretty && !matches!(node, Node::Element(_)) {
        out.push('\n');
    }
}

fn write_element(
    out: &mut String,
    el: &Element,
    opts: &SerializeOptions,
    depth: usize,
    text_ctx: bool,
) {
    out.push('<');
    out.push_str(&el.name);
    // Emit attrs in Vec order. `sortAttrs` (plugin_strip) makes that
    // deterministic for gzip; this writer does not re-sort.
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
        if opts.pretty && !text_ctx {
            out.push('\n');
        }
        return;
    }

    let child_text_ctx = text_ctx || is_text_container(el.local_name());
    out.push('>');
    if opts.pretty && !child_text_ctx {
        out.push('\n');
    }
    if is_text_container(el.local_name()) || text_ctx {
        for child in &el.children {
            write_node(out, child, opts, depth + 1, true);
        }
    } else {
        for child in &el.children {
            if child.is_ignorable_whitespace() {
                continue;
            }
            write_node(out, child, opts, depth + 1, false);
        }
    }
    if opts.pretty && !child_text_ctx {
        write_indent(out, opts.indent, depth);
    }
    out.push_str("</");
    out.push_str(&el.name);
    out.push('>');
    if opts.pretty && !text_ctx {
        out.push('\n');
    }
}

fn write_indent(out: &mut String, indent: u8, depth: usize) {
    let n = indent as usize * depth;
    out.extend(std::iter::repeat(' ').take(n));
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{Document, Element, Node};

    fn sample_doc() -> Document {
        let mut circle = Element::new("circle");
        circle.self_closing = true;
        circle.attrs.push(("cx".into(), "10".into()));
        circle.attrs.push(("cy".into(), "10".into()));
        circle.attrs.push(("r".into(), "5".into()));

        let mut g = Element::new("g");
        g.children.push(Node::Element(circle));

        let mut title = Element::new("title");
        title.children.push(Node::Text("Hi".into()));

        let mut svg = Element::new("svg");
        svg.attrs
            .push(("xmlns".into(), "http://www.w3.org/2000/svg".into()));
        svg.children.push(Node::Element(title));
        svg.children.push(Node::Element(g));

        Document {
            nodes: vec![Node::Element(svg)],
        }
    }

    #[test]
    fn compact_stays_single_line() {
        let out = serialize(&sample_doc());
        assert_eq!(
            out,
            r#"<svg xmlns="http://www.w3.org/2000/svg"><title>Hi</title><g><circle cx="10" cy="10" r="5"/></g></svg>"#
        );
    }

    #[test]
    fn pretty_indents_nested_and_keeps_title_inline() {
        let out = serialize_pretty(&sample_doc());
        assert_eq!(
            out,
            concat!(
                "<svg xmlns=\"http://www.w3.org/2000/svg\">\n",
                "    <title>Hi</title>\n",
                "    <g>\n",
                "        <circle cx=\"10\" cy=\"10\" r=\"5\"/>\n",
                "    </g>\n",
                "</svg>\n",
            )
        );
    }

    #[test]
    fn pretty_indent_two_spaces() {
        let out = serialize_with(
            &sample_doc(),
            SerializeOptions {
                pretty: true,
                indent: 2,
            },
        );
        assert!(out.contains("\n  <title>Hi</title>\n"), "{out}");
        assert!(out.contains("\n    <circle"), "{out}");
        assert!(!out.contains("\n    <title>"), "{out}");
    }

    #[test]
    fn datauri_base64_enc_unenc() {
        let svg = "<svg/>";
        let b64 = encode_svg_datauri(svg, DataUri::Base64);
        assert!(b64.starts_with("data:image/svg+xml;base64,"), "{b64}");
        let payload = b64.strip_prefix("data:image/svg+xml;base64,").unwrap();
        let decoded = String::from_utf8(B64.decode(payload).unwrap()).unwrap();
        assert_eq!(decoded, svg);

        let enc = encode_svg_datauri(svg, DataUri::Enc);
        assert_eq!(enc, "data:image/svg+xml,%3Csvg%2F%3E");

        let unenc = encode_svg_datauri(svg, DataUri::Unenc);
        assert_eq!(unenc, "data:image/svg+xml,<svg/>");
    }

    #[test]
    fn datauri_rejects_unknown_format() {
        let err = DataUri::parse_cli("hex").unwrap_err();
        assert!(err.contains("base64"), "{err}");
        assert!(err.contains("enc"), "{err}");
        assert!(err.contains("unenc"), "{err}");
    }
}
