//! Opt-in destructive SVGO-named plugins. **Default OFF.**
//!
//! Public contracts only — not a port of SVGO / oxvg / vexy source:
//!
//! - [`run_remove_raster_images`] — [removeRasterImages](https://svgo.dev/docs/plugins/removeRasterImages/)
//! - [`run_remove_off_canvas_paths`] — [removeOffCanvasPaths](https://svgo.dev/docs/plugins/removeOffCanvasPaths/)
//! - [`run_remove_scripts`] — [removeScripts](https://svgo.dev/docs/plugins/removeScripts/)
//! - [`run_remove_style_element`] — [removeStyleElement](https://svgo.dev/docs/plugins/removeStyleElement/)
//!
//! Never wired from `run_default` / `DEFAULT_PLUGIN_NAMES`. Task 10 activates
//! them via `--plugin`. Callers invoke the named `run_*` functions.

use crate::animation::detect_document;
use crate::ast::{Document, Element, Node};

/// SVGO opt-in IDs implemented in this module. Not in the default preset.
pub const OPTIN_DANGER_PLUGIN_NAMES: &[&str] = &[
    "removeRasterImages",
    "removeOffCanvasPaths",
    "removeScripts",
    "removeStyleElement",
];

/// Dispatch by SVGO plugin id (case-insensitive). Unknown names are a no-op.
///
/// Animation-aware gates stay on (`removeOffCanvasPaths` / `removeStyleElement`).
pub fn run_optin_danger(name: &str, root: &mut Node) -> bool {
    if name.eq_ignore_ascii_case("removeRasterImages") {
        run_remove_raster_images_node(root);
        true
    } else if name.eq_ignore_ascii_case("removeOffCanvasPaths") {
        run_remove_off_canvas_paths_node(root);
        true
    } else if name.eq_ignore_ascii_case("removeScripts") {
        run_remove_scripts_node(root);
        true
    } else if name.eq_ignore_ascii_case("removeStyleElement") {
        run_remove_style_element_node(root);
        true
    } else {
        false
    }
}

fn motion_sensitive_node(root: &Node) -> bool {
    detect_document(
        &Document {
            nodes: vec![root.clone()],
        },
        &[],
    )
    .motion_sensitive
}

/// How far past the viewBox a control-polygon must sit before we drop a path.
const CLEARLY_OUTSIDE_PAD: f64 = 0.5;

const RASTER_EXTS: &[&str] = &[
    ".png", ".jpg", ".jpeg", ".jfif", ".pjpeg", ".gif", ".webp", ".bmp", ".ico", ".tif", ".tiff",
    ".avif", ".apng",
];

// ---------------------------------------------------------------------------
// removeRasterImages
// ---------------------------------------------------------------------------

/// `removeRasterImages` — drop `<image>` whose `href` / `xlink:href` is a
/// raster file or `data:image/…` payload (not `image/svg+xml`).
///
/// Visual data loss. The default pipeline recompresses instead
/// (`recompressEmbeddedRasters`).
pub fn run_remove_raster_images(doc: &mut Document) {
    drop_raster_images(&mut doc.nodes);
}

/// Same pass on a single subtree (SVGO-shaped `--plugin` / Task 10).
pub fn run_remove_raster_images_node(root: &mut Node) {
    if let Node::Element(el) = root {
        drop_raster_images(&mut el.children);
    }
}

fn drop_raster_images(nodes: &mut Vec<Node>) {
    nodes.retain(|n| match n {
        Node::Element(el) => !element_is_raster_image(el),
        _ => true,
    });
    for node in nodes.iter_mut() {
        if let Node::Element(el) = node {
            drop_raster_images(&mut el.children);
        }
    }
}

fn element_is_raster_image(el: &Element) -> bool {
    if !el.local_name().eq_ignore_ascii_case("image") {
        return false;
    }
    href_values(el).any(is_raster_href)
}

fn href_values(el: &Element) -> impl Iterator<Item = &str> {
    el.attrs.iter().filter_map(|(k, v)| {
        let local = k.rsplit_once(':').map(|(_, l)| l).unwrap_or(k.as_str());
        if local.eq_ignore_ascii_case("href") {
            Some(v.as_str())
        } else {
            None
        }
    })
}

fn is_raster_href(raw: &str) -> bool {
    let t = raw.trim();
    if t.is_empty() {
        return false;
    }
    let lower = t.to_ascii_lowercase();
    if let Some(rest) = lower.strip_prefix("data:") {
        let mime = rest
            .split(|c| c == ';' || c == ',')
            .next()
            .unwrap_or("")
            .trim();
        if mime.starts_with("image/svg") {
            return false;
        }
        return mime.starts_with("image/");
    }
    let path = lower.split(['?', '#']).next().unwrap_or(lower.as_str());
    RASTER_EXTS.iter().any(|ext| path.ends_with(ext))
}

// ---------------------------------------------------------------------------
// removeOffCanvasPaths
// ---------------------------------------------------------------------------

