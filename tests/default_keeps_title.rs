//! Default `optimize()` keeps a11y `<title>` and `role="img"`.
//! `--plugin removeTitle` drops the title only.

use std::fs;
use std::process::Command;

use minsvg::{optimize, Config};

const ICON: &str = concat!(
    r##"<svg xmlns="http://www.w3.org/2000/svg" role="img" viewBox="0 0 24 24" width="24" height="24">"##,
    r##"<title>Keep</title>"##,
    r##"<path d="M4 12h16"/>"##,
    "</svg>",
);

fn minsvg() -> Command {
    Command::new(env!("CARGO_BIN_EXE_minsvg"))
}

fn assert_title_and_role_kept(svg: &str) {
    assert!(svg.contains("<title>Keep</title>"), "{svg}");
    assert!(svg.contains("role=\"img\""), "{svg}");
}

fn assert_title_dropped_only(svg: &str) {
    assert!(!svg.contains("<title"), "{svg}");
    assert!(!svg.contains("</title>"), "{svg}");
    assert!(!svg.contains("Keep"), "{svg}");
    assert!(svg.contains("role=\"img\""), "{svg}");
    assert!(svg.contains("viewBox=\"0 0 24 24\""), "{svg}");
    assert!(svg.contains("width=\"24\""), "{svg}");
    assert!(svg.contains("height=\"24\""), "{svg}");
    assert!(svg.contains("xmlns="), "{svg}");
    assert!(svg.contains("<path"), "{svg}");
}

#[test]
fn default_optimize_keeps_title_and_role_img() {
    let cfg = Config::default();
    assert!(cfg.enable_plugins.is_empty());
    assert!(!cfg.plugin_enabled("removeTitle"));

    let out = optimize(ICON.as_bytes(), &cfg).expect("optimize");
    assert_title_and_role_kept(&out.svg);
    assert!(!out.plugins_run.iter().any(|n| *n == "removeTitle"));
}

#[test]
fn enable_remove_title_drops_title_only() {
    let mut cfg = Config::default();
    cfg.enable_plugins.push("removeTitle".into());
    let out = optimize(ICON.as_bytes(), &cfg).expect("optimize --plugin removeTitle");
    assert_title_dropped_only(&out.svg);
    assert!(out.plugins_run.iter().any(|n| *n == "removeTitle"));
}

#[test]
fn cli_plugin_remove_title_drops_title_only() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("in.svg"), ICON).unwrap();
    let status = minsvg()
        .current_dir(dir.path())
        .args(["in.svg", "-o", "out.svg", "--plugin", "removeTitle"])
        .status()
        .expect("spawn minsvg --plugin removeTitle");
    assert!(
        status.success(),
        "minsvg --plugin removeTitle failed: {status}"
    );
    let out = fs::read_to_string(dir.path().join("out.svg")).unwrap();
    assert_title_dropped_only(&out);
}
