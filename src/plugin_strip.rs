//! Strip / cleanup plugins (SVGO-inspired names, clean-room implementations).
//!
//! Written from public plugin contracts only — not from `plugins.rs` or any
//! reference-engine source. Safety notes come from:
//!
//! - [SVGO preset-default](https://svgo.dev/docs/preset-default/)
//! - [SVGO plugin pages](https://svgo.dev/docs/plugins/) (`removeDoctype`,
//!   `removeXMLProcInst`, `removeComments`, `removeMetadata`,
//!   `removeEditorsNSData`, `cleanupAttrs`, `removeEmptyAttrs`,
//!   `removeEmptyContainers`, `removeUnusedNS`)
//! - [oxvg_optimiser](https://docs.rs/oxvg_optimiser/latest/oxvg_optimiser/)
//!   job summaries (`Remove*` / `CleanupAttrs`)
//! - [Vexy SVGO plugin reference](https://vexy.dev/vexy-svgo/user/plugins/)

use crate::ast::{Document, Element, Node};
use std::collections::HashSet;

/// SVGO / oxvg / vexy names for the strip subset, in preset-default order.
pub const STRIP_PLUGIN_NAMES: &[&str] = &[
    "removeDoctype",
    "removeXMLProcInst",
    "removeComments",
    "removeMetadata",
    "removeEditorsNSData",
    "cleanupAttrs",
    "removeEmptyAttrs",
    "removeDeprecatedAttrs",
    "removeUselessDefs",
    "removeDesc",
    "removeEmptyText",
    "removeHiddenElems",
    "collapseGroups",
    "removeEmptyContainers",
    "removeUnusedNS",
];

/// Run the strip subset. Minify / color / id plugins stay elsewhere.
///
/// `removeUnusedNS` is last on purpose: metadata + editor stripping is what
/// leaves dead `xmlns:*` on the root (high-ROI leftover).
///
/// Structure passes that can hide motion hooks (`removeHiddenElems`,
/// `collapseGroups`, unused-id defs) stay off here — `run_default` gates them.
pub fn run_strip(doc: &mut Document) {
    remove_doctype(doc);
    remove_xml_proc_inst(doc);
    remove_comments(doc);
    remove_metadata(doc);
    remove_editors_ns_data(doc);
    cleanup_attrs(doc);
    remove_empty_attrs(doc);
    remove_deprecated_attrs(doc);
    remove_useless_defs(doc, false);
    remove_desc(doc);
    remove_empty_text(doc);
    remove_empty_containers(doc);
    remove_unused_ns(doc);
}

/// `removeDoctype` — drop the DTD. Safe for SVG clients (SVGO).
pub fn remove_doctype(doc: &mut Document) {
    retain_matching(&mut doc.nodes, |node| !matches!(node, Node::Doctype(_)));
}

/// `removeXMLProcInst` — drop the XML declaration (`<?xml …?>`).
///
/// SVGO: optional in XML 1.0 (SVG's dialect); some tools may then fail to
/// sniff `image/svg+xml`. oxvg: "Removes the xml declaration". Other PIs
/// (e.g. `xml-stylesheet`) are left alone.
pub fn remove_xml_proc_inst(doc: &mut Document) {
    retain_matching(&mut doc.nodes, |node| match node {
        Node::Decl { .. } => false,
        Node::Pi { target, .. } => !target.eq_ignore_ascii_case("xml"),
        _ => true,
    });
}

/// `removeComments` — drop XML comments.
///
/// Default in SVGO / oxvg / vexy **keeps legal comments** whose body starts
/// with `!` (copyright / license / attribution). Removing those can breach
/// CC-BY-style terms. Conditional comments (`<!--[if IE]>`) are not legal
/// comments and are removed; they do not affect SVG rendering.
pub fn remove_comments(doc: &mut Document) {
    retain_matching(&mut doc.nodes, |node| match node {
        Node::Comment(body) => is_legal_comment(body),
        _ => true,
    });
}

/// `removeMetadata` — drop `<metadata>` (any prefix). No render effect.
///
/// Disable later if the file stores CC REL / copyright only in metadata
/// (SVGO docs). Leftover `xmlns:rdf` / `xmlns:cc` are for `removeUnusedNS`.
pub fn remove_metadata(doc: &mut Document) {
    retain_matching(&mut doc.nodes, |node| match node {
        Node::Element(el) => !el.local_name().eq_ignore_ascii_case("metadata"),
        _ => true,
    });
}

/// `removeEditorsNSData` — editor namespaces, elements, and attributes.
///
/// SVGO: safe for redistribution; keep a source copy if you still edit in
/// Inkscape / Illustrator / Figma / Sketch / Serif. Prefix list is the
/// publicly named tools, not a copied URI table.
pub fn remove_editors_ns_data(doc: &mut Document) {
    retain_matching(&mut doc.nodes, |node| match node {
        Node::Element(el) => !element_is_editor_chrome(el),
        _ => true,
    });
    doc.walk_elements_mut(&mut |el| {
        el.retain_attrs(|name, _| !attr_is_editor_chrome(name));
    });
}

