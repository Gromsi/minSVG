//! minSVG vs pinned SVGO 4.1.0 on **real** svgo-rust corpus files
//! (capybara / rust / onsen plus small icons: circle / heart / star / check).
//!
//! Read-only. Files stay in the sibling bench (`svgo-rust/corpus/…`). This
//! crate does **not** copy `fun/00-driving-pov.svg` (~4 MB) or
//! `large/corvus-corax-iucn.svg` (~25 MB) into git.
//!
//! Measures UTF-8 size, wall ms, dest-count, and visual/id notes
//! (`url(#poolFill)`, hat ticks, rust path). Not a resvg heatmap (MSRV 1.83).
//! Not an oxvg / svgm / vexy claim.
//!
//! SVGO spawn (when Node + `adapters/svgo_runner` are present):
//!
//! ```text
//! node adapters/svgo_runner/run.mjs --input <in> --output <out>
//! ```
//!
//! Refresh the committed table after an intentional optimizer change:
//! `UPDATE_GOLDEN=1 cargo test --test parity_reallife --offline -- --nocapture`

use minsvg::{optimize, Config, MOTION_SKIP_PLUGINS};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Instant;

const PINNED_SVGO: &str = "4.1.0";

/// Sibling-corpus files we measure. Never add the POV movie or raven map here.
const FILES: &[(&str, &str)] = &[
    ("capybara", "fun/capybara.svg"),
    ("rust-logo", "logos/rust.svg"),
    ("onsen", "animation/capybara-hot-spring.svg"),
    ("circle-icon", "circle_icon.svg"),
    ("heart", "heart.svg"),
    ("star", "star.svg"),
    ("check", "check.svg"),
];

const FORBIDDEN: &[&str] = &["fun/00-driving-pov.svg", "large/corvus-corax-iucn.svg"];

/// Hard cap so a path typo cannot slurp the 25 MB raven into this test.
const MAX_INPUT_BYTES: usize = 100 * 1024;

#[test]
fn parity_reallife_corpus_files_vs_svgo_4_1_0() {
    for rel in FORBIDDEN {
        assert!(
            !FILES.iter().any(|(_, f)| f == rel),
            "do not add {rel} to this table (keep it out of minSVG git)"
        );
    }

    let md_path = results_md_path();
    let Some(corpus) = corpus_dir() else {
        eprintln!(
            "svgo-rust corpus missing — skipping live measure; asserting committed table copy\n\
             looked under sibling `svgo-rust/corpus` (or CORPUS_DIR / SVGO_RUST_ROOT)"
        );
        assert!(
            md_path.is_file(),
            "missing {} — generate it next to a svgo-rust checkout with\n  \
             UPDATE_GOLDEN=1 cargo test --test parity_reallife --offline -- --nocapture",
            md_path.display()
        );
        let committed = fs::read_to_string(&md_path)
            .unwrap_or_else(|e| panic!("read {}: {e}", md_path.display()));
        assert_honesty_copy(&committed);
        return;
    };

    let rows = measure_all(&corpus);
    let rendered = render_markdown(&rows);
    eprintln!("{rendered}");

    assert_row_invariants(&rows);

    if update_golden() {
        fs::write(&md_path, &rendered)
            .unwrap_or_else(|e| panic!("write {}: {e}", md_path.display()));
        eprintln!("wrote {}", md_path.display());
        return;
    }

    assert!(
        md_path.is_file(),
        "missing {} — generate it with\n  UPDATE_GOLDEN=1 cargo test --test parity_reallife --offline -- --nocapture",
        md_path.display()
    );
    let committed =
        fs::read_to_string(&md_path).unwrap_or_else(|e| panic!("read {}: {e}", md_path.display()));
    assert_honesty_copy(&committed);
    assert_stable_columns(&committed, &rows);
}

