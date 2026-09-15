//! Actual `minsvg` CLI coverage for SVGO-shaped flags.
//!
//! Spawns the `minsvg` binary. These are not clap parse-only checks: each
//! test writes an SVG and asserts bytes / stderr. `removeViewBox` stays off
//! unless `--plugin` asks for it.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const ICON: &str = concat!(
    r##"<svg xmlns="http://www.w3.org/2000/svg" role="img" viewBox="0 0 24 24" width="24" height="24">"##,
    r##"<title>Keep</title>"##,
    r##"<path d="M4 12h16"/>"##,
    "</svg>",
);

const PRECISE_PATH: &str = concat!(
    r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10">"##,
    r##"<path d="M 1.23456 2.34567 C 3.45678 4.56789 5.67891 6.78901 7.89012 8.90123"/>"##,
    "</svg>",
);

const VERBOSE: &str = concat!(
    r##"<?xml version="1.0" encoding="UTF-8"?>"##,
    r##"<!DOCTYPE svg PUBLIC "-//W3C//DTD SVG 1.1//EN" "http://www.w3.org/Graphics/SVG/1.1/DTD/svg11.dtd">"##,
    r##"<svg xmlns="http://www.w3.org/2000/svg" version="1.1">"##,
    "<!-- leftover -->",
    r##"<metadata><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description/></rdf:RDF></metadata>"##,
    r##"<g><g id="unused"><g>"##,
    r##"<rect x="0.0000" y="0.0000" width="10.0000" height="10.0000" fill="#000000" class="" style="  fill-opacity: 1 ;  "/>"##,
    r##"</g></g></g></svg>"##,
);

const COMMENTED: &str = concat!(
    r##"<svg xmlns="http://www.w3.org/2000/svg">"##,
    "<!-- keep -->",
    r##"<rect width="10" height="10"/>"##,
    "</svg>",
);

const MARKED: &str = concat!(
    r##"<svg xmlns="http://www.w3.org/2000/svg">"##,
    r##"<defs><linearGradient id="g1"><stop offset="0" stop-color="#f00"/></linearGradient></defs>"##,
    r##"<rect fill="url(#g1)" class="ink" width="10" height="10"/>"##,
    "</svg>",
);

const LIST: &str = concat!(
    r##"<svg xmlns="http://www.w3.org/2000/svg">"##,
    r##"<polyline points="0.5000 1.2349"/>"##,
    r##"<polygon points="0pt,12pt"/>"##,
    "</svg>",
);

fn minsvg() -> Command {
    Command::new(env!("CARGO_BIN_EXE_minsvg"))
}

fn write_named(dir: &Path, name: &str, body: &str) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, body).unwrap();
    path
}