/// `cleanupAttrs` — squeeze redundant whitespace in **values** only.
///
/// Does not rename keys and does not drop an attribute if the value becomes
/// empty (SVGO). `removeEmptyAttrs` is the follow-on pass.
pub fn cleanup_attrs(doc: &mut Document) {
    doc.walk_elements_mut(&mut |el| {
        for (_, value) in &mut el.attrs {
            *value = squeeze_attr_value(value);
        }
    });
}

/// `removeEmptyAttrs` — drop attributes whose value is empty.
///
/// SVGO keeps the SVG conditional-processing trio (`requiredFeatures`,
/// `requiredExtensions`, `systemLanguage`) even when empty — empty means
/// "no restriction", which is not the same as omitting the attribute on
/// every client.
pub fn remove_empty_attrs(doc: &mut Document) {
    doc.walk_elements_mut(&mut |el| {
        el.retain_attrs(|name, value| {
            if value.is_empty() && !keep_empty_conditional(name) {
                return false;
            }
            true
        });
    });
}

/// `removeEmptyContainers` — drop empty structural containers, never `<svg>`.
///
/// SVG 2 container set: `a`, `clipPath`, `defs`, `g`, `marker`, `mask`,
/// `pattern`, `svg`, `switch`, `symbol`, `unknown`. Also drop empty
/// `linearGradient` / `radialGradient` / `title` / `desc` / `missing-glyph`
/// when they have no functional children.
///
/// Keep a container whose `id` is referenced (`href` / `xlink:href` / `url(#)`).
/// oxvg: should not change rendering; whitespace-only children do not count.
pub fn remove_empty_containers(doc: &mut Document) {
    remove_empty_containers_with_keep(doc, false);
}

/// When IDs are motion hooks (SMIL / GSAP extras), keep empty marked groups.
pub fn remove_empty_containers_keep_ids(doc: &mut Document) {
    remove_empty_containers_with_keep(doc, true);
}

fn remove_empty_containers_with_keep(doc: &mut Document, keep_ids: bool) {
    let mut refs = HashSet::new();
    collect_hash_refs(&doc.nodes, &mut refs);
    if keep_ids {
        collect_element_ids(&doc.nodes, &mut refs);
    }
    drop_empty_containers(&mut doc.nodes, &refs);
}

/// `removeUnusedNS` — drop `xmlns:prefix` that no qualified name uses.
///
/// SVGO currently only inspects the outermost `<svg>` and says so as a
/// limitation. We scan the **whole tree** for uses, then drop unused
/// declarations on **every** element. Default `xmlns` is never removed.
/// oxvg: "should never visually change the document".
pub fn remove_unused_ns(doc: &mut Document) {
    let mut live = HashSet::new();
    collect_live_prefixes(&doc.nodes, &mut live);
    doc.walk_elements_mut(&mut |el| {
        el.retain_attrs(|name, _| match xmlns_prefix(name) {
            Some(prefix) => live.contains(&prefix.to_ascii_lowercase()),
            None => true,
        });
    });
}

/// `removeDeprecatedAttrs` (`removeAny: false`) plus a few ignored defaults.
///
/// Drops safe-deprecated presentation leftovers (`clip`, `kerning`,
/// `color-profile`) and `enable-background` when the document has no
/// `<filter>`. Root `version` is ignored by every modern SVG client — SVGO
/// `removeUnknownsAndDefaults` drops it; we do the same without the rest of
/// that plugin.
pub fn remove_deprecated_attrs(doc: &mut Document) {
    let has_filter = document_has_local_name(&doc.nodes, "filter");
    doc.walk_elements_mut(&mut |el| {
        if el.local_name() == "svg" {
            el.remove_attr("version");
        }
        el.retain_attrs(|name, value| keep_deprecated_attr(name, value, has_filter));
    });
}

/// `removeUselessDefs` — drop unreferenced paint servers / clip / filter / etc.
///
/// Public SVGO contract: children of `<defs>` that cannot be referenced (no
/// `id`, or an `id` nothing points at) go away. `<style>` / `<script>` stay.
/// oxvg also keeps a node with `class` (CSS target). We do the same.
///
/// `keep_unreferenced_ids`: motion docs skip unused-id deletion (JS / SMIL
/// hooks). Id-less junk still drops.
pub fn remove_useless_defs(doc: &mut Document, keep_unreferenced_ids: bool) {
    let mut refs = HashSet::new();
    collect_iri_refs(&doc.nodes, &mut refs);
    for node in &mut doc.nodes {
        strip_useless_defs_in(node, &refs, keep_unreferenced_ids);
    }
}

/// `removeDesc` at `removeAny: false` — empty or editor-attribution only.
pub fn remove_desc(doc: &mut Document) {
    retain_matching(&mut doc.nodes, |node| match node {
        Node::Element(el) if el.local_name().eq_ignore_ascii_case("desc") => {
            !desc_is_disposable(el)
        }
        _ => true,
    });
}

/// `removeEmptyText` — empty `<text>` / `<tspan>`, dangling `<tref>`.
pub fn remove_empty_text(doc: &mut Document) {
    retain_matching(&mut doc.nodes, |node| match node {
        Node::Element(el) => !is_empty_text_elem(el),
        _ => true,
    });
}

