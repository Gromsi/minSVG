//! Leftover SVGO 4.1.0 built-in plugin IDs = **0**.
//!
//! Ground truth: [svgo@4.1.0](https://github.com/svg/svgo/releases/tag/v4.1.0)
//! `plugins/preset-default.js` (34) + `lib/builtin.js` (those 34 + 19 opt-ins +
//! the `preset-default` bundle). The bundle is not a plugin implementation.
//!
//! The 19 opt-ins (default **OFF**; `--plugin NAME` or a public `run_*`):
//! `removeViewBox`, `removeTitle`, `removeDimensions`, `removeXMLNS`,
//! `removeXlink`, `removeScripts`, `removeStyleElement`, `removeRasterImages`,
//! `removeOffCanvasPaths`, `prefixIds`, `reusePaths`, `convertOneStopGradients`,
//! `convertStyleToAttrs`, `cleanupListOfValues`, `addAttributesToSVGElement`,
//! `addClassesToSVGElement`, `removeAttrs`, `removeAttributesBySelector`,
//! `removeElementsByAttr`.

use std::collections::BTreeSet;
use std::fs;
use std::process::Command;

use minsvg::{
    apply_plugin_cli, is_optin_plugin, optimize_str, parse_plugin_spec, Config,
    DEFAULT_PLUGIN_NAMES, OPTIN_PLUGIN_NAMES,
};

/// 4.1.0 `plugins/preset-default.js` source order.
const SVGO_410_PRESET_DEFAULT: &[&str] = &[
    "removeDoctype",
    "removeXMLProcInst",
    "removeComments",
    "removeDeprecatedAttrs",
    "removeMetadata",
    "removeEditorsNSData",
    "cleanupAttrs",
    "mergeStyles",
    "inlineStyles",
    "minifyStyles",
    "cleanupIds",
    "removeUselessDefs",
    "cleanupNumericValues",
    "convertColors",
    "removeUnknownsAndDefaults",
    "removeNonInheritableGroupAttrs",
    "removeUselessStrokeAndFill",
    "cleanupEnableBackground",
    "removeHiddenElems",
    "removeEmptyText",
    "convertShapeToPath",
    "convertEllipseToCircle",
    "moveElemsAttrsToGroup",
    "moveGroupAttrsToElems",
    "collapseGroups",
    "convertPathData",
    "convertTransform",
    "removeEmptyAttrs",
    "removeEmptyContainers",
    "mergePaths",
    "removeUnusedNS",
    "sortAttrs",
    "sortDefsChildren",
    "removeDesc",
];

/// 4.1.0 built-ins that are **not** in `preset-default` (19). Stay default OFF.
const SVGO_410_OPTIN: &[&str] = &[
    "removeViewBox",
    "removeTitle",
    "removeDimensions",
    "removeXMLNS",
    "removeXlink",
    "removeScripts",
    "removeStyleElement",
    "removeRasterImages",
    "removeOffCanvasPaths",
    "prefixIds",
    "reusePaths",
    "convertOneStopGradients",
    "convertStyleToAttrs",
    "cleanupListOfValues",
    "addAttributesToSVGElement",
    "addClassesToSVGElement",
    "removeAttrs",
    "removeAttributesBySelector",
    "removeElementsByAttr",
];

/// 4.1.0 `lib/builtin.js` export order, minus the `preset-default` bundle.
const SVGO_410_BUILTIN_PLUGIN_IDS: &[&str] = &[
    "addAttributesToSVGElement",
    "addClassesToSVGElement",
    "cleanupAttrs",
    "cleanupEnableBackground",
    "cleanupIds",
    "cleanupListOfValues",
    "cleanupNumericValues",
    "collapseGroups",
    "convertColors",
    "convertEllipseToCircle",
    "convertOneStopGradients",
    "convertPathData",
    "convertShapeToPath",
    "convertStyleToAttrs",
    "convertTransform",
    "inlineStyles",
    "mergePaths",
    "mergeStyles",
    "minifyStyles",
    "moveElemsAttrsToGroup",
    "moveGroupAttrsToElems",
    "prefixIds",
    "removeAttributesBySelector",
    "removeAttrs",
    "removeComments",
    "removeDeprecatedAttrs",
    "removeDesc",
    "removeDimensions",
    "removeDoctype",
    "removeEditorsNSData",
    "removeElementsByAttr",
    "removeEmptyAttrs",
    "removeEmptyContainers",
    "removeEmptyText",
    "removeHiddenElems",
    "removeMetadata",
    "removeNonInheritableGroupAttrs",
    "removeOffCanvasPaths",
    "removeRasterImages",
    "removeScripts",
    "removeStyleElement",
    "removeTitle",
    "removeUnknownsAndDefaults",
    "removeUnusedNS",
    "removeUselessDefs",
    "removeUselessStrokeAndFill",
    "removeViewBox",
    "removeXlink",
    "removeXMLNS",
    "removeXMLProcInst",
    "reusePaths",
    "sortAttrs",
    "sortDefsChildren",
];

