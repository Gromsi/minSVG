//! Wave 4 landmines — correctness that must stay green.
//!
//! Does **not** steal wave 3 bench / corpus / speed-gate files. Tiny local
//! fixtures plus inline stand-ins. Stock SVGO may delete SMIL hidden frames;
//! we do not copy that.

use std::fs;
use std::path::PathBuf;

use minsvg::{minify_path_d, optimize, optimize_str, optimize_str_with, Config};

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn fixture(name: &str) -> PathBuf {
    fixtures_dir().join(name)
}

fn read_fixture(name: &str) -> Vec<u8> {
    fs::read(fixture(name)).unwrap_or_else(|e| panic!("read {name}: {e}"))
}

fn optional_fixture(name: &str) -> Option<Vec<u8>> {
    let path = fixture(name);
    path.is_file()
        .then(|| fs::read(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display())))
}

/// Rectangle of axis-aligned cubics — same construction as the blankmap ocean.
const OCEAN_RECT: &str = "M 100,200 C 80,200 60,200 40,200 C 40,160 40,120 40,80 C 60,80 80,80 100,80 C 100,120 100,160 100,200";

/// Two-cubic top-edge excerpt that used to emit `6.408 014.1912`.
const OCEAN_TOP_EDGE: &str = "M -26.0058,61.9866 C -23.8711,61.9866 -21.7344,61.9866 -19.5978,61.9866 C -5.4066,61.9866 8.7864,61.9866 22.9794,61.9866";

fn extract_d<'a>(svg: &'a str, class: &str) -> &'a str {
    let marker = format!("class=\"{class}\"");
    let start = svg
        .find(&marker)
        .unwrap_or_else(|| panic!("missing class={class} in {svg}"));
    let tag_start = svg[..start].rfind('<').unwrap_or(0);
    let tag = &svg[tag_start..];
    let tag_end = tag.find('>').unwrap_or(tag.len());
    let tag = &tag[..tag_end];
    let d0 = tag
        .find("d=\"")
        .unwrap_or_else(|| panic!("missing d on {class}: {svg}"));
    let rest = &tag[d0 + 3..];
    let d1 = rest
        .find('"')
        .unwrap_or_else(|| panic!("unterminated d on {class}"));
    &rest[..d1]
}

/// Command-arity dest count. Enough to reject a 2–3 point ocean triangle.
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
            i += 1;
            nums = 0;
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

fn assert_no_ocean_glue(d: &str) {
    assert!(
        !d.contains("014.1912") && !d.contains("6.4080"),
        "implicit-command 0-glue in ocean: {d}"
    );
}

fn dests_in_svg(svg: &str) -> usize {
    let mut total = 0;
    let mut rest = svg;
    loop {
        let Some(i) = rest.find("d=\"") else {
            break;
        };
        let after = &rest[i + 3..];
        let Some(end) = after.find('"') else {
            break;
        };
        total += count_dest_points(&after[..end]);
        rest = &after[end + 1..];
    }
    total
}

#[test]
fn smil_hidden_frames_are_not_deleted() {
    let input = read_fixture("smil-visibility.svg");
    let out = optimize(&input, &Config::default()).expect("optimize smil-visibility");
    assert!(out.animation.motion_sensitive, "{:?}", out.animation);
    assert!(
        out.plugins_skipped
            .iter()
            .any(|p| *p == "removeHiddenElems"),
        "motion must skip removeHiddenElems: {:?}",
        out.plugins_skipped
    );
    assert!(out.svg.contains("id=\"frameA\""), "{}", out.svg);
    assert!(out.svg.contains("id=\"frameB\""), "{}", out.svg);
    assert!(
        out.svg.contains("attributeName=\"visibility\""),
        "{}",
        out.svg
    );
    assert_eq!(
        out.svg.matches("<rect").count(),
        2,
        "do not delete SMIL frames to match SVGO: {}",
        out.svg
    );
}