/// `removeHiddenElems` subset: `display="none"` / `visibility="hidden|collapse"`
/// as attributes or in `style=""`. Does **not** delete `opacity:0` (the map
/// `.circle` class uses that) and is skipped on motion in `run_default`.
pub fn remove_hidden_elems(doc: &mut Document) {
    let mut refs = HashSet::new();
    collect_iri_refs(&doc.nodes, &mut refs);
    drop_hidden_elems(&mut doc.nodes, &refs);
}

/// `collapseGroups` subset: unwrap `<g>` that has **no attributes**.
///
/// No filter / mask / clip-path / transform movers — those need rasters.
/// After `cleanupIds` drops unused ids, id-only country wrappers become
/// attrless and flatten. Skipped on motion in `run_default`.
pub fn collapse_groups(doc: &mut Document) {
    collapse_attrless_groups(&mut doc.nodes);
}

// ---------------------------------------------------------------------------
// Internals
// ---------------------------------------------------------------------------

fn is_legal_comment(body: &str) -> bool {
    body.trim_start().starts_with('!')
}

fn squeeze_attr_value(raw: &str) -> String {
    raw.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn keep_empty_conditional(name: &str) -> bool {
    matches!(
        name,
        "requiredFeatures" | "requiredExtensions" | "systemLanguage"
    )
}

fn element_is_editor_chrome(el: &Element) -> bool {
    if el.local_name().eq_ignore_ascii_case("namedview") {
        return true;
    }
    el.prefix().is_some_and(prefix_is_editor)
}

fn attr_is_editor_chrome(name: &str) -> bool {
    let folded = name.to_ascii_lowercase();
    if folded.starts_with("data-figma") {
        return true;
    }
    if let Some(declared) = xmlns_prefix(name) {
        return prefix_is_editor(declared);
    }
    match name.split_once(':') {
        Some((prefix, _)) => prefix_is_editor(prefix),
        None => false,
    }
}

/// Publicly named editor prefixes (SVGO `removeEditorsNSData` docs).
fn prefix_is_editor(prefix: &str) -> bool {
    matches!(
        prefix.to_ascii_lowercase().as_str(),
        "inkscape" | "sodipodi" | "sketch" | "serif" | "illustrator" | "i" | "figma" | "adobe"
    )
}

fn xmlns_prefix(attr_name: &str) -> Option<&str> {
    let (head, tail) = attr_name.split_once(':')?;
    if head.eq_ignore_ascii_case("xmlns") && !tail.is_empty() {
        Some(tail)
    } else {
        None
    }
}

fn retain_matching<F>(nodes: &mut Vec<Node>, keep: F)
where
    F: Fn(&Node) -> bool + Copy,
{
    nodes.retain(keep);
    for node in nodes.iter_mut() {
        if let Node::Element(el) = node {
            retain_matching(&mut el.children, keep);
        }
    }
}

/// SVG 2 containers plus common empty-after-cleanup paint / a11y wrappers.
/// `<svg>` is never in this set and is never dropped.
fn is_droppable_container(local: &str) -> bool {
    matches!(
        local,
        "a" | "clipPath"
            | "defs"
            | "g"
            | "marker"
            | "mask"
            | "pattern"
            | "switch"
            | "symbol"
            | "unknown"
            | "linearGradient"
            | "radialGradient"
            | "title"
            | "desc"
            | "missing-glyph"
    )
}

fn drop_empty_containers(nodes: &mut Vec<Node>, refs: &HashSet<String>) {
    for node in nodes.iter_mut() {
        if let Node::Element(el) = node {
            drop_empty_containers(&mut el.children, refs);
        }
    }
    nodes.retain(|node| !empty_container_may_drop(node, refs));
}

fn empty_container_may_drop(node: &Node, refs: &HashSet<String>) -> bool {
    let Node::Element(el) = node else {
        return false;
    };
    if !is_droppable_container(el.local_name()) {
        return false;
    }
    if let Some(id) = el.attr("id") {
        if refs.contains(id) {
            return false;
        }
    }
    !has_keepworthy_child(el)
}

fn has_keepworthy_child(el: &Element) -> bool {
    el.children.iter().any(|child| match child {
        Node::Element(_) => true,
        Node::Text(text) => !text.chars().all(char::is_whitespace),
        // Preserved legal comments must not vanish with their wrapper.
        Node::Comment(_) | Node::Pi { .. } => true,
        Node::Doctype(_) | Node::Decl { .. } => false,
    })
}

fn collect_element_ids(nodes: &[Node], into: &mut HashSet<String>) {
    for node in nodes {
        let Node::Element(el) = node else {
            continue;
        };
        if let Some(id) = el.attr("id") {
            into.insert(id.to_string());
        }
        collect_element_ids(&el.children, into);
    }
}

fn collect_hash_refs(nodes: &[Node], into: &mut HashSet<String>) {
    for node in nodes {
        let Node::Element(el) = node else {
            continue;
        };
        for (key, value) in &el.attrs {
            take_pointer_ids(key, value, into);
        }
        collect_hash_refs(&el.children, into);
    }
}

fn take_pointer_ids(key: &str, value: &str, into: &mut HashSet<String>) {
    let folded = key.to_ascii_lowercase();
    if folded == "href" || folded.ends_with(":href") {
        if let Some(id) = value.strip_prefix('#') {
            if !id.is_empty() {
                into.insert(id.to_string());
            }
        }
    }
    pull_url_fragment_ids(value, into);
}

fn pull_url_fragment_ids(value: &str, into: &mut HashSet<String>) {
    let folded = value.to_ascii_lowercase();
    let mut cursor = 0;
    while let Some(rel) = folded[cursor..].find("url(") {
        let open = cursor + rel + 4;
        let tail = value.get(open..).unwrap_or("");
        let trimmed = tail.trim_start().trim_start_matches(['\'', '"']);
        if let Some(rest) = trimmed.strip_prefix('#') {
            let id: String = rest
                .chars()
                .take_while(|c| *c != ')' && *c != '\'' && *c != '"' && !c.is_whitespace())
                .collect();
            if !id.is_empty() {
                into.insert(id);
            }
        }
        cursor = open.saturating_add(1);
        if cursor >= value.len() {
            break;
        }
    }
}

fn collect_live_prefixes(nodes: &[Node], into: &mut HashSet<String>) {
    for node in nodes {
        let Node::Element(el) = node else {
            continue;
        };
        if let Some(prefix) = el.prefix() {
            into.insert(prefix.to_ascii_lowercase());
        }
        for (key, _) in &el.attrs {
            if xmlns_prefix(key).is_some() {
                continue;
            }
            if let Some((prefix, _)) = key.split_once(':') {
                into.insert(prefix.to_ascii_lowercase());
            }
        }
        collect_live_prefixes(&el.children, into);
    }
}

fn keep_deprecated_attr(name: &str, value: &str, has_filter: bool) -> bool {
    let n = name.to_ascii_lowercase();
    if n == "enable-background" {
        if !has_filter {
            return false;
        }
        if value.trim().eq_ignore_ascii_case("accumulate") {
            return false;
        }
    }
    !matches!(
        n.as_str(),
        "clip" | "color-profile" | "kerning" | "glyph-orientation-vertical"
    )
}

fn document_has_local_name(nodes: &[Node], name: &str) -> bool {
    for node in nodes {
        let Node::Element(el) = node else {
            continue;
        };
        if el.local_name().eq_ignore_ascii_case(name) {
            return true;
        }
        if document_has_local_name(&el.children, name) {
            return true;
        }
    }
    false
}

fn collect_iri_refs(nodes: &[Node], into: &mut HashSet<String>) {
    collect_hash_refs(nodes, into);
    for node in nodes {
        let Node::Element(el) = node else {
            continue;
        };
        if el.local_name() == "style" {
            for child in &el.children {
                if let Node::Text(t) = child {
                    pull_url_fragment_ids(t, into);
                    collect_css_hash_ids(t, into);
                }
            }
        }
        collect_iri_refs(&el.children, into);
    }
}

fn collect_css_hash_ids(css: &str, into: &mut HashSet<String>) {
    let mut chars = css.chars().peekable();
    let mut after_colon = false;
    while let Some(ch) = chars.next() {
        if ch == '"' || ch == '\'' {
            let q = ch;
            while let Some(c) = chars.next() {
                if c == '\\' {
                    chars.next();
                } else if c == q {
                    break;
                }
            }
            continue;
        }
        if ch == ':' {
            after_colon = true;
            continue;
        }
        if ch == ';' || ch == '{' || ch == '}' {
            after_colon = false;
            continue;
        }
        if ch == '#' && !after_colon {
            let mut id = String::new();
            while let Some(&c) = chars.peek() {
                if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                    id.push(c);
                    chars.next();
                } else {
                    break;
                }
            }
            if !id.is_empty() {
                into.insert(id);
            }
        }
    }
}