/// `removeOffCanvasPaths` — drop `<path>` whose control-polygon bbox sits
/// clearly outside the nearest `<svg viewBox>`.
///
/// Conservative vs stock SVGO: skip the whole pass on motion-sensitive docs
/// (SMIL / CSS / script / events). Always skip a path that has a `transform`
/// (self or ancestor), SMIL children, markers, filter/mask, an arc, or an
/// unparseable `d`. Stroke is inflated when present.
pub fn run_remove_off_canvas_paths(doc: &mut Document) {
    run_remove_off_canvas_paths_with(doc, true);
}

/// Same pass on a single subtree. Skips motion-sensitive roots.
pub fn run_remove_off_canvas_paths_node(root: &mut Node) {
    run_remove_off_canvas_paths_node_with(root, true);
}

/// Node entry with an animation-aware gate.
pub fn run_remove_off_canvas_paths_node_with(root: &mut Node, skip_if_motion: bool) {
    if skip_if_motion && motion_sensitive_node(root) {
        return;
    }
    let ctx = OffCanvasCtx {
        view_box: None,
        transformed: false,
        skip_geom: false,
        stroke: false,
        stroke_width: 1.0,
    };
    if let Node::Element(el) = root {
        let next = child_off_canvas_ctx(el, ctx);
        drop_off_canvas_paths(&mut el.children, next);
    }
}

/// Same as [`run_remove_off_canvas_paths`], with an animation-aware gate.
///
/// `skip_if_motion` matches `Config.animation_aware`: when true, a
/// motion-sensitive tree is left untouched.
pub fn run_remove_off_canvas_paths_with(doc: &mut Document, skip_if_motion: bool) {
    if skip_if_motion && detect_document(doc, &[]).motion_sensitive {
        return;
    }
    let ctx = OffCanvasCtx {
        view_box: None,
        transformed: false,
        skip_geom: false,
        stroke: false,
        stroke_width: 1.0,
    };
    drop_off_canvas_paths(&mut doc.nodes, ctx);
}

#[derive(Clone, Copy)]
struct OffCanvasCtx {
    view_box: Option<(f64, f64, f64, f64)>,
    transformed: bool,
    skip_geom: bool,
    stroke: bool,
    stroke_width: f64,
}

fn drop_off_canvas_paths(nodes: &mut Vec<Node>, ctx: OffCanvasCtx) {
    nodes.retain(|n| match n {
        Node::Element(el) => !should_drop_off_canvas_path(el, ctx),
        _ => true,
    });
    for node in nodes.iter_mut() {
        let Node::Element(el) = node else {
            continue;
        };
        let next = child_off_canvas_ctx(el, ctx);
        drop_off_canvas_paths(&mut el.children, next);
    }
}

fn child_off_canvas_ctx(el: &Element, parent: OffCanvasCtx) -> OffCanvasCtx {
    let mut ctx = parent;
    if is_skip_geom_host(el.local_name()) {
        ctx.skip_geom = true;
    }
    if el.local_name().eq_ignore_ascii_case("svg") {
        if let Some(vb) = element_view_box(el) {
            ctx.view_box = Some(vb);
        }
    }
    if has_transform(el) {
        ctx.transformed = true;
    }
    if let Some(stroke) = presentation(el, "stroke") {
        ctx.stroke = !stroke.eq_ignore_ascii_case("none");
    }
    if let Some(sw) = presentation(el, "stroke-width") {
        if let Some(n) = parse_user_unit(sw) {
            ctx.stroke_width = n;
        }
    }
    ctx
}

fn should_drop_off_canvas_path(el: &Element, ctx: OffCanvasCtx) -> bool {
    if !el.local_name().eq_ignore_ascii_case("path") {
        return false;
    }
    if ctx.skip_geom || ctx.transformed || has_transform(el) {
        return false;
    }
    if element_has_smil(el) || has_paint_effects(el) {
        return false;
    }
    let Some(vb) = ctx.view_box else {
        return false;
    };
    let Some(d) = el.attr("d") else {
        return false;
    };
    let Some(bbox) = path_control_bbox(d) else {
        return false;
    };
    let inflate = path_stroke_inflate(el, ctx);
    let Some(inflate) = inflate else {
        return false;
    };
    clearly_outside(bbox, vb, inflate + CLEARLY_OUTSIDE_PAD)
}

fn path_stroke_inflate(el: &Element, ctx: OffCanvasCtx) -> Option<f64> {
    let stroke = presentation(el, "stroke").map(str::to_ascii_lowercase);
    let stroked = match stroke.as_deref() {
        Some("none") => false,
        Some(_) => true,
        None => ctx.stroke,
    };
    if !stroked {
        return Some(0.0);
    }
    match presentation(el, "stroke-width") {
        Some(sw) => parse_user_unit(sw).map(|n| n.max(0.0) * 0.5),
        None => Some(ctx.stroke_width.max(0.0) * 0.5),
    }
}

