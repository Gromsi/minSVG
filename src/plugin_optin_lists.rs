//! Opt-in SVGO-named list / style passes. Default **off**.
//!
//! * [`cleanup_list_of_values`] — round `points` (polyline/polygon),
//!   `enable-background`, `viewBox`, `stroke-dasharray`, and list-valued
//!   `x` / `y` / `dx` / `dy`.
//! * [`convert_style_to_attrs`] — `style=""` presentation decls → attributes.
//!
//! Not wired into [`crate::DEFAULT_PLUGIN_NAMES`]. Call the fns directly or
//! enable via `--plugin` / [`Config::enable_plugins`]. Do not confuse the
//! named style pass with the cascade-safe sliver inside `minifyStyles`.

use crate::ast::{Document, Element, Node};
use crate::config::Config;
use serde_json::Value;

pub const LIST_OPTIN_PLUGIN_NAMES: &[&str] = &["cleanupListOfValues", "convertStyleToAttrs"];

/// SVGO `cleanupListOfValues` params.
#[derive(Debug, Clone, PartialEq)]
pub struct CleanupListOfValuesParams {
    pub float_precision: u8,
    pub leading_zero: bool,
    pub default_px: bool,
    pub convert_to_px: bool,
}

impl Default for CleanupListOfValuesParams {
    fn default() -> Self {
        Self {
            float_precision: 3,
            leading_zero: true,
            default_px: true,
            convert_to_px: true,
        }
    }
}

impl CleanupListOfValuesParams {
    pub fn from_value(value: &Value) -> Self {
        let mut p = Self::default();
        if let Some(obj) = value.as_object() {
            if let Some(n) = obj.get("floatPrecision").and_then(json_u8) {
                p.float_precision = n;
            }
            if let Some(b) = obj.get("leadingZero").and_then(Value::as_bool) {
                p.leading_zero = b;
            }
            if let Some(b) = obj.get("defaultPx").and_then(Value::as_bool) {
                p.default_px = b;
            }
            if let Some(b) = obj.get("convertToPx").and_then(Value::as_bool) {
                p.convert_to_px = b;
            }
        }
        p
    }

    pub fn from_config(config: &Config) -> Self {
        let mut params = config
            .plugin_param("cleanupListOfValues")
            .map(Self::from_value)
            .unwrap_or_default();
        let has_precision = config
            .plugin_param("cleanupListOfValues")
            .and_then(|v| v.get("floatPrecision"))
            .is_some();
        if !has_precision {
            if let Some(n) = config.precision {
                params.float_precision = n;
            }
        }
        params
    }
}

/// SVGO `convertStyleToAttrs` params.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConvertStyleToAttrsParams {
    /// Leave `!important` decls in `style=""` instead of promoting them.
    pub keep_important: bool,
}

impl Default for ConvertStyleToAttrsParams {
    fn default() -> Self {
        Self {
            keep_important: false,
        }
    }
}

impl ConvertStyleToAttrsParams {
    pub fn from_value(value: &Value) -> Self {
        let mut p = Self::default();
        if let Some(obj) = value.as_object() {
            if let Some(b) = obj.get("keepImportant").and_then(Value::as_bool) {
                p.keep_important = b;
            }
        }
        p
    }

    pub fn from_config(config: &Config) -> Self {
        config
            .plugin_param("convertStyleToAttrs")
            .map(Self::from_value)
            .unwrap_or_default()
    }
}

pub fn cleanup_list_of_values(doc: &mut Document) {
    cleanup_list_of_values_with(doc, &CleanupListOfValuesParams::default());
}

pub fn cleanup_list_of_values_with(doc: &mut Document, params: &CleanupListOfValuesParams) {
    for node in &mut doc.nodes {
        cleanup_list_of_values_in(node, params);
    }
}

pub fn run_cleanup_list_of_values(root: &mut Node) {
    cleanup_list_of_values_in(root, &CleanupListOfValuesParams::default());
}

