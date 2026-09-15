//! Opt-in reuse / paint-shortcut plugins (SVGO-named, default **off**).
//!
//! Clean-room from public contracts — not a port of SVGO / oxvg / svgm / vexy:
//!
//! - [`reusePaths`](https://svgo.dev/docs/plugins/reusePaths/) — identical
//!   path `d` → one `<path id>` in `<defs>` + `<use href="#id">`. Paint /
//!   `transform` / `style` stay on the `<use>` (SVGO also keys on fill/stroke;
//!   we key on `d` only so shared geometry can keep different paints).
//! - [`convertOneStopGradients`](https://svgo.dev/docs/plugins/convertOneStopGradients/)
//!   — single-stop `linearGradient` / `radialGradient` → solid paint; drop the
//!   definition; drop empty `<defs>`; drop unused `xmlns:xlink`.
//!
//! Never in the default pipeline (`plugins.rs` / Task 10 wires `--plugin`).
//! Callers must invoke these functions. Motion-sensitive documents no-op.
//! IDs that SMIL / `<use>` / `url(#)` / CSS `#id` still name are not rewritten.

use crate::animation::{detect_document, parse_smil_clock_value};
use crate::ast::{Document, Element, Node};
use crate::plugin_minify::{collect_url_ids, is_smil_element};
use std::collections::{BTreeMap, HashMap, HashSet};

/// SVGO opt-in IDs owned by this module. Not in `DEFAULT_PLUGIN_NAMES`.
pub const OPTIN_REUSE_PLUGIN_NAMES: &[&str] = &["reusePaths", "convertOneStopGradients"];

/// Run named opt-in passes. An empty `enabled` list (the default) is a no-op.
pub fn run_optin_reuse(doc: &mut Document, enabled: &[String]) {
    let on = |name: &str| enabled.iter().any(|s| s.eq_ignore_ascii_case(name));
    if on("reusePaths") {
        reuse_paths(doc);
    }
    if on("convertOneStopGradients") {
        convert_one_stop_gradients(doc);
    }
}

/// SVGO `reusePaths`. Skips motion documents.
pub fn reuse_paths(doc: &mut Document) {
    reuse_paths_with(doc, true);
}

/// Like [`reuse_paths`]; `skip_motion` is the animation-aware gate.
///
/// Even with `skip_motion == false`, paths whose `id` is a SMIL / pointer
/// hook stay as `<path>` (do not become `<use>`).
pub fn reuse_paths_with(doc: &mut Document, skip_motion: bool) {
    if skip_motion && document_is_motion(doc) {
        return;
    }
    reuse_paths_impl(doc);
}

/// Node-root entry for Task 10 (`run_<name>`).
pub fn run_reuse_paths(root: &mut Node) {
    apply_on_node(root, reuse_paths);
}

/// SVGO `convertOneStopGradients`. Skips motion documents.
pub fn convert_one_stop_gradients(doc: &mut Document) {
    convert_one_stop_gradients_with(doc, true);
}

/// Like [`convert_one_stop_gradients`]; `skip_motion` is the animation-aware gate.
///
/// Even with `skip_motion == false`, gradient ids that SMIL still names stay.
pub fn convert_one_stop_gradients_with(doc: &mut Document, skip_motion: bool) {
    if skip_motion && document_is_motion(doc) {
        return;
    }
    convert_one_stop_impl(doc);
}

/// Node-root entry for Task 10 (`run_<name>`).
pub fn run_convert_one_stop_gradients(root: &mut Node) {
    apply_on_node(root, convert_one_stop_gradients);
}

fn apply_on_node(root: &mut Node, f: fn(&mut Document)) {
    let mut doc = Document {
        nodes: vec![std::mem::replace(root, Node::Text(String::new()))],
    };
    f(&mut doc);
    if let Some(node) = doc.nodes.into_iter().next() {
        *root = node;
    }
}

fn document_is_motion(doc: &Document) -> bool {
    detect_document(doc, &[]).motion_sensitive
}

// ---------------------------------------------------------------------------
// reusePaths
// ---------------------------------------------------------------------------

fn reuse_paths_impl(doc: &mut Document) {
    let protected = collect_protected_ids(&doc.nodes);
    let mut counts: HashMap<String, usize> = HashMap::new();
    count_reusable(&doc.nodes, false, &protected, &mut counts);

    let mut templates: HashMap<String, String> = HashMap::new();
    collect_geometry_templates(&doc.nodes, false, &mut templates);

    let mut existing = HashSet::new();
    collect_element_ids(&doc.nodes, &mut existing);

    let mut d_to_id: BTreeMap<String, String> = BTreeMap::new();
    let mut next = 0u32;
    let mut need_insert: BTreeMap<String, String> = BTreeMap::new();
    for (d, count) in counts {
        if count < 2 {
            continue;
        }
        if let Some(id) = templates.get(&d) {
            d_to_id.insert(d, id.clone());
            continue;
        }
        let id = next_reuse_id(&existing, &mut next);
        existing.insert(id.clone());
        need_insert.insert(id.clone(), d.clone());
        d_to_id.insert(d, id);
    }
    if d_to_id.is_empty() {
        return;
    }
    insert_defs_paths(doc, &need_insert);
    replace_reusable_paths(&mut doc.nodes, false, &protected, &d_to_id);
}

