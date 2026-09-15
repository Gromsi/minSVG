//! Clean-room minify plugins (SVGO-inspired names, not a port of any optimizer).
//!
//! Conservative defaults: unused-only `cleanupIds` (keep `url(#Id)` case),
//! non-zero CSS `px` stays, no `currentColor`, no unused-selector deletion.

use crate::ast::{Document, Element, Node};
use std::collections::HashSet;

/// SVGO `cleanupNumericValues` default. Applied to presentation attrs only —
/// never to path `d` (that is convertPathData / a sibling).
const NON_PATH_FLOAT_PRECISION: i32 = 3;

/// Run the minify plugins in pipeline order.
pub fn run_minify(doc: &mut Document) {
    minify_styles(doc);
    convert_colors(doc);
    minify_styles(doc);
    cleanup_numeric_values(doc);
    cleanup_ids(doc);
    remove_useless_stroke_and_fill(doc);
    convert_ellipse_to_circle(doc);
    collapse_whitespace(doc);
}

// ---------------------------------------------------------------------------
// minifyStyles
// ---------------------------------------------------------------------------

/// Minify `<style>` text and `style=""` attributes.
///
/// Conservative subset of SVGO `minifyStyles` (CSSO) / oxvg LightningCSS:
/// strip comments, collapse safe whitespace, drop empty declarations and empty
/// rules, drop SVG default declarations (`opacity:1`, `fill-rule:nonzero`, …).
/// Does **not** restructure selectors or drop unused rules — `.ocean` / `.land`
/// stay even if a later unused-selector pass would prune them.
/// Non-zero CSS `px` is kept (`transform-origin:140px 110px`, `font-size:12px`);
/// unitless lengths are invalid CSS and browsers drop them.
pub fn minify_styles(doc: &mut Document) {
    doc.walk_elements_mut(&mut |el| {
        if el.local_name() == "style" {
            if el
                .attr("type")
                .is_some_and(|t| t.eq_ignore_ascii_case("text/css"))
            {
                el.remove_attr("type");
            }
            for child in &mut el.children {
                if let Node::Text(t) = child {
                    *t = minify_css(t);
                }
            }
        }
        if let Some(style) = el.attr("style").map(str::to_string) {
            let mini = minify_css(&style);
            if mini.is_empty() {
                el.remove_attr("style");
            } else {
                el.set_attr("style", mini);
            }
        }
    });
}

/// `convertEllipseToCircle` — `rx == ry` only (SVGO / oxvg default).
///
/// Skips an ellipse that already has a SMIL child targeting `rx` / `ry` / `r`.
pub fn convert_ellipse_to_circle(doc: &mut Document) {
    doc.walk_elements_mut(&mut |el| {
        if el.local_name() != "ellipse" {
            return;
        }
        if ellipse_has_animated_radii(el) {
            return;
        }
        let rx = el
            .attr("rx")
            .map(|s| s.trim().to_string())
            .unwrap_or_default();
        let ry = el
            .attr("ry")
            .map(|s| s.trim().to_string())
            .unwrap_or_default();
        if rx.is_empty() && ry.is_empty() {
            return;
        }
        let a = if rx.is_empty() {
            ry.clone()
        } else {
            rx.clone()
        };
        let b = if ry.is_empty() { rx } else { ry };
        if lossless_numeric(&a) != lossless_numeric(&b) {
            return;
        }
        el.name = "circle".into();
        el.remove_attr("rx");
        el.remove_attr("ry");
        el.set_attr("r", lossless_numeric(&a));
    });
}

// ---------------------------------------------------------------------------
// convertColors
// ---------------------------------------------------------------------------

/// Convert color literals to the shortest equivalent (name / `#rgb` / `#rrggbb`).
///
/// Matches SVGO `convertColors` defaults: `names2hex`, `rgb2hex`, `shorthex`,
/// `shortname`, `convertCase: lower`. `currentColor` is **off**. RGBA/HSLA with
/// alpha ≠ 1 is left untouched.
pub fn convert_colors(doc: &mut Document) {
    doc.walk_elements_mut(&mut |el| {
        let keys: Vec<String> = el.attrs.iter().map(|(k, _)| k.clone()).collect();
        for k in keys {
            if is_color_attr(&k) {
                if let Some(v) = el.attr(&k).map(str::to_string) {
                    el.set_attr(&k, convert_color_value(&v));
                }
            }
        }
        if let Some(style) = el.attr("style").map(str::to_string) {
            el.set_attr("style", rewrite_css_colors(&style));
        }
        if el.local_name() == "style" {
            for child in &mut el.children {
                if let Node::Text(t) = child {
                    *t = rewrite_css_colors(t);
                }
            }
        }
    });
}

// ---------------------------------------------------------------------------
// cleanupNumericValues (lossless — motion-OK)
// ---------------------------------------------------------------------------

/// Compact numeric presentation attrs. Rounds to
/// [`NON_PATH_FLOAT_PRECISION`] (SVGO `cleanupNumericValues` default = 3).
///
/// `0.18` → `.18`, `1.50px` → `1.5`, `60.9866` → `60.987`. Does **not**
/// touch SMIL timing (`begin` / `end` / `dur` / `values` / `keyTimes` /
/// `keySplines`) or path `d`.
pub fn cleanup_numeric_values(doc: &mut Document) {
    doc.walk_elements_mut(&mut |el| {
        let smil = is_smil_element(el.local_name());
        let keys: Vec<String> = el.attrs.iter().map(|(k, _)| k.clone()).collect();
        for k in keys {
            if skip_numeric_attr(smil, &k) {
                continue;
            }
            if !is_numeric_attr(&k) {
                continue;
            }
            if let Some(v) = el.attr(&k).map(str::to_string) {
                let next = minify_numeric_attr(&v, true);
                if next != v {
                    el.set_attr(&k, next);
                }
            }
        }
    });
}

pub(crate) fn is_smil_element(local: &str) -> bool {
    matches!(
        local.to_ascii_lowercase().as_str(),
        "animate" | "animatetransform" | "animatemotion" | "animatecolor" | "set"
    )
}

fn skip_numeric_attr(smil: bool, name: &str) -> bool {
    matches!(
        name,
        "begin"
            | "end"
            | "dur"
            | "values"
            | "keyTimes"
            | "keySplines"
            | "min"
            | "max"
            | "repeatDur"
            | "d"
            | "style"
            | "class"
            | "id"
            | "href"
            | "xlink:href"
            | "transform"
    ) || (smil && matches!(name, "from" | "to" | "by"))
}

fn is_numeric_attr(name: &str) -> bool {
    matches!(
        name,
        "x" | "y"
            | "x1"
            | "y1"
            | "x2"
            | "y2"
            | "cx"
            | "cy"
            | "r"
            | "rx"
            | "ry"
            | "width"
            | "height"
            | "stroke-width"
            | "stroke-miterlimit"
            | "stroke-dashoffset"
            | "stroke-dasharray"
            | "opacity"
            | "offset"
            | "dx"
            | "dy"
            | "font-size"
            | "letter-spacing"
            | "word-spacing"
            | "startOffset"
            | "pathLength"
            | "stdDeviation"
            | "baseFrequency"
            | "viewBox"
            | "points"
    ) || name.ends_with("-opacity")
}

fn minify_numeric_attr(raw: &str, round: bool) -> String {
    let t = raw.trim();
    if t.is_empty() || t.eq_ignore_ascii_case("none") || t.eq_ignore_ascii_case("inherit") {
        return raw.trim().to_string();
    }
    if t.contains(',') || t.contains(char::is_whitespace) {
        return minify_number_list(t, round);
    }
    minify_one_numeric(t, round)
}

fn lossless_numeric(tok: &str) -> String {
    minify_one_numeric(tok, false)
}

fn minify_number_list(s: &str, round: bool) -> String {
    let mut p = NumericParser::new(s);
    let mut out = String::new();
    loop {
        p.skip_sep();
        if p.done() {
            break;
        }
        let start = p.i;
        if p.parse_lexeme().is_none() {
            // leave the remainder untouched
            out.push_str(s[start..].trim());
            break;
        }
        let tok = &s[start..p.i];
        let mini = minify_one_numeric(tok.trim(), round);
        if needs_numeric_sep(&out, &mini) {
            out.push(' ');
        }
        out.push_str(&mini);
    }
    if out.is_empty() {
        raw_or_trim(s)
    } else {
        out
    }
}

fn raw_or_trim(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn needs_numeric_sep(out: &str, next: &str) -> bool {
    let Some(prev) = out.chars().next_back() else {
        return false;
    };
    if next.starts_with('-') {
        return false;
    }
    if next.starts_with('.') {
        return !out
            .chars()
            .rev()
            .take_while(|c| c.is_ascii_digit() || *c == '.')
            .any(|c| c == '.');
    }
    prev.is_ascii_digit() || prev == '.'
}

fn minify_one_numeric(tok: &str, round: bool) -> String {
    let tok = tok.trim();
    let (num, unit) = split_trailing_unit(tok);
    let compact = if round {
        round_then_compact(num)
    } else {
        crate::plugin_paths::minify_number_lexeme(num)
    };
    if unit.eq_ignore_ascii_case("px") || unit.is_empty() {
        return compact;
    }
    // Keep em/%/s/… — still compact the mantissa.
    format!("{compact}{unit}")
}

fn round_then_compact(num: &str) -> String {
    let Ok(v) = num.parse::<f64>() else {
        return crate::plugin_paths::minify_number_lexeme(num);
    };
    if !v.is_finite() {
        return crate::plugin_paths::minify_number_lexeme(num);
    }
    let factor = 10f64.powi(NON_PATH_FLOAT_PRECISION);
    let rounded = (v * factor).round() / factor;
    // Enough decimals for the precision, then strip trailing zeros.
    let formatted = format!("{rounded:.3}");
    crate::plugin_paths::minify_number_lexeme(&formatted)
}

fn ellipse_has_animated_radii(el: &Element) -> bool {
    el.children.iter().any(|n| match n {
        Node::Element(c) if is_smil_element(c.local_name()) => c
            .attr("attributeName")
            .is_some_and(|a| matches!(a, "rx" | "ry" | "r")),
        _ => false,
    })
}

fn split_trailing_unit(s: &str) -> (&str, &str) {
    let bytes = s.as_bytes();
    let mut i = bytes.len();
    while i > 0 && bytes[i - 1].is_ascii_alphabetic() {
        i -= 1;
    }
    if i == 0 || i == bytes.len() {
        if s.ends_with('%') {
            return (&s[..s.len() - 1], "%");
        }
        return (s, "");
    }
    (&s[..i], &s[i..])
}

struct NumericParser<'a> {
    b: &'a [u8],
    i: usize,
}