pub fn run_cleanup_list_of_values_with(root: &mut Node, params: &CleanupListOfValuesParams) {
    cleanup_list_of_values_in(root, params);
}

pub fn convert_style_to_attrs(doc: &mut Document) {
    convert_style_to_attrs_with(doc, &ConvertStyleToAttrsParams::default());
}

pub fn convert_style_to_attrs_with(doc: &mut Document, params: &ConvertStyleToAttrsParams) {
    for node in &mut doc.nodes {
        convert_style_to_attrs_in(node, params);
    }
}

pub fn run_convert_style_to_attrs(root: &mut Node) {
    convert_style_to_attrs_in(root, &ConvertStyleToAttrsParams::default());
}

pub fn run_convert_style_to_attrs_with(root: &mut Node, params: &ConvertStyleToAttrsParams) {
    convert_style_to_attrs_in(root, params);
}

/// Run the two opt-in passes when `--plugin` enabled them (`--skip` wins).
///
/// `convertStyleToAttrs` first so later default minify sees presentation attrs.
/// `cleanupListOfValues` next so `12pt` becomes `px` before numeric minify.
pub fn run_enabled_style_pass(doc: &mut Document, config: &Config) -> bool {
    if !config.plugin_enabled("convertStyleToAttrs") {
        return false;
    }
    convert_style_to_attrs_with(doc, &ConvertStyleToAttrsParams::from_config(config));
    true
}

pub fn run_enabled_list_pass(doc: &mut Document, config: &Config) -> bool {
    if !config.plugin_enabled("cleanupListOfValues") {
        return false;
    }
    cleanup_list_of_values_with(doc, &CleanupListOfValuesParams::from_config(config));
    true
}

fn cleanup_list_of_values_in(node: &mut Node, params: &CleanupListOfValuesParams) {
    node.walk_elements_mut(&mut |el| {
        if is_smil(el.local_name()) {
            return;
        }
        let keys: Vec<String> = el.attrs.iter().map(|(k, _)| k.clone()).collect();
        for key in keys {
            if !is_list_attr(&key) {
                continue;
            }
            if let Some(raw) = el.attr(&key).map(str::to_string) {
                let next = round_list(&raw, params);
                if next != raw {
                    el.set_attr(&key, next);
                }
            }
        }
    });
}

fn convert_style_to_attrs_in(node: &mut Node, params: &ConvertStyleToAttrsParams) {
    node.walk_elements_mut(&mut |el| {
        promote_style_decls(el, params);
    });
}

fn is_list_attr(name: &str) -> bool {
    matches!(
        name,
        "points" | "enable-background" | "viewBox" | "stroke-dasharray" | "dx" | "dy" | "x" | "y"
    )
}

fn is_smil(local: &str) -> bool {
    matches!(
        local.to_ascii_lowercase().as_str(),
        "animate" | "animatetransform" | "animatemotion" | "animatecolor" | "set"
    )
}

fn round_list(raw: &str, params: &CleanupListOfValuesParams) -> String {
    let tokens = split_list_tokens(raw);
    if tokens.is_empty() {
        return raw.trim().to_string();
    }
    let mut out = Vec::with_capacity(tokens.len());
    for tok in tokens {
        if tok.eq_ignore_ascii_case("new") {
            out.push("new".to_string());
        } else if let Some(rounded) = round_numeric_token(tok, params) {
            out.push(rounded);
        } else if !tok.is_empty() {
            out.push(tok.to_string());
        }
    }
    out.join(" ")
}

fn split_list_tokens(raw: &str) -> Vec<&str> {
    raw.split(|c: char| c.is_ascii_whitespace() || c == ',')
        .filter(|t| !t.is_empty())
        .collect()
}

