//! Conservative path / shape plugins (SVGO-inspired names, clean-room).
//!
//! Public contracts only — not a port of oxvg / svgm / vexy / SVGO source.
//!
//! * [`convert_path_data`] — parse `d`, pick shorter rel/abs, emit H/V/S/T/Z,
//!   strip leading zeros, omit separators before `-` and command letters.
//!   Implicit-command chunks are space-separated so `0` cannot glue onto the
//!   next number. Digit-glue and dest-count collapse are refused without a
//!   second geometry parse.
//!   Optional `--precision` / `Config.precision` rounding (default: lossless,
//!   no implicit `floatPrecision: 3`). No arc conversion or overlapping-subpath
//!   merge.
//! * [`convert_shape_to_path`] — `line`, `polyline`, axis-aligned `rect`
//!   (no `rx`/`ry`). Circles / ellipses stay (`convertArcs` off).
//! * [`merge_paths`] — concatenate sibling **stroke-only** (`fill="none"`)
//!   `<path>` elements with identical paint attrs. Skips `id`, markers,
//!   filled/class-painted shapes (maps, coats), and SMIL children. A
//!   leading relative `m` is rewritten as an absolute subpath (not a
//!   first-letter swap).
//! * [`convert_transform`] — collapse `translate` / `scale` / `matrix` to a
//!   shorter equivalent. Drops identity. Does **not** bake into path `d`.
//!   Skips SMIL-animated transforms (`animateTransform` / `animateMotion`).
//!
//! Icon-only: `plugins.rs` skips these when `plan.skip_cleanup_ids`.
//! A `d` that SMIL animates is never rewritten even if the pass is invoked.

use crate::ast::{Document, Element, Node};
use std::collections::HashSet;

/// XML 1.0 `S` (`#x20 | #x9 | #xD | #xA`). Not `char::is_whitespace` (NBSP).
fn is_xml_s(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\r')
}

pub const PATH_PLUGIN_NAMES: &[&str] = &[
    "convertShapeToPath",
    "convertPathData",
    "convertTransform",
    "mergePaths",
];

/// Conservative `d` rewrite on path-data hosts. Skips SMIL-animated `d`.
pub fn convert_path_data(doc: &mut Document) {
    convert_path_data_with(doc, None);
}

/// Like [`convert_path_data`], with optional coordinate rounding.
pub fn convert_path_data_with(doc: &mut Document, precision: Option<u8>) {
    let locked = collect_smil_locked_d_ids(&doc.nodes);
    convert_path_data_in(&mut doc.nodes, &locked, precision);
}

fn convert_path_data_in(nodes: &mut [Node], locked: &HashSet<String>, precision: Option<u8>) {
    for node in nodes.iter_mut() {
        let Node::Element(el) = node else {
            continue;
        };
        let child_locks = element_has_smil_d_child(el);
        let id_locked = el.attr("id").is_some_and(|id| locked.contains(id));
        if is_path_d_host(el.local_name()) && !child_locks && !id_locked {
            if let Some(d) = el.attr("d") {
                let skip_tight = path_d_already_tight(d)
                    && !precision.is_some_and(|p| has_excess_precision(d, p));
                if !skip_tight {
                    let next = match minify_path_d_with(d, precision) {
                        Some(mini) => mini,
                        None => trim_path_d_whitespace(d),
                    };
                    if next.len() < d.len() {
                        el.set_attr("d", next);
                    }
                }
            }
        }
        convert_path_data_in(&mut el.children, locked, precision);
    }
}

/// True when a `d` has a fractional run longer than `prec` (after trailing zeros).
fn has_excess_precision(d: &str, prec: u8) -> bool {
    let b = d.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'.' {
            let mut j = i + 1;
            while j < b.len() && b[j].is_ascii_digit() {
                j += 1;
            }
            let mut k = j;
            while k > i + 1 && b[k - 1] == b'0' {
                k -= 1;
            }
            if k - (i + 1) > prec as usize {
                return true;
            }
            i = j;
            continue;
        }
        i += 1;
    }
    false
}

/// Skip convertPathData when `d` has no commas, wasted XML space, or
/// leading/trailing zeros a rewrite would strip. oxvg/svgm-style cheap reject.
fn path_d_already_tight(d: &str) -> bool {
    if d.is_empty() {
        return true;
    }
    let b = d.as_bytes();
    let mut prev = 0u8;
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        match c {
            b',' | b'\t' | b'\n' | b'\r' => return false,
            b' ' => {
                if prev == 0 || prev == b' ' || is_path_command_byte(prev) {
                    return false;
                }
                if i + 1 < b.len() && (b[i + 1] == b'-' || is_path_command_byte(b[i + 1])) {
                    return false;
                }
            }
            b'0' => {
                if b.get(i + 1) == Some(&b'.') && !prev.is_ascii_digit() && prev != b'.' {
                    return false;
                }
            }
            b'.' => {
                let mut j = i + 1;
                while j < b.len() && b[j].is_ascii_digit() {
                    j += 1;
                }
                if j > i + 1 && b[j - 1] == b'0' {
                    return false;
                }
            }
            _ => {}
        }
        prev = c;
        i += 1;
    }
    true
}

fn is_path_command_byte(c: u8) -> bool {
    matches!(
        c,
        b'M' | b'm'
            | b'Z'
            | b'z'
            | b'L'
            | b'l'
            | b'H'
            | b'h'
            | b'V'
            | b'v'
            | b'C'
            | b'c'
            | b'S'
            | b's'
            | b'Q'
            | b'q'
            | b'T'
            | b't'
            | b'A'
            | b'a'
    )
}

/// SVG 1.1 / 2 path-data hosts named by SVGO and oxvg (local name).
pub fn is_path_d_host(local_name: &str) -> bool {
    matches!(local_name, "path" | "glyph" | "missing-glyph")
}

/// Lossless `d` whitespace / comma-space trim (fallback when parse fails).
pub fn trim_path_d_whitespace(d: &str) -> String {
    let mut out = String::with_capacity(d.len());
    let mut pending_space = false;
    let mut seen_token = false;

    for c in d.chars() {
        if is_xml_s(c) {
            if seen_token {
                pending_space = true;
            }
            continue;
        }
        if c == ',' {
            out.push(',');
            pending_space = false;
            seen_token = true;
            continue;
        }
        if pending_space {
            if !skip_space_before_next(&out) {
                out.push(' ');
            }
            pending_space = false;
        }
        out.push(c);
        seen_token = true;
    }
    out
}

fn skip_space_before_next(out: &str) -> bool {
    match out.chars().next_back() {
        Some(',') => true,
        Some(prev) if is_path_command_letter(prev) => true,
        _ => false,
    }
}

fn is_path_command_letter(c: char) -> bool {
    c.is_ascii() && is_path_command_byte(c as u8)
}

/// Minify one `d` string. `None` if the path cannot be parsed safely
/// or the rewrite would collapse dest-count / glue digits.
pub fn minify_path_d(d: &str) -> Option<String> {
    minify_path_d_with(d, None)
}

/// Minify `d` with optional coordinate rounding. Failed precision rewrites
/// fall back to the lossless pass (dest-count / glue / bbox still apply).
pub fn minify_path_d_with(d: &str, precision: Option<u8>) -> Option<String> {
    if precision.is_some() {
        if let Some(out) = minify_path_d_attempt(d, precision) {
            return Some(out);
        }
    }
    minify_path_d_attempt(d, None)
}

fn minify_path_d_attempt(d: &str, precision: Option<u8>) -> Option<String> {
    let atoms = parse_path(d)?;
    if atoms.is_empty() {
        return Some(String::new());
    }
    let mut abs = to_abs(&atoms);
    let orig_pts = dest_point_count(&abs);
    let orig_bbox = path_bbox(&abs);
    if let Some(p) = precision {
        round_abs(&mut abs, i32::from(p));
    }
    let opt = optimize_abs(abs);
    if orig_pts >= 8 && dest_point_count(&opt) <= 3 {
        return None;
    }
    let emitted = emit_path(&opt);
    if emit_has_digit_glue(&emitted) {
        return None;
    }
    let reparsed = parse_path(&emitted)?;
    let new_abs = to_abs(&reparsed);
    if dest_point_count(&new_abs) != dest_point_count(&opt) {
        return None;
    }
    if !bbox_compatible(orig_bbox, path_bbox(&new_abs)) {
        return None;
    }
    Some(emitted)
}

/// Cheap post-emit scan for glued numbers (`6.408 014.1912`, `12`+`.297`).
/// Two-digit `00`/`01` arc flags are allowed.
fn emit_has_digit_glue(s: &str) -> bool {
    let b = s.as_bytes();
    let mut i = 0;
    let mut prev_was_number = false;
    let mut prev_had_dot = false;
    while i < b.len() {
        let c = b[i];
        if is_path_command_byte(c) || matches!(c, b' ' | b',' | b'\t' | b'\n' | b'\r') {
            prev_was_number = false;
            i += 1;
            continue;
        }
        if c == b'+' || c == b'-' || c == b'.' || c.is_ascii_digit() {
            if prev_was_number {
                if c.is_ascii_digit() {
                    return true;
                }
                if c == b'.' && !prev_had_dot {
                    return true;
                }
            }
            let mut had_dot = c == b'.';
            let mut j = i + 1;
            if (c == b'+' || c == b'-') && j < b.len() && b[j] == b'.' {
                had_dot = true;
                j += 1;
            }
            let first = if c == b'+' || c == b'-' {
                b.get(i + 1).copied()
            } else {
                Some(c)
            };
            let after_zero = if c == b'+' || c == b'-' { i + 2 } else { i + 1 };
            if first == Some(b'0') {
                if let Some(&n) = b.get(after_zero) {
                    if n.is_ascii_digit() {
                        let mut k = after_zero;
                        while k < b.len() && b[k].is_ascii_digit() {
                            k += 1;
                        }
                        let int_digits = 1 + (k - after_zero);
                        let has_frac = k < b.len() && b[k] == b'.';
                        // `00`/`01` arc flags stay; `014` / `014.1912` do not.
                        if int_digits > 2 || has_frac {
                            return true;
                        }
                    }
                }
            }
            i = j;
            while i < b.len() {
                let ch = b[i];
                if ch.is_ascii_digit() {
                    i += 1;
                } else if ch == b'.' && !had_dot {
                    had_dot = true;
                    i += 1;
                } else if ch == b'e' || ch == b'E' {
                    i += 1;
                    if i < b.len() && matches!(b[i], b'+' | b'-') {
                        i += 1;
                    }
                    while i < b.len() && b[i].is_ascii_digit() {
                        i += 1;
                    }
                    break;
                } else {
                    break;
                }
            }
            prev_was_number = true;
            prev_had_dot = had_dot;
            continue;
        }
        i += 1;
    }
    false
}

