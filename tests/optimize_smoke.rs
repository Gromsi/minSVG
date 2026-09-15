//! Small-fixture smoke tests. No multi-megabyte corpus.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

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
    assert!(
        out.plugins_skipped.iter().any(|p| *p == "inlineStyles"),
        "motion must skip inlineStyles: {:?}",
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
fn default_fill_rule_drops_but_role_viewbox_title_xmlns_stay() {
    let input = concat!(
        r##"<svg role="img" viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg">"##,
        r##"<title>Mark</title>"##,
        r##"<path d="M4 4h8v8H4z" fill-rule="nonzero" opacity="1"/>"##,
        "</svg>",
    );
    let out = optimize_str(input).expect("optimize");
    assert!(out.svg.contains("role=\"img\""), "{}", out.svg);
    assert!(out.svg.contains("viewBox=\"0 0 24 24\""), "{}", out.svg);
    assert!(out.svg.contains("xmlns="), "{}", out.svg);
    assert!(out.svg.contains("<title>Mark</title>"), "{}", out.svg);
    assert!(!out.svg.contains("fill-rule"), "{}", out.svg);
    assert!(!out.svg.contains("opacity="), "{}", out.svg);
}

#[test]
fn static_class_style_inlines() {
    let input = concat!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10">"##,
        r##"<style>.ink{fill:#ff0000}rect{stroke:#0000ff}</style>"##,
        r##"<rect class="ink" width="10" height="10"/>"##,
        "</svg>",
    );
    let out = optimize_str(input).expect("optimize");
    assert!(!out.animation.motion_sensitive, "{:?}", out.animation);
    assert!(
        !out.plugins_skipped.iter().any(|p| *p == "inlineStyles"),
        "static icon must run inlineStyles: {:?}",
        out.plugins_skipped
    );
    assert!(
        !out.svg.contains(".ink{") && !out.svg.contains(".ink {"),
        "once-matched class should inline: {}",
        out.svg
    );
    assert!(
        out.svg.contains("fill:red")
            || out.svg.contains("fill=\"red\"")
            || out.svg.contains("fill=\"#f00\"")
            || out.svg.contains("fill:#f00")
            || out.svg.contains("fill:#ff0000"),
        "{}",
        out.svg
    );
    assert!(
        out.svg.contains("stroke")
            && (out.svg.contains("blue")
                || out.svg.contains("#00f")
                || out.svg.contains("#0000ff")),
        "{}",
        out.svg
    );
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

#[test]
fn multipass_verbose_svg_is_not_larger_than_one_pass() {
    let input = concat!(
        r##"<?xml version="1.0" encoding="UTF-8"?>"##,
        r##"<!DOCTYPE svg PUBLIC "-//W3C//DTD SVG 1.1//EN" "http://www.w3.org/Graphics/SVG/1.1/DTD/svg11.dtd">"##,
        r##"<svg xmlns="http://www.w3.org/2000/svg" version="1.1">"##,
        "<!-- leftover -->",
        r##"<metadata><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description/></rdf:RDF></metadata>"##,
        r##"<g><g id="unused"><g>"##,
        r##"<rect x="0.0000" y="0.0000" width="10.0000" height="10.0000" fill="#000000" class="" style="  fill-opacity: 1 ;  "/>"##,
        r##"</g></g></g></svg>"##,
    );
    let one = optimize_str(input).expect("one pass");
    let multi = optimize(
        input.as_bytes(),
        &Config {
            multipass: true,
            ..Config::default()
        },
    )
    .expect("multipass");
    assert!(
        multi.svg.len() <= one.svg.len(),
        "multipass {}B > one-pass {}B\nmulti={}\none={}",
        multi.svg.len(),
        one.svg.len(),
        multi.svg,
        one.svg
    );
    assert!(multi.svg.contains("<svg"), "{}", multi.svg);
}

#[test]
fn cargo_default_features_exclude_serve_and_mcp() {
    let toml = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"));
    let features = toml.split("[features]").nth(1).expect("[features]");
    let default_line = features
        .lines()
        .map(str::trim)
        .find(|l| l.starts_with("default"))
        .expect("default =");
    assert!(
        default_line.contains("[]")
            && !default_line.contains("serve")
            && !default_line.contains("mcp"),
        "default features must stay empty so minsvg does not link HTTP/MCP: {default_line}"
    );
}

#[test]
fn default_minsvg_bin_does_not_link_http_serve() {
    if cfg!(feature = "serve") {
        return;
    }
    let bin = env!("CARGO_BIN_EXE_minsvg");
    let out = Command::new(bin)
        .args(["serve", "--bind", "127.0.0.1:0"])
        .output()
        .expect("spawn minsvg");
    assert!(!out.status.success(), "default minsvg serve must fail");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("without HTTP serve"), "{err}");
    assert!(err.contains("--features serve"), "{err}");
}

#[test]
fn default_cargo_test_does_not_build_minsvg_mcp() {
    let exe = option_env!("CARGO_BIN_EXE_minsvg_mcp");
    if cfg!(feature = "mcp") {
        // Cargo 1.83 injects CARGO_BIN_EXE_* into some integration crates
        // (e.g. tests/mcp_stdio.rs) but not all. The sibling next to
        // `minsvg` is the same artifact `required-features = ["mcp"]` built.
        let path = match exe {
            Some(p) => PathBuf::from(p),
            None => {
                let mut p = PathBuf::from(env!("CARGO_BIN_EXE_minsvg"));
                p.set_file_name(format!("minsvg-mcp{}", std::env::consts::EXE_SUFFIX));
                p
            }
        };
        assert!(
            path.is_file(),
            "minsvg-mcp must exist when --features mcp is on: {}",
            path.display()
        );
        return;
    }
    assert!(
        exe.is_none(),
        "minsvg-mcp must not be built into a default cargo test"
    );
}