fn is_skip_geom_host(local: &str) -> bool {
    matches!(
        local.to_ascii_lowercase().as_str(),
        "defs" | "symbol" | "marker" | "clippath" | "mask" | "pattern" | "foreignobject" | "switch"
    )
}

fn has_transform(el: &Element) -> bool {
    if el.attr("transform").is_some_and(|v| !v.trim().is_empty()) {
        return true;
    }
    if let Some(style) = el.attr("style") {
        if css_decl(style, "transform").is_some_and(|v| !v.eq_ignore_ascii_case("none")) {
            return true;
        }
    }
    false
}

fn has_paint_effects(el: &Element) -> bool {
    for key in [
        "filter",
        "mask",
        "clip-path",
        "marker",
        "marker-start",
        "marker-mid",
        "marker-end",
    ] {
        if el.attr(key).is_some_and(|v| !v.trim().is_empty()) {
            return true;
        }
    }
    if let Some(style) = el.attr("style") {
        for prop in [
            "filter",
            "mask",
            "clip-path",
            "marker",
            "marker-start",
            "marker-mid",
            "marker-end",
        ] {
            if css_decl(style, prop).is_some_and(|v| !v.eq_ignore_ascii_case("none")) {
                return true;
            }
        }
    }
    false
}

fn element_has_smil(el: &Element) -> bool {
    if is_smil_name(el.local_name()) {
        return true;
    }
    el.children.iter().any(|n| match n {
        Node::Element(c) => element_has_smil(c),
        _ => false,
    })
}

fn is_smil_name(local: &str) -> bool {
    matches!(
        local.to_ascii_lowercase().as_str(),
        "animate" | "animatetransform" | "animatemotion" | "animatecolor" | "set"
    )
}

fn element_view_box(el: &Element) -> Option<(f64, f64, f64, f64)> {
    let raw = el.attr("viewBox").or_else(|| el.attr("viewbox"))?;
    parse_view_box(raw)
}

fn include_pt(
    x: f64,
    y: f64,
    minx: &mut f64,
    miny: &mut f64,
    maxx: &mut f64,
    maxy: &mut f64,
    any: &mut bool,
) {
    if !x.is_finite() || !y.is_finite() {
        return;
    }
    *any = true;
    *minx = (*minx).min(x);
    *miny = (*miny).min(y);
    *maxx = (*maxx).max(x);
    *maxy = (*maxy).max(y);
}

fn parse_view_box(raw: &str) -> Option<(f64, f64, f64, f64)> {
    let mut nums = Vec::with_capacity(4);
    for part in raw.split(|c: char| c.is_ascii_whitespace() || c == ',') {
        if part.is_empty() {
            continue;
        }
        let n: f64 = part.parse().ok()?;
        if !n.is_finite() {
            return None;
        }
        nums.push(n);
    }
    if nums.len() != 4 {
        return None;
    }
    let w = nums[2];
    let h = nums[3];
    if w <= 0.0 || h <= 0.0 {
        return None;
    }
    Some((nums[0], nums[1], w, h))
}

fn clearly_outside(bbox: (f64, f64, f64, f64), vb: (f64, f64, f64, f64), pad: f64) -> bool {
    let (minx, miny, maxx, maxy) = bbox;
    let (vx, vy, vw, vh) = vb;
    let left = vx - pad;
    let top = vy - pad;
    let right = vx + vw + pad;
    let bottom = vy + vh + pad;
    maxx < left || maxy < top || minx > right || miny > bottom
}