fn assert_row_invariants(rows: &[Row]) {
    for row in rows {
        assert!(
            row.input_bytes > 0 && row.input_bytes <= MAX_INPUT_BYTES,
            "{}: input {}B outside 1..={MAX_INPUT_BYTES} (refusing POV/raven-sized files)",
            row.rel,
            row.input_bytes
        );
        assert!(
            row.minsvg_svg.contains("<svg"),
            "{}: minSVG lost <svg:\n{}",
            row.rel,
            row.minsvg_svg
        );
        assert!(
            row.minsvg_bytes > 0 && row.minsvg_bytes <= row.input_bytes * 2,
            "{}: minSVG size {} vs input {}",
            row.rel,
            row.minsvg_bytes,
            row.input_bytes
        );
        assert!(
            !row.minsvg_svg.contains("014.1912"),
            "{}: dest-count / digit glue:\n{}",
            row.rel,
            row.minsvg_svg
        );
    }

    let capy = row(rows, "capybara");
    assert!(
        capy.minsvg_svg.contains("<ellipse") || capy.minsvg_svg.contains("<path"),
        "capybara: painted shape lost:\n{}",
        capy.minsvg_svg
    );
    assert!(
        !capy.minsvg_svg.contains("M139 56 15-16") && !capy.minsvg_svg.contains("M139 56 15 -16"),
        "capybara: hat tick became L(15,-16):\n{}",
        capy.minsvg_svg
    );
    assert!(
        capy.minsvg_svg.contains("154")
            || capy.minsvg_svg.contains("x2=\"154\"")
            || capy.minsvg_svg.contains("l15-16")
            || capy.minsvg_svg.contains("15-16"),
        "capybara: right hat tick lost:\n{}",
        capy.minsvg_svg
    );
    assert!(
        capy.minsvg_dests >= 4 && capy.minsvg_dests >= capy.input_dests.saturating_sub(2),
        "capybara: dest-count collapsed in={} minSVG={}",
        capy.input_dests,
        capy.minsvg_dests
    );

    let rust = row(rows, "rust-logo");
    assert!(
        rust.minsvg_svg.contains("<path"),
        "rust.svg: path lost:\n{}",
        rust.minsvg_svg
    );
    assert!(
        rust.minsvg_svg.contains("<title>") && rust.minsvg_svg.contains("Rust"),
        "rust.svg: title lost (a11y):\n{}",
        rust.minsvg_svg
    );
    assert_eq!(
        rust.minsvg_dests, rust.input_dests,
        "rust.svg: dest-count drifted in={} minSVG={} (convertPathData guard)",
        rust.input_dests, rust.minsvg_dests
    );
    assert!(
        rust.input_dests >= 20,
        "rust.svg: dest-count too small to be the gear ({})",
        rust.input_dests
    );

    let onsen = row(rows, "onsen");
    assert!(
        onsen.minsvg_motion,
        "onsen: expected motion-sensitive; skipped={:?}",
        onsen.minsvg_skipped
    );
    for plugin in MOTION_SKIP_PLUGINS {
        assert!(
            onsen.minsvg_skipped.iter().any(|s| s == plugin),
            "onsen: {plugin} must stay skipped; skipped={:?}",
            onsen.minsvg_skipped
        );
    }
    assert!(
        onsen.minsvg_svg.contains("id=\"poolFill\""),
        "onsen: id=poolFill lost:\n{}",
        onsen.minsvg_svg
    );
    assert!(
        onsen.minsvg_svg.contains("url(#poolFill)"),
        "onsen: url(#poolFill) lost:\n{}",
        onsen.minsvg_svg
    );
    assert!(
        !onsen.minsvg_svg.contains("url(#poolfill)"),
        "onsen: url(#poolFill) case folded:\n{}",
        onsen.minsvg_svg
    );
    assert!(
        onsen.minsvg_svg.contains("url(#waterShine)"),
        "onsen: url(#waterShine) lost:\n{}",
        onsen.minsvg_svg
    );
    for id in [
        "capyFloater",
        "orangeHat",
        "steamA",
        "steamB",
        "steamC",
        "leafBoat",
        "chew",
        "ripplePulse",
        "yuzuLift",
        "unused-onsen-wrap",
    ] {
        assert!(
            onsen.minsvg_svg.contains(&format!("id=\"{id}\"")),
            "onsen: missing id={id}:\n{}",
            onsen.minsvg_svg
        );
    }
    assert!(
        onsen.minsvg_svg.contains("transform-origin:140px 110px"),
        "onsen: capyFloater origin lost px:\n{}",
        onsen.minsvg_svg
    );
    assert_eq!(
        onsen.minsvg_dests, onsen.input_dests,
        "onsen: dest-count drifted in={} minSVG={} (motion skips convertPathData)",
        onsen.input_dests, onsen.minsvg_dests
    );

    let icon = row(rows, "circle-icon");
    assert!(
        icon.minsvg_svg.contains("<circle") || icon.minsvg_svg.contains("<path"),
        "circle_icon: painted shape lost:\n{}",
        icon.minsvg_svg
    );
    assert!(
        icon.minsvg_svg.contains("viewBox"),
        "circle_icon: viewBox lost (icons need it):\n{}",
        icon.minsvg_svg
    );
    assert!(
        icon.minsvg_svg.contains("#36f") || icon.minsvg_svg.contains("#3366ff"),
        "circle_icon: blue fill lost:\n{}",
        icon.minsvg_svg
    );
    assert!(
        !icon.minsvg_svg.contains("<?xml") && !icon.minsvg_svg.contains("DOCTYPE"),
        "circle_icon: decl/doctype survived:\n{}",
        icon.minsvg_svg
    );

    let heart = row(rows, "heart");
    assert!(
        heart.minsvg_svg.contains("<path"),
        "heart: path lost:\n{}",
        heart.minsvg_svg
    );
    assert!(
        heart.minsvg_svg.contains("viewBox"),
        "heart: viewBox lost:\n{}",
        heart.minsvg_svg
    );
    assert!(
        heart.minsvg_svg.contains("#ff4060")
            || heart.minsvg_svg.contains("rgb(255,64,96)")
            || heart.minsvg_svg.contains("rgb(255, 64, 96)"),
        "heart: fill lost:\n{}",
        heart.minsvg_svg
    );
    assert!(
        !heart.minsvg_svg.contains("unused-heart-wrap"),
        "heart: unused wrap should drop:\n{}",
        heart.minsvg_svg
    );
    assert_eq!(
        heart.minsvg_dests, heart.input_dests,
        "heart: dest-count drifted in={} minSVG={}",
        heart.input_dests, heart.minsvg_dests
    );
    assert!(
        heart.input_dests >= 4,
        "heart: dest-count too small ({})",
        heart.input_dests
    );

    let star = row(rows, "star");
    assert!(
        star.minsvg_svg.contains("<polygon") || star.minsvg_svg.contains("<path"),
        "star: painted shape lost:\n{}",
        star.minsvg_svg
    );
    assert!(
        star.minsvg_svg.contains("viewBox"),
        "star: viewBox lost:\n{}",
        star.minsvg_svg
    );
    assert!(
        star.minsvg_svg.contains("gold") || star.minsvg_svg.contains("#ffd700"),
        "star: gold fill lost:\n{}",
        star.minsvg_svg
    );
    assert!(
        !star.minsvg_svg.contains("unused-star-wrap"),
        "star: unused wrap should drop:\n{}",
        star.minsvg_svg
    );
    if star.minsvg_svg.contains("<polygon") {
        assert!(
            star.minsvg_svg.contains("48") && star.minsvg_svg.contains("88"),
            "star: polygon points collapsed:\n{}",
            star.minsvg_svg
        );
    }

    let check = row(rows, "check");
    assert!(
        check.minsvg_svg.contains("<circle") || check.minsvg_svg.contains("<path"),
        "check: badge shape lost:\n{}",
        check.minsvg_svg
    );
    assert!(
        check.minsvg_svg.contains("<path"),
        "check: tick path lost:\n{}",
        check.minsvg_svg
    );
    assert!(
        check.minsvg_svg.contains("viewBox"),
        "check: viewBox lost:\n{}",
        check.minsvg_svg
    );
    assert!(
        check.minsvg_svg.contains("#22c55e") || check.minsvg_svg.contains("#22C55E"),
        "check: badge fill lost:\n{}",
        check.minsvg_svg
    );
    assert!(
        !check.minsvg_svg.contains("unused-check-grad"),
        "check: unused gradient should drop:\n{}",
        check.minsvg_svg
    );
    assert_eq!(
        check.minsvg_dests, check.input_dests,
        "check: dest-count drifted in={} minSVG={}",
        check.input_dests, check.minsvg_dests
    );
    assert!(
        check.input_dests >= 3,
        "check: dest-count too small ({})",
        check.input_dests
    );

    if let Some(svgo) = &rust.svgo {
        assert_eq!(
            svgo.version, PINNED_SVGO,
            "adapter must be pinned SVGO {PINNED_SVGO}, got {}",
            svgo.version
        );
    }
}