const ICON: &str = concat!(
    r##"<svg xmlns="http://www.w3.org/2000/svg" role="img" viewBox="0 0 24 24" width="24" height="24">"##,
    r##"<title>Keep</title>"##,
    r##"<path d="M4 12h16"/>"##,
    "</svg>",
);

fn minsvg() -> Command {
    Command::new(env!("CARGO_BIN_EXE_minsvg"))
}

fn leftover_svgo_410_ids() -> Vec<&'static str> {
    SVGO_410_BUILTIN_PLUGIN_IDS
        .iter()
        .copied()
        .filter(|id| !DEFAULT_PLUGIN_NAMES.contains(id) && !OPTIN_PLUGIN_NAMES.contains(id))
        .collect()
}

/// Compile-time: each official opt-in has a public `run_*` (or `prefix_ids`).
fn optin_run_exists(name: &str) -> bool {
    match name {
        "removeViewBox" => {
            let _ = minsvg::run_remove_view_box;
            true
        }
        "removeTitle" => {
            let _ = minsvg::run_remove_title;
            true
        }
        "removeDimensions" => {
            let _ = minsvg::run_remove_dimensions;
            true
        }
        "removeXMLNS" => {
            let _ = minsvg::run_remove_xmlns;
            true
        }
        "removeXlink" => {
            let _ = minsvg::run_remove_xlink;
            true
        }
        "removeScripts" => {
            let _ = minsvg::run_remove_scripts;
            true
        }
        "removeStyleElement" => {
            let _ = minsvg::run_remove_style_element;
            true
        }
        "removeRasterImages" => {
            let _ = minsvg::run_remove_raster_images;
            true
        }
        "removeOffCanvasPaths" => {
            let _ = minsvg::run_remove_off_canvas_paths;
            true
        }
        "prefixIds" => {
            let _ = minsvg::prefix_ids;
            true
        }
        "reusePaths" => {
            let _ = minsvg::run_reuse_paths;
            true
        }
        "convertOneStopGradients" => {
            let _ = minsvg::run_convert_one_stop_gradients;
            true
        }
        "convertStyleToAttrs" => {
            let _ = minsvg::run_convert_style_to_attrs;
            true
        }
        "cleanupListOfValues" => {
            let _ = minsvg::run_cleanup_list_of_values;
            true
        }
        "addAttributesToSVGElement" => {
            let _ = minsvg::run_add_attributes_to_svg_element;
            true
        }
        "addClassesToSVGElement" => {
            let _ = minsvg::run_add_classes_to_svg_element;
            true
        }
        "removeAttrs" => {
            let _ = minsvg::run_remove_attrs;
            true
        }
        "removeAttributesBySelector" => {
            let _ = minsvg::run_remove_attributes_by_selector;
            true
        }
        "removeElementsByAttr" => {
            let _ = minsvg::run_remove_elements_by_attr;
            true
        }
        _ => false,
    }
}

#[test]
fn leftover_svgo_410_builtin_plugin_ids_is_zero() {
    assert_eq!(SVGO_410_PRESET_DEFAULT.len(), 34);
    assert_eq!(SVGO_410_OPTIN.len(), 19);
    assert_eq!(SVGO_410_BUILTIN_PLUGIN_IDS.len(), 53);

    let preset: BTreeSet<&str> = SVGO_410_PRESET_DEFAULT.iter().copied().collect();
    let optin: BTreeSet<&str> = SVGO_410_OPTIN.iter().copied().collect();
    let builtin: BTreeSet<&str> = SVGO_410_BUILTIN_PLUGIN_IDS.iter().copied().collect();
    assert_eq!(preset.len(), 34, "duplicate preset-default id");
    assert_eq!(optin.len(), 19, "duplicate opt-in id");
    assert!(preset.is_disjoint(&optin));
    assert_eq!(&preset | &optin, builtin);

    let leftover = leftover_svgo_410_ids();
    assert_eq!(
        leftover,
        Vec::<&str>::new(),
        "leftover SVGO 4.1.0 built-in plugin IDs: {leftover:?}"
    );
}