fn strip_useless_defs_in(node: &mut Node, refs: &HashSet<String>, keep_ids: bool) {
    let Node::Element(el) = node else {
        return;
    };
    for child in &mut el.children {
        strip_useless_defs_in(child, refs, keep_ids);
    }
    if el.local_name() != "defs" {
        return;
    }
    let useful = collect_useful_def_nodes(el, refs, keep_ids);
    el.children = useful;
}

fn collect_useful_def_nodes(el: &Element, refs: &HashSet<String>, keep_ids: bool) -> Vec<Node> {
    let mut useful = Vec::new();
    for child in &el.children {
        match child {
            Node::Element(c) => {
                if c.local_name() == "style" || c.local_name() == "script" {
                    useful.push(child.clone());
                    continue;
                }
                if c.attr("class").is_some() {
                    useful.push(child.clone());
                    continue;
                }
                if let Some(id) = c.attr("id") {
                    if keep_ids || refs.contains(id) {
                        useful.push(child.clone());
                        continue;
                    }
                    useful.extend(collect_useful_def_nodes(c, refs, keep_ids));
                    continue;
                }
                useful.extend(collect_useful_def_nodes(c, refs, keep_ids));
            }
            Node::Comment(body) if is_legal_comment(body) => useful.push(child.clone()),
            _ => {}
        }
    }
    useful
}

