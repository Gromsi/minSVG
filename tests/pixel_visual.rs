//! Visual-safety: compare [`minsvg::optimize`] output to the **original**.
//!
//! Guards: dest-count (ocean-triangle), `url(#Id)` fragment case, SMIL
//! frame presence. Cheap 8×8 occupancy on the circle icon — not a resvg
//! heatmap. The `image` crate in this package decodes PNG/JPEG/WebP only;
//! there is no SVG rasterizer on the 1.83 graph.
//!
//! Tiny fixtures only. The 1.7MB world map and the 162-frame POV movie
//! stay out of this file.

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

use minsvg::{count_id_attrs, optimize, Config};

/// Refuse POV / raven / world-map sized inputs.
const MAX_INPUT_BYTES: usize = 16 * 1024;
const GRID: usize = 8;

const TINY_FIXTURES: &[&str] = &[
    "circle_icon.svg",
    "path.svg",
    "smil-visibility.svg",
    "url-id.svg",
];

const FORBIDDEN: &[&str] = &[
    "fun/00-driving-pov.svg",
    "large/corvus-corax-iucn.svg",
    "large/blankmap-world-flattened.svg",
];

#[test]
fn fixtures_stay_tiny_and_skip_huge_corpus() {
    for name in TINY_FIXTURES {
        let bytes = read_fixture(name);
        assert!(
            bytes.len() <= MAX_INPUT_BYTES,
            "{name}: {}B > {MAX_INPUT_BYTES} — skip huge files",
            bytes.len()
        );
        assert!(
            !FORBIDDEN.iter().any(|f| name.ends_with(f) || *f == *name),
            "{name} must not be a forbidden corpus file"
        );
    }
    for rel in FORBIDDEN {
        assert!(
            !fixture_path(rel).is_file(),
            "do not copy {rel} into tests/fixtures"
        );
    }
}

#[test]
fn ocean_path_dest_count_matches_original() {
    let (input, orig) = load("path.svg");
    let out = optimize(&input, &Config::default()).expect("optimize path.svg");
    assert!(out.svg.contains("<svg"), "{}", out.svg);
    assert!(
        !out.svg.contains("014.1912") && !orig.contains("014.1912"),
        "digit glue:\norig={orig}\nout={}",
        out.svg
    );

    let dests_in = count_dests(&orig);
    let dests_out = count_dests(&out.svg);
    assert!(
        dests_in >= 4,
        "fixture dest-count too small to be the ocean rect: {dests_in}"
    );
    assert_eq!(
        dests_out,
        dests_in,
        "dest-count drifted in={dests_in} out={dests_out}\norig d={}\nout d={}",
        first_d(&orig),
        first_d(&out.svg)
    );
}

#[test]
fn url_id_fragment_case_matches_original() {
    let (input, orig) = load("url-id.svg");
    let ids_in = url_hash_ids(&orig);
    assert!(
        ids_in.contains("poolFill") && ids_in.contains("waterShine") && ids_in.contains("ClipMe"),
        "fixture must name mixed-case url(#Id) paints: {ids_in:?}"
    );

    let out = optimize(&input, &Config::default()).expect("optimize url-id.svg");
    let ids_out = url_hash_ids(&out.svg);
    assert_eq!(
        ids_out, ids_in,
        "url(#Id) set drifted\norig={orig}\nout={}",
        out.svg
    );
    assert!(out.svg.contains("id=\"poolFill\""), "{}", out.svg);
    assert!(out.svg.contains("id=\"waterShine\""), "{}", out.svg);
    assert!(out.svg.contains("id=\"ClipMe\""), "{}", out.svg);
    assert!(out.svg.contains("url(#poolFill)"), "{}", out.svg);
    assert!(out.svg.contains("url(#waterShine)"), "{}", out.svg);
    assert!(out.svg.contains("url(#ClipMe)"), "{}", out.svg);
    assert!(!out.svg.contains("url(#poolfill)"), "{}", out.svg);
    assert!(!out.svg.contains("url(#watershine)"), "{}", out.svg);
    assert!(!out.svg.contains("url(#clipme)"), "{}", out.svg);
    assert!(
        count_id_attrs(&out.svg) >= 3,
        "paint/clip ids dropped: {}",
        out.svg
    );
}