fn run(dir: &Path, args: &[&str]) -> Output {
    let output = minsvg()
        .current_dir(dir)
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("spawn minsvg {args:?}: {e}"));
    assert!(
        output.status.success(),
        "minsvg {args:?} failed: {}\nstdout:{}\nstderr:{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

fn read_out(dir: &Path, name: &str) -> String {
    fs::read_to_string(dir.join(name)).unwrap()
}

fn read_bytes(dir: &Path, name: &str) -> Vec<u8> {
    fs::read(dir.join(name)).unwrap()
}

fn assert_viewbox_kept(svg: &str) {
    assert!(svg.contains("viewBox=\"0 0 24 24\""), "{svg}");
    assert!(svg.contains("<title>Keep</title>"), "{svg}");
    assert!(svg.contains("role=\"img\""), "{svg}");
    assert!(svg.contains("width=\"24\""), "{svg}");
    assert!(svg.contains("height=\"24\""), "{svg}");
}

fn assert_viewbox_dropped(svg: &str) {
    assert!(!svg.contains("viewBox"), "{svg}");
    assert!(svg.contains("width=\"24\""), "{svg}");
    assert!(svg.contains("height=\"24\""), "{svg}");
    assert!(svg.contains("<title>Keep</title>"), "{svg}");
    assert!(svg.contains("role=\"img\""), "{svg}");
}

fn has_bare_lf(bytes: &[u8]) -> bool {
    bytes
        .iter()
        .enumerate()
        .any(|(i, &b)| b == b'\n' && (i == 0 || bytes[i - 1] != b'\r'))
}

#[test]
fn cli_default_keeps_viewbox() {
    let dir = tempfile::tempdir().unwrap();
    write_named(dir.path(), "in.svg", ICON);
    run(dir.path(), &["in.svg", "-o", "out.svg"]);
    assert_viewbox_kept(&read_out(dir.path(), "out.svg"));
}

#[test]
fn cli_plugin_remove_viewbox_drops_viewbox() {
    let dir = tempfile::tempdir().unwrap();
    write_named(dir.path(), "in.svg", ICON);
    run(
        dir.path(),
        &["in.svg", "-o", "out.svg", "--plugin", "removeViewBox"],
    );
    assert_viewbox_dropped(&read_out(dir.path(), "out.svg"));
}

#[test]
fn cli_skip_beats_plugin_and_keeps_comments() {
    let dir = tempfile::tempdir().unwrap();
    write_named(dir.path(), "in.svg", ICON);
    write_named(dir.path(), "comment.svg", COMMENTED);

    run(
        dir.path(),
        &[
            "in.svg",
            "-o",
            "skip-vb.svg",
            "--plugin",
            "removeViewBox",
            "--skip",
            "removeViewBox",
        ],
    );
    assert_viewbox_kept(&read_out(dir.path(), "skip-vb.svg"));

    run(dir.path(), &["comment.svg", "-o", "dropped.svg"]);
    let dropped = read_out(dir.path(), "dropped.svg");
    assert!(!dropped.contains("keep"), "{dropped}");
    assert!(!dropped.contains("<!--"), "{dropped}");

    run(
        dir.path(),
        &["comment.svg", "-o", "kept.svg", "--skip", "removeComments"],
    );
    let kept = read_out(dir.path(), "kept.svg");
    assert!(kept.contains("keep"), "{kept}");
    assert!(kept.contains("<!--"), "{kept}");
}

#[test]
fn cli_param_sets_plugin_json_but_does_not_enable() {
    let dir = tempfile::tempdir().unwrap();
    write_named(dir.path(), "in.svg", ICON);
    write_named(dir.path(), "marked.svg", MARKED);
    write_named(dir.path(), "list.svg", LIST);

    run(
        dir.path(),
        &[
            "in.svg",
            "-o",
            "param-only.svg",
            "--param",
            r#"removeViewBox={"fake":true}"#,
        ],
    );
    assert_viewbox_kept(&read_out(dir.path(), "param-only.svg"));

    run(
        dir.path(),
        &[
            "marked.svg",
            "-o",
            "prefixed.svg",
            "--plugin",
            "prefixIds",
            "--param",
            r#"prefixIds={"prefix":"x"}"#,
        ],
    );
    let prefixed = read_out(dir.path(), "prefixed.svg");
    assert!(prefixed.contains("id=\"x__g1\""), "{prefixed}");
    assert!(prefixed.contains("url(#x__g1)"), "{prefixed}");
    assert!(prefixed.contains("x__ink"), "{prefixed}");
    assert!(!prefixed.contains("id=\"g1\""), "{prefixed}");

    run(
        dir.path(),
        &[
            "list.svg",
            "-o",
            "list.svg.out",
            "--plugin",
            "cleanupListOfValues",
            "--param",
            r#"cleanupListOfValues={"floatPrecision":2}"#,
        ],
    );
    let list = read_out(dir.path(), "list.svg.out");
    assert!(list.contains("points=\".5 1.23\""), "{list}");
    assert!(
        list.contains("points=\"0 16\"") || list.contains("16"),
        "{list}"
    );
    assert!(!list.contains("12pt"), "{list}");
}

#[test]
fn cli_precision_rounds_path_d() {
    let dir = tempfile::tempdir().unwrap();
    write_named(dir.path(), "in.svg", PRECISE_PATH);

    run(dir.path(), &["in.svg", "-o", "lossless.svg"]);
    let lossless = read_out(dir.path(), "lossless.svg");
    assert!(lossless.contains("1.23456"), "{lossless}");
    assert!(lossless.contains("<path"), "{lossless}");

    run(dir.path(), &["in.svg", "-o", "p1.svg", "--precision", "1"]);
    let p1 = read_out(dir.path(), "p1.svg");
    assert!(!p1.contains("1.23456"), "{p1}");
    assert!(p1.contains("1.2"), "{p1}");
    assert!(p1.contains("<path"), "{p1}");
    assert!(!p1.contains("014.1912"), "{p1}");

    run(dir.path(), &["in.svg", "-o", "p2.svg", "--precision", "2"]);
    run(dir.path(), &["in.svg", "-o", "p3.svg", "--precision", "3"]);
    let p2 = read_out(dir.path(), "p2.svg");
    let p3 = read_out(dir.path(), "p3.svg");
    assert!(p2.contains("1.23"), "{p2}");
    assert!(!p2.contains("1.23456"), "{p2}");
    assert!(
        p2.len() <= p3.len(),
        "precision 2 ({}B) > precision 3 ({}B)\n{p2}\n{p3}",
        p2.len(),
        p3.len()
    );
    assert!(
        p2.len() < p3.len(),
        "expected precision 2 smaller than 3: {p2} vs {p3}"
    );
}

#[test]
fn cli_pretty_indents_after_compact_default() {
    let dir = tempfile::tempdir().unwrap();
    write_named(dir.path(), "in.svg", ICON);

    run(dir.path(), &["in.svg", "-o", "compact.svg"]);
    let compact = read_out(dir.path(), "compact.svg");
    assert!(compact.contains("<svg"), "{compact}");
    assert!(
        !compact.contains("\n    <") && !compact.contains("\n  <"),
        "default emit must stay compact: {compact}"
    );

    run(dir.path(), &["in.svg", "-o", "pretty.svg", "--pretty"]);
    let pretty = read_out(dir.path(), "pretty.svg");
    assert!(pretty.contains('\n'), "{pretty}");
    assert!(pretty.contains("\n    <"), "default indent 4: {pretty}");
    assert_viewbox_kept(&pretty);

    run(
        dir.path(),
        &["in.svg", "-o", "indent2.svg", "--pretty", "--indent", "2"],
    );
    let indent2 = read_out(dir.path(), "indent2.svg");
    assert!(indent2.contains("\n  <"), "{indent2}");
    assert!(!indent2.contains("\n    <"), "{indent2}");
}

#[test]
fn cli_datauri_base64_enc_unenc() {
    let dir = tempfile::tempdir().unwrap();
    write_named(dir.path(), "in.svg", ICON);

    run(dir.path(), &["in.svg", "-o", "b64.txt", "--datauri"]);
    let b64 = read_out(dir.path(), "b64.txt");
    assert!(b64.starts_with("data:image/svg+xml;base64,"), "{b64}");
    assert!(!b64.contains("<svg"), "{b64}");

    run(
        dir.path(),
        &["in.svg", "-o", "b64-named.txt", "--datauri", "base64"],
    );
    let named = read_out(dir.path(), "b64-named.txt");
    assert_eq!(named, b64);

    run(dir.path(), &["in.svg", "-o", "enc.txt", "--datauri", "enc"]);
    let enc = read_out(dir.path(), "enc.txt");
    assert!(enc.starts_with("data:image/svg+xml,"), "{enc}");
    assert!(enc.contains("%3Csvg"), "{enc}");

    run(
        dir.path(),
        &["in.svg", "-o", "unenc.txt", "--datauri", "unenc"],
    );
    let unenc = read_out(dir.path(), "unenc.txt");
    assert!(unenc.starts_with("data:image/svg+xml,"), "{unenc}");
    assert!(unenc.contains("<svg"), "{unenc}");
    assert_viewbox_kept(unenc.strip_prefix("data:image/svg+xml,").unwrap_or(&unenc));
}

#[test]
fn cli_quiet_suppresses_stderr_summary() {
    let dir = tempfile::tempdir().unwrap();
    write_named(dir.path(), "in.svg", ICON);

    let loud = run(dir.path(), &["in.svg", "-o", "loud.svg"]);
    let loud_err = String::from_utf8_lossy(&loud.stderr);
    assert!(loud_err.contains("optimized"), "{loud_err}");
    assert!(loud_err.contains("in.svg"), "{loud_err}");
    assert!(loud.stdout.is_empty(), "{:?}", loud.stdout);
    assert_viewbox_kept(&read_out(dir.path(), "loud.svg"));

    let quiet = run(dir.path(), &["in.svg", "-o", "quiet.svg", "--quiet"]);
    assert!(
        quiet.stderr.is_empty(),
        "stderr={}",
        String::from_utf8_lossy(&quiet.stderr)
    );
    assert_viewbox_kept(&read_out(dir.path(), "quiet.svg"));

    let report_quiet = run(
        dir.path(),
        &["in.svg", "-o", "rq.svg", "--report", "--quiet"],
    );
    assert!(
        report_quiet.stderr.is_empty(),
        "stderr={}",
        String::from_utf8_lossy(&report_quiet.stderr)
    );
}

#[test]
fn cli_eol_lf_and_crlf() {
    let dir = tempfile::tempdir().unwrap();
    write_named(dir.path(), "in.svg", ICON);

    run(
        dir.path(),
        &["in.svg", "-o", "lf.svg", "--pretty", "--eol", "lf"],
    );
    let lf = read_bytes(dir.path(), "lf.svg");
    assert!(lf.contains(&b'\n'), "{lf:?}");
    assert!(!lf.contains(&b'\r'), "unexpected CR after --eol lf: {lf:?}");
    assert_viewbox_kept(&String::from_utf8(lf).unwrap());

    run(
        dir.path(),
        &["in.svg", "-o", "crlf.svg", "--pretty", "--eol", "crlf"],
    );
    let crlf = read_bytes(dir.path(), "crlf.svg");
    assert!(
        crlf.windows(2).any(|w| w == b"\r\n"),
        "missing CRLF: {crlf:?}"
    );
    assert!(!has_bare_lf(&crlf), "bare LF after --eol crlf: {crlf:?}");
    let crlf_text = String::from_utf8(crlf).unwrap();
    assert_viewbox_kept(&crlf_text);

    run(
        dir.path(),
        &[
            "in.svg",
            "-o",
            "uri.txt",
            "--pretty",
            "--datauri",
            "unenc",
            "--eol",
            "crlf",
        ],
    );
    let uri = read_bytes(dir.path(), "uri.txt");
    let uri_text = String::from_utf8_lossy(&uri);
    assert!(uri_text.starts_with("data:image/svg+xml,"), "{uri_text}");
    assert!(
        !uri.windows(2).any(|w| w == b"\r\n"),
        "data URI must skip --eol conversion: {uri:?}"
    );
}

#[test]
fn cli_multipass_is_not_larger_than_one_pass() {
    let dir = tempfile::tempdir().unwrap();
    write_named(dir.path(), "in.svg", VERBOSE);

    run(dir.path(), &["in.svg", "-o", "one.svg"]);
    run(dir.path(), &["in.svg", "-o", "multi.svg", "--multipass"]);
    let one = read_out(dir.path(), "one.svg");
    let multi = read_out(dir.path(), "multi.svg");
    assert!(one.contains("<svg"), "{one}");
    assert!(multi.contains("<svg"), "{multi}");
    assert!(
        multi.contains("<rect") || multi.contains("<path"),
        "{multi}"
    );
    assert!(
        multi.len() <= one.len(),
        "multipass {}B > one-pass {}B\nmulti={multi}\none={one}",
        multi.len(),
        one.len()
    );
}
