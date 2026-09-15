//! minSVG vs pinned SVGO 4.1.0 on three tiny fixtures.
//!
//! Measures UTF-8 size, wall ms, dest-count, and an 8×8 occupancy check.
//! Not a resvg heatmap (MSRV 1.83). Not an oxvg / svgm / vexy claim.
//!
//! Fixtures stay tiny. The 162-frame driving-POV file and the 1.7MB world
//! map are **not** loaded here — `smil-visibility.svg` and `path.svg` are
//! the stand-ins. Existing ocean / 162-frame tests stay in their files.
//!
//! SVGO spawn (when Node + `adapters/svgo_runner` are present):
//!
//! ```text
//! node adapters/svgo_runner/run.mjs --input <in> --output <out>
//! ```
//!
//! Refresh the committed table after an intentional optimizer change:
//! `UPDATE_GOLDEN=1 cargo test --test parity_bench --offline -- --nocapture`

use minsvg::{optimize, Config};
use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Instant;

const PINNED_SVGO: &str = "4.1.0";
const GRID: usize = 8;

const FIXTURES: &[(&str, &str)] = &[
    ("icon", "circle_icon.svg"),
    ("path", "path.svg"),
    ("smil-visibility", "smil-visibility.svg"),
];

#[test]
fn parity_bench_three_tiny_fixtures_vs_svgo_4_1_0() {
    let rows = measure_all();
    let rendered = render_markdown(&rows);
    eprintln!("{rendered}");

    for row in &rows {
        assert!(
            row.minsvg_svg.contains("<svg"),
            "{}: minSVG lost <svg:\n{}",
            row.slug,
            row.minsvg_svg
        );
        assert!(
            row.minsvg_bytes > 0 && row.minsvg_bytes <= row.input_bytes * 2,
            "{}: minSVG size {} vs input {}",
            row.slug,
            row.minsvg_bytes,
            row.input_bytes
        );
        assert!(
            !row.minsvg_svg.contains("014.1912"),
            "{}: ocean-triangle digit glue:\n{}",
            row.slug,
            row.minsvg_svg
        );
        assert_eq!(
            row.minsvg_occ_delta, 0,
            "{}: 8×8 occupancy drifted {} cells vs input:\n{}",
            row.slug, row.minsvg_occ_delta, row.minsvg_svg
        );
    }

    let icon = row(&rows, "icon");
    assert!(
        icon.minsvg_svg.contains("<circle") || icon.minsvg_svg.contains("<path"),
        "icon: painted shape lost:\n{}",
        icon.minsvg_svg
    );
    assert!(
        has_fill(&icon.minsvg_fills, "#36f") || has_fill(&icon.minsvg_fills, "#3366ff"),
        "icon: blue fill lost: {:?}",
        icon.minsvg_fills
    );

    let path = row(&rows, "path");
    assert!(
        path.input_dests >= 4 && path.minsvg_dests >= 4,
        "path: dest-count collapsed (ocean triangle): in={} minSVG={}",
        path.input_dests,
        path.minsvg_dests
    );
    assert!(
        path.minsvg_svg.contains("class=\"ocean\"") || path.minsvg_svg.contains("class='ocean'"),
        "path: .ocean class lost:\n{}",
        path.minsvg_svg
    );

    let smil = row(&rows, "smil-visibility");
    assert!(
        smil.minsvg_svg.contains("id=\"frameA\"") && smil.minsvg_svg.contains("id=\"frameB\""),
        "smil: 162-frame stand-in lost an id:\n{}",
        smil.minsvg_svg
    );
    assert_eq!(
        smil.minsvg_shapes, 2,
        "smil: minSVG must keep both frames:\n{}",
        smil.minsvg_svg
    );
    assert!(
        smil.minsvg_vis_animates >= 2,
        "smil: visibility animates dropped:\n{}",
        smil.minsvg_svg
    );
    assert!(
        has_fill(&smil.minsvg_fills, "#c00") && has_fill(&smil.minsvg_fills, "#0a0"),
        "smil: frame fills lost: {:?}",
        smil.minsvg_fills
    );

    if let Some(svgo) = &icon.svgo {
        assert_eq!(
            svgo.version, PINNED_SVGO,
            "adapter must be pinned SVGO {PINNED_SVGO}, got {}",
            svgo.version
        );
    }
    if let Some(svgo) = &smil.svgo {
        if svgo.shapes < 2 || !has_fill(&svgo.fills, "#c00") {
            eprintln!(
                "smil: SVGO {PINNED_SVGO} dropped the hidden frame (162-frame stand-in); shapes={} vis_animates={} fills={:?}",
                svgo.shapes, svgo.vis_animates, svgo.fills
            );
        }
    }

    let md_path = results_md_path();
    if update_golden() {
        fs::write(&md_path, &rendered)
            .unwrap_or_else(|e| panic!("write {}: {e}", md_path.display()));
        eprintln!("wrote {}", md_path.display());
        return;
    }

    assert!(
        md_path.is_file(),
        "missing {} — generate it with\n  UPDATE_GOLDEN=1 cargo test --test parity_bench --offline -- --nocapture",
        md_path.display()
    );
    let committed =
        fs::read_to_string(&md_path).unwrap_or_else(|e| panic!("read {}: {e}", md_path.display()));
    assert_honesty_copy(&committed);
    assert_stable_columns(&committed, &rows);
}

