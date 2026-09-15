//! Animation detection and motion-safe plugin planning.
//!
//! Differentiator vs stock SVGO defaults: when a document is motion-sensitive
//! we **skip** passes that historically break SMIL / CSS / external JS hooks
//! (`cleanupIds` rename/drop, `mergePaths`, `convertShapeToPath`,
//! `collapseGroups`, aggressive path rounding). Embedded-raster recompress
//! and wrapper minify still run — a tiny `<animate>` must not freeze a
//! multi-megabyte `data:` PNG/JPEG. We do **not** prefix IDs.
//! If a caller ever prefixes, [`rewrite_smil_clock_value`] updates
//! `begin`/`end` tokens such as `y1.end+.33s` (SVGO #848 / #2073 / #2207).
//!
//! Sibling-directory JS/TS/CSS walk is **not** automatic. Pass extra file
//! texts via [`Config::extra_sources`]. A full workspace scan is TODO.

use crate::ast::{Document, Element, Node};
use std::collections::{BTreeSet, HashMap};

/// Plugins we refuse to run (or would refuse, if implemented) on motion docs.
pub const MOTION_SKIP_PLUGINS: &[&str] = &[
    "cleanupIds",
    "mergePaths",
    "convertShapeToPath",
    "collapseGroups",
    "removeHiddenElems",
    "numericRounding",
    "convertPathData",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum AnimationKind {
    Smil,
    CssKeyframes,
    Script,
    Events,
    ExternalIds,
}

impl AnimationKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Smil => "smil",
            Self::CssKeyframes => "css-keyframes",
            Self::Script => "script",
            Self::Events => "events",
            Self::ExternalIds => "external-ids",
        }
    }
}

#[derive(Debug, Clone)]
pub struct AnimationReport {
    pub motion_sensitive: bool,
    pub kinds: Vec<AnimationKind>,
    pub plugins_skipped: Vec<&'static str>,
    pub input_ids: u32,
    pub ids_preserved: u32,
    pub external_ids: Vec<String>,
    pub smil_sync_refs: Vec<String>,
    /// `request-extras` | `not-run` | `todo-sibling-dir`
    pub workspace_scan: String,
    pub summary: String,
}

impl Default for AnimationReport {
    fn default() -> Self {
        Self {
            motion_sensitive: false,
            kinds: vec![],
            plugins_skipped: vec![],
            input_ids: 0,
            ids_preserved: 0,
            external_ids: vec![],
            smil_sync_refs: vec![],
            workspace_scan: "not-run".into(),
            summary: String::new(),
        }
    }
}

impl AnimationReport {
    pub fn kind_labels(&self) -> Vec<&'static str> {
        self.kinds.iter().map(|k| k.as_str()).collect()
    }
}

#[derive(Debug, Clone, Default)]
pub struct PluginPlan {
    pub skip_cleanup_ids: bool,
}

pub fn plan_from_report(report: &AnimationReport, animation_aware: bool) -> PluginPlan {
    PluginPlan {
        skip_cleanup_ids: animation_aware && report.motion_sensitive,
    }
}

pub fn skipped_plugins(report: &AnimationReport, animation_aware: bool) -> Vec<&'static str> {
    if animation_aware && report.motion_sensitive {
        MOTION_SKIP_PLUGINS.to_vec()
    } else {
        Vec::new()
    }
}

/// Detect motion from a parsed tree plus optional sibling source texts.
pub fn detect_document(doc: &Document, extra_sources: &[(String, String)]) -> AnimationReport {
    let mut kinds = BTreeSet::new();
    let mut smil_sync = BTreeSet::new();
    walk_detect(&doc.nodes, &mut kinds, &mut smil_sync);

    let (mut extra_ids, scan_note) = if extra_sources.is_empty() {
        (Vec::new(), "not-run".to_string())
    } else {
        (
            scan_sources_for_id_refs(extra_sources),
            "request-extras".to_string(),
        )
    };
    let present: BTreeSet<String> = collect_id_names(&doc.nodes);
    extra_ids.retain(|id| present.contains(id));
    if !extra_ids.is_empty() {
        kinds.insert(AnimationKind::ExternalIds);
    }

    let motion_sensitive = !kinds.is_empty();
    AnimationReport {
        motion_sensitive,
        kinds: kinds.into_iter().collect(),
        plugins_skipped: Vec::new(),
        input_ids: count_ids_in_nodes(&doc.nodes),
        ids_preserved: 0,
        external_ids: extra_ids,
        smil_sync_refs: smil_sync.into_iter().collect(),
        workspace_scan: scan_note,
        summary: String::new(),
    }
}