impl<'a> NumericParser<'a> {
    fn new(s: &'a str) -> Self {
        Self {
            b: s.as_bytes(),
            i: 0,
        }
    }
    fn done(&self) -> bool {
        self.i >= self.b.len()
    }
    fn skip_sep(&mut self) {
        while self.i < self.b.len() {
            let c = self.b[self.i];
            if c == b',' || c == b' ' || c == b'\t' || c == b'\n' || c == b'\r' {
                self.i += 1;
            } else {
                break;
            }
        }
    }
    fn parse_lexeme(&mut self) -> Option<()> {
        let start = self.i;
        if matches!(self.b.get(self.i), Some(b'+') | Some(b'-')) {
            self.i += 1;
        }
        let mut saw = false;
        let mut dot = false;
        while self.i < self.b.len() {
            let c = self.b[self.i];
            if c.is_ascii_digit() {
                saw = true;
                self.i += 1;
            } else if c == b'.' && !dot {
                dot = true;
                self.i += 1;
            } else {
                break;
            }
        }
        while self.i < self.b.len() && self.b[self.i].is_ascii_alphabetic() {
            self.i += 1;
        }
        if self.i < self.b.len() && self.b[self.i] == b'%' {
            self.i += 1;
        }
        if saw || (dot && self.i > start) {
            Some(())
        } else {
            self.i = start;
            None
        }
    }
}

// ---------------------------------------------------------------------------
// cleanupIds (unused only)
// ---------------------------------------------------------------------------

/// Drop unreferenced `id` / `xml:id` attributes. Does **not** minify names.
///
/// Safer than a full SVGO `cleanupIds` pass: documents with `<script>` or
/// `on*` handlers are left alone (scripts may look up nodes by id). `<style>`
/// does **not** abort the pass — `#id` selectors and `url(#id)` are collected
/// instead, which is enough for unused-only removal.
pub fn cleanup_ids(doc: &mut Document) {
    if document_has_script(&doc.nodes) {
        return;
    }
    let mut referenced = HashSet::new();
    collect_refs(&doc.nodes, &mut referenced);
    strip_unused_ids(&mut doc.nodes, &referenced);
}

// ---------------------------------------------------------------------------
// removeUselessStrokeAndFill
// ---------------------------------------------------------------------------

/// Drop redundant stroke/fill presentation attributes (and matching `style`
/// declarations) when they cannot change painting.
///
/// Improvements over a naive “delete `fill=black` everywhere” pass:
/// * inheritance-aware (do not strip a child’s `fill="#000"` when a parent
///   set a different fill);
/// * never mutates `g` / `svg` / other inheriting containers;
/// * never deletes elements (`removeNone` stays false);
/// * treats `stroke-width:0` / `stroke-opacity:0` as a disabled stroke.
pub fn remove_useless_stroke_and_fill(doc: &mut Document) {
    let root = PaintCtx::svg_defaults();
    for node in &mut doc.nodes {
        walk_paint(node, &root);
    }
}

// ---------------------------------------------------------------------------
// collapseWhitespace
// ---------------------------------------------------------------------------

/// Remove ignorable whitespace text nodes outside text-ish containers.
///
/// Honors `xml:space="preserve"` / `"default"` (inherited). Does not rewrite
/// the contents of remaining text nodes.
pub fn collapse_whitespace(doc: &mut Document) {
    collapse_ws_nodes(&mut doc.nodes, false);
}

// ===========================================================================
// CSS minify
// ===========================================================================

fn minify_css(css: &str) -> String {
    let stripped = strip_css_comments(css);
    let collapsed = collapse_css_ws(&stripped);
    let emptied = drop_empty_css(&collapsed);
    let compacted = if emptied.contains('{') {
        compact_stylesheet(&emptied)
    } else {
        compact_decl_list(&emptied)
    };
    drop_empty_css(&compacted)
}

/// Drop SVG initial-value declarations and lossless-minify numbers.
///
/// Does **not** delete class selectors. `@keyframes` / other at-rules are
/// copied through so `opacity:1` inside a keyframe stays (motion).
fn compact_stylesheet(css: &str) -> String {
    let mut out = String::with_capacity(css.len());
    let mut rest = css;
    while !rest.is_empty() {
        match next_css_rule(rest) {
            Some((selector, body, tail)) => {
                if selector.trim_start().starts_with('@') {
                    out.push_str(selector.trim());
                    out.push('{');
                    out.push_str(body);
                    out.push('}');
                } else {
                    let decls = compact_decl_list(body);
                    if !decls.is_empty() {
                        out.push_str(selector.trim());
                        out.push('{');
                        out.push_str(&decls);
                        out.push('}');
                    }
                }
                rest = tail;
            }
            None => {
                out.push_str(rest);
                break;
            }
        }
    }
    out
}