fn measure_all() -> Vec<Row> {
    let have_svgo = svgo_ready();
    if !have_svgo {
        eprintln!(
            "svgo/node missing — skipping SVGO columns; still asserting minSVG checks\n\
             looked for {} and `node`",
            svgo_runner_dir().join("node_modules/svgo").display()
        );
    }

    FIXTURES
        .iter()
        .map(|(slug, file)| measure_one(slug, file, have_svgo))
        .collect()
}

fn measure_one(slug: &str, file: &str, have_svgo: bool) -> Row {
    let input = read_fixture(file);
    let input_svg = String::from_utf8(input.clone()).expect("fixture utf-8");
    let input_check = paint_check(&input_svg);

    let _ = optimize(&input, &Config::default()).expect("warmup");
    let t = Instant::now();
    let out = optimize(&input, &Config::default()).unwrap_or_else(|e| panic!("{slug}: {e}"));
    let minsvg_ms = t.elapsed().as_secs_f64() * 1000.0;
    let minsvg_check = paint_check(&out.svg);

    let svgo = if have_svgo {
        Some(run_svgo(slug, &input, &input_check))
    } else {
        None
    };

    Row {
        slug: slug.to_string(),
        file: file.to_string(),
        input_bytes: input.len(),
        input_dests: input_check.dests,
        minsvg_bytes: out.svg.len(),
        minsvg_ms,
        minsvg_dests: minsvg_check.dests,
        minsvg_occ_delta: occ_delta(&input_check.occupancy, &minsvg_check.occupancy),
        minsvg_fills: minsvg_check.fills,
        minsvg_shapes: minsvg_check.shapes,
        minsvg_vis_animates: minsvg_check.vis_animates,
        minsvg_svg: out.svg,
        svgo,
    }
}