#[test]
fn smil_frames_match_original() {
    let (input, orig) = load("smil-visibility.svg");
    let frames_in = collect_ids(&orig);
    assert!(
        frames_in.contains("frameA") && frames_in.contains("frameB"),
        "fixture must keep both SMIL frames: {frames_in:?}"
    );
    let vis_in = count_visibility_animates(&orig);
    let shapes_in = count_open_tags(&orig, "rect");

    let out = optimize(&input, &Config::default()).expect("optimize smil-visibility.svg");
    assert!(out.animation.motion_sensitive, "{:?}", out.animation);
    assert!(
        out.plugins_skipped
            .iter()
            .any(|p| *p == "removeHiddenElems"),
        "motion must skip removeHiddenElems: {:?}",
        out.plugins_skipped
    );

    let frames_out = collect_ids(&out.svg);
    assert!(
        frames_out.contains("frameA") && frames_out.contains("frameB"),
        "SMIL frame lost:\norig={orig}\nout={}",
        out.svg
    );
    assert!(
        frames_in.is_subset(&frames_out),
        "SMIL ids drifted in={frames_in:?} out={frames_out:?}\n{}",
        out.svg
    );
    assert_eq!(
        count_visibility_animates(&out.svg),
        vis_in,
        "visibility animate count drifted:\n{}",
        out.svg
    );
    assert_eq!(
        count_open_tags(&out.svg, "rect"),
        shapes_in,
        "SMIL rect frames drifted:\n{}",
        out.svg
    );
    assert!(
        out.svg.contains("attributeName=\"visibility\""),
        "{}",
        out.svg
    );
}

/// One document: dest-count + `url(#Id)` + SMIL frames vs the original.
#[test]
fn optimize_preserves_dests_url_ids_and_smil_frames() {
    let input = concat!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 20 20">"##,
        r##"<defs><linearGradient id="poolFill">"##,
        r##"<stop offset="0" stop-color="#fba"/></linearGradient></defs>"##,
        r##"<path class="ocean" fill="url(#poolFill)" "##,
        r##"d="M 16,18 C 12,18 8,18 4,18 C 4,12 4,8 4,4 C 8,4 12,4 16,4 C 16,8 16,12 16,18"/>"##,
        r##"<g visibility="hidden">"##,
        r##"<rect id="frameA" width="20" height="20" fill="#c00">"##,
        r##"<animate attributeName="visibility" values="hidden;visible;hidden" "##,
        r##"keyTimes="0;0.5;1" dur="1s" repeatCount="indefinite"/>"##,
        "</rect></g>",
        r##"<rect id="frameB" width="4" height="4" fill="url(#poolFill)">"##,
        r##"<animate attributeName="visibility" values="visible;hidden" "##,
        r##"keyTimes="0;1" dur="1s" repeatCount="indefinite"/>"##,
        "</rect></svg>",
    );
    assert!(input.len() <= MAX_INPUT_BYTES);

    let dests_in = count_dests(input);
    let urls_in = url_hash_ids(input);
    let vis_in = count_visibility_animates(input);
    assert_eq!(dests_in, 5, "ocean cubics should be 5 dests: {input}");
    assert_eq!(urls_in, BTreeSet::from(["poolFill".into()]));
    assert_eq!(vis_in, 2);

    let out = optimize(input.as_bytes(), &Config::default()).expect("optimize combined");
    assert!(out.animation.motion_sensitive, "{:?}", out.animation);
    assert_eq!(
        count_dests(&out.svg),
        dests_in,
        "dest-count drifted (motion should skip convertPathData):\n{}",
        out.svg
    );
    assert_eq!(
        url_hash_ids(&out.svg),
        urls_in,
        "url(#Id) drifted:\n{}",
        out.svg
    );
    assert!(out.svg.contains("url(#poolFill)"), "{}", out.svg);
    assert!(!out.svg.contains("url(#poolfill)"), "{}", out.svg);
    assert!(out.svg.contains("id=\"frameA\""), "{}", out.svg);
    assert!(out.svg.contains("id=\"frameB\""), "{}", out.svg);
    assert_eq!(
        count_visibility_animates(&out.svg),
        vis_in,
        "SMIL visibility frames dropped:\n{}",
        out.svg
    );
    assert!(!out.svg.contains("014.1912"), "digit glue:\n{}", out.svg);
}