#[test]
fn smil_hidden_frames_survive_even_without_animation_aware() {
    // `removeHiddenElems` itself refuses SMIL visibility frames. Turning
    // animation-aware off must not copy stock SVGO's slideshow deletion.
    let bytes = read_fixture("smil-visibility.svg");
    let input = std::str::from_utf8(&bytes).expect("utf-8");
    let out = optimize_str_with(
        input,
        &Config {
            animation_aware: false,
            ..Config::default()
        },
    )
    .expect("optimize --no-animation-aware");
    assert_eq!(
        out.svg.matches("<animate").count(),
        2,
        "do not delete SMIL frames to match SVGO: {}",
        out.svg
    );
    assert_eq!(out.svg.matches("<rect").count(), 2, "{}", out.svg);
    assert!(
        out.svg.contains("attributeName=\"visibility\""),
        "{}",
        out.svg
    );
}

#[test]
fn url_id_and_poolfill_case_is_preserved() {
    let input = read_fixture("url-id.svg");
    let out = optimize(&input, &Config::default()).expect("optimize url-id");
    assert!(out.svg.contains("id=\"poolFill\""), "{}", out.svg);
    assert!(out.svg.contains("url(#poolFill)"), "{}", out.svg);
    assert!(!out.svg.contains("url(#poolfill)"), "{}", out.svg);
    assert!(out.svg.contains("id=\"waterShine\""), "{}", out.svg);
    assert!(out.svg.contains("url(#waterShine)"), "{}", out.svg);
    assert!(
        out.svg.contains("id=\"ClipMe\"") && out.svg.contains("url(#ClipMe)"),
        "clip-path url(#ClipMe) case: {}",
        out.svg
    );
    assert!(!out.svg.contains("url(#clipme)"), "{}", out.svg);
}

#[test]
fn url_mixed_case_id_is_not_folded() {
    let input = concat!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10">"##,
        r##"<defs><linearGradient id="Id">"##,
        r##"<stop offset="0" stop-color="#fba"/></linearGradient></defs>"##,
        r##"<rect fill="url( '#Id' )" width="10" height="10"/>"##,
        "</svg>",
    );
    let out = optimize_str(input).expect("optimize url(#Id)");
    assert!(out.svg.contains("id=\"Id\""), "{}", out.svg);
    assert!(
        out.svg.contains("#Id"),
        "url(#Id) fragment lost: {}",
        out.svg
    );
    assert!(
        !out.svg.contains("#id") && !out.svg.contains("#ID"),
        "url(#Id) case folded: {}",
        out.svg
    );
}

#[test]
fn required_css_px_is_not_stripped_to_unitless() {
    let input = concat!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 200 160">"##,
        r##"<style>"##,
        "#bob{transform-origin:140px 110px;font-size:12px}",
        "#z{margin:0px}",
        "</style>",
        r##"<g id="bob" style="transform-origin:140px 110px;letter-spacing:0.50px">"##,
        r##"<text>hi</text>"##,
        "</g></svg>",
    );
    let out = optimize_str(input).expect("optimize css px");
    assert!(
        out.svg.contains("transform-origin:140px 110px"),
        "unitless transform-origin is invalid CSS (0-break): {}",
        out.svg
    );
    assert!(
        !out.svg.contains("transform-origin:140 110")
            && !out.svg.contains("transform-origin:140px110px"),
        "px stripped or glued: {}",
        out.svg
    );
    assert!(
        out.svg.contains("font-size:12px") || out.svg.contains("font-size=\"12\""),
        "font-size 12px must stay CSS-valid or a presentation attr: {}",
        out.svg
    );
    if out.svg.contains("font-size:12") {
        assert!(
            out.svg.contains("font-size:12px"),
            "unitless font-size in CSS is a 0-break: {}",
            out.svg
        );
    }
    assert!(
        !out.svg.contains("margin:0px"),
        "0px may drop the unit: {}",
        out.svg
    );
}