#[derive(Clone, Debug)]
enum Atom {
    Move {
        x: f64,
        y: f64,
        rel: bool,
    },
    Line {
        x: f64,
        y: f64,
        rel: bool,
    },
    H {
        x: f64,
        rel: bool,
    },
    V {
        y: f64,
        rel: bool,
    },
    Cubic {
        x1: f64,
        y1: f64,
        x2: f64,
        y2: f64,
        x: f64,
        y: f64,
        rel: bool,
    },
    SmoothC {
        x2: f64,
        y2: f64,
        x: f64,
        y: f64,
        rel: bool,
    },
    Quad {
        x1: f64,
        y1: f64,
        x: f64,
        y: f64,
        rel: bool,
    },
    SmoothQ {
        x: f64,
        y: f64,
        rel: bool,
    },
    Arc {
        rx: f64,
        ry: f64,
        rot: f64,
        large: bool,
        sweep: bool,
        x: f64,
        y: f64,
        rel: bool,
    },
    Close,
}

#[derive(Clone, Debug)]
enum Abs {
    Move {
        x: f64,
        y: f64,
    },
    Line {
        x: f64,
        y: f64,
    },
    H {
        x: f64,
    },
    V {
        y: f64,
    },
    Cubic {
        x1: f64,
        y1: f64,
        x2: f64,
        y2: f64,
        x: f64,
        y: f64,
    },
    SmoothC {
        x2: f64,
        y2: f64,
        x: f64,
        y: f64,
    },
    Quad {
        x1: f64,
        y1: f64,
        x: f64,
        y: f64,
    },
    SmoothQ {
        x: f64,
        y: f64,
    },
    Arc {
        rx: f64,
        ry: f64,
        rot: f64,
        large: bool,
        sweep: bool,
        x: f64,
        y: f64,
    },
    Close,
}

fn nearly_eq(a: f64, b: f64) -> bool {
    if a == b {
        return true;
    }
    (a - b).abs() <= 1e-12_f64 * (1.0 + a.abs().max(b.abs()))
}

fn to_abs(atoms: &[Atom]) -> Vec<Abs> {
    let mut cx = 0.0;
    let mut cy = 0.0;
    let mut sx = 0.0;
    let mut sy = 0.0;
    let mut out = Vec::with_capacity(atoms.len());
    for atom in atoms {
        match *atom {
            Atom::Move { x, y, rel } => {
                let (x, y) = if rel { (cx + x, cy + y) } else { (x, y) };
                cx = x;
                cy = y;
                sx = x;
                sy = y;
                out.push(Abs::Move { x, y });
            }
            Atom::Line { x, y, rel } => {
                let (x, y) = if rel { (cx + x, cy + y) } else { (x, y) };
                cx = x;
                cy = y;
                out.push(Abs::Line { x, y });
            }
            Atom::H { x, rel } => {
                let x = if rel { cx + x } else { x };
                cx = x;
                out.push(Abs::H { x });
            }
            Atom::V { y, rel } => {
                let y = if rel { cy + y } else { y };
                cy = y;
                out.push(Abs::V { y });
            }
            Atom::Cubic {
                x1,
                y1,
                x2,
                y2,
                x,
                y,
                rel,
            } => {
                let (x1, y1, x2, y2, x, y) = if rel {
                    (cx + x1, cy + y1, cx + x2, cy + y2, cx + x, cy + y)
                } else {
                    (x1, y1, x2, y2, x, y)
                };
                cx = x;
                cy = y;
                out.push(Abs::Cubic {
                    x1,
                    y1,
                    x2,
                    y2,
                    x,
                    y,
                });
            }
            Atom::SmoothC { x2, y2, x, y, rel } => {
                let (x2, y2, x, y) = if rel {
                    (cx + x2, cy + y2, cx + x, cy + y)
                } else {
                    (x2, y2, x, y)
                };
                cx = x;
                cy = y;
                out.push(Abs::SmoothC { x2, y2, x, y });
            }
            Atom::Quad { x1, y1, x, y, rel } => {
                let (x1, y1, x, y) = if rel {
                    (cx + x1, cy + y1, cx + x, cy + y)
                } else {
                    (x1, y1, x, y)
                };
                cx = x;
                cy = y;
                out.push(Abs::Quad { x1, y1, x, y });
            }
            Atom::SmoothQ { x, y, rel } => {
                let (x, y) = if rel { (cx + x, cy + y) } else { (x, y) };
                cx = x;
                cy = y;
                out.push(Abs::SmoothQ { x, y });
            }
            Atom::Arc {
                rx,
                ry,
                rot,
                large,
                sweep,
                x,
                y,
                rel,
            } => {
                let (x, y) = if rel { (cx + x, cy + y) } else { (x, y) };
                cx = x;
                cy = y;
                out.push(Abs::Arc {
                    rx,
                    ry,
                    rot,
                    large,
                    sweep,
                    x,
                    y,
                });
            }
            Atom::Close => {
                cx = sx;
                cy = sy;
                out.push(Abs::Close);
            }
        }
    }
    out
}

fn optimize_abs(atoms: Vec<Abs>) -> Vec<Abs> {
    let mut cx = 0.0;
    let mut cy = 0.0;
    let mut sx = 0.0;
    let mut sy = 0.0;
    let mut prev_ctrl: Option<(f64, f64)> = None;
    let mut prev_kind = ' ';
    let mut out: Vec<Abs> = Vec::with_capacity(atoms.len());

    for atom in atoms {
        let mut atom = atom;
        match atom {
            Abs::Line { x, y } => {
                if nearly_eq(x, cx) && nearly_eq(y, cy) {
                    continue;
                }
                if nearly_eq(y, cy) {
                    atom = Abs::H { x };
                } else if nearly_eq(x, cx) {
                    atom = Abs::V { y };
                }
            }
            Abs::H { x } if nearly_eq(x, cx) => continue,
            Abs::V { y } if nearly_eq(y, cy) => continue,
            Abs::Cubic {
                x1,
                y1,
                x2,
                y2,
                x,
                y,
            } => {
                let (rx, ry) = reflect_ctrl(prev_ctrl, prev_kind, cx, cy, true);
                if nearly_eq(x1, rx) && nearly_eq(y1, ry) {
                    atom = Abs::SmoothC { x2, y2, x, y };
                }
            }
            Abs::Quad { x1, y1, x, y } => {
                let (rx, ry) = reflect_ctrl(prev_ctrl, prev_kind, cx, cy, false);
                if nearly_eq(x1, rx) && nearly_eq(y1, ry) {
                    atom = Abs::SmoothQ { x, y };
                }
            }
            _ => {}
        }

        match &atom {
            Abs::Move { x, y } => {
                cx = *x;
                cy = *y;
                sx = *x;
                sy = *y;
                prev_ctrl = None;
                prev_kind = 'M';
            }
            Abs::Line { x, y } => {
                cx = *x;
                cy = *y;
                prev_ctrl = None;
                prev_kind = 'L';
            }
            Abs::H { x } => {
                cx = *x;
                prev_ctrl = None;
                prev_kind = 'H';
            }
            Abs::V { y } => {
                cy = *y;
                prev_ctrl = None;
                prev_kind = 'V';
            }
            Abs::Cubic { x2, y2, x, y, .. } => {
                prev_ctrl = Some((*x2, *y2));
                cx = *x;
                cy = *y;
                prev_kind = 'C';
            }
            Abs::SmoothC { x2, y2, x, y } => {
                prev_ctrl = Some((*x2, *y2));
                cx = *x;
                cy = *y;
                prev_kind = 'S';
            }
            Abs::Quad { x1, y1, x, y } => {
                prev_ctrl = Some((*x1, *y1));
                cx = *x;
                cy = *y;
                prev_kind = 'Q';
            }
            Abs::SmoothQ { x, y } => {
                let (rx, ry) = reflect_ctrl(prev_ctrl, prev_kind, cx, cy, false);
                prev_ctrl = Some((rx, ry));
                cx = *x;
                cy = *y;
                prev_kind = 'T';
            }
            Abs::Arc { x, y, .. } => {
                cx = *x;
                cy = *y;
                prev_ctrl = None;
                prev_kind = 'A';
            }
            Abs::Close => {
                cx = sx;
                cy = sy;
                prev_ctrl = None;
                prev_kind = 'Z';
            }
        }
        out.push(atom);
    }

    drop_lineto_before_close(out)
}

fn reflect_ctrl(
    prev_ctrl: Option<(f64, f64)>,
    prev_kind: char,
    cx: f64,
    cy: f64,
    cubic: bool,
) -> (f64, f64) {
    let ok = if cubic {
        matches!(prev_kind, 'C' | 'S')
    } else {
        matches!(prev_kind, 'Q' | 'T')
    };
    if ok {
        if let Some((px, py)) = prev_ctrl {
            return (2.0 * cx - px, 2.0 * cy - py);
        }
    }
    (cx, cy)
}

fn drop_lineto_before_close(atoms: Vec<Abs>) -> Vec<Abs> {
    let mut cx = 0.0;
    let mut cy = 0.0;
    let mut sx = 0.0;
    let mut sy = 0.0;
    let mut out = Vec::with_capacity(atoms.len());
    let mut i = 0;
    while i < atoms.len() {
        let atom = atoms[i].clone();
        let next_close = matches!(atoms.get(i + 1), Some(Abs::Close));
        let drop = next_close
            && match &atom {
                Abs::Line { x, y } => nearly_eq(*x, sx) && nearly_eq(*y, sy),
                Abs::H { x } => nearly_eq(*x, sx) && nearly_eq(cy, sy),
                Abs::V { y } => nearly_eq(cx, sx) && nearly_eq(*y, sy),
                _ => false,
            };
        if !drop {
            match &atom {
                Abs::Move { x, y } => {
                    cx = *x;
                    cy = *y;
                    sx = *x;
                    sy = *y;
                }
                Abs::Line { x, y }
                | Abs::Cubic { x, y, .. }
                | Abs::SmoothC { x, y, .. }
                | Abs::Quad { x, y, .. }
                | Abs::SmoothQ { x, y }
                | Abs::Arc { x, y, .. } => {
                    cx = *x;
                    cy = *y;
                }
                Abs::H { x } => cx = *x,
                Abs::V { y } => cy = *y,
                Abs::Close => {
                    cx = sx;
                    cy = sy;
                }
            }
            out.push(atom);
        }
        i += 1;
    }
    out
}

fn emit_path(atoms: &[Abs]) -> String {
    emit_path_inner(atoms, false)
}

/// Emit a standalone subpath whose first moveto is absolute.
///
/// A leading relative `m` is relative to (0,0) only as its own `d`. After
/// concatenation it would be relative to the previous pen — so the first
/// move must be `M`. Implicit linetos after that move stay relative (`l`),
/// unlike a naive first-letter `m` → `M` swap (`M139 56 15-16` is L 15,-16).
fn emit_path_new_subpath(atoms: &[Abs]) -> String {
    emit_path_inner(atoms, true)
}