#[test]
fn circle_icon_occupancy_matches_original() {
    let (input, orig) = load("circle_icon.svg");
    let out = optimize(&input, &Config::default()).expect("optimize circle_icon.svg");
    assert!(
        out.svg.contains("<circle") || out.svg.contains("<path"),
        "icon shape lost: {}",
        out.svg
    );
    assert!(
        out.svg.contains("#36f") || out.svg.contains("#3366ff") || out.svg.contains("#3366FF"),
        "icon fill lost: {}",
        out.svg
    );

    let occ_in = occupancy(&orig);
    let occ_out = occupancy(&out.svg);
    let delta = occ_delta(&occ_in, &occ_out);
    assert_eq!(
        delta, 0,
        "8×8 occupancy drifted {delta} cells\norig={orig}\nout={}",
        out.svg
    );
    // Center cell of 64×64 / 8 = 8px cells: index 4,4 is near (36,36), inside r=22.
    assert!(occ_in[4 * GRID + 4], "fixture center must be painted");
    assert!(occ_out[4 * GRID + 4], "optimize() center must stay painted");
    assert!(!occ_in[0] && !occ_out[0], "icon corner must stay empty");
}

#[test]
fn ocean_occupancy_matches_original() {
    let (input, orig) = load("path.svg");
    let out = optimize(&input, &Config::default()).expect("optimize path.svg");
    let occ_in = occupancy(&orig);
    let occ_out = occupancy(&out.svg);
    assert_eq!(
        occ_delta(&occ_in, &occ_out),
        0,
        "ocean dest-polygon occupancy drifted\norig d={}\nout d={}",
        first_d(&orig),
        first_d(&out.svg)
    );
    assert!(
        occ_in.iter().filter(|c| **c).count() >= 4,
        "ocean occupancy too thin"
    );
}

fn load(name: &str) -> (Vec<u8>, String) {
    let bytes = read_fixture(name);
    assert!(
        bytes.len() <= MAX_INPUT_BYTES,
        "{name}: {}B > {MAX_INPUT_BYTES}",
        bytes.len()
    );
    let text = String::from_utf8(bytes.clone()).unwrap_or_else(|e| panic!("{name} utf-8: {e}"));
    (bytes, text)
}

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn read_fixture(name: &str) -> Vec<u8> {
    fs::read(fixture_path(name)).unwrap_or_else(|e| panic!("read {name}: {e}"))
}

fn first_d(svg: &str) -> String {
    attr_values(svg, "d").into_iter().next().unwrap_or_default()
}

fn count_dests(svg: &str) -> usize {
    attr_values(svg, "d")
        .into_iter()
        .map(|d| dest_count(&d))
        .sum()
}

/// Destination points in `d` (M/L/H/V/C/S/Q/T/A). Z adds none.
fn dest_count(d: &str) -> usize {
    let mut cmd = None;
    let mut nums = 0usize;
    let mut dests = 0usize;
    let flush = |cmd: Option<char>, nums: usize, dests: &mut usize| {
        let Some(c) = cmd else { return };
        let arity = match c {
            'M' | 'm' | 'L' | 'l' | 'T' | 't' => 2,
            'H' | 'h' | 'V' | 'v' => 1,
            'C' | 'c' => 6,
            'S' | 's' | 'Q' | 'q' => 4,
            'A' | 'a' => 7,
            'Z' | 'z' => 0,
            _ => return,
        };
        if arity == 0 {
            return;
        }
        *dests += nums / arity;
    };
    let bytes = d.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i] as char;
        if c.is_ascii_alphabetic() {
            flush(cmd, nums, &mut dests);
            cmd = Some(c);
            nums = 0;
            i += 1;
            continue;
        }
        if c == ',' || c.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        if c == '+' || c == '-' || c == '.' || c.is_ascii_digit() {
            nums += 1;
            if c == '+' || c == '-' {
                i += 1;
            }
            let mut saw_dot = false;
            while i < bytes.len() {
                let ch = bytes[i];
                if ch.is_ascii_digit() {
                    i += 1;
                } else if ch == b'.' && !saw_dot {
                    saw_dot = true;
                    i += 1;
                } else if ch == b'e' || ch == b'E' {
                    i += 1;
                    if i < bytes.len() && matches!(bytes[i], b'+' | b'-') {
                        i += 1;
                    }
                    while i < bytes.len() && bytes[i].is_ascii_digit() {
                        i += 1;
                    }
                    break;
                } else {
                    break;
                }
            }
            continue;
        }
        i += 1;
    }
    flush(cmd, nums, &mut dests);
    dests
}