fn measure_all(corpus: &Path) -> Vec<Row> {
    let have_svgo = svgo_ready();
    if !have_svgo {
        eprintln!(
            "svgo/node missing — skipping SVGO columns; still asserting minSVG checks\n\
             looked for {} and `node`",
            svgo_runner_dir().join("node_modules/svgo").display()
        );
    }
    FILES
        .iter()
        .map(|(slug, rel)| measure_one(slug, rel, corpus, have_svgo))
        .collect()
}

fn measure_one(slug: &str, rel: &str, corpus: &Path, have_svgo: bool) -> Row {
    assert!(
        !FORBIDDEN.contains(&rel),
        "refusing to read forbidden {rel}"
    );
    let path = corpus.join(rel);
    assert!(
        path.is_file(),
        "missing corpus file {} (read-only sibling; do not copy POV/raven into minSVG)",
        path.display()
    );
    let input = fs::read(&path).unwrap_or_else(|e| panic!("read {rel}: {e}"));
    assert!(
        input.len() <= MAX_INPUT_BYTES,
        "{rel}: {}B > {MAX_INPUT_BYTES} — refuse POV/raven-sized inputs",
        input.len()
    );
    let input_svg = String::from_utf8(input.clone()).expect("corpus utf-8");
    let input_dests = count_dests(&input_svg);

    let cfg = Config::default();
    let _ = optimize(&input, &cfg).expect("warmup");
    let t = Instant::now();
    let out = optimize(&input, &cfg).unwrap_or_else(|e| panic!("{rel}: {e}"));
    let minsvg_ms = t.elapsed().as_secs_f64() * 1000.0;
    let minsvg_dests = count_dests(&out.svg);

    let svgo = if have_svgo {
        Some(run_svgo(slug, &input))
    } else {
        None
    };

    Row {
        slug: slug.to_string(),
        rel: rel.to_string(),
        input_bytes: input.len(),
        input_dests,
        minsvg_bytes: out.svg.len(),
        minsvg_ms,
        minsvg_dests,
        minsvg_motion: out.animation.motion_sensitive,
        minsvg_skipped: out
            .plugins_skipped
            .iter()
            .map(|s| (*s).to_string())
            .collect(),
        minsvg_svg: out.svg,
        svgo,
    }
}

