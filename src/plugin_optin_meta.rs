//! Opt-in SVGO-named meta plugins (default **off**).
//!
//! Public contracts only — [svgo.dev plugin pages](https://svgo.dev/docs/plugins/)
//! for `removeViewBox`, `removeDimensions`, `removeXMLNS`, `removeXlink`,
//! `removeTitle`. Not a port of SVGO / oxvg / vexy source.
//!
//! These IDs are **not** in the default pipeline. Task 10 wires `--plugin`.
//! Call [`run`] / `run_remove_*` yourself; nothing here registers a pass.
//!
//! Safety:
//! - Never drop `role` (including `role="img"`).
//! - Never delete SMIL frames / animation elements — titles and a few
//!   attributes only.

use crate::ast::{local_name, prefix, Document, Element, Node};
use std::collections::HashMap;

/// XLink namespace URI (SVG 1.1 / `xmlns:xlink`).
const XLINK_NS: &str = "http://www.w3.org/1999/xlink";

/// SVG 2 dropped `href` on these; default `removeXlink` leaves their XLink.
const LEGACY_XLINK_HREF: &[&str] = &["cursor", "filter", "font-face-uri", "glyphRef", "tref"];

/// SVGO IDs implemented here. Not listed in `DEFAULT_PLUGIN_NAMES`.
pub const OPTIN_META_PLUGIN_NAMES: &[&str] = &[
    "removeViewBox",
    "removeDimensions",
    "removeXMLNS",
    "removeXlink",
    "removeTitle",
];

fn apply_doc(doc: &mut Document, f: fn(&mut Node)) {
    for node in &mut doc.nodes {
        f(node);
    }
}

/// Run the named opt-in passes in [`OPTIN_META_PLUGIN_NAMES`] order.
///
/// Only IDs present in `enabled` (case-insensitive) run. An empty list is a
/// no-op — this is never part of the default pipeline.
pub fn run_optin_meta(doc: &mut Document, enabled: &[String]) {
    for name in OPTIN_META_PLUGIN_NAMES {
        if enabled.iter().any(|e| e.eq_ignore_ascii_case(name)) {
            for node in &mut doc.nodes {
                run(name, node);
            }
        }
    }
}

/// Document wrapper for [`run_remove_view_box`].
pub fn remove_view_box(doc: &mut Document) {
    apply_doc(doc, run_remove_view_box);
}

/// Document wrapper for [`run_remove_dimensions`].
pub fn remove_dimensions(doc: &mut Document) {
    apply_doc(doc, run_remove_dimensions);
}

/// Document wrapper for [`run_remove_xmlns`].
pub fn remove_xmlns(doc: &mut Document) {
    apply_doc(doc, run_remove_xmlns);
}

/// Document wrapper for [`run_remove_xlink`].
pub fn remove_xlink(doc: &mut Document) {
    apply_doc(doc, run_remove_xlink);
}

/// Document wrapper for [`run_remove_xlink_with`].
pub fn remove_xlink_with(doc: &mut Document, include_legacy: bool) {
    for node in &mut doc.nodes {
        run_remove_xlink_with(node, include_legacy);
    }
}

/// Document wrapper for [`run_remove_title`].
pub fn remove_title(doc: &mut Document) {
    apply_doc(doc, run_remove_title);
}

/// Dispatch by SVGO plugin id (case-insensitive). Unknown names are a no-op.
pub fn run(name: &str, root: &mut Node) -> bool {
    if name.eq_ignore_ascii_case("removeViewBox") {
        run_remove_view_box(root);
        true
    } else if name.eq_ignore_ascii_case("removeDimensions") {
        run_remove_dimensions(root);
        true
    } else if name.eq_ignore_ascii_case("removeXMLNS") {
        run_remove_xmlns(root);
        true
    } else if name.eq_ignore_ascii_case("removeXlink") {
        run_remove_xlink(root);
        true
    } else if name.eq_ignore_ascii_case("removeTitle") {
        run_remove_title(root);
        true
    } else {
        false
    }
}