pub fn finish_report(
    report: &mut AnimationReport,
    name: &str,
    input_bytes: usize,
    output: &str,
    animation_aware: bool,
) {
    report.plugins_skipped = skipped_plugins(report, animation_aware);
    report.ids_preserved = count_id_attrs(output);
    report.summary = format_summary(name, input_bytes, output.len(), report);
}

pub fn format_summary(
    name: &str,
    input_bytes: usize,
    output_bytes: usize,
    report: &AnimationReport,
) -> String {
    let delta = output_bytes as i64 - input_bytes as i64;
    let size = format_delta_bytes(delta);
    let skip = if report.plugins_skipped.is_empty() {
        "none".to_string()
    } else {
        report.plugins_skipped.join(", ")
    };
    format!(
        "optimized {name} {size} · preserved {} ids · skipped {skip}",
        report.ids_preserved
    )
}

fn format_delta_bytes(delta: i64) -> String {
    if delta == 0 {
        return "±0B".into();
    }
    let sign = if delta < 0 { "−" } else { "+" };
    let abs = delta.unsigned_abs();
    if abs < 1024 {
        format!("{sign}{abs}B")
    } else if abs < 1024 * 1024 {
        format!("{sign}{:.1}KB", abs as f64 / 1024.0)
    } else {
        format!("{sign}{:.2}MB", abs as f64 / (1024.0 * 1024.0))
    }
}

pub fn count_id_attrs(svg: &str) -> u32 {
    let mut n = 0u32;
    let bytes = svg.as_bytes();
    let mut i = 0;
    while i + 4 <= bytes.len() {
        let three = &bytes[i..i + 3];
        if (three.eq_ignore_ascii_case(b"id=")) && matches!(bytes[i + 3], b'"' | b'\'') {
            let boundary = i == 0 || !bytes[i - 1].is_ascii_alphanumeric();
            if boundary {
                n = n.saturating_add(1);
            }
        }
        i += 1;
    }
    n
}

fn collect_id_names(nodes: &[Node]) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    walk_id_names(nodes, &mut out);
    out
}

fn walk_id_names(nodes: &[Node], out: &mut BTreeSet<String>) {
    for node in nodes {
        if let Node::Element(el) = node {
            if let Some(id) = el.attr("id") {
                out.insert(id.to_string());
            }
            walk_id_names(&el.children, out);
        }
    }
}

fn count_ids_in_nodes(nodes: &[Node]) -> u32 {
    let mut n = 0u32;
    for node in nodes {
        if let Node::Element(el) = node {
            if el.attr("id").is_some() {
                n = n.saturating_add(1);
            }
            n = n.saturating_add(count_ids_in_nodes(&el.children));
        }
    }
    n
}

const SMIL_TAGS: &[&str] = &[
    "animate",
    "animatetransform",
    "animatemotion",
    "animatecolor",
    "set",
];

fn walk_detect(nodes: &[Node], kinds: &mut BTreeSet<AnimationKind>, smil: &mut BTreeSet<String>) {
    for node in nodes {
        let Node::Element(el) = node else {
            continue;
        };
        let local = el.local_name().to_ascii_lowercase();
        if SMIL_TAGS.contains(&local.as_str()) {
            kinds.insert(AnimationKind::Smil);
            if let Some(v) = el.attr("begin") {
                collect_smil_sync_ids(v, smil);
            }
            if let Some(v) = el.attr("end") {
                collect_smil_sync_ids(v, smil);
            }
        }
        if local == "script" {
            kinds.insert(AnimationKind::Script);
        }
        if local == "style" {
            for child in &el.children {
                if let Node::Text(t) = child {
                    if style_has_motion(t) {
                        kinds.insert(AnimationKind::CssKeyframes);
                    }
                }
            }
        }
        for (k, v) in &el.attrs {
            let key = k.rsplit_once(':').map(|(_, l)| l).unwrap_or(k);
            if is_event_attr(key) {
                kinds.insert(AnimationKind::Events);
            }
            if key.eq_ignore_ascii_case("style") && style_has_motion(v) {
                kinds.insert(AnimationKind::CssKeyframes);
            }
            if matches!(key, "begin" | "end") {
                collect_smil_sync_ids(v, smil);
            }
        }
        walk_detect(&el.children, kinds, smil);
    }
}