fn dest_points(d: &str) -> Vec<(f64, f64)> {
    let toks = tokenize_path(d);
    let mut dests = Vec::new();
    let mut x = 0.0f64;
    let mut y = 0.0f64;
    let mut sx = 0.0f64;
    let mut sy = 0.0f64;
    let mut cmd = None;
    let mut implicit_lineto = false;
    let mut i = 0;
    while i < toks.len() {
        match toks[i] {
            Tok::Cmd(c) => {
                if c == 'Z' || c == 'z' {
                    x = sx;
                    y = sy;
                    cmd = Some(c);
                    implicit_lineto = false;
                    i += 1;
                    continue;
                }
                cmd = Some(c);
                implicit_lineto = false;
                i += 1;
                continue;
            }
            Tok::Num(_) => {}
        }
        let Some(c) = cmd else {
            i += 1;
            continue;
        };
        let rel = c.is_ascii_lowercase();
        let kind = c.to_ascii_uppercase();
        let take = |i: &mut usize, n: usize| -> Option<Vec<f64>> {
            let mut out = Vec::with_capacity(n);
            for _ in 0..n {
                match toks.get(*i) {
                    Some(Tok::Num(v)) => {
                        out.push(*v);
                        *i += 1;
                    }
                    _ => return None,
                }
            }
            Some(out)
        };
        let apply = |cur: f64, v: f64| if rel { cur + v } else { v };
        match kind {
            'M' => {
                let Some(n) = take(&mut i, 2) else { break };
                x = apply(x, n[0]);
                y = apply(y, n[1]);
                if !implicit_lineto {
                    sx = x;
                    sy = y;
                }
                dests.push((x, y));
                cmd = Some(if rel { 'l' } else { 'L' });
                implicit_lineto = true;
            }
            'L' | 'T' => {
                let Some(n) = take(&mut i, 2) else { break };
                x = apply(x, n[0]);
                y = apply(y, n[1]);
                dests.push((x, y));
            }
            'H' => {
                let Some(n) = take(&mut i, 1) else { break };
                x = apply(x, n[0]);
                dests.push((x, y));
            }
            'V' => {
                let Some(n) = take(&mut i, 1) else { break };
                y = apply(y, n[0]);
                dests.push((x, y));
            }
            'C' => {
                let Some(n) = take(&mut i, 6) else { break };
                x = apply(x, n[4]);
                y = apply(y, n[5]);
                dests.push((x, y));
            }
            'S' | 'Q' => {
                let Some(n) = take(&mut i, 4) else { break };
                x = apply(x, n[2]);
                y = apply(y, n[3]);
                dests.push((x, y));
            }
            'A' => {
                let Some(n) = take(&mut i, 7) else { break };
                x = apply(x, n[5]);
                y = apply(y, n[6]);
                dests.push((x, y));
            }
            'Z' => {
                x = sx;
                y = sy;
            }
            _ => i += 1,
        }
    }
    dests
}

#[derive(Clone, Copy)]
enum Tok {
    Cmd(char),
    Num(f64),
}