/// Control-polygon bbox of `d`. `None` if the path has an arc or will not parse.
///
/// Cubic / quadratic curves lie in their control polygon, so a polygon that
/// sits entirely outside the viewBox is a safe drop. Arcs are refused.
fn path_control_bbox(d: &str) -> Option<(f64, f64, f64, f64)> {
    let mut lex = PathLex::new(d);
    let mut cx = 0.0;
    let mut cy = 0.0;
    let mut sx = 0.0;
    let mut sy = 0.0;
    let mut last_c2: Option<(f64, f64)> = None;
    let mut last_q: Option<(f64, f64)> = None;
    let mut last_cubic = false;
    let mut last_quad = false;
    let mut minx = f64::INFINITY;
    let mut miny = f64::INFINITY;
    let mut maxx = f64::NEG_INFINITY;
    let mut maxy = f64::NEG_INFINITY;
    let mut any = false;

    let mut cmd: Option<u8> = None;
    loop {
        lex.skip_sep();
        if lex.done() {
            break;
        }
        if lex.peek().is_some_and(|c| c.is_ascii_alphabetic()) {
            cmd = Some(lex.bump());
        }
        let c = cmd?;
        match c {
            b'M' | b'm' => {
                let (mut x, mut y) = lex.xy()?;
                if c == b'm' {
                    x += cx;
                    y += cy;
                }
                cx = x;
                cy = y;
                sx = cx;
                sy = cy;
                include_pt(cx, cy, &mut minx, &mut miny, &mut maxx, &mut maxy, &mut any);
                last_c2 = None;
                last_q = None;
                last_cubic = false;
                last_quad = false;
                cmd = Some(if c == b'm' { b'l' } else { b'L' });
            }
            b'L' | b'l' => {
                let (mut x, mut y) = lex.xy()?;
                if c == b'l' {
                    x += cx;
                    y += cy;
                }
                cx = x;
                cy = y;
                include_pt(cx, cy, &mut minx, &mut miny, &mut maxx, &mut maxy, &mut any);
                last_cubic = false;
                last_quad = false;
            }
            b'H' | b'h' => {
                let mut x = lex.number()?;
                if c == b'h' {
                    x += cx;
                }
                cx = x;
                include_pt(cx, cy, &mut minx, &mut miny, &mut maxx, &mut maxy, &mut any);
                last_cubic = false;
                last_quad = false;
            }
            b'V' | b'v' => {
                let mut y = lex.number()?;
                if c == b'v' {
                    y += cy;
                }
                cy = y;
                include_pt(cx, cy, &mut minx, &mut miny, &mut maxx, &mut maxy, &mut any);
                last_cubic = false;
                last_quad = false;
            }
            b'C' | b'c' => {
                let (mut x1, mut y1) = lex.xy()?;
                let (mut x2, mut y2) = lex.xy()?;
                let (mut x, mut y) = lex.xy()?;
                if c == b'c' {
                    x1 += cx;
                    y1 += cy;
                    x2 += cx;
                    y2 += cy;
                    x += cx;
                    y += cy;
                }
                include_pt(x1, y1, &mut minx, &mut miny, &mut maxx, &mut maxy, &mut any);
                include_pt(x2, y2, &mut minx, &mut miny, &mut maxx, &mut maxy, &mut any);
                include_pt(x, y, &mut minx, &mut miny, &mut maxx, &mut maxy, &mut any);
                last_c2 = Some((x2, y2));
                last_q = None;
                last_cubic = true;
                last_quad = false;
                cx = x;
                cy = y;
            }
            b'S' | b's' => {
                let (mut x2, mut y2) = lex.xy()?;
                let (mut x, mut y) = lex.xy()?;
                if c == b's' {
                    x2 += cx;
                    y2 += cy;
                    x += cx;
                    y += cy;
                }
                let (rx, ry) = if last_cubic {
                    last_c2
                        .map(|(lx, ly)| (2.0 * cx - lx, 2.0 * cy - ly))
                        .unwrap_or((cx, cy))
                } else {
                    (cx, cy)
                };
                include_pt(rx, ry, &mut minx, &mut miny, &mut maxx, &mut maxy, &mut any);
                include_pt(x2, y2, &mut minx, &mut miny, &mut maxx, &mut maxy, &mut any);
                include_pt(x, y, &mut minx, &mut miny, &mut maxx, &mut maxy, &mut any);
                last_c2 = Some((x2, y2));
                last_q = None;
                last_cubic = true;
                last_quad = false;
                cx = x;
                cy = y;
            }
            b'Q' | b'q' => {
                let (mut x1, mut y1) = lex.xy()?;
                let (mut x, mut y) = lex.xy()?;
                if c == b'q' {
                    x1 += cx;
                    y1 += cy;
                    x += cx;
                    y += cy;
                }
                include_pt(x1, y1, &mut minx, &mut miny, &mut maxx, &mut maxy, &mut any);
                include_pt(x, y, &mut minx, &mut miny, &mut maxx, &mut maxy, &mut any);
                last_q = Some((x1, y1));
                last_c2 = None;
                last_cubic = false;
                last_quad = true;
                cx = x;
                cy = y;
            }
            b'T' | b't' => {
                let (mut x, mut y) = lex.xy()?;
                if c == b't' {
                    x += cx;
                    y += cy;
                }
                let (qx, qy) = if last_quad {
                    last_q
                        .map(|(lx, ly)| (2.0 * cx - lx, 2.0 * cy - ly))
                        .unwrap_or((cx, cy))
                } else {
                    (cx, cy)
                };
                include_pt(qx, qy, &mut minx, &mut miny, &mut maxx, &mut maxy, &mut any);
                include_pt(x, y, &mut minx, &mut miny, &mut maxx, &mut maxy, &mut any);
                last_q = Some((qx, qy));
                last_c2 = None;
                last_cubic = false;
                last_quad = true;
                cx = x;
                cy = y;
            }
            b'A' | b'a' => return None,
            b'Z' | b'z' => {
                cx = sx;
                cy = sy;
                last_cubic = false;
                last_quad = false;
                cmd = None;
            }
            _ => return None,
        }
    }
    if any {
        Some((minx, miny, maxx, maxy))
    } else {
        None
    }
}

struct PathLex<'a> {
    b: &'a [u8],
    i: usize,
}

impl<'a> PathLex<'a> {
    fn new(d: &'a str) -> Self {
        Self {
            b: d.as_bytes(),
            i: 0,
        }
    }