fn style_has_motion(css: &str) -> bool {
    let lower = css.to_ascii_lowercase();
    lower.contains("@keyframes")
        || lower.contains("animation:")
        || lower.contains("animation-name:")
}

fn is_event_attr(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    matches!(
        n.as_str(),
        "onclick"
            | "onload"
            | "onunload"
            | "onbegin"
            | "onend"
            | "onrepeat"
            | "onfocus"
            | "onblur"
            | "onactivate"
            | "onmouseover"
            | "onmouseout"
            | "onmousedown"
            | "onmouseup"
            | "onmousemove"
            | "onmouseenter"
            | "onmouseleave"
            | "onkeydown"
            | "onkeyup"
            | "onkeypress"
            | "onabort"
            | "onerror"
            | "onresize"
            | "onscroll"
            | "onchange"
            | "oninput"
            | "onsubmit"
            | "onfocusin"
            | "onfocusout"
            | "ontouchstart"
            | "ontouchend"
            | "ontouchmove"
            | "onpointerdown"
            | "onpointerup"
            | "onpointermove"
            | "onzoom"
    )
}

/// Scan JS/TS/JSX/CSS/HTML text for `#id` and common DOM lookups.
/// Hex colors (`#fff`, `#ff00aa`) are ignored.
pub fn scan_sources_for_id_refs(sources: &[(String, String)]) -> Vec<String> {
    let mut ids = BTreeSet::new();
    for (_, text) in sources {
        collect_hash_ids(text, &mut ids);
        collect_dom_lookups(text, &mut ids);
    }
    ids.into_iter().collect()
}

fn collect_hash_ids(text: &str, out: &mut BTreeSet<String>) {
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'#' && i + 1 < bytes.len() {
            let start = i + 1;
            if !is_name_start(bytes[start]) {
                i += 1;
                continue;
            }
            let mut end = start + 1;
            while end < bytes.len() && is_name_char(bytes[end]) {
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
        i += 1;
    }
}

fn collect_dom_lookups(text: &str, out: &mut BTreeSet<String>) {
    for needle in [
        "getElementById(",
        "getElementById (",
        "querySelector(",
        "querySelector (",
    ] {
        let mut search = text;
        while let Some(idx) = search.find(needle) {
            let after = &search[idx + needle.len()..];
            let trimmed = after.trim_start();
            if let Some(q) = trimmed.chars().next() {
                if q == '"' || q == '\'' {
                    let rest = &trimmed[q.len_utf8()..];
                    if let Some(end) = rest.find(q) {
                        let mut inner = rest[..end].trim();
                        if let Some(hash) = inner.strip_prefix('#') {
                            inner = hash;
                        }
                        if is_xml_name(inner) && !is_hex_color(inner) {
                            out.insert(inner.to_string());
                        }
                    }
                }
            }
            search = &search[idx + needle.len()..];
        }
    }
}

fn is_hex_color(s: &str) -> bool {
    matches!(s.len(), 3 | 4 | 6 | 8) && s.bytes().all(|b| b.is_ascii_hexdigit())
}

fn is_name_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_'
}

fn is_name_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'-' || b == b':'
}

fn is_xml_name(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == ':')
}

/// Tokens of the form `id.begin`, `id.end+.33s`, `id.end-0.5s`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SmilSyncRef {
    pub id: String,
    pub event: String,
    pub offset: Option<String>,
}

pub fn parse_smil_clock_value(value: &str) -> Vec<SmilSyncRef> {
    let mut out = Vec::new();
    for token in value.split([';', ',']) {
        if let Some(r) = parse_smil_token(token.trim()) {
            out.push(r);
        }
    }
    out
}