/// `removeViewBox` — drop `viewBox` when it is `0 0 width height`.
///
/// [SVGO](https://svgo.dev/docs/plugins/removeViewBox/): “Removes the
/// `viewBox` attribute where it matches the documents width and height.”
/// Hosts: outermost `<svg>`, plus `<pattern>` / `<symbol>`. Nested `<svg>`
/// viewports stay (they are not the document box).
pub fn run_remove_view_box(root: &mut Node) {
    strip_matching_view_box(root, true);
}

/// `removeDimensions` — drop outermost `<svg>` `width` / `height`.
///
/// [SVGO](https://svgo.dev/docs/plugins/removeDimensions/): opposite of
/// `removeViewBox`. If `viewBox` is missing and both dimensions are plain
/// numbers, synthesize `viewBox="0 0 W H"` first.
pub fn run_remove_dimensions(root: &mut Node) {
    if let Some(svg) = outermost_svg_mut(root) {
        apply_remove_dimensions(svg);
    }
}

/// `removeXMLNS` — drop the default `xmlns` on the outermost `<svg>`.
///
/// [SVGO](https://svgo.dev/docs/plugins/removeXMLNS/): HTML-inline only.
/// `xmlns:*` (including `xmlns:xlink`) and `role` stay.
pub fn run_remove_xmlns(root: &mut Node) {
    if let Some(svg) = outermost_svg_mut(root) {
        svg.remove_attr("xmlns");
    }
}

/// `removeXlink` — migrate XLink to SVG 2 (`includeLegacy: false`).
///
/// [SVGO](https://svgo.dev/docs/plugins/removeXlink/): `*:href` → `href`,
/// `*:show` → `target`, `*:title` → `<title>`, drop other XLink attrs and
/// unused XLink xmlns. Legacy hosts (`cursor`, `filter`, `font-face-uri`,
/// `glyphRef`, `tref`) keep XLink. Does not touch `role`.
pub fn run_remove_xlink(root: &mut Node) {
    run_remove_xlink_with(root, false);
}

/// Like [`run_remove_xlink`], with SVGO `includeLegacy`.
pub fn run_remove_xlink_with(root: &mut Node, include_legacy: bool) {
    if let Node::Element(el) = root {
        let mut bound = HashMap::new();
        bound.insert("xlink".to_string(), true);
        migrate_xlink_element(el, &bound, include_legacy);
    }
}

/// `removeTitle` — detach every `<title>`.
///
/// [SVGO](https://svgo.dev/docs/plugins/removeTitle/): a11y trade-off; opt-in.
/// SMIL / graphics siblings stay. `role` stays.
pub fn run_remove_title(root: &mut Node) {
    drop_title_elements(root);
}

fn strip_matching_view_box(node: &mut Node, may_touch_svg: bool) {
    let Node::Element(el) = node else {
        return;
    };
    let local = el.local_name();
    let is_svg = local.eq_ignore_ascii_case("svg");
    let is_pattern_or_symbol =
        local.eq_ignore_ascii_case("pattern") || local.eq_ignore_ascii_case("symbol");
    if is_pattern_or_symbol || (is_svg && may_touch_svg) {
        drop_matching_view_box(el);
    }
    let child_may_svg = may_touch_svg && !is_svg;
    for child in &mut el.children {
        strip_matching_view_box(child, child_may_svg);
    }
}

fn drop_matching_view_box(el: &mut Element) {
    let (vb, width, height) = match (el.attr("viewBox"), el.attr("width"), el.attr("height")) {
        (Some(vb), Some(w), Some(h)) => (vb.to_string(), w.to_string(), h.to_string()),
        _ => return,
    };
    if view_box_matches_dimensions(&vb, &width, &height) {
        el.remove_attr("viewBox");
    }
}

fn view_box_matches_dimensions(view_box: &str, width: &str, height: &str) -> bool {
    let tokens: Vec<&str> = view_box
        .split(|c: char| matches!(c, ' ' | ',' | '\t' | '\n' | '\r'))
        .filter(|t| !t.is_empty())
        .collect();
    if tokens.len() < 4 {
        return false;
    }
    is_zero_token(tokens[0])
        && is_zero_token(tokens[1])
        && dim_eq(width, tokens[2])
        && dim_eq(height, tokens[3])
}