fn next_css_rule(css: &str) -> Option<(&str, &str, &str)> {
    let bytes = css.as_bytes();
    let mut i = 0;
    let mut quote: Option<u8> = None;
    while i < bytes.len() {
        let b = bytes[i];
        if let Some(q) = quote {
            if b == b'\\' {
                i += 2;
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
        if b == b'{' {
            let selector = &css[..i];
            let (body, after) = css_brace_body(&css[i + 1..])?;
            return Some((selector, body, after));
        }
        i += 1;
    }
    None
}

fn css_brace_body(after_open: &str) -> Option<(&str, &str)> {
    let bytes = after_open.as_bytes();
    let mut depth = 1i32;
    let mut i = 0;
    let mut quote: Option<u8> = None;
    while i < bytes.len() {
        let b = bytes[i];
        if let Some(q) = quote {
            if b == b'\\' {
                i += 2;
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
        if b == b'{' {
            depth += 1;
        } else if b == b'}' {
            depth -= 1;
            if depth == 0 {
                return Some((&after_open[..i], &after_open[i + 1..]));
            }
        }
        i += 1;
    }
    None
}

fn compact_decl_list(body: &str) -> String {
    parse_style_decls(body)
        .into_iter()
        .filter(|(k, v)| !is_default_css_decl(k, v))
        .map(|(k, v)| format!("{k}:{}", minify_css_value(&v)))
        .collect::<Vec<_>>()
        .join(";")
}

fn minify_css_value(v: &str) -> String {
    let t = v.trim();
    if t.is_empty() {
        return t.to_string();
    }
    // Lossless only — do not precision-3 `stroke-width:.99986893` on the map.
    // `url(` / `var(` / `calc(` all contain `(`; leave the argument intact.
    if t.contains('(') {
        return t.to_string();
    }
    // CSS lengths (stylesheet + `style=""`) must keep non-zero `px`.
    // `transform-origin:140 110` and `font-size:12` are invalid CSS and
    // browsers drop them (onsen bob/tilt origin, heraldry type size).
    // Presentation attrs still strip `px` via `cleanupNumericValues`.
    minify_css_length_list(t)
}

/// Compact a CSS length list without stripping required `px`.
fn minify_css_length_list(s: &str) -> String {
    let mut p = NumericParser::new(s);
    let mut out = String::new();
    loop {
        p.skip_sep();
        if p.done() {
            break;
        }
        let start = p.i;
        if p.parse_lexeme().is_none() {
            out.push_str(s[start..].trim());
            break;
        }
        let tok = &s[start..p.i];
        let mini = minify_one_css_numeric(tok.trim());
        // CSS value lists need a real space (`140px 110px`). Digit-glue
        // rules from path/attr minify would emit `140px110px`.
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(&mini);
    }
    if out.is_empty() {
        raw_or_trim(s)
    } else {
        out
    }
}

fn minify_one_css_numeric(tok: &str) -> String {
    let (num, unit) = split_trailing_unit(tok);
    let compact = crate::plugin_paths::minify_number_lexeme(num);
    if unit.eq_ignore_ascii_case("px") {
        if is_zero_number(&compact) {
            return compact;
        }
        return format!("{compact}px");
    }
    if unit.is_empty() {
        return compact;
    }
    format!("{compact}{unit}")
}

fn is_default_css_decl(prop: &str, value: &str) -> bool {
    let p = prop.trim();
    let v = value.trim().to_ascii_lowercase();
    match p {
        "fill-opacity" | "stroke-opacity" | "stop-opacity" | "flood-opacity" => is_one(&v),
        "opacity" => is_one(&v),
        "stroke-dasharray" => v == "none",
        "stroke-dashoffset" => is_zero_number(&v),
        "stroke-linecap" => v == "butt",
        "stroke-linejoin" => v == "miter",
        "fill-rule" => v == "nonzero",
        "visibility" => v == "visible",
        "display" => v == "inline",
        "overflow" => v == "visible",
        "marker" | "marker-start" | "marker-mid" | "marker-end" => v == "none",
        "color" => matches!(v.as_str(), "#000" | "#000000" | "black"),
        _ => false,
    }
}

fn strip_css_comments(css: &str) -> String {
    let mut out = String::with_capacity(css.len());
    let mut chars = css.chars().peekable();
    let mut quote: Option<char> = None;
    while let Some(ch) = chars.next() {
        if let Some(q) = quote {
            out.push(ch);
            if ch == '\\' {
                if let Some(n) = chars.next() {
                    out.push(n);
                }
            } else if ch == q {
                quote = None;
            }
            continue;
        }
        if ch == '"' || ch == '\'' {
            quote = Some(ch);
            out.push(ch);
            continue;
        }
        if ch == '/' && chars.peek() == Some(&'*') {
            chars.next();
            loop {
                match chars.next() {
                    None => break,
                    Some('*') if chars.peek() == Some(&'/') => {
                        chars.next();
                        break;
                    }
                    _ => {}
                }
            }
            // Keep a separator so `a/**/b` does not become `ab`.
            if !out.ends_with(char::is_whitespace) {
                out.push(' ');
            }
            continue;
        }
        out.push(ch);
    }
    out
}

fn collapse_css_ws(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut quote: Option<char> = None;
    let mut prev_ws = false;
    let mut chars = s.chars().peekable();
    while let Some(ch) = chars.next() {
        if let Some(q) = quote {
            out.push(ch);
            if ch == '\\' {
                if let Some(n) = chars.next() {
                    out.push(n);
                }
            } else if ch == q {
                quote = None;
            }
            prev_ws = false;
            continue;
        }
        if ch == '"' || ch == '\'' {
            quote = Some(ch);
            prev_ws = false;
            out.push(ch);
            continue;
        }
        if ch.is_whitespace() {
            if !prev_ws {
                out.push(' ');
                prev_ws = true;
            }
            continue;
        }
        if matches!(ch, '{' | '}' | ':' | ';' | ',') {
            if out.ends_with(' ') {
                out.pop();
            }
            out.push(ch);
            prev_ws = true;
            continue;
        }
        if ch == '!' {
            if out.ends_with(' ') {
                out.pop();
            }
            out.push(ch);
            prev_ws = false;
            continue;
        }
        prev_ws = false;
        out.push(ch);
    }
    out.trim().trim_end_matches(';').to_string()
}

fn drop_empty_css(s: &str) -> String {
    let mut out = s.to_string();
    // `prop:;` or `prop:}` → drop the empty declaration.
    loop {
        let next = drop_one_empty_decl(&out);
        if next == out {
            break;
        }
        out = next;
    }
    // `selector{}` → drop the empty rule (including empty at-rules).
    loop {
        let next = drop_one_empty_rule(&out);
        if next == out {
            break;
        }
        out = next;
    }
    while out.contains(";}") {
        out = out.replace(";}", "}");
    }
    out.trim().trim_end_matches(';').to_string()
}

fn drop_one_empty_decl(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b':' {
            let mut j = i + 1;
            while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                j += 1;
            }
            if j < bytes.len() && (bytes[j] == b';' || bytes[j] == b'}') {
                // Walk back the property name.
                let mut start = i;
                while start > 0 {
                    let c = bytes[start - 1];
                    if c.is_ascii_whitespace() || matches!(c, b';' | b'{' | b'}') {
                        break;
                    }
                    start -= 1;
                }
                let mut end = j;
                if bytes[j] == b';' {
                    end = j + 1;
                }
                let mut out = String::with_capacity(s.len());
                out.push_str(&s[..start]);
                out.push_str(&s[end..]);
                return out;
            }
        }
        i += 1;
    }
    s.to_string()
}

fn drop_one_empty_rule(s: &str) -> String {
    if let Some(idx) = s.find("{}") {
        let mut start = idx;
        while start > 0 {
            let c = s.as_bytes()[start - 1];
            if c == b'}' || c == b';' {
                break;
            }
            start -= 1;
        }
        let mut out = String::with_capacity(s.len());
        out.push_str(&s[..start]);
        out.push_str(&s[idx + 2..]);
        return out;
    }
    s.to_string()
}

// ===========================================================================
// Color conversion
// ===========================================================================

fn is_color_attr(name: &str) -> bool {
    matches!(name, "fill" | "stroke" | "color" | "viewport-fill") || name.ends_with("-color")
}

fn is_untouched_color_keyword(s: &str) -> bool {
    matches!(
        s,
        "none"
            | "inherit"
            | "initial"
            | "unset"
            | "revert"
            | "currentcolor"
            | "context-fill"
            | "context-stroke"
            | "transparent"
    )
}

/// `url(#Id)` and `var(--x)` fragments are case-sensitive in SVG/CSS.
/// Detect the function case-insensitively; never fold the argument.
fn is_case_sensitive_paint_ref(s: &str) -> bool {
    let lower = s.to_ascii_lowercase();
    lower.starts_with("url(") || lower.starts_with("var(")
}

fn convert_color_value(raw: &str) -> String {
    let s = raw.trim();
    if s.is_empty() {
        return raw.to_string();
    }
    if is_case_sensitive_paint_ref(s) {
        return s.to_string();
    }
    let lower = s.to_ascii_lowercase();
    if is_untouched_color_keyword(&lower) {
        return lower;
    }
    let rgb = if let Some(hex) = named_to_rgb(&lower) {
        Some(hex)
    } else if let Some(hex) = parse_hex(&lower) {
        Some(hex)
    } else if let Some(hex) = parse_rgb_fn(&lower) {
        Some(hex)
    } else {
        parse_hsl_fn(&lower)
    };
    match rgb {
        Some(rgb) => shortest_color(rgb),
        None => s.to_string(),
    }
}

fn rewrite_css_colors(css: &str) -> String {
    let mut out = String::with_capacity(css.len());
    let mut rest = css;
    while !rest.is_empty() {
        if let Some(after_colon) = next_color_prop(rest) {
            let prefix_end = rest.len() - after_colon.len();
            out.push_str(&rest[..prefix_end]);
            let (value, tail) = split_decl_value(after_colon);
            out.push_str(&convert_color_value(&value));
            rest = tail;
        } else {
            out.push_str(rest);
            break;
        }
    }
    out
}

fn next_color_prop(s: &str) -> Option<&str> {
    let bytes = s.as_bytes();
    let mut i = 0;
    let mut quote: Option<u8> = None;
    while i < bytes.len() {
        let b = bytes[i];
        if let Some(q) = quote {
            if b == b'\\' {
                i += 2;
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
        if b == b':' {
            let mut start = i;
            while start > 0 {
                let c = bytes[start - 1];
                if c.is_ascii_whitespace() || matches!(c, b';' | b'{' | b'}' | b',') {
                    break;
                }
                start -= 1;
            }
            let prop = s[start..i].trim();
            if is_color_attr(prop) {
                return Some(&s[i + 1..]);
            }
        }
        i += 1;
    }
    None
}

fn split_decl_value(s: &str) -> (String, &str) {
    let bytes = s.as_bytes();
    let mut i = 0;
    let mut quote: Option<u8> = None;
    let mut depth = 0i32;
    while i < bytes.len() {
        let b = bytes[i];
        if let Some(q) = quote {
            if b == b'\\' {
                i += 2;
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
        if b == b'(' {
            depth += 1;
        } else if b == b')' && depth > 0 {
            depth -= 1;
        } else if depth == 0 && (b == b';' || b == b'}') {
            break;
        }
        i += 1;
    }
    (s[..i].trim().to_string(), &s[i..])
}

#[derive(Clone, Copy)]
struct Rgb {
    r: u8,
    g: u8,
    b: u8,
}

fn shortest_color(rgb: Rgb) -> String {
    let hex = format_hex(rgb, true);
    let long = format_hex(rgb, false);
    let mut best = if hex.len() <= long.len() { hex } else { long };
    if let Some(name) = rgb_to_shortest_name(rgb) {
        if name.len() < best.len() {
            best = name.to_string();
        }
    }
    best
}

fn format_hex(rgb: Rgb, short: bool) -> String {
    if short && nybble(rgb.r).is_some() && nybble(rgb.g).is_some() && nybble(rgb.b).is_some() {
        return format!("#{:x}{:x}{:x}", rgb.r >> 4, rgb.g >> 4, rgb.b >> 4);
    }
    format!("#{:02x}{:02x}{:02x}", rgb.r, rgb.g, rgb.b)
}

fn nybble(v: u8) -> Option<u8> {
    if v >> 4 == v & 0x0f {
        Some(v >> 4)
    } else {
        None
    }
}

fn parse_hex(s: &str) -> Option<Rgb> {
    let t = s.strip_prefix('#')?;
    if !t.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    match t.len() {
        3 => Some(Rgb {
            r: hex_pair(t.as_bytes()[0], t.as_bytes()[0])?,
            g: hex_pair(t.as_bytes()[1], t.as_bytes()[1])?,
            b: hex_pair(t.as_bytes()[2], t.as_bytes()[2])?,
        }),
        4 => {
            let a = hex_pair(t.as_bytes()[3], t.as_bytes()[3])?;
            if a != 255 {
                return None;
            }
            Some(Rgb {
                r: hex_pair(t.as_bytes()[0], t.as_bytes()[0])?,
                g: hex_pair(t.as_bytes()[1], t.as_bytes()[1])?,
                b: hex_pair(t.as_bytes()[2], t.as_bytes()[2])?,
            })
        }
        6 => Some(Rgb {
            r: hex_pair(t.as_bytes()[0], t.as_bytes()[1])?,
            g: hex_pair(t.as_bytes()[2], t.as_bytes()[3])?,
            b: hex_pair(t.as_bytes()[4], t.as_bytes()[5])?,
        }),
        8 => {
            let a = hex_pair(t.as_bytes()[6], t.as_bytes()[7])?;
            if a != 255 {
                return None;
            }
            Some(Rgb {
                r: hex_pair(t.as_bytes()[0], t.as_bytes()[1])?,
                g: hex_pair(t.as_bytes()[2], t.as_bytes()[3])?,
                b: hex_pair(t.as_bytes()[4], t.as_bytes()[5])?,
            })
        }
        _ => None,
    }
}

fn hex_pair(a: u8, b: u8) -> Option<u8> {
    let hi = hex_val(a)?;
    let lo = hex_val(b)?;
    Some((hi << 4) | lo)
}

fn hex_val(b: u8) -> Option<u8> {
    // `parse_hex` is only fed lowercased input.
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        _ => None,
    }
}

/// `rgb()` / `rgba()` — comma or space syntax. Alpha must be omitted or 1.
fn parse_rgb_fn(s: &str) -> Option<Rgb> {
    let (inner, has_rgba) = if let Some(rest) = s.strip_prefix("rgba(") {
        (rest.strip_suffix(')')?, true)
    } else if let Some(rest) = s.strip_prefix("rgb(") {
        (rest.strip_suffix(')')?, false)
    } else {
        return None;
    };
    let (channels, alpha) = split_color_fn_args(inner)?;
    if channels.len() != 3 {
        return None;
    }
    if (has_rgba || alpha.is_some()) && !is_one(alpha.unwrap_or("1")) {
        return None;
    }
    Some(Rgb {
        r: parse_rgb_channel(channels[0])?,
        g: parse_rgb_channel(channels[1])?,
        b: parse_rgb_channel(channels[2])?,
    })
}

/// `hsl()` / `hsla()` — comma or space syntax. Alpha must be omitted or 1.
fn parse_hsl_fn(s: &str) -> Option<Rgb> {
    let (inner, has_hsla) = if let Some(rest) = s.strip_prefix("hsla(") {
        (rest.strip_suffix(')')?, true)
    } else if let Some(rest) = s.strip_prefix("hsl(") {
        (rest.strip_suffix(')')?, false)
    } else {
        return None;
    };
    let (channels, alpha) = split_color_fn_args(inner)?;
    if channels.len() != 3 {
        return None;
    }
    if (has_hsla || alpha.is_some()) && !is_one(alpha.unwrap_or("1")) {
        return None;
    }
    let h = parse_hue(channels[0])?;
    let sat = parse_percent(channels[1])?;
    let lit = parse_percent(channels[2])?;
    Some(hsl_to_rgb(h, sat, lit))
}

fn split_color_fn_args(inner: &str) -> Option<(Vec<&str>, Option<&str>)> {
    let inner = inner.trim();
    if inner.is_empty() {
        return None;
    }
    if let Some((body, a)) = inner.split_once('/') {
        let ch: Vec<&str> = body.split_whitespace().collect();
        return Some((ch, Some(a.trim())));
    }
    if inner.contains(',') {
        let parts: Vec<&str> = inner.split(',').map(str::trim).collect();
        if parts.len() == 4 {
            return Some((parts[..3].to_vec(), Some(parts[3])));
        }
        if parts.len() == 3 {
            return Some((parts, None));
        }
        return None;
    }
    let parts: Vec<&str> = inner.split_whitespace().collect();
    if parts.len() == 3 {
        Some((parts, None))
    } else {
        None
    }
}

fn parse_rgb_channel(s: &str) -> Option<u8> {
    let s = s.trim();
    if let Some(p) = s.strip_suffix('%') {
        let n: f32 = p.parse().ok()?;
        Some(((n.clamp(0.0, 100.0) / 100.0) * 255.0).round() as u8)
    } else {
        let n: f32 = s.parse().ok()?;
        Some(n.clamp(0.0, 255.0).round() as u8)
    }
}

fn parse_percent(s: &str) -> Option<f32> {
    let t = s.trim().strip_suffix('%')?;
    let n: f32 = t.parse().ok()?;
    Some((n.clamp(0.0, 100.0)) / 100.0)
}

fn parse_hue(s: &str) -> Option<f32> {
    // Caller already lowercased the color function.
    let t = s.trim().trim_end_matches("deg").trim();
    if t.ends_with("rad") || t.ends_with("turn") || t.ends_with("grad") {
        return None;
    }
    t.parse::<f32>().ok().map(|h| ((h % 360.0) + 360.0) % 360.0)
}

fn hsl_to_rgb(h: f32, s: f32, l: f32) -> Rgb {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let hp = h / 60.0;
    let x = c * (1.0 - ((hp % 2.0) - 1.0).abs());
    let (r1, g1, b1) = if hp < 1.0 {
        (c, x, 0.0)
    } else if hp < 2.0 {
        (x, c, 0.0)
    } else if hp < 3.0 {
        (0.0, c, x)
    } else if hp < 4.0 {
        (0.0, x, c)
    } else if hp < 5.0 {
        (x, 0.0, c)
    } else {
        (c, 0.0, x)
    };
    let m = l - c / 2.0;
    Rgb {
        r: ((r1 + m) * 255.0).round().clamp(0.0, 255.0) as u8,
        g: ((g1 + m) * 255.0).round().clamp(0.0, 255.0) as u8,
        b: ((b1 + m) * 255.0).round().clamp(0.0, 255.0) as u8,
    }
}

fn named_to_rgb(name: &str) -> Option<Rgb> {
    for &(n, r, g, b) in NAMED_COLORS {
        if n == name {
            return Some(Rgb { r, g, b });
        }
    }
    None
}

fn rgb_to_shortest_name(rgb: Rgb) -> Option<&'static str> {
    let mut best: Option<&'static str> = None;
    for &(n, r, g, b) in NAMED_COLORS {
        if r == rgb.r && g == rgb.g && b == rgb.b {
            match best {
                None => best = Some(n),
                Some(prev) if n.len() < prev.len() => best = Some(n),
                _ => {}
            }
        }
    }
    best
}

/// CSS Color Module named colors (W3C), plus `grey` aliases.
/// Public keyword table — not copied from any optimizer.
const NAMED_COLORS: &[(&str, u8, u8, u8)] = &[
    ("aliceblue", 240, 248, 255),
    ("antiquewhite", 250, 235, 215),
    ("aqua", 0, 255, 255),
    ("aquamarine", 127, 255, 212),
    ("azure", 240, 255, 255),
    ("beige", 245, 245, 220),
    ("bisque", 255, 228, 196),
    ("black", 0, 0, 0),
    ("blanchedalmond", 255, 235, 205),
    ("blue", 0, 0, 255),
    ("blueviolet", 138, 43, 226),
    ("brown", 165, 42, 42),
    ("burlywood", 222, 184, 135),
    ("cadetblue", 95, 158, 160),
    ("chartreuse", 127, 255, 0),
    ("chocolate", 210, 105, 30),
    ("coral", 255, 127, 80),
    ("cornflowerblue", 100, 149, 237),
    ("cornsilk", 255, 248, 220),
    ("crimson", 220, 20, 60),
    ("cyan", 0, 255, 255),
    ("darkblue", 0, 0, 139),
    ("darkcyan", 0, 139, 139),
    ("darkgoldenrod", 184, 134, 11),
    ("darkgray", 169, 169, 169),
    ("darkgreen", 0, 100, 0),
    ("darkgrey", 169, 169, 169),
    ("darkkhaki", 189, 183, 107),
    ("darkmagenta", 139, 0, 139),
    ("darkolivegreen", 85, 107, 47),
    ("darkorange", 255, 140, 0),
    ("darkorchid", 153, 50, 204),
    ("darkred", 139, 0, 0),
    ("darksalmon", 233, 150, 122),
    ("darkseagreen", 143, 188, 143),
    ("darkslateblue", 72, 61, 139),
    ("darkslategray", 47, 79, 79),
    ("darkslategrey", 47, 79, 79),
    ("darkturquoise", 0, 206, 209),
    ("darkviolet", 148, 0, 211),
    ("deeppink", 255, 20, 147),
    ("deepskyblue", 0, 191, 255),
    ("dimgray", 105, 105, 105),
    ("dimgrey", 105, 105, 105),
    ("dodgerblue", 30, 144, 255),
    ("firebrick", 178, 34, 34),
    ("floralwhite", 255, 250, 240),
    ("forestgreen", 34, 139, 34),
    ("fuchsia", 255, 0, 255),
    ("gainsboro", 220, 220, 220),
    ("ghostwhite", 248, 248, 255),
    ("gold", 255, 215, 0),
    ("goldenrod", 218, 165, 32),
    ("gray", 128, 128, 128),
    ("green", 0, 128, 0),
    ("greenyellow", 173, 255, 47),
    ("grey", 128, 128, 128),
    ("honeydew", 240, 255, 240),
    ("hotpink", 255, 105, 180),
    ("indianred", 205, 92, 92),
    ("indigo", 75, 0, 130),
    ("ivory", 255, 255, 240),
    ("khaki", 240, 230, 140),
    ("lavender", 230, 230, 250),
    ("lavenderblush", 255, 240, 245),
    ("lawngreen", 124, 252, 0),
    ("lemonchiffon", 255, 250, 205),
    ("lightblue", 173, 216, 230),
    ("lightcoral", 240, 128, 128),
    ("lightcyan", 224, 255, 255),
    ("lightgoldenrodyellow", 250, 250, 210),
    ("lightgray", 211, 211, 211),
    ("lightgreen", 144, 238, 144),
    ("lightgrey", 211, 211, 211),
    ("lightpink", 255, 182, 193),
    ("lightsalmon", 255, 160, 122),
    ("lightseagreen", 32, 178, 170),
    ("lightskyblue", 135, 206, 250),
    ("lightslategray", 119, 136, 153),
    ("lightslategrey", 119, 136, 153),
    ("lightsteelblue", 176, 196, 222),
    ("lightyellow", 255, 255, 224),
    ("lime", 0, 255, 0),
    ("limegreen", 50, 205, 50),
    ("linen", 250, 240, 230),
    ("magenta", 255, 0, 255),
    ("maroon", 128, 0, 0),
    ("mediumaquamarine", 102, 205, 170),
    ("mediumblue", 0, 0, 205),
    ("mediumorchid", 186, 85, 211),
    ("mediumpurple", 147, 112, 219),
    ("mediumseagreen", 60, 179, 113),
    ("mediumslateblue", 123, 104, 238),
    ("mediumspringgreen", 0, 250, 154),
    ("mediumturquoise", 72, 209, 204),
    ("mediumvioletred", 199, 21, 133),
    ("midnightblue", 25, 25, 112),
    ("mintcream", 245, 255, 250),
    ("mistyrose", 255, 228, 225),
    ("moccasin", 255, 228, 181),
    ("navajowhite", 255, 222, 173),
    ("navy", 0, 0, 128),
    ("oldlace", 253, 245, 230),
    ("olive", 128, 128, 0),
    ("olivedrab", 107, 142, 35),
    ("orange", 255, 165, 0),
    ("orangered", 255, 69, 0),
    ("orchid", 218, 112, 214),
    ("palegoldenrod", 238, 232, 170),
    ("palegreen", 152, 251, 152),
    ("paleturquoise", 175, 238, 238),
    ("palevioletred", 219, 112, 147),
    ("papayawhip", 255, 239, 213),
    ("peachpuff", 255, 218, 185),
    ("peru", 205, 133, 63),
    ("pink", 255, 192, 203),
    ("plum", 221, 160, 221),
    ("powderblue", 176, 224, 230),
    ("purple", 128, 0, 128),
    ("rebeccapurple", 102, 51, 153),
    ("red", 255, 0, 0),
    ("rosybrown", 188, 143, 143),
    ("royalblue", 65, 105, 225),
    ("saddlebrown", 139, 69, 19),
    ("salmon", 250, 128, 114),
    ("sandybrown", 244, 164, 96),
    ("seagreen", 46, 139, 87),
    ("seashell", 255, 245, 238),
    ("sienna", 160, 82, 45),
    ("silver", 192, 192, 192),
    ("skyblue", 135, 206, 235),
    ("slateblue", 106, 90, 205),
    ("slategray", 112, 128, 144),
    ("slategrey", 112, 128, 144),
    ("snow", 255, 250, 250),
    ("springgreen", 0, 255, 127),
    ("steelblue", 70, 130, 180),
    ("tan", 210, 180, 140),
    ("teal", 0, 128, 128),
    ("thistle", 216, 191, 216),
    ("tomato", 255, 99, 71),
    ("turquoise", 64, 224, 208),
    ("violet", 238, 130, 238),
    ("wheat", 245, 222, 179),
    ("white", 255, 255, 255),
    ("whitesmoke", 245, 245, 245),
    ("yellow", 255, 255, 0),
    ("yellowgreen", 154, 205, 50),
];

// ===========================================================================
// ID references
// ===========================================================================

fn document_has_script(nodes: &[Node]) -> bool {
    for node in nodes {
        if let Node::Element(el) = node {
            if el.local_name() == "script" {
                return true;
            }
            for (k, _) in &el.attrs {
                if is_event_attr(k) {
                    return true;
                }
            }
            if document_has_script(&el.children) {
                return true;
            }
        }
    }
    false
}

fn is_event_attr(name: &str) -> bool {
    // Presentation attrs (`opacity`, `orient`, …) do not start with `on`.
    let n = name.rsplit_once(':').map(|(_, l)| l).unwrap_or(name);
    n.to_ascii_lowercase().starts_with("on")
}

fn collect_refs(nodes: &[Node], out: &mut HashSet<String>) {
    for node in nodes {
        if let Node::Element(el) = node {
            for (k, v) in &el.attrs {
                push_attr_refs(k, v, out);
            }
            if el.local_name() == "style" {
                for child in &el.children {
                    if let Node::Text(t) = child {
                        collect_css_id_refs(t, out);
                    }
                }
            }
            collect_refs(&el.children, out);
        }
    }
}

fn push_attr_refs(key: &str, value: &str, out: &mut HashSet<String>) {
    let kl = key.to_ascii_lowercase();
    let local = kl.rsplit_once(':').map(|(_, l)| l).unwrap_or(kl.as_str());

    if local == "href" || local == "src" {
        push_hash_id(value, out);
    }
    if matches!(
        local,
        "aria-labelledby"
            | "aria-describedby"
            | "aria-controls"
            | "aria-owns"
            | "aria-flowto"
            | "aria-activedescendant"
    ) {
        for part in value.split_whitespace() {
            push_hash_id(part, out);
            if !part.starts_with('#') && is_name(part) {
                out.insert(part.to_string());
            }
        }
    }
    if matches!(local, "begin" | "end") {
        collect_smil_ids(value, out);
    }
    collect_url_ids(value, out);
}

fn push_hash_id(value: &str, out: &mut HashSet<String>) {
    let v = value.trim();
    if let Some(id) = v.strip_prefix('#') {
        if !id.is_empty() && !id.contains(['(', ' ', ')']) {
            out.insert(id.to_string());
        }
    }
}

pub(crate) fn collect_url_ids(value: &str, out: &mut HashSet<String>) {
    // Case-insensitive `url(`; the `#Id` argument stays verbatim.
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
                out.insert(id);
            }
        }
        cursor = open.saturating_add(1);
        if cursor >= value.len() {
            break;
        }
    }
}

fn collect_smil_ids(value: &str, out: &mut HashSet<String>) {
    // `other.begin`, `other.end`, `other.repeat(2)`, `other.click`
    for token in value.split([';', ',']) {
        let token = token.trim();
        if let Some((id, ev)) = token.split_once('.') {
            let id = id.trim();
            let ev_name = ev
                .split_once(['(', '+', '-', ' ', '\t'])
                .map(|(a, _)| a)
                .unwrap_or(ev)
                .trim();
            if is_name(id) && is_smil_event(ev_name) {
                out.insert(id.to_string());
            }
        }
    }
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

fn is_name(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == ':')
}

fn collect_css_id_refs(css: &str, out: &mut HashSet<String>) {
    collect_url_ids(css, out);
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
                out.insert(id);
            }
        }
    }
}