fn parse_smil_token(token: &str) -> Option<SmilSyncRef> {
    let token = token.trim();
    let (id, rest) = token.split_once('.')?;
    let id = id.trim();
    if !is_xml_name(id) {
        return None;
    }
    let rest = rest.trim();
    let (event, offset) = split_event_offset(rest)?;
    if !is_smil_event(event) {
        return None;
    }
    Some(SmilSyncRef {
        id: id.to_string(),
        event: event.to_string(),
        offset,
    })
}

fn split_event_offset(rest: &str) -> Option<(&str, Option<String>)> {
    // `end+.33s` / `end-0.5s` / `end + 0.06s` / `repeat(2)` / `click`
    let bytes = rest.as_bytes();
    let mut i = 0;
    while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
        i += 1;
    }
    if i == 0 {
        return None;
    }
    let event = rest[..i].trim();
    let tail = rest[i..].trim();
    if tail.is_empty() {
        return Some((event, None));
    }
    if tail.starts_with('(') {
        // `repeat(2)` — no clock offset
        return Some((event, None));
    }
    if tail.starts_with('+') || tail.starts_with('-') {
        return Some((event, Some(tail.to_string())));
    }
    Some((event, None))
}

fn is_smil_event(name: &str) -> bool {
    matches!(
        name,
        "begin"
            | "end"
            | "repeat"
            | "click"
            | "focus"
            | "blur"
            | "activate"
            | "mouseover"
            | "mouseout"
            | "mousedown"
            | "mouseup"
            | "load"
            | "unload"
            | "accesskey"
            | "repeatEvent"
            | "beginEvent"
            | "endEvent"
    )
}

pub fn collect_smil_sync_ids(value: &str, out: &mut BTreeSet<String>) {
    for r in parse_smil_clock_value(value) {
        out.insert(r.id);
    }
}

/// Rewrite `begin`/`end` if IDs were prefixed. We skip prefixing in v1;
/// this exists so a later prefix pass cannot repeat SVGO #2073 / #2207.
pub fn rewrite_smil_clock_value(value: &str, rename: &HashMap<String, String>) -> String {
    if rename.is_empty() {
        return value.to_string();
    }
    let mut parts = Vec::new();
    // Preserve original separators by scanning the raw string.
    let seps: Vec<usize> = value
        .char_indices()
        .filter(|(_, c)| *c == ';' || *c == ',')
        .map(|(i, _)| i)
        .collect();
    let mut starts = vec![0usize];
    starts.extend(seps.iter().map(|i| i + 1));
    let mut ends = seps.clone();
    ends.push(value.len());
    for (start, end) in starts.into_iter().zip(ends) {
        let raw = &value[start..end];
        let trimmed = raw.trim();
        let rewritten = if let Some(r) = parse_smil_token(trimmed) {
            if let Some(new_id) = rename.get(&r.id) {
                let mut s = String::new();
                // keep leading ws
                let lead = raw.len() - raw.trim_start().len();
                s.push_str(&raw[..lead]);
                s.push_str(new_id);
                s.push('.');
                s.push_str(&r.event);
                if let Some(off) = &r.offset {
                    s.push_str(off);
                }
                let trail = raw.len() - raw.trim_end().len();
                if trail > 0 {
                    s.push_str(&raw[raw.len() - trail..]);
                }
                s
            } else {
                raw.to_string()
            }
        } else {
            raw.to_string()
        };
        parts.push(rewritten);
        if end < value.len() {
            parts.push(value[end..end + 1].to_string());
        }
    }
    if seps.is_empty() && parts.len() == 1 {
        return parts.remove(0);
    }
    parts.concat()
}