fn run_svgo(slug: &str, input: &[u8]) -> SvgoRow {
    let runner_dir = svgo_runner_dir();
    let runner = runner_dir.join("run.mjs");
    let tmp = env::temp_dir().join(format!(
        "minsvg-parity-reallife-{}-{}",
        std::process::id(),
        slug
    ));
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
    SvgoRow {
        version,
        bytes: svg.len(),
        wall_ms,
        dests: count_dests(&svg),
        svg,
    }
}

fn render_markdown(rows: &[Row]) -> String {
    let have_svgo = rows.iter().any(|r| r.svgo.is_some());
    let mut buf = String::new();
    buf.push_str("# minSVG vs SVGO 4.1.0 (real-life corpus)\n\n");
    buf.push_str("In-process [`minsvg::optimize`] vs pinned **SVGO 4.1.0** ");
    buf.push_str("(`svgo-rust/adapters/svgo_runner`, `multipass: true` in `run.mjs`).\n\n");
    buf.push_str("This is **not** an oxvg, svgm, or vexy-vsvg claim. ");
    buf.push_str("A smaller byte count is **not more correct**.\n\n");
    buf.push_str("**Wall time:** minSVG is library-only (`std::time::Instant` around ");
    buf.push_str("`optimize`, one warmup). SVGO is a fresh `node run.mjs` spawn — ");
    buf.push_str("Node startup is included. Times are from this machine; they drift.\n\n");
    buf.push_str("**Dest-count:** destination points in `d` (M/L/H/V/C/S/Q/T/A). ");
    buf.push_str("The ocean-triangle / rust-gear guard. No resvg (MSRV 1.83).\n\n");
    buf.push_str("Files are **read-only** from sibling `svgo-rust/corpus/`. ");
    buf.push_str("The table keeps the three illustrated files and adds small ");
    buf.push_str("icons (`circle_icon.svg`, `heart.svg`, `star.svg`, `check.svg`). ");
    buf.push_str("`fun/00-driving-pov.svg` (~4 MB, 162-frame WebP movie) and ");
    buf.push_str("`large/corvus-corax-iucn.svg` (~25 MB raven map) are **not** ");
    buf.push_str("copied into this repo and are **not** in this table.\n\n");
    buf.push_str("Refresh after an intentional optimizer change:\n\n");
    buf.push_str("```\n");
    buf.push_str("UPDATE_GOLDEN=1 cargo test --test parity_reallife --offline -- --nocapture\n");
    buf.push_str("```\n\n");

    if have_svgo {
        buf.push_str(&format!(
            "- SVGO: {PINNED_SVGO} (`node adapters/svgo_runner/run.mjs --input --output`)\n"
        ));
        buf.push_str("- minSVG: `minsvg::optimize` (in-process)\n\n");
        buf.push_str("| file | in | minSVG B | SVGO B | minSVG ms | SVGO ms | dests in/minSVG/SVGO | notes |\n");
        buf.push_str("|---|---:|---:|---:|---:|---:|---:|---|\n");
        for row in rows {
            let svgo = row.svgo.as_ref().expect("svgo column");
            buf.push_str(&format!(
                "| `{}` | {} | {} | {} | {} | {} | {}/{}/{} | {} |\n",
                row.rel,
                row.input_bytes,
                row.minsvg_bytes,
                svgo.bytes,
                fmt_ms(row.minsvg_ms),
                fmt_ms(svgo.wall_ms),
                row.input_dests,
                row.minsvg_dests,
                svgo.dests,
                notes(row),
            ));
        }
    } else {
        buf.push_str("SVGO columns skipped: `node` or `adapters/svgo_runner/node_modules/svgo` ");
        buf.push_str("was missing. minSVG optimize still ran.\n\n");
        buf.push_str("| file | in | minSVG B | minSVG ms | dests in/minSVG | notes |\n");
        buf.push_str("|---|---:|---:|---:|---:|---|\n");
        for row in rows {
            buf.push_str(&format!(
                "| `{}` | {} | {} | {} | {}/{} | {} |\n",
                row.rel,
                row.input_bytes,
                row.minsvg_bytes,
                fmt_ms(row.minsvg_ms),
                row.input_dests,
                row.minsvg_dests,
                notes(row),
            ));
        }
    }
    buf.push('\n');
    buf.push_str("Negative `minSVG B − SVGO B` is a compression badge, not a ");
    buf.push_str("correctness claim. On `animation/capybara-hot-spring.svg`, ");
    buf.push_str("minSVG keeps `url(#poolFill)` / motion ids (animation-aware). ");
    buf.push_str("Stock SVGO 4.1.0 may inline CSS selectors and drop unused wraps; ");
    buf.push_str("that is smaller, not more correct.\n");
    buf
}