fn emit_path_inner(atoms: &[Abs], abs_first_move: bool) -> String {
    let mut out = String::with_capacity(atoms.len().saturating_mul(12));
    let mut abs_s = String::with_capacity(48);
    let mut rel_s = String::with_capacity(48);
    let mut abs_n = [0.0f64; 6];
    let mut rel_n = [0.0f64; 6];
    let mut num_buf = [0u8; 32];
    let mut cx = 0.0;
    let mut cy = 0.0;
    let mut sx = 0.0;
    let mut sy = 0.0;
    let mut prev: Option<char> = None;
    let mut first_cmd = true;

    for atom in atoms {
        if matches!(atom, Abs::Close) {
            out.push('z');
            cx = sx;
            cy = sy;
            prev = None;
            first_cmd = false;
            continue;
        }

        let (abs_l, abs_len, abs_arc) = fill_cmd(atom, 0.0, 0.0, false, &mut abs_n);
        let (rel_l, rel_len, rel_arc) = fill_cmd(atom, cx, cy, true, &mut rel_n);

        abs_s.clear();
        rel_s.clear();
        write_cmd(
            &mut abs_s,
            abs_l,
            &abs_n[..abs_len],
            abs_arc,
            prev,
            &mut num_buf,
        );
        write_cmd(
            &mut rel_s,
            rel_l,
            &rel_n[..rel_len],
            rel_arc,
            prev,
            &mut num_buf,
        );

        let use_rel = if first_cmd && abs_first_move {
            false
        } else {
            rel_s.len() < abs_s.len() || (rel_s.len() == abs_s.len() && rel_l.is_ascii_lowercase())
        };
        first_cmd = false;
        let (letter, chunk) = if use_rel {
            (rel_l, &rel_s)
        } else {
            (abs_l, &abs_s)
        };
        // Implicit-command chunks start with a number. Joining `6.408 0` +
        // `14.1912` must not become `6.408 014.1912` (world-map ocean triangle).
        if needs_sep(&out, chunk) {
            out.push(' ');
        }
        out.push_str(chunk);
        prev = Some(letter);
        update_cursor(atom, &mut cx, &mut cy, &mut sx, &mut sy);
    }
    out
}

fn update_cursor(atom: &Abs, cx: &mut f64, cy: &mut f64, sx: &mut f64, sy: &mut f64) {
    match *atom {
        Abs::Move { x, y } => {
            *cx = x;
            *cy = y;
            *sx = x;
            *sy = y;
        }
        Abs::Line { x, y }
        | Abs::Cubic { x, y, .. }
        | Abs::SmoothC { x, y, .. }
        | Abs::Quad { x, y, .. }
        | Abs::SmoothQ { x, y }
        | Abs::Arc { x, y, .. } => {
            *cx = x;
            *cy = y;
        }
        Abs::H { x } => *cx = x,
        Abs::V { y } => *cy = y,
        Abs::Close => {
            *cx = *sx;
            *cy = *sy;
        }
    }
}

fn fill_cmd(
    atom: &Abs,
    cx: f64,
    cy: f64,
    rel: bool,
    nums: &mut [f64; 6],
) -> (char, usize, Option<(bool, bool)>) {
    let (dx, dy) = if rel { (cx, cy) } else { (0.0, 0.0) };
    match *atom {
        Abs::Move { x, y } => {
            nums[0] = x - dx;
            nums[1] = y - dy;
            (if rel { 'm' } else { 'M' }, 2, None)
        }
        Abs::Line { x, y } => {
            nums[0] = x - dx;
            nums[1] = y - dy;
            (if rel { 'l' } else { 'L' }, 2, None)
        }
        Abs::H { x } => {
            nums[0] = x - dx;
            (if rel { 'h' } else { 'H' }, 1, None)
        }
        Abs::V { y } => {
            nums[0] = y - dy;
            (if rel { 'v' } else { 'V' }, 1, None)
        }
        Abs::Cubic {
            x1,
            y1,
            x2,
            y2,
            x,
            y,
        } => {
            nums[0] = x1 - dx;
            nums[1] = y1 - dy;
            nums[2] = x2 - dx;
            nums[3] = y2 - dy;
            nums[4] = x - dx;
            nums[5] = y - dy;
            (if rel { 'c' } else { 'C' }, 6, None)
        }
        Abs::SmoothC { x2, y2, x, y } => {
            nums[0] = x2 - dx;
            nums[1] = y2 - dy;
            nums[2] = x - dx;
            nums[3] = y - dy;
            (if rel { 's' } else { 'S' }, 4, None)
        }
        Abs::Quad { x1, y1, x, y } => {
            nums[0] = x1 - dx;
            nums[1] = y1 - dy;
            nums[2] = x - dx;
            nums[3] = y - dy;
            (if rel { 'q' } else { 'Q' }, 4, None)
        }
        Abs::SmoothQ { x, y } => {
            nums[0] = x - dx;
            nums[1] = y - dy;
            (if rel { 't' } else { 'T' }, 2, None)
        }
        Abs::Arc {
            rx,
            ry,
            rot,
            large,
            sweep,
            x,
            y,
        } => {
            nums[0] = rx;
            nums[1] = ry;
            nums[2] = rot;
            nums[3] = x - dx;
            nums[4] = y - dy;
            (if rel { 'a' } else { 'A' }, 5, Some((large, sweep)))
        }
        Abs::Close => (if rel { 'z' } else { 'Z' }, 0, None),
    }
}

fn write_cmd(
    out: &mut String,
    letter: char,
    nums: &[f64],
    arc_flags: Option<(bool, bool)>,
    prev: Option<char>,
    buf: &mut [u8; 32],
) {
    let omit = prev == Some(letter)
        || (prev == Some('M') && letter == 'L')
        || (prev == Some('m') && letter == 'l');
    if !omit {
        out.push(letter);
    }
    if let Some((large, sweep)) = arc_flags {
        if nums.len() != 5 {
            for n in nums {
                push_num(out, *n, buf);
            }
            return;
        }
        push_num(out, nums[0], buf);
        push_num(out, nums[1], buf);
        push_num(out, nums[2], buf);
        if out.chars().next_back().is_some_and(|c| c.is_ascii_digit()) {
            out.push(' ');
        }
        out.push(if large { '1' } else { '0' });
        out.push(if sweep { '1' } else { '0' });
        push_num(out, nums[3], buf);
        push_num(out, nums[4], buf);
    } else {
        for n in nums {
            push_num(out, *n, buf);
        }
    }
}

fn push_num(out: &mut String, n: f64, buf: &mut [u8; 32]) {
    let s = write_compact_f64(buf, n);
    if needs_sep(out, s) {
        out.push(' ');
    }
    out.push_str(s);
}

fn needs_sep(out: &str, next: &str) -> bool {
    let Some(prev) = out.chars().next_back() else {
        return false;
    };
    if next.starts_with('-') {
        return false;
    }
    if next.chars().next().is_some_and(is_path_command_letter) {
        return false;
    }
    if is_path_command_letter(prev) {
        return false;
    }
    if next.starts_with('.') && prev == '.' || (next.starts_with('.') && matches!(prev, 'e' | 'E'))
    {
        return true;
    }
    // `12` + `.297` must not become `12.297`. `0.5` + `.5` may be `0.5.5`.
    if next.starts_with('.') {
        return !prev_token_has_dot(out);
    }
    prev.is_ascii_digit() || prev == '.'
}

fn prev_token_has_dot(out: &str) -> bool {
    let mut saw_dot = false;
    for c in out.chars().rev() {
        if c == '.' {
            saw_dot = true;
            continue;
        }
        if c.is_ascii_digit() {
            continue;
        }
        break;
    }
    saw_dot
}

fn fmt_f64(n: f64) -> String {
    let mut buf = [0u8; 32];
    write_compact_f64(&mut buf, n).to_string()
}

fn write_compact_f64<'a>(buf: &'a mut [u8; 32], n: f64) -> &'a str {
    if !n.is_finite() {
        buf[0] = b'0';
        return std::str::from_utf8(&buf[..1]).unwrap();
    }
    let n = if n == 0.0 { 0.0 } else { n };
    if (n - n.round()).abs() < 1e-12 && n.abs() < 1e14 {
        return write_i64_into(buf, n.round() as i64);
    }
    if n.abs() >= 1e12 {
        let s = minify_number_lexeme(&format!("{n:.8}"));
        let len = s.len().min(32);
        buf[..len].copy_from_slice(&s.as_bytes()[..len]);
        return std::str::from_utf8(&buf[..len]).unwrap();
    }
    let neg = n < 0.0;
    let scaled = (n.abs() * 100_000_000.0).round() as u64;
    let int = scaled / 100_000_000;
    let mut frac = scaled % 100_000_000;
    if frac == 0 {
        let v = if neg { -(int as i64) } else { int as i64 };
        return write_i64_into(buf, v);
    }
    let mut frac_digits = 8u32;
    while frac % 10 == 0 {
        frac /= 10;
        frac_digits -= 1;
    }
    let mut i = 32;
    let mut f = frac;
    for _ in 0..frac_digits {
        i -= 1;
        buf[i] = b'0' + (f % 10) as u8;
        f /= 10;
    }
    i -= 1;
    buf[i] = b'.';
    if int != 0 {
        let mut v = int;
        while v > 0 {
            i -= 1;
            buf[i] = b'0' + (v % 10) as u8;
            v /= 10;
        }
    }
    if neg {
        i -= 1;
        buf[i] = b'-';
    }
    std::str::from_utf8(&buf[i..32]).unwrap()
}

fn write_i64_into(buf: &mut [u8; 32], n: i64) -> &str {
    if n == 0 {
        buf[31] = b'0';
        return std::str::from_utf8(&buf[31..32]).unwrap();
    }
    let neg = n < 0;
    let mut v = n.unsigned_abs();
    let mut i = 32;
    while v > 0 {
        i -= 1;
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
    }
    if neg {
        i -= 1;
        buf[i] = b'-';
    }
    std::str::from_utf8(&buf[i..32]).unwrap()
}

/// Lossless lexeme compact: `0.18` → `.18`, `10.0` → `10`, `+4` → `4`.
pub fn minify_number_lexeme(raw: &str) -> String {
    let raw = raw.trim();
    if raw.is_empty() {
        return raw.to_string();
    }
    let (neg, s) = if let Some(r) = raw.strip_prefix('+') {
        (false, r)
    } else if let Some(r) = raw.strip_prefix('-') {
        (true, r)
    } else {
        (false, raw)
    };
    if s.find(['e', 'E']).is_some() {
        return raw.to_string();
    }
    let (int, frac) = match s.find('.') {
        Some(i) => (&s[..i], Some(&s[i + 1..])),
        None => (s, None),
    };
    let int = int.trim_start_matches('0');
    let frac = frac.map(|f| f.trim_end_matches('0'));
    let sign = if neg { "-" } else { "" };
    match frac {
        None | Some("") => {
            let i = if int.is_empty() { "0" } else { int };
            format!("{sign}{i}")
        }
        Some(f) => {
            if int.is_empty() {
                format!("{sign}.{f}")
            } else {
                format!("{sign}{int}.{f}")
            }
        }
    }
}

struct Parser<'a> {
    b: &'a [u8],
    i: usize,
}