fn run_svgo(slug: &str, input: &[u8], input_check: &PaintCheck) -> SvgoRow {
    let runner_dir = svgo_runner_dir();
    let runner = runner_dir.join("run.mjs");
    let tmp = env::temp_dir().join(format!("minsvg-parity-{}-{}", std::process::id(), slug));
    fs::create_dir_all(&tmp).expect("temp dir");
    let in_path = tmp.join("in.svg");
    let out_path = tmp.join("out.svg");
    fs::write(&in_path, input).expect("write svgo input");

    let t = Instant::now();
    let output = Command::new("node")
        .arg(&runner)
        .arg("--input")
        .arg(&in_path)
        .arg("--output")
        .arg(&out_path)
        .current_dir(&runner_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .unwrap_or_else(|e| panic!("{slug}: spawn node: {e}"));
    let wall_ms = t.elapsed().as_secs_f64() * 1000.0;

    let stderr = String::from_utf8_lossy(&output.stderr);
    if !output.status.success() {
        let _ = fs::remove_dir_all(&tmp);
        panic!("{slug}: svgo runner failed:\n{stderr}");
    }
    let svg = fs::read_to_string(&out_path).unwrap_or_else(|e| panic!("{slug}: read svgo: {e}"));
    let _ = fs::remove_dir_all(&tmp);

    let version = json_field(&stderr, "version").unwrap_or_else(|| "unknown".into());
    let check = paint_check(&svg);
    SvgoRow {
        version,
        bytes: svg.len(),
        wall_ms,
        dests: check.dests,
        occ_delta: occ_delta(&input_check.occupancy, &check.occupancy),
        fills: check.fills,
        shapes: check.shapes,
        vis_animates: check.vis_animates,
    }
}

fn render_markdown(rows: &[Row]) -> String {
    let have_svgo = rows.iter().any(|r| r.svgo.is_some());
    let mut buf = String::new();
    buf.push_str("# minSVG vs SVGO 4.1.0 (tiny fixtures)\n\n");
    buf.push_str("In-process [`minsvg::optimize`] vs pinned **SVGO 4.1.0** ");
    buf.push_str("(`svgo-rust/adapters/svgo_runner`, `multipass: true` in `run.mjs`).\n\n");
    buf.push_str("This is **not** an oxvg, svgm, or vexy-vsvg claim. ");
    buf.push_str("A smaller byte count is **not more correct**.\n\n");
    buf.push_str("**Wall time:** minSVG is library-only (`std::time::Instant` around ");
    buf.push_str("`optimize`, one warmup). SVGO is a fresh `node run.mjs` spawn — ");
    buf.push_str("Node startup is included. Times are from this machine; they drift.\n\n");
    buf.push_str("**Cheap pixel / dest-count:** 8×8 occupancy from circle / rect / ");
    buf.push_str("path dest-polygon (even-odd). Dest-count is destination points in ");
    buf.push_str("`d` — the ocean-triangle guard. No resvg (MSRV 1.83).\n\n");
    buf.push_str("Fixtures are tiny on purpose. The 162-frame driving-POV movie and ");
    buf.push_str("the 1.7MB world-map ocean are **not** in this table — ");
    buf.push_str("`smil-visibility.svg` and `path.svg` are the stand-ins so CI stays ");
    buf.push_str("small. Existing 162-frame / ocean tests are unchanged.\n\n");
    buf.push_str("Refresh after an intentional optimizer change:\n\n");
    buf.push_str("```\n");
    buf.push_str("UPDATE_GOLDEN=1 cargo test --test parity_bench --offline -- --nocapture\n");
    buf.push_str("```\n\n");

    if have_svgo {
        buf.push_str(&format!(
            "- SVGO: {PINNED_SVGO} (`node adapters/svgo_runner/run.mjs --input --output`)\n"
        ));
        buf.push_str("- minSVG: `minsvg::optimize` (in-process)\n\n");
        buf.push_str("| fixture | in | minSVG B | SVGO B | minSVG ms | SVGO ms | dests in/minSVG/SVGO | occ Δ minSVG | occ Δ SVGO | notes |\n");
        buf.push_str("|---|---:|---:|---:|---:|---:|---:|---:|---:|---|\n");
        for row in rows {
            let svgo = row.svgo.as_ref().expect("svgo column");
            buf.push_str(&format!(
                "| `{}` | {} | {} | {} | {} | {} | {}/{}/{} | {} | {} | {} |\n",
                row.file,
                row.input_bytes,
                row.minsvg_bytes,
                svgo.bytes,
                fmt_ms(row.minsvg_ms),
                fmt_ms(svgo.wall_ms),
                row.input_dests,
                row.minsvg_dests,
                svgo.dests,
                row.minsvg_occ_delta,
                svgo.occ_delta,
                notes(row),
            ));
        }
    } else {
        buf.push_str("SVGO columns skipped: `node` or `adapters/svgo_runner/node_modules/svgo` ");
        buf.push_str("was missing. minSVG optimize still ran.\n\n");
        buf.push_str(
            "| fixture | in | minSVG B | minSVG ms | dests in/minSVG | occ Δ minSVG | notes |\n",
        );
        buf.push_str("|---|---:|---:|---:|---:|---:|---|\n");
        for row in rows {
            buf.push_str(&format!(
                "| `{}` | {} | {} | {} | {}/{} | {} | {} |\n",
                row.file,
                row.input_bytes,
                row.minsvg_bytes,
                fmt_ms(row.minsvg_ms),
                row.input_dests,
                row.minsvg_dests,
                row.minsvg_occ_delta,
                notes(row),
            ));
        }
    }
    buf.push('\n');
    buf.push_str("Negative `minSVG B − SVGO B` is a compression badge, not a ");
    buf.push_str("correctness claim. On `smil-visibility.svg`, stock SVGO 4.1.0 ");
    buf.push_str("`removeHiddenElems` deletes the hidden SMIL frame (tiny stand-in ");
    buf.push_str("for the 162-frame driving-POV bug). minSVG keeps both frames.\n");
    buf
}

fn notes(row: &Row) -> String {
    match row.slug.as_str() {
        "icon" => "circle + #36f; occupancy match".into(),
        "path" => "ocean cubic rect; dests≥4; no `014.1912`".into(),
        "smil-visibility" => {
            if let Some(svgo) = &row.svgo {
                if svgo.shapes < 2 || !has_fill(&svgo.fills, "#c00") {
                    return "minSVG keeps 2 frames; SVGO drops hidden".into();
                }
            }
            "minSVG keeps both SMIL frames".into()
        }
        _ => String::new(),
    }
}

fn assert_honesty_copy(md: &str) {
    let lower = md.to_ascii_lowercase();
    assert!(
        md.contains(PINNED_SVGO),
        "results must name pinned SVGO {PINNED_SVGO}"
    );
    assert!(
        lower.contains("not more correct")
            || (lower.contains("not") && lower.contains("more correct")),
        "results must say a smaller output is not more correct"
    );
    assert!(
        lower.contains("node") && (lower.contains("startup") || lower.contains("spawn")),
        "results must say SVGO wall includes Node startup"
    );
    for (slug, file) in FIXTURES {
        assert!(
            md.contains(file),
            "results must name fixture {file} ({slug})"
        );
    }
    assert!(
        lower.contains("dest")
            && (lower.contains("occup") || lower.contains("8×8") || lower.contains("8x8")),
        "results must mention dest-count and occupancy"
    );
}

fn assert_stable_columns(md: &str, rows: &[Row]) {
    for row in rows {
        let needle = format!("| `{}` |", row.file);
        let line = md
            .lines()
            .find(|l| l.contains(&needle))
            .unwrap_or_else(|| panic!("results missing table row for {}", row.file));
        let cells: Vec<&str> = line
            .trim_matches('|')
            .split('|')
            .map(|c| c.trim())
            .collect();
        assert!(
            cells.len() >= 3,
            "malformed results row for {}: {line}",
            row.file
        );
        let in_b = parse_int(cells[1]);
        let rust_b = parse_int(cells[2]);
        assert_eq!(
            in_b, row.input_bytes,
            "{}: input bytes drifted in results ({in_b} vs {})",
            row.file, row.input_bytes
        );
        assert_eq!(
            rust_b, row.minsvg_bytes,
            "{}: minSVG bytes drifted vs {} — refresh with UPDATE_GOLDEN=1",
            row.file, row.minsvg_bytes
        );
        if row.svgo.is_some() && cells.len() >= 4 && cells[3].chars().all(|c| c.is_ascii_digit()) {
            let svgo_b = parse_int(cells[3]);
            let got = row.svgo.as_ref().unwrap().bytes;
            assert_eq!(
                svgo_b, got,
                "{}: SVGO bytes drifted vs {got} — refresh with UPDATE_GOLDEN=1",
                row.file
            );
        }
    }
}

fn parse_int(s: &str) -> usize {
    s.trim()
        .replace(',', "")
        .parse::<usize>()
        .unwrap_or_else(|_| panic!("not an integer: {s}"))
}

fn fmt_ms(ms: f64) -> String {
    if ms < 1.0 {
        format!("{ms:.3}")
    } else if ms < 10.0 {
        format!("{ms:.2}")
    } else {
        format!("{ms:.1}")
    }
}

fn row<'a>(rows: &'a [Row], slug: &str) -> &'a Row {
    rows.iter()
        .find(|r| r.slug == slug)
        .unwrap_or_else(|| panic!("missing row {slug}"))
}