fn notes(row: &Row) -> String {
    match row.slug.as_str() {
        "capybara" => {
            let hat = if row.minsvg_svg.contains("M139 56 15-16")
                || row.minsvg_svg.contains("M139 56 15 -16")
            {
                "hat OFF-CANVAS"
            } else {
                "hat ticks on-canvas"
            };
            let wrap = if row.minsvg_svg.contains("unused-capy-wrap") {
                "unused wrap kept"
            } else {
                "unused wrap dropped"
            };
            format!("{hat}; {wrap}")
        }
        "rust-logo" => {
            let title = if row.minsvg_svg.contains("<title>") {
                "title+path kept"
            } else {
                "title lost"
            };
            format!("{title}; dest-count held")
        }
        "onsen" => {
            let pool = if row.minsvg_svg.contains("url(#poolFill)")
                && row.minsvg_svg.contains("id=\"poolFill\"")
            {
                "url(#poolFill) kept"
            } else {
                "url(#poolFill) LOST"
            };
            let wrap = if row.minsvg_svg.contains("id=\"unused-onsen-wrap\"") {
                "unused wrap kept (motion)"
            } else {
                "unused wrap dropped"
            };
            let svgo_note = match &row.svgo {
                Some(s) if !s.svg.contains("url(#poolFill)") => "; SVGO inlined/dropped poolFill",
                _ => "",
            };
            format!("{pool}; {wrap}{svgo_note}")
        }
        "circle-icon" => {
            let shape = if row.minsvg_svg.contains("<circle") {
                "circle kept"
            } else if row.minsvg_svg.contains("<path") {
                "circle→path"
            } else {
                "shape LOST"
            };
            let fill = if row.minsvg_svg.contains("#36f") || row.minsvg_svg.contains("#3366ff") {
                "fill #36f"
            } else {
                "fill lost"
            };
            format!("{shape}; {fill}")
        }
        "heart" => {
            let path = if row.minsvg_svg.contains("<path") {
                "path kept"
            } else {
                "path LOST"
            };
            let wrap = if row.minsvg_svg.contains("unused-heart-wrap") {
                "unused wrap kept"
            } else {
                "unused wrap dropped"
            };
            format!("{path}; {wrap}")
        }
        "star" => {
            let shape = if row.minsvg_svg.contains("<polygon") {
                "polygon kept"
            } else if row.minsvg_svg.contains("<path") {
                "polygon→path"
            } else {
                "shape LOST"
            };
            let wrap = if row.minsvg_svg.contains("unused-star-wrap") {
                "unused wrap kept"
            } else {
                "unused wrap dropped"
            };
            format!("{shape}; {wrap}")
        }
        "check" => {
            let badge = if row.minsvg_svg.contains("<circle") {
                "badge circle"
            } else if row.minsvg_svg.contains("<path") {
                "badge→path"
            } else {
                "badge LOST"
            };
            let grad = if row.minsvg_svg.contains("unused-check-grad") {
                "unused grad kept"
            } else {
                "unused grad dropped"
            };
            format!("{badge}; {grad}")
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
    assert!(
        lower.contains("poolfill") || md.contains("url(#poolFill)"),
        "results must mention url(#poolFill)"
    );
    assert!(lower.contains("dest"), "results must mention dest-count");
    for forbidden in FORBIDDEN {
        let stem = Path::new(forbidden)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or(forbidden);
        let in_table = md.lines().any(|l| {
            l.starts_with('|') && l.contains(&format!("`{forbidden}`"))
                || l.starts_with('|') && l.contains(&format!("`{stem}`"))
        });
        assert!(!in_table, "results table must not include {forbidden}");
        assert!(
            lower.contains("not")
                && (md.contains(forbidden) || lower.contains("raven") || lower.contains("pov")),
            "results must say {forbidden} is not copied / not in the table"
        );
    }
    for (_, file) in FILES {
        assert!(md.contains(file), "results must name {file}");
    }
}

fn assert_stable_columns(md: &str, rows: &[Row]) {
    for row in rows {
        let needle = format!("| `{}` |", row.rel);
        let line = md
            .lines()
            .find(|l| l.contains(&needle))
            .unwrap_or_else(|| panic!("results missing table row for {}", row.rel));
        let cells: Vec<&str> = line
            .trim_matches('|')
            .split('|')
            .map(|c| c.trim())
            .collect();
        assert!(
            cells.len() >= 3,
            "malformed results row for {}: {line}",
            row.rel
        );
        let in_b = parse_int(cells[1]);
        let rust_b = parse_int(cells[2]);
        assert_eq!(
            in_b, row.input_bytes,
            "{}: input bytes drifted in results ({in_b} vs {})",
            row.rel, row.input_bytes
        );
        assert_eq!(
            rust_b, row.minsvg_bytes,
            "{}: minSVG bytes drifted vs {} — refresh with UPDATE_GOLDEN=1",
            row.rel, row.minsvg_bytes
        );
        if row.svgo.is_some() && cells.len() >= 4 && cells[3].chars().all(|c| c.is_ascii_digit()) {
            let svgo_b = parse_int(cells[3]);
            let got = row.svgo.as_ref().unwrap().bytes;
            assert_eq!(
                svgo_b, got,
                "{}: SVGO bytes drifted vs {got} — refresh with UPDATE_GOLDEN=1",
                row.rel
            );
        }
        let dest_cell = cells.iter().rev().nth(1).copied().unwrap_or("");
        if dest_cell.contains('/') {
            let parts: Vec<&str> = dest_cell.split('/').collect();
            if parts.len() >= 2 {
                assert_eq!(
                    parse_int(parts[0]),
                    row.input_dests,
                    "{}: dests-in drifted — refresh with UPDATE_GOLDEN=1",
                    row.rel
                );
                assert_eq!(
                    parse_int(parts[1]),
                    row.minsvg_dests,
                    "{}: dests-minSVG drifted — refresh with UPDATE_GOLDEN=1",
                    row.rel
                );
            }
            if parts.len() >= 3 {
                if let Some(svgo) = &row.svgo {
                    assert_eq!(
                        parse_int(parts[2]),
                        svgo.dests,
                        "{}: dests-SVGO drifted — refresh with UPDATE_GOLDEN=1",
                        row.rel
                    );
                }
            }
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

#[derive(Debug)]
struct Row {
    slug: String,
    rel: String,
    input_bytes: usize,
    input_dests: usize,
    minsvg_bytes: usize,
    minsvg_ms: f64,
    minsvg_dests: usize,
    minsvg_motion: bool,
    minsvg_skipped: Vec<String>,
    minsvg_svg: String,
    svgo: Option<SvgoRow>,
}

#[derive(Debug)]
struct SvgoRow {
    version: String,
    bytes: usize,
    wall_ms: f64,
    dests: usize,
    svg: String,
}

fn count_dests(svg: &str) -> usize {
    attr_values(svg, "d")
        .into_iter()
        .map(|d| dest_points(&d).len())
        .sum()
}

/// Destination points. Arc flags are single 0/1 digits so `00-.0283` (rust.svg)
/// is 0, 0, −0.0283 — not the number 00.
fn dest_points(d: &str) -> Vec<(f64, f64)> {
    let b = d.as_bytes();
    let mut i = 0;
    let mut dests = Vec::new();
    let mut x = 0.0f64;
    let mut y = 0.0f64;
    let mut sx = 0.0f64;
    let mut sy = 0.0f64;
    let mut cmd = None;
    let mut implicit_lineto = false;

    while i < b.len() {
        skip_sep(b, &mut i);
        if i >= b.len() {
            break;
        }
        let c = b[i] as char;
        if c.is_ascii_alphabetic() {
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
        let Some(c) = cmd else {
            i += 1;
            continue;
        };
        let rel = c.is_ascii_lowercase();
        let kind = c.to_ascii_uppercase();
        let apply = |cur: f64, v: f64| if rel { cur + v } else { v };
        match kind {
            'M' => {
                let Some(nx) = parse_number(b, &mut i) else {
                    break;
                };
                let Some(ny) = parse_number(b, &mut i) else {
                    break;
                };
                x = apply(x, nx);
                y = apply(y, ny);
                if !implicit_lineto {
                    sx = x;
                    sy = y;
                }
                dests.push((x, y));
                cmd = Some(if rel { 'l' } else { 'L' });
                implicit_lineto = true;
            }
            'L' | 'T' => {
                let Some(nx) = parse_number(b, &mut i) else {
                    break;
                };
                let Some(ny) = parse_number(b, &mut i) else {
                    break;
                };
                x = apply(x, nx);
                y = apply(y, ny);
                dests.push((x, y));
            }
            'H' => {
                let Some(nx) = parse_number(b, &mut i) else {
                    break;
                };
                x = apply(x, nx);
                dests.push((x, y));
            }
            'V' => {
                let Some(ny) = parse_number(b, &mut i) else {
                    break;
                };
                y = apply(y, ny);
                dests.push((x, y));
            }
            'C' => {
                let Some(n) = take_nums(b, &mut i, 6) else {
                    break;
                };
                x = apply(x, n[4]);
                y = apply(y, n[5]);
                dests.push((x, y));
            }
            'S' | 'Q' => {
                let Some(n) = take_nums(b, &mut i, 4) else {
                    break;
                };
                x = apply(x, n[2]);
                y = apply(y, n[3]);
                dests.push((x, y));
            }
            'A' => {
                if parse_number(b, &mut i).is_none() || parse_number(b, &mut i).is_none() {
                    break;
                }
                if parse_number(b, &mut i).is_none() {
                    break;
                }
                if parse_flag(b, &mut i).is_none() || parse_flag(b, &mut i).is_none() {
                    break;
                }
                let Some(nx) = parse_number(b, &mut i) else {
                    break;
                };
                let Some(ny) = parse_number(b, &mut i) else {
                    break;
                };
                x = apply(x, nx);
                y = apply(y, ny);
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

fn take_nums(b: &[u8], i: &mut usize, n: usize) -> Option<Vec<f64>> {
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        out.push(parse_number(b, i)?);
    }
    Some(out)
}

fn skip_sep(b: &[u8], i: &mut usize) {
    while *i < b.len() {
        let c = b[*i];
        if c == b',' || c.is_ascii_whitespace() {
            *i += 1;
        } else {
            break;
        }
    }
}

fn parse_flag(b: &[u8], i: &mut usize) -> Option<f64> {
    skip_sep(b, i);
    if *i >= b.len() {
        return None;
    }
    match b[*i] {
        b'0' | b'1' => {
            let v = (b[*i] - b'0') as f64;
            *i += 1;
            Some(v)
        }
        _ => parse_number(b, i),
    }
}

fn parse_number(b: &[u8], i: &mut usize) -> Option<f64> {
    skip_sep(b, i);
    if *i >= b.len() {
        return None;
    }
    let start = *i;
    let c = b[*i];
    if c == b'+' || c == b'-' {
        *i += 1;
    }
    if *i >= b.len() {
        return None;
    }
    let mut saw_dot = false;
    let mut saw_digit = false;
    while *i < b.len() {
        let ch = b[*i];
        if ch.is_ascii_digit() {
            saw_digit = true;
            *i += 1;
        } else if ch == b'.' && !saw_dot {
            saw_dot = true;
            *i += 1;
        } else if ch == b'e' || ch == b'E' {
            *i += 1;
            if *i < b.len() && matches!(b[*i], b'+' | b'-') {
                *i += 1;
            }
            while *i < b.len() && b[*i].is_ascii_digit() {
                *i += 1;
            }
            break;
        } else {
            break;
        }
    }
    if !saw_digit && !saw_dot {
        *i = start;
        return None;
    }
    std::str::from_utf8(&b[start..*i]).ok()?.parse::<f64>().ok()
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

fn json_field(s: &str, key: &str) -> Option<String> {
    let pat = format!("\"{key}\":\"");
    let i = s.find(&pat)?;
    let after = &s[i + pat.len()..];
    let end = after.find('"')?;
    Some(after[..end].to_string())
}

fn results_md_path() -> PathBuf {
    if let Ok(p) = env::var("PARITY_REALLIFE_MD") {
        return PathBuf::from(p);
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/parity_reallife.md")
}

fn corpus_dir() -> Option<PathBuf> {
    if let Ok(p) = env::var("CORPUS_DIR") {
        let dir = PathBuf::from(p);
        return dir.is_dir().then_some(dir);
    }
    if let Ok(p) = env::var("SVGO_RUST_ROOT") {
        let dir = PathBuf::from(p).join("corpus");
        return dir.is_dir().then_some(dir);
    }
    let here = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    for cand in [
        here.join("../svgo-rust/corpus"),
        here.join("../../svgo-rust/corpus"),
    ] {
        if cand.is_dir() {
            return Some(cand);
        }
    }
    None
}

fn svgo_runner_dir() -> PathBuf {
    if let Ok(p) = env::var("SVGO_RUNNER") {
        return PathBuf::from(p);
    }
    if let Ok(p) = env::var("SVGO_RUST_ROOT") {
        return PathBuf::from(p).join("adapters/svgo_runner");
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
fn dest_points_hold_ocean_rect_and_hat_relative() {
    let cubics = "M 100,200 C 80,200 60,200 40,200 C 40,160 40,120 40,80 C 60,80 80,80 100,80 C 100,120 100,160 100,200";
    let hat = "m139 56-15-14m15 14V36m0 20 15-16";
    assert_eq!(dest_points(cubics).len(), 5);
    assert_eq!(dest_points(hat).len(), 6);
}

#[test]
fn dest_points_split_compact_arc_flags() {
    // rust.svg gear: `a rx ry 0 00-.0283-.2936` is flags 0,0 + dest, not number 00.
    let glued = "M23.8346 11.7033l-1.0073-.6236a13.7268 13.7268 0 00-.0283-.2936";
    assert_eq!(dest_points(glued).len(), 3, "{:?}", dest_points(glued));
    let zero_x = "M0 0a.3442.3442 0 000 .5892";
    assert_eq!(dest_points(zero_x).len(), 2, "{:?}", dest_points(zero_x));
}

#[test]
fn fixture_list_excludes_pov_and_raven() {
    for rel in FORBIDDEN {
        assert!(
            !FILES.iter().any(|(_, f)| f == rel),
            "{rel} must stay out of minSVG git"
        );
    }
    for required in [
        "fun/capybara.svg",
        "logos/rust.svg",
        "animation/capybara-hot-spring.svg",
        "circle_icon.svg",
        "heart.svg",
        "star.svg",
        "check.svg",
    ] {
        assert!(
            FILES.iter().any(|(_, f)| *f == required),
            "{required} must stay in the real-life table"
        );
    }
    assert_eq!(FILES.len(), 7);
}