impl<'a> Parser<'a> {
    fn new(s: &'a str) -> Self {
        Self {
            b: s.as_bytes(),
            i: 0,
        }
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

    fn peek(&self) -> Option<u8> {
        self.b.get(self.i).copied()
    }

    fn parse_number(&mut self) -> Option<f64> {
        self.skip_sep();
        let start = self.i;
        if matches!(self.peek(), Some(b'+') | Some(b'-')) {
            self.i += 1;
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
            let exp_start = self.i;
            while self.peek().is_some_and(|c| c.is_ascii_digit()) {
                self.i += 1;
            }
            if self.i == exp_start {
                self.i = save;
            }
        }
        if !saw_digit && !(saw_dot && start < self.i) {
            self.i = start;
            return None;
        }
        let token = std::str::from_utf8(&self.b[start..self.i]).ok()?;
        token.parse().ok()
    }

    fn parse_flag(&mut self) -> Option<bool> {
        self.skip_sep();
        match self.peek() {
            Some(b'0') => {
                self.i += 1;
                Some(false)
            }
            Some(b'1') => {
                self.i += 1;
                Some(true)
            }
            _ => None,
        }
    }
}

fn parse_path(d: &str) -> Option<Vec<Atom>> {
    let mut p = Parser::new(d);
    let mut atoms = Vec::with_capacity((d.len() / 8).max(4));
    let mut cmd: Option<u8> = None;
    loop {
        p.skip_sep();
        if p.i >= p.b.len() {
            break;
        }
        if p.peek().is_some_and(|c| c.is_ascii_alphabetic()) {
            cmd = Some(p.b[p.i]);
            p.i += 1;
        }
        let c = cmd?;
        match c {
            b'M' | b'm' => {
                let x = p.parse_number()?;
                let y = p.parse_number()?;
                let rel = c == b'm';
                atoms.push(Atom::Move { x, y, rel });
                // subsequent pairs are implicit L/l
                loop {
                    let save = p.i;
                    p.skip_sep();
                    if p.peek().is_some_and(|ch| ch.is_ascii_alphabetic()) {
                        p.i = save;
                        break;
                    }
                    match p.parse_number() {
                        Some(x) => {
                            let y = p.parse_number()?;
                            atoms.push(Atom::Line { x, y, rel });
                        }
                        None => {
                            p.i = save;
                            break;
                        }
                    }
                }
                cmd = Some(if rel { b'l' } else { b'L' });
            }
            b'L' | b'l' => {
                let x = p.parse_number()?;
                let y = p.parse_number()?;
                atoms.push(Atom::Line {
                    x,
                    y,
                    rel: c == b'l',
                });
            }
            b'H' | b'h' => {
                let x = p.parse_number()?;
                atoms.push(Atom::H { x, rel: c == b'h' });
            }
            b'V' | b'v' => {
                let y = p.parse_number()?;
                atoms.push(Atom::V { y, rel: c == b'v' });
            }
            b'C' | b'c' => {
                let x1 = p.parse_number()?;
                let y1 = p.parse_number()?;
                let x2 = p.parse_number()?;
                let y2 = p.parse_number()?;
                let x = p.parse_number()?;
                let y = p.parse_number()?;
                atoms.push(Atom::Cubic {
                    x1,
                    y1,
                    x2,
                    y2,
                    x,
                    y,
                    rel: c == b'c',
                });
            }
            b'S' | b's' => {
                let x2 = p.parse_number()?;
                let y2 = p.parse_number()?;
                let x = p.parse_number()?;
                let y = p.parse_number()?;
                atoms.push(Atom::SmoothC {
                    x2,
                    y2,
                    x,
                    y,
                    rel: c == b's',
                });
            }
            b'Q' | b'q' => {
                let x1 = p.parse_number()?;
                let y1 = p.parse_number()?;
                let x = p.parse_number()?;
                let y = p.parse_number()?;
                atoms.push(Atom::Quad {
                    x1,
                    y1,
                    x,
                    y,
                    rel: c == b'q',
                });
            }
            b'T' | b't' => {
                let x = p.parse_number()?;
                let y = p.parse_number()?;
                atoms.push(Atom::SmoothQ {
                    x,
                    y,
                    rel: c == b't',
                });
            }
            b'A' | b'a' => {
                let rx = p.parse_number()?;
                let ry = p.parse_number()?;
                let rot = p.parse_number()?;
                let large = p.parse_flag()?;
                let sweep = p.parse_flag()?;
                let x = p.parse_number()?;
                let y = p.parse_number()?;
                atoms.push(Atom::Arc {
                    rx,
                    ry,
                    rot,
                    large,
                    sweep,
                    x,
                    y,
                    rel: c == b'a',
                });
            }
            b'Z' | b'z' => atoms.push(Atom::Close),
            _ => return None,
        }
    }
    Some(atoms)
}

/// `line` / `polyline` / axis-aligned `rect` → `<path>`. No circles, no rounded rects.
pub fn convert_shape_to_path(doc: &mut Document) {
    doc.walk_elements_mut(&mut |el| {
        if has_element_child(el) {
            return;
        }
        match el.local_name() {
            "line" => convert_line(el),
            "polyline" => convert_polyline(el),
            "rect" => convert_axis_rect(el),
            _ => {}
        }
    });
}

fn has_element_child(el: &Element) -> bool {
    el.children.iter().any(|n| matches!(n, Node::Element(_)))
}

fn rename_to_path(el: &mut Element) {
    el.name = match el.prefix() {
        Some(p) => format!("{p}:path"),
        None => "path".into(),
    };
    el.self_closing = true;
}

fn parse_attr_f64(el: &Element, name: &str, default: f64) -> f64 {
    el.attr(name)
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(default)
}

fn convert_line(el: &mut Element) {
    let x1 = parse_attr_f64(el, "x1", 0.0);
    let y1 = parse_attr_f64(el, "y1", 0.0);
    let x2 = parse_attr_f64(el, "x2", 0.0);
    let y2 = parse_attr_f64(el, "y2", 0.0);
    let d = minify_path_d(&format!("M {x1} {y1} L {x2} {y2}")).unwrap_or_else(|| {
        format!(
            "M{} {}L{} {}",
            fmt_f64(x1),
            fmt_f64(y1),
            fmt_f64(x2),
            fmt_f64(y2)
        )
    });
    rename_to_path(el);
    el.set_attr("d", d);
    for a in ["x1", "y1", "x2", "y2"] {
        el.remove_attr(a);
    }
    // `<line>` is never filled. A bare `<path>` defaults to fill=black.
    if !has_fill_spec(el) {
        el.set_attr("fill", "none".to_string());
    }
}

fn convert_polyline(el: &mut Element) {
    let Some(pts) = el.attr("points") else {
        return;
    };
    let pairs = parse_points(pts);
    if pairs.len() < 2 {
        return;
    }
    let mut raw = format!("M {} {}", pairs[0].0, pairs[0].1);
    for (x, y) in pairs.iter().skip(1) {
        raw.push_str(&format!(" L {x} {y}"));
    }
    let d = minify_path_d(&raw).unwrap_or(raw);
    rename_to_path(el);
    el.set_attr("d", d);
    el.remove_attr("points");
}

fn convert_axis_rect(el: &mut Element) {
    let rx = el.attr("rx").and_then(|s| s.trim().parse::<f64>().ok());
    let ry = el.attr("ry").and_then(|s| s.trim().parse::<f64>().ok());
    if rx.is_some_and(|v| v > 0.0) || ry.is_some_and(|v| v > 0.0) {
        return;
    }
    let x = parse_attr_f64(el, "x", 0.0);
    let y = parse_attr_f64(el, "y", 0.0);
    let w = parse_attr_f64(el, "width", 0.0);
    let h = parse_attr_f64(el, "height", 0.0);
    if w <= 0.0 || h <= 0.0 {
        return;
    }
    let raw = format!("M {x} {y} H {} V {} H {x} Z", x + w, y + h);
    let d = minify_path_d(&raw).unwrap_or(raw);
    rename_to_path(el);
    el.set_attr("d", d);
    for a in ["x", "y", "width", "height", "rx", "ry"] {
        el.remove_attr(a);
    }
}

fn parse_points(s: &str) -> Vec<(f64, f64)> {
    let mut p = Parser::new(s);
    let mut out = Vec::new();
    loop {
        match p.parse_number() {
            Some(x) => match p.parse_number() {
                Some(y) => out.push((x, y)),
                None => break,
            },
            None => break,
        }
    }
    out
}

/// Concatenate consecutive sibling paths that share paint attributes.
/// Returns whether any pair was merged.
pub fn merge_paths(doc: &mut Document) -> bool {
    merge_paths_in(&mut doc.nodes)
}

fn merge_paths_in(nodes: &mut Vec<Node>) -> bool {
    let mut merged = false;
    for node in nodes.iter_mut() {
        if let Node::Element(el) = node {
            merged |= merge_paths_in(&mut el.children);
        }
    }
    let mut i = 0;
    while i < nodes.len() {
        let mut j = i + 1;
        while j < nodes.len() && can_merge_pair(&nodes[i], &nodes[j]) {
            j += 1;
        }
        if j > i + 1 {
            if let Some(combined) = concat_run(&nodes[i..j]) {
                if let Node::Element(first) = &mut nodes[i] {
                    first.set_attr("d", combined);
                }
                nodes.drain(i + 1..j);
                merged = true;
            }
        }
        i += 1;
    }
    merged
}

fn concat_run(nodes: &[Node]) -> Option<String> {
    let mut total = 0usize;
    let mut ds = Vec::with_capacity(nodes.len());
    for n in nodes {
        let Node::Element(el) = n else {
            return None;
        };
        let d = el.attr("d")?;
        ds.push(d);
        total += d.len();
    }
    let mut expect_dest = 0usize;
    let mut part_bboxes = Vec::new();
    for d in &ds {
        let abs = to_abs(&parse_path(d)?);
        expect_dest += dest_point_count(&abs);
        if let Some(bb) = path_bbox(&abs) {
            part_bboxes.push(bb);
        }
    }
    let mut out = String::with_capacity(total);
    for (i, d) in ds.iter().enumerate() {
        if i == 0 {
            out.push_str(d);
        } else {
            append_subpath(&mut out, d)?;
        }
    }
    let combined = to_abs(&parse_path(&out)?);
    if dest_point_count(&combined) != expect_dest {
        return None;
    }
    if let (Some(union), Some(got)) = (union_bboxes(&part_bboxes), path_bbox(&combined)) {
        if !bbox_close(union, got) {
            return None;
        }
    }
    Some(out)
}

/// Append `b` as its own subpath.
///
/// A lone `d` that starts with `m` is relative to (0,0). After another
/// subpath the pen is elsewhere, so the first move must be absolute `M`.
/// Implicit pairs after `m` are relative `l` — rewriting only the letter
/// (`m139 56 15-16` → `M139 56 15-16`) turns them into absolute `L`.
fn append_subpath(out: &mut String, b: &str) -> Option<()> {
    let b = trim_leading_xml_s(b);
    if b.is_empty() {
        return Some(());
    }
    let first = b.as_bytes()[0];
    if first == b'M' {
        out.push_str(b);
        return Some(());
    }
    if first == b'm' {
        let abs = to_abs(&parse_path(b)?);
        let emitted = emit_path_new_subpath(&abs);
        if emit_has_digit_glue(&emitted) {
            return None;
        }
        let reparsed = to_abs(&parse_path(&emitted)?);
        if dest_point_count(&reparsed) != dest_point_count(&abs) {
            return None;
        }
        if !bbox_compatible(path_bbox(&abs), path_bbox(&reparsed)) {
            return None;
        }
        out.push_str(&emitted);
        return Some(());
    }
    None
}

fn trim_leading_xml_s(s: &str) -> &str {
    s.trim_start_matches([' ', '\t', '\n', '\r'])
}

fn can_merge_pair(a: &Node, b: &Node) -> bool {
    let (Node::Element(a), Node::Element(b)) = (a, b) else {
        return false;
    };
    if a.local_name() != "path" || b.local_name() != "path" {
        return false;
    }
    if has_element_child(a) || has_element_child(b) {
        return false;
    }
    if a.attr("id").is_some() || b.attr("id").is_some() {
        return false;
    }
    if a.attr("d").is_none() || b.attr("d").is_none() {
        return false;
    }
    if has_marker_attr(a) || has_marker_attr(b) {
        return false;
    }
    // Filled siblings (default fill is black; class/parent fill too) must
    // stay separate — merge + evenodd/nonzero punches holes in maps/arms.
    // Only explicit `fill="none"` strokes (hat ticks, icon strokes) merge.
    if !is_explicit_fill_none(a) || !is_explicit_fill_none(b) {
        return false;
    }
    paint_attrs_equal(a, b)
}

fn has_fill_spec(el: &Element) -> bool {
    el.attr("fill").is_some() || style_decl_value(el, "fill").is_some()
}

fn is_explicit_fill_none(el: &Element) -> bool {
    if let Some(v) = style_decl_value(el, "fill") {
        return v.eq_ignore_ascii_case("none");
    }
    el.attr("fill")
        .is_some_and(|v| v.trim().eq_ignore_ascii_case("none"))
}

fn style_decl_value(el: &Element, prop: &str) -> Option<String> {
    let style = el.attr("style")?;
    let mut found = None;
    for decl in style.split(';') {
        let Some((k, v)) = decl.split_once(':') else {
            continue;
        };
        if k.trim().eq_ignore_ascii_case(prop) {
            found = Some(v.trim().to_string());
        }
    }
    found
}

fn has_marker_attr(el: &Element) -> bool {
    el.attrs.iter().any(|(k, _)| {
        let l = k.rsplit_once(':').map(|(_, x)| x).unwrap_or(k);
        l.eq_ignore_ascii_case("marker")
            || l.eq_ignore_ascii_case("marker-start")
            || l.eq_ignore_ascii_case("marker-mid")
            || l.eq_ignore_ascii_case("marker-end")
    })
}

fn paint_attrs_equal(a: &Element, b: &Element) -> bool {
    let fa = filtered_attrs(a);
    let fb = filtered_attrs(b);
    fa == fb
}

fn filtered_attrs(el: &Element) -> Vec<(&str, &str)> {
    let mut v: Vec<(&str, &str)> = el
        .attrs
        .iter()
        .filter(|(k, _)| k != "d")
        .map(|(k, val)| (k.as_str(), val.as_str()))
        .collect();
    v.sort_by(|x, y| x.0.cmp(y.0));
    v
}

fn dest_point_count(atoms: &[Abs]) -> usize {
    atoms.iter().filter(|a| !matches!(a, Abs::Close)).count()
}

fn round_prec(n: f64, prec: i32) -> f64 {
    if !n.is_finite() {
        return 0.0;
    }
    let f = 10f64.powi(prec);
    (n * f).round() / f
}

fn round_abs(atoms: &mut [Abs], prec: i32) {
    let r = |n: f64| round_prec(n, prec);
    for a in atoms {
        match a {
            Abs::Move { x, y } | Abs::Line { x, y } | Abs::SmoothQ { x, y } => {
                *x = r(*x);
                *y = r(*y);
            }
            Abs::H { x } => *x = r(*x),
            Abs::V { y } => *y = r(*y),
            Abs::Cubic {
                x1,
                y1,
                x2,
                y2,
                x,
                y,
            } => {
                *x1 = r(*x1);
                *y1 = r(*y1);
                *x2 = r(*x2);
                *y2 = r(*y2);
                *x = r(*x);
                *y = r(*y);
            }
            Abs::SmoothC { x2, y2, x, y } => {
                *x2 = r(*x2);
                *y2 = r(*y2);
                *x = r(*x);
                *y = r(*y);
            }
            Abs::Quad { x1, y1, x, y } => {
                *x1 = r(*x1);
                *y1 = r(*y1);
                *x = r(*x);
                *y = r(*y);
            }
            Abs::Arc {
                rx, ry, rot, x, y, ..
            } => {
                *rx = r(*rx);
                *ry = r(*ry);
                *rot = r(*rot);
                *x = r(*x);
                *y = r(*y);
            }
            Abs::Close => {}
        }
    }
}

fn bbox_compatible(a: Option<(f64, f64, f64, f64)>, b: Option<(f64, f64, f64, f64)>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => bbox_close(a, b),
        _ => false,
    }
}

