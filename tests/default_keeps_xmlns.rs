//! Default `optimize()` keeps the default `xmlns`.
//! `--plugin removeXMLNS` drops default xmlns only.

use std::fs;
use std::process::Command;

use minsvg::{optimize, Config};

/// Live `xlink:href` so default `removeUnusedNS` cannot drop `xmlns:xlink`.
const ICON: &str = concat!(
    r##"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" role="img" viewBox="0 0 24 24" width="24" height="24">"##,
    r##"<title>Keep</title>"##,
    r##"<defs><path id="p" d="M4 12h16"/></defs>"##,
    r##"<use xlink:href="#p"/>"##,
    "</svg>",
);

fn minsvg() -> Command {
    Command::new(env!("CARGO_BIN_EXE_minsvg"))
}

fn assert_default_xmlns_kept(svg: &str) {
    assert!(
        svg.contains(r#"xmlns="http://www.w3.org/2000/svg""#),
        "{svg}"
    );
    assert!(svg.contains("xmlns:xlink"), "{svg}");
    assert!(svg.contains("role=\"img\""), "{svg}");
}

fn assert_default_xmlns_dropped_only(svg: &str) {
    assert!(
        !svg.contains(r#"xmlns="http://www.w3.org/2000/svg""#),
        "{svg}"
    );
    assert!(!svg.contains(" xmlns="), "{svg}");
    assert!(svg.contains("xmlns:xlink"), "{svg}");
    assert!(svg.contains("xlink:href"), "{svg}");
    assert!(svg.contains("role=\"img\""), "{svg}");
    assert!(svg.contains("viewBox=\"0 0 24 24\""), "{svg}");
    assert!(svg.contains("width=\"24\""), "{svg}");
    assert!(svg.contains("height=\"24\""), "{svg}");
    assert!(svg.contains("<title>Keep</title>"), "{svg}");
    assert!(svg.contains("<path"), "{svg}");
}

#[test]
fn default_optimize_keeps_xmlns() {
    let cfg = Config::default();
    assert!(cfg.enable_plugins.is_empty());
    assert!(!cfg.plugin_enabled("removeXMLNS"));

    let out = optimize(ICON.as_bytes(), &cfg).expect("optimize");
    assert_default_xmlns_kept(&out.svg);
    assert!(!out.plugins_run.iter().any(|n| *n == "removeXMLNS"));
}

#[test]
fn enable_remove_xmlns_drops_default_xmlns_only() {
    let mut cfg = Config::default();
    cfg.enable_plugins.push("removeXMLNS".into());
    let out = optimize(ICON.as_bytes(), &cfg).expect("optimize --plugin removeXMLNS");
    assert_default_xmlns_dropped_only(&out.svg);
    assert!(out.plugins_run.iter().any(|n| *n == "removeXMLNS"));
}

#[test]
fn cli_plugin_remove_xmlns_drops_default_xmlns_only() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("in.svg"), ICON).unwrap();
    let status = minsvg()
        .current_dir(dir.path())
        .args(["in.svg", "-o", "out.svg", "--plugin", "removeXMLNS"])
        .status()
        .expect("spawn minsvg --plugin removeXMLNS");
    assert!(
        status.success(),
        "minsvg --plugin removeXMLNS failed: {status}"
    );
    let out = fs::read_to_string(dir.path().join("out.svg")).unwrap();
    assert_default_xmlns_dropped_only(&out);
}