/// Apply [`rewrite_smil_clock_value`] to every `begin`/`end` in the tree.
#[allow(dead_code)]
pub fn rewrite_smil_sync_attrs(el: &mut Element, rename: &HashMap<String, String>) {
    for (k, v) in el.attrs.iter_mut() {
        let local = k.rsplit_once(':').map(|(_, l)| l).unwrap_or(k.as_str());
        if local == "begin" || local == "end" {
            *v = rewrite_smil_clock_value(v, rename);
        }
    }
    for child in &mut el.children {
        if let Node::Element(child) = child {
            rewrite_smil_sync_attrs(child, rename);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{optimize_str, optimize_with, Config};

    fn svg_smil() -> &'static str {
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 32 32">
  <circle id="orb" cx="16" cy="16" r="6" fill="#38bdf8">
    <animate id="pulse" attributeName="r" values="4;10;4" dur="1s" begin="0s; pulse.end+.33s" repeatCount="indefinite"/>
  </circle>
  <circle id="unusedHook" cx="4" cy="4" r="1" fill="#fff"/>
</svg>"##
    }

    #[test]
    fn detects_smil_and_parses_offset_sync() {
        let doc = crate::parse::parse_svg(svg_smil()).unwrap();
        let report = detect_document(&doc, &[]);
        assert!(report.motion_sensitive);
        assert!(report.kinds.contains(&AnimationKind::Smil));
        assert!(report.smil_sync_refs.iter().any(|id| id == "pulse"));
        let refs = parse_smil_clock_value("0s; pulse.end+.33s");
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].id, "pulse");
        assert_eq!(refs[0].event, "end");
        assert_eq!(refs[0].offset.as_deref(), Some("+.33s"));
    }

    #[test]
    fn parses_minus_offset_like_svgo_2207() {
        let refs = parse_smil_clock_value("0;b.end-0.5s");
        assert_eq!(refs[0].id, "b");
        assert_eq!(refs[0].event, "end");
        assert_eq!(refs[0].offset.as_deref(), Some("-0.5s"));
        let refs = parse_smil_clock_value("a.begin+0.1s");
        assert_eq!(refs[0].id, "a");
        assert_eq!(refs[0].offset.as_deref(), Some("+0.1s"));
    }

    #[test]
    fn rewrite_prefix_keeps_offset() {
        let mut map = HashMap::new();
        map.insert("y1".into(), "icon-y1".into());
        map.insert("b".into(), "icon-b".into());
        assert_eq!(
            rewrite_smil_clock_value("0s; y1.end+.33s", &map),
            "0s; icon-y1.end+.33s"
        );
        assert_eq!(
            rewrite_smil_clock_value("b.end-0.5s", &map),
            "icon-b.end-0.5s"
        );
        assert_eq!(rewrite_smil_clock_value("0s", &map), "0s");
    }

    #[test]
    fn animation_aware_keeps_unused_ids() {
        let out = optimize_str(svg_smil()).unwrap();
        assert!(out.svg.contains("id=\"unusedHook\""), "{}", out.svg);
        assert!(out.svg.contains("id=\"pulse\""), "{}", out.svg);
        assert!(out.svg.contains("pulse.end+.33s"), "{}", out.svg);
        assert!(out.animation.motion_sensitive);
        assert!(out.plugins_skipped.contains(&"cleanupIds"));
        assert!(out.summary.contains("preserved"));
        assert!(out.summary.contains("skipped"));
    }

    #[test]
    fn animation_aware_off_drops_unused_id() {
        let mut cfg = Config::default();
        cfg.animation_aware = false;
        let out = optimize_with(svg_smil().as_bytes(), &cfg).unwrap();
        assert!(!out.svg.contains("unusedHook"), "{}", out.svg);
        assert!(out.plugins_skipped.is_empty());
    }

    #[test]
    fn detects_css_keyframes() {
        let input = r#"<svg xmlns="http://www.w3.org/2000/svg">
          <style>@keyframes spin { to { transform: rotate(360deg) } } .a { animation: spin 1s infinite }</style>
          <rect class="a" width="10" height="10"/>
        </svg>"#;
        let doc = crate::parse::parse_svg(input).unwrap();
        let report = detect_document(&doc, &[]);
        assert!(report.kinds.contains(&AnimationKind::CssKeyframes));
    }

    #[test]
    fn external_jsx_marks_motion_and_preserves_ids() {
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg"><circle id="heroOrb" cx="8" cy="8" r="4"/><path id="orbitPath" d="M2 8h12"/><g id="captionMark"></g></svg>"#;
        let jsx = r##"
          import { gsap } from "gsap"
          export default function App() {
            gsap.to("#heroOrb", { duration: 2, x: 40 })
            document.getElementById("orbitPath")
          }
        "##;
        let mut cfg = Config::default();
        cfg.extra_sources = vec![("App.jsx".into(), jsx.into())];
        cfg.source_name = Some("gsap-trap.svg".into());
        let out = optimize_with(svg.as_bytes(), &cfg).unwrap();
        assert!(out.svg.contains("heroOrb"), "{}", out.svg);
        assert!(out.svg.contains("orbitPath"), "{}", out.svg);
        assert!(out.svg.contains("captionMark"), "{}", out.svg);
        assert!(out.animation.kinds.contains(&AnimationKind::ExternalIds));
        assert_eq!(out.animation.workspace_scan, "request-extras");
        assert!(out.summary.contains("gsap-trap.svg"));
    }

    #[test]
    fn scan_ignores_hex_colors() {
        let ids = scan_sources_for_id_refs(&[(
            "x.css".into(),
            "#fff { color: #00ffaa } #heroOrb { color: red }".into(),
        )]);
        assert_eq!(ids, vec!["heroOrb".to_string()]);
    }

    #[test]
    fn count_id_attrs_finds_quoted() {
        assert_eq!(count_id_attrs(r#"<g id="a"><b id='c'/></g>"#), 2);
    }

    #[test]
    fn rewrite_y1_end_offset_with_prefix_map() {
        let mut map = HashMap::new();
        map.insert("y1".into(), "pfx-y1".into());
        assert_eq!(
            rewrite_smil_clock_value("0s; y1.end+.33s", &map),
            "0s; pfx-y1.end+.33s"
        );
    }

    #[test]
    fn parses_b_end_minus_and_a_begin_plus() {
        let minus = parse_smil_clock_value("b.end-0.5s");
        assert_eq!(minus.len(), 1);
        assert_eq!(minus[0].id, "b");
        assert_eq!(minus[0].event, "end");
        assert_eq!(minus[0].offset.as_deref(), Some("-0.5s"));
        let plus = parse_smil_clock_value("a.begin+0.1s");
        assert_eq!(plus.len(), 1);
        assert_eq!(plus[0].id, "a");
        assert_eq!(plus[0].event, "begin");
        assert_eq!(plus[0].offset.as_deref(), Some("+0.1s"));
    }

    #[test]
    fn detects_animation_name_without_keyframes_when_animation_present() {
        let input = r##"<svg xmlns="http://www.w3.org/2000/svg">
          <style>.spin { animation: spin 1s linear infinite; animation-name: spin; color: #38bdf8 }</style>
          <rect class="spin" width="10" height="10"/>
        </svg>"##;
        assert!(!input.to_ascii_lowercase().contains("@keyframes"));
        let doc = crate::parse::parse_svg(input).unwrap();
        let report = detect_document(&doc, &[]);
        assert!(report.motion_sensitive);
        assert!(report.kinds.contains(&AnimationKind::CssKeyframes));
    }

    #[test]
    fn extra_source_ids_absent_from_svg_do_not_mark_external() {
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg"><circle id="presentOrb" cx="8" cy="8" r="4" fill="#fff"/></svg>"##;
        let jsx = r##"gsap.to("#ghostOrb", { x: 40 }); document.getElementById("missingHook")"##;
        let doc = crate::parse::parse_svg(svg).unwrap();
        let report = detect_document(&doc, &[("App.jsx".into(), jsx.into())]);
        assert!(!report.kinds.contains(&AnimationKind::ExternalIds));
        assert!(report.external_ids.is_empty());
        assert_eq!(report.workspace_scan, "request-extras");
        assert!(!report.motion_sensitive);
    }

    #[test]
    fn detects_script_and_onclick() {
        let input = r##"<svg xmlns="http://www.w3.org/2000/svg">
          <script>console.log("#orb")</script>
          <rect id="hit" width="10" height="10" fill="#000" onclick="alert(1)"/>
        </svg>"##;
        let doc = crate::parse::parse_svg(input).unwrap();
        let report = detect_document(&doc, &[]);
        assert!(report.motion_sensitive);
        assert!(report.kinds.contains(&AnimationKind::Script));
        assert!(report.kinds.contains(&AnimationKind::Events));
    }
}
