//! Default `minsvg` **release** binary size budget.
//!
//! Debug `cargo test` (CI) ignores this file — unstripped debug bins are
//! ~14 MB and are not the product. Enforce with:
//!
//! ```text
//! cargo test --release --test binary_size
//! ```
//!
//! Measure by hand (same artifact):
//!
//! ```text
//! cargo build --release --bin minsvg
//! wc -c target/release/minsvg
//! ```
//!
//! MSRV pair: `cargo +1.83.0 build --release --bin minsvg`. See
//! [`PACKAGE_SIZE.md`](../PACKAGE_SIZE.md). This test does **not** budget
//! `minsvg-mcp` or `--features serve`.

use std::fs;
use std::path::Path;

/// 2.5 MiB. Current default release `minsvg` measured 2_058_888
/// (`PACKAGE_SIZE.md`; rustc 1.83 and 1.98.1, Mach-O arm64). Ceiling is
/// that number plus slack for Linux / rustc variance. Still fails a
/// `[profile.release]` revert (stock 1.83 release was 3_460_616).
const MAX_RELEASE_BYTES: u64 = 2 * 1024 * 1024 + 512 * 1024;

#[test]
#[cfg_attr(
    debug_assertions,
    ignore = "debug minsvg is unstripped; run cargo test --release --test binary_size"
)]
fn default_release_minsvg_stays_under_budget() {
    if cfg!(debug_assertions) {
        return;
    }
    if cfg!(feature = "serve") {
        eprintln!("skipping: --features serve is not the default CLI (see PACKAGE_SIZE.md)");
        return;
    }

    let path = Path::new(env!("CARGO_BIN_EXE_minsvg"));
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    assert!(
        name == "minsvg" || name == "minsvg.exe",
        "budget is the default `minsvg` CLI, not {name}: {}",
        path.display()
    );

    let bytes = fs::metadata(path)
        .unwrap_or_else(|e| panic!("stat {}: {e}", path.display()))
        .len();
    eprintln!(
        "{}: {bytes} bytes (budget {MAX_RELEASE_BYTES} = 2.5 MiB)",
        path.display()
    );
    assert!(
        bytes > 0 && bytes <= MAX_RELEASE_BYTES,
        "{} is {bytes} bytes; default release minsvg budget is {MAX_RELEASE_BYTES} \
         (2.5 MiB = rustc 1.83 measured 2058888 + slack). \
         Measure: cargo build --release --bin minsvg && wc -c target/release/minsvg",
        path.display()
    );
}