fn union_bboxes(boxes: &[(f64, f64, f64, f64)]) -> Option<(f64, f64, f64, f64)> {
    let first = boxes.first()?;
    let mut u = *first;
    for b in boxes.iter().skip(1) {
        u.0 = u.0.min(b.0);
        u.1 = u.1.min(b.1);
        u.2 = u.2.max(b.2);
        u.3 = u.3.max(b.3);
    }
    Some(u)
}

fn path_bbox(atoms: &[Abs]) -> Option<(f64, f64, f64, f64)> {
    let mut cx = 0.0;
    let mut cy = 0.0;
    let mut minx = f64::INFINITY;
    let mut miny = f64::INFINITY;
    let mut maxx = f64::NEG_INFINITY;
    let mut maxy = f64::NEG_INFINITY;
    let mut any = false;
    for atom in atoms {
        match *atom {
            Abs::Move { x, y }
            | Abs::Line { x, y }
            | Abs::Cubic { x, y, .. }
            | Abs::SmoothC { x, y, .. }
            | Abs::Quad { x, y, .. }
            | Abs::SmoothQ { x, y }
            | Abs::Arc { x, y, .. } => {
                cx = x;
                cy = y;
            }
            Abs::H { x } => cx = x,
            Abs::V { y } => cy = y,
            Abs::Close => continue,
        }
        any = true;
        minx = minx.min(cx);
        miny = miny.min(cy);
        maxx = maxx.max(cx);
        maxy = maxy.max(cy);
    }
    any.then_some((minx, miny, maxx, maxy))
}

fn bbox_close(a: (f64, f64, f64, f64), b: (f64, f64, f64, f64)) -> bool {
    let scale = (a.2 - a.0)
        .abs()
        .max((a.3 - a.1).abs())
        .max((b.2 - b.0).abs())
        .max((b.3 - b.1).abs())
        .max(1.0);
    let tol = (scale * 0.01).max(0.05);
    (a.0 - b.0).abs() <= tol
        && (a.1 - b.1).abs() <= tol
        && (a.2 - b.2).abs() <= tol
        && (a.3 - b.3).abs() <= tol
}

/// Collapse `translate` / `scale` / `matrix` to a shorter equivalent.
/// Skips SMIL-animated transforms. Does **not** bake into path `d`.
pub fn convert_transform(doc: &mut Document) {
    convert_transform_with(doc, None);
}

/// Like [`convert_transform`], with optional component rounding.
pub fn convert_transform_with(doc: &mut Document, precision: Option<u8>) {
    let locked = collect_smil_locked_transform_ids(&doc.nodes);
    convert_transform_in(&mut doc.nodes, &locked, precision);
}

/// Minify one transform list. `Some("")` is identity (drop the attr).
/// `None` if the list cannot be parsed safely.
pub fn minify_transform(raw: &str) -> Option<String> {
    minify_transform_with(raw, None)
}

/// Like [`minify_transform`], with optional component rounding.
pub fn minify_transform_with(raw: &str, precision: Option<u8>) -> Option<String> {
    let parsed = parse_transform_list(raw)?;
    let mut list: Vec<Tf> = parsed
        .into_iter()
        .filter(|tf| !tf_is_identity(tf, precision))
        .collect();
    collapse_adjacent_matrices(&mut list);
    Some(emit_transform_list(&list, precision))
}

fn convert_transform_in(nodes: &mut [Node], locked: &HashSet<String>, precision: Option<u8>) {
    for node in nodes.iter_mut() {
        let Node::Element(el) = node else {
            continue;
        };
        if is_smil_tag(el.local_name()) {
            convert_transform_in(&mut el.children, locked, precision);
            continue;
        }
        let id_locked = el.attr("id").is_some_and(|id| locked.contains(id));
        if !id_locked && !element_has_transform_smil_child(el) {
            let keys: Vec<String> = el
                .attrs
                .iter()
                .filter(|(k, _)| is_transform_attr_name(k))
                .map(|(k, _)| k.clone())
                .collect();
            for k in keys {
                let Some(v) = el.attr(&k).map(str::to_string) else {
                    continue;
                };
                let Some(next) = minify_transform_with(&v, precision) else {
                    continue;
                };
                if next.is_empty() {
                    el.remove_attr(&k);
                } else if next.len() < v.len() {
                    el.set_attr(&k, next);
                }
            }
        }
        convert_transform_in(&mut el.children, locked, precision);
    }
}

fn is_transform_attr_name(name: &str) -> bool {
    let local = name.rsplit_once(':').map(|(_, l)| l).unwrap_or(name);
    local.eq_ignore_ascii_case("transform")
        || local.eq_ignore_ascii_case("gradientTransform")
        || local.eq_ignore_ascii_case("patternTransform")
}

fn collect_smil_locked_transform_ids(nodes: &[Node]) -> HashSet<String> {
    let mut out = HashSet::new();
    walk_smil_transform_locks(nodes, &mut out);
    out
}

fn walk_smil_transform_locks(nodes: &[Node], out: &mut HashSet<String>) {
    for node in nodes {
        let Node::Element(el) = node else {
            continue;
        };
        if locks_transform_target(el) {
            for key in ["href", "xlink:href"] {
                if let Some(v) = el.attr(key) {
                    if let Some(id) = v.strip_prefix('#') {
                        if !id.is_empty() {
                            out.insert(id.to_string());
                        }
                    }
                }
            }
        }
        walk_smil_transform_locks(&el.children, out);
    }
}

fn element_has_transform_smil_child(el: &Element) -> bool {
    el.children.iter().any(|n| match n {
        Node::Element(child) => locks_transform_target(child),
        _ => false,
    })
}

fn locks_transform_target(el: &Element) -> bool {
    let local = el.local_name();
    if local.eq_ignore_ascii_case("animatetransform") || local.eq_ignore_ascii_case("animatemotion")
    {
        return true;
    }
    is_smil_tag(local) && attr_is_transform_name(el.attr("attributeName"))
}

fn attr_is_transform_name(v: Option<&str>) -> bool {
    v.is_some_and(|s| {
        let s = s.trim();
        s.eq_ignore_ascii_case("transform")
            || s.eq_ignore_ascii_case("gradientTransform")
            || s.eq_ignore_ascii_case("patternTransform")
    })
}