fn has_fill(fills: &BTreeSet<String>, want: &str) -> bool {
    let want = normalize_color(want);
    fills.iter().any(|f| normalize_color(f) == want)
}

fn normalize_color(raw: &str) -> String {
    let s = raw.trim().trim_matches('"').to_ascii_lowercase();
    if let Some(hex) = s.strip_prefix('#') {
        if hex.len() == 3 {
            let b = hex.as_bytes();
            return format!(
                "#{0}{0}{1}{1}{2}{2}",
                b[0] as char, b[1] as char, b[2] as char
            );
        }
        if hex.len() == 6 {
            return format!("#{hex}");
        }
    }
    s
}

#[derive(Debug)]
struct Row {
    slug: String,
    file: String,
    input_bytes: usize,
    input_dests: usize,
    minsvg_bytes: usize,
    minsvg_ms: f64,
    minsvg_dests: usize,
    minsvg_occ_delta: usize,
    minsvg_fills: BTreeSet<String>,
    minsvg_shapes: usize,
    minsvg_vis_animates: usize,
    minsvg_svg: String,
    svgo: Option<SvgoRow>,
}

#[derive(Debug)]
struct SvgoRow {
    version: String,
    bytes: usize,
    wall_ms: f64,
    dests: usize,
    occ_delta: usize,
    fills: BTreeSet<String>,
    shapes: usize,
    vis_animates: usize,
}