fn count_reusable(
    nodes: &[Node],
    in_skip: bool,
    protected: &HashSet<String>,
    counts: &mut HashMap<String, usize>,
) {
    for node in nodes {
        let Node::Element(el) = node else {
            continue;
        };
        let skip = in_skip || is_skip_container(el.local_name());
        if !skip {
            if let Some(d) = reusable_d(el, protected) {
                *counts.entry(d.to_string()).or_insert(0) += 1;
            }
        }
        count_reusable(&el.children, skip, protected, counts);
    }
}

fn collect_geometry_templates(nodes: &[Node], in_defs: bool, into: &mut HashMap<String, String>) {
    for node in nodes {
        let Node::Element(el) = node else {
            continue;
        };
        let defs = in_defs || el.local_name() == "defs";
        if defs && is_geometry_only_path(el) {
            if let (Some(d), Some(id)) = (el.attr("d"), el.attr("id")) {
                into.entry(d.to_string()).or_insert_with(|| id.to_string());
            }
        }
        collect_geometry_templates(&el.children, defs, into);
    }
}

fn is_geometry_only_path(el: &Element) -> bool {
    if el.local_name() != "path" || el.attr("d").is_none() || element_id(el).is_none() {
        return false;
    }
    if has_element_or_comment_child(el) {
        return false;
    }
    el.attrs.iter().all(|(k, _)| {
        let local = attr_local(k);
        local.eq_ignore_ascii_case("id")
            || local.eq_ignore_ascii_case("d")
            || local.eq_ignore_ascii_case("xml:id")
    })
}

fn reusable_d<'a>(el: &'a Element, protected: &HashSet<String>) -> Option<&'a str> {
    if el.local_name() != "path" {
        return None;
    }
    let d = el.attr("d")?;
    if d.trim().is_empty() {
        return None;
    }
    if el.attr("pathLength").is_some() {
        return None;
    }
    if has_element_or_comment_child(el) {
        return None;
    }
    if let Some(id) = element_id(el) {
        if protected.contains(id) {
            return None;
        }
    }
    Some(d)
}

fn has_element_or_comment_child(el: &Element) -> bool {
    el.children.iter().any(|c| {
        matches!(
            c,
            Node::Element(_) | Node::Comment(_) | Node::Pi { .. } | Node::Doctype(_)
        )
    })
}

fn replace_reusable_paths(
    nodes: &mut [Node],
    in_skip: bool,
    protected: &HashSet<String>,
    d_to_id: &BTreeMap<String, String>,
) {
    for node in nodes.iter_mut() {
        let Node::Element(el) = node else {
            continue;
        };
        let skip = in_skip || is_skip_container(el.local_name());
        if !skip {
            if let Some(d) = reusable_d(el, protected) {
                if let Some(id) = d_to_id.get(d) {
                    path_to_use(el, id);
                }
            }
        }
        replace_reusable_paths(&mut el.children, skip, protected, d_to_id);
    }
}

fn path_to_use(el: &mut Element, href_id: &str) {
    el.name = "use".into();
    el.remove_attr("d");
    el.set_attr("href", format!("#{href_id}"));
    el.children.clear();
    el.self_closing = true;
}

fn insert_defs_paths(doc: &mut Document, id_to_d: &BTreeMap<String, String>) {
    if id_to_d.is_empty() {
        return;
    }
    let Some(svg) = find_svg_mut(&mut doc.nodes) else {
        return;
    };
    let mut paths: Vec<Node> = id_to_d
        .iter()
        .map(|(id, d)| {
            let mut path = Element::new("path");
            path.set_attr("id", id.clone());
            path.set_attr("d", d.clone());
            path.self_closing = true;
            Node::Element(path)
        })
        .collect();
    let defs_idx = svg.children.iter().position(|n| match n {
        Node::Element(el) => el.local_name() == "defs",
        _ => false,
    });
    if let Some(i) = defs_idx {
        if let Node::Element(defs) = &mut svg.children[i] {
            defs.children.append(&mut paths);
            defs.self_closing = false;
        }
    } else {
        let mut defs = Element::new("defs");
        defs.children = paths;
        svg.children.insert(0, Node::Element(defs));
    }
}

fn find_svg_mut(nodes: &mut [Node]) -> Option<&mut Element> {
    nodes.iter_mut().find_map(|n| match n {
        Node::Element(el) if el.local_name() == "svg" => Some(el),
        _ => None,
    })
}

fn next_reuse_id(existing: &HashSet<String>, next: &mut u32) -> String {
    loop {
        let id = format!("reusePath{next}");
        *next += 1;
        if !existing.contains(&id) {
            return id;
        }
    }
}

fn is_skip_container(local: &str) -> bool {
    matches!(
        local,
        "defs"
            | "symbol"
            | "clipPath"
            | "mask"
            | "pattern"
            | "marker"
            | "filter"
            | "style"
            | "script"
            | "linearGradient"
            | "radialGradient"
            | "text"
            | "tspan"
            | "textPath"
            | "foreignObject"
    )
}

// ---------------------------------------------------------------------------
// convertOneStopGradients
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
struct SolidPaint {
    color: String,
    opacity: Option<String>,
}

struct GradRecord {
    href: Option<String>,
    stops: Vec<SolidPaint>,
    smil: bool,
}