fn strip_unused_ids(nodes: &mut [Node], refs: &HashSet<String>) {
    for node in nodes.iter_mut() {
        if let Node::Element(el) = node {
            if let Some(id) = el.attr("id").map(str::to_string) {
                if !refs.contains(&id) {
                    el.remove_attr("id");
                }
            }
            if let Some(id) = el.attr("xml:id").map(str::to_string) {
                if !refs.contains(&id) {
                    el.remove_attr("xml:id");
                }
            }
            strip_unused_ids(&mut el.children, refs);
        }
    }
}

// ===========================================================================
// Stroke / fill (inheritance-aware)
// ===========================================================================

#[derive(Clone)]
struct PaintCtx {
    fill: String,
    stroke: String,
    fill_opacity: String,
    stroke_opacity: String,
    opacity: String,
    stroke_width: String,
}

impl PaintCtx {
    fn svg_defaults() -> Self {
        Self {
            fill: "black".into(),
            stroke: "none".into(),
            fill_opacity: "1".into(),
            stroke_opacity: "1".into(),
            opacity: "1".into(),
            stroke_width: "1".into(),
        }
    }
}

const STROKE_EXTRAS: &[&str] = &[
    "stroke-width",
    "stroke-opacity",
    "stroke-linecap",
    "stroke-linejoin",
    "stroke-dasharray",
    "stroke-dashoffset",
    "stroke-miterlimit",
];

