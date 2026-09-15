//! Strip / cleanup plugins (SVGO-inspired names, clean-room implementations).
//!
//! Written from public plugin contracts only — not from `plugins.rs` or any
//! reference-engine source. Safety notes come from:
//!
//! - [SVGO preset-default](https://svgo.dev/docs/preset-default/)
//! - [SVGO plugin pages](https://svgo.dev/docs/plugins/) (`removeDoctype`,
//!   `removeXMLProcInst`, `removeComments`, `removeMetadata`,
//!   `removeEditorsNSData`, `cleanupAttrs`, `removeEmptyAttrs`,
//!   `removeEmptyContainers`, `removeUnusedNS`,
//!   `removeNonInheritableGroupAttrs`, `cleanupEnableBackground`,
//!   `removeUselessDefs`, `removeDesc`, `removeUnknownsAndDefaults`,
//!   `moveElemsAttrsToGroup`, `moveGroupAttrsToElems`, `sortAttrs`,
//!   `sortDefsChildren`)
//! - [SVG 1.1 property index](https://www.w3.org/TR/SVG11/propidx.html)
//!   (inherit column — group attr safety / presentation initials)
//! - [oxvg_optimiser](https://docs.rs/oxvg_optimiser/latest/oxvg_optimiser/)
//!   job summaries (`Remove*` / `CleanupAttrs` / `CleanupEnableBackground` /
//!   `MoveElemsAttrsToGroup` / `MoveGroupAttrsToElems` /
//!   `SortAttrs` / `SortDefsChildren`)
//! - [Vexy SVGO plugin reference](https://vexy.dev/vexy-svgo/user/plugins/)
//!
//! Merge-wire (`plugins.rs`, not this file): call
//! [`remove_unknowns_and_defaults`], [`remove_non_inheritable_group_attrs`],
//! [`cleanup_enable_background`], [`move_elems_attrs_to_group`],
//! [`move_group_attrs_to_elems`], [`sort_attrs`], and [`sort_defs_children`]
//! from `run_default` (SVGO 4.1.0 default IDs). Do **not** enable
//! `removeViewBox` / `removeTitle`. `removeHiddenElems` stays motion-gated
//! and also refuses SMIL `visibility` frames if invoked directly.
//! `removeUnknownsAndDefaults` never deletes elements and never drops
//! `role` / `viewBox` / `xmlns` / `<title>`.

use crate::animation::parse_smil_clock_value;
use crate::ast::{Document, Element, Node};
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

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
    "removeUnknownsAndDefaults",
    "removeUselessDefs",
    "removeNonInheritableGroupAttrs",
    "cleanupEnableBackground",
    "removeDesc",
    "removeEmptyText",
    "removeHiddenElems",
    "moveElemsAttrsToGroup",
    "moveGroupAttrsToElems",
    "collapseGroups",
    "removeEmptyContainers",
    "removeUnusedNS",
    "sortAttrs",
    "sortDefsChildren",
];

/// Run the strip subset. Minify / color / id plugins stay elsewhere.
///
/// `removeUnusedNS` runs before the sort passes: metadata + editor stripping
/// is what leaves dead `xmlns:*` on the root (high-ROI leftover). `sortAttrs`
/// / `sortDefsChildren` are last so gzip sees a stable final order.
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
    remove_unknowns_and_defaults(doc);
    remove_useless_defs(doc, false);
    remove_non_inheritable_group_attrs(doc);
    cleanup_enable_background(doc);
    remove_desc(doc);
    remove_empty_text(doc);
    remove_empty_containers(doc);
    remove_unused_ns(doc);
    sort_attrs(doc);
    sort_defs_children(doc);
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
/// `<filter>`. Root `version` is ignored by every modern SVG client — also
/// dropped by [`remove_unknowns_and_defaults`].
pub fn remove_deprecated_attrs(doc: &mut Document) {
    let has_filter = document_has_local_name(&doc.nodes, "filter");
    doc.walk_elements_mut(&mut |el| {
        if el.local_name() == "svg" {
            el.remove_attr("version");
        }
        el.retain_attrs(|name, value| keep_deprecated_attr(name, value, has_filter));
    });
}