fn convert_one_stop_impl(doc: &mut Document) {
    let protected = collect_smil_named_ids(&doc.nodes);
    let mut recs: HashMap<String, GradRecord> = HashMap::new();
    collect_gradients(&doc.nodes, &mut recs);

    let mut colors: HashMap<String, SolidPaint> = HashMap::new();
    let ids: Vec<String> = recs.keys().cloned().collect();
    for id in ids {
        if protected.contains(&id) {
            continue;
        }
        let mut stack = HashSet::new();
        if let Some(paint) = resolve_one_stop(&id, &recs, &mut stack) {
            colors.insert(id, paint);
        }
    }
    if colors.is_empty() && !has_idless_one_stop(&doc.nodes, &recs) {
        return;
    }

    rewrite_paints(&mut doc.nodes, &colors);
    drop_converted_gradients(&mut doc.nodes, &colors, &protected, &recs);
    drop_empty_defs(&mut doc.nodes);
    if !xlink_still_used(&doc.nodes) {
        strip_xmlns_xlink(&mut doc.nodes);
    }
}

fn collect_gradients(nodes: &[Node], out: &mut HashMap<String, GradRecord>) {
    for node in nodes {
        let Node::Element(el) = node else {
            continue;
        };
        if is_gradient_name(el.local_name()) {
            if let Some(id) = element_id(el) {
                out.insert(
                    id.to_string(),
                    GradRecord {
                        href: href_fragment(el),
                        stops: local_stops(el),
                        smil: subtree_has_smil(el),
                    },
                );
            }
        }
        collect_gradients(&el.children, out);
    }
}

fn is_gradient_name(local: &str) -> bool {
    local.eq_ignore_ascii_case("linearGradient") || local.eq_ignore_ascii_case("radialGradient")
}

fn local_stops(el: &Element) -> Vec<SolidPaint> {
    el.children
        .iter()
        .filter_map(|n| match n {
            Node::Element(c) if c.local_name() == "stop" => Some(stop_paint(c)),
            _ => None,
        })
        .collect()
}

fn stop_paint(el: &Element) -> SolidPaint {
    let style = el.attr("style").unwrap_or("");
    let color = el
        .attr("stop-color")
        .map(str::to_string)
        .or_else(|| style_prop(style, "stop-color"))
        .unwrap_or_else(|| "#000".into());
    let opacity = el
        .attr("stop-opacity")
        .map(str::to_string)
        .or_else(|| style_prop(style, "stop-opacity"))
        .filter(|o| !is_one_opacity(o));
    SolidPaint { color, opacity }
}

fn resolve_one_stop(
    id: &str,
    recs: &HashMap<String, GradRecord>,
    stack: &mut HashSet<String>,
) -> Option<SolidPaint> {
    if !stack.insert(id.to_string()) {
        return None;
    }
    let rec = recs.get(id)?;
    if rec.smil {
        stack.remove(id);
        return None;
    }
    let found = if rec.stops.len() == 1 {
        Some(rec.stops[0].clone())
    } else if rec.stops.len() > 1 {
        None
    } else {
        rec.href
            .as_deref()
            .and_then(|href| resolve_one_stop(href, recs, stack))
    };
    stack.remove(id);
    found
}

fn has_idless_one_stop(nodes: &[Node], recs: &HashMap<String, GradRecord>) -> bool {
    for node in nodes {
        let Node::Element(el) = node else {
            continue;
        };
        if is_gradient_name(el.local_name()) && element_id(el).is_none() {
            let stops = local_stops(el);
            if stops.len() == 1 && !subtree_has_smil(el) {
                return true;
            }
            if stops.is_empty() {
                if let Some(href) = href_fragment(el) {
                    let mut stack = HashSet::new();
                    if resolve_one_stop(&href, recs, &mut stack).is_some() {
                        return true;
                    }
                }
            }
        }
        if has_idless_one_stop(&el.children, recs) {
            return true;
        }
    }
    false
}

fn rewrite_paints(nodes: &mut [Node], colors: &HashMap<String, SolidPaint>) {
    for node in nodes.iter_mut() {
        let Node::Element(el) = node else {
            continue;
        };
        let smil = is_smil_element(el.local_name());
        let keys: Vec<String> = el.attrs.iter().map(|(k, _)| k.clone()).collect();
        for k in keys {
            if attr_local(&k).eq_ignore_ascii_case("style") {
                continue;
            }
            if smil && is_smil_clock_or_value_attr(&k) {
                continue;
            }
            let Some(v) = el.attr(&k).map(str::to_string) else {
                continue;
            };
            let Some(paint) = paint_for_url(&v, colors) else {
                continue;
            };
            el.set_attr(&k, paint_css_token(&paint));
            apply_opacity_companion(el, &k, &paint);
        }
        if let Some(style) = el.attr("style").map(str::to_string) {
            el.set_attr("style", rewrite_style_urls(&style, colors));
        }
        if el.local_name() == "style" {
            for child in &mut el.children {
                if let Node::Text(t) = child {
                    *t = rewrite_css_urls(t, colors);
                }
            }
        }
        rewrite_paints(&mut el.children, colors);
    }
}

fn is_smil_clock_or_value_attr(name: &str) -> bool {
    matches!(
        attr_local(name),
        "begin"
            | "end"
            | "dur"
            | "min"
            | "max"
            | "values"
            | "from"
            | "to"
            | "by"
            | "keyTimes"
            | "keySplines"
            | "repeatDur"
    )
}