    fn done(&self) -> bool {
        self.i >= self.b.len()
    }

    fn peek(&self) -> Option<u8> {
        self.b.get(self.i).copied()
    }

    fn bump(&mut self) -> u8 {
        let c = self.b[self.i];
        self.i += 1;
        c
    }

    fn skip_sep(&mut self) {
        while self.i < self.b.len() {
            let c = self.b[self.i];
            if c.is_ascii_whitespace() || c == b',' {
                self.i += 1;
            } else {
                break;
            }
        }
    }

    fn xy(&mut self) -> Option<(f64, f64)> {
        let x = self.number()?;
        let y = self.number()?;
        Some((x, y))
    }

    fn number(&mut self) -> Option<f64> {
        self.skip_sep();
        let start = self.i;
        match self.peek() {
            Some(b'+') | Some(b'-') => self.i += 1,
            _ => {}
        }
        let mut saw_digit = false;
        let mut saw_dot = false;
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() {
                saw_digit = true;
                self.i += 1;
            } else if c == b'.' && !saw_dot {
                saw_dot = true;
                self.i += 1;
            } else {
                break;
            }
        }
        if matches!(self.peek(), Some(b'e') | Some(b'E')) {
            let save = self.i;
            self.i += 1;
            if matches!(self.peek(), Some(b'+') | Some(b'-')) {
                self.i += 1;
            }
            let exp0 = self.i;
            while self.peek().is_some_and(|c| c.is_ascii_digit()) {
                self.i += 1;
            }
            if self.i == exp0 {
                self.i = save;
            }
        }
        if !saw_digit {
            self.i = start;
            return None;
        }
        std::str::from_utf8(&self.b[start..self.i])
            .ok()?
            .parse()
            .ok()
    }
}

// ---------------------------------------------------------------------------
// removeScripts
// ---------------------------------------------------------------------------

/// `removeScripts` — drop `<script>` and `on*` event attributes.
///
/// Sanitizer, not a minifier. The element that carried `onclick` stays.
pub fn run_remove_scripts(doc: &mut Document) {
    strip_scripts(&mut doc.nodes);
}

/// Same pass on a single subtree.
pub fn run_remove_scripts_node(root: &mut Node) {
    if let Node::Element(el) = root {
        el.retain_attrs(|k, _| !is_event_attr(k));
        strip_scripts(&mut el.children);
    }
}

fn strip_scripts(nodes: &mut Vec<Node>) {
    nodes.retain(|n| match n {
        Node::Element(el) => !el.local_name().eq_ignore_ascii_case("script"),
        _ => true,
    });
    for node in nodes.iter_mut() {
        let Node::Element(el) = node else {
            continue;
        };
        el.retain_attrs(|k, _| !is_event_attr(k));
        strip_scripts(&mut el.children);
    }
}

fn is_event_attr(name: &str) -> bool {
    let local = name.rsplit_once(':').map(|(_, l)| l).unwrap_or(name);
    local.len() > 2 && local.as_bytes()[..2].eq_ignore_ascii_case(b"on")
}

// ---------------------------------------------------------------------------
// removeStyleElement
// ---------------------------------------------------------------------------

/// `removeStyleElement` — drop `<style>` elements.
///
/// Skips the pass when the document is motion-sensitive (`@keyframes`, SMIL,
/// script / `on*`). Inline `style=""` attributes stay.
pub fn run_remove_style_element(doc: &mut Document) {
    run_remove_style_element_with(doc, true);
}

/// Same pass on a single subtree. Skips motion-sensitive roots.
pub fn run_remove_style_element_node(root: &mut Node) {
    run_remove_style_element_node_with(root, true);
}

/// Node entry with an animation-aware gate.
pub fn run_remove_style_element_node_with(root: &mut Node, skip_if_motion: bool) {
    if skip_if_motion && motion_sensitive_node(root) {
        return;
    }
    if let Node::Element(el) = root {
        drop_style_elements(&mut el.children);
    }
}

/// Same as [`run_remove_style_element`], with an animation-aware gate.
pub fn run_remove_style_element_with(doc: &mut Document, skip_if_motion: bool) {
    if skip_if_motion && detect_document(doc, &[]).motion_sensitive {
        return;
    }
    drop_style_elements(&mut doc.nodes);
}

fn drop_style_elements(nodes: &mut Vec<Node>) {
    nodes.retain(|n| match n {
        Node::Element(el) => !el.local_name().eq_ignore_ascii_case("style"),
        _ => true,
    });
    for node in nodes.iter_mut() {
        if let Node::Element(el) = node {
            drop_style_elements(&mut el.children);
        }
    }
}

// ---------------------------------------------------------------------------
// shared
// ---------------------------------------------------------------------------