#[derive(Debug)]
struct PaintCheck {
    dests: usize,
    occupancy: [bool; GRID * GRID],
    fills: BTreeSet<String>,
    shapes: usize,
    vis_animates: usize,
}

fn paint_check(svg: &str) -> PaintCheck {
    let vb = view_box(svg).unwrap_or((0.0, 0.0, 1.0, 1.0));
    let mut occupancy = [false; GRID * GRID];
    let mut dests = 0usize;
    let mut fills = BTreeSet::new();
    let mut shapes = 0usize;

    for tag in ["circle", "rect", "path", "ellipse", "polygon"] {
        shapes += count_open_tags(svg, tag);
    }

    for fill in attr_values(svg, "fill") {
        if fill != "none" {
            fills.insert(normalize_color(&fill));
        }
    }

    for (cx, cy, r) in circles(svg) {
        raster_circle(&mut occupancy, vb, cx, cy, r);
    }
    for (x, y, w, h) in rects(svg) {
        raster_rect(&mut occupancy, vb, x, y, w, h);
    }
    for d in attr_values(svg, "d") {
        dests += count_dest_points(&d);
        let pts = dest_points(&d);
        raster_poly(&mut occupancy, vb, &pts);
    }

    PaintCheck {
        dests,
        occupancy,
        fills,
        shapes,
        vis_animates: count_visibility_animates(svg),
    }
}

fn occ_delta(a: &[bool; GRID * GRID], b: &[bool; GRID * GRID]) -> usize {
    a.iter().zip(b.iter()).filter(|(x, y)| x != y).count()
}

fn raster_circle(
    grid: &mut [bool; GRID * GRID],
    vb: (f64, f64, f64, f64),
    cx: f64,
    cy: f64,
    r: f64,
) {
    for i in 0..GRID * GRID {
        let (px, py) = cell_center(vb, i);
        if (px - cx).hypot(py - cy) <= r {
            grid[i] = true;
        }
    }
}

