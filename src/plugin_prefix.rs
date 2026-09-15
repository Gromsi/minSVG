//! Opt-in SVGO-shaped `prefixIds` (not in the default pipeline).
//!
//! Public contract from [svgo.dev/docs/plugins/prefixIds](https://svgo.dev/docs/plugins/prefixIds/):
//! prefix `id` / class names and rewrite `url(#id)` plus `#` href refs so inlined
//! sprites do not collide. Default prefix is the source filename (`.` / space →
//! `_`) plus `__`, or the literal `prefix` token when no path is known.
//!
//! Clean-room: we do **not** lowercase `url(#Id)` fragments (only `url(` is
//! matched case-insensitively). `begin`/`end` syncbases use
//! [`rewrite_smil_clock_value`](crate::animation::rewrite_smil_clock_value)
//! so we do not repeat SVGO #848 / #2073 / #2207.

use crate::animation::{collect_smil_sync_ids, rewrite_smil_clock_value};
use crate::ast::{Document, Element, Node};
use crate::config::Config;
use serde_json::Value;
use std::collections::{BTreeSet, HashMap};

/// SVGO plugin id. Stay out of `DEFAULT_PLUGIN_NAMES` — this pass is opt-in.
pub const PREFIX_IDS_NAME: &str = "prefixIds";

/// SVGO `delim` default: inserted between the prefix and the original name.
const DEFAULT_DELIM: &str = "__";

/// Params for [`prefix_ids`] / [`prefix_ids_with`].
///
/// Matches the public [prefixIds](https://svgo.dev/docs/plugins/prefixIds/)
/// object: `prefix`, `delim`, `prefixIds`, `prefixClassNames`.
///
/// `prefix` is the string *before* [`Self::delim`]. `None` uses the filename
/// (when a path is passed to [`prefix_ids_with`]) or `"prefix"`. JSON
/// `prefix: false` disables the pass (empty resolved prefix).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrefixIdsParams {
    pub prefix: Option<String>,
    pub delim: String,
    pub prefix_ids: bool,
    pub prefix_class_names: bool,
}

impl Default for PrefixIdsParams {
    fn default() -> Self {
        Self {
            prefix: None,
            delim: DEFAULT_DELIM.to_string(),
            prefix_ids: true,
            prefix_class_names: true,
        }
    }
}

impl PrefixIdsParams {
    pub fn from_prefix(prefix: impl Into<String>) -> Self {
        Self {
            prefix: Some(prefix.into()),
            ..Self::default()
        }
    }

    /// Parse SVGO `params` JSON (`prefix` / `delim` / `prefixIds` /
    /// `prefixClassNames`). A bare string is `prefix`.
    pub fn from_value(value: &Value) -> Self {
        let mut p = Self::default();
        match value {
            Value::String(s) => {
                p.prefix = Some(s.clone());
                return p;
            }
            Value::Bool(false) => {
                p.prefix = Some(String::new());
                p.delim.clear();
                return p;
            }
            Value::Object(obj) => {
                if matches!(obj.get("prefix"), Some(Value::Bool(false))) {
                    p.prefix = Some(String::new());
                    p.delim.clear();
                } else if matches!(obj.get("prefix"), Some(Value::Bool(true))) {
                    // auto: filename / `"prefix"`
                } else if let Some(s) = obj.get("prefix").and_then(Value::as_str) {
                    p.prefix = Some(s.to_string());
                } else if let Some(n) = obj.get("prefix").and_then(Value::as_number) {
                    p.prefix = Some(n.to_string());
                }
                if !matches!(obj.get("prefix"), Some(Value::Bool(false))) {
                    if let Some(d) = obj.get("delim").and_then(Value::as_str) {
                        p.delim = d.to_string();
                    }
                }
                if let Some(b) = obj.get("prefixIds").and_then(Value::as_bool) {
                    p.prefix_ids = b;
                }
                if let Some(b) = obj.get("prefixClassNames").and_then(Value::as_bool) {
                    p.prefix_class_names = b;
                }
            }
            _ => {}
        }
        p
    }