fn is_zero_token(token: &str) -> bool {
    token == "0" || token.parse::<f64>().is_ok_and(|n| n == 0.0)
}

fn dim_eq(attr: &str, token: &str) -> bool {
    let stripped = strip_trailing_px(attr.trim());
    if stripped == token {
        return true;
    }
    match (stripped.parse::<f64>(), token.parse::<f64>()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

fn strip_trailing_px(value: &str) -> &str {
    if value.len() >= 2 {
        let (head, tail) = value.split_at(value.len() - 2);
        if tail.eq_ignore_ascii_case("px") {
            return head.trim_end();
        }
    }
    value
}

fn apply_remove_dimensions(el: &mut Element) {
    if el.attr("viewBox").is_some() {
        el.remove_attr("width");
        el.remove_attr("height");
        return;
    }
    let width = el.attr("width").map(str::to_string);
    let height = el.attr("height").map(str::to_string);
    let (Some(width), Some(height)) = (width, height) else {
        return;
    };
    let (Some(w), Some(h)) = (js_finite_number(&width), js_finite_number(&height)) else {
        return;
    };
    el.set_attr("viewBox", format!("0 0 {w} {h}"));
    el.remove_attr("width");
    el.remove_attr("height");
}

fn js_finite_number(raw: &str) -> Option<f64> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    trimmed.parse::<f64>().ok().filter(|n| n.is_finite())
}

fn outermost_svg_mut(node: &mut Node) -> Option<&mut Element> {
    let Node::Element(el) = node else {
        return None;
    };
    if el.local_name().eq_ignore_ascii_case("svg") {
        return Some(el);
    }
    el.children.iter_mut().find_map(outermost_svg_mut)
}

fn drop_title_elements(node: &mut Node) {
    let Node::Element(el) = node else {
        return;
    };
    el.children.retain(|child| !node_is_title(child));
    for child in &mut el.children {
        drop_title_elements(child);
    }
}

fn node_is_title(node: &Node) -> bool {
    match node {
        Node::Element(el) => el.local_name().eq_ignore_ascii_case("title"),
        _ => false,
    }
}

fn migrate_xlink_element(
    el: &mut Element,
    inherited: &HashMap<String, bool>,
    include_legacy: bool,
) -> bool {
    let mut bound = inherited.clone();
    for (key, value) in &el.attrs {
        if let Some(p) = xmlns_prefix(key) {
            bound.insert(p.to_ascii_lowercase(), value == XLINK_NS);
        }
    }

    let legacy_keep = !include_legacy
        && is_legacy_xlink_host(el.local_name())
        && el
            .attrs
            .iter()
            .any(|(k, _)| is_xlink_named(k, &bound, "href"));
    if !legacy_keep {
        apply_xlink_migrations(el, &bound);
    }

    let mut still_xlink = el.attrs.iter().any(|(k, _)| is_xlink_attr(k, &bound));
    for child in &mut el.children {
        if let Node::Element(child_el) = child {
            still_xlink |= migrate_xlink_element(child_el, &bound, include_legacy);
        }
    }

    if !still_xlink {
        el.retain_attrs(|key, value| match xmlns_prefix(key) {
            Some(_) if value == XLINK_NS => false,
            _ => true,
        });
    }
    still_xlink
}

fn apply_xlink_migrations(el: &mut Element, bound: &HashMap<String, bool>) {
    let mut drop_keys = Vec::new();
    let mut href = None;
    let mut target = None;
    let mut title = None;

    for (key, value) in &el.attrs {
        if !is_xlink_attr(key, bound) {
            continue;
        }
        let local = local_name(key);
        if local.eq_ignore_ascii_case("href") {
            if href.is_none() {
                href = Some(value.clone());
            }
            drop_keys.push(key.clone());
        } else if local.eq_ignore_ascii_case("show") {
            if target.is_none() {
                if let Some(mapped) = map_xlink_show(value) {
                    target = Some(mapped.to_string());
                }
            }
            drop_keys.push(key.clone());
        } else if local.eq_ignore_ascii_case("title") {
            if title.is_none() {
                title = Some(value.clone());
            }
            drop_keys.push(key.clone());
        } else {
            drop_keys.push(key.clone());
        }
    }

    for key in &drop_keys {
        el.remove_attr(key);
    }
    if let Some(value) = href {
        if el.attr("href").is_none() {
            el.set_attr("href", value);
        }
    }
    if let Some(value) = target {
        if el.attr("target").is_none() {
            el.set_attr("target", value);
        }
    }
    if let Some(text) = title {
        if !has_title_child(el) {
            let mut title_el = Element::new("title");
            title_el.children.push(Node::Text(text));
            el.children.insert(0, Node::Element(title_el));
        }
    }
}

fn map_xlink_show(value: &str) -> Option<&'static str> {
    match value.trim() {
        "new" => Some("_blank"),
        "replace" => Some("_self"),
        _ => None,
    }
}