fn walk_paint(node: &mut Node, inherited: &PaintCtx) {
    let Node::Element(el) = node else {
        return;
    };
    let specified = read_paint(el);
    let computed = apply_paint(inherited, &specified);

    if is_paint_target(el.local_name()) {
        strip_useless_paint(el, inherited, &specified, &computed);
    }

    let next = apply_paint(inherited, &read_paint(el));
    for child in &mut el.children {
        walk_paint(child, &next);
    }
}

struct SpecifiedPaint {
    fill: Option<String>,
    stroke: Option<String>,
    fill_opacity: Option<String>,
    stroke_opacity: Option<String>,
    opacity: Option<String>,
    stroke_width: Option<String>,
    style: Vec<(String, String)>,
}

fn read_paint(el: &Element) -> SpecifiedPaint {
    let style = parse_style_decls(el.attr("style").unwrap_or(""));
    let from_style = |name: &str| {
        style
            .iter()
            .rev()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.clone())
    };
    SpecifiedPaint {
        fill: from_style("fill").or_else(|| el.attr("fill").map(str::to_string)),
        stroke: from_style("stroke").or_else(|| el.attr("stroke").map(str::to_string)),
        fill_opacity: from_style("fill-opacity")
            .or_else(|| el.attr("fill-opacity").map(str::to_string)),
        stroke_opacity: from_style("stroke-opacity")
            .or_else(|| el.attr("stroke-opacity").map(str::to_string)),
        opacity: from_style("opacity").or_else(|| el.attr("opacity").map(str::to_string)),
        stroke_width: from_style("stroke-width")
            .or_else(|| el.attr("stroke-width").map(str::to_string)),
        style,
    }
}

