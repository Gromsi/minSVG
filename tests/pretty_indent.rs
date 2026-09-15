//! `--pretty --indent` CLI emit.
//!
//! Indent 2 vs default 4 is already covered by
//! `e2e_all_flags::cli_pretty_indents_after_compact_default`. This file
//! re-asserts indent 2 and adds the extra `--pretty --indent 0` case
//! (newlines, no leading spaces).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const ICON: &str = concat!(
    r##"<svg xmlns="http://www.w3.org/2000/svg" role="img" viewBox="0 0 24 24" width="24" height="24">"##,
    r##"<title>Keep</title>"##,
    r##"<path d="M4 12h16"/>"##,
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

#[test]
fn cli_pretty_indent_2_and_0() {
    let dir = tempfile::tempdir().unwrap();
    write_named(dir.path(), "in.svg", ICON);

    // Re-assert indent 2 (same expectation as e2e_all_flags).
    run(
        dir.path(),
        &["in.svg", "-o", "indent2.svg", "--pretty", "--indent", "2"],
    );
    let indent2 = read_out(dir.path(), "indent2.svg");
    assert!(indent2.contains("\n  <"), "{indent2}");
    assert!(!indent2.contains("\n    <"), "{indent2}");
    assert!(indent2.contains("<title>Keep</title>"), "{indent2}");

    run(
        dir.path(),
        &["in.svg", "-o", "indent0.svg", "--pretty", "--indent", "0"],
    );
    let indent0 = read_out(dir.path(), "indent0.svg");
    assert!(indent0.contains('\n'), "{indent0}");
    assert!(indent0.contains("\n<title"), "{indent0}");
    assert!(indent0.contains("\n<path"), "{indent0}");
    assert!(
        !indent0.contains("\n <"),
        "indent 0 must not pad children: {indent0}"
    );
    assert!(indent0.contains("<title>Keep</title>"), "{indent0}");
    assert!(indent0.contains("<path"), "{indent0}");
}