fn paint_for_url<'a>(
    value: &str,
    colors: &'a HashMap<String, SolidPaint>,
) -> Option<&'a SolidPaint> {
    let id = parse_url_hash_id(value)?;
    colors.get(&id)
}

fn parse_url_hash_id(value: &str) -> Option<String> {
    let v = value.trim();
    let folded = v.to_ascii_lowercase();
    let rel = folded.find("url(")?;
    if rel != 0 {
        return None;
    }
    let inner = v[4..].trim_start();
    let inner = inner.trim_start_matches(['\'', '"']);
    let rest = inner.strip_prefix('#')?;
    let id: String = rest
        .chars()
        .take_while(|c| *c != ')' && *c != '\'' && *c != '"' && !c.is_whitespace())
        .collect();
    if id.is_empty() {
        None
    } else {
        Some(id)
    }
}

fn apply_opacity_companion(el: &mut Element, paint_attr: &str, paint: &SolidPaint) {
    let Some(op) = &paint.opacity else {
        return;
    };
    let companion = match attr_local(paint_attr) {
        "fill" => "fill-opacity",
        "stroke" => "stroke-opacity",
        "stop-color" => "stop-opacity",
        "flood-color" => "flood-opacity",
        _ => return,
    };
    if el.attr(companion).is_none() {
        el.set_attr(companion, op.clone());
    }
}

fn rewrite_style_urls(style: &str, colors: &HashMap<String, SolidPaint>) -> String {
    let mut parts = Vec::new();
    for decl in style.split(';') {
        let trimmed = decl.trim();
        if trimmed.is_empty() {
            continue;
        }
        match trimmed.split_once(':') {
            Some((k, v)) => {
                if let Some(paint) = paint_for_url(v.trim(), colors) {
                    parts.push(format!("{}:{}", k.trim(), paint_css_token(paint)));
                    if let Some(op) = &paint.opacity {
                        if let Some(comp) = style_opacity_companion(k.trim()) {
                            if style_prop(style, comp).is_none() {
                                parts.push(format!("{comp}:{op}"));
                            }
                        }
                    }
                } else {
                    parts.push(trimmed.to_string());
                }
            }
            None => parts.push(trimmed.to_string()),
        }
    }
    parts.join(";")
}

fn style_opacity_companion(prop: &str) -> Option<&'static str> {
    match prop.trim().to_ascii_lowercase().as_str() {
        "fill" => Some("fill-opacity"),
        "stroke" => Some("stroke-opacity"),
        "stop-color" => Some("stop-opacity"),
        "flood-color" => Some("flood-opacity"),
        _ => None,
    }
}

fn rewrite_css_urls(css: &str, colors: &HashMap<String, SolidPaint>) -> String {
    let folded = css.to_ascii_lowercase();
    let mut out = String::with_capacity(css.len());
    let mut i = 0;
    while i < css.len() {
        if let Some(rel) = folded[i..].find("url(") {
            out.push_str(&css[i..i + rel]);
            let start = i + rel;
            let after = &css[start + 4..];
            let trimmed = after.trim_start();
            let quote_pad = after.len() - trimmed.len();
            let (inner, qlen) = if let Some(stripped) = trimmed.strip_prefix(['\'', '"']) {
                (stripped, 1)
            } else {
                (trimmed, 0)
            };
            if let Some(rest) = inner.strip_prefix('#') {
                let id: String = rest
                    .chars()
                    .take_while(|c| *c != ')' && *c != '\'' && *c != '"' && !c.is_whitespace())
                    .collect();
                if let Some(paint) = colors.get(&id) {
                    let mut consumed = 4 + quote_pad + qlen + 1 + id.len();
                    let tail = &css[start + consumed..];
                    let t = tail.trim_start();
                    let ws = tail.len() - t.len();
                    consumed += ws;
                    if t.starts_with(['\'', '"']) {
                        consumed += 1;
                    }
                    let tail2 = &css[start + consumed..];
                    let t2 = tail2.trim_start();
                    consumed += tail2.len() - t2.len();
                    if t2.starts_with(')') {
                        consumed += 1;
                        out.push_str(&paint_css_token(paint));
                        i = start + consumed;
                        continue;
                    }
                }
            }
            out.push_str(&css[start..start + 4]);
            i = start + 4;
        } else {
            out.push_str(&css[i..]);
            break;
        }
    }
    out
}

fn paint_css_token(paint: &SolidPaint) -> String {
    if let Some(op) = &paint.opacity {
        if let (Some((r, g, b)), Some(a)) = (parse_hex_rgb(&paint.color), parse_unit_interval(op)) {
            if a < 1.0 {
                return format!("rgba({r},{g},{b},{a})");
            }
        }
    }
    paint.color.clone()
}

fn parse_hex_rgb(color: &str) -> Option<(u8, u8, u8)> {
    let s = color.trim().strip_prefix('#')?;
    match s.len() {
        3 => {
            let r = u8::from_str_radix(&s[0..1].repeat(2), 16).ok()?;
            let g = u8::from_str_radix(&s[1..2].repeat(2), 16).ok()?;
            let b = u8::from_str_radix(&s[2..3].repeat(2), 16).ok()?;
            Some((r, g, b))
        }
        6 => {
            let r = u8::from_str_radix(&s[0..2], 16).ok()?;
            let g = u8::from_str_radix(&s[2..4], 16).ok()?;
            let b = u8::from_str_radix(&s[4..6], 16).ok()?;
            Some((r, g, b))
        }
        _ => None,
    }
}