    pub fn from_config(config: &Config) -> Self {
        config
            .plugin_param(PREFIX_IDS_NAME)
            .map(Self::from_value)
            .unwrap_or_default()
    }
}

/// Prefix ids / refs. No path → `"prefix__…"` when [`PrefixIdsParams::prefix`] is `None`.
pub fn prefix_ids(doc: &mut Document, params: &PrefixIdsParams) {
    prefix_ids_with(doc, params, None);
}

/// Like [`prefix_ids`], with an optional source path for the default prefix.
///
/// `path` may be a file name or a `/` / `\` path; only the last component is
/// used (SVGO `basename`).
pub fn prefix_ids_with(doc: &mut Document, params: &PrefixIdsParams, path: Option<&str>) {
    if !params.prefix_ids && !params.prefix_class_names {
        return;
    }
    let prefix = resolve_prefix(params, path);
    if prefix.is_empty() {
        return;
    }
    doc.walk_elements_mut(&mut |el| apply_element(el, &prefix, params));
}

fn resolve_prefix(params: &PrefixIdsParams, path: Option<&str>) -> String {
    if let Some(p) = params.prefix.as_deref() {
        return format!("{p}{}", params.delim);
    }
    if let Some(path) = path {
        if !path.is_empty() {
            return format!(
                "{}{}",
                escape_identifier_name(&basename(path)),
                params.delim
            );
        }
    }
    format!("prefix{}", params.delim)
}

fn basename(path: &str) -> &str {
    path.rsplit(|c| c == '/' || c == '\\')
        .next()
        .unwrap_or(path)
}

fn escape_identifier_name(s: &str) -> String {
    s.replace('.', "_").replace(' ', "_")
}

fn prefix_body(prefix: &str, body: &str) -> String {
    if body.is_empty() {
        return String::new();
    }
    if body.starts_with(prefix) {
        body.to_string()
    } else {
        let mut out = String::with_capacity(prefix.len() + body.len());
        out.push_str(prefix);
        out.push_str(body);
        out
    }
}

fn apply_element(el: &mut Element, prefix: &str, params: &PrefixIdsParams) {
    if el.local_name() == "style" {
        for child in &mut el.children {
            if let Node::Text(t) = child {
                *t = rewrite_css(t, prefix, params);
            }
        }
    }
    for (key, value) in &mut el.attrs {
        if value.is_empty() {
            continue;
        }
        if params.prefix_ids && is_id_attr(key) {
            *value = prefix_body(prefix, value);
            continue;
        }
        if params.prefix_class_names && is_class_attr(key) {
            *value = prefix_classes(value, prefix);
            continue;
        }
        if params.prefix_ids && is_href_attr(key) {
            if let Some(next) = prefix_href(value, prefix) {
                *value = next;
                continue;
            }
        }
        if params.prefix_ids && is_aria_id_list(key) {
            *value = prefix_token_list(value, prefix);
            continue;
        }
        if params.prefix_ids && is_smil_clock_attr(key) {
            *value = rewrite_clock(value, prefix);
            continue;
        }
        if params.prefix_ids && contains_url_fn(value) {
            *value = rewrite_url_ids(value, prefix);
        }
    }
}

fn is_id_attr(name: &str) -> bool {
    name == "id" || name == "xml:id"
}

fn is_class_attr(name: &str) -> bool {
    name == "class"
}

fn is_href_attr(name: &str) -> bool {
    let local = name.rsplit_once(':').map(|(_, l)| l).unwrap_or(name);
    local.eq_ignore_ascii_case("href")
}

fn is_aria_id_list(name: &str) -> bool {
    matches!(
        name,
        "aria-labelledby"
            | "aria-describedby"
            | "aria-controls"
            | "aria-owns"
            | "aria-flowto"
            | "aria-activedescendant"
    )
}