fn tokenize_path(d: &str) -> Vec<Tok> {
    let b = d.as_bytes();
    let mut i = 0;
    let mut out = Vec::new();
    while i < b.len() {
        let c = b[i] as char;
        if c.is_ascii_alphabetic() {
            out.push(Tok::Cmd(c));
            i += 1;
            continue;
        }
        if c == ',' || c.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        if c == '+' || c == '-' || c == '.' || c.is_ascii_digit() {
            let start = i;
            if c == '+' || c == '-' {
                i += 1;
            }
            let mut saw_dot = false;
            while i < b.len() {
                let ch = b[i];
                if ch.is_ascii_digit() {
                    i += 1;
                } else if ch == b'.' && !saw_dot {
                    saw_dot = true;
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
            if let Ok(n) = d[start..i].parse::<f64>() {
                out.push(Tok::Num(n));
            }
            continue;
        }
        i += 1;
    }
    out
}

/// `url(#Id)` / `url( '#Id' )` / `URL(#Id)` — fragment case is kept.
fn url_hash_ids(svg: &str) -> BTreeSet<String> {
    let bytes = svg.as_bytes();
    let mut out = BTreeSet::new();
    let mut i = 0;
    while i + 4 < bytes.len() {
        let is_url = bytes[i].eq_ignore_ascii_case(&b'u')
            && bytes[i + 1].eq_ignore_ascii_case(&b'r')
            && bytes[i + 2].eq_ignore_ascii_case(&b'l')
            && bytes[i + 3] == b'(';
        if !is_url {
            i += 1;
            continue;
        }
        let mut j = i + 4;
        while j < bytes.len()
            && (bytes[j].is_ascii_whitespace() || bytes[j] == b'\'' || bytes[j] == b'"')
        {
            j += 1;
        }
        if j < bytes.len() && bytes[j] == b'#' {
            j += 1;
            let start = j;
            while j < bytes.len() {
                let c = bytes[j];
                if c == b')' || c == b'\'' || c == b'"' || c.is_ascii_whitespace() {
                    break;
                }
                j += 1;
            }
            if j > start {
                out.insert(svg[start..j].to_string());
            }
        }
        i = j.max(i + 1);
    }
    out
}

fn collect_ids(svg: &str) -> BTreeSet<String> {
    attr_values(svg, "id").into_iter().collect()
}

fn count_visibility_animates(svg: &str) -> usize {
    open_tags(svg, "animate")
        .into_iter()
        .filter(|tag| {
            attr_in_tag(tag, "attributeName").is_some_and(|v| v.eq_ignore_ascii_case("visibility"))
        })
        .count()
}

fn occupancy(svg: &str) -> [bool; GRID * GRID] {
    let vb = view_box(svg).unwrap_or((0.0, 0.0, 1.0, 1.0));
    let mut grid = [false; GRID * GRID];
    for tag in open_tags(svg, "circle") {
        if let (Some(cx), Some(cy), Some(r)) =
            (attr_f64(tag, "cx"), attr_f64(tag, "cy"), attr_f64(tag, "r"))
        {
            for i in 0..GRID * GRID {
                let (px, py) = cell_center(vb, i);
                if (px - cx).hypot(py - cy) <= r {
                    grid[i] = true;
                }
            }
        }
    }
    for tag in open_tags(svg, "rect") {
        if let (Some(w), Some(h)) = (attr_f64(tag, "width"), attr_f64(tag, "height")) {
            let x = attr_f64(tag, "x").unwrap_or(0.0);
            let y = attr_f64(tag, "y").unwrap_or(0.0);
            for i in 0..GRID * GRID {
                let (px, py) = cell_center(vb, i);
                if px >= x && px <= x + w && py >= y && py <= y + h {
                    grid[i] = true;
                }
            }
        }
    }
    for d in attr_values(svg, "d") {
        raster_poly(&mut grid, vb, &dest_points(&d));
    }
    grid
}

fn raster_poly(grid: &mut [bool; GRID * GRID], vb: (f64, f64, f64, f64), pts: &[(f64, f64)]) {
    if pts.len() < 3 {
        return;
    }
    for i in 0..GRID * GRID {
        let (px, py) = cell_center(vb, i);
        if point_in_poly(px, py, pts) {
            grid[i] = true;
        }
    }
}

fn cell_center(vb: (f64, f64, f64, f64), i: usize) -> (f64, f64) {
    let x = i % GRID;
    let y = i / GRID;
    (
        vb.0 + (x as f64 + 0.5) / GRID as f64 * vb.2,
        vb.1 + (y as f64 + 0.5) / GRID as f64 * vb.3,
    )
}

fn point_in_poly(x: f64, y: f64, pts: &[(f64, f64)]) -> bool {
    let n = pts.len();
    let mut inside = false;
    let mut j = n - 1;
    for i in 0..n {
        let (xi, yi) = pts[i];
        let (xj, yj) = pts[j];
        if (yi > y) != (yj > y) {
            let denom = yj - yi;
            if denom.abs() > f64::EPSILON {
                let xinters = (xj - xi) * (y - yi) / denom + xi;
                if x < xinters {
                    inside = !inside;
                }
            }
        }
        j = i;
    }
    inside
}

fn occ_delta(a: &[bool; GRID * GRID], b: &[bool; GRID * GRID]) -> usize {
    a.iter().zip(b.iter()).filter(|(x, y)| x != y).count()
}

fn view_box(svg: &str) -> Option<(f64, f64, f64, f64)> {
    let raw = attr_values(svg, "viewBox").into_iter().next()?;
    let nums: Vec<f64> = raw
        .split(|c: char| c.is_ascii_whitespace() || c == ',')
        .filter(|s| !s.is_empty())
        .filter_map(|s| s.parse().ok())
        .collect();
    if nums.len() == 4 {
        Some((nums[0], nums[1], nums[2], nums[3]))
    } else {
        None
    }
}

fn open_tags<'a>(svg: &'a str, name: &str) -> Vec<&'a str> {
    let open = format!("<{name}");
    let mut out = Vec::new();
    let mut rest = svg;
    while let Some(i) = rest.find(&open) {
        let after = &rest[i..];
        let end = after.find('>').unwrap_or(after.len());
        out.push(&after[..end]);
        rest = &after[open.len().min(after.len())..];
    }
    out
}