fn is_legacy_xlink_host(local: &str) -> bool {
    LEGACY_XLINK_HREF
        .iter()
        .any(|name| local.eq_ignore_ascii_case(name))
}

fn xmlns_prefix(attr_name: &str) -> Option<&str> {
    let (head, tail) = attr_name.split_once(':')?;
    if head.eq_ignore_ascii_case("xmlns") && !tail.is_empty() {
        Some(tail)
    } else {
        None
    }
}

fn is_xlink_attr(name: &str, bound: &HashMap<String, bool>) -> bool {
    let Some(p) = prefix(name) else {
        return false;
    };
    if p.eq_ignore_ascii_case("xmlns") {
        return false;
    }
    bound.get(&p.to_ascii_lowercase()) == Some(&true)
}

fn is_xlink_named(name: &str, bound: &HashMap<String, bool>, local: &str) -> bool {
    is_xlink_attr(name, bound) && local_name(name).eq_ignore_ascii_case(local)
}

fn has_title_child(el: &Element) -> bool {
    el.children.iter().any(node_is_title)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{Document, Element};
    use crate::parse::parse_svg;
    use crate::serialize::serialize;
    use crate::DEFAULT_PLUGIN_NAMES;

    fn apply(svg: &str, f: fn(&mut Node)) -> Document {
        let mut doc = parse_svg(svg).unwrap();
        for node in &mut doc.nodes {
            f(node);
        }
        doc
    }

    fn apply_named(svg: &str, name: &str) -> Document {
        let mut doc = parse_svg(svg).unwrap();
        for node in &mut doc.nodes {
            run(name, node);
        }
        doc
    }

    fn root(doc: &Document) -> &Element {
        doc.nodes
            .iter()
            .find_map(|n| match n {
                Node::Element(el) if el.local_name().eq_ignore_ascii_case("svg") => Some(el),
                _ => None,
            })
            .expect("root svg")
    }

    fn root_mut(doc: &mut Document) -> &mut Node {
        doc.nodes
            .iter_mut()
            .find(|n| matches!(n, Node::Element(el) if el.local_name().eq_ignore_ascii_case("svg")))
            .expect("root svg")
    }

    fn has_name(el: &Element, name: &str) -> bool {
        if el.local_name().eq_ignore_ascii_case(name) {
            return true;
        }
        el.children.iter().any(|n| match n {
            Node::Element(child) => has_name(child, name),
            _ => false,
        })
    }

    fn find_id<'a>(el: &'a Element, id: &str) -> Option<&'a Element> {
        if el.attr("id") == Some(id) {
            return Some(el);
        }
        el.children.iter().find_map(|n| match n {
            Node::Element(child) => find_id(child, id),
            _ => None,
        })
    }

    fn has_attr_deep(el: &Element, name: &str) -> bool {
        el.attr(name).is_some()
            || el.children.iter().any(|n| match n {
                Node::Element(child) => has_attr_deep(child, name),
                _ => false,
            })
    }

    const ICON: &str = concat!(
        r##"<svg role="img" viewBox="0 0 24 24" width="24" height="24" "##,
        r##"xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink">"##,
        r##"<title>GitHub</title>"##,
        r##"<path d="M12 0h12"/>"##,
        "</svg>",
    );

    const SMIL: &str = concat!(
        r##"<svg role="img" viewBox="0 0 10 10" width="10" height="10" "##,
        r##"xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink">"##,
        r##"<title>Movie</title>"##,
        r##"<path id="frameA" visibility="hidden" d="M0 0h10v10H0z">"##,
        r##"<animate attributeName="visibility" values="hidden;visible;hidden" dur="1s" xlink:href="#frameA"/>"##,
        "</path>",
        r##"<path id="frameB" visibility="hidden" d="M0 0h10v10H0z">"##,
        r##"<set attributeName="visibility" to="visible" begin="1s" dur="1s"/>"##,
        "</path>",
        "</svg>",
    );

    #[test]
    fn plugin_optin_meta_ids_are_opt_in_only() {
        assert_eq!(
            OPTIN_META_PLUGIN_NAMES,
            &[
                "removeViewBox",
                "removeDimensions",
                "removeXMLNS",
                "removeXlink",
                "removeTitle"
            ]
        );
        for id in OPTIN_META_PLUGIN_NAMES {
            assert!(
                !DEFAULT_PLUGIN_NAMES.contains(id),
                "{id} must stay out of the default pipeline"
            );
        }
        // Unused xmlns:xlink is dropped by default removeUnusedNS — that is
        // not removeXlink. A live xlink:href must survive the default set.
        let live_xlink = concat!(
            r##"<svg role="img" viewBox="0 0 24 24" width="24" height="24" "##,
            r##"xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink">"##,
            r##"<title>GitHub</title>"##,
            r##"<defs><path id="p" d="M12 0h12"/></defs>"##,
            r##"<use xlink:href="#p"/>"##,
            "</svg>",
        );
        let out = crate::optimize_str(live_xlink).unwrap().svg;
        assert!(out.contains("viewBox="), "{out}");
        assert!(out.contains("width="), "{out}");
        assert!(out.contains("xmlns="), "{out}");
        assert!(out.contains("xmlns:xlink"), "{out}");
        assert!(out.contains("xlink:href"), "{out}");
        assert!(out.contains("<title>GitHub</title>"), "{out}");
        assert!(out.contains("role=\"img\""), "{out}");
    }

    #[test]
    fn run_remove_view_box_drops_matching_keeps_mismatch_and_nested() {
        let doc = apply(ICON, run_remove_view_box);
        let svg = root(&doc);
        assert!(svg.attr("viewBox").is_none(), "{}", serialize(&doc));
        assert_eq!(svg.attr("width"), Some("24"));
        assert_eq!(svg.attr("role"), Some("img"));

        let mismatch = apply(
            r#"<svg viewBox="0 0 24 24" width="48" height="24"><path d="M0 0"/></svg>"#,
            run_remove_view_box,
        );
        assert_eq!(root(&mismatch).attr("viewBox"), Some("0 0 24 24"));

        let nested = apply(
            concat!(
                r##"<svg viewBox="0 0 24 24" width="24" height="24">"##,
                r##"<svg id="inner" viewBox="0 0 10 10" width="10" height="10"><path d="M0 0"/></svg>"##,
                "</svg>",
            ),
            run_remove_view_box,
        );
        let outer = root(&nested);
        assert!(outer.attr("viewBox").is_none(), "{}", serialize(&nested));
        let inner = find_id(outer, "inner").expect("inner");
        assert_eq!(inner.attr("viewBox"), Some("0 0 10 10"));

        let pattern = apply(
            concat!(
                r##"<svg viewBox="1 2 10 10" width="10" height="10">"##,
                r##"<pattern id="p" viewBox="0 0 8 8" width="8" height="8"/>"##,
                "</svg>",
            ),
            run_remove_view_box,
        );
        let svg = root(&pattern);
        assert_eq!(svg.attr("viewBox"), Some("1 2 10 10"));
        let p = find_id(svg, "p").expect("pattern");
        assert!(p.attr("viewBox").is_none());

        let px = apply(
            r#"<svg viewBox="0 0 24 24" width="24px" height="24px"><path d="M0 0"/></svg>"#,
            run_remove_view_box,
        );
        assert!(root(&px).attr("viewBox").is_none());
    }

    #[test]
    fn run_remove_dimensions_drops_or_synthesizes_view_box() {
        let doc = apply(ICON, run_remove_dimensions);
        let svg = root(&doc);
        assert_eq!(svg.attr("viewBox"), Some("0 0 24 24"));
        assert!(svg.attr("width").is_none());
        assert!(svg.attr("height").is_none());
        assert_eq!(svg.attr("role"), Some("img"));
        assert!(svg.attr("xmlns").is_some());

        let synthesized = apply(
            r#"<svg role="img" width="12" height="8"><path d="M0 0"/></svg>"#,
            run_remove_dimensions,
        );
        let svg = root(&synthesized);
        assert_eq!(svg.attr("viewBox"), Some("0 0 12 8"));
        assert!(svg.attr("width").is_none());
        assert!(svg.attr("height").is_none());
        assert_eq!(svg.attr("role"), Some("img"));

        let px = apply(
            r#"<svg width="12px" height="8px"><path d="M0 0"/></svg>"#,
            run_remove_dimensions,
        );
        let svg = root(&px);
        assert!(svg.attr("viewBox").is_none());
        assert_eq!(svg.attr("width"), Some("12px"));

        let nested = apply(
            concat!(
                r##"<svg viewBox="0 0 24 24" width="24" height="24">"##,
                r##"<svg id="inner" viewBox="0 0 10 10" width="10" height="10"/>"##,
                "</svg>",
            ),
            run_remove_dimensions,
        );
        let outer = root(&nested);
        assert!(outer.attr("width").is_none());
        let inner = find_id(outer, "inner").expect("inner");
        assert_eq!(inner.attr("width"), Some("10"));
        assert_eq!(inner.attr("height"), Some("10"));
    }

    #[test]
    fn run_remove_xmlns_drops_default_ns_only() {
        let doc = apply(ICON, run_remove_xmlns);
        let svg = root(&doc);
        assert!(svg.attr("xmlns").is_none(), "{}", serialize(&doc));
        assert_eq!(
            svg.attr("xmlns:xlink"),
            Some("http://www.w3.org/1999/xlink")
        );
        assert_eq!(svg.attr("role"), Some("img"));
        assert_eq!(svg.attr("viewBox"), Some("0 0 24 24"));
        assert!(has_name(svg, "title"));
    }

    #[test]
    fn run_remove_xlink_migrates_and_keeps_legacy() {
        let use_doc = apply(
            concat!(
                r##"<svg role="img" xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink">"##,
                r##"<defs><g id="charge"><path d="M0 0h2v2H0z"/></g></defs>"##,
                r##"<use xlink:href="#charge" xlink:show="new" xlink:title="Charge"/>"##,
                "</svg>",
            ),
            run_remove_xlink,
        );
        let svg = root(&use_doc);
        assert!(svg.attr("xmlns:xlink").is_none(), "{}", serialize(&use_doc));
        assert_eq!(svg.attr("role"), Some("img"));
        assert_eq!(svg.attr("xmlns"), Some("http://www.w3.org/2000/svg"));
        assert!(!has_attr_deep(svg, "xlink:href"));
        let use_el = svg
            .children
            .iter()
            .find_map(|n| match n {
                Node::Element(el) if el.local_name() == "use" => Some(el),
                _ => None,
            })
            .expect("use");
        assert_eq!(use_el.attr("href"), Some("#charge"));
        assert_eq!(use_el.attr("target"), Some("_blank"));
        assert!(has_name(use_el, "title"));
        assert!(use_el.attr("xlink:title").is_none());
        assert!(use_el.attr("role").is_none());

        let legacy = apply(
            concat!(
                r##"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink">"##,
                r##"<tref xlink:href="#label"/>"##,
                r##"<filter id="blur" xlink:href="#other"/>"##,
                "</svg>",
            ),
            run_remove_xlink,
        );
        let svg = root(&legacy);
        assert_eq!(
            svg.attr("xmlns:xlink"),
            Some("http://www.w3.org/1999/xlink"),
            "{}",
            serialize(&legacy)
        );
        assert!(has_attr_deep(svg, "xlink:href"));

        let forced = apply_named(
            concat!(
                r##"<svg xmlns:xlink="http://www.w3.org/1999/xlink">"##,
                r##"<tref xlink:href="#label"/></svg>"##,
            ),
            "removeXlink",
        );
        // named dispatch uses includeLegacy=false — tref stays
        assert!(has_attr_deep(root(&forced), "xlink:href"));

        let mut include = parse_svg(concat!(
            r##"<svg xmlns:xlink="http://www.w3.org/1999/xlink">"##,
            r##"<tref xlink:href="#label"/></svg>"##,
        ))
        .unwrap();
        run_remove_xlink_with(root_mut(&mut include), true);
        let svg = root(&include);
        assert!(svg.attr("xmlns:xlink").is_none());
        let tref = svg
            .children
            .iter()
            .find_map(|n| match n {
                Node::Element(el) if el.local_name() == "tref" => Some(el),
                _ => None,
            })
            .expect("tref");
        assert_eq!(tref.attr("href"), Some("#label"));
        assert!(tref.attr("xlink:href").is_none());
    }

    #[test]
    fn run_remove_title_drops_titles_only() {
        let doc = apply(ICON, run_remove_title);
        let svg = root(&doc);
        assert!(!has_name(svg, "title"), "{}", serialize(&doc));
        assert_eq!(svg.attr("role"), Some("img"));
        assert_eq!(svg.attr("viewBox"), Some("0 0 24 24"));
        assert!(has_name(svg, "path"));
        assert!(svg.attr("xmlns").is_some());
    }

    #[test]
    fn plugin_optin_meta_keeps_role_img_and_smil_frames() {
        for name in OPTIN_META_PLUGIN_NAMES {
            let doc = apply_named(SMIL, name);
            let svg = root(&doc);
            assert_eq!(svg.attr("role"), Some("img"), "{name}");
            let frame_a = find_id(svg, "frameA").expect("frameA");
            assert_eq!(frame_a.attr("visibility"), Some("hidden"), "{name}");
            assert!(has_name(frame_a, "animate"), "{name} dropped SMIL");
            let frame_b = find_id(svg, "frameB").expect("frameB");
            assert_eq!(frame_b.attr("visibility"), Some("hidden"), "{name}");
            assert!(has_name(frame_b, "set"), "{name} dropped SMIL set");
        }

        let mut doc = parse_svg(SMIL).unwrap();
        for node in &mut doc.nodes {
            run_remove_title(node);
            run_remove_view_box(node);
            run_remove_dimensions(node);
            run_remove_xmlns(node);
            run_remove_xlink(node);
        }
        let svg = root(&doc);
        let out = serialize(&doc);
        assert_eq!(svg.attr("role"), Some("img"), "{out}");
        assert!(!has_name(svg, "title"), "{out}");
        assert!(svg.attr("xmlns").is_none(), "{out}");
        assert!(!has_attr_deep(svg, "xlink:href"), "{out}");
        let frame_a = find_id(svg, "frameA").expect("frameA");
        assert_eq!(frame_a.attr("visibility"), Some("hidden"));
        assert!(has_name(frame_a, "animate"));
        let animate = frame_a
            .children
            .iter()
            .find_map(|n| match n {
                Node::Element(el) if el.local_name() == "animate" => Some(el),
                _ => None,
            })
            .expect("animate");
        assert_eq!(animate.attr("href"), Some("#frameA"));
        assert!(find_id(svg, "frameB").is_some());
    }

    #[test]
    fn run_unknown_name_is_noop() {
        let mut doc = parse_svg(ICON).unwrap();
        let before = serialize(&doc);
        assert!(!run("removeScripts", root_mut(&mut doc)));
        assert_eq!(serialize(&doc), before);
    }

    #[test]
    fn run_optin_meta_empty_is_noop_enabled_runs() {
        let mut idle = parse_svg(ICON).unwrap();
        let before = serialize(&idle);
        run_optin_meta(&mut idle, &[]);
        assert_eq!(serialize(&idle), before);

        let mut on = parse_svg(ICON).unwrap();
        run_optin_meta(&mut on, &["removeTitle".into(), "removeXMLNS".into()]);
        let svg = root(&on);
        assert!(!has_name(svg, "title"));
        assert!(svg.attr("xmlns").is_none());
        assert_eq!(svg.attr("role"), Some("img"));
        assert_eq!(svg.attr("viewBox"), Some("0 0 24 24"));
    }
}