fn raster_rect(
    grid: &mut [bool; GRID * GRID],
    vb: (f64, f64, f64, f64),
    x: f64,
    y: f64,
    w: f64,
    h: f64,
) {
    for i in 0..GRID * GRID {
        let (px, py) = cell_center(vb, i);
        if px >= x && px <= x + w && py >= y && py <= y + h {
            grid[i] = true;
        }
    }
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

fn circles(svg: &str) -> Vec<(f64, f64, f64)> {
    open_tags(svg, "circle")
        .into_iter()
        .filter_map(|tag| {
            Some((
                attr_f64(tag, "cx")?,
                attr_f64(tag, "cy")?,
                attr_f64(tag, "r")?,
            ))
        })
        .collect()
}

fn rects(svg: &str) -> Vec<(f64, f64, f64, f64)> {
    open_tags(svg, "rect")
        .into_iter()
        .filter_map(|tag| {
            Some((
                attr_f64(tag, "x").unwrap_or(0.0),
                attr_f64(tag, "y").unwrap_or(0.0),
                attr_f64(tag, "width")?,
                attr_f64(tag, "height")?,
            ))
        })
        .collect()
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

fn count_visibility_animates(svg: &str) -> usize {
    open_tags(svg, "animate")
        .into_iter()
        .filter(|tag| {
            attr_in_tag(tag, "attributeName").is_some_and(|v| v.eq_ignore_ascii_case("visibility"))
        })
        .count()
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

fn count_dest_points(d: &str) -> usize {
    dest_points(d).len()
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

fn json_field(s: &str, key: &str) -> Option<String> {
    let pat = format!("\"{key}\":\"");
    let i = s.find(&pat)?;
    let after = &s[i + pat.len()..];
    let end = after.find('"')?;
    Some(after[..end].to_string())
}

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn read_fixture(name: &str) -> Vec<u8> {
    fs::read(fixture_path(name)).unwrap_or_else(|e| panic!("read {name}: {e}"))
}

fn results_md_path() -> PathBuf {
    if let Ok(p) = env::var("PARITY_BENCH_MD") {
        return PathBuf::from(p);
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/parity_bench.md")
}

fn svgo_runner_dir() -> PathBuf {
    if let Ok(p) = env::var("SVGO_RUNNER") {
        return PathBuf::from(p);
    }
    let here = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    for cand in [
        here.join("../svgo-rust/adapters/svgo_runner"),
        here.join("../../svgo-rust/adapters/svgo_runner"),
    ] {
        if cand.join("run.mjs").is_file() {
            return cand;
        }
    }
    here.join("../svgo-rust/adapters/svgo_runner")
}

fn svgo_ready() -> bool {
    let dir = svgo_runner_dir();
    dir.join("run.mjs").is_file() && dir.join("node_modules/svgo").is_dir() && node_available()
}

fn node_available() -> bool {
    Command::new("node")
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn update_golden() -> bool {
    match env::var("UPDATE_GOLDEN") {
        Ok(v) => {
            let v = v.trim();
            v == "1" || v.eq_ignore_ascii_case("true") || v.eq_ignore_ascii_case("yes")
        }
        Err(_) => false,
    }
}

#[test]
fn dest_points_match_ocean_rectangle_and_svgo_hv() {
    let cubics = "M 100,200 C 80,200 60,200 40,200 C 40,160 40,120 40,80 C 60,80 80,80 100,80 C 100,120 100,160 100,200";
    let hv = "M100 200H40V80h60v120";
    let a = dest_points(cubics);
    let b = dest_points(hv);
    assert_eq!(a.len(), 5, "{a:?}");
    assert_eq!(b.len(), 5, "{b:?}");
    assert!(
        (a[1].0 - 40.0).abs() < 0.01 && (a[2].1 - 80.0).abs() < 0.01,
        "cubic dests: {a:?}"
    );
}

#[test]
fn cheap_occupancy_keeps_icon_center_and_ocean_box() {
    let icon = String::from_utf8(read_fixture("circle_icon.svg")).unwrap();
    let path = String::from_utf8(read_fixture("path.svg")).unwrap();
    let icon_occ = paint_check(&icon);
    let path_occ = paint_check(&path);
    let (icx, _icy) = cell_center((0.0, 0.0, 64.0, 64.0), 4 * GRID + 4);
    assert!((icx - 36.0).abs() < 8.0, "sanity cell center {icx}");
    assert!(
        icon_occ.occupancy[4 * GRID + 4],
        "icon center cell must be painted"
    );
    assert!(!icon_occ.occupancy[0], "icon corner cell must be empty");
    assert!(path_occ.dests >= 4, "path dests {}", path_occ.dests);
    assert!(
        path_occ.occupancy.iter().filter(|c| **c).count() >= 4,
        "ocean rect occupancy too thin"
    );
}