#[test]
fn every_preset_default_name_is_wired() {
    for name in SVGO_410_PRESET_DEFAULT {
        assert!(
            DEFAULT_PLUGIN_NAMES.contains(name),
            "{name} must be in DEFAULT_PLUGIN_NAMES"
        );
        assert!(
            !OPTIN_PLUGIN_NAMES.contains(name),
            "{name} is preset-default, not opt-in"
        );
        assert!(
            !is_optin_plugin(name),
            "{name} must not be classified opt-in"
        );
    }
}

#[test]
fn nineteen_optins_exist_and_are_default_off() {
    assert_eq!(OPTIN_PLUGIN_NAMES.len(), 19);
    let catalog: BTreeSet<&str> = OPTIN_PLUGIN_NAMES.iter().copied().collect();
    let official: BTreeSet<&str> = SVGO_410_OPTIN.iter().copied().collect();
    assert_eq!(
        catalog, official,
        "OPTIN_PLUGIN_NAMES must be exactly the 19 SVGO 4.1.0 opt-ins"
    );

    let cfg = Config::default();
    assert!(cfg.enable_plugins.is_empty());
    assert!(!DEFAULT_PLUGIN_NAMES.contains(&"removeViewBox"));
    assert!(!cfg.plugin_enabled("removeViewBox"));

    for name in SVGO_410_OPTIN {
        assert!(
            OPTIN_PLUGIN_NAMES.contains(name),
            "{name} must exist as an opt-in"
        );
        assert!(
            !DEFAULT_PLUGIN_NAMES.contains(name),
            "{name} must stay default OFF"
        );
        assert!(is_optin_plugin(name), "{name}");
        assert!(!cfg.plugin_enabled(name), "{name} must stay default OFF");
    }
}

#[test]
fn default_optimize_keeps_viewbox_and_skips_optins() {
    let out = optimize_str(ICON).expect("optimize");
    assert!(out.svg.contains("viewBox=\"0 0 24 24\""), "{}", out.svg);
    assert!(out.svg.contains("<title>Keep</title>"), "{}", out.svg);
    assert!(out.svg.contains("role=\"img\""), "{}", out.svg);
    assert!(out.svg.contains("xmlns="), "{}", out.svg);
    for name in SVGO_410_OPTIN {
        assert!(
            !out.plugins_run.iter().any(|n| n == name),
            "default plugins_run must not include {name}: {:?}",
            out.plugins_run
        );
    }
}

#[test]
fn plugin_flag_or_run_fn_exists_for_each_optin() {
    let output = minsvg()
        .args(["plugins", "--json"])
        .output()
        .expect("minsvg plugins --json");
    assert!(output.status.success());
    let payload = String::from_utf8_lossy(&output.stdout);
    assert!(payload.contains("\"optin\":["), "{payload}");

    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("in.svg"), ICON).unwrap();

    for name in SVGO_410_OPTIN {
        assert!(
            payload.contains(&format!("\"{name}\"")),
            "minsvg plugins --json missing {name}: {payload}"
        );

        let spec = parse_plugin_spec(name).expect(name);
        assert_eq!(spec.name, *name);
        let mut cfg = Config::default();
        apply_plugin_cli(&mut cfg, &[spec], &[]).expect(name);
        assert!(cfg.plugin_enabled(name), "--plugin {name} must enable");

        assert!(
            optin_run_exists(name),
            "{name} needs a public run_* / prefix_ids"
        );

        let out_name = format!("{name}.svg");
        let status = minsvg()
            .current_dir(dir.path())
            .args(["in.svg", "-o", &out_name, "--quiet", "--plugin", name])
            .status()
            .unwrap_or_else(|e| panic!("spawn --plugin {name}: {e}"));
        assert!(
            status.success(),
            "--plugin {name} was not accepted: {status}"
        );
    }
}
