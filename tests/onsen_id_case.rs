//! Onsen paint id case: `url(#poolFill)` must survive `optimize()`.
//!
//! Reads the sibling bench file in place. Do **not** copy it into this crate.

use std::path::PathBuf;

use minsvg::{optimize, Config};

fn onsen_path() -> Option<PathBuf> {
    if let Ok(root) = std::env::var("SVGO_RUST_ROOT") {
        let p = PathBuf::from(root).join("corpus/animation/capybara-hot-spring.svg");
        if p.is_file() {
            return Some(p);
        }
    }
    let here = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    [
        here.join("../svgo-rust/corpus/animation/capybara-hot-spring.svg"),
        here.join("../../svgo-rust/corpus/animation/capybara-hot-spring.svg"),
    ]
    .into_iter()
    .find(|p| p.is_file())
}

#[test]
fn optimize_keeps_url_poolfill_exact_case() {
    let Some(path) = onsen_path() else {
        eprintln!("svgo-rust onsen corpus missing — skipping");
        return;
    };

    let input = std::fs::read(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let out = optimize(&input, &Config::default()).expect("optimize onsen");
    assert!(
        out.svg.contains("url(#poolFill)"),
        "url(#poolFill) lost (exact case):\n{}",
        out.svg
    );
}