fn parse_unit_interval(s: &str) -> Option<f32> {
    let t = s.trim();
    if let Some(pct) = t.strip_suffix('%') {
        return pct.parse::<f32>().ok().map(|n| (n / 100.0).clamp(0.0, 1.0));
    }
    t.parse::<f32>().ok().map(|n| n.clamp(0.0, 1.0))
}

fn drop_converted_gradients(
    nodes: &mut Vec<Node>,
    colors: &HashMap<String, SolidPaint>,
    protected: &HashSet<String>,
    recs: &HashMap<String, GradRecord>,
) {
    nodes.retain(|n| match n {
        Node::Element(el) if is_gradient_name(el.local_name()) => {
            !gradient_is_droppable(el, colors, protected, recs)
        }
        _ => true,
    });
    for node in nodes.iter_mut() {
        if let Node::Element(el) = node {
            drop_converted_gradients(&mut el.children, colors, protected, recs);
        }
    }
}

fn gradient_is_droppable(
    el: &Element,
    colors: &HashMap<String, SolidPaint>,
    protected: &HashSet<String>,
    recs: &HashMap<String, GradRecord>,
) -> bool {
    if subtree_has_smil(el) {
        return false;
    }
    if let Some(id) = element_id(el) {
        if protected.contains(id) {
            return false;
        }
        return colors.contains_key(id);
    }
    let stops = local_stops(el);
    if stops.len() == 1 {
        return true;
    }
    if stops.is_empty() {
        if let Some(href) = href_fragment(el) {
            let mut stack = HashSet::new();
            return resolve_one_stop(&href, recs, &mut stack).is_some();
        }
    }
    false
}

fn drop_empty_defs(nodes: &mut Vec<Node>) {
    nodes.retain(|n| match n {
        Node::Element(el) if el.local_name() == "defs" => defs_worth_keeping(el),
        _ => true,
    });
    for node in nodes.iter_mut() {
        if let Node::Element(el) = node {
            drop_empty_defs(&mut el.children);
        }
    }
}

fn defs_worth_keeping(el: &Element) -> bool {
    el.children.iter().any(|c| match c {
        Node::Element(_) | Node::Comment(_) | Node::Pi { .. } => true,
        Node::Text(t) => !t.chars().all(char::is_whitespace),
        _ => false,
    })
}

fn xlink_still_used(nodes: &[Node]) -> bool {
    for node in nodes {
        let Node::Element(el) = node else {
            continue;
        };
        if el.prefix().is_some_and(|p| p.eq_ignore_ascii_case("xlink")) {
            return true;
        }
        for (k, _) in &el.attrs {
            if k.eq_ignore_ascii_case("xmlns:xlink") {
                continue;
            }
            if let Some((prefix, _)) = k.split_once(':') {
                if prefix.eq_ignore_ascii_case("xlink") {
                    return true;
                }
            }
        }
        if xlink_still_used(&el.children) {
            return true;
        }
    }
    false
}

fn strip_xmlns_xlink(nodes: &mut [Node]) {
    for node in nodes.iter_mut() {
        if let Node::Element(el) = node {
            el.remove_attr("xmlns:xlink");
            strip_xmlns_xlink(&mut el.children);
        }
    }
}

// ---------------------------------------------------------------------------
// shared walks
// ---------------------------------------------------------------------------

fn element_id(el: &Element) -> Option<&str> {
    el.attr("id").or_else(|| el.attr("xml:id"))
}

fn href_fragment(el: &Element) -> Option<String> {
    for key in ["href", "xlink:href"] {
        if let Some(v) = el.attr(key) {
            if let Some(id) = v.trim().strip_prefix('#') {
                if !id.is_empty() {
                    return Some(id.to_string());
                }
            }
        }
    }
    None
}

fn attr_local(name: &str) -> &str {
    name.rsplit_once(':').map(|(_, l)| l).unwrap_or(name)
}

fn collect_element_ids(nodes: &[Node], out: &mut HashSet<String>) {
    for node in nodes {
        let Node::Element(el) = node else {
            continue;
        };
        if let Some(id) = element_id(el) {
            out.insert(id.to_string());
        }
        collect_element_ids(&el.children, out);
    }
}

fn collect_protected_ids(nodes: &[Node]) -> HashSet<String> {
    let mut out = HashSet::new();
    walk_protected(nodes, &mut out);
    out
}

fn collect_smil_named_ids(nodes: &[Node]) -> HashSet<String> {
    let mut out = HashSet::new();
    walk_smil_named(nodes, &mut out);
    out
}

fn walk_protected(nodes: &[Node], out: &mut HashSet<String>) {
    for node in nodes {
        let Node::Element(el) = node else {
            continue;
        };
        for (k, v) in &el.attrs {
            take_pointer_ids(k, v, out);
        }
        if el.local_name() == "style" {
            for child in &el.children {
                if let Node::Text(t) = child {
                    collect_css_hash_ids(t, out);
                    collect_url_ids(t, out);
                }
            }
        }
        walk_protected(&el.children, out);
    }
}