fn presentation<'a>(el: &'a Element, name: &str) -> Option<&'a str> {
    if let Some(style) = el.attr("style") {
        if let Some(v) = css_decl(style, name) {
            return Some(v);
        }
    }
    el.attrs.iter().find_map(|(k, v)| {
        if k.eq_ignore_ascii_case(name) {
            Some(v.as_str())
        } else {
            None
        }
    })
}

fn css_decl<'a>(style: &'a str, name: &str) -> Option<&'a str> {
    let mut hit = None;
    for part in style.split(';') {
        let (k, v) = match part.split_once(':') {
            Some(kv) => kv,
            None => continue,
        };
        if k.trim().eq_ignore_ascii_case(name) {
            let t = v.trim();
            if !t.is_empty() {
                hit = Some(t);
            }
        }
    }
    hit
}

fn parse_user_unit(s: &str) -> Option<f64> {
    let t = s.trim();
    if t.ends_with('%') {
        return None;
    }
    let t = t
        .strip_suffix("px")
        .or_else(|| t.strip_suffix("PX"))
        .or_else(|| t.strip_suffix("Px"))
        .unwrap_or(t)
        .trim();
    let n: f64 = t.parse().ok()?;
    n.is_finite().then_some(n)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::parse_svg;
    use crate::serialize::serialize;
    use crate::{optimize_str, DEFAULT_PLUGIN_NAMES};

    fn parse(s: &str) -> Document {
        parse_svg(s).unwrap()
    }

    fn emit(doc: &Document) -> String {
        serialize(doc)
    }

    fn has_name(doc: &Document, name: &str) -> bool {
        fn walk(nodes: &[Node], name: &str) -> bool {
            nodes.iter().any(|n| match n {
                Node::Element(el) => {
                    el.local_name().eq_ignore_ascii_case(name) || walk(&el.children, name)
                }
                _ => false,
            })
        }
        walk(&doc.nodes, name)
    }

    fn find_attr<'a>(doc: &'a Document, tag: &str, attr: &str) -> Option<&'a str> {
        fn walk<'a>(nodes: &'a [Node], tag: &str, attr: &str) -> Option<&'a str> {
            for n in nodes {
                let Node::Element(el) = n else {
                    continue;
                };
                if el.local_name().eq_ignore_ascii_case(tag) {
                    if let Some(v) = el.attr(attr) {
                        return Some(v);
                    }
                }
                if let Some(v) = walk(&el.children, tag, attr) {
                    return Some(v);
                }
            }
            None
        }
        walk(&doc.nodes, tag, attr)
    }

    #[test]
    fn optin_names_are_stable_and_not_default() {
        assert_eq!(
            OPTIN_DANGER_PLUGIN_NAMES,
            &[
                "removeRasterImages",
                "removeOffCanvasPaths",
                "removeScripts",
                "removeStyleElement",
            ]
        );
        for name in OPTIN_DANGER_PLUGIN_NAMES {
            assert!(
                !DEFAULT_PLUGIN_NAMES.contains(name),
                "{name} must stay opt-in / default OFF"
            );
        }
    }

    #[test]
    fn default_optimize_keeps_raster_script_style_and_offcanvas() {
        let input = concat!(
            r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10">"##,
            r##"<style>.a{fill:red}</style>"##,
            r##"<path d="M200 200L210 210" class="a" onclick="go()"/>"##,
            r##"<image href="data:image/png;base64,AAAA"/>"##,
            r##"<script type="text/javascript">1</script>"##,
            "</svg>",
        );
        let out = optimize_str(input).unwrap();
        assert!(
            out.svg.contains("<style") && out.svg.contains(".a"),
            "default must not strip <style>: {}",
            out.svg
        );
        assert!(
            out.svg.contains("<script"),
            "default must not strip <script>: {}",
            out.svg
        );
        assert!(
            out.svg.contains("onclick=") || out.svg.contains("onclick="),
            "default must not strip on*: {}",
            out.svg
        );
        assert!(
            out.svg.contains("<image") && out.svg.contains("data:image/png"),
            "default must not drop rasters: {}",
            out.svg
        );
        assert!(
            out.svg.contains("M200") || out.svg.contains("M 200") || out.svg.contains("200"),
            "default must not drop off-canvas paths: {}",
            out.svg
        );
    }

    #[test]
    fn remove_raster_images_drops_data_image_and_file_href() {
        let input = concat!(
            r##"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink">"##,
            r##"<image href="data:image/png;base64,iVBOR"/>"##,
            r##"<image href="photo.jpg"/>"##,
            r##"<image href="data:image/svg+xml;utf8,<svg/>"/>"##,
            r##"<image xlink:href="data:image/gif;base64,R0lG"/>"##,
            r##"<path d="M0 0h1"/>"##,
            "</svg>",
        );
        let mut doc = parse(input);
        run_remove_raster_images(&mut doc);
        let out = emit(&doc);
        assert!(!out.contains("data:image/png"), "{out}");
        assert!(!out.contains("photo.jpg"), "{out}");
        assert!(!out.contains("data:image/gif"), "{out}");
        assert!(
            out.contains("data:image/svg+xml"),
            "embedded SVG image stays: {out}"
        );
        assert!(out.contains("<path"), "{out}");
        assert_eq!(out.matches("<image").count(), 1, "{out}");
    }

    #[test]
    fn remove_raster_images_keeps_svg_data_and_non_image() {
        let input = concat!(
            r##"<svg xmlns="http://www.w3.org/2000/svg">"##,
            r##"<image href="icon.svg"/>"##,
            r##"<image href="#sprite"/>"##,
            r##"<use href="#sprite"/>"##,
            "</svg>",
        );
        let mut doc = parse(input);
        run_remove_raster_images(&mut doc);
        let out = emit(&doc);
        assert!(out.contains("icon.svg"), "{out}");
        assert!(out.contains("#sprite"), "{out}");
        assert!(out.contains("<use"), "{out}");
    }

    #[test]
    fn remove_off_canvas_paths_drops_only_clearly_outside() {
        let input = concat!(
            r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100">"##,
            r##"<path id="in" d="M10 10L20 20"/>"##,
            r##"<path id="out" d="M200 200L250 250"/>"##,
            r##"<path id="overlap" d="M90 90L150 150"/>"##,
            r##"<path id="cross" d="M200 50L50 50"/>"##,
            r##"<path id="tf" d="M200 200L210 210" transform="translate(-200 -200)"/>"##,
            "</svg>",
        );
        let mut doc = parse(input);
        run_remove_off_canvas_paths(&mut doc);
        let out = emit(&doc);
        assert!(out.contains("id=\"in\""), "{out}");
        assert!(!out.contains("id=\"out\""), "{out}");
        assert!(out.contains("id=\"overlap\""), "{out}");
        assert!(out.contains("id=\"cross\""), "{out}");
        assert!(out.contains("id=\"tf\""), "{out}");
    }

    #[test]
    fn remove_off_canvas_paths_skips_without_view_box() {
        let input = concat!(
            r##"<svg xmlns="http://www.w3.org/2000/svg">"##,
            r##"<path d="M200 200L250 250"/>"##,
            "</svg>",
        );
        let mut doc = parse(input);
        run_remove_off_canvas_paths(&mut doc);
        assert!(emit(&doc).contains("<path"), "{}", emit(&doc));
    }

    #[test]
    fn remove_off_canvas_paths_skips_motion_and_arcs() {
        let motion = concat!(
            r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10">"##,
            r##"<path d="M200 200L210 210"><animate attributeName="d" dur="1s" values="M200 200L210 210;M0 0L1 1"/></path>"##,
            "</svg>",
        );
        let mut doc = parse(motion);
        run_remove_off_canvas_paths(&mut doc);
        assert!(has_name(&doc, "path"), "motion-aware skip: {}", emit(&doc));

        let mut forced = parse(motion);
        run_remove_off_canvas_paths_with(&mut forced, false);
        assert!(
            has_name(&forced, "path"),
            "SMIL child still keeps the path: {}",
            emit(&forced)
        );

        let arc = concat!(
            r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10">"##,
            r##"<path d="M200 200A10 10 0 0 1 220 220"/>"##,
            "</svg>",
        );
        let mut adoc = parse(arc);
        run_remove_off_canvas_paths(&mut adoc);
        assert!(has_name(&adoc, "path"), "arcs stay: {}", emit(&adoc));
    }

    #[test]
    fn remove_off_canvas_paths_keeps_curve_that_may_enter() {
        let input = concat!(
            r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100">"##,
            r##"<path id="bulge" d="M200 200C0 0 0 0 200 201"/>"##,
            r##"<path id="far" d="M200 200C201 201 202 202 203 203"/>"##,
            "</svg>",
        );
        let mut doc = parse(input);
        run_remove_off_canvas_paths(&mut doc);
        let out = emit(&doc);
        assert!(out.contains("id=\"bulge\""), "{out}");
        assert!(!out.contains("id=\"far\""), "{out}");
    }

    #[test]
    fn remove_off_canvas_paths_skips_defs_and_ancestor_transform() {
        let input = concat!(
            r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10">"##,
            r##"<defs><path id="clipd" d="M200 200L210 210"/></defs>"##,
            r##"<g transform="translate(-200 -200)"><path id="moved" d="M200 200L210 210"/></g>"##,
            "</svg>",
        );
        let mut doc = parse(input);
        run_remove_off_canvas_paths(&mut doc);
        let out = emit(&doc);
        assert!(out.contains("id=\"clipd\""), "{out}");
        assert!(out.contains("id=\"moved\""), "{out}");
    }

    #[test]
    fn remove_scripts_drops_script_and_on_handlers() {
        let input = concat!(
            r##"<svg xmlns="http://www.w3.org/2000/svg" onload="boot()">"##,
            r##"<script>alert(1)</script>"##,
            r##"<circle id="c" cx="1" cy="1" r="1" onclick="go()" onmouseover="x()" opacity="0.5"/>"##,
            r##"<g ev:onclick="ns()"><path d="M0 0h1"/></g>"##,
            "</svg>",
        );
        let mut doc = parse(input);
        run_remove_scripts(&mut doc);
        let out = emit(&doc);
        assert!(!has_name(&doc, "script"), "{out}");
        assert!(!out.contains("onclick"), "{out}");
        assert!(!out.contains("onload"), "{out}");
        assert!(!out.contains("onmouseover"), "{out}");
        assert!(out.contains("id=\"c\""), "{out}");
        assert!(
            out.contains("opacity=\"0.5\""),
            "opacity is not an event: {out}"
        );
        assert!(has_name(&doc, "circle"), "{out}");
        assert!(has_name(&doc, "path"), "{out}");
    }

    #[test]
    fn remove_style_element_strips_static_keeps_motion() {
        let static_svg = concat!(
            r##"<svg xmlns="http://www.w3.org/2000/svg">"##,
            r##"<style type="text/css">.a{fill:red}</style>"##,
            r##"<circle class="a" r="2" style="stroke:blue"/>"##,
            "</svg>",
        );
        let mut doc = parse(static_svg);
        run_remove_style_element(&mut doc);
        let out = emit(&doc);
        assert!(!has_name(&doc, "style"), "{out}");
        assert!(out.contains("style=\"stroke:blue\""), "{out}");
        assert!(has_name(&doc, "circle"), "{out}");

        let keyframes = concat!(
            r##"<svg xmlns="http://www.w3.org/2000/svg">"##,
            r##"<style>@keyframes spin{to{transform:rotate(1turn)}}</style>"##,
            r##"<circle r="2"/>"##,
            "</svg>",
        );
        let mut kdoc = parse(keyframes);
        run_remove_style_element(&mut kdoc);
        assert!(
            has_name(&kdoc, "style"),
            "keyframes style stays: {}",
            emit(&kdoc)
        );

        let smil = concat!(
            r##"<svg xmlns="http://www.w3.org/2000/svg">"##,
            r##"<style>.f{fill:red}</style>"##,
            r##"<rect width="1" height="1"><animate attributeName="opacity" values="0;1" dur="1s"/></rect>"##,
            "</svg>",
        );
        let mut sdoc = parse(smil);
        run_remove_style_element(&mut sdoc);
        assert!(
            has_name(&sdoc, "style"),
            "SMIL + style stays when animation-aware: {}",
            emit(&sdoc)
        );

        let mut forced = parse(smil);
        run_remove_style_element_with(&mut forced, false);
        assert!(
            !has_name(&forced, "style"),
            "unchecked strip: {}",
            emit(&forced)
        );
    }

    #[test]
    fn run_dispatcher_matches_named_fns() {
        let input = concat!(
            r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10">"##,
            r##"<style>.a{fill:red}</style>"##,
            r##"<path d="M0 0L1 1" onclick="x()"/>"##,
            r##"<image href="data:image/png;base64,AAAA"/>"##,
            r##"<circle r="1"/>"##,
            "</svg>",
        );
        let mut doc = parse(input);
        for node in &mut doc.nodes {
            assert!(run_optin_danger("removeRasterImages", node));
            assert!(run_optin_danger("removeScripts", node));
            assert!(!run_optin_danger("removeViewBox", node));
        }
        let out = emit(&doc);
        assert!(!out.contains("<image"), "{out}");
        assert!(!out.contains("onclick"), "{out}");
        assert!(
            out.contains("<style"),
            "style stays until its own pass: {out}"
        );
        assert!(out.contains("viewBox"), "{out}");

        for node in &mut doc.nodes {
            assert!(run_optin_danger("removeStyleElement", node));
        }
        assert!(!emit(&doc).contains("<style"), "{}", emit(&doc));

        let far = concat!(
            r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10">"##,
            r##"<path d="M200 200L210 210"/>"##,
            r##"<circle r="1"/>"##,
            "</svg>",
        );
        let mut fdoc = parse(far);
        for node in &mut fdoc.nodes {
            assert!(run_optin_danger("removeOffCanvasPaths", node));
        }
        let fout = emit(&fdoc);
        assert!(!fout.contains("M200"), "{fout}");
        assert!(fout.contains("<circle"), "{fout}");
    }

    #[test]
    fn remove_scripts_does_not_treat_opacity_orient_as_events() {
        let input = concat!(
            r##"<svg xmlns="http://www.w3.org/2000/svg">"##,
            r##"<path d="M0 0h1" opacity="1" orient="auto" overflow="visible"/>"##,
            "</svg>",
        );
        let mut doc = parse(input);
        run_remove_scripts(&mut doc);
        assert_eq!(find_attr(&doc, "path", "opacity"), Some("1"));
        assert_eq!(find_attr(&doc, "path", "orient"), Some("auto"));
        assert_eq!(find_attr(&doc, "path", "overflow"), Some("visible"));
    }
}