fn desc_is_disposable(el: &Element) -> bool {
    let text: String = el
        .children
        .iter()
        .filter_map(|n| match n {
            Node::Text(t) => Some(t.as_str()),
            _ => None,
        })
        .collect();
    let t = text.trim();
    if t.is_empty() {
        return true;
    }
    let lower = t.to_ascii_lowercase();
    lower.starts_with("created with")
        || lower.contains("created with inkscape")
        || lower.contains("created with adobe")
        || lower.contains("created with illustrator")
        || lower.contains("created with sketch")
        || lower.contains("created with figma")
}

fn is_empty_text_elem(el: &Element) -> bool {
    match el.local_name() {
        "text" | "tspan" => !el.children.iter().any(|c| match c {
            Node::Text(t) => !t.chars().all(char::is_whitespace),
            Node::Element(_) => true,
            _ => false,
        }),
        "tref" => el.attr("href").is_none() && el.attr("xlink:href").is_none(),
        _ => false,
    }
}

fn drop_hidden_elems(nodes: &mut Vec<Node>, refs: &HashSet<String>) {
    nodes.retain(|node| !is_hidden_droppable(node, refs));
    for node in nodes.iter_mut() {
        if let Node::Element(el) = node {
            drop_hidden_elems(&mut el.children, refs);
        }
    }
}

fn is_hidden_droppable(node: &Node, refs: &HashSet<String>) -> bool {
    let Node::Element(el) = node else {
        return false;
    };
    if el.local_name() == "svg" {
        return false;
    }
    if subtree_has_referenced_id(el, refs) {
        return false;
    }
    display_is_none(el) || visibility_is_hidden(el)
}

fn subtree_has_referenced_id(el: &Element, refs: &HashSet<String>) -> bool {
    if let Some(id) = el.attr("id") {
        if refs.contains(id) {
            return true;
        }
    }
    el.children.iter().any(|n| match n {
        Node::Element(c) => subtree_has_referenced_id(c, refs),
        _ => false,
    })
}

fn display_is_none(el: &Element) -> bool {
    if el
        .attr("display")
        .is_some_and(|v| v.trim().eq_ignore_ascii_case("none"))
    {
        return true;
    }
    style_decl_is(el, "display", "none")
}

fn visibility_is_hidden(el: &Element) -> bool {
    if el.attr("visibility").is_some_and(|v| {
        let t = v.trim();
        t.eq_ignore_ascii_case("hidden") || t.eq_ignore_ascii_case("collapse")
    }) {
        return true;
    }
    style_decl_is(el, "visibility", "hidden") || style_decl_is(el, "visibility", "collapse")
}

fn style_decl_is(el: &Element, prop: &str, value: &str) -> bool {
    let Some(style) = el.attr("style") else {
        return false;
    };
    style.split(';').any(|decl| {
        let Some((k, v)) = decl.split_once(':') else {
            return false;
        };
        k.trim().eq_ignore_ascii_case(prop) && v.trim().eq_ignore_ascii_case(value)
    })
}