#[test]
fn motion_css_px_origin_stays_on_floater() {
    let input = concat!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 200 160">"##,
        r##"<style>"##,
        "@keyframes bob{to{transform:translateY(-2px)}}",
        "#capyFloater{animation:bob 1s infinite;transform-origin:140px 110px}",
        "</style>",
        r##"<g id="capyFloater"><ellipse cx="140" cy="110" rx="20" ry="12">"##,
        r##"<animate attributeName="visibility" values="hidden;visible" dur="1s" repeatCount="indefinite"/>"##,
        "</ellipse></g></svg>",
    );
    let out = optimize_str(input).expect("optimize motion css px");
    assert!(out.animation.motion_sensitive, "{:?}", out.animation);
    assert!(
        out.svg.contains("transform-origin:140px 110px"),
        "onsen-style origin lost px (unitless 0-break): {}",
        out.svg
    );
    assert!(out.svg.contains("id=\"capyFloater\""), "{}", out.svg);
}

#[test]
fn dest_count_ocean_glue_zero_inline() {
    let input = format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 120 220">
<path class="ocean" fill="#fff" d="{OCEAN_RECT}"/>
</svg>"##
    );
    let out = optimize_str(&input).expect("optimize ocean rect");
    let ocean = extract_d(&out.svg, "ocean");
    assert_no_ocean_glue(ocean);
    let dests = count_dest_points(ocean);
    assert!(
        dests >= 4,
        "ocean collapsed to {dests} dest points (triangle): {ocean}"
    );

    let top = minify_path_d(OCEAN_TOP_EDGE).expect("top-edge snippet");
    assert_no_ocean_glue(&top);
    assert_eq!(
        count_dest_points(&top),
        count_dest_points(OCEAN_TOP_EDGE),
        "lost a cubic dest: {top}"
    );
}

#[test]
fn dest_count_ocean_glue_zero_if_fixture_exists() {
    let Some(input) = optional_fixture("path.svg") else {
        eprintln!("tests/fixtures/path.svg missing — skipping dest-count ocean glue-zero");
        return;
    };
    let src = std::str::from_utf8(&input).expect("utf-8");
    let out = optimize(&input, &Config::default()).expect("optimize path.svg");
    assert!(
        out.svg.contains("class=\"ocean\"") || out.svg.contains("class='ocean'"),
        "path.svg: .ocean class lost:\n{}",
        out.svg
    );
    assert_no_ocean_glue(&out.svg);
    let in_dests = dests_in_svg(src);
    let out_dests = dests_in_svg(&out.svg);
    assert!(
        in_dests >= 4 && out_dests >= 4,
        "path.svg: dest-count collapsed (ocean triangle): in={in_dests} out={out_dests}\n{}",
        out.svg
    );
    assert!(
        out_dests >= in_dests / 2,
        "path.svg: dest-count collapsed in={in_dests} out={out_dests}\n{}",
        out.svg
    );
}

#[test]
fn viewbox_kept_by_default() {
    let cfg = Config::default();
    assert!(
        !cfg.plugin_enabled("removeViewBox"),
        "removeViewBox must stay default OFF"
    );

    let icon = concat!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" role="img" viewBox="0 0 24 24" width="24" height="24">"##,
        r##"<title>Keep</title>"##,
        r##"<path d="M4 12h16"/>"##,
        "</svg>",
    );
    let out = optimize_str(icon).expect("optimize icon");
    assert!(out.svg.contains("viewBox=\"0 0 24 24\""), "{}", out.svg);
    assert!(!out.plugins_run.iter().any(|n| *n == "removeViewBox"));

    let bytes = read_fixture("circle_icon.svg");
    let circle = optimize(&bytes, &cfg).expect("optimize circle_icon");
    assert!(
        circle.svg.contains("viewBox=\"0 0 64 64\""),
        "circle_icon viewBox lost: {}",
        circle.svg
    );
}