fn is_smil_clock_attr(name: &str) -> bool {
    name == "begin" || name == "end"
}

fn prefix_classes(value: &str, prefix: &str) -> String {
    value
        .split_ascii_whitespace()
        .map(|name| prefix_body(prefix, name))
        .collect::<Vec<_>>()
        .join(" ")
}

fn prefix_href(value: &str, prefix: &str) -> Option<String> {
    let id = value.strip_prefix('#')?;
    if id.is_empty() {
        return None;
    }
    Some(format!("#{}", prefix_body(prefix, id)))
}

fn prefix_token_list(value: &str, prefix: &str) -> String {
    value
        .split_ascii_whitespace()
        .map(|part| {
            if let Some(id) = part.strip_prefix('#') {
                format!("#{}", prefix_body(prefix, id))
            } else {
                prefix_body(prefix, part)
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn rewrite_clock(value: &str, prefix: &str) -> String {
    let mut ids = BTreeSet::new();
    collect_smil_sync_ids(value, &mut ids);
    if ids.is_empty() {
        return value.to_string();
    }
    let mut map = HashMap::new();
    for id in ids {
        map.insert(id.clone(), prefix_body(prefix, &id));
    }
    rewrite_smil_clock_value(value, &map)
}

fn contains_url_fn(value: &str) -> bool {
    let bytes = value.as_bytes();
    let mut i = 0;
    while i + 4 <= bytes.len() {
        if looks_like_url_fn(bytes, i) {
            return true;
        }
        i += 1;
    }
    false
}

fn looks_like_url_fn(bytes: &[u8], i: usize) -> bool {
    if i + 4 > bytes.len() {
        return false;
    }
    if !bytes[i..i + 4].eq_ignore_ascii_case(b"url(") {
        return false;
    }
    if i > 0 {
        let prev = bytes[i - 1];
        if prev.is_ascii_alphanumeric() || prev == b'_' {
            return false;
        }
    }
    true
}

/// Rewrite `url(#Id)` / `url('#Id')` fragments. The `#Id` spelling is kept.
fn rewrite_url_ids(value: &str, prefix: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = String::with_capacity(value.len() + prefix.len());
    let mut i = 0;
    while i < bytes.len() {
        if looks_like_url_fn(bytes, i) {
            let open = i + 4;
            out.push_str(&value[i..open]);
            match find_url_close(bytes, open) {
                Some(close) => {
                    out.push_str(&rewrite_url_inner(&value[open..close], prefix));
                    out.push(')');
                    i = close + 1;
                }
                None => {
                    out.push_str(&value[open..]);
                    break;
                }
            }
            continue;
        }
        let ch = value[i..].chars().next().unwrap_or('\0');
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

fn find_url_close(bytes: &[u8], start: usize) -> Option<usize> {
    let mut quote = None;
    let mut i = start;
    while i < bytes.len() {
        let b = bytes[i];
        if let Some(q) = quote {
            if b == b'\\' {
                i = i.saturating_add(2);
                continue;
            }
            if b == q {
                quote = None;
            }
            i += 1;
            continue;
        }
        if b == b'"' || b == b'\'' {
            quote = Some(b);
            i += 1;
            continue;
        }
        if b == b')' {
            return Some(i);
        }
        i += 1;
    }
    None
}

fn rewrite_url_inner(inner: &str, prefix: &str) -> String {
    let Some(hash) = inner.find('#') else {
        return inner.to_string();
    };
    let after = &inner[hash + 1..];
    let id_len = after
        .chars()
        .take_while(|c| *c != '\'' && *c != '"' && *c != ')' && !c.is_whitespace())
        .map(|c| c.len_utf8())
        .sum::<usize>();
    if id_len == 0 {
        return inner.to_string();
    }
    let id = &after[..id_len];
    let mut out = String::with_capacity(inner.len() + prefix.len());
    out.push_str(&inner[..hash]);
    out.push('#');
    out.push_str(&prefix_body(prefix, id));
    out.push_str(&after[id_len..]);
    out
}

fn rewrite_css(css: &str, prefix: &str, params: &PrefixIdsParams) -> String {
    let with_urls = if params.prefix_ids {
        rewrite_url_ids(css, prefix)
    } else {
        css.to_string()
    };
    rewrite_css_selectors(&with_urls, prefix, params)
}

/// Prefix `#id` / `.class` in selector context. Hex colors after `:` stay.
fn rewrite_css_selectors(css: &str, prefix: &str, params: &PrefixIdsParams) -> String {
    let bytes = css.as_bytes();
    let mut out = String::with_capacity(css.len() + prefix.len());
    let mut i = 0;
    let mut after_colon = false;
    let mut quote: Option<u8> = None;
    let mut in_comment = false;
    while i < bytes.len() {
        if in_comment {
            if bytes[i] == b'*' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
                out.push_str("*/");
                i += 2;
                in_comment = false;
            } else {
                out.push(bytes[i] as char);
                i += 1;
            }
            continue;
        }
        if let Some(q) = quote {
            out.push(bytes[i] as char);
            if bytes[i] == b'\\' {
                i += 1;
                if i < bytes.len() {
                    out.push(bytes[i] as char);
                    i += 1;
                }
                continue;
            }
            if bytes[i] == q {
                quote = None;
            }
            i += 1;
            continue;
        }
        if bytes[i] == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'*' {
            out.push_str("/*");
            i += 2;
            in_comment = true;
            continue;
        }
        if bytes[i] == b'"' || bytes[i] == b'\'' {
            quote = Some(bytes[i]);
            out.push(bytes[i] as char);
            i += 1;
            continue;
        }
        if bytes[i] == b':' {
            after_colon = true;
            out.push(':');
            i += 1;
            continue;
        }
        if bytes[i] == b';' || bytes[i] == b'{' || bytes[i] == b'}' {
            after_colon = false;
            out.push(bytes[i] as char);
            i += 1;
            continue;
        }
        if params.prefix_ids && !after_colon && bytes[i] == b'#' {
            if let Some((ident, n)) = take_css_ident(&bytes[i + 1..]) {
                out.push('#');
                out.push_str(&prefix_body(prefix, ident));
                i += 1 + n;
                continue;
            }
        }
        if params.prefix_class_names && !after_colon && bytes[i] == b'.' {
            if let Some((ident, n)) = take_css_ident(&bytes[i + 1..]) {
                out.push('.');
                out.push_str(&prefix_body(prefix, ident));
                i += 1 + n;
                continue;
            }
        }
        let ch = css[i..].chars().next().unwrap_or('\0');
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

fn take_css_ident(bytes: &[u8]) -> Option<(&str, usize)> {
    if bytes.is_empty() {
        return None;
    }
    let mut i = 0;
    if bytes[0] == b'-' {
        i = 1;
    }
    if i >= bytes.len() || !(bytes[i].is_ascii_alphabetic() || bytes[i] == b'_') {
        return None;
    }
    i += 1;
    while i < bytes.len()
        && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_' || bytes[i] == b'-')
    {
        i += 1;
    }
    let ident = std::str::from_utf8(&bytes[..i]).ok()?;
    Some((ident, i))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{Document, Element, Node};
    use crate::parse::parse_svg;
    use crate::serialize::serialize;
    use crate::DEFAULT_PLUGIN_NAMES;

    fn el(name: &str, attrs: &[(&str, &str)], kids: Vec<Node>) -> Element {
        let mut e = Element::new(name);
        for (k, v) in attrs {
            e.attrs.push(((*k).to_string(), (*v).to_string()));
        }
        e.children = kids;
        e
    }

    fn run(svg: &str, params: PrefixIdsParams, path: Option<&str>) -> String {
        let mut doc = parse_svg(svg).unwrap();
        prefix_ids_with(&mut doc, &params, path);
        serialize(&doc)
    }

    fn params(prefix: &str) -> PrefixIdsParams {
        PrefixIdsParams::from_prefix(prefix)
    }

    const REF_SVG: &str = concat!(
        r##"<svg xmlns="http://www.w3.org/2000/svg">"##,
        r##"<defs><linearGradient id="g1"><stop offset="0" stop-color="#f00"/></linearGradient></defs>"##,
        r##"<rect fill="url(#g1)" class="ink" width="10" height="10"/>"##,
        "</svg>",
    );

    #[test]
    fn prefix_ids_is_opt_in() {
        assert_eq!(PREFIX_IDS_NAME, "prefixIds");
        assert!(
            !DEFAULT_PLUGIN_NAMES.contains(&PREFIX_IDS_NAME),
            "prefixIds must stay off the default pipeline"
        );
    }

    #[test]
    fn prefixes_id_and_fragment_href() {
        let out = run(
            concat!(
                r##"<svg xmlns="http://www.w3.org/2000/svg">"##,
                r##"<defs><g id="charge"><path d="M0 0h2v2H0z"/></g></defs>"##,
                r##"<use href="#charge"/>"##,
                "</svg>",
            ),
            params("icon"),
            None,
        );
        assert!(out.contains("id=\"icon__charge\""), "{out}");
        assert!(out.contains("href=\"#icon__charge\""), "{out}");
        assert!(!out.contains("id=\"charge\""), "{out}");
        assert!(!out.contains("href=\"#charge\""), "{out}");
    }

    #[test]
    fn prefixes_xlink_href() {
        let out = run(
            concat!(
                r##"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink">"##,
                r##"<path id="p1" d="M0 0h1"/>"##,
                r##"<use xlink:href="#p1"/>"##,
                "</svg>",
            ),
            params("n"),
            None,
        );
        assert!(out.contains("id=\"n__p1\""), "{out}");
        assert!(out.contains("xlink:href=\"#n__p1\""), "{out}");
    }

    #[test]
    fn leaves_external_href_alone() {
        let out = run(
            concat!(
                r##"<svg xmlns="http://www.w3.org/2000/svg">"##,
                r##"<a href="https://example.com#charge"><path id="charge" d="M0 0h1"/></a>"##,
                "</svg>",
            ),
            params("icon"),
            None,
        );
        assert!(out.contains("href=\"https://example.com#charge\""), "{out}");
        assert!(out.contains("id=\"icon__charge\""), "{out}");
    }

    #[test]
    fn prefixes_url_paint_without_lowercasing_fragment() {
        let out = run(
            concat!(
                r##"<svg xmlns="http://www.w3.org/2000/svg">"##,
                r##"<defs><linearGradient id="poolFill"><stop offset="0" stop-color="#fba"/></linearGradient></defs>"##,
                r##"<ellipse fill="url(#poolFill)" stroke="URL(#waterShine)" cx="10" cy="10" rx="8" ry="4"/>"##,
                "</svg>",
            ),
            params("icon"),
            None,
        );
        assert!(out.contains(r#"id="icon__poolFill""#), "{out}");
        assert!(out.contains(r#"fill="url(#icon__poolFill)""#), "{out}");
        assert!(out.contains(r#"stroke="URL(#icon__waterShine)""#), "{out}");
        assert!(!out.contains("url(#poolfill)"), "{out}");
        assert!(!out.contains("url(#icon__poolfill)"), "{out}");
        assert!(!out.contains("url(#poolFill)"), "{out}");
    }

    #[test]
    fn prefixes_quoted_url_and_style_attr() {
        let mut doc = Document {
            nodes: vec![Node::Element(el(
                "svg",
                &[],
                vec![
                    Node::Element(el("radialGradient", &[("id", "ClipMe")], vec![])),
                    Node::Element(el(
                        "rect",
                        &[
                            ("fill", "url('#ClipMe')"),
                            ("style", "stroke:url(\"#ClipMe\");clip-path:url(#ClipMe)"),
                        ],
                        vec![],
                    )),
                ],
            ))],
        };
        prefix_ids(&mut doc, &params("n"));
        let rect = match &doc.nodes[0] {
            Node::Element(svg) => match &svg.children[1] {
                Node::Element(r) => r,
                _ => panic!("rect"),
            },
            _ => panic!("svg"),
        };
        assert_eq!(rect.attr("fill"), Some("url('#n__ClipMe')"));
        assert_eq!(
            rect.attr("style"),
            Some("stroke:url(\"#n__ClipMe\");clip-path:url(#n__ClipMe)")
        );
        assert!(!rect.attr("fill").unwrap().contains("clipme"));
        assert!(!rect.attr("style").unwrap().contains("clipme"));
    }

    #[test]
    fn default_prefix_from_filename() {
        let out = run(
            r##"<svg xmlns="http://www.w3.org/2000/svg"><circle id="orb" class="glow"/></svg>"##,
            PrefixIdsParams::default(),
            Some("icons/my icon.svg"),
        );
        assert!(out.contains(r#"id="my_icon_svg__orb""#), "{out}");
        assert!(out.contains(r#"class="my_icon_svg__glow""#), "{out}");
    }

    #[test]
    fn default_prefix_token_without_path() {
        let out = run(
            r##"<svg xmlns="http://www.w3.org/2000/svg"><g id="a"/></svg>"##,
            PrefixIdsParams::default(),
            None,
        );
        assert!(out.contains(r#"id="prefix__a""#), "{out}");
    }

    #[test]
    fn explicit_prefix_wins_over_filename() {
        let out = run(
            r##"<svg xmlns="http://www.w3.org/2000/svg"><g id="a"/></svg>"##,
            params("logo"),
            Some("heart.svg"),
        );
        assert!(out.contains(r#"id="logo__a""#), "{out}");
        assert!(!out.contains("heart"), "{out}");
    }

    #[test]
    fn prefixes_classes_and_stylesheet_selectors() {
        let out = run(
            concat!(
                r##"<svg xmlns="http://www.w3.org/2000/svg">"##,
                r##"<style>#poolFill,.ocean{fill:url(#waterShine);color:#f00}</style>"##,
                r##"<g id="poolFill" class="ocean land"/>"##,
                "</svg>",
            ),
            params("map"),
            None,
        );
        assert!(out.contains(r#"id="map__poolFill""#), "{out}");
        assert!(out.contains(r#"class="map__ocean map__land""#), "{out}");
        assert!(out.contains("#map__poolFill"), "{out}");
        assert!(out.contains(".map__ocean"), "{out}");
        assert!(out.contains("url(#map__waterShine)"), "{out}");
        assert!(out.contains("#f00"), "{out}");
        assert!(!out.contains("url(#watershine)"), "{out}");
    }

    #[test]
    fn prefixes_xml_id_and_aria() {
        let out = run(
            concat!(
                r##"<svg xmlns="http://www.w3.org/2000/svg">"##,
                r##"<title id="name">Hi</title>"##,
                r##"<g xml:id="layer" aria-labelledby="name"/>"##,
                "</svg>",
            ),
            params("n"),
            None,
        );
        assert!(out.contains(r#"id="n__name""#), "{out}");
        assert!(out.contains(r#"xml:id="n__layer""#), "{out}");
        assert!(out.contains(r#"aria-labelledby="n__name""#), "{out}");
    }

    #[test]
    fn rewrites_smil_syncbase_offsets() {
        let out = run(
            concat!(
                r##"<svg xmlns="http://www.w3.org/2000/svg">"##,
                r##"<circle id="y1" cx="1" cy="1" r="1">"##,
                r##"<animate id="chew" attributeName="r" begin="0s; y1.end+.33s" end="b.end-0.5s" dur="1s"/>"##,
                "</circle></svg>",
            ),
            params("icon"),
            None,
        );
        assert!(out.contains(r#"id="icon__y1""#), "{out}");
        assert!(out.contains(r#"id="icon__chew""#), "{out}");
        assert!(out.contains("icon__y1.end+.33s"), "{out}");
        assert!(out.contains("icon__b.end-0.5s"), "{out}");
        assert!(
            !out.contains("; y1.end+.33s") && !out.contains("begin=\"y1.end"),
            "{out}"
        );
    }

    #[test]
    fn is_idempotent_across_a_second_pass() {
        let input = concat!(
            r##"<svg xmlns="http://www.w3.org/2000/svg">"##,
            r##"<defs><linearGradient id="g1"/></defs>"##,
            r##"<rect fill="url(#g1)" class="ink"/>"##,
            "</svg>",
        );
        let once = run(input, params("icon"), None);
        let mut doc = parse_svg(&once).unwrap();
        prefix_ids(&mut doc, &params("icon"));
        let twice = serialize(&doc);
        assert_eq!(once, twice);
        assert!(once.contains(r#"id="icon__g1""#), "{once}");
        assert!(!once.contains("icon__icon__"), "{once}");
    }

    #[test]
    fn windows_basename_and_dot_escape() {
        let out = run(
            r##"<svg xmlns="http://www.w3.org/2000/svg"><g id="a13"/></svg>"##,
            PrefixIdsParams::default(),
            Some(r"C:\vec\prefixIds.13.svg.txt"),
        );
        assert!(out.contains(r#"id="prefixIds_13_svg_txt__a13""#), "{out}");
    }

    #[test]
    fn custom_delim_and_prefix_false_is_noop() {
        let mut custom = PrefixIdsParams::from_prefix("n");
        custom.delim = "-".into();
        let out = run(
            r##"<svg xmlns="http://www.w3.org/2000/svg"><g id="a" class="b"/></svg>"##,
            custom,
            None,
        );
        assert!(out.contains(r#"id="n-a""#), "{out}");
        assert!(out.contains(r#"class="n-b""#), "{out}");

        let off = PrefixIdsParams::from_value(&serde_json::json!(false));
        let same = run(
            r##"<svg xmlns="http://www.w3.org/2000/svg"><g id="a"/></svg>"##,
            off,
            None,
        );
        assert!(same.contains(r#"id="a""#), "{same}");
        assert!(!same.contains("__"), "{same}");
    }

    #[test]
    fn prefix_ids_flag_skips_ids_keeps_classes() {
        let mut p = PrefixIdsParams::from_prefix("n");
        p.prefix_ids = false;
        let out = run(
            concat!(
                r##"<svg xmlns="http://www.w3.org/2000/svg">"##,
                r##"<style>#g1,.ink{fill:url(#g1)}</style>"##,
                r##"<g id="g1" class="ink"/>"##,
                "</svg>",
            ),
            p,
            None,
        );
        assert!(out.contains(r#"id="g1""#), "{out}");
        assert!(out.contains(r#"class="n__ink""#), "{out}");
        assert!(out.contains("#g1"), "{out}");
        assert!(out.contains(".n__ink"), "{out}");
        assert!(out.contains("url(#g1)"), "{out}");
    }

    #[test]
    fn prefix_class_names_flag_skips_classes_keeps_ids() {
        let mut p = PrefixIdsParams::from_prefix("n");
        p.prefix_class_names = false;
        let out = run(
            concat!(
                r##"<svg xmlns="http://www.w3.org/2000/svg">"##,
                r##"<style>#g1,.ink{color:#f00}</style>"##,
                r##"<g id="g1" class="ink"/>"##,
                "</svg>",
            ),
            p,
            None,
        );
        assert!(out.contains(r#"id="n__g1""#), "{out}");
        assert!(out.contains(r#"class="ink""#), "{out}");
        assert!(out.contains("#n__g1"), "{out}");
        assert!(out.contains(".ink"), "{out}");
        assert!(!out.contains(".n__ink"), "{out}");
    }

    #[test]
    fn from_value_reads_svgo_object() {
        let p = PrefixIdsParams::from_value(&serde_json::json!({
            "prefix": "x",
            "delim": "--",
            "prefixIds": true,
            "prefixClassNames": false
        }));
        assert_eq!(p.prefix.as_deref(), Some("x"));
        assert_eq!(p.delim, "--");
        assert!(p.prefix_ids);
        assert!(!p.prefix_class_names);

        let bare = PrefixIdsParams::from_value(&serde_json::json!("logo"));
        assert_eq!(bare.prefix.as_deref(), Some("logo"));
        assert_eq!(bare.delim, DEFAULT_DELIM);
    }

    #[test]
    fn from_config_and_plugin_params_prefix() {
        let mut cfg = crate::Config::default();
        assert_eq!(
            PrefixIdsParams::from_config(&cfg),
            PrefixIdsParams::default()
        );
        cfg.plugin_params.insert(
            "prefixIds".into(),
            serde_json::json!({ "prefix": "x", "delim": "_", "prefixClassNames": false }),
        );
        let p = PrefixIdsParams::from_config(&cfg);
        assert_eq!(p.prefix.as_deref(), Some("x"));
        assert_eq!(p.delim, "_");
        assert!(!p.prefix_class_names);

        cfg.enable_plugins.push("prefixIds".into());
        let out = crate::optimize_str_with(REF_SVG, &cfg).unwrap();
        assert!(
            out.plugins_run.iter().any(|n| *n == PREFIX_IDS_NAME),
            "{:?}",
            out.plugins_run
        );
        assert!(
            out.svg.contains(r#"id="x_g1""#) || out.svg.contains("x_g1"),
            "{}",
            out.svg
        );
        assert!(out.svg.contains("url(#x_g1)"), "{}", out.svg);
        assert!(
            out.svg.contains(r#"class="ink""#) || out.svg.contains("class=\"ink\""),
            "prefixClassNames false must leave class: {}",
            out.svg
        );
        assert!(!out.svg.contains("x_ink"), "{}", out.svg);
    }

    #[test]
    fn plugin_params_prefix_json_rewrites_ids() {
        let mut cfg = crate::Config::default();
        cfg.enable_plugins.push("prefixIds".into());
        cfg.plugin_params
            .insert("prefixIds".into(), serde_json::json!({ "prefix": "x" }));
        let out = crate::optimize_str_with(REF_SVG, &cfg).unwrap();
        assert!(
            out.plugins_run.contains(&"prefixIds"),
            "{:?}",
            out.plugins_run
        );
        assert!(out.svg.contains(r#"id="x__g1""#), "{}", out.svg);
        assert!(out.svg.contains("url(#x__g1)"), "{}", out.svg);
        assert!(out.svg.contains("x__ink"), "{}", out.svg);

        let off = crate::optimize_str(REF_SVG).unwrap();
        assert!(!off.plugins_run.iter().any(|n| *n == "prefixIds"));
        assert!(off.svg.contains(r#"id="g1""#), "{}", off.svg);
        assert!(!off.svg.contains("x__"), "{}", off.svg);
    }

    #[test]
    fn apply_plugin_cli_param_shape_prefixes() {
        let mut cfg = crate::Config::default();
        crate::apply_plugin_cli(
            &mut cfg,
            &[crate::parse_plugin_spec("prefixIds").unwrap()],
            &[r#"prefixIds={"prefix":"x"}"#.into()],
        )
        .unwrap();
        assert!(cfg.plugin_enabled("prefixIds"));
        let out = crate::optimize_str_with(REF_SVG, &cfg).unwrap();
        assert!(out.svg.contains(r#"id="x__g1""#), "{}", out.svg);
        assert!(out.svg.contains("url(#x__g1)"), "{}", out.svg);
    }
}
