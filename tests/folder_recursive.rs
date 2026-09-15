//! CLI folder mode: `-f` writes one output per `*.svg`; `--recursive` walks subdirs.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn minsvg() -> Command {
    Command::new(env!("CARGO_BIN_EXE_minsvg"))
}

fn run(args: &[&str]) -> Output {
    let output = minsvg()
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

fn icon(mark: &str) -> String {
    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><title>{mark}</title><rect width="10" height="10"/></svg>"##
    )
}

fn write_svg(dir: &Path, name: &str, mark: &str) -> PathBuf {
    let path = dir.join(name);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(&path, icon(mark)).unwrap();
    path
}

fn assert_written_svg(path: &Path, mark: &str) {
    let svg = fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    assert!(
        svg.contains("<svg"),
        "{} is not SVG: {svg}",
        path.display()
    );
    assert!(
        svg.contains(mark),
        "{} missing title {mark:?}: {svg}",
        path.display()
    );
}

#[test]
fn cli_folder_two_svgs_writes_two_outputs() {
    let root = tempfile::tempdir().unwrap();
    let input = root.path().join("in");
    let output = root.path().join("out");
    fs::create_dir_all(&input).unwrap();
    write_svg(&input, "a.svg", "alpha");
    write_svg(&input, "b.svg", "bravo");
    fs::write(input.join("readme.txt"), "not svg").unwrap();

    run(&[
        "-f",
        input.to_str().unwrap(),
        "-o",
        output.to_str().unwrap(),
    ]);

    assert_written_svg(&output.join("a.svg"), "alpha");
    assert_written_svg(&output.join("b.svg"), "bravo");
    assert!(
        !output.join("readme.txt").exists(),
        "folder mode must skip non-svg files"
    );
}

#[test]
fn cli_folder_recursive_writes_nested_svg() {
    let root = tempfile::tempdir().unwrap();
    let input = root.path().join("in");
    let flat = root.path().join("out-flat");
    let deep = root.path().join("out-deep");
    fs::create_dir_all(input.join("nested")).unwrap();
    write_svg(&input, "a.svg", "alpha");
    write_svg(&input, "b.svg", "bravo");
    write_svg(&input, "nested/c.svg", "charlie");

    run(&[
        "-f",
        input.to_str().unwrap(),
        "-o",
        flat.to_str().unwrap(),
    ]);
    assert_written_svg(&flat.join("a.svg"), "alpha");
    assert_written_svg(&flat.join("b.svg"), "bravo");
    assert!(
        !flat.join("nested").join("c.svg").exists(),
        "non-recursive -f must skip nested/*.svg"
    );

    run(&[
        "-f",
        input.to_str().unwrap(),
        "-o",
        deep.to_str().unwrap(),
        "--recursive",
    ]);
    assert_written_svg(&deep.join("a.svg"), "alpha");
    assert_written_svg(&deep.join("b.svg"), "bravo");
    assert_written_svg(&deep.join("nested").join("c.svg"), "charlie");
}