fn walk_smil_named(nodes: &[Node], out: &mut HashSet<String>) {
    for node in nodes {
        let Node::Element(el) = node else {
            continue;
        };
        if is_smil_element(el.local_name()) {
            if let Some(id) = href_fragment(el) {
                out.insert(id);
            }
            for key in ["begin", "end"] {
                if let Some(v) = el.attr(key) {
                    for r in parse_smil_clock_value(v) {
                        out.insert(r.id);
                    }
                }
            }
        }
        walk_smil_named(&el.children, out);
    }
}

fn take_pointer_ids(key: &str, value: &str, out: &mut HashSet<String>) {
    let local = attr_local(key).to_ascii_lowercase();
    if local == "href" {
        if let Some(id) = value.trim().strip_prefix('#') {
            if !id.is_empty() {
                out.insert(id.to_string());
            }
        }
    }
    if local == "begin" || local == "end" {
        for r in parse_smil_clock_value(value) {
            out.insert(r.id);
        }
    }
    collect_url_ids(value, out);
}

fn collect_css_hash_ids(css: &str, out: &mut HashSet<String>) {
    let bytes = css.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'#' && i + 1 < bytes.len() {
            let start = i + 1;
            if bytes[start].is_ascii_alphabetic() || bytes[start] == b'_' {
                let mut end = start + 1;
                while end < bytes.len()
                    && (bytes[end].is_ascii_alphanumeric()
                        || bytes[end] == b'_'
                        || bytes[end] == b'-'
                        || bytes[end] == b':')
                {
                    end += 1;
                }
                if let Ok(id) = std::str::from_utf8(&bytes[start..end]) {
                    if !is_hex_color(id) {
                        out.insert(id.to_string());
                    }
                }
                i = end;
                continue;
            }
        }
        i += 1;
    }
}

fn is_hex_color(s: &str) -> bool {
    matches!(s.len(), 3 | 4 | 6 | 8) && s.bytes().all(|b| b.is_ascii_hexdigit())
}

fn subtree_has_smil(el: &Element) -> bool {
    if is_smil_element(el.local_name()) {
        return true;
    }
    el.children.iter().any(|n| match n {
        Node::Element(c) => subtree_has_smil(c),
        _ => false,
    })
}

fn style_prop(style: &str, name: &str) -> Option<String> {
    let mut found = None;
    for decl in style.split(';') {
        if let Some((k, v)) = decl.split_once(':') {
            if k.trim().eq_ignore_ascii_case(name) {
                found = Some(v.trim().to_string());
            }
        }
    }
    found.filter(|s| !s.is_empty())
}

