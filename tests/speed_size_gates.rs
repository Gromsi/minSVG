//! Speed and size gates for in-process [`minsvg::optimize`].
//!
//! A small icon must finish under a generous wall budget (50 ms after one
//! warmup). Debug builds that miss that budget skip the timing assert — dest
//! count and UTF-8 size vs the original still must not explode.
//!
//! This file does **not** spawn SVGO or Node. Node startup on
//! `adapters/svgo_runner` is routinely slower than 50 ms; a slower Node
//! process must never fail these gates. MSRV rustc 1.83.

use minsvg::{optimize, Config};
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// Generous in-process budget. Debug is allowed to skip if it still overruns.
const BUDGET: Duration = Duration::from_millis(50);
/// Output UTF-8 length may not exceed this multiple of the original.
const SIZE_CAP: usize = 2;
/// Path dest-count may not exceed this multiple of the original (plus slack
/// so a circle that later becomes a short path is not a false explode).
const DEST_CAP: usize = 2;
const DEST_SLACK: usize = 4;

const ICON: &str = "circle_icon.svg";
const PATH: &str = "path.svg";

#[test]
fn small_icon_optimize_finishes_under_budget() {
    let input = read_fixture(ICON);
    let out = optimize_timed(ICON, &input);
    assert_not_exploding(ICON, &input, &out.svg);
    assert!(
        out.svg.contains("<circle") || out.svg.contains("<path"),
        "{ICON}: painted shape lost:\n{}",
        out.svg
    );
}

#[test]
fn path_icon_dest_count_and_size_do_not_explode() {
    let input = read_fixture(PATH);
    let out = optimize_timed(PATH, &input);
    assert_not_exploding(PATH, &input, &out.svg);
    let in_dests = dests_in_svg(std::str::from_utf8(&input).expect("utf-8"));
    let out_dests = dests_in_svg(&out.svg);
    assert!(
        in_dests >= 4 && out_dests >= 4,
        "{PATH}: dest-count collapsed (ocean triangle): in={in_dests} out={out_dests}\n{}",
        out.svg
    );
    assert!(
        out.svg.contains("class=\"ocean\"") || out.svg.contains("class='ocean'"),
        "{PATH}: .ocean class lost:\n{}",
        out.svg
    );
}

#[test]
fn dest_count_helper_sees_ocean_rectangle() {
    let cubics = "M 100,200 C 80,200 60,200 40,200 C 40,160 40,120 40,80 \
         C 60,80 80,80 100,80 C 100,120 100,160 100,200";
    let hv = "M100 200H40V80h60v120";
    assert_eq!(count_dest_points(cubics), 5, "{cubics}");
    assert_eq!(count_dest_points(hv), 5, "{hv}");
}

fn optimize_timed(slug: &str, input: &[u8]) -> minsvg::OptimizeOutput {
    let cfg = Config::default();
    let _ = optimize(input, &cfg).unwrap_or_else(|e| panic!("{slug} warmup: {e}"));

    let t = Instant::now();
    let out = optimize(input, &cfg).unwrap_or_else(|e| panic!("{slug}: {e}"));
    let elapsed = t.elapsed();
    eprintln!(
        "{slug}: optimize {elapsed:?} in={}B out={}B dests {}→{}",
        input.len(),
        out.svg.len(),
        dests_in_svg(std::str::from_utf8(input).expect("utf-8")),
        dests_in_svg(&out.svg)
    );

    if elapsed > BUDGET {
        if cfg!(debug_assertions) {
            eprintln!(
                "{slug}: debug optimize took {elapsed:?} > {BUDGET:?} — \
                 skipping speed gate; dest-count/size still apply"
            );
        } else {
            panic!("{slug}: optimize() took {elapsed:?} (release budget {BUDGET:?})");
        }
    }
    out
}

fn assert_not_exploding(slug: &str, input: &[u8], svg: &str) {
    assert!(svg.contains("<svg"), "{slug}: lost <svg:\n{svg}");
    assert!(
        !svg.contains("014.1912"),
        "{slug}: dest-count / digit glue:\n{svg}"
    );
    assert!(
        !svg.is_empty() && svg.len() <= input.len().saturating_mul(SIZE_CAP),
        "{slug}: size exploded {}B vs original {}B",
        svg.len(),
        input.len()
    );

    let in_dests = dests_in_svg(std::str::from_utf8(input).expect("utf-8"));
    let out_dests = dests_in_svg(svg);
    let dest_cap = in_dests
        .saturating_mul(DEST_CAP)
        .max(in_dests.saturating_add(DEST_SLACK));
    assert!(
        out_dests <= dest_cap,
        "{slug}: dest-count exploded in={in_dests} out={out_dests} cap={dest_cap}\n{svg}"
    );
    if in_dests >= DEST_SLACK {
        let dest_floor = in_dests / DEST_CAP;
        assert!(
            out_dests >= dest_floor,
            "{slug}: dest-count collapsed in={in_dests} out={out_dests}\n{svg}"
        );
    }
}

fn dests_in_svg(svg: &str) -> usize {
    attr_values(svg, "d")
        .into_iter()
        .map(|d| count_dest_points(&d))
        .sum()
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
        let Some(end) = after.find(quote) else {
            break;
        };
        out.push(after[..end].to_string());
        rest = &after[end + 1..];
    }
    out
}

/// Command-arity dest count (same idea as `path_fidelity`). Enough to reject
/// an exploding rewrite or a 2–3 point ocean triangle.
fn count_dest_points(d: &str) -> usize {
    let mut cmd = None;
    let mut nums = 0usize;
    let mut dests = 0usize;
    let flush = |cmd: Option<char>, nums: usize, dests: &mut usize| {
        let Some(c) = cmd else {
            return;
        };
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

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn read_fixture(name: &str) -> Vec<u8> {
    fs::read(fixture(name)).unwrap_or_else(|e| panic!("read {name}: {e}"))
}