fn apply_paint(inherited: &PaintCtx, spec: &SpecifiedPaint) -> PaintCtx {
    PaintCtx {
        fill: spec
            .fill
            .clone()
            .filter(|v| !v.eq_ignore_ascii_case("inherit"))
            .unwrap_or_else(|| inherited.fill.clone()),
        stroke: spec
            .stroke
            .clone()
            .filter(|v| !v.eq_ignore_ascii_case("inherit"))
            .unwrap_or_else(|| inherited.stroke.clone()),
        fill_opacity: spec
            .fill_opacity
            .clone()
            .filter(|v| !v.eq_ignore_ascii_case("inherit"))
            .unwrap_or_else(|| inherited.fill_opacity.clone()),
        stroke_opacity: spec
            .stroke_opacity
            .clone()
            .filter(|v| !v.eq_ignore_ascii_case("inherit"))
            .unwrap_or_else(|| inherited.stroke_opacity.clone()),
        opacity: spec
            .opacity
            .clone()
            .filter(|v| !v.eq_ignore_ascii_case("inherit"))
            .unwrap_or_else(|| inherited.opacity.clone()),
        stroke_width: spec
            .stroke_width
            .clone()
            .filter(|v| !v.eq_ignore_ascii_case("inherit"))
            .unwrap_or_else(|| inherited.stroke_width.clone()),
    }
}

fn strip_useless_paint(
    el: &mut Element,
    inherited: &PaintCtx,
    spec: &SpecifiedPaint,
    computed: &PaintCtx,
) {
    // Presentation attr overridden by a style declaration is dead.
    for name in [
        "fill",
        "stroke",
        "fill-opacity",
        "stroke-opacity",
        "opacity",
        "stroke-width",
    ] {
        if spec.style.iter().any(|(k, _)| k.eq_ignore_ascii_case(name)) && el.attr(name).is_some() {
            el.remove_attr(name);
        }
    }

    let stroke_off = paint_is_none(&computed.stroke)
        || is_zero_number(&computed.stroke_width)
        || is_zero_number(&computed.stroke_opacity);

    if stroke_off {
        // Keep an explicit `stroke="none"` (or zero width/opacity) when it
        // overrides a parent that would otherwise paint a stroke.
        let keep_stroke_none =
            spec.stroke.as_deref().is_some_and(paint_is_none) && !paint_is_none(&inherited.stroke);
        let keep_width_zero = spec.stroke_width.as_deref().is_some_and(is_zero_number)
            && !is_zero_number(&inherited.stroke_width);
        let keep_opacity_zero = spec.stroke_opacity.as_deref().is_some_and(is_zero_number)
            && !is_zero_number(&inherited.stroke_opacity);

        if !keep_stroke_none {
            remove_paint_prop(el, "stroke");
        }
        for name in STROKE_EXTRAS {
            let keep = (*name == "stroke-width" && keep_width_zero)
                || (*name == "stroke-opacity" && keep_opacity_zero);
            if !keep {
                remove_paint_prop(el, name);
            }
        }
    }

    if spec.fill.as_deref().is_some_and(is_default_fill)
        && is_default_fill(&inherited.fill)
        && !is_complex_paint(&computed.fill)
    {
        remove_paint_prop(el, "fill");
    }

    if spec.fill_opacity.as_deref().is_some_and(is_one) && is_one(&inherited.fill_opacity) {
        remove_paint_prop(el, "fill-opacity");
    }
    if spec.stroke_opacity.as_deref().is_some_and(is_one)
        && is_one(&inherited.stroke_opacity)
        && !stroke_off
    {
        remove_paint_prop(el, "stroke-opacity");
    }
    if spec.opacity.as_deref().is_some_and(is_one) && is_one(&inherited.opacity) {
        remove_paint_prop(el, "opacity");
    }
}

fn remove_paint_prop(el: &mut Element, name: &str) {
    el.remove_attr(name);
    if let Some(style) = el.attr("style").map(str::to_string) {
        let next = rewrite_style_drop(&style, name);
        if next.is_empty() {
            el.remove_attr("style");
        } else {
            el.set_attr("style", next);
        }
    }
}

fn rewrite_style_drop(style: &str, drop: &str) -> String {
    parse_style_decls(style)
        .into_iter()
        .filter(|(k, _)| !k.eq_ignore_ascii_case(drop))
        .map(|(k, v)| format!("{k}:{v}"))
        .collect::<Vec<_>>()
        .join(";")
}

fn parse_style_decls(style: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut rest = style;
    while !rest.is_empty() {
        let trimmed = rest.trim_start();
        if trimmed.is_empty() {
            break;
        }
        if trimmed.starts_with(';') {
            rest = &trimmed[1..];
            continue;
        }
        let colon = match trimmed.find(':') {
            Some(i) => i,
            None => break,
        };
        let key = trimmed[..colon].trim();
        let (value, tail) = split_decl_value(&trimmed[colon + 1..]);
        if !key.is_empty() && !value.is_empty() {
            out.push((key.to_ascii_lowercase(), value));
        }
        rest = tail.strip_prefix(';').unwrap_or(tail);
        if rest == tail {
            // hit `}` or end
            break;
        }
    }
    out
}

fn is_paint_target(name: &str) -> bool {
    matches!(
        name,
        "circle"
            | "ellipse"
            | "line"
            | "path"
            | "polygon"
            | "polyline"
            | "rect"
            | "use"
            | "text"
            | "tspan"
            | "textPath"
    )
}

fn paint_is_none(s: &str) -> bool {
    s.trim().eq_ignore_ascii_case("none")
}

fn is_default_fill(s: &str) -> bool {
    matches!(
        s.trim().to_ascii_lowercase().as_str(),
        "black" | "#000" | "#000000" | "#000000ff" | "#000f"
    )
}

fn is_complex_paint(s: &str) -> bool {
    let t = s.trim().to_ascii_lowercase();
    t.starts_with("url(") || t.starts_with("var(") || t.contains("currentcolor")
}

pub(crate) fn is_zero_number(s: &str) -> bool {
    let t = s.trim();
    if t.is_empty() {
        return false;
    }
    // `0`, `0px`, `0.0`, `0%`
    let num: String = t
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.' || *c == '+' || *c == '-')
        .collect();
    if num.is_empty() {
        return false;
    }
    num.parse::<f32>().ok().is_some_and(|n| n == 0.0)
}

fn is_one(s: &str) -> bool {
    let t = s.trim();
    if let Some(p) = t.strip_suffix('%') {
        return p
            .parse::<f32>()
            .ok()
            .is_some_and(|n| (n - 100.0).abs() < 0.05);
    }
    t.parse::<f32>()
        .ok()
        .is_some_and(|n| (n - 1.0).abs() < 0.001)
}

// ===========================================================================
// Whitespace
// ===========================================================================

fn collapse_ws_nodes(nodes: &mut Vec<Node>, keep_text: bool) {
    if !keep_text {
        nodes.retain(|n| !n.is_ignorable_whitespace());
    }
    for node in nodes.iter_mut() {
        if let Node::Element(el) = node {
            let keep = match el.attr("xml:space") {
                Some("preserve") => true,
                Some("default") => is_text_container(el.local_name()),
                _ => keep_text || is_text_container(el.local_name()),
            };
            collapse_ws_nodes(&mut el.children, keep);
        }
    }
}

