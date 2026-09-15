//! Opt-in plugins stay **off** on the default path. `--plugin NAME` turns them on.

use std::fs;
use std::process::Command;

use minsvg::{
    merge_config, optimize_str, optimize_str_with, CliOverrides, Config, DEFAULT_PLUGIN_NAMES,
    OPTIN_PLUGIN_NAMES,
};

const ICON: &str = concat!(
    r##"<svg xmlns="http://www.w3.org/2000/svg" role="img" viewBox="0 0 24 24" width="24" height="24">"##,
    r##"<title>Keep</title>"##,
    r##"<path d="M4 12h16"/>"##,
    "</svg>",
);

fn minsvg() -> Command {
    Command::new(env!("CARGO_BIN_EXE_minsvg"))
}

#[test]
fn optin_names_are_not_in_the_default_set() {
    assert!(OPTIN_PLUGIN_NAMES.contains(&"removeViewBox"));
    assert!(OPTIN_PLUGIN_NAMES.contains(&"prefixIds"));
    for name in OPTIN_PLUGIN_NAMES {
        assert!(
            !DEFAULT_PLUGIN_NAMES.contains(name),
            "{name} must stay default OFF"
        );
    }
}

#[test]
fn default_optimize_keeps_viewbox() {
    let out = optimize_str(ICON).expect("optimize");
    assert!(out.svg.contains("viewBox=\"0 0 24 24\""), "{}", out.svg);
    assert!(out.svg.contains("<title>Keep</title>"), "{}", out.svg);
    assert!(out.svg.contains("role=\"img\""), "{}", out.svg);
    assert!(out.svg.contains("xmlns="), "{}", out.svg);
    assert!(!out.plugins_run.iter().any(|n| *n == "removeViewBox"));
}

#[test]
fn enable_remove_viewbox_drops_matching_viewbox() {
    let mut cfg = Config::default();
    cfg.enable_plugins.push("removeViewBox".into());
    let out = optimize_str_with(ICON, &cfg).expect("optimize --plugin removeViewBox");
    assert!(!out.svg.contains("viewBox"), "{}", out.svg);
    assert!(out.svg.contains("width=\"24\""), "{}", out.svg);
    assert!(out.svg.contains("height=\"24\""), "{}", out.svg);
    assert!(out.svg.contains("<title>Keep</title>"), "{}", out.svg);
    assert!(out.svg.contains("role=\"img\""), "{}", out.svg);
    assert!(out.plugins_run.iter().any(|n| *n == "removeViewBox"));
}

#[test]
fn skip_wins_over_plugin_remove_viewbox() {
    let cfg = merge_config(
        None,
        &CliOverrides {
            plugin: vec!["removeViewBox".into()],
            skip: vec!["removeViewBox".into()],
            ..CliOverrides::default()
        },
    );
    assert!(!cfg.plugin_enabled("removeViewBox"));
    let out = optimize_str_with(ICON, &cfg).expect("skip beats plugin");
    assert!(out.svg.contains("viewBox=\"0 0 24 24\""), "{}", out.svg);
}

#[test]
fn cli_default_keeps_viewbox() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("in.svg");
    let output = dir.path().join("out.svg");
    fs::write(&input, ICON).unwrap();
    let status = minsvg()
        .current_dir(dir.path())
        .args(["in.svg", "-o", "out.svg"])
        .status()
        .expect("spawn minsvg");
    assert!(status.success(), "minsvg default failed: {status}");
    let out = fs::read_to_string(&output).unwrap();
    assert!(out.contains("viewBox=\"0 0 24 24\""), "{out}");
    assert!(out.contains("<title>Keep</title>"), "{out}");
}

#[test]
fn cli_plugin_remove_viewbox_drops_viewbox() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("in.svg"), ICON).unwrap();
    let status = minsvg()
        .current_dir(dir.path())
        .args(["in.svg", "-o", "out.svg", "--plugin", "removeViewBox"])
        .status()
        .expect("spawn minsvg --plugin removeViewBox");
    assert!(
        status.success(),
        "minsvg --plugin removeViewBox failed: {status}"
    );
    let out = fs::read_to_string(dir.path().join("out.svg")).unwrap();
    assert!(!out.contains("viewBox"), "{out}");
    assert!(out.contains("role=\"img\""), "{out}");
    assert!(out.contains("<title>Keep</title>"), "{out}");
}

#[test]
fn cli_plugin_prefix_ids_param_prefixes() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("in.svg"),
        concat!(
            r##"<svg xmlns="http://www.w3.org/2000/svg">"##,
            r##"<defs><linearGradient id="g1"><stop offset="0" stop-color="#f00"/></linearGradient></defs>"##,
            r##"<rect fill="url(#g1)" class="ink" width="10" height="10"/>"##,
            "</svg>",
        ),
    )
    .unwrap();
    let status = minsvg()
        .current_dir(dir.path())
        .args([
            "in.svg",
            "-o",
            "out.svg",
            "--plugin",
            "prefixIds",
            "--param",
            r#"prefixIds={"prefix":"x"}"#,
        ])
        .status()
        .expect("spawn minsvg --plugin prefixIds --param");
    assert!(
        status.success(),
        "minsvg --plugin prefixIds --param prefixIds={{\"prefix\":\"x\"}} failed: {status}"
    );
    let out = fs::read_to_string(dir.path().join("out.svg")).unwrap();
    assert!(out.contains("id=\"x__g1\""), "{out}");
    assert!(out.contains("url(#x__g1)"), "{out}");
    assert!(out.contains("x__ink"), "{out}");
    assert!(!out.contains("id=\"g1\""), "{out}");

    let default = minsvg()
        .current_dir(dir.path())
        .args(["in.svg", "-o", "default.svg"])
        .status()
        .expect("spawn minsvg default");
    assert!(default.success());
    let kept = fs::read_to_string(dir.path().join("default.svg")).unwrap();
    assert!(kept.contains("id=\"g1\""), "{kept}");
    assert!(!kept.contains("x__"), "{kept}");
}

#[test]
fn cli_plugins_json_lists_optin_off_by_default() {
    let output = minsvg()
        .args(["plugins", "--json"])
        .output()
        .expect("minsvg plugins --json");
    assert!(output.status.success());
    let payload = String::from_utf8_lossy(&output.stdout);
    assert!(payload.contains("\"optin\":["), "{payload}");
    assert!(payload.contains("\"removeViewBox\""), "{payload}");
    assert!(payload.contains("\"prefixIds\""), "{payload}");
}