#[derive(Clone, Copy, Debug)]
struct M {
    a: f64,
    b: f64,
    c: f64,
    d: f64,
    e: f64,
    f: f64,
}

impl M {
    fn translate(x: f64, y: f64) -> Self {
        Self {
            a: 1.0,
            b: 0.0,
            c: 0.0,
            d: 1.0,
            e: x,
            f: y,
        }
    }

    fn scale(x: f64, y: f64) -> Self {
        Self {
            a: x,
            b: 0.0,
            c: 0.0,
            d: y,
            e: 0.0,
            f: 0.0,
        }
    }

    fn mul(self, o: Self) -> Self {
        Self {
            a: self.a * o.a + self.c * o.b,
            b: self.b * o.a + self.d * o.b,
            c: self.a * o.c + self.c * o.d,
            d: self.b * o.c + self.d * o.d,
            e: self.a * o.e + self.c * o.f + self.e,
            f: self.b * o.e + self.d * o.f + self.f,
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum Tf {
    Matrix(M),
    Rotate { a: f64, cxy: Option<(f64, f64)> },
    SkewX(f64),
    SkewY(f64),
}

fn parse_transform_list(s: &str) -> Option<Vec<Tf>> {
    let t = s.trim();
    if t.is_empty() || t.eq_ignore_ascii_case("none") {
        return Some(Vec::new());
    }
    let b = t.as_bytes();
    let mut i = 0usize;
    let mut out = Vec::new();
    loop {
        skip_tf_sep(b, &mut i);
        if i >= b.len() {
            break;
        }
        let start = i;
        while i < b.len() && b[i].is_ascii_alphabetic() {
            i += 1;
        }
        if start == i {
            return None;
        }
        let name = t[start..i].to_ascii_lowercase();
        skip_tf_wsp(b, &mut i);
        if i >= b.len() || b[i] != b'(' {
            return None;
        }
        i += 1;
        let mut nums = Vec::new();
        loop {
            skip_tf_sep(b, &mut i);
            if i >= b.len() {
                return None;
            }
            if b[i] == b')' {
                i += 1;
                break;
            }
            if !is_tf_num_start(b[i]) {
                return None;
            }
            nums.push(parse_tf_number(b, &mut i)?);
            if i < b.len() && b[i].is_ascii_alphabetic() {
                return None;
            }
        }
        out.push(tf_from_fn(&name, &nums)?);
    }
    Some(out)
}

fn skip_tf_wsp(b: &[u8], i: &mut usize) {
    while *i < b.len() && matches!(b[*i], b' ' | b'\t' | b'\n' | b'\r') {
        *i += 1;
    }
}

fn skip_tf_sep(b: &[u8], i: &mut usize) {
    while *i < b.len() && matches!(b[*i], b' ' | b'\t' | b'\n' | b'\r' | b',') {
        *i += 1;
    }
}

fn is_tf_num_start(c: u8) -> bool {
    c.is_ascii_digit() || matches!(c, b'+' | b'-' | b'.')
}

fn parse_tf_number(b: &[u8], i: &mut usize) -> Option<f64> {
    let start = *i;
    if *i < b.len() && matches!(b[*i], b'+' | b'-') {
        *i += 1;
    }
    let mut saw_digit = false;
    while *i < b.len() && b[*i].is_ascii_digit() {
        saw_digit = true;
        *i += 1;
    }
    if *i < b.len() && b[*i] == b'.' {
        *i += 1;
        while *i < b.len() && b[*i].is_ascii_digit() {
            saw_digit = true;
            *i += 1;
        }
    }
    if *i < b.len() && matches!(b[*i], b'e' | b'E') {
        let e = *i;
        *i += 1;
        if *i < b.len() && matches!(b[*i], b'+' | b'-') {
            *i += 1;
        }
        let exp = *i;
        while *i < b.len() && b[*i].is_ascii_digit() {
            *i += 1;
        }
        if *i == exp {
            *i = e;
        }
    }
    if !saw_digit || start == *i {
        return None;
    }
    let s = std::str::from_utf8(&b[start..*i]).ok()?;
    let n: f64 = s.parse().ok()?;
    n.is_finite().then_some(n)
}

fn tf_from_fn(name: &str, n: &[f64]) -> Option<Tf> {
    match name {
        "matrix" if n.len() == 6 => Some(Tf::Matrix(M {
            a: n[0],
            b: n[1],
            c: n[2],
            d: n[3],
            e: n[4],
            f: n[5],
        })),
        "translate" if n.len() == 1 => Some(Tf::Matrix(M::translate(n[0], 0.0))),
        "translate" if n.len() == 2 => Some(Tf::Matrix(M::translate(n[0], n[1]))),
        "scale" if n.len() == 1 => Some(Tf::Matrix(M::scale(n[0], n[0]))),
        "scale" if n.len() == 2 => Some(Tf::Matrix(M::scale(n[0], n[1]))),
        "rotate" if n.len() == 1 => Some(Tf::Rotate { a: n[0], cxy: None }),
        "rotate" if n.len() == 3 => Some(Tf::Rotate {
            a: n[0],
            cxy: Some((n[1], n[2])),
        }),
        "skewx" if n.len() == 1 => Some(Tf::SkewX(n[0])),
        "skewy" if n.len() == 1 => Some(Tf::SkewY(n[0])),
        _ => None,
    }
}

fn collapse_adjacent_matrices(list: &mut Vec<Tf>) {
    let mut i = 0;
    while i + 1 < list.len() {
        match (list[i], list[i + 1]) {
            (Tf::Matrix(a), Tf::Matrix(b)) => {
                list[i] = Tf::Matrix(a.mul(b));
                list.remove(i + 1);
            }
            _ => i += 1,
        }
    }
}

fn tf_is_identity(tf: &Tf, precision: Option<u8>) -> bool {
    match *tf {
        Tf::Matrix(m) => matrix_is_identity(&m, precision),
        Tf::Rotate { a, .. } => tf_near0(tf_snap(a, precision)),
        Tf::SkewX(a) | Tf::SkewY(a) => tf_near0(tf_snap(a, precision)),
    }
}

fn matrix_is_identity(m: &M, precision: Option<u8>) -> bool {
    tf_near(tf_snap(m.a, precision), 1.0)
        && tf_near0(tf_snap(m.b, precision))
        && tf_near0(tf_snap(m.c, precision))
        && tf_near(tf_snap(m.d, precision), 1.0)
        && tf_near0(tf_snap(m.e, precision))
        && tf_near0(tf_snap(m.f, precision))
}

fn emit_transform_list(list: &[Tf], precision: Option<u8>) -> String {
    let mut out = String::new();
    for tf in list {
        out.push_str(&emit_tf(tf, precision));
    }
    out
}

fn emit_tf(tf: &Tf, precision: Option<u8>) -> String {
    match *tf {
        Tf::Matrix(m) => emit_matrix_shortest(&m, precision),
        Tf::Rotate { a, cxy } => {
            let a = tf_snap(a, precision);
            if tf_near0(a) {
                return String::new();
            }
            match cxy {
                Some((cx, cy)) => {
                    let cx = tf_snap(cx, precision);
                    let cy = tf_snap(cy, precision);
                    if tf_near0(cx) && tf_near0(cy) {
                        emit_call("rotate", &[a], precision)
                    } else {
                        emit_call("rotate", &[a, cx, cy], precision)
                    }
                }
                None => emit_call("rotate", &[a], precision),
            }
        }
        Tf::SkewX(a) => {
            let a = tf_snap(a, precision);
            if tf_near0(a) {
                String::new()
            } else {
                emit_call("skewX", &[a], precision)
            }
        }
        Tf::SkewY(a) => {
            let a = tf_snap(a, precision);
            if tf_near0(a) {
                String::new()
            } else {
                emit_call("skewY", &[a], precision)
            }
        }
    }
}

fn emit_matrix_shortest(m: &M, precision: Option<u8>) -> String {
    let a = tf_snap(m.a, precision);
    let b = tf_snap(m.b, precision);
    let c = tf_snap(m.c, precision);
    let d = tf_snap(m.d, precision);
    let e = tf_snap(m.e, precision);
    let f = tf_snap(m.f, precision);
    if tf_near(a, 1.0)
        && tf_near0(b)
        && tf_near0(c)
        && tf_near(d, 1.0)
        && tf_near0(e)
        && tf_near0(f)
    {
        return String::new();
    }
    let mx = emit_call("matrix", &[a, b, c, d, e, f], precision);
    if !tf_near0(b) || !tf_near0(c) {
        return mx;
    }
    let mut best = mx;
    if tf_near(a, 1.0) && tf_near(d, 1.0) {
        keep_shorter(&mut best, emit_translate(e, f, precision));
    } else if tf_near0(e) && tf_near0(f) {
        keep_shorter(&mut best, emit_scale(a, d, precision));
    } else {
        let ts = format!(
            "{}{}",
            emit_translate(e, f, precision),
            emit_scale(a, d, precision)
        );
        keep_shorter(&mut best, ts);
        if !tf_near0(a) && !tf_near0(d) {
            let st = format!(
                "{}{}",
                emit_scale(a, d, precision),
                emit_translate(e / a, f / d, precision)
            );
            keep_shorter(&mut best, st);
        }
    }
    best
}

fn emit_translate(x: f64, y: f64, precision: Option<u8>) -> String {
    if tf_near0(x) && tf_near0(y) {
        return String::new();
    }
    if tf_near0(y) {
        emit_call("translate", &[x], precision)
    } else {
        emit_call("translate", &[x, y], precision)
    }
}

fn emit_scale(x: f64, y: f64, precision: Option<u8>) -> String {
    if tf_near(x, 1.0) && tf_near(y, 1.0) {
        return String::new();
    }
    if tf_near(x, y) {
        emit_call("scale", &[x], precision)
    } else {
        emit_call("scale", &[x, y], precision)
    }
}

fn keep_shorter(best: &mut String, cand: String) {
    if cand.is_empty() {
        return;
    }
    if cand.len() < best.len() {
        *best = cand;
    }
}

fn emit_call(name: &str, args: &[f64], precision: Option<u8>) -> String {
    let mut s = String::from(name);
    s.push('(');
    for (i, n) in args.iter().enumerate() {
        let tok = emit_tf_num(*n, precision);
        if i > 0 && tf_needs_sep(&s, &tok) {
            s.push(' ');
        }
        s.push_str(&tok);
    }
    s.push(')');
    s
}

fn emit_tf_num(n: f64, precision: Option<u8>) -> String {
    let n = tf_snap(n, precision);
    let n = if n == 0.0 { 0.0 } else { n };
    minify_number_lexeme(&fmt_f64(n))
}

fn tf_needs_sep(out: &str, next: &str) -> bool {
    let Some(prev) = out.chars().next_back() else {
        return false;
    };
    if next.starts_with('-') {
        return false;
    }
    if next.starts_with('.') {
        return !prev_token_has_dot(out);
    }
    prev.is_ascii_digit() || prev == '.'
}

fn tf_snap(n: f64, precision: Option<u8>) -> f64 {
    if !n.is_finite() {
        return 0.0;
    }
    let n = if n == 0.0 { 0.0 } else { n };
    match precision {
        Some(p) => {
            let f = 10f64.powi(i32::from(p));
            (n * f).round() / f
        }
        None => {
            if n.abs() < 1e-8 {
                0.0
            } else {
                n
            }
        }
    }
}

fn tf_near0(n: f64) -> bool {
    n.abs() < 1e-8
}

fn tf_near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-8
}

fn collect_smil_locked_d_ids(nodes: &[Node]) -> HashSet<String> {
    let mut out = HashSet::new();
    walk_smil_d_locks(nodes, &mut out);
    out
}

fn walk_smil_d_locks(nodes: &[Node], out: &mut HashSet<String>) {
    for node in nodes {
        let Node::Element(el) = node else {
            continue;
        };
        if is_smil_tag(el.local_name()) && attr_is_d(el.attr("attributeName")) {
            for key in ["href", "xlink:href"] {
                if let Some(v) = el.attr(key) {
                    if let Some(id) = v.strip_prefix('#') {
                        if !id.is_empty() {
                            out.insert(id.to_string());
                        }
                    }
                }
            }
        }
        walk_smil_d_locks(&el.children, out);
    }
}

fn element_has_smil_d_child(el: &Element) -> bool {
    el.children.iter().any(|n| match n {
        Node::Element(child) => {
            is_smil_tag(child.local_name()) && attr_is_d(child.attr("attributeName"))
        }
        _ => false,
    })
}

fn is_smil_tag(local: &str) -> bool {
    local.eq_ignore_ascii_case("animate")
        || local.eq_ignore_ascii_case("animatetransform")
        || local.eq_ignore_ascii_case("animatemotion")
        || local.eq_ignore_ascii_case("animatecolor")
        || local.eq_ignore_ascii_case("set")
}

fn attr_is_d(v: Option<&str>) -> bool {
    v.is_some_and(|s| s.eq_ignore_ascii_case("d"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{Document, Element, Node};

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

    fn doc_svg(kids: Vec<Node>) -> Document {
        Document {
            nodes: vec![node(
                "svg",
                &[("xmlns", "http://www.w3.org/2000/svg")],
                kids,
            )],
        }
    }

    fn first_attr(doc: &Document, local: &str, attr: &str) -> Option<String> {
        fn walk(nodes: &[Node], local: &str, attr: &str, found: &mut Option<String>) {
            for node in nodes {
                let Node::Element(el) = node else {
                    continue;
                };
                if found.is_none() && el.local_name() == local {
                    *found = el.attr(attr).map(str::to_string);
                }
                walk(&el.children, local, attr, found);
            }
        }
        let mut found = None;
        walk(&doc.nodes, local, attr, &mut found);
        found
    }

    fn first_el<'a>(doc: &'a Document, local: &str) -> Option<&'a Element> {
        fn walk<'a>(nodes: &'a [Node], local: &str, found: &mut Option<&'a Element>) {
            for node in nodes {
                let Node::Element(el) = node else {
                    continue;
                };
                if found.is_none() && el.local_name() == local {
                    *found = Some(el);
                }
                walk(&el.children, local, found);
            }
        }
        let mut found = None;
        walk(&doc.nodes, local, &mut found);
        found
    }

    #[test]
    fn trim_collapses_xml_s_and_comma_space() {
        assert_eq!(trim_path_d_whitespace("  M  10   10  "), "M10 10");
        assert_eq!(trim_path_d_whitespace("M 0, 0 L 1,  2"), "M0,0 L1,2");
        assert_eq!(trim_path_d_whitespace("M\n10\t10\r\n"), "M10 10");
        assert_eq!(trim_path_d_whitespace("M10 10"), "M10 10");
        assert_eq!(trim_path_d_whitespace(""), "");
        assert_eq!(trim_path_d_whitespace("   "), "");
    }

    #[test]
    fn minify_trims_floats_and_leading_zeros() {
        let out = minify_path_d("M 46.0000,8.0000 L 20.2500,48.0000").unwrap();
        assert!(!out.contains("0000"), "{out}");
        assert!(out.contains("20.25") || out.contains("-25.75"), "{out}");
        assert_eq!(minify_number_lexeme("0.18"), ".18");
        assert_eq!(minify_number_lexeme("10.0"), "10");
        assert_eq!(minify_number_lexeme("-0.50"), "-.5");
    }

    #[test]
    fn minify_prefers_hv_and_relative_and_z() {
        let out = minify_path_d("M 0 0 L 10 0 L 10 10 L 0 10 Z").unwrap();
        assert!(
            out.contains('h') || out.contains('H') || out.contains('v') || out.contains('V'),
            "{out}"
        );
        assert!(out.ends_with('z'), "{out}");
        assert!(!out.contains('Z'), "{out}");
    }

    #[test]
    fn minify_omits_space_before_minus_keeps_m12_dot() {
        let out = minify_path_d("M12 .297c-6.63 0-12 5.373-12 12").unwrap();
        assert!(!out.contains("M12.297"), "{out}");
        assert!(out.contains(".297"), "{out}");
        assert!(!out.contains(" -"), "{out}");
    }

    #[test]
    fn minify_smooth_s_from_reflected_cubic() {
        // C whose first control is the current point → S
        let out = minify_path_d("M0 0 C 0 0 10 10 20 0").unwrap();
        assert!(out.contains('s') || out.contains('S'), "{out}");
    }

    #[test]
    fn stub_rewrites_path_and_glyph_skips_polygon() {
        let mut doc = doc_svg(vec![
            node(
                "g",
                &[],
                vec![node("path", &[("d", "  M  0   0  L  10,  10  ")], vec![])],
            ),
            node("glyph", &[("d", " M 1  2 ")], vec![]),
            node("polygon", &[("points", " 0, 0  10, 10 ")], vec![]),
            node("rect", &[("d", "  M  1  1  ")], vec![]),
        ]);
        convert_path_data(&mut doc);
        let d = first_attr(&doc, "path", "d").unwrap();
        assert!(d.starts_with('M') || d.starts_with('m'), "{d}");
        assert!(!d.contains("  "), "{d}");
        let glyph = first_attr(&doc, "glyph", "d").unwrap();
        assert!(glyph == "M1 2" || glyph == "m1 2", "{glyph}");
        assert_eq!(
            first_attr(&doc, "polygon", "points").as_deref(),
            Some(" 0, 0  10, 10 ")
        );
        assert_eq!(
            first_attr(&doc, "rect", "d").as_deref(),
            Some("  M  1  1  ")
        );
    }

    #[test]
    fn smil_animated_d_is_not_rewritten() {
        let mut doc = doc_svg(vec![node(
            "path",
            &[("id", "mouth"), ("d", "M 156.0000 106.0000 q 10 5 18 0")],
            vec![node(
                "animate",
                &[("attributeName", "d"), ("values", "M 156 106 q 10 5 18 0")],
                vec![],
            )],
        )]);
        convert_path_data(&mut doc);
        assert_eq!(
            first_attr(&doc, "path", "d").as_deref(),
            Some("M 156.0000 106.0000 q 10 5 18 0")
        );
    }

    #[test]
    fn convert_line_polyline_rect_skips_rounded_and_circle() {
        let mut doc = doc_svg(vec![
            node(
                "line",
                &[
                    ("x1", "0"),
                    ("y1", "0"),
                    ("x2", "10"),
                    ("y2", "0"),
                    ("stroke", "red"),
                ],
                vec![],
            ),
            node(
                "polyline",
                &[("points", "0,0 10,0 10,10"), ("fill", "none")],
                vec![],
            ),
            node(
                "rect",
                &[
                    ("x", "1"),
                    ("y", "2"),
                    ("width", "8"),
                    ("height", "4"),
                    ("fill", "#fff"),
                ],
                vec![],
            ),
            node(
                "rect",
                &[
                    ("x", "0"),
                    ("y", "0"),
                    ("width", "8"),
                    ("height", "4"),
                    ("rx", "2"),
                ],
                vec![],
            ),
            node("circle", &[("cx", "4"), ("cy", "4"), ("r", "3")], vec![]),
        ]);
        convert_shape_to_path(&mut doc);
        let paths = {
            fn count(nodes: &[Node]) -> usize {
                nodes
                    .iter()
                    .map(|n| match n {
                        Node::Element(el) => {
                            (el.local_name() == "path") as usize + count(&el.children)
                        }
                        _ => 0,
                    })
                    .sum()
            }
            count(&doc.nodes)
        };
        assert_eq!(paths, 3, "line + polyline + axis rect");
        assert!(first_el(&doc, "circle").is_some());
        let rounded = first_el(&doc, "rect").unwrap();
        assert_eq!(rounded.attr("rx"), Some("2"));
        assert!(first_attr(&doc, "path", "d").is_some());
        let line_path = first_el(&doc, "path").unwrap();
        assert_eq!(
            line_path.attr("fill"),
            Some("none"),
            "line→path must not pick up default black fill: {:?}",
            line_path.attrs
        );
    }

    #[test]
    fn merge_paths_same_paint_skips_id_and_markers() {
        let mut doc = doc_svg(vec![
            node(
                "path",
                &[("d", "M0 0h4"), ("stroke", "red"), ("fill", "none")],
                vec![],
            ),
            node(
                "path",
                &[("d", "M0 2h4"), ("stroke", "red"), ("fill", "none")],
                vec![],
            ),
            node(
                "path",
                &[
                    ("id", "keep"),
                    ("d", "M0 4h4"),
                    ("stroke", "red"),
                    ("fill", "none"),
                ],
                vec![],
            ),
            node(
                "path",
                &[
                    ("d", "M0 6h4"),
                    ("stroke", "red"),
                    ("fill", "none"),
                    ("marker-end", "url(#m)"),
                ],
                vec![],
            ),
        ]);
        merge_paths(&mut doc);
        let svg = match &doc.nodes[0] {
            Node::Element(el) => el,
            _ => panic!("svg"),
        };
        let paths: Vec<&Element> = svg
            .children
            .iter()
            .filter_map(|n| match n {
                Node::Element(el) if el.local_name() == "path" => Some(el),
                _ => None,
            })
            .collect();
        assert_eq!(paths.len(), 3, "merged first pair only");
        let d = paths[0].attr("d").unwrap();
        assert!(d.contains('M') || d.contains('m'), "{d}");
        assert!(d.len() > 5, "{d}");
        assert_eq!(paths[1].attr("id"), Some("keep"));
        assert!(paths[2].attr("marker-end").is_some());
    }

    #[test]
    fn merge_paths_keeps_relative_implicit_lineto_after_first_move() {
        // Capybara orange-hat ticks: three stroke lines after convertShapeToPath.
        // Naive `m` → `M` turns `m139 56 15-16` into L(15,-16).
        let mut doc = doc_svg(vec![
            node(
                "path",
                &[
                    ("d", "m139 56-15-14"),
                    ("stroke", "#ffb040"),
                    ("fill", "none"),
                ],
                vec![],
            ),
            node(
                "path",
                &[("d", "M139 56V36"), ("stroke", "#ffb040"), ("fill", "none")],
                vec![],
            ),
            node(
                "path",
                &[
                    ("d", "m139 56 15-16"),
                    ("stroke", "#ffb040"),
                    ("fill", "none"),
                ],
                vec![],
            ),
        ]);
        merge_paths(&mut doc);
        let d = first_attr(&doc, "path", "d").unwrap();
        assert!(
            !d.contains("M139 56 15-16") && !d.contains("M139 56 15 -16"),
            "implicit lineto became absolute (15,-16): {d}"
        );
        let abs = to_abs(&parse_path(&d).expect("merged d"));
        let pts: Vec<(f64, f64)> = abs
            .iter()
            .filter_map(|a| match *a {
                Abs::Move { x, y } | Abs::Line { x, y } => Some((x, y)),
                Abs::V { y } => Some((139.0, y)),
                Abs::H { x } => Some((x, 56.0)),
                _ => None,
            })
            .collect();
        assert!(
            pts.iter()
                .any(|(x, y)| (x - 154.0).abs() < 0.05 && (y - 40.0).abs() < 0.05),
            "right hat tick must end at (154,40): {d} pts={pts:?}"
        );
        assert!(
            pts.iter()
                .any(|(x, y)| (x - 124.0).abs() < 0.05 && (y - 42.0).abs() < 0.05),
            "left hat tick must end at (124,42): {d} pts={pts:?}"
        );
    }

    #[test]
    fn merge_paths_refuses_filled_siblings() {
        let mut doc = doc_svg(vec![
            node("path", &[("d", "M0 0h4v4H0z"), ("fill", "red")], vec![]),
            node("path", &[("d", "M8 0h4v4H8z"), ("fill", "red")], vec![]),
            node("path", &[("d", "M0 0h1"), ("class", "land")], vec![]),
            node("path", &[("d", "M2 0h1"), ("class", "land")], vec![]),
        ]);
        merge_paths(&mut doc);
        let svg = match &doc.nodes[0] {
            Node::Element(el) => el,
            _ => panic!("svg"),
        };
        let n = svg
            .children
            .iter()
            .filter(|n| matches!(n, Node::Element(el) if el.local_name() == "path"))
            .count();
        assert_eq!(n, 4, "filled / classed land paths must stay separate");
    }

    #[test]
    fn never_grows_already_compact_d() {
        let compact = "M12 .297c-6.63 0-12 5.373-12 12";
        let mut doc = doc_svg(vec![node("path", &[("d", compact)], vec![])]);
        convert_path_data(&mut doc);
        let d = first_attr(&doc, "path", "d").unwrap();
        assert!(
            d.len() <= compact.len(),
            "grew {} → {}: {d}",
            compact.len(),
            d.len()
        );
    }

    #[test]
    fn plugin_name_is_svgo_camel_case() {
        assert!(PATH_PLUGIN_NAMES.contains(&"convertPathData"));
        assert!(PATH_PLUGIN_NAMES.contains(&"convertShapeToPath"));
        assert!(PATH_PLUGIN_NAMES.contains(&"convertTransform"));
        assert!(PATH_PLUGIN_NAMES.contains(&"mergePaths"));
    }

    #[test]
    fn digit_glue_flags_ocean_014_not_spaced_zero() {
        assert!(emit_has_digit_glue("6.408 014.1912"));
        assert!(!emit_has_digit_glue("6.408 0 14.1912"));
        assert!(!emit_has_digit_glue("A10 10 0 00 20 20"));
        assert!(emit_has_digit_glue("014.1912"));
    }

    #[test]
    fn minify_does_not_glue_zero_onto_next_implicit_cubic() {
        // Extracted from corpus/large/blankmap-world-flattened.svg ocean (top edge).
        // Implicit `c` after a cubic that ends in `0` used to emit `6.408 014.1912`.
        let d = "M -26.0058,61.9866 C -23.8711,61.9866 -21.7344,61.9866 -19.5978,61.9866 C -5.4066,61.9866 8.7864,61.9866 22.9794,61.9866";
        let out = minify_path_d(d).expect("ocean snippet must minify");
        assert!(
            !out.contains("014.1912") && !out.contains("6.4080"),
            "glued 0 onto the next number (world-map triangle): {out}"
        );
        assert!(
            out.contains("6.408 0 14.1912") || out.contains("6.408 0 14"),
            "expected an explicit separator after the trailing 0: {out}"
        );
        let reparsed = parse_path(&out).expect("emitted path must reparse");
        assert_eq!(
            dest_point_count(&to_abs(&reparsed)),
            dest_point_count(&to_abs(&parse_path(d).unwrap())),
            "dest count changed: {out}"
        );
    }

    #[test]
    fn minify_implicit_lineto_does_not_glue_after_move() {
        let out = minify_path_d("M 10 20 L 30 40 L 50 60").unwrap();
        assert!(!out.contains("2030"), "M/L glue: {out}");
        assert!(!out.contains("4050"), "L/L glue: {out}");
    }

    #[test]
    fn minify_precision_2_vs_3_keeps_dest_count() {
        let d = "M 1.23456 2.34567 C 3.45678 4.56789 5.67891 6.78901 7.89012 8.90123 C 1.11111 2.22222 3.33333 4.44444 5.55555 6.66666";
        let orig = dest_point_count(&to_abs(&parse_path(d).unwrap()));
        let a = minify_path_d_with(d, Some(2)).expect("prec 2");
        let b = minify_path_d_with(d, Some(3)).expect("prec 3");
        assert!(
            !a.contains("014.1912") && !b.contains("014.1912"),
            "{a} / {b}"
        );
        assert_eq!(
            dest_point_count(&to_abs(&parse_path(&a).unwrap())),
            orig,
            "prec2 dests: {a}"
        );
        assert_eq!(
            dest_point_count(&to_abs(&parse_path(&b).unwrap())),
            orig,
            "prec3 dests: {b}"
        );
        assert!(a.len() <= b.len(), "prec2 {a} longer than prec3 {b}");
    }

    #[test]
    fn minify_ocean_rect_cubics_keep_four_sides() {
        // Tiny stand-in for the world-map ocean: a rectangle drawn as four
        // axis-aligned cubics. A smashed `d` becomes a 2–3 point triangle.
        let d = "M 100,200 C 80,200 60,200 40,200 C 40,160 40,120 40,80 C 60,80 80,80 100,80 C 100,120 100,160 100,200";
        let out = minify_path_d(d).expect("rect cubics must minify");
        let orig = to_abs(&parse_path(d).unwrap());
        let got = to_abs(&parse_path(&out).expect("reparse"));
        assert!(
            dest_point_count(&got) >= 4,
            "collapsed to a triangle: {out}"
        );
        assert!(
            bbox_compatible(path_bbox(&orig), path_bbox(&got)),
            "bbox/dest-count guard failed: {out}"
        );
        let (x0, y0, x1, y1) = path_bbox(&got).unwrap();
        assert!((x0 - 40.0).abs() < 0.5 && (x1 - 100.0).abs() < 0.5, "{out}");
        assert!((y0 - 80.0).abs() < 0.5 && (y1 - 200.0).abs() < 0.5, "{out}");
    }

    #[test]
    fn minify_transform_shortens_translate_scale_matrix() {
        assert_eq!(
            minify_transform("translate(10, 0)").as_deref(),
            Some("translate(10)")
        );
        assert_eq!(minify_transform("scale(2, 2)").as_deref(), Some("scale(2)"));
        assert_eq!(
            minify_transform("matrix(1,0,0,1,10,20)").as_deref(),
            Some("translate(10 20)")
        );
        assert_eq!(
            minify_transform("matrix(2 0 0 2 0 0)").as_deref(),
            Some("scale(2)")
        );
        assert_eq!(minify_transform("matrix(1 0 0 1 0 0)").as_deref(), Some(""));
        assert_eq!(
            minify_transform("translate(10) translate(5)").as_deref(),
            Some("translate(15)")
        );
        assert_eq!(
            minify_transform("translate(10.000, 20.000)").as_deref(),
            Some("translate(10 20)")
        );
        assert_eq!(minify_transform("none").as_deref(), Some(""));
        assert!(minify_transform("translate(10px)").is_none());
        let mixed = minify_transform("translate(10) rotate(45)").unwrap();
        assert!(mixed.contains("translate(10)"), "{mixed}");
        assert!(mixed.contains("rotate(45)"), "{mixed}");
        assert!(
            !mixed.contains("matrix"),
            "must not collapse across rotate: {mixed}"
        );
    }

    #[test]
    fn convert_transform_does_not_bake_into_path() {
        let d = "M0 0C80 200 60 200 40 200";
        let mut doc = doc_svg(vec![node(
            "g",
            &[("transform", "translate(10.000, 0.000)")],
            vec![node("path", &[("d", d)], vec![])],
        )]);
        convert_transform(&mut doc);
        assert_eq!(
            first_attr(&doc, "g", "transform").as_deref(),
            Some("translate(10)")
        );
        assert_eq!(first_attr(&doc, "path", "d").as_deref(), Some(d));
    }

    #[test]
    fn convert_transform_skips_animatetransform() {
        let mut doc = doc_svg(vec![node(
            "g",
            &[("id", "spin"), ("transform", "translate(10.000, 0.000)")],
            vec![
                node(
                    "animateTransform",
                    &[
                        ("attributeName", "transform"),
                        ("type", "translate"),
                        ("from", "10 0"),
                        ("to", "20 0"),
                    ],
                    vec![],
                ),
                node("path", &[("d", "M0 0h2")], vec![]),
            ],
        )]);
        convert_transform(&mut doc);
        assert_eq!(
            first_attr(&doc, "g", "transform").as_deref(),
            Some("translate(10.000, 0.000)")
        );
    }

    #[test]
    fn convert_transform_skips_href_animatetransform() {
        let mut doc = doc_svg(vec![
            node(
                "g",
                &[("id", "pin"), ("transform", "scale(2.000, 2.000)")],
                vec![node("circle", &[("r", "1")], vec![])],
            ),
            node(
                "animateTransform",
                &[
                    ("href", "#pin"),
                    ("attributeName", "transform"),
                    ("type", "scale"),
                ],
                vec![],
            ),
        ]);
        convert_transform(&mut doc);
        assert_eq!(
            first_attr(&doc, "g", "transform").as_deref(),
            Some("scale(2.000, 2.000)")
        );
    }

    #[test]
    fn convert_transform_drops_identity_and_shortens_gradient() {
        let mut doc = doc_svg(vec![
            node(
                "g",
                &[("transform", "matrix(1,0,0,1,0,0)")],
                vec![node("rect", &[("width", "1"), ("height", "1")], vec![])],
            ),
            node(
                "linearGradient",
                &[("id", "g"), ("gradientTransform", "matrix(1 0 0 1 4 0)")],
                vec![],
            ),
        ]);
        convert_transform(&mut doc);
        assert!(first_attr(&doc, "g", "transform").is_none());
        assert_eq!(
            first_attr(&doc, "linearGradient", "gradientTransform").as_deref(),
            Some("translate(4)")
        );
    }
}