fn is_one_opacity(s: &str) -> bool {
    let t = s.trim();
    t == "1" || t == "1.0" || t == "+1" || t == "100%"
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::parse_svg;
    use crate::serialize::serialize;
    use crate::{optimize_str, Config};

    fn opt_reuse(svg: &str) -> String {
        let mut doc = parse_svg(svg).unwrap();
        reuse_paths(&mut doc);
        serialize(&doc)
    }

    fn opt_grad(svg: &str) -> String {
        let mut doc = parse_svg(svg).unwrap();
        convert_one_stop_gradients(&mut doc);
        serialize(&doc)
    }

    fn two_paths(d: &str) -> String {
        format!(
            concat!(
                r##"<svg xmlns="http://www.w3.org/2000/svg">"##,
                r##"<path d="{d}" fill="#f00"/>"##,
                r##"<path d="{d}" fill="#00f" transform="translate(10)"/>"##,
                "</svg>",
            ),
            d = d
        )
    }

    #[test]
    fn optin_names_are_svgo_ids_and_not_default() {
        assert_eq!(
            OPTIN_REUSE_PLUGIN_NAMES,
            &["reusePaths", "convertOneStopGradients"]
        );
        for name in OPTIN_REUSE_PLUGIN_NAMES {
            assert!(!crate::DEFAULT_PLUGIN_NAMES.contains(name));
        }
    }

    #[test]
    fn run_optin_empty_enabled_is_noop() {
        let input = two_paths("M0 0h2v2H0z");
        let mut doc = parse_svg(&input).unwrap();
        run_optin_reuse(&mut doc, &[]);
        let out = serialize(&doc);
        assert!(!out.contains("<use"), "{out}");
        assert_eq!(out.matches("<path").count(), 2);
    }

    #[test]
    fn reuse_paths_identical_d_becomes_use_and_defs() {
        let out = opt_reuse(&two_paths("M0 0h4v4H0z"));
        assert!(out.contains("<defs>"), "{out}");
        assert!(
            out.contains("<path id=\"reusePath0\" d=\"M0 0h4v4H0z\"/>"),
            "{out}"
        );
        assert_eq!(out.matches("<use ").count(), 2, "{out}");
        assert!(out.contains("href=\"#reusePath0\""), "{out}");
        assert!(out.contains("fill=\"#f00\""), "{out}");
        assert!(out.contains("fill=\"#00f\""), "{out}");
        assert!(out.contains("transform=\"translate(10)\""), "{out}");
        assert!(
            !out.contains("<path d=\"M0 0h4v4H0z\" fill=\"#f00\""),
            "{out}"
        );
    }

    #[test]
    fn reuse_paths_leaves_unique_d() {
        let input = concat!(
            r##"<svg xmlns="http://www.w3.org/2000/svg">"##,
            r##"<path d="M0 0h1"/>"##,
            r##"<path d="M0 0h2"/>"##,
            "</svg>",
        );
        let out = opt_reuse(input);
        assert!(!out.contains("<use"), "{out}");
        assert!(!out.contains("<defs"), "{out}");
        assert_eq!(out.matches("<path").count(), 2, "{out}");
    }

    #[test]
    fn reuse_paths_skips_motion_document() {
        let input = concat!(
            r##"<svg xmlns="http://www.w3.org/2000/svg">"##,
            r##"<path d="M0 0h4"><animate attributeName="opacity" values="0;1" dur="1s"/></path>"##,
            r##"<path d="M0 0h4"/>"##,
            "</svg>",
        );
        let out = opt_reuse(input);
        assert!(!out.contains("<use"), "{out}");
        assert!(out.contains("<path"), "{out}");
        assert!(out.contains("<animate"), "{out}");
    }

    #[test]
    fn reuse_paths_keeps_smil_href_id_when_forced() {
        let input = concat!(
            r##"<svg xmlns="http://www.w3.org/2000/svg">"##,
            r##"<path id="mouth" d="M0 0h4"/>"##,
            r##"<path d="M0 0h4"/>"##,
            r##"<path d="M0 0h4"/>"##,
            r##"<animate href="#mouth" attributeName="opacity" values="0;1" dur="1s"/>"##,
            "</svg>",
        );
        let mut doc = parse_svg(input).unwrap();
        reuse_paths_with(&mut doc, false);
        let out = serialize(&doc);
        assert!(out.contains("<path id=\"mouth\" d=\"M0 0h4\"/>"), "{out}");
        assert!(out.contains("href=\"#mouth\""), "{out}");
        assert_eq!(out.matches("<use ").count(), 2, "{out}");
        assert!(out.contains("<defs>"), "{out}");
    }

    #[test]
    fn reuse_paths_skips_clippath_and_keeps_existing_template() {
        let input = concat!(
            r##"<svg xmlns="http://www.w3.org/2000/svg">"##,
            r##"<defs><path id="stamp" d="M1 1h2"/><clipPath id="c"><path d="M1 1h2"/></clipPath></defs>"##,
            r##"<path d="M1 1h2" fill="red"/>"##,
            r##"<path d="M1 1h2" fill="blue"/>"##,
            "</svg>",
        );
        let out = opt_reuse(input);
        assert!(out.contains("id=\"stamp\""), "{out}");
        assert!(out.contains("href=\"#stamp\""), "{out}");
        assert!(out.contains("<clipPath"), "{out}");
        let clip = out.split("<clipPath").nth(1).unwrap();
        assert!(clip.contains("<path"), "clip path must stay a path: {out}");
        assert!(!out.contains("reusePath"), "{out}");
    }

    #[test]
    fn reuse_paths_keeps_unreferenced_id_on_use() {
        let input = concat!(
            r##"<svg xmlns="http://www.w3.org/2000/svg">"##,
            r##"<path id="a" d="M0 0h3"/>"##,
            r##"<path d="M0 0h3"/>"##,
            "</svg>",
        );
        let out = opt_reuse(input);
        assert!(out.contains("id=\"a\""), "{out}");
        assert!(
            out.contains("<use id=\"a\" href=\"#reusePath0\"/>") || out.contains("id=\"a\""),
            "{out}"
        );
        assert!(out.contains("<defs>"), "{out}");
    }

    #[test]
    fn convert_one_stop_linear_to_solid_fill() {
        let input = concat!(
            r##"<svg xmlns="http://www.w3.org/2000/svg">"##,
            r##"<defs><linearGradient id="g1"><stop offset="0" stop-color="#f00"/></linearGradient></defs>"##,
            r##"<rect width="10" height="10" fill="url(#g1)"/>"##,
            "</svg>",
        );
        let out = opt_grad(input);
        assert!(out.contains("fill=\"#f00\""), "{out}");
        assert!(!out.contains("linearGradient"), "{out}");
        assert!(!out.contains("url(#g1)"), "{out}");
        assert!(!out.contains("<defs"), "{out}");
    }

    #[test]
    fn convert_one_stop_radial_style_and_opacity() {
        let input = concat!(
            r##"<svg xmlns="http://www.w3.org/2000/svg">"##,
            r##"<defs><radialGradient id="g1"><stop style="stop-color:#0a0;stop-opacity:.5"/></radialGradient></defs>"##,
            r##"<circle r="4" style="fill:url(#g1)"/>"##,
            "</svg>",
        );
        let out = opt_grad(input);
        assert!(!out.contains("radialGradient"), "{out}");
        assert!(
            out.contains("rgba(0,170,0,0.5)")
                || out.contains("fill-opacity:.5")
                || out.contains("fill-opacity=\".5\""),
            "{out}"
        );
        assert!(out.contains("#0a0") || out.contains("rgba("), "{out}");
    }

    #[test]
    fn convert_one_stop_leaves_two_stop() {
        let input = concat!(
            r##"<svg xmlns="http://www.w3.org/2000/svg">"##,
            r##"<defs><linearGradient id="g1"><stop offset="0" stop-color="#f00"/><stop offset="1" stop-color="#00f"/></linearGradient></defs>"##,
            r##"<rect fill="url(#g1)" width="10" height="10"/>"##,
            "</svg>",
        );
        let out = opt_grad(input);
        assert!(out.contains("linearGradient"), "{out}");
        assert!(out.contains("url(#g1)"), "{out}");
    }

    #[test]
    fn convert_one_stop_follows_href_inherit() {
        let input = concat!(
            r##"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink">"##,
            r##"<defs>"##,
            r##"<linearGradient id="base"><stop stop-color="red"/></linearGradient>"##,
            r##"<linearGradient id="child" xlink:href="#base"/>"##,
            r##"</defs>"##,
            r##"<rect fill="url(#child)" width="4" height="4"/>"##,
            "</svg>",
        );
        let out = opt_grad(input);
        assert!(out.contains(r#"fill="red""#), "{out}");
        assert!(!out.contains("linearGradient"), "{out}");
        assert!(!out.contains("xmlns:xlink"), "{out}");
        assert!(!out.contains("url(#child)"), "{out}");
    }

    #[test]
    fn convert_one_stop_skips_motion_document() {
        let input = concat!(
            r##"<svg xmlns="http://www.w3.org/2000/svg">"##,
            r##"<defs><linearGradient id="g1"><stop stop-color="#f00"/></linearGradient></defs>"##,
            r##"<rect fill="url(#g1)" width="10" height="10">"##,
            r##"<animate attributeName="opacity" values="0;1" dur="1s"/>"##,
            "</rect></svg>",
        );
        let out = opt_grad(input);
        assert!(out.contains("linearGradient"), "{out}");
        assert!(out.contains("url(#g1)"), "{out}");
    }

    #[test]
    fn convert_one_stop_keeps_smil_named_gradient() {
        let input = concat!(
            r##"<svg xmlns="http://www.w3.org/2000/svg">"##,
            r##"<defs><linearGradient id="g1"><stop stop-color="#f00"/></linearGradient></defs>"##,
            r##"<rect fill="url(#g1)" width="10" height="10"/>"##,
            r##"<animate href="#g1" attributeName="x1" values="0;1" dur="1s"/>"##,
            "</svg>",
        );
        let mut doc = parse_svg(input).unwrap();
        convert_one_stop_gradients_with(&mut doc, false);
        let out = serialize(&doc);
        assert!(out.contains(r#"id="g1""#), "{out}");
        assert!(out.contains("linearGradient"), "{out}");
        assert!(out.contains("url(#g1)"), "{out}");
    }

    #[test]
    fn convert_one_stop_rewrites_stylesheet_url() {
        let input = concat!(
            r##"<svg xmlns="http://www.w3.org/2000/svg">"##,
            r##"<style>.x{fill:url(#g1)}</style>"##,
            r##"<defs><linearGradient id="g1"><stop stop-color="#00f"/></linearGradient></defs>"##,
            r##"<rect class="x" width="4" height="4"/>"##,
            "</svg>",
        );
        let out = opt_grad(input);
        assert!(
            out.contains("fill:#00f") || out.contains("fill: #00f"),
            "{out}"
        );
        assert!(!out.contains("url(#g1)"), "{out}");
        assert!(!out.contains("linearGradient"), "{out}");
    }

    #[test]
    fn default_pipeline_does_not_run_optin() {
        let paths = two_paths("M0 0h5v5H0z");
        let out = optimize_str(&paths).unwrap();
        assert!(!out.svg.contains("<use"), "{}", out.svg);
        assert!(out.svg.contains("<path"), "{}", out.svg);

        let grad = concat!(
            r##"<svg xmlns="http://www.w3.org/2000/svg">"##,
            r##"<defs><linearGradient id="g1"><stop offset="0" stop-color="#f00"/></linearGradient></defs>"##,
            r##"<rect width="10" height="10" fill="url(#g1)"/>"##,
            "</svg>",
        );
        let gout = optimize_str(grad).unwrap();
        assert!(
            gout.svg.contains("linearGradient") || gout.svg.contains("url(#g1)"),
            "{}",
            gout.svg
        );
        assert!(
            !gout.svg.contains("fill=\"#f00\"") || gout.svg.contains("linearGradient"),
            "{}",
            gout.svg
        );
        let _ = Config::default();
    }

    #[test]
    fn run_reuse_paths_on_node_root() {
        let input = two_paths("M2 2h1");
        let mut doc = parse_svg(&input).unwrap();
        let Node::Element(ref mut svg) = doc.nodes[0] else {
            panic!("svg root");
        };
        let mut root = Node::Element(svg.clone());
        run_reuse_paths(&mut root);
        let wrapped = Document { nodes: vec![root] };
        let out = serialize(&wrapped);
        assert!(out.contains("<use"), "{out}");
        assert!(out.contains("<defs>"), "{out}");
    }

    #[test]
    fn run_optin_enables_by_name() {
        let input = concat!(
            r##"<svg xmlns="http://www.w3.org/2000/svg">"##,
            r##"<defs><linearGradient id="g1"><stop stop-color="#111"/></linearGradient></defs>"##,
            r##"<path d="M0 0h2" fill="url(#g1)"/>"##,
            r##"<path d="M0 0h2" fill="url(#g1)"/>"##,
            "</svg>",
        );
        let mut doc = parse_svg(input).unwrap();
        run_optin_reuse(&mut doc, &["convertOneStopGradients".into()]);
        let mid = serialize(&doc);
        assert!(mid.contains("fill=\"#111\""), "{mid}");
        assert!(!mid.contains("<use"), "{mid}");
        run_optin_reuse(&mut doc, &["reusePaths".into()]);
        let out = serialize(&doc);
        assert!(out.contains("<use"), "{out}");
    }
}