fn collapse_attrless_groups(nodes: &mut Vec<Node>) {
    for node in nodes.iter_mut() {
        if let Node::Element(el) = node {
            collapse_attrless_groups(&mut el.children);
        }
    }
    let mut i = 0;
    while i < nodes.len() {
        let unwrap = matches!(
            &nodes[i],
            Node::Element(el) if el.local_name() == "g" && el.attrs.is_empty()
        );
        if !unwrap {
            i += 1;
            continue;
        }
        let Node::Element(el) = nodes.remove(i) else {
            i += 1;
            continue;
        };
        let n = el.children.len();
        nodes.splice(i..i, el.children);
        i += n;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn elem(name: &str, attrs: &[(&str, &str)], kids: Vec<Node>) -> Element {
        let mut el = Element::new(name);
        for (k, v) in attrs {
            el.set_attr(k, (*v).to_string());
        }
        el.children = kids;
        el
    }

    fn node(name: &str, attrs: &[(&str, &str)], kids: Vec<Node>) -> Node {
        Node::Element(elem(name, attrs, kids))
    }

    fn doc_svg(attrs: &[(&str, &str)], kids: Vec<Node>) -> Document {
        Document {
            nodes: vec![node("svg", attrs, kids)],
        }
    }

    fn root(doc: &Document) -> &Element {
        match doc.nodes.iter().find_map(|n| match n {
            Node::Element(el) if el.local_name() == "svg" => Some(el),
            _ => None,
        }) {
            Some(el) => el,
            None => panic!("expected a root <svg>"),
        }
    }

    fn names_under(el: &Element) -> Vec<&str> {
        el.children
            .iter()
            .filter_map(|n| match n {
                Node::Element(child) => Some(child.name.as_str()),
                _ => None,
            })
            .collect()
    }

    fn has_attr(el: &Element, name: &str) -> bool {
        el.attr(name).is_some()
    }

    fn descendant_has_name(el: &Element, name: &str) -> bool {
        el.children.iter().any(|n| match n {
            Node::Element(child) => {
                child.name == name || child.local_name() == name || descendant_has_name(child, name)
            }
            _ => false,
        })
    }

    #[test]
    fn remove_doctype_drops_dtd_keeps_svg() {
        let mut doc = Document {
            nodes: vec![
                Node::Doctype("svg PUBLIC \"-//W3C//DTD SVG 1.1//EN\"".into()),
                node("svg", &[("xmlns", "http://www.w3.org/2000/svg")], vec![]),
            ],
        };
        remove_doctype(&mut doc);
        assert!(doc.nodes.iter().all(|n| !matches!(n, Node::Doctype(_))));
        assert!(matches!(doc.nodes[0], Node::Element(_)));
    }

    #[test]
    fn remove_xml_proc_inst_drops_decl_and_xml_pi_keeps_stylesheet() {
        let mut doc = Document {
            nodes: vec![
                Node::Decl {
                    version: "1.0".into(),
                    encoding: Some("UTF-8".into()),
                    standalone: None,
                },
                Node::Pi {
                    target: "xml".into(),
                    value: "version=\"1.0\"".into(),
                },
                Node::Pi {
                    target: "xml-stylesheet".into(),
                    value: "href=\"x.css\"".into(),
                },
                node("svg", &[], vec![]),
            ],
        };
        remove_xml_proc_inst(&mut doc);
        assert!(doc.nodes.iter().all(|n| !matches!(n, Node::Decl { .. })));
        assert!(doc.nodes.iter().any(|n| matches!(
            n,
            Node::Pi { target, .. } if target == "xml-stylesheet"
        )));
    }

    #[test]
    fn remove_comments_keeps_legal_bang_comments() {
        let mut doc = doc_svg(
            &[],
            vec![
                Node::Comment(" note ".into()),
                Node::Comment("! Font Awesome Free 6 — CC-BY-4.0".into()),
                node("circle", &[("r", "2")], vec![]),
            ],
        );
        remove_comments(&mut doc);
        let comments: Vec<&str> = root(&doc)
            .children
            .iter()
            .filter_map(|n| match n {
                Node::Comment(c) => Some(c.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(comments, ["! Font Awesome Free 6 — CC-BY-4.0"]);
        assert!(descendant_has_name(root(&doc), "circle"));
    }

    #[test]
    fn remove_metadata_drops_prefixed_and_plain() {
        let mut doc = doc_svg(
            &[("xmlns:rdf", "http://www.w3.org/1999/02/22-rdf-syntax-ns#")],
            vec![
                node("metadata", &[], vec![node("rdf:Description", &[], vec![])]),
                node("circle", &[("r", "1")], vec![]),
            ],
        );
        remove_metadata(&mut doc);
        assert!(!descendant_has_name(root(&doc), "metadata"));
        assert!(descendant_has_name(root(&doc), "circle"));
        // Declaration stays until removeUnusedNS — that pairing is the ROI.
        assert!(has_attr(root(&doc), "xmlns:rdf"));
    }

    #[test]
    fn remove_editors_ns_strips_namedview_and_inkscape_attrs() {
        let mut doc = doc_svg(
            &[
                ("xmlns", "http://www.w3.org/2000/svg"),
                (
                    "xmlns:inkscape",
                    "http://www.inkscape.org/namespaces/inkscape",
                ),
                ("inkscape:version", "1.3"),
            ],
            vec![
                node("sodipodi:namedview", &[("inkscape:cx", "0")], vec![]),
                node(
                    "g",
                    &[("inkscape:label", "Layer 1")],
                    vec![node("circle", &[("r", "2")], vec![])],
                ),
            ],
        );
        remove_editors_ns_data(&mut doc);
        let svg = root(&doc);
        assert!(!has_attr(svg, "xmlns:inkscape"));
        assert!(!has_attr(svg, "inkscape:version"));
        assert!(!descendant_has_name(svg, "sodipodi:namedview"));
        assert!(!descendant_has_name(svg, "namedview"));
        let g = svg.children.iter().find_map(|n| match n {
            Node::Element(el) if el.local_name() == "g" => Some(el),
            _ => None,
        });
        let g = g.expect("group kept");
        assert!(!has_attr(g, "inkscape:label"));
        assert!(descendant_has_name(g, "circle"));
    }

    #[test]
    fn cleanup_attrs_squeezes_ws_but_does_not_drop() {
        let mut doc = doc_svg(
            &[("viewBox", "  0   0\n 24   24  ")],
            vec![node("rect", &[("class", "   ")], vec![])],
        );
        cleanup_attrs(&mut doc);
        assert_eq!(root(&doc).attr("viewBox"), Some("0 0 24 24"));
        let rect = match &root(&doc).children[0] {
            Node::Element(el) => el,
            _ => panic!("rect"),
        };
        assert_eq!(rect.attr("class"), Some(""));
    }

    #[test]
    fn remove_empty_attrs_keeps_conditional_processing() {
        let mut doc = doc_svg(
            &[],
            vec![node(
                "rect",
                &[
                    ("class", ""),
                    ("id", ""),
                    ("style", ""),
                    ("requiredFeatures", ""),
                    ("requiredExtensions", ""),
                    ("systemLanguage", ""),
                    ("width", "10"),
                ],
                vec![],
            )],
        );
        remove_empty_attrs(&mut doc);
        let rect = match &root(&doc).children[0] {
            Node::Element(el) => el,
            _ => panic!("rect"),
        };
        assert!(!has_attr(rect, "class"));
        assert!(!has_attr(rect, "id"));
        assert!(!has_attr(rect, "style"));
        assert!(has_attr(rect, "requiredFeatures"));
        assert!(has_attr(rect, "requiredExtensions"));
        assert!(has_attr(rect, "systemLanguage"));
        assert_eq!(rect.attr("width"), Some("10"));
    }

    #[test]
    fn remove_empty_containers_nested_and_referenced() {
        let mut doc = doc_svg(
            &[],
            vec![
                node(
                    "defs",
                    &[],
                    vec![
                        node("g", &[("id", "unused")], vec![node("g", &[], vec![])]),
                        node("linearGradient", &[("id", "paint")], vec![]),
                    ],
                ),
                node("rect", &[("fill", "url(#paint)")], vec![]),
                node("g", &[], vec![Node::Text("   ".into())]),
            ],
        );
        remove_empty_containers(&mut doc);
        let svg = root(&doc);
        assert!(!names_under(svg).contains(&"g"));
        assert!(descendant_has_name(svg, "linearGradient"));
        assert!(descendant_has_name(svg, "rect"));
        // unused empty wrapper chain is gone; defs may remain if it still
        // holds the referenced gradient.
        let defs = svg.children.iter().find_map(|n| match n {
            Node::Element(el) if el.local_name() == "defs" => Some(el),
            _ => None,
        });
        let defs = defs.expect("defs kept for referenced gradient");
        assert!(!descendant_has_name(defs, "g"));
        assert_eq!(
            defs.children
                .iter()
                .filter_map(|n| match n {
                    Node::Element(el) => el.attr("id"),
                    _ => None,
                })
                .collect::<Vec<_>>(),
            ["paint"]
        );
    }

    #[test]
    fn remove_empty_containers_never_drops_svg() {
        let mut doc = doc_svg(&[], vec![]);
        remove_empty_containers(&mut doc);
        assert_eq!(root(&doc).local_name(), "svg");
    }

    #[test]
    fn remove_unused_ns_after_metadata_is_high_roi() {
        let mut doc = doc_svg(
            &[
                ("xmlns", "http://www.w3.org/2000/svg"),
                ("xmlns:rdf", "http://www.w3.org/1999/02/22-rdf-syntax-ns#"),
                ("xmlns:cc", "http://creativecommons.org/ns#"),
                ("xmlns:xlink", "http://www.w3.org/1999/xlink"),
            ],
            vec![
                node("metadata", &[], vec![node("rdf:Description", &[], vec![])]),
                node("use", &[("xlink:href", "#icon")], vec![]),
            ],
        );
        remove_metadata(&mut doc);
        remove_unused_ns(&mut doc);
        let svg = root(&doc);
        assert!(has_attr(svg, "xmlns"));
        assert!(has_attr(svg, "xmlns:xlink"));
        assert!(!has_attr(svg, "xmlns:rdf"));
        assert!(!has_attr(svg, "xmlns:cc"));
    }

    #[test]
    fn run_strip_full_pipeline_editor_and_unused_ns() {
        let mut doc = Document {
            nodes: vec![
                Node::Decl {
                    version: "1.0".into(),
                    encoding: None,
                    standalone: None,
                },
                Node::Doctype("svg".into()),
                Node::Comment(" draft ".into()),
                node(
                    "svg",
                    &[
                        ("xmlns", "http://www.w3.org/2000/svg"),
                        (
                            "xmlns:inkscape",
                            "http://www.inkscape.org/namespaces/inkscape",
                        ),
                        ("xmlns:rdf", "http://www.w3.org/1999/02/22-rdf-syntax-ns#"),
                        ("viewBox", "  0  0  10  10 "),
                    ],
                    vec![
                        node("metadata", &[], vec![]),
                        node("g", &[("id", "unused"), ("class", "")], vec![]),
                        node("circle", &[("cx", "1"), ("cy", "1"), ("r", "1")], vec![]),
                    ],
                ),
            ],
        };
        run_strip(&mut doc);
        assert!(doc.nodes.iter().all(|n| !matches!(n, Node::Decl { .. })));
        assert!(doc.nodes.iter().all(|n| !matches!(n, Node::Doctype(_))));
        let svg = root(&doc);
        assert_eq!(svg.attr("viewBox"), Some("0 0 10 10"));
        assert!(has_attr(svg, "xmlns"));
        assert!(!has_attr(svg, "xmlns:inkscape"));
        assert!(!has_attr(svg, "xmlns:rdf"));
        assert!(!descendant_has_name(svg, "metadata"));
        assert!(!descendant_has_name(svg, "g"));
        assert!(descendant_has_name(svg, "circle"));
    }

    #[test]
    fn strip_plugin_names_match_svgo_ids() {
        assert!(STRIP_PLUGIN_NAMES.contains(&"removeUnusedNS"));
        assert!(STRIP_PLUGIN_NAMES.contains(&"removeEditorsNSData"));
        assert!(STRIP_PLUGIN_NAMES.contains(&"removeUselessDefs"));
        assert!(STRIP_PLUGIN_NAMES.contains(&"collapseGroups"));
        assert_eq!(STRIP_PLUGIN_NAMES[0], "removeDoctype");
    }

    #[test]
    fn remove_useless_defs_drops_unreferenced_keeps_url_paint() {
        let mut doc = doc_svg(
            &[],
            vec![
                node(
                    "defs",
                    &[],
                    vec![
                        node("linearGradient", &[("id", "keep-me")], vec![]),
                        node("linearGradient", &[("id", "drop-me")], vec![]),
                        node(
                            "clipPath",
                            &[("id", "also-unused")],
                            vec![node("rect", &[("width", "10")], vec![])],
                        ),
                    ],
                ),
                node("rect", &[("fill", "url(#keep-me)")], vec![]),
            ],
        );
        remove_useless_defs(&mut doc, false);
        let svg = root(&doc);
        let ids: Vec<_> = svg
            .children
            .iter()
            .filter_map(|n| match n {
                Node::Element(el) if el.local_name() == "defs" => Some(el),
                _ => None,
            })
            .flat_map(|defs| {
                defs.children.iter().filter_map(|n| match n {
                    Node::Element(el) => el.attr("id"),
                    _ => None,
                })
            })
            .collect();
        assert_eq!(ids, ["keep-me"]);
    }

    #[test]
    fn remove_useless_defs_keeps_ids_when_motion_asks() {
        let mut doc = doc_svg(
            &[],
            vec![node(
                "defs",
                &[],
                vec![node("linearGradient", &[("id", "maybe-js")], vec![])],
            )],
        );
        remove_useless_defs(&mut doc, true);
        assert!(descendant_has_name(root(&doc), "linearGradient"));
    }

    #[test]
    fn collapse_groups_unwraps_attrless_keeps_classed() {
        let mut doc = doc_svg(
            &[],
            vec![
                node("g", &[], vec![node("circle", &[("r", "2")], vec![])]),
                node(
                    "g",
                    &[("class", "land fr")],
                    vec![node("path", &[("d", "M0 0")], vec![])],
                ),
            ],
        );
        collapse_groups(&mut doc);
        let svg = root(&doc);
        assert!(
            !names_under(svg).contains(&"g") || {
                svg.children.iter().any(|n| match n {
                    Node::Element(el) => {
                        el.local_name() == "g" && el.attr("class") == Some("land fr")
                    }
                    _ => false,
                })
            }
        );
        assert!(descendant_has_name(svg, "circle"));
        let classed = svg.children.iter().any(|n| match n {
            Node::Element(el) => el.attr("class") == Some("land fr"),
            _ => false,
        });
        assert!(classed, "classed country group must stay");
    }

    #[test]
    fn remove_hidden_skips_opacity_zero_and_referenced() {
        let mut doc = doc_svg(
            &[],
            vec![
                node(
                    "g",
                    &[("display", "none")],
                    vec![node("rect", &[("width", "1")], vec![])],
                ),
                node(
                    "g",
                    &[("visibility", "hidden")],
                    vec![node("circle", &[("r", "1")], vec![])],
                ),
                node("circle", &[("class", "circle"), ("r", "2")], vec![]),
                node("rect", &[("id", "hook"), ("display", "none")], vec![]),
                node("use", &[("href", "#hook")], vec![]),
            ],
        );
        remove_hidden_elems(&mut doc);
        let svg = root(&doc);
        assert!(!descendant_has_name(svg, "rect") || find_id(svg, "hook").is_some());
        assert!(descendant_has_name(svg, "circle"));
        assert_eq!(
            find_id(svg, "hook").and_then(|e| e.attr("id")),
            Some("hook")
        );
    }

    #[test]
    fn remove_desc_empty_and_editor_only() {
        let mut doc = doc_svg(
            &[],
            vec![
                node("desc", &[], vec![Node::Text("   ".into())]),
                node(
                    "desc",
                    &[],
                    vec![Node::Text(
                        "Created with Inkscape (https://inkscape.org/)".into(),
                    )],
                ),
                node(
                    "desc",
                    &[],
                    vec![Node::Text("A real map legend for screen readers".into())],
                ),
            ],
        );
        remove_desc(&mut doc);
        let texts: Vec<String> = root(&doc)
            .children
            .iter()
            .filter_map(|n| match n {
                Node::Element(el) if el.local_name() == "desc" => Some(
                    el.children
                        .iter()
                        .filter_map(|c| match c {
                            Node::Text(t) => Some(t.as_str()),
                            _ => None,
                        })
                        .collect(),
                ),
                _ => None,
            })
            .collect();
        assert_eq!(texts, ["A real map legend for screen readers"]);
    }

    fn find_id<'a>(el: &'a Element, id: &str) -> Option<&'a Element> {
        if el.attr("id") == Some(id) {
            return Some(el);
        }
        for c in &el.children {
            if let Node::Element(e) = c {
                if let Some(hit) = find_id(e, id) {
                    return Some(hit);
                }
            }
        }
        None
    }
}