fn count_open_tags(svg: &str, name: &str) -> usize {
    open_tags(svg, name).len()
}

fn attr_values(svg: &str, name: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = svg;
    let key_dq = format!("{name}=\"");
    let key_sq = format!("{name}='");
    loop {
        let dq = rest.find(&key_dq);
        let sq = rest.find(&key_sq);
        let (i, quote, key_len) = match (dq, sq) {
            (Some(a), Some(b)) if a <= b => (a, '"', key_dq.len()),
            (Some(a), None) => (a, '"', key_dq.len()),
            (None, Some(b)) => (b, '\'', key_sq.len()),
            (Some(_), Some(b)) => (b, '\'', key_sq.len()),
            (None, None) => break,
        };
        let after = &rest[i + key_len..];
        let Some(end) = after.find(quote) else { break };
        out.push(after[..end].to_string());
        rest = &after[end + 1..];
    }
    out
}

fn attr_in_tag<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    for (key, quote) in [(format!("{name}=\""), '"'), (format!("{name}='"), '\'')] {
        if let Some(i) = tag.find(&key) {
            let after = &tag[i + key.len()..];
            let end = after.find(quote)?;
            return Some(&after[..end]);
        }
    }
    None
}

fn attr_f64(tag: &str, name: &str) -> Option<f64> {
    attr_in_tag(tag, name)?.parse().ok()
}

#[test]
fn dest_count_ocean_rect_and_hv_agree() {
    let cubics = "M 100,200 C 80,200 60,200 40,200 C 40,160 40,120 40,80 C 60,80 80,80 100,80 C 100,120 100,160 100,200";
    let hv = "M100 200H40V80h60v120";
    assert_eq!(dest_count(cubics), 5);
    assert_eq!(dest_count(hv), 5);
    assert_eq!(dest_points(cubics).len(), 5);
    assert_eq!(dest_points(hv).len(), 5);
}

#[test]
fn url_hash_ids_keep_fragment_case() {
    let s = r#"fill="url(#poolFill)" stroke="URL( '#waterShine' )" clip-path="url(#ClipMe)""#;
    let ids = url_hash_ids(s);
    assert_eq!(
        ids,
        BTreeSet::from(["poolFill".into(), "waterShine".into(), "ClipMe".into()])
    );
    assert!(!ids.contains("poolfill"));
}
