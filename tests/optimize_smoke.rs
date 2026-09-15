//! Small-fixture smoke tests. No multi-megabyte corpus.

use std::fs;
use std::path::PathBuf;

use minsvg::{optimize, optimize_str, Config};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn read_fixture(name: &str) -> Vec<u8> {
    fs::read(fixture(name)).unwrap_or_else(|e| panic!("read {name}: {e}"))
}

#[test]
fn circle_icon_strips_decl_and_keeps_svg() {
    let input = read_fixture("circle_icon.svg");
    let out = optimize(&input, &Config::default()).expect("optimize");
    assert!(out.svg.contains("<svg"), "{}", out.svg);
    assert!(!out.svg.contains("<?xml"), "{}", out.svg);
    assert!(!out.svg.contains("DOCTYPE"), "{}", out.svg);
    assert!(!out.svg.contains("<!--"), "{}", out.svg);
    assert!(
        out.svg.contains("<circle") || out.svg.contains("<path"),
        "{}",
        out.svg
    );
}

#[test]
fn empty_groups_drop_and_shape_survives() {
    let input = read_fixture("empty_groups.svg");
    let out = optimize(&input, &Config::default()).expect("optimize");
    assert!(out.svg.contains("<svg"), "{}", out.svg);
    assert!(!out.svg.contains("<g"), "{}", out.svg);
    assert!(
        out.svg.contains("<circle") || out.svg.contains("<path"),
        "{}",
        out.svg
    );
}

#[test]
fn smil_hidden_frames_are_not_deleted() {
    let input = read_fixture("smil-visibility.svg");
    let out = optimize(&input, &Config::default()).expect("optimize");
    assert!(out.animation.motion_sensitive, "{:?}", out.animation);
    assert!(
        out.plugins_skipped
            .iter()
            .any(|p| *p == "removeHiddenElems"),
        "skipped={:?}",
        out.plugins_skipped
    );
    assert!(out.svg.contains("id=\"frameA\""), "{}", out.svg);
    assert!(out.svg.contains("id=\"frameB\""), "{}", out.svg);
    assert!(
        out.svg.contains("attributeName=\"visibility\""),
        "{}",
        out.svg
    );
    assert_eq!(out.svg.matches("<rect").count(), 2, "{}", out.svg);
}

#[test]
fn url_paint_id_case_is_not_folded() {
    // Tiny stand-in for onsen `url(#poolFill)` — no 8MB coat, same case assert.
    let input = concat!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10">"##,
        r##"<defs><linearGradient id="poolFill">"##,
        r##"<stop offset="0" stop-color="#fba"/></linearGradient></defs>"##,
        r##"<ellipse fill="url(#poolFill)" cx="5" cy="5" rx="4" ry="3"/>"##,
        "</svg>",
    );
    let out = optimize_str(input).expect("optimize");
    assert!(out.svg.contains("id=\"poolFill\""), "{}", out.svg);
    assert!(out.svg.contains("url(#poolFill)"), "{}", out.svg);
    assert!(!out.svg.contains("url(#poolfill)"), "{}", out.svg);
}

#[test]
fn reoptimize_is_stable_enough_to_parse() {
    let bytes = read_fixture("circle_icon.svg");
    let input = std::str::from_utf8(&bytes).unwrap();
    let once = optimize_str(input).expect("first");
    let again = optimize_str(&once.svg).expect("second");
    assert!(again.svg.contains("<svg"), "{}", again.svg);
}
