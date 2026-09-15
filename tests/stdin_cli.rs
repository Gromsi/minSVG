//! Pipe a tiny SVG through `minsvg --stdin` and check stdout.

use std::io::Write;
use std::process::{Command, Stdio};

const TINY: &str = concat!(
    r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24">"##,
    "<!-- leftover -->",
    r##"<rect x="0.0000" y="0.0000" width="10.0000" height="10.0000" fill="#000000"/>"##,
    "</svg>",
);

fn minsvg_stdin(input: &[u8]) -> std::process::Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_minsvg"))
        .arg("--stdin")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn minsvg --stdin");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(input)
        .expect("write stdin");
    child.wait_with_output().expect("wait minsvg --stdin")
}

#[test]
fn stdin_writes_smaller_or_equal_svg_and_keeps_viewbox() {
    let output = minsvg_stdin(TINY.as_bytes());
    assert!(
        output.status.success(),
        "minsvg --stdin failed: {}\nstdout:{}\nstderr:{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).expect("stdout utf-8");
    assert!(stdout.contains("<svg"), "{stdout}");
    assert!(
        stdout.len() <= TINY.len(),
        "stdout {}B > input {}B\n{stdout}",
        stdout.len(),
        TINY.len()
    );
    assert!(stdout.contains("viewBox=\"0 0 24 24\""), "{stdout}");
}