/// `removeUnknownsAndDefaults` — default presentation attrs only.
///
/// Public [SVGO](https://svgo.dev/docs/plugins/removeUnknownsAndDefaults/)
/// contract also has `unknownContent` / `unknownAttrs`. We leave those **off**:
/// dropping unknown names can strip SVG2 / tool-specific bits. This pass
/// removes presentation attributes that equal the inherited (or initial)
/// [SVG 1.1](https://www.w3.org/TR/SVG11/propidx.html) value, including
/// useless overrides (`fill-rule="nonzero"` when that is already the used
/// value).
///
/// Policy diverge vs stock SVGO `keepRoleAttr: false`: **never** drop `role`
/// (including `role="img"`), `viewBox`, `xmlns` / `xmlns:*`, `aria-*`,
/// `data-*`, or `<title>`. Never deletes elements — SMIL
/// `visibility="hidden"` frames stay. Attributes targeted by a SMIL child
/// (or `href="#id"` animate) are left alone.
pub fn remove_unknowns_and_defaults(doc: &mut Document) {
    let has_stylesheet = document_has_local_name(&doc.nodes, "style");
    let mut remote = HashMap::new();
    collect_remote_smil_attr_targets(&doc.nodes, &mut remote);
    let inherited = svg_initial_presentation();
    for node in &mut doc.nodes {
        walk_unknowns_and_defaults(node, &inherited, has_stylesheet, &remote);
    }
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

/// `removeNonInheritableGroupAttrs` — drop presentation attrs on `<g>` that
/// neither inherit nor paint the group as a unit.
///
/// [SVGO](https://svgo.dev/docs/plugins/removeNonInheritableGroupAttrs/):
/// "Removes non-inheritable presentation attributes from groups."
/// [oxvg](https://docs.rs/oxvg_optimiser/latest/oxvg_optimiser/struct.RemoveNonInheritableGroupAttrs.html):
/// "should never visually change the document".
///
/// Inherit column is [SVG 1.1](https://www.w3.org/TR/SVG11/propidx.html).
/// `fill` / `stroke` / fonts stay (inheritable). `opacity` / `filter` /
/// `mask` / `clip-path` / `display` stay (apply to the group). Dead-on-group
/// primitives (`flood-color`, `stop-color`, `alignment-baseline`, …) go.
/// Attributes only — not `style=""` or `<style>` sheets.
pub fn remove_non_inheritable_group_attrs(doc: &mut Document) {
    doc.walk_elements_mut(&mut |el| {
        if !el.local_name().eq_ignore_ascii_case("g") {
            return;
        }
        el.retain_attrs(|name, _| !is_dead_on_group_presentation(name));
    });
}

/// `cleanupEnableBackground` — drop or shorten the deprecated filter attr.
///
/// [SVGO](https://svgo.dev/docs/plugins/cleanupEnableBackground/): attrs +
/// inline `style` only (not `<style>` sheets). [oxvg](https://docs.rs/oxvg_optimiser/latest/oxvg_optimiser/struct.CleanupEnableBackground.html):
/// drop when the document has no `<filter>`; drop on `<svg>` when `new 0 0 W H`
/// matches the node's width/height; replace with `new` on `<mask>` /
/// `<pattern>` in that same case.
pub fn cleanup_enable_background(doc: &mut Document) {
    let has_filter = document_has_local_name(&doc.nodes, "filter");
    for node in &mut doc.nodes {
        cleanup_enable_background_in(node, has_filter);
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

/// `moveElemsAttrsToGroup` subset: hoist identical inheritable presentation
/// attrs from **two or more** content children onto `<g>`.
///
/// [SVGO](https://svgo.dev/docs/plugins/moveElemsAttrsToGroup/): “Move an
/// elements attributes to their enclosing group.”
/// [oxvg](https://docs.rs/oxvg_optimiser/latest/oxvg_optimiser/struct.MoveElemsAttrsToGroup.html):
/// “should never visually change the document”.
///
/// Conservative vs stock SVGO: no `transform` / `opacity` / `style` hoist
/// (filter bbox + compositing). Skip groups with `filter` / `mask` /
/// `clip-path` / `clip` (attr, `style=""`, or those children). Skip a group
/// whose `id` is a motion hook (`keep_motion_ids`, SMIL `href` / `begin` /
/// `end`). Attributes only — not `<style>` sheets.
pub fn move_elems_attrs_to_group(doc: &mut Document, keep_motion_ids: bool) {
    let mut motion = HashSet::new();
    collect_motion_target_ids(&doc.nodes, &mut motion);
    hoist_common_group_attrs(&mut doc.nodes, &motion, keep_motion_ids);
}

/// `moveGroupAttrsToElems` subset: copy a `<g transform>` onto transformable
/// children (concat: group list, then the child’s), then drop it on the group.
///
/// [SVGO](https://svgo.dev/docs/plugins/moveGroupAttrsToElems/): “Move some
/// group attributes to the contained elements.”
/// [oxvg](https://docs.rs/oxvg_optimiser/latest/oxvg_optimiser/struct.MoveGroupAttrsToElems.html):
/// “should never visually change the document”.
///
/// No matrix bake (that is `convertTransform`). Skip filter / mask / clip
/// groups and motion-id groups. Skip when a SMIL child targets the group’s
/// `transform` (`animateTransform` / `attributeName=transform` without
/// `href`). Top-down so nested groups concat outer then inner.
pub fn move_group_attrs_to_elems(doc: &mut Document, keep_motion_ids: bool) {
    let mut motion = HashSet::new();
    collect_motion_target_ids(&doc.nodes, &mut motion);
    push_group_transforms(&mut doc.nodes, &motion, keep_motion_ids);
}

/// `collapseGroups` subset: unwrap `<g>` that has **no attributes**.
///
/// Transform / fill movers run first (`moveGroupAttrsToElems` /
/// `moveElemsAttrsToGroup`). After `cleanupIds` drops unused ids, id-only
/// country wrappers become attrless and flatten. Skipped on motion in
/// `run_default`.
pub fn collapse_groups(doc: &mut Document) {
    collapse_attrless_groups(&mut doc.nodes);
}

/// Default [`sortAttrs`] `order` — public SVGO / oxvg contract.
const SORT_ATTR_ORDER: &[&str] = &[
    "id", "width", "height", "x", "x1", "x2", "y", "y1", "y2", "cx", "cy", "r", "fill", "stroke",
    "marker", "d", "points",
];

/// `sortAttrs` — deterministic attribute order (gzip / brotli, not visual).
///
/// [SVGO](https://svgo.dev/docs/plugins/sortAttrs/): `xmlnsOrder: 'front'`
/// (default) puts XML namespace declarations first. Then the `order` list.
/// Keys not in that list sort alphabetically. `stroke-*` / `marker-*` follow
/// their prefix key so related paint attrs stay adjacent.
///
/// Does not add, drop, or rewrite values.
pub fn sort_attrs(doc: &mut Document) {
    doc.walk_elements_mut(&mut |el| {
        el.attrs.sort_by(|(a, _), (b, _)| cmp_attr_names(a, b));
    });
}

/// `sortDefsChildren` — group `<defs>` children for compression.
///
/// [SVGO](https://svgo.dev/docs/plugins/sortDefsChildren/): frequency, then
/// element-name length, then element name. Frequency is descending so the
/// largest same-tag cohort leads. Same-name siblings tie-break on `id`.
/// Comments / PIs stay ahead of elements, in source order.
///
/// Paint-neutral: SVG paint is by `id` / `url(#)`, not defs child order.
pub fn sort_defs_children(doc: &mut Document) {
    doc.walk_elements_mut(&mut |el| {
        if el.local_name().eq_ignore_ascii_case("defs") {
            sort_one_defs(el);
        }
    });
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

/// Presentation attrs that [SVG 1.1](https://www.w3.org/TR/SVG11/propidx.html)
/// marks **not inheritable** and that apply only to text / gradient stops /
/// filter primitives — never to a `<g>` as a painted unit.
fn is_dead_on_group_presentation(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "alignment-baseline"
            | "baseline-shift"
            | "dominant-baseline"
            | "flood-color"
            | "flood-opacity"
            | "lighting-color"
            | "stop-color"
            | "stop-opacity"
            | "text-decoration"
            | "unicode-bidi"
    )
}

fn cleanup_enable_background_in(node: &mut Node, has_filter: bool) {
    let Node::Element(el) = node else {
        return;
    };
    for child in &mut el.children {
        cleanup_enable_background_in(child, has_filter);
    }
    apply_enable_background(el, has_filter);
}

fn apply_enable_background(el: &mut Element, has_filter: bool) {
    if let Some(raw) = el.attr("enable-background").map(str::to_string) {
        match decide_enable_background(el, &raw, has_filter) {
            EnableBgAction::Drop => {
                el.remove_attr("enable-background");
            }
            EnableBgAction::Set(v) => {
                if v != raw {
                    el.set_attr("enable-background", v);
                }
            }
            EnableBgAction::Keep => {}
        }
    }
    if let Some(style) = el.attr("style").map(str::to_string) {
        let rewritten = map_style_enable_background(el, &style, has_filter);
        if rewritten.is_empty() {
            el.remove_attr("style");
        } else if rewritten != style {
            el.set_attr("style", rewritten);
        }
    }
}

enum EnableBgAction {
    Drop,
    Keep,
    Set(String),
}

fn decide_enable_background(el: &Element, raw: &str, has_filter: bool) -> EnableBgAction {
    if !has_filter {
        return EnableBgAction::Drop;
    }
    let parts: Vec<&str> = raw.split_whitespace().collect();
    if parts.is_empty() {
        return EnableBgAction::Drop;
    }
    if parts.len() == 1 && parts[0].eq_ignore_ascii_case("accumulate") {
        return EnableBgAction::Drop;
    }
    if parts.len() == 5 && parts[0].eq_ignore_ascii_case("new") {
        let w = parts[3];
        let h = parts[4];
        if dims_match_elem(el, w, h) {
            let local = el.local_name();
            if local.eq_ignore_ascii_case("svg") {
                return EnableBgAction::Drop;
            }
            if local.eq_ignore_ascii_case("mask")
                || local.eq_ignore_ascii_case("pattern")
                || local.eq_ignore_ascii_case("filter")
            {
                return EnableBgAction::Set("new".into());
            }
        }
    }
    EnableBgAction::Keep
}

fn dims_match_elem(el: &Element, w: &str, h: &str) -> bool {
    let Some(ew) = el.attr("width") else {
        return false;
    };
    let Some(eh) = el.attr("height") else {
        return false;
    };
    len_eq(ew, w) && len_eq(eh, h)
}

fn len_eq(a: &str, b: &str) -> bool {
    normalize_len(a) == normalize_len(b)
}

fn normalize_len(raw: &str) -> String {
    let folded = raw.trim().to_ascii_lowercase();
    let t = folded.strip_suffix("px").unwrap_or(&folded).trim();
    if let Ok(n) = t.parse::<f64>() {
        if n.is_finite() && n.fract() == 0.0 && n.abs() < (i64::MAX as f64) {
            return (n as i64).to_string();
        }
        if n.is_finite() {
            return n.to_string();
        }
    }
    t.to_string()
}

fn map_style_enable_background(el: &Element, style: &str, has_filter: bool) -> String {
    let mut out = Vec::new();
    for decl in style.split(';') {
        let decl = decl.trim();
        if decl.is_empty() {
            continue;
        }
        let Some((k, v)) = decl.split_once(':') else {
            out.push(decl.to_string());
            continue;
        };
        let key = k.trim();
        let val = v.trim();
        if !key.eq_ignore_ascii_case("enable-background") {
            out.push(format!("{key}:{val}"));
            continue;
        }
        match decide_enable_background(el, val, has_filter) {
            EnableBgAction::Drop => {}
            EnableBgAction::Keep => out.push(format!("{key}:{val}")),
            EnableBgAction::Set(new_v) => out.push(format!("{key}:{new_v}")),
        }
    }
    out.join(";")
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
    // Stock SVGO `removeHiddenElems` deletes SMIL `visibility="hidden"`
    // frames. We never do — those are later revealed by `<set>` / `<animate>`.
    if subtree_has_smil(el) {
        return false;
    }
    display_is_none(el) || visibility_is_hidden(el)
}

fn subtree_has_smil(el: &Element) -> bool {
    if is_smil_tag(el.local_name()) {
        return true;
    }
    el.children.iter().any(|n| match n {
        Node::Element(c) => subtree_has_smil(c),
        _ => false,
    })
}

fn is_smil_tag(local: &str) -> bool {
    matches!(
        local.to_ascii_lowercase().as_str(),
        "animate" | "animatetransform" | "animatemotion" | "animatecolor" | "set"
    )
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

fn is_xmlns_decl(name: &str) -> bool {
    name == "xmlns" || name.starts_with("xmlns:")
}

fn order_pos(name: &str) -> Option<usize> {
    SORT_ATTR_ORDER.iter().position(|key| {
        name == *key
            || (name.len() > key.len()
                && name.starts_with(key)
                && name.as_bytes()[key.len()] == b'-')
    })
}

fn attr_group_and_index(name: &str) -> (u8, usize) {
    if is_xmlns_decl(name) {
        let idx = if name == "xmlns" { 0 } else { 1 };
        return (0, idx);
    }
    if let Some(i) = order_pos(name) {
        return (1, i);
    }
    (2, 0)
}

fn cmp_attr_names(a: &str, b: &str) -> Ordering {
    attr_group_and_index(a)
        .cmp(&attr_group_and_index(b))
        .then_with(|| a.cmp(b))
}

fn sort_one_defs(el: &mut Element) {
    let mut freq: HashMap<String, usize> = HashMap::new();
    for child in &el.children {
        if let Node::Element(c) = child {
            *freq.entry(c.name.clone()).or_insert(0) += 1;
        }
    }
    el.children.sort_by(|a, b| match (a, b) {
        (Node::Element(ae), Node::Element(be)) => {
            let af = freq.get(&ae.name).copied().unwrap_or(0);
            let bf = freq.get(&be.name).copied().unwrap_or(0);
            bf.cmp(&af)
                .then_with(|| ae.name.len().cmp(&be.name.len()))
                .then_with(|| ae.name.cmp(&be.name))
                .then_with(|| ae.attr("id").cmp(&be.attr("id")))
        }
        (Node::Element(_), _) => Ordering::Greater,
        (_, Node::Element(_)) => Ordering::Less,
        _ => Ordering::Equal,
    });
}

fn hoist_common_group_attrs(nodes: &mut [Node], motion: &HashSet<String>, keep_motion_ids: bool) {
    for node in nodes.iter_mut() {
        let Node::Element(el) = node else {
            continue;
        };
        hoist_common_group_attrs(&mut el.children, motion, keep_motion_ids);
        if el.local_name().eq_ignore_ascii_case("g")
            && !skip_group_movers(el, motion, keep_motion_ids, false)
        {
            hoist_into_group(el);
        }
    }
}

fn hoist_into_group(group: &mut Element) {
    if group_has_raw_text(group) {
        return;
    }
    let kids: Vec<usize> = group
        .children
        .iter()
        .enumerate()
        .filter_map(|(i, n)| match n {
            Node::Element(el) if is_group_content_child(el) => Some(i),
            _ => None,
        })
        .collect();
    if kids.len() < 2 {
        return;
    }
    let Some(Node::Element(first)) = group.children.get(kids[0]) else {
        return;
    };
    let candidates: Vec<(String, String)> = first
        .attrs
        .iter()
        .filter(|(name, _)| is_hoistable_presentation(name))
        .map(|(name, value)| (name.clone(), value.clone()))
        .collect();
    for (name, value) in candidates {
        if !every_content_child_has(group, &kids, &name, &value) {
            continue;
        }
        if group.attr(&name).is_some_and(|existing| existing != value) {
            continue;
        }
        if style_has_prop(group, &name) {
            continue;
        }
        if kids.iter().any(|&i| match &group.children[i] {
            Node::Element(el) => style_has_prop(el, &name) || child_smil_targets_attr(el, &name),
            _ => false,
        }) {
            continue;
        }
        if group.attr(&name).is_none() {
            group.set_attr(&name, value);
        }
        for &i in &kids {
            if let Node::Element(el) = &mut group.children[i] {
                el.remove_attr(&name);
            }
        }
    }
}

fn every_content_child_has(group: &Element, kids: &[usize], name: &str, value: &str) -> bool {
    kids.iter().all(|&i| match &group.children[i] {
        Node::Element(el) => el.attr(name) == Some(value),
        _ => false,
    })
}

fn push_group_transforms(nodes: &mut [Node], motion: &HashSet<String>, keep_motion_ids: bool) {
    for node in nodes.iter_mut() {
        let Node::Element(el) = node else {
            continue;
        };
        if el.local_name().eq_ignore_ascii_case("g")
            && !skip_group_movers(el, motion, keep_motion_ids, true)
        {
            push_transform_to_children(el);
        }
        push_group_transforms(&mut el.children, motion, keep_motion_ids);
    }
}

fn push_transform_to_children(group: &mut Element) {
    let Some(group_tf) = group.attr("transform").map(str::to_string) else {
        return;
    };
    if group_tf.trim().is_empty() {
        return;
    }
    if style_has_prop(group, "transform") {
        return;
    }
    if group_has_raw_text(group) {
        return;
    }
    let mut idxs = Vec::new();
    for (i, child) in group.children.iter().enumerate() {
        let Node::Element(el) = child else {
            continue;
        };
        if is_smil_tag(el.local_name()) || is_ignored_group_child(el.local_name()) {
            continue;
        }
        if !is_transformable_elem(el.local_name()) || style_has_prop(el, "transform") {
            return;
        }
        idxs.push(i);
    }
    if idxs.is_empty() {
        return;
    }
    for i in idxs {
        let Node::Element(child) = &mut group.children[i] else {
            continue;
        };
        match child.attr("transform") {
            Some(existing) if !existing.trim().is_empty() => {
                child.set_attr("transform", format!("{group_tf} {existing}"));
            }
            _ => {
                child.set_attr("transform", group_tf.clone());
            }
        }
    }
    group.remove_attr("transform");
}

fn skip_group_movers(
    el: &Element,
    motion: &HashSet<String>,
    keep_motion_ids: bool,
    moving_transform: bool,
) -> bool {
    if group_has_filter_mask_clip(el) {
        return true;
    }
    if let Some(id) = el.attr("id") {
        if keep_motion_ids || motion.contains(id) {
            return true;
        }
    }
    if moving_transform && group_has_transform_smil(el) {
        return true;
    }
    false
}

fn group_has_filter_mask_clip(el: &Element) -> bool {
    for name in ["filter", "mask", "clip-path", "clip"] {
        if el.attr(name).is_some() || style_has_prop(el, name) {
            return true;
        }
    }
    el.children.iter().any(|n| match n {
        Node::Element(c) => matches!(c.local_name(), "filter" | "mask" | "clipPath" | "clip-path"),
        _ => false,
    })
}

fn group_has_transform_smil(el: &Element) -> bool {
    el.children.iter().any(|n| match n {
        Node::Element(c) if is_smil_tag(c.local_name()) => {
            let targets_self = c.attr("href").is_none() && c.attr("xlink:href").is_none();
            if !targets_self {
                return false;
            }
            c.local_name().eq_ignore_ascii_case("animatetransform")
                || c.attr("attributeName")
                    .is_some_and(|v| v.eq_ignore_ascii_case("transform"))
        }
        _ => false,
    })
}

fn group_has_raw_text(el: &Element) -> bool {
    el.children.iter().any(|n| match n {
        Node::Text(t) => !t.chars().all(char::is_whitespace),
        _ => false,
    })
}

fn is_group_content_child(el: &Element) -> bool {
    !is_smil_tag(el.local_name()) && !is_ignored_group_child(el.local_name())
}

fn is_ignored_group_child(local: &str) -> bool {
    matches!(
        local.to_ascii_lowercase().as_str(),
        "title" | "desc" | "metadata" | "style" | "script"
    )
}

fn is_hoistable_presentation(name: &str) -> bool {
    matches!(
        name,
        "fill"
            | "fill-opacity"
            | "fill-rule"
            | "stroke"
            | "stroke-opacity"
            | "stroke-width"
            | "stroke-linecap"
            | "stroke-linejoin"
            | "stroke-miterlimit"
            | "stroke-dasharray"
            | "stroke-dashoffset"
            | "color"
            | "font-family"
            | "font-size"
            | "font-weight"
            | "font-style"
            | "font-variant"
            | "letter-spacing"
            | "word-spacing"
            | "text-anchor"
            | "visibility"
            | "pointer-events"
            | "paint-order"
            | "clip-rule"
            | "marker-start"
            | "marker-mid"
            | "marker-end"
            | "shape-rendering"
            | "color-interpolation"
            | "color-rendering"
            | "image-rendering"
            | "text-rendering"
    )
}

fn is_transformable_elem(local: &str) -> bool {
    matches!(
        local.to_ascii_lowercase().as_str(),
        "a" | "circle"
            | "ellipse"
            | "foreignObject"
            | "g"
            | "image"
            | "line"
            | "path"
            | "polygon"
            | "polyline"
            | "rect"
            | "switch"
            | "text"
            | "textPath"
            | "tspan"
            | "use"
    )
}

fn style_has_prop(el: &Element, prop: &str) -> bool {
    let Some(style) = el.attr("style") else {
        return false;
    };
    style.split(';').any(|decl| {
        let Some((k, _)) = decl.split_once(':') else {
            return false;
        };
        k.trim().eq_ignore_ascii_case(prop)
    })
}

fn child_smil_targets_attr(el: &Element, attr: &str) -> bool {
    el.children.iter().any(|n| match n {
        Node::Element(c) if is_smil_tag(c.local_name()) => {
            let targets_self = c.attr("href").is_none() && c.attr("xlink:href").is_none();
            targets_self
                && c.attr("attributeName")
                    .is_some_and(|v| v.eq_ignore_ascii_case(attr))
        }
        _ => false,
    })
}

fn collect_motion_target_ids(nodes: &[Node], into: &mut HashSet<String>) {
    for node in nodes {
        let Node::Element(el) = node else {
            continue;
        };
        if is_smil_tag(el.local_name()) {
            for key in ["href", "xlink:href"] {
                if let Some(v) = el.attr(key) {
                    if let Some(id) = v.strip_prefix('#') {
                        if !id.is_empty() {
                            into.insert(id.to_string());
                        }
                    }
                }
            }
            for key in ["begin", "end"] {
                if let Some(v) = el.attr(key) {
                    for r in parse_smil_clock_value(v) {
                        into.insert(r.id);
                    }
                }
            }
        }
        collect_motion_target_ids(&el.children, into);
    }
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

// ---------------------------------------------------------------------------
// removeUnknownsAndDefaults (default attrs only)
// ---------------------------------------------------------------------------

/// Inheritable presentation attrs we may drop when they match the parent.
const INHERITABLE_DEFAULTS: &[&str] = &[
    "fill-rule",
    "clip-rule",
    "fill-opacity",
    "stroke-opacity",
    "stroke-dasharray",
    "stroke-dashoffset",
    "stroke-linecap",
    "stroke-linejoin",
    "stroke-miterlimit",
    "stroke-width",
    "visibility",
    "fill",
    "stroke",
    "font-weight",
    "font-style",
    "text-anchor",
    "letter-spacing",
    "word-spacing",
    "color",
    "marker",
    "marker-start",
    "marker-mid",
    "marker-end",
];

/// Not inherited — compare to the SVG initial value only.
const NON_INHERITABLE_DEFAULTS: &[&str] = &[
    "opacity",
    "display",
    "stop-opacity",
    "flood-opacity",
    "clip-path",
    "mask",
    "filter",
];

fn svg_initial_presentation() -> HashMap<String, String> {
    [
        ("fill-rule", "nonzero"),
        ("clip-rule", "nonzero"),
        ("fill-opacity", "1"),
        ("stroke-opacity", "1"),
        ("stroke-dasharray", "none"),
        ("stroke-dashoffset", "0"),
        ("stroke-linecap", "butt"),
        ("stroke-linejoin", "miter"),
        ("stroke-miterlimit", "4"),
        ("stroke-width", "1"),
        ("visibility", "visible"),
        ("fill", "black"),
        ("stroke", "none"),
        ("font-weight", "normal"),
        ("font-style", "normal"),
        ("text-anchor", "start"),
        ("letter-spacing", "normal"),
        ("word-spacing", "normal"),
        ("color", "black"),
        ("marker", "none"),
        ("marker-start", "none"),
        ("marker-mid", "none"),
        ("marker-end", "none"),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect()
}

fn walk_unknowns_and_defaults(
    node: &mut Node,
    inherited: &HashMap<String, String>,
    has_stylesheet: bool,
    remote: &HashMap<String, HashSet<String>>,
) {
    let Node::Element(el) = node else {
        return;
    };
    if el.local_name().eq_ignore_ascii_case("foreignObject") {
        return;
    }
    if is_smil_tag(el.local_name()) {
        for child in &mut el.children {
            walk_unknowns_and_defaults(child, inherited, has_stylesheet, remote);
        }
        return;
    }

    strip_default_presentation(el, inherited, has_stylesheet, remote);
    let next = inherit_presentation(el, inherited);
    for child in &mut el.children {
        walk_unknowns_and_defaults(child, &next, has_stylesheet, remote);
    }
}

fn strip_default_presentation(
    el: &mut Element,
    inherited: &HashMap<String, String>,
    has_stylesheet: bool,
    remote: &HashMap<String, HashSet<String>>,
) {
    let is_svg = el.local_name().eq_ignore_ascii_case("svg");
    let mut locked = el
        .attr("id")
        .and_then(|id| remote.get(id))
        .cloned()
        .unwrap_or_default();
    collect_local_smil_attr_targets(el, &mut locked);
    el.retain_attrs(|name, value| {
        keep_presentation_attr(name, value, is_svg, inherited, has_stylesheet, &locked)
    });
}

fn collect_local_smil_attr_targets(el: &Element, into: &mut HashSet<String>) {
    for child in &el.children {
        let Node::Element(c) = child else {
            continue;
        };
        if !is_smil_tag(c.local_name()) {
            continue;
        }
        if c.attr("href").is_some() || c.attr("xlink:href").is_some() {
            continue;
        }
        if let Some(name) = c.attr("attributeName") {
            into.insert(name.trim().to_ascii_lowercase());
        }
    }
}

fn keep_presentation_attr(
    name: &str,
    value: &str,
    is_svg: bool,
    inherited: &HashMap<String, String>,
    has_stylesheet: bool,
    locked: &HashSet<String>,
) -> bool {
    if is_protected_default_attr(name) {
        return true;
    }
    if is_svg && name.eq_ignore_ascii_case("version") {
        return false;
    }
    let folded = name.to_ascii_lowercase();
    if locked.contains(&folded) {
        return true;
    }
    if NON_INHERITABLE_DEFAULTS
        .iter()
        .any(|k| k.eq_ignore_ascii_case(name))
    {
        return !values_equivalent(&folded, value, initial_non_inheritable(&folded));
    }
    if INHERITABLE_DEFAULTS
        .iter()
        .any(|k| k.eq_ignore_ascii_case(name))
    {
        if has_stylesheet {
            return true;
        }
        let Some(inh) = inherited.get(&folded) else {
            return true;
        };
        return !values_equivalent(&folded, value, inh);
    }
    true
}

fn is_protected_default_attr(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n == "role"
        || n == "viewbox"
        || n == "xmlns"
        || n.starts_with("xmlns:")
        || n == "title"
        || n.starts_with("aria-")
        || n.starts_with("data-")
}

fn initial_non_inheritable(name: &str) -> &'static str {
    match name {
        "opacity" | "stop-opacity" | "flood-opacity" => "1",
        "display" => "inline",
        "clip-path" | "mask" | "filter" => "none",
        _ => "",
    }
}

fn inherit_presentation(el: &Element, parent: &HashMap<String, String>) -> HashMap<String, String> {
    let mut next = parent.clone();
    for key in INHERITABLE_DEFAULTS {
        if let Some(v) = specified_presentation(el, key) {
            next.insert((*key).to_string(), v);
        }
    }
    next
}

fn specified_presentation(el: &Element, name: &str) -> Option<String> {
    let raw = style_decl_value(el, name).or_else(|| el.attr(name).map(|s| s.trim().to_string()))?;
    if raw.eq_ignore_ascii_case("inherit") {
        return None;
    }
    Some(raw)
}

fn style_decl_value(el: &Element, prop: &str) -> Option<String> {
    let style = el.attr("style")?;
    let mut found = None;
    for decl in style.split(';') {
        let Some((k, v)) = decl.split_once(':') else {
            continue;
        };
        if k.trim().eq_ignore_ascii_case(prop) {
            let val = v.trim();
            if !val.is_empty() {
                found = Some(val.to_string());
            }
        }
    }
    found
}

fn collect_remote_smil_attr_targets(nodes: &[Node], into: &mut HashMap<String, HashSet<String>>) {
    for node in nodes {
        let Node::Element(el) = node else {
            continue;
        };
        if is_smil_tag(el.local_name()) {
            if let Some(attr) = el.attr("attributeName") {
                for key in ["href", "xlink:href"] {
                    if let Some(v) = el.attr(key) {
                        if let Some(id) = v.strip_prefix('#') {
                            if !id.is_empty() {
                                into.entry(id.to_string())
                                    .or_default()
                                    .insert(attr.trim().to_ascii_lowercase());
                            }
                        }
                    }
                }
            }
        }
        collect_remote_smil_attr_targets(&el.children, into);
    }
}

fn values_equivalent(name: &str, a: &str, b: &str) -> bool {
    let a = a.trim();
    let b = b.trim();
    if a.eq_ignore_ascii_case("inherit") {
        return true;
    }
    match name {
        "fill-opacity" | "stroke-opacity" | "stop-opacity" | "flood-opacity" | "opacity" => {
            (is_one_num(a) && is_one_num(b)) || numeric_len_eq(a, b)
        }
        "stroke-dashoffset" | "letter-spacing" | "word-spacing" => spacing_eq(a, b),
        "stroke-width" | "stroke-miterlimit" => numeric_len_eq(a, b),
        "fill" | "color" => {
            (is_default_black(a) && is_default_black(b)) || a.eq_ignore_ascii_case(b)
        }
        "stroke" | "clip-path" | "mask" | "filter" | "marker" | "marker-start" | "marker-mid"
        | "marker-end" => (is_none_kw(a) && is_none_kw(b)) || a.eq_ignore_ascii_case(b),
        "font-weight" => font_weight_eq(a, b),
        "stroke-dasharray" => (is_none_kw(a) && is_none_kw(b)) || a.eq_ignore_ascii_case(b),
        _ => a.eq_ignore_ascii_case(b),
    }
}

fn is_default_black(s: &str) -> bool {
    matches!(
        s.trim().to_ascii_lowercase().as_str(),
        "black" | "#000" | "#000000" | "#000000ff" | "#000f"
    )
}

fn is_none_kw(s: &str) -> bool {
    s.trim().eq_ignore_ascii_case("none")
}

fn font_weight_eq(a: &str, b: &str) -> bool {
    normalize_font_weight(a) == normalize_font_weight(b)
}

fn normalize_font_weight(s: &str) -> String {
    match s.trim().to_ascii_lowercase().as_str() {
        "normal" | "400" => "400".into(),
        "bold" | "700" => "700".into(),
        other => other.to_string(),
    }
}

fn spacing_eq(a: &str, b: &str) -> bool {
    let na = a.eq_ignore_ascii_case("normal") || is_zero_num(a);
    let nb = b.eq_ignore_ascii_case("normal") || is_zero_num(b);
    (na && nb) || numeric_len_eq(a, b) || a.eq_ignore_ascii_case(b)
}

fn is_one_num(s: &str) -> bool {
    let t = s.trim();
    if let Some(p) = t.strip_suffix('%') {
        return p
            .parse::<f64>()
            .ok()
            .is_some_and(|n| (n - 100.0).abs() < 0.05);
    }
    parse_user_unit_opt(t).is_some_and(|n| (n - 1.0).abs() < 0.001)
}

fn is_zero_num(s: &str) -> bool {
    parse_user_unit_opt(s).is_some_and(|n| n == 0.0)
}

fn numeric_len_eq(a: &str, b: &str) -> bool {
    match (parse_user_unit_opt(a), parse_user_unit_opt(b)) {
        (Some(x), Some(y)) => (x - y).abs() < 1e-6,
        _ => a.eq_ignore_ascii_case(b),
    }
}

fn parse_user_unit_opt(raw: &str) -> Option<f64> {
    let folded = raw.trim().to_ascii_lowercase();
    let t = folded.strip_suffix("px").unwrap_or(&folded).trim();
    t.parse::<f64>().ok().filter(|n| n.is_finite())
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
                        ("enable-background", "new"),
                    ],
                    vec![
                        node("metadata", &[], vec![]),
                        node("g", &[("id", "unused"), ("class", "")], vec![]),
                        node(
                            "g",
                            &[
                                ("fill", "#f00"),
                                ("flood-color", "blue"),
                                ("opacity", "0.5"),
                            ],
                            vec![node(
                                "circle",
                                &[
                                    ("cx", "1"),
                                    ("cy", "1"),
                                    ("r", "1"),
                                    ("fill-rule", "nonzero"),
                                ],
                                vec![],
                            )],
                        ),
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
        assert!(!has_attr(svg, "enable-background"));
        assert!(!descendant_has_name(svg, "metadata"));
        assert!(descendant_has_name(svg, "circle"));
        let painted = svg.children.iter().find_map(|n| match n {
            Node::Element(el) if el.local_name() == "g" => Some(el),
            _ => None,
        });
        let painted = painted.expect("painted group kept");
        assert_eq!(painted.attr("fill"), Some("#f00"));
        assert_eq!(painted.attr("opacity"), Some("0.5"));
        assert!(!has_attr(painted, "flood-color"));
        let circle = painted.children.iter().find_map(|n| match n {
            Node::Element(el) if el.local_name() == "circle" => Some(el),
            _ => None,
        });
        assert!(
            !has_attr(circle.expect("circle"), "fill-rule"),
            "default fill-rule=nonzero drops in run_strip"
        );
    }

    #[test]
    fn strip_plugin_names_match_svgo_ids() {
        assert!(STRIP_PLUGIN_NAMES.contains(&"removeUnusedNS"));
        assert!(STRIP_PLUGIN_NAMES.contains(&"removeEditorsNSData"));
        assert!(STRIP_PLUGIN_NAMES.contains(&"removeUselessDefs"));
        assert!(STRIP_PLUGIN_NAMES.contains(&"removeNonInheritableGroupAttrs"));
        assert!(STRIP_PLUGIN_NAMES.contains(&"cleanupEnableBackground"));
        assert!(STRIP_PLUGIN_NAMES.contains(&"removeDesc"));
        assert!(STRIP_PLUGIN_NAMES.contains(&"collapseGroups"));
        assert!(STRIP_PLUGIN_NAMES.contains(&"moveElemsAttrsToGroup"));
        assert!(STRIP_PLUGIN_NAMES.contains(&"moveGroupAttrsToElems"));
        assert!(STRIP_PLUGIN_NAMES.contains(&"sortAttrs"));
        assert!(STRIP_PLUGIN_NAMES.contains(&"sortDefsChildren"));
        assert!(STRIP_PLUGIN_NAMES.contains(&"removeUnknownsAndDefaults"));
        assert_eq!(STRIP_PLUGIN_NAMES[0], "removeDoctype");
        assert_eq!(*STRIP_PLUGIN_NAMES.last().unwrap(), "sortDefsChildren");
    }

    #[test]
    fn remove_unknowns_drops_fill_rule_nonzero_keeps_evenodd() {
        let mut doc = doc_svg(
            &[
                ("xmlns", "http://www.w3.org/2000/svg"),
                ("viewBox", "0 0 10 10"),
                ("role", "img"),
            ],
            vec![
                node("title", &[], vec![Node::Text("Logo".into())]),
                node(
                    "path",
                    &[
                        ("d", "M0 0h10v10z"),
                        ("fill-rule", "nonzero"),
                        ("clip-rule", "nonzero"),
                        ("opacity", "1"),
                        ("stroke-linecap", "butt"),
                    ],
                    vec![],
                ),
                node(
                    "path",
                    &[("d", "M1 1h2v2z"), ("fill-rule", "evenodd")],
                    vec![],
                ),
            ],
        );
        remove_unknowns_and_defaults(&mut doc);
        let svg = root(&doc);
        assert_eq!(svg.attr("role"), Some("img"));
        assert_eq!(svg.attr("viewBox"), Some("0 0 10 10"));
        assert_eq!(svg.attr("xmlns"), Some("http://www.w3.org/2000/svg"));
        assert!(descendant_has_name(svg, "title"));
        let paths: Vec<&Element> = svg
            .children
            .iter()
            .filter_map(|n| match n {
                Node::Element(el) if el.local_name() == "path" => Some(el),
                _ => None,
            })
            .collect();
        assert!(!has_attr(paths[0], "fill-rule"), "{:?}", paths[0].attrs);
        assert!(!has_attr(paths[0], "clip-rule"));
        assert!(!has_attr(paths[0], "opacity"));
        assert!(!has_attr(paths[0], "stroke-linecap"));
        assert_eq!(paths[1].attr("fill-rule"), Some("evenodd"));
    }

    #[test]
    fn remove_unknowns_keeps_nonzero_override_of_parent_evenodd() {
        let mut doc = doc_svg(
            &[],
            vec![node(
                "g",
                &[("fill-rule", "evenodd")],
                vec![
                    node("path", &[("d", "M0 0"), ("fill-rule", "nonzero")], vec![]),
                    node("path", &[("d", "M1 1"), ("fill-rule", "evenodd")], vec![]),
                ],
            )],
        );
        remove_unknowns_and_defaults(&mut doc);
        let g = match &root(&doc).children[0] {
            Node::Element(el) => el,
            _ => panic!("g"),
        };
        assert_eq!(g.attr("fill-rule"), Some("evenodd"));
        let kids: Vec<&Element> = g
            .children
            .iter()
            .filter_map(|n| match n {
                Node::Element(el) => Some(el),
                _ => None,
            })
            .collect();
        assert_eq!(
            kids[0].attr("fill-rule"),
            Some("nonzero"),
            "override of inherited evenodd must stay"
        );
        assert!(
            !has_attr(kids[1], "fill-rule"),
            "useless evenodd override drops"
        );
    }

    #[test]
    fn remove_unknowns_never_drops_role_viewbox_title_xmlns() {
        let mut doc = doc_svg(
            &[
                ("xmlns", "http://www.w3.org/2000/svg"),
                ("xmlns:xlink", "http://www.w3.org/1999/xlink"),
                ("viewBox", "0 0 24 24"),
                ("role", "img"),
                ("aria-hidden", "false"),
                ("data-icon", "github"),
                ("version", "1.1"),
            ],
            vec![node("title", &[], vec![Node::Text("GitHub".into())])],
        );
        remove_unknowns_and_defaults(&mut doc);
        let svg = root(&doc);
        assert_eq!(svg.attr("role"), Some("img"));
        assert_eq!(svg.attr("viewBox"), Some("0 0 24 24"));
        assert_eq!(svg.attr("xmlns"), Some("http://www.w3.org/2000/svg"));
        assert_eq!(
            svg.attr("xmlns:xlink"),
            Some("http://www.w3.org/1999/xlink")
        );
        assert_eq!(svg.attr("aria-hidden"), Some("false"));
        assert_eq!(svg.attr("data-icon"), Some("github"));
        assert!(!has_attr(svg, "version"));
        assert!(descendant_has_name(svg, "title"));
    }

    #[test]
    fn remove_unknowns_leaves_smil_hidden_frames_and_targeted_defaults() {
        let mut doc = doc_svg(
            &[],
            vec![
                node(
                    "g",
                    &[("visibility", "hidden")],
                    vec![node(
                        "rect",
                        &[
                            ("id", "frameA"),
                            ("visibility", "hidden"),
                            ("fill-rule", "nonzero"),
                            ("width", "10"),
                            ("height", "10"),
                        ],
                        vec![node(
                            "set",
                            &[
                                ("attributeName", "visibility"),
                                ("to", "visible"),
                                ("begin", "0s"),
                            ],
                            vec![],
                        )],
                    )],
                ),
                node(
                    "g",
                    &[("visibility", "visible")],
                    vec![node(
                        "rect",
                        &[
                            ("id", "frameB"),
                            ("opacity", "1"),
                            ("fill-rule", "nonzero"),
                            ("width", "10"),
                            ("height", "10"),
                        ],
                        vec![node(
                            "animate",
                            &[
                                ("attributeName", "opacity"),
                                ("values", "1;0"),
                                ("dur", "1s"),
                            ],
                            vec![],
                        )],
                    )],
                ),
                node(
                    "rect",
                    &[("id", "remote"), ("fill-rule", "nonzero"), ("width", "1")],
                    vec![],
                ),
                node(
                    "set",
                    &[
                        ("href", "#remote"),
                        ("attributeName", "fill-rule"),
                        ("to", "evenodd"),
                    ],
                    vec![],
                ),
            ],
        );
        remove_unknowns_and_defaults(&mut doc);
        let svg = root(&doc);
        let frame_a = find_id(svg, "frameA").expect("frameA");
        assert_eq!(
            frame_a.attr("visibility"),
            Some("hidden"),
            "SMIL hidden frame stays hidden"
        );
        assert!(descendant_has_name(frame_a, "set"), "SMIL child must stay");
        let hidden_g = svg.children.iter().find_map(|n| match n {
            Node::Element(el)
                if el.local_name() == "g" && el.attr("visibility") == Some("hidden") =>
            {
                Some(el)
            }
            _ => None,
        });
        assert!(hidden_g.is_some(), "hidden group is not a default — keep");
        let visible_g = svg.children.iter().find_map(|n| match n {
            Node::Element(el)
                if el.local_name() == "g" && el.attr("visibility") == Some("visible") =>
            {
                Some(el)
            }
            _ => None,
        });
        assert!(
            visible_g.is_none(),
            "visibility=visible is the initial value and may drop"
        );
        let frame_b = find_id(svg, "frameB").expect("frameB");
        assert_eq!(
            frame_b.attr("opacity"),
            Some("1"),
            "SMIL-targeted opacity stays even when 1"
        );
        assert!(!has_attr(frame_b, "fill-rule"));
        let remote = find_id(svg, "remote").expect("remote");
        assert_eq!(
            remote.attr("fill-rule"),
            Some("nonzero"),
            "href-targeted SMIL attr stays"
        );
    }

    #[test]
    fn remove_unknowns_skips_inheritable_when_stylesheet_present() {
        let mut doc = doc_svg(
            &[],
            vec![
                node(
                    "style",
                    &[],
                    vec![Node::Text("path{fill-rule:evenodd}".into())],
                ),
                node(
                    "path",
                    &[("d", "M0 0"), ("fill-rule", "nonzero"), ("opacity", "1")],
                    vec![],
                ),
            ],
        );
        remove_unknowns_and_defaults(&mut doc);
        let path = match root(&doc).children.iter().find_map(|n| match n {
            Node::Element(el) if el.local_name() == "path" => Some(el),
            _ => None,
        }) {
            Some(el) => el,
            None => panic!("path"),
        };
        assert_eq!(
            path.attr("fill-rule"),
            Some("nonzero"),
            "stylesheet can make inherited evenodd — keep the override"
        );
        assert!(
            !has_attr(path, "opacity"),
            "non-inheritable opacity:1 still drops"
        );
    }

    #[test]
    fn remove_unknowns_does_not_delete_unknown_or_custom_attrs() {
        let mut doc = doc_svg(
            &[],
            vec![node(
                "path",
                &[
                    ("d", "M0 0"),
                    ("foo", "bar"),
                    ("inkscape:label", "Layer"),
                    ("fill-rule", "nonzero"),
                ],
                vec![],
            )],
        );
        remove_unknowns_and_defaults(&mut doc);
        let path = match &root(&doc).children[0] {
            Node::Element(el) => el,
            _ => panic!("path"),
        };
        assert_eq!(path.attr("foo"), Some("bar"));
        assert_eq!(path.attr("inkscape:label"), Some("Layer"));
        assert!(!has_attr(path, "fill-rule"));
    }

    fn attr_names(el: &Element) -> Vec<&str> {
        el.attrs.iter().map(|(k, _)| k.as_str()).collect()
    }

    #[test]
    fn sort_attrs_xmlns_front_then_order_then_alpha() {
        let mut doc = doc_svg(
            &[
                ("viewBox", "0 0 10 10"),
                ("height", "10"),
                ("xmlns:xlink", "http://www.w3.org/1999/xlink"),
                ("width", "10"),
                ("id", "root"),
                ("xmlns", "http://www.w3.org/2000/svg"),
                ("class", "icon"),
            ],
            vec![node(
                "circle",
                &[
                    ("stroke-width", "2"),
                    ("fill", "#f00"),
                    ("r", "4"),
                    ("opacity", "0.8"),
                    ("cy", "5"),
                    ("stroke", "#000"),
                    ("cx", "5"),
                    ("id", "dot"),
                    ("marker-end", "url(#m)"),
                ],
                vec![],
            )],
        );
        sort_attrs(&mut doc);
        assert_eq!(
            attr_names(root(&doc)),
            [
                "xmlns",
                "xmlns:xlink",
                "id",
                "width",
                "height",
                "class",
                "viewBox",
            ]
        );
        let circle = match &root(&doc).children[0] {
            Node::Element(el) => el,
            _ => panic!("circle"),
        };
        assert_eq!(
            attr_names(circle),
            [
                "id",
                "cx",
                "cy",
                "r",
                "fill",
                "stroke",
                "stroke-width",
                "marker-end",
                "opacity",
            ]
        );
    }

    #[test]
    fn sort_attrs_source_order_does_not_matter() {
        let mut a = doc_svg(
            &[
                ("viewBox", "0 0 1 1"),
                ("xmlns", "http://www.w3.org/2000/svg"),
            ],
            vec![node(
                "rect",
                &[
                    ("fill", "red"),
                    ("height", "1"),
                    ("width", "1"),
                    ("x", "0"),
                    ("y", "0"),
                ],
                vec![],
            )],
        );
        let mut b = doc_svg(
            &[
                ("xmlns", "http://www.w3.org/2000/svg"),
                ("viewBox", "0 0 1 1"),
            ],
            vec![node(
                "rect",
                &[
                    ("y", "0"),
                    ("x", "0"),
                    ("width", "1"),
                    ("height", "1"),
                    ("fill", "red"),
                ],
                vec![],
            )],
        );
        sort_attrs(&mut a);
        sort_attrs(&mut b);
        assert_eq!(attr_names(root(&a)), attr_names(root(&b)));
        let ra = match &root(&a).children[0] {
            Node::Element(el) => attr_names(el),
            _ => panic!("rect a"),
        };
        let rb = match &root(&b).children[0] {
            Node::Element(el) => attr_names(el),
            _ => panic!("rect b"),
        };
        assert_eq!(ra, rb);
        assert_eq!(ra, ["width", "height", "x", "y", "fill"]);
    }

    #[test]
    fn sort_attrs_is_idempotent() {
        let mut doc = doc_svg(
            &[
                ("class", "z"),
                ("id", "a"),
                ("xmlns", "http://www.w3.org/2000/svg"),
            ],
            vec![],
        );
        sort_attrs(&mut doc);
        let once: Vec<String> = attr_names(root(&doc))
            .into_iter()
            .map(str::to_string)
            .collect();
        sort_attrs(&mut doc);
        let twice: Vec<String> = attr_names(root(&doc))
            .into_iter()
            .map(str::to_string)
            .collect();
        assert_eq!(once, twice);
        assert_eq!(once, ["xmlns", "id", "class"]);
    }

    #[test]
    fn sort_defs_children_frequency_then_len_then_name() {
        let mut doc = doc_svg(
            &[],
            vec![node(
                "defs",
                &[],
                vec![
                    Node::Comment("! keep".into()),
                    node("clipPath", &[("id", "c")], vec![]),
                    node("linearGradient", &[("id", "g1")], vec![]),
                    node("mask", &[("id", "m")], vec![]),
                    node("linearGradient", &[("id", "g2")], vec![]),
                    node("filter", &[("id", "f")], vec![]),
                    node("linearGradient", &[("id", "g0")], vec![]),
                ],
            )],
        );
        sort_defs_children(&mut doc);
        let defs = match &root(&doc).children[0] {
            Node::Element(el) => el,
            _ => panic!("defs"),
        };
        assert!(matches!(&defs.children[0], Node::Comment(c) if c == "! keep"));
        assert_eq!(
            names_under(defs),
            [
                "linearGradient",
                "linearGradient",
                "linearGradient",
                "mask",
                "filter",
                "clipPath",
            ]
        );
        let ids: Vec<_> = defs
            .children
            .iter()
            .filter_map(|n| match n {
                Node::Element(el) => el.attr("id"),
                _ => None,
            })
            .collect();
        assert_eq!(ids, ["g0", "g1", "g2", "m", "f", "c"]);
    }

    #[test]
    fn sort_defs_children_source_order_does_not_matter() {
        let kids_a = vec![
            node("mask", &[("id", "m")], vec![]),
            node("clipPath", &[("id", "c")], vec![]),
            node("linearGradient", &[("id", "g")], vec![]),
        ];
        let kids_b = vec![
            node("linearGradient", &[("id", "g")], vec![]),
            node("clipPath", &[("id", "c")], vec![]),
            node("mask", &[("id", "m")], vec![]),
        ];
        let mut a = doc_svg(&[], vec![node("defs", &[], kids_a)]);
        let mut b = doc_svg(&[], vec![node("defs", &[], kids_b)]);
        sort_defs_children(&mut a);
        sort_defs_children(&mut b);
        let da = match &root(&a).children[0] {
            Node::Element(el) => names_under(el),
            _ => panic!("defs a"),
        };
        let db = match &root(&b).children[0] {
            Node::Element(el) => names_under(el),
            _ => panic!("defs b"),
        };
        assert_eq!(da, db);
        // freq=1 each: shorter name first, then alpha — mask(4), filter would
        // be 6; clipPath(8). mask < clipPath by length; linearGradient is 15.
        assert_eq!(da, ["mask", "clipPath", "linearGradient"]);
    }

    #[test]
    fn sort_defs_children_is_idempotent() {
        let mut doc = doc_svg(
            &[],
            vec![node(
                "defs",
                &[],
                vec![
                    node("clipPath", &[("id", "c")], vec![]),
                    node("mask", &[("id", "m")], vec![]),
                ],
            )],
        );
        sort_defs_children(&mut doc);
        let once: Vec<String> = match &root(&doc).children[0] {
            Node::Element(el) => names_under(el).into_iter().map(str::to_string).collect(),
            _ => panic!("defs"),
        };
        sort_defs_children(&mut doc);
        let twice: Vec<String> = match &root(&doc).children[0] {
            Node::Element(el) => names_under(el).into_iter().map(str::to_string).collect(),
            _ => panic!("defs"),
        };
        assert_eq!(once, twice);
        assert_eq!(once, ["mask", "clipPath"]);
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

    #[test]
    fn remove_non_inheritable_group_attrs_drops_dead_keeps_paint() {
        let mut doc = doc_svg(
            &[],
            vec![
                node(
                    "g",
                    &[
                        ("id", "layer"),
                        ("class", "land"),
                        ("fill", "#0a0"),
                        ("stroke", "red"),
                        ("opacity", "0.8"),
                        ("filter", "url(#glow)"),
                        ("mask", "url(#m)"),
                        ("clip-path", "url(#c)"),
                        ("display", "inline"),
                        ("flood-color", "lime"),
                        ("flood-opacity", "0.2"),
                        ("lighting-color", "white"),
                        ("stop-color", "black"),
                        ("stop-opacity", "1"),
                        ("alignment-baseline", "middle"),
                        ("baseline-shift", "10"),
                        ("dominant-baseline", "central"),
                        ("text-decoration", "underline"),
                        ("unicode-bidi", "embed"),
                    ],
                    vec![node("path", &[("d", "M0 0")], vec![])],
                ),
                node(
                    "rect",
                    &[("flood-color", "pink"), ("width", "2"), ("height", "2")],
                    vec![],
                ),
            ],
        );
        remove_non_inheritable_group_attrs(&mut doc);
        let svg = root(&doc);
        let g = svg.children.iter().find_map(|n| match n {
            Node::Element(el) if el.local_name() == "g" => Some(el),
            _ => None,
        });
        let g = g.expect("group");
        assert_eq!(g.attr("id"), Some("layer"));
        assert_eq!(g.attr("class"), Some("land"));
        assert_eq!(g.attr("fill"), Some("#0a0"));
        assert_eq!(g.attr("stroke"), Some("red"));
        assert_eq!(g.attr("opacity"), Some("0.8"));
        assert_eq!(g.attr("filter"), Some("url(#glow)"));
        assert_eq!(g.attr("mask"), Some("url(#m)"));
        assert_eq!(g.attr("clip-path"), Some("url(#c)"));
        assert_eq!(g.attr("display"), Some("inline"));
        for dead in [
            "flood-color",
            "flood-opacity",
            "lighting-color",
            "stop-color",
            "stop-opacity",
            "alignment-baseline",
            "baseline-shift",
            "dominant-baseline",
            "text-decoration",
            "unicode-bidi",
        ] {
            assert!(!has_attr(g, dead), "{dead} should leave the group");
        }
        let rect = svg.children.iter().find_map(|n| match n {
            Node::Element(el) if el.local_name() == "rect" => Some(el),
            _ => None,
        });
        let rect = rect.expect("rect");
        assert_eq!(
            rect.attr("flood-color"),
            Some("pink"),
            "non-group presentation stays"
        );
    }

    #[test]
    fn cleanup_enable_background_drops_without_filter() {
        let mut doc = doc_svg(
            &[
                ("width", "24"),
                ("height", "24"),
                ("enable-background", "new 0 0 24 24"),
            ],
            vec![node(
                "g",
                &[("style", "opacity:0.5;enable-background:new")],
                vec![node("circle", &[("r", "2")], vec![])],
            )],
        );
        cleanup_enable_background(&mut doc);
        let svg = root(&doc);
        assert!(!has_attr(svg, "enable-background"));
        let g = match &svg.children[0] {
            Node::Element(el) => el,
            _ => panic!("g"),
        };
        assert_eq!(g.attr("style"), Some("opacity:0.5"));
        assert!(!has_attr(g, "enable-background"));
    }

    #[test]
    fn cleanup_enable_background_shortens_mask_when_filter_exists() {
        let mut doc = doc_svg(
            &[
                ("width", "100"),
                ("height", "50"),
                ("enable-background", "new 0 0 100 50"),
            ],
            vec![
                node("filter", &[("id", "blur")], vec![]),
                node(
                    "mask",
                    &[
                        ("id", "m"),
                        ("width", "10px"),
                        ("height", "10"),
                        ("enable-background", "new 0 0 10 10"),
                    ],
                    vec![],
                ),
                node(
                    "pattern",
                    &[
                        ("id", "p"),
                        ("width", "8"),
                        ("height", "8"),
                        ("style", "enable-background:new 0 0 8 8"),
                    ],
                    vec![],
                ),
                node(
                    "g",
                    &[("enable-background", "new 0 0 99 99")],
                    vec![node("rect", &[("width", "1")], vec![])],
                ),
            ],
        );
        cleanup_enable_background(&mut doc);
        let svg = root(&doc);
        assert!(
            !has_attr(svg, "enable-background"),
            "svg canvas-sized value drops"
        );
        let mask = find_id(svg, "m").expect("mask");
        assert_eq!(mask.attr("enable-background"), Some("new"));
        let pattern = find_id(svg, "p").expect("pattern");
        assert_eq!(pattern.attr("style"), Some("enable-background:new"));
        let g = svg.children.iter().find_map(|n| match n {
            Node::Element(el) if el.local_name() == "g" => Some(el),
            _ => None,
        });
        assert_eq!(
            g.expect("g").attr("enable-background"),
            Some("new 0 0 99 99"),
            "unmatched leftover stays when a filter exists"
        );
    }

    #[test]
    fn remove_hidden_keeps_smil_visibility_frames() {
        let mut doc = doc_svg(
            &[],
            vec![
                node(
                    "rect",
                    &[
                        ("id", "frameA"),
                        ("visibility", "hidden"),
                        ("width", "10"),
                        ("height", "10"),
                    ],
                    vec![node(
                        "set",
                        &[
                            ("attributeName", "visibility"),
                            ("to", "visible"),
                            ("begin", "0s"),
                            ("dur", "1s"),
                        ],
                        vec![],
                    )],
                ),
                node(
                    "rect",
                    &[
                        ("id", "frameB"),
                        ("visibility", "hidden"),
                        ("width", "10"),
                        ("height", "10"),
                    ],
                    vec![node(
                        "animate",
                        &[
                            ("attributeName", "visibility"),
                            ("values", "hidden;visible"),
                            ("dur", "1s"),
                        ],
                        vec![],
                    )],
                ),
                node(
                    "g",
                    &[("display", "none")],
                    vec![node("circle", &[("r", "1")], vec![])],
                ),
            ],
        );
        remove_hidden_elems(&mut doc);
        let svg = root(&doc);
        assert!(find_id(svg, "frameA").is_some(), "SMIL hidden frame A");
        assert!(find_id(svg, "frameB").is_some(), "SMIL hidden frame B");
        assert!(
            descendant_has_name(svg, "set") && descendant_has_name(svg, "animate"),
            "SMIL children stay"
        );
        assert!(
            !svg.children.iter().any(|n| match n {
                Node::Element(el) => {
                    el.local_name() == "g" && el.attr("display") == Some("none")
                }
                _ => false,
            }),
            "plain display:none junk still drops"
        );
    }

    fn first_g(el: &Element) -> &Element {
        el.children
            .iter()
            .find_map(|n| match n {
                Node::Element(child) if child.local_name() == "g" => Some(child),
                _ => None,
            })
            .expect("group")
    }

    #[test]
    fn move_elems_hoists_shared_fill_and_stroke() {
        let mut doc = doc_svg(
            &[],
            vec![node(
                "g",
                &[],
                vec![
                    node(
                        "path",
                        &[("d", "M0 0"), ("fill", "#f00"), ("stroke", "#00f")],
                        vec![],
                    ),
                    node(
                        "circle",
                        &[("r", "2"), ("fill", "#f00"), ("stroke", "#00f")],
                        vec![],
                    ),
                ],
            )],
        );
        move_elems_attrs_to_group(&mut doc, false);
        let g = first_g(root(&doc));
        assert_eq!(g.attr("fill"), Some("#f00"));
        assert_eq!(g.attr("stroke"), Some("#00f"));
        for child in &g.children {
            let Node::Element(el) = child else {
                continue;
            };
            assert!(!has_attr(el, "fill"), "{}", el.name);
            assert!(!has_attr(el, "stroke"), "{}", el.name);
        }
    }

    #[test]
    fn move_elems_skips_mixed_fill_and_single_child() {
        let mut mixed = doc_svg(
            &[],
            vec![node(
                "g",
                &[],
                vec![
                    node("path", &[("d", "M0 0"), ("fill", "#f00")], vec![]),
                    node("circle", &[("r", "2"), ("fill", "#0f0")], vec![]),
                ],
            )],
        );
        move_elems_attrs_to_group(&mut mixed, false);
        let g = first_g(root(&mixed));
        assert!(!has_attr(g, "fill"));
        assert_eq!(
            g.children
                .iter()
                .filter_map(|n| match n {
                    Node::Element(el) => el.attr("fill"),
                    _ => None,
                })
                .collect::<Vec<_>>(),
            ["#f00", "#0f0"]
        );

        let mut one = doc_svg(
            &[],
            vec![node(
                "g",
                &[],
                vec![node("circle", &[("r", "2"), ("fill", "#f00")], vec![])],
            )],
        );
        move_elems_attrs_to_group(&mut one, false);
        let g = first_g(root(&one));
        assert!(!has_attr(g, "fill"));
        assert_eq!(
            match &g.children[0] {
                Node::Element(el) => el.attr("fill"),
                _ => None,
            },
            Some("#f00")
        );
    }

    #[test]
    fn move_elems_skips_filter_mask_clip_and_style() {
        for (attr, value) in [
            ("filter", "url(#blur)"),
            ("mask", "url(#m)"),
            ("clip-path", "url(#c)"),
            ("clip", "rect(0 0 1 1)"),
        ] {
            let mut doc = doc_svg(
                &[],
                vec![node(
                    "g",
                    &[(attr, value)],
                    vec![
                        node("path", &[("d", "M0 0"), ("fill", "#f00")], vec![]),
                        node("circle", &[("r", "1"), ("fill", "#f00")], vec![]),
                    ],
                )],
            );
            move_elems_attrs_to_group(&mut doc, false);
            let g = first_g(root(&doc));
            assert!(!has_attr(g, "fill"), "{attr} group must not hoist");
            assert_eq!(g.attr(attr), Some(value));
        }

        let mut styled = doc_svg(
            &[],
            vec![node(
                "g",
                &[("style", "filter:url(#blur)")],
                vec![
                    node("path", &[("d", "M0 0"), ("fill", "#f00")], vec![]),
                    node("circle", &[("r", "1"), ("fill", "#f00")], vec![]),
                ],
            )],
        );
        move_elems_attrs_to_group(&mut styled, false);
        assert!(!has_attr(first_g(root(&styled)), "fill"));
    }

    #[test]
    fn move_elems_skips_motion_ids_and_does_not_hoist_transform() {
        let mut motion = doc_svg(
            &[],
            vec![
                node(
                    "g",
                    &[("id", "layer")],
                    vec![
                        node("path", &[("d", "M0 0"), ("fill", "#f00")], vec![]),
                        node("circle", &[("r", "1"), ("fill", "#f00")], vec![]),
                    ],
                ),
                node(
                    "g",
                    &[("id", "hook")],
                    vec![
                        node("path", &[("d", "M1 1"), ("fill", "#0f0")], vec![]),
                        node("circle", &[("r", "2"), ("fill", "#0f0")], vec![]),
                    ],
                ),
                node(
                    "animate",
                    &[("href", "#hook"), ("attributeName", "opacity")],
                    vec![],
                ),
                node(
                    "g",
                    &[("id", "sync")],
                    vec![
                        node("path", &[("d", "M2 2"), ("fill", "#00f")], vec![]),
                        node("circle", &[("r", "3"), ("fill", "#00f")], vec![]),
                    ],
                ),
                node("animate", &[("begin", "sync.end+0.2s")], vec![]),
            ],
        );
        move_elems_attrs_to_group(&mut motion, true);
        let svg = root(&motion);
        assert!(
            !has_attr(find_id(svg, "layer").expect("layer"), "fill"),
            "keep_motion_ids skips every id'd group"
        );
        assert!(!has_attr(find_id(svg, "hook").expect("hook"), "fill"));
        assert!(!has_attr(find_id(svg, "sync").expect("sync"), "fill"));

        let mut static_doc = doc_svg(
            &[],
            vec![node(
                "g",
                &[("id", "plain")],
                vec![
                    node("path", &[("d", "M0 0"), ("fill", "#f00")], vec![]),
                    node("circle", &[("r", "1"), ("fill", "#f00")], vec![]),
                ],
            )],
        );
        move_elems_attrs_to_group(&mut static_doc, false);
        assert_eq!(
            find_id(root(&static_doc), "plain")
                .expect("plain")
                .attr("fill"),
            Some("#f00"),
            "static unused id may hoist"
        );

        let mut tf = doc_svg(
            &[],
            vec![node(
                "g",
                &[],
                vec![
                    node(
                        "path",
                        &[("d", "M0 0"), ("transform", "scale(2)"), ("opacity", "0.5")],
                        vec![],
                    ),
                    node(
                        "circle",
                        &[("r", "1"), ("transform", "scale(2)"), ("opacity", "0.5")],
                        vec![],
                    ),
                ],
            )],
        );
        move_elems_attrs_to_group(&mut tf, false);
        let g = first_g(root(&tf));
        assert!(!has_attr(g, "transform"));
        assert!(!has_attr(g, "opacity"));
    }

    #[test]
    fn move_group_pushes_transform_and_concats() {
        let mut doc = doc_svg(
            &[],
            vec![node(
                "g",
                &[("transform", "translate(1 2)")],
                vec![
                    node("circle", &[("r", "2")], vec![]),
                    node("path", &[("d", "M0 0"), ("transform", "scale(2)")], vec![]),
                ],
            )],
        );
        move_group_attrs_to_elems(&mut doc, false);
        let g = first_g(root(&doc));
        assert!(!has_attr(g, "transform"));
        let kids: Vec<&Element> = g
            .children
            .iter()
            .filter_map(|n| match n {
                Node::Element(el) => Some(el),
                _ => None,
            })
            .collect();
        assert_eq!(kids[0].attr("transform"), Some("translate(1 2)"));
        assert_eq!(kids[1].attr("transform"), Some("translate(1 2) scale(2)"));
    }

    #[test]
    fn move_group_skips_filter_mask_clip_and_motion() {
        let mut filtered = doc_svg(
            &[],
            vec![node(
                "g",
                &[("transform", "translate(1 2)"), ("filter", "url(#blur)")],
                vec![node("circle", &[("r", "2")], vec![])],
            )],
        );
        move_group_attrs_to_elems(&mut filtered, false);
        let g = first_g(root(&filtered));
        assert_eq!(g.attr("transform"), Some("translate(1 2)"));
        assert!(!has_attr(
            match &g.children[0] {
                Node::Element(el) => el,
                _ => panic!("circle"),
            },
            "transform"
        ));

        let mut motion = doc_svg(
            &[],
            vec![node(
                "g",
                &[("id", "spin"), ("transform", "rotate(10)")],
                vec![node("circle", &[("r", "2")], vec![])],
            )],
        );
        move_group_attrs_to_elems(&mut motion, true);
        assert_eq!(
            find_id(root(&motion), "spin")
                .expect("spin")
                .attr("transform"),
            Some("rotate(10)")
        );

        let mut smil = doc_svg(
            &[],
            vec![node(
                "g",
                &[("transform", "translate(3 4)")],
                vec![
                    node("circle", &[("r", "2")], vec![]),
                    node(
                        "animateTransform",
                        &[
                            ("attributeName", "transform"),
                            ("type", "rotate"),
                            ("dur", "1s"),
                        ],
                        vec![],
                    ),
                ],
            )],
        );
        move_group_attrs_to_elems(&mut smil, false);
        assert_eq!(
            first_g(root(&smil)).attr("transform"),
            Some("translate(3 4)"),
            "SMIL transform on the group stays put"
        );
    }

    #[test]
    fn movers_then_collapse_unwraps_translated_group() {
        let mut doc = doc_svg(
            &[],
            vec![node(
                "g",
                &[("transform", "translate(1 2)")],
                vec![node("circle", &[("r", "3"), ("fill", "#0a0")], vec![])],
            )],
        );
        move_elems_attrs_to_group(&mut doc, false);
        move_group_attrs_to_elems(&mut doc, false);
        collapse_groups(&mut doc);
        let svg = root(&doc);
        assert!(!names_under(svg).contains(&"g"));
        let circle = match &svg.children[0] {
            Node::Element(el) => el,
            _ => panic!("circle"),
        };
        assert_eq!(circle.local_name(), "circle");
        assert_eq!(circle.attr("transform"), Some("translate(1 2)"));
        assert_eq!(circle.attr("fill"), Some("#0a0"));
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