fn round_numeric_token(tok: &str, params: &CleanupListOfValuesParams) -> Option<String> {
    let (num, unit) = split_number_unit(tok)?;
    let precision = i32::from(params.float_precision.min(20));
    let mut value = round_to_precision(num, precision);
    let mut unit = unit.to_ascii_lowercase();

    if params.convert_to_px {
        if let Some(factor) = absolute_px_factor(&unit) {
            let px = round_to_precision(num * factor, precision);
            let px_str = format_compact(px, params.leading_zero);
            if px_str.len() < tok.len() {
                value = px;
                unit = "px".to_string();
            }
        }
    }

    let mut s = format_compact(value, params.leading_zero);
    if unit == "px" && params.default_px {
        return Some(s);
    }
    if !unit.is_empty() {
        s.push_str(&unit);
    }
    Some(s)
}

fn split_number_unit(tok: &str) -> Option<(f64, &str)> {
    let bytes = tok.as_bytes();
    if bytes.is_empty() {
        return None;
    }
    let mut i = 0usize;
    if matches!(bytes[0], b'+' | b'-') {
        i = 1;
    }
    let mut saw_digit = false;
    let mut saw_dot = false;
    while i < bytes.len() {
        match bytes[i] {
            b'0'..=b'9' => {
                saw_digit = true;
                i += 1;
            }
            b'.' if !saw_dot => {
                saw_dot = true;
                i += 1;
            }
            _ => break,
        }
    }
    if i < bytes.len() && matches!(bytes[i], b'e' | b'E') {
        let e = i;
        i += 1;
        if i < bytes.len() && matches!(bytes[i], b'+' | b'-') {
            i += 1;
        }
        let exp_start = i;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
        if i == exp_start {
            i = e;
        }
    }
    if !saw_digit || i == 0 || (i == 1 && matches!(bytes[0], b'+' | b'-')) {
        return None;
    }
    let num: f64 = tok[..i].parse().ok()?;
    if !num.is_finite() {
        return None;
    }
    let unit = &tok[i..];
    if unit.is_empty() || unit == "%" || unit.bytes().all(|c| c.is_ascii_alphabetic()) {
        Some((num, unit))
    } else {
        None
    }
}

fn absolute_px_factor(unit: &str) -> Option<f64> {
    Some(match unit {
        "px" => 1.0,
        "pt" => 4.0 / 3.0,
        "pc" => 16.0,
        "in" => 96.0,
        "cm" => 96.0 / 2.54,
        "mm" => 96.0 / 25.4,
        _ => return None,
    })
}

fn round_to_precision(n: f64, precision: i32) -> f64 {
    let factor = 10f64.powi(precision.max(0));
    (n * factor).round() / factor
}

fn format_compact(n: f64, leading_zero: bool) -> String {
    if !n.is_finite() {
        return n.to_string();
    }
    if n.abs() < 1e-12 {
        return "0".into();
    }
    let mut s = n.to_string();
    if let Some(dot) = s.find('.') {
        if !s.contains('e') && !s.contains('E') {
            let bytes = s.as_bytes();
            let mut end = s.len();
            while end > dot + 1 && bytes[end - 1] == b'0' {
                end -= 1;
            }
            if end > 0 && bytes[end - 1] == b'.' {
                end -= 1;
            }
            s.truncate(end);
        }
    }
    if leading_zero {
        if let Some(rest) = s.strip_prefix("0.") {
            s = format!(".{rest}");
        } else if let Some(rest) = s.strip_prefix("-0.") {
            s = format!("-.{rest}");
        }
    }
    s
}

fn promote_style_decls(el: &mut Element, params: &ConvertStyleToAttrsParams) {
    let Some(style) = el.attr("style").map(str::to_string) else {
        return;
    };
    let decls = parse_style_decls(&style);
    if decls.is_empty() {
        return;
    }
    let smil_targets = smil_attribute_names(el);
    let mut kept = Vec::new();
    let mut promoted = Vec::new();
    for (prop, value, important) in decls {
        if important && params.keep_important {
            kept.push(format_decl(&prop, &value, true));
            continue;
        }
        if !is_promotable_presentation(&prop) {
            kept.push(format_decl(&prop, &value, important));
            continue;
        }
        if is_css_only(&value) || is_css_wide(&value) {
            kept.push(format_decl(&prop, &value, important));
            continue;
        }
        if smil_targets.iter().any(|t| t.eq_ignore_ascii_case(&prop)) {
            kept.push(format_decl(&prop, &value, important));
            continue;
        }
        promoted.push((prop, compact_promoted_value(&value)));
    }
    if promoted.is_empty() {
        return;
    }
    for (k, v) in promoted {
        el.set_attr(&k, v);
    }
    if kept.is_empty() {
        el.remove_attr("style");
    } else {
        el.set_attr("style", kept.join(";"));
    }
}