fn is_text_container(name: &str) -> bool {
    matches!(
        name,
        "text" | "tspan" | "textPath" | "title" | "desc" | "style" | "script"
    )
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn el(name: &str, attrs: &[(&str, &str)], kids: Vec<Node>) -> Element {
        let mut e = Element::new(name);
        for (k, v) in attrs {
            e.attrs.push(((*k).to_string(), (*v).to_string()));
        }
        e.children = kids;
        e
    }

    fn svg(kids: Vec<Node>) -> Document {
        Document {
            nodes: vec![Node::Element(el(
                "svg",
                &[("xmlns", "http://www.w3.org/2000/svg")],
                kids,
            ))],
        }
    }

    fn root(doc: &Document) -> &Element {
        match &doc.nodes[0] {
            Node::Element(e) => e,
            _ => panic!("root is not an element"),
        }
    }

    fn find<'a>(el: &'a Element, name: &str) -> Option<&'a Element> {
        if el.local_name() == name {
            return Some(el);
        }
        for c in &el.children {
            if let Node::Element(e) = c {
                if let Some(hit) = find(e, name) {
                    return Some(hit);
                }
            }
        }
        None
    }

    fn text_of(el: &Element) -> String {
        el.children
            .iter()
            .filter_map(|n| match n {
                Node::Text(t) => Some(t.as_str()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn minify_styles_strips_comments_and_empty_decls() {
        let mut doc = svg(vec![Node::Element(el(
            "style",
            &[],
            vec![Node::Text(
                " /* hi */ rect { fill:  red ; color: ; } .x { } ".into(),
            )],
        ))]);
        minify_styles(&mut doc);
        let css = text_of(find(root(&doc), "style").unwrap());
        assert_eq!(css, "rect{fill:red}");
    }

    #[test]
    fn minify_styles_keeps_calc_spaces_and_strings() {
        let mut doc = svg(vec![Node::Element(el(
            "rect",
            &[(
                "style",
                "width: calc(1px + 2px); font-family: \"A B\" ; fill: blue;",
            )],
            vec![],
        ))]);
        minify_styles(&mut doc);
        let style = find(root(&doc), "rect").unwrap().attr("style").unwrap();
        assert!(style.contains("calc(1px + 2px)"), "{style}");
        assert!(style.contains("\"A B\""), "{style}");
        assert!(!style.contains("fill: blue"), "{style}");
    }

    #[test]
    fn minify_styles_drops_empty_style_attr() {
        let mut doc = svg(vec![Node::Element(el(
            "rect",
            &[("style", "  ;  /* x */  ")],
            vec![],
        ))]);
        minify_styles(&mut doc);
        assert!(find(root(&doc), "rect").unwrap().attr("style").is_none());
    }

    #[test]
    fn minify_styles_keeps_nonzero_px_in_css() {
        let mut doc = svg(vec![
            Node::Element(el(
                "style",
                &[],
                vec![Node::Text(
                    "#bob{transform-origin:140px 110px;font-size:12px}#z{margin:0px}".into(),
                )],
            )),
            Node::Element(el(
                "text",
                &[("style", "font-size:12px;letter-spacing:0.50px")],
                vec![],
            )),
        ]);
        minify_styles(&mut doc);
        let css = text_of(find(root(&doc), "style").unwrap());
        assert!(
            css.contains("transform-origin:140px 110px"),
            "unitless transform-origin is invalid CSS: {css}"
        );
        assert!(css.contains("font-size:12px"), "{css}");
        assert!(
            css.contains("margin:0") && !css.contains("margin:0px"),
            "0px may drop the unit: {css}"
        );
        let style = find(root(&doc), "text").unwrap().attr("style").unwrap();
        assert!(style.contains("font-size:12px"), "{style}");
        assert!(style.contains("letter-spacing:.5px"), "{style}");
    }

    #[test]
    fn convert_colors_picks_shortest() {
        assert_eq!(convert_color_value("rgb(255, 0, 0)"), "red");
        assert_eq!(convert_color_value("#FF0000"), "red");
        assert_eq!(convert_color_value("white"), "#fff");
        assert_eq!(convert_color_value("#FFFFFF"), "#fff");
        assert_eq!(convert_color_value("rgb(255 165 0)"), "orange");
        assert_eq!(convert_color_value("#D2B48C"), "tan");
        assert_eq!(convert_color_value("navy"), "navy");
        assert_eq!(convert_color_value("#000080"), "navy");
        assert_eq!(convert_color_value("rgba(0, 128, 0, 1)"), "green");
        assert_eq!(convert_color_value("hsl(120, 100%, 50%)"), "#0f0");
    }

    #[test]
    fn convert_colors_leaves_alpha_and_keywords() {
        assert_eq!(
            convert_color_value("rgba(255, 0, 0, 0.5)"),
            "rgba(255, 0, 0, 0.5)"
        );
        assert_eq!(convert_color_value("currentColor"), "currentcolor");
        assert_eq!(convert_color_value("none"), "none");
        assert_eq!(convert_color_value("url(#g)"), "url(#g)");
        assert_eq!(convert_color_value("url(#poolFill)"), "url(#poolFill)");
        assert_eq!(
            convert_color_value("url('#waterShine')"),
            "url('#waterShine')"
        );
        assert_eq!(convert_color_value("URL(#poolFill)"), "URL(#poolFill)");
        assert_eq!(convert_color_value("  url(#poolFill)  "), "url(#poolFill)");
        assert_eq!(convert_color_value("var(--BrandFill)"), "var(--BrandFill)");
        assert_eq!(convert_color_value("#f008"), "#f008");
    }

    #[test]
    fn cleanup_numeric_leading_zero_and_px_skips_smil() {
        let mut doc = svg(vec![
            Node::Element(el(
                "ellipse",
                &[("cx", "100"), ("opacity", "0.18"), ("rx", "3.50px")],
                vec![],
            )),
            Node::Element(el("svg", &[("viewBox", "0.0 0.0 24.00 24.0")], vec![])),
            Node::Element(el(
                "animate",
                &[
                    ("attributeName", "ry"),
                    ("begin", "0.50s"),
                    ("dur", "0.38s"),
                    ("values", "5.5;3.4;5.5"),
                    ("keyTimes", "0; 0.5; 1"),
                ],
                vec![],
            )),
            Node::Element(el("path", &[("d", "M 10.000 10.000")], vec![])),
        ]);
        cleanup_numeric_values(&mut doc);
        let e = find(root(&doc), "ellipse").unwrap();
        assert_eq!(e.attr("opacity"), Some(".18"));
        assert_eq!(e.attr("rx"), Some("3.5"));
        let nested = root(&doc)
            .children
            .iter()
            .filter_map(|n| match n {
                Node::Element(el) if el.local_name() == "svg" => Some(el),
                _ => None,
            })
            .next()
            .unwrap();
        assert_eq!(nested.attr("viewBox"), Some("0 0 24 24"));
        let anim = find(root(&doc), "animate").unwrap();
        assert_eq!(anim.attr("begin"), Some("0.50s"));
        assert_eq!(anim.attr("dur"), Some("0.38s"));
        assert_eq!(anim.attr("values"), Some("5.5;3.4;5.5"));
        assert_eq!(anim.attr("keyTimes"), Some("0; 0.5; 1"));
        assert_eq!(
            find(root(&doc), "path").unwrap().attr("d"),
            Some("M 10.000 10.000")
        );
    }

    #[test]
    fn cleanup_numeric_rounds_non_path_leaves_d() {
        let mut doc = svg(vec![
            Node::Element(el(
                "svg",
                &[("viewBox", "-29.947200 60.986600 2754.066400 1396.573900")],
                vec![],
            )),
            Node::Element(el("path", &[("d", "M 1.234567 8.901234")], vec![])),
        ]);
        cleanup_numeric_values(&mut doc);
        let nested = root(&doc)
            .children
            .iter()
            .filter_map(|n| match n {
                Node::Element(el) if el.local_name() == "svg" => Some(el),
                _ => None,
            })
            .next()
            .unwrap();
        assert_eq!(
            nested.attr("viewBox"),
            Some("-29.947 60.987 2754.066 1396.574")
        );
        assert_eq!(
            find(root(&doc), "path").unwrap().attr("d"),
            Some("M 1.234567 8.901234")
        );
    }

    #[test]
    fn minify_styles_keeps_ocean_land_drops_css_defaults() {
        let mut doc = svg(vec![Node::Element(el(
            "style",
            &[("type", "text/css")],
            vec![Node::Text(
                ".ocean{opacity:1;fill:white;fill-rule:nonzero;stroke:#000;stroke-width:0.5;stroke-linecap:butt}.land{fill:#b9b9b9;fill-opacity:1;stroke:#fff}.circle{opacity:0;stroke:black}".into(),
            )],
        ))]);
        minify_styles(&mut doc);
        let style_el = find(root(&doc), "style").unwrap();
        assert!(style_el.attr("type").is_none());
        let css = text_of(style_el);
        assert!(css.contains(".ocean"), "{css}");
        assert!(css.contains(".land"), "{css}");
        assert!(css.contains(".circle"), "{css}");
        assert!(
            css.contains("fill:#fff") || css.contains("fill:white") || css.contains("fill:#ffffff"),
            "{css}"
        );
        assert!(!css.contains("fill-rule"), "{css}");
        assert!(!css.contains("stroke-linecap"), "{css}");
        assert!(css.contains("opacity:0"), "{css}");
        assert!(!css.contains("opacity:1"), "{css}");
    }

    #[test]
    fn convert_ellipse_to_circle_when_radii_match() {
        let mut doc = svg(vec![
            Node::Element(el(
                "ellipse",
                &[("cx", "4"), ("cy", "4"), ("rx", "3"), ("ry", "3.0")],
                vec![],
            )),
            Node::Element(el(
                "ellipse",
                &[("cx", "8"), ("cy", "8"), ("rx", "3"), ("ry", "2")],
                vec![],
            )),
        ]);
        convert_ellipse_to_circle(&mut doc);
        let circles: Vec<_> = root(&doc)
            .children
            .iter()
            .filter_map(|n| match n {
                Node::Element(el) => Some(el),
                _ => None,
            })
            .collect();
        assert_eq!(circles[0].local_name(), "circle");
        assert_eq!(circles[0].attr("r"), Some("3"));
        assert!(circles[0].attr("rx").is_none());
        assert_eq!(circles[1].local_name(), "ellipse");
    }

    #[test]
    fn convert_colors_preserves_url_id_case() {
        let mut doc = svg(vec![
            Node::Element(el("linearGradient", &[("id", "poolFill")], vec![])),
            Node::Element(el(
                "ellipse",
                &[("fill", "url(#poolFill)"), ("stroke", "url(#waterShine)")],
                vec![],
            )),
            Node::Element(el(
                "rect",
                &[("style", "fill:url(#poolFill);stroke:url(#ClipMe)")],
                vec![],
            )),
        ]);
        convert_colors(&mut doc);
        let ellipse = find(root(&doc), "ellipse").unwrap();
        assert_eq!(ellipse.attr("fill"), Some("url(#poolFill)"));
        assert_eq!(ellipse.attr("stroke"), Some("url(#waterShine)"));
        let style = find(root(&doc), "rect").unwrap().attr("style").unwrap();
        assert!(style.contains("url(#poolFill)"), "{style}");
        assert!(style.contains("url(#ClipMe)"), "{style}");
        assert!(!style.contains("url(#poolfill)"), "{style}");
        assert!(!style.contains("url(#clipme)"), "{style}");
    }

    #[test]
    fn convert_colors_rewrites_attrs_and_style() {
        let mut doc = svg(vec![Node::Element(el(
            "rect",
            &[
                ("fill", "rgb(255, 255, 255)"),
                ("stroke", "#FF0000"),
                ("style", "stop-color: rgb(0, 0, 0); flood-color: yellow"),
            ],
            vec![],
        ))]);
        convert_colors(&mut doc);
        let r = find(root(&doc), "rect").unwrap();
        assert_eq!(r.attr("fill"), Some("#fff"));
        assert_eq!(r.attr("stroke"), Some("red"));
        let style = r.attr("style").unwrap();
        assert!(
            style.contains("stop-color:#000") || style.contains("stop-color:black"),
            "{style}"
        );
        assert!(style.contains("flood-color:#ff0"), "{style}");
    }

    #[test]
    fn convert_colors_in_stylesheet() {
        let mut doc = svg(vec![Node::Element(el(
            "style",
            &[],
            vec![Node::Text(".a{fill:rgb(0,0,255)}".into())],
        ))]);
        convert_colors(&mut doc);
        let css = text_of(find(root(&doc), "style").unwrap());
        assert!(
            css.contains("fill:#00f") || css.contains("fill:blue"),
            "{css}"
        );
    }

    #[test]
    fn cleanup_ids_drops_unused_keeps_url_and_href() {
        let mut doc = svg(vec![
            Node::Element(el(
                "defs",
                &[],
                vec![
                    Node::Element(el("linearGradient", &[("id", "g1")], vec![])),
                    Node::Element(el("linearGradient", &[("id", "unused")], vec![])),
                    Node::Element(el("path", &[("id", "p1")], vec![])),
                ],
            )),
            Node::Element(el("rect", &[("fill", "url(#g1)")], vec![])),
            Node::Element(el("use", &[("href", "#p1")], vec![])),
        ]);
        cleanup_ids(&mut doc);
        let s = format!("{:?}", doc);
        assert!(s.contains("g1"), "{s}");
        assert!(s.contains("p1"), "{s}");
        assert!(!s.contains("unused"), "{s}");
    }

    #[test]
    fn cleanup_ids_keeps_css_selector_and_aria() {
        let mut doc = svg(vec![
            Node::Element(el("style", &[], vec![Node::Text("#face{fill:red}".into())])),
            Node::Element(el("rect", &[("id", "face")], vec![])),
            Node::Element(el(
                "g",
                &[("id", "lbl"), ("aria-labelledby", "title1")],
                vec![],
            )),
            Node::Element(el("title", &[("id", "title1")], vec![])),
            Node::Element(el("rect", &[("id", "gone")], vec![])),
        ]);
        cleanup_ids(&mut doc);
        assert!(
            find(root(&doc), "rect").unwrap().attr("id") == Some("face")
                || root(&doc)
                    .children
                    .iter()
                    .filter_map(|n| match n {
                        Node::Element(e) if e.local_name() == "rect" => Some(e),
                        _ => None,
                    })
                    .any(|e| e.attr("id") == Some("face"))
        );
        let title = find(root(&doc), "title").unwrap();
        assert_eq!(title.attr("id"), Some("title1"));
        let gone = root(&doc)
            .children
            .iter()
            .filter_map(|n| match n {
                Node::Element(e) if e.attr("id") == Some("gone") => Some(e),
                _ => None,
            })
            .count();
        assert_eq!(gone, 0);
    }

    #[test]
    fn cleanup_ids_keeps_smil_and_skips_scripts() {
        let mut animated = svg(vec![
            Node::Element(el("circle", &[("id", "c1")], vec![])),
            Node::Element(el(
                "animate",
                &[("begin", "c1.click"), ("id", "animUnused")],
                vec![],
            )),
        ]);
        cleanup_ids(&mut animated);
        assert_eq!(
            find(root(&animated), "circle").unwrap().attr("id"),
            Some("c1")
        );
        assert!(find(root(&animated), "animate")
            .unwrap()
            .attr("id")
            .is_none());

        let mut offset = svg(vec![
            Node::Element(el("circle", &[("id", "b")], vec![])),
            Node::Element(el(
                "animate",
                &[("begin", "b.end-0.5s"), ("id", "a")],
                vec![],
            )),
        ]);
        cleanup_ids(&mut offset);
        assert_eq!(find(root(&offset), "circle").unwrap().attr("id"), Some("b"));

        let mut scripted = svg(vec![
            Node::Element(el("script", &[], vec![Node::Text("/* js */".into())])),
            Node::Element(el("rect", &[("id", "jsHook")], vec![])),
        ]);
        cleanup_ids(&mut scripted);
        assert_eq!(
            find(root(&scripted), "rect").unwrap().attr("id"),
            Some("jsHook")
        );

        let mut onclick = svg(vec![Node::Element(el(
            "rect",
            &[("id", "hook"), ("onclick", "ok()")],
            vec![],
        ))]);
        cleanup_ids(&mut onclick);
        assert_eq!(
            find(root(&onclick), "rect").unwrap().attr("id"),
            Some("hook")
        );

        // `opacity` must not be treated as an event handler.
        let mut faded = svg(vec![Node::Element(el(
            "rect",
            &[("id", "dead"), ("opacity", "0.5")],
            vec![],
        ))]);
        cleanup_ids(&mut faded);
        assert!(find(root(&faded), "rect").unwrap().attr("id").is_none());
    }

    #[test]
    fn stroke_fill_strips_none_stroke_extras_on_shapes() {
        let mut doc = svg(vec![Node::Element(el(
            "path",
            &[
                ("d", "M0 0"),
                ("stroke", "none"),
                ("stroke-width", "4"),
                ("stroke-linecap", "round"),
                ("fill-opacity", "1"),
                ("opacity", "1"),
            ],
            vec![],
        ))]);
        remove_useless_stroke_and_fill(&mut doc);
        let p = find(root(&doc), "path").unwrap();
        assert!(p.attr("stroke").is_none(), "{:?}", p.attrs);
        assert!(p.attr("stroke-width").is_none());
        assert!(p.attr("stroke-linecap").is_none());
        assert!(p.attr("fill-opacity").is_none());
        assert!(p.attr("opacity").is_none());
    }

    #[test]
    fn stroke_fill_keeps_override_when_parent_paints() {
        let mut doc = svg(vec![Node::Element(el(
            "g",
            &[("stroke", "red"), ("fill", "blue")],
            vec![Node::Element(el(
                "rect",
                &[
                    ("x", "0"),
                    ("stroke", "none"),
                    ("fill", "black"),
                    ("stroke-width", "2"),
                ],
                vec![],
            ))],
        ))]);
        remove_useless_stroke_and_fill(&mut doc);
        let r = find(root(&doc), "rect").unwrap();
        assert_eq!(r.attr("stroke"), Some("none"));
        assert_eq!(r.attr("fill"), Some("black"));
        // parent has no stroke-width; child's 2 is unused while stroke is none
        assert!(r.attr("stroke-width").is_none());
        // group attrs must survive (inheritance)
        let g = find(root(&doc), "g").unwrap();
        assert_eq!(g.attr("stroke"), Some("red"));
        assert_eq!(g.attr("fill"), Some("blue"));
    }

    #[test]
    fn stroke_fill_zero_width_and_style_override() {
        let mut doc = svg(vec![Node::Element(el(
            "circle",
            &[
                ("r", "3"),
                ("stroke", "#f00"),
                ("stroke-width", "0"),
                ("style", "fill: #000000; fill-opacity: 1"),
            ],
            vec![],
        ))]);
        remove_useless_stroke_and_fill(&mut doc);
        let c = find(root(&doc), "circle").unwrap();
        assert!(c.attr("stroke").is_none());
        assert_eq!(c.attr("stroke-width"), Some("0"));
        assert!(
            c.attr("style").is_none() || !c.attr("style").unwrap().contains("fill"),
            "{:?}",
            c.attr("style")
        );
    }

    #[test]
    fn collapse_whitespace_drops_layout_keeps_text_and_preserve() {
        let mut doc = svg(vec![
            Node::Text("\n  ".into()),
            Node::Element(el("text", &[], vec![Node::Text("  hi  ".into())])),
            Node::Element(el(
                "g",
                &[("xml:space", "preserve")],
                vec![
                    Node::Text("\n  ".into()),
                    Node::Element(el("rect", &[("x", "0")], vec![])),
                    Node::Text("\n".into()),
                ],
            )),
        ]);
        collapse_whitespace(&mut doc);
        let r = root(&doc);
        assert!(
            !r.children
                .iter()
                .any(|n| matches!(n, Node::Text(t) if t.chars().all(char::is_whitespace))),
            "root ignorable ws should be gone"
        );
        let text = find(r, "text").unwrap();
        assert_eq!(text_of(text), "  hi  ");
        let g = find(r, "g").unwrap();
        let ws = g
            .children
            .iter()
            .filter(|n| n.is_ignorable_whitespace())
            .count();
        assert!(ws >= 1, "xml:space=preserve must keep whitespace");
    }

    #[test]
    fn run_minify_pipeline_order_is_safe() {
        let mut doc = svg(vec![
            Node::Text("\n".into()),
            Node::Element(el(
                "style",
                &[],
                vec![Node::Text(" .a { fill: rgb(255, 0, 0); } ".into())],
            )),
            Node::Element(el(
                "rect",
                &[
                    ("class", "a"),
                    ("id", "unused"),
                    ("fill", "rgb(255,255,255)"),
                    ("stroke", "none"),
                    ("stroke-width", "3"),
                    ("style", "  fill-opacity: 1 ; "),
                ],
                vec![],
            )),
        ]);
        run_minify(&mut doc);
        let r = find(root(&doc), "rect").unwrap();
        assert_eq!(r.attr("fill"), Some("#fff"));
        assert!(r.attr("id").is_none());
        assert!(r.attr("stroke").is_none());
        assert!(r.attr("stroke-width").is_none());
        let css = text_of(find(root(&doc), "style").unwrap());
        assert!(
            css.contains("fill:red") || css.contains("fill:#f00"),
            "{css}"
        );
    }
}