fn smil_attribute_names(el: &Element) -> Vec<String> {
    el.children
        .iter()
        .filter_map(|n| match n {
            Node::Element(c) if is_smil(c.local_name()) => {
                c.attr("attributeName").map(str::to_string)
            }
            _ => None,
        })
        .collect()
}

fn format_decl(prop: &str, value: &str, important: bool) -> String {
    if important {
        format!("{prop}:{value}!important")
    } else {
        format!("{prop}:{value}")
    }
}

fn parse_style_decls(style: &str) -> Vec<(String, String, bool)> {
    let stripped = strip_css_comments(style);
    let mut out = Vec::new();
    for part in split_decls(&stripped) {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let Some((k, raw_v)) = part.split_once(':') else {
            continue;
        };
        let prop = k.trim().to_ascii_lowercase();
        if prop.is_empty() {
            continue;
        }
        let (value, important) = split_important(raw_v);
        let value = unquote(value.trim());
        if value.is_empty() {
            continue;
        }
        out.push((prop, value, important));
    }
    out
}

fn split_decls(style: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let bytes = style.as_bytes();
    let mut start = 0usize;
    let mut i = 0usize;
    let mut depth = 0i32;
    let mut quote: Option<u8> = None;
    while i < bytes.len() {
        let c = bytes[i];
        if let Some(q) = quote {
            if c == q {
                quote = None;
            } else if c == b'\\' && i + 1 < bytes.len() {
                i += 1;
            }
            i += 1;
            continue;
        }
        match c {
            b'\'' | b'"' => quote = Some(c),
            b'(' => depth += 1,
            b')' if depth > 0 => depth -= 1,
            b';' if depth == 0 => {
                out.push(&style[start..i]);
                start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    if start < style.len() {
        out.push(&style[start..]);
    }
    out
}

fn strip_css_comments(css: &str) -> String {
    let mut out = String::with_capacity(css.len());
    let bytes = css.as_bytes();
    let mut i = 0usize;
    let mut quote: Option<u8> = None;
    while i < bytes.len() {
        if let Some(q) = quote {
            out.push(bytes[i] as char);
            if bytes[i] == q {
                quote = None;
            } else if bytes[i] == b'\\' && i + 1 < bytes.len() {
                i += 1;
                out.push(bytes[i] as char);
            }
            i += 1;
            continue;
        }
        if bytes[i] == b'\'' || bytes[i] == b'"' {
            quote = Some(bytes[i]);
            out.push(bytes[i] as char);
            i += 1;
            continue;
        }
        if bytes[i] == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'*' {
            i += 2;
            while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                i += 1;
            }
            i = i.saturating_add(2).min(bytes.len());
            continue;
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

fn split_important(value: &str) -> (String, bool) {
    let t = value.trim();
    let lower = t.to_ascii_lowercase();
    if let Some(head) = lower.strip_suffix("!important") {
        let keep = t[..head.len()].trim().to_string();
        (keep, true)
    } else {
        (t.to_string(), false)
    }
}

fn unquote(v: &str) -> String {
    let t = v.trim();
    if t.len() >= 2 {
        let b = t.as_bytes();
        if (b[0] == b'"' && b[b.len() - 1] == b'"') || (b[0] == b'\'' && b[b.len() - 1] == b'\'') {
            return t[1..t.len() - 1].to_string();
        }
    }
    t.to_string()
}

fn compact_promoted_value(v: &str) -> String {
    let t = v.trim();
    let lower = t.to_ascii_lowercase();
    if lower.starts_with("url(") && t.ends_with(')') {
        let inner = unquote(t[4..t.len() - 1].trim());
        return format!("url({})", inner.trim());
    }
    t.to_string()
}

fn is_css_only(v: &str) -> bool {
    let l = v.to_ascii_lowercase();
    l.contains("var(")
        || l.contains("calc(")
        || l.contains("min(")
        || l.contains("max(")
        || l.contains("clamp(")
        || l.contains("env(")
        || l.contains("attr(")
}

fn is_css_wide(v: &str) -> bool {
    matches!(
        v.trim().to_ascii_lowercase().as_str(),
        "initial" | "unset" | "revert" | "revert-layer"
    )
}

/// Presentation attrs that map 1:1. `transform` / `transform-origin` /
/// `font-family` stay in CSS (syntax / px / quotes).
fn is_promotable_presentation(name: &str) -> bool {
    matches!(
        name,
        "alignment-baseline"
            | "baseline-shift"
            | "clip-path"
            | "clip-rule"
            | "clip"
            | "color-interpolation-filters"
            | "color-interpolation"
            | "color-profile"
            | "color-rendering"
            | "color"
            | "cursor"
            | "direction"
            | "display"
            | "dominant-baseline"
            | "enable-background"
            | "fill-opacity"
            | "fill-rule"
            | "fill"
            | "filter"
            | "flood-color"
            | "flood-opacity"
            | "font-size-adjust"
            | "font-size"
            | "font-stretch"
            | "font-style"
            | "font-variant"
            | "font-weight"
            | "glyph-orientation-horizontal"
            | "glyph-orientation-vertical"
            | "image-rendering"
            | "letter-spacing"
            | "lighting-color"
            | "marker-end"
            | "marker-mid"
            | "marker-start"
            | "mask"
            | "opacity"
            | "overflow"
            | "paint-order"
            | "pointer-events"
            | "shape-rendering"
            | "stop-color"
            | "stop-opacity"
            | "stroke-dasharray"
            | "stroke-dashoffset"
            | "stroke-linecap"
            | "stroke-linejoin"
            | "stroke-miterlimit"
            | "stroke-opacity"
            | "stroke-width"
            | "stroke"
            | "text-anchor"
            | "text-decoration"
            | "text-overflow"
            | "text-rendering"
            | "unicode-bidi"
            | "vector-effect"
            | "visibility"
            | "word-spacing"
            | "writing-mode"
    )
}

fn json_u8(v: &Value) -> Option<u8> {
    if let Some(n) = v.as_u64() {
        return u8::try_from(n).ok().filter(|n| *n <= 20);
    }
    if let Some(n) = v.as_i64() {
        return u8::try_from(n).ok().filter(|n| *n <= 20);
    }
    if let Some(n) = v.as_f64() {
        if n.is_finite() && (0.0..=20.0).contains(&n) {
            return Some(n as u8);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::parse_svg;
    use crate::serialize::serialize;
    use crate::{optimize_str, optimize_str_with};

    fn find<'a>(el: &'a Element, name: &str) -> Option<&'a Element> {
        if el.local_name() == name {
            return Some(el);
        }
        for child in &el.children {
            if let Node::Element(c) = child {
                if let Some(hit) = find(c, name) {
                    return Some(hit);
                }
            }
        }
        None
    }

    fn root(doc: &Document) -> &Element {
        doc.nodes
            .iter()
            .find_map(|n| match n {
                Node::Element(el) => Some(el),
                _ => None,
            })
            .expect("root")
    }

    #[test]
    fn list_optin_names_are_not_default() {
        for name in LIST_OPTIN_PLUGIN_NAMES {
            assert!(
                !crate::DEFAULT_PLUGIN_NAMES.contains(name),
                "{name} must stay opt-in"
            );
        }
    }

    #[test]
    fn cleanup_list_rounds_points_polygon_and_enable_background() {
        let mut doc = parse_svg(concat!(
            r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0.000 0.000 100.5000 50.000" enable-background="new 0.00 0.00 100.50 50.00">"##,
            r##"<polygon points="0.000, 10.500 20.1234,30.000"/>"##,
            r##"<polyline points="1.2300 2.000"/>"##,
            "</svg>",
        ))
        .unwrap();
        cleanup_list_of_values(&mut doc);
        let svg = root(&doc);
        assert_eq!(svg.attr("viewBox"), Some("0 0 100.5 50"));
        assert_eq!(svg.attr("enable-background"), Some("new 0 0 100.5 50"));
        let poly = find(svg, "polygon").unwrap();
        assert_eq!(poly.attr("points"), Some("0 10.5 20.123 30"));
        let line = find(svg, "polyline").unwrap();
        assert_eq!(line.attr("points"), Some("1.23 2"));
    }

    #[test]
    fn cleanup_list_converts_pt_and_keeps_new() {
        let mut doc = parse_svg(concat!(
            r##"<svg xmlns="http://www.w3.org/2000/svg">"##,
            r##"<polygon points="0pt,12pt"/>"##,
            r##"<rect enable-background="new 0 0 10 10"/>"##,
            "</svg>",
        ))
        .unwrap();
        cleanup_list_of_values(&mut doc);
        let svg = root(&doc);
        assert_eq!(find(svg, "polygon").unwrap().attr("points"), Some("0 16"));
        assert_eq!(
            find(svg, "rect").unwrap().attr("enable-background"),
            Some("new 0 0 10 10")
        );
    }

    #[test]
    fn cleanup_list_leading_zero_and_precision_param() {
        let mut doc = parse_svg(
            r#"<svg xmlns="http://www.w3.org/2000/svg"><polyline points="0.5000 1.2349"/></svg>"#,
        )
        .unwrap();
        cleanup_list_of_values_with(
            &mut doc,
            &CleanupListOfValuesParams {
                float_precision: 2,
                ..CleanupListOfValuesParams::default()
            },
        );
        assert_eq!(
            find(root(&doc), "polyline").unwrap().attr("points"),
            Some(".5 1.23")
        );
    }

    #[test]
    fn cleanup_list_skips_path_d_and_smil() {
        let mut doc = parse_svg(concat!(
            r##"<svg xmlns="http://www.w3.org/2000/svg">"##,
            r##"<path d="M 1.2300 2.0000"/>"##,
            r##"<rect x="1.2300" y="4.0000">"##,
            r##"<animate attributeName="x" from="1.2300" to="2.0000" dur="1s"/>"##,
            "</rect></svg>",
        ))
        .unwrap();
        cleanup_list_of_values(&mut doc);
        let svg = root(&doc);
        assert_eq!(
            find(svg, "path").unwrap().attr("d"),
            Some("M 1.2300 2.0000")
        );
        let anim = find(svg, "animate").unwrap();
        assert_eq!(anim.attr("from"), Some("1.2300"));
        assert_eq!(find(svg, "rect").unwrap().attr("x"), Some("1.23"));
    }

    #[test]
    fn cleanup_list_from_config_json() {
        let mut cfg = Config::default();
        cfg.plugin_params.insert(
            "cleanupListOfValues".into(),
            serde_json::json!({ "floatPrecision": 1, "leadingZero": false }),
        );
        let params = CleanupListOfValuesParams::from_config(&cfg);
        assert_eq!(params.float_precision, 1);
        assert!(!params.leading_zero);
        let mut doc = parse_svg(
            r#"<svg xmlns="http://www.w3.org/2000/svg"><polyline points="0.55 1.26"/></svg>"#,
        )
        .unwrap();
        cleanup_list_of_values_with(&mut doc, &params);
        assert_eq!(
            find(root(&doc), "polyline").unwrap().attr("points"),
            Some("0.6 1.3")
        );
    }

    #[test]
    fn convert_style_promotes_and_drops_style() {
        let mut doc = parse_svg(concat!(
            r##"<svg xmlns="http://www.w3.org/2000/svg">"##,
            r##"<rect style="fill:url('#poolFill');stroke:#f00;opacity:0.50"/>"##,
            "</svg>",
        ))
        .unwrap();
        convert_style_to_attrs(&mut doc);
        let r = find(root(&doc), "rect").unwrap();
        assert_eq!(r.attr("fill"), Some("url(#poolFill)"));
        assert_eq!(r.attr("stroke"), Some("#f00"));
        assert_eq!(r.attr("opacity"), Some("0.50"));
        assert!(r.attr("style").is_none(), "{:?}", r.attrs);
    }

    #[test]
    fn convert_style_overwrites_attr_and_keeps_css_only() {
        let mut doc = parse_svg(concat!(
            r##"<svg xmlns="http://www.w3.org/2000/svg">"##,
            r##"<style>rect{fill:blue}</style>"##,
            r##"<rect class="ocean" fill="blue" style="fill:red;stroke:var(--x);transform-origin:140px 110px"/>"##,
            "</svg>",
        ))
        .unwrap();
        convert_style_to_attrs(&mut doc);
        let r = find(root(&doc), "rect").unwrap();
        assert_eq!(r.attr("fill"), Some("red"));
        let style = r.attr("style").unwrap();
        assert!(style.contains("var(--x)"), "{style}");
        assert!(style.contains("transform-origin:140px 110px"), "{style}");
    }

    #[test]
    fn convert_style_keep_important_and_smil_target() {
        let mut important = parse_svg(
            r#"<svg xmlns="http://www.w3.org/2000/svg"><rect style="fill:red!important;opacity:.5"/></svg>"#,
        )
        .unwrap();
        convert_style_to_attrs_with(
            &mut important,
            &ConvertStyleToAttrsParams {
                keep_important: true,
            },
        );
        let r = find(root(&important), "rect").unwrap();
        assert_eq!(r.attr("opacity"), Some(".5"));
        assert_eq!(r.attr("style"), Some("fill:red!important"));
        assert!(r.attr("fill").is_none());

        let mut motion = parse_svg(concat!(
            r##"<svg xmlns="http://www.w3.org/2000/svg">"##,
            r##"<rect style="fill:red;opacity:.5">"##,
            r##"<animate attributeName="fill" to="blue" dur="1s"/>"##,
            "</rect></svg>",
        ))
        .unwrap();
        convert_style_to_attrs(&mut motion);
        let r = find(root(&motion), "rect").unwrap();
        assert_eq!(r.attr("opacity"), Some(".5"));
        assert_eq!(r.attr("style"), Some("fill:red"));
        assert!(r.attr("fill").is_none());
    }

    #[test]
    fn convert_style_run_on_node() {
        let mut doc = parse_svg(
            r#"<svg xmlns="http://www.w3.org/2000/svg"><circle style="fill:red"/></svg>"#,
        )
        .unwrap();
        run_convert_style_to_attrs(&mut doc.nodes[0]);
        assert_eq!(
            find(root(&doc), "circle").unwrap().attr("fill"),
            Some("red")
        );
    }

    #[test]
    fn default_optimize_does_not_run_list_or_aggressive_style() {
        let lists = concat!(
            r##"<svg xmlns="http://www.w3.org/2000/svg">"##,
            r##"<polygon points="0pt,12pt"/>"##,
            "</svg>",
        );
        let out = optimize_str(lists).unwrap();
        assert!(
            out.svg.contains("12pt") || out.svg.contains("0pt"),
            "cleanupListOfValues must stay off: {}",
            out.svg
        );
        assert!(!out.svg.contains("points=\"0 16\""), "{}", out.svg);
        assert!(!out.plugins_run.iter().any(|n| *n == "cleanupListOfValues"));

        // Shared class keeps the sheet (inlineStyles) so the cascade-safe
        // sliver inside minifyStyles refuses to promote.
        let styled = concat!(
            r##"<svg xmlns="http://www.w3.org/2000/svg">"##,
            r##"<style>.ocean{fill:blue}</style>"##,
            r##"<rect class="ocean" style="fill:red"/>"##,
            r##"<rect class="ocean"/>"##,
            "</svg>",
        );
        let out = optimize_str(styled).unwrap();
        assert!(
            out.svg.contains("style=\"fill:red\"") || out.svg.contains("style=\"fill:red;\""),
            "named convertStyleToAttrs must stay off when a shared class+sheet exists: {}",
            out.svg
        );
        assert!(!out.plugins_run.iter().any(|n| *n == "convertStyleToAttrs"));
    }

    #[test]
    fn enable_plugins_runs_optins_through_optimize() {
        let mut cfg = Config::default();
        cfg.enable_plugins.push("cleanupListOfValues".into());
        cfg.enable_plugins.push("convertStyleToAttrs".into());
        cfg.plugin_params.insert(
            "cleanupListOfValues".into(),
            serde_json::json!({ "floatPrecision": 2 }),
        );

        let lists =
            r#"<svg xmlns="http://www.w3.org/2000/svg"><polygon points="0pt,12pt 1.239,2"/></svg>"#;
        let out = optimize_str_with(lists, &cfg).unwrap();
        assert!(
            out.plugins_run.contains(&"cleanupListOfValues"),
            "{:?}",
            out.plugins_run
        );
        assert!(out.svg.contains("16"), "{}", out.svg);
        assert!(!out.svg.contains("12pt"), "{}", out.svg);

        let styled = concat!(
            r##"<svg xmlns="http://www.w3.org/2000/svg">"##,
            r##"<style>.ocean{fill:blue}</style>"##,
            r##"<rect class="ocean" style="fill:red"/>"##,
            r##"<rect class="ocean"/>"##,
            "</svg>",
        );
        let out = optimize_str_with(styled, &cfg).unwrap();
        assert!(
            out.plugins_run.contains(&"convertStyleToAttrs"),
            "{:?}",
            out.plugins_run
        );
        assert!(out.svg.contains("fill=\"red\""), "{}", out.svg);
        assert!(
            !out.svg.contains("style=\"fill:red\""),
            "named pass should promote even with sheet+class: {}",
            out.svg
        );
    }

    #[test]
    fn skip_wins_over_enable() {
        let mut cfg = Config::default();
        cfg.enable_plugins.push("cleanupListOfValues".into());
        cfg.skip_plugins.push("cleanupListOfValues".into());
        let out = optimize_str_with(
            r#"<svg xmlns="http://www.w3.org/2000/svg"><polygon points="0pt,12pt"/></svg>"#,
            &cfg,
        )
        .unwrap();
        assert!(out.svg.contains("12pt"), "{}", out.svg);
        assert!(!out.plugins_run.contains(&"cleanupListOfValues"));
    }

    #[test]
    fn serialize_after_list_pass() {
        let mut doc = parse_svg(
            r#"<svg xmlns="http://www.w3.org/2000/svg"><polygon points="0.000,1.000"/></svg>"#,
        )
        .unwrap();
        run_cleanup_list_of_values(&mut doc.nodes[0]);
        run_cleanup_list_of_values_with(&mut doc.nodes[0], &CleanupListOfValuesParams::default());
        let out = serialize(&doc);
        assert!(out.contains("points=\"0 1\""), "{out}");
    }

    #[test]
    fn convert_style_run_with_params_on_node() {
        let mut doc = parse_svg(
            r#"<svg xmlns="http://www.w3.org/2000/svg"><g style="opacity:.5;fill:red!important"/></svg>"#,
        )
        .unwrap();
        run_convert_style_to_attrs_with(
            &mut doc.nodes[0],
            &ConvertStyleToAttrsParams {
                keep_important: true,
            },
        );
        let g = find(root(&doc), "g").unwrap();
        assert_eq!(g.attr("opacity"), Some(".5"));
        assert_eq!(g.attr("style"), Some("fill:red!important"));
    }
}
