use clap::{Args, Parser, Subcommand};
use minsvg::{optimize, Config, DEFAULT_PLUGIN_NAMES, MOTION_SKIP_PLUGINS};
use std::fs;
use std::io::{self, IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

#[derive(Parser, Debug)]
#[command(
    name = "minsvg",
    version,
    about = "Clean-room Rust SVG optimizer",
    args_conflicts_with_subcommands = true,
    arg_required_else_help = true,
    disable_help_subcommand = true,
    help_template = "\
{about} {version}

{usage-heading} {usage}

{all-args}
"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
    #[command(flatten)]
    optimize: OptimizeArgs,
}

#[derive(Args, Debug, Default)]
struct OptimizeArgs {
    /// SVG file, or `-` for stdin
    input: Option<PathBuf>,
    /// Write file (`-` = stdout)
    #[arg(short, long)]
    output: Option<PathBuf>,
    /// Read stdin
    #[arg(long, conflicts_with = "input")]
    stdin: bool,
    /// Summary on stderr
    #[arg(long)]
    report: bool,
    /// Do not skip motion-unsafe passes
    #[arg(long)]
    no_animation_aware: bool,
    /// Extra JS/TS/CSS/JSX for `#id` refs
    #[arg(long)]
    extra: Vec<PathBuf>,
    /// Skip a named pass
    #[arg(long)]
    skip: Vec<String>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// List wired pass names
    Plugins {
        /// JSON
        #[arg(long)]
        json: bool,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Some(Commands::Plugins { json }) => print_plugins(json),
        None => {
            if let Err(err) = run_from_args(cli.optimize) {
                eprintln!("minsvg: {err}");
                return ExitCode::FAILURE;
            }
        }
    }
    ExitCode::SUCCESS
}

fn run_from_args(args: OptimizeArgs) -> Result<(), String> {
    run_optimize(
        args.input,
        args.output,
        args.stdin,
        args.report,
        !args.no_animation_aware,
        args.extra,
        args.skip,
    )
}

fn print_plugins(json: bool) {
    if json {
        println!("{}", plugins_json());
        return;
    }
    println!("# wired (minsvg::DEFAULT_PLUGIN_NAMES)");
    for name in DEFAULT_PLUGIN_NAMES {
        println!("{name}");
    }
    println!("# convertPathData is a conservative d minify (rel/abs, H/V/S/T/Z, leading zeros; no precision-3); skipped on motion");
    println!("# skipped on motion-sensitive docs (animation-aware default)");
    for name in MOTION_SKIP_PLUGINS {
        println!("{name}");
    }
}

fn plugins_json() -> String {
    format!(
        "{{\"wired\":{},\"motion_skip\":{},\"path_data\":\"convertPathData conservative minify (rel/abs, H/V/S/T/Z, leading zeros; no precision-3)\"}}",
        json_string_array(DEFAULT_PLUGIN_NAMES),
        json_string_array(MOTION_SKIP_PLUGINS),
    )
}

fn json_string_array(items: &[&str]) -> String {
    let mut out = String::from("[");
    for (i, item) in items.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push('"');
        out.push_str(item);
        out.push('"');
    }
    out.push(']');
    out
}

fn run_optimize(
    input: Option<PathBuf>,
    output: Option<PathBuf>,
    use_stdin: bool,
    report: bool,
    animation_aware: bool,
    extra: Vec<PathBuf>,
    skip: Vec<String>,
) -> Result<(), String> {
    let source = resolve_input(input, use_stdin, io::stdin().is_terminal())?;
    let bytes = read_input(&source)?;
    if bytes.is_empty() {
        return Err(empty_input_error(&source));
    }

    let mut cfg = Config::default();
    cfg.animation_aware = animation_aware;
    for path in extra {
        let text =
            fs::read_to_string(&path).map_err(|e| format!("read extra {}: {e}", path.display()))?;
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("extra")
            .to_string();
        cfg.extra_sources.push((name, text));
    }
    cfg.skip_plugins = skip;
    cfg.source_name = Some(match &source {
        InputSource::Stdin => "stdin.svg".into(),
        InputSource::File(path) => path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("input.svg")
            .to_string(),
    });
    let result = optimize(&bytes, &cfg).map_err(|e| e.to_string())?;
    let writing_file = matches!(output, Some(ref p) if p.as_os_str() != "-");
    if report || writing_file {
        eprintln!("{}", result.summary);
    }
    write_output(output.as_deref(), result.svg.as_bytes())
}

#[derive(Debug, PartialEq, Eq)]
enum InputSource {
    Stdin,
    File(PathBuf),
}

fn resolve_input(
    input: Option<PathBuf>,
    use_stdin: bool,
    stdin_is_terminal: bool,
) -> Result<InputSource, String> {
    if use_stdin {
        return Ok(InputSource::Stdin);
    }
    match input {
        Some(path) if path.as_os_str() == "-" => Ok(InputSource::Stdin),
        Some(path) => Ok(InputSource::File(path)),
        None if stdin_is_terminal => {
            Err("missing input (pass a file, `-`, or --stdin; refuse to block on a TTY)".into())
        }
        None => Ok(InputSource::Stdin),
    }
}

fn read_input(source: &InputSource) -> Result<Vec<u8>, String> {
    match source {
        InputSource::Stdin => {
            let mut buf = Vec::new();
            io::stdin()
                .read_to_end(&mut buf)
                .map_err(|e| format!("read stdin: {e}"))?;
            Ok(buf)
        }
        InputSource::File(path) => {
            fs::read(path).map_err(|e| format!("read {}: {e}", path.display()))
        }
    }
}

fn empty_input_error(source: &InputSource) -> String {
    match source {
        InputSource::Stdin => "input is empty (stdin)".into(),
        InputSource::File(path) => format!("input is empty ({})", path.display()),
    }
}

fn write_output(output: Option<&Path>, bytes: &[u8]) -> Result<(), String> {
    match output {
        Some(path) if path.as_os_str() != "-" => {
            if let Some(parent) = path.parent() {
                if !parent.as_os_str().is_empty() {
                    fs::create_dir_all(parent)
                        .map_err(|e| format!("create {}: {e}", parent.display()))?;
                }
            }
            fs::write(path, bytes).map_err(|e| format!("write {}: {e}", path.display()))?;
        }
        _ => {
            let mut stdout = io::stdout();
            stdout
                .write_all(bytes)
                .map_err(|e| format!("write stdout: {e}"))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_file_dash_and_stdin_flag() {
        let file = PathBuf::from("in.svg");
        assert_eq!(
            resolve_input(Some(file.clone()), false, true).unwrap(),
            InputSource::File(file)
        );
        assert_eq!(
            resolve_input(Some(PathBuf::from("-")), false, true).unwrap(),
            InputSource::Stdin
        );
        assert_eq!(resolve_input(None, true, true).unwrap(), InputSource::Stdin);
        assert_eq!(
            resolve_input(None, false, false).unwrap(),
            InputSource::Stdin
        );
    }

    #[test]
    fn resolve_refuses_bare_tty() {
        let err = resolve_input(None, false, true).unwrap_err();
        assert!(err.contains("TTY"), "{err}");
        assert!(err.contains("missing input"), "{err}");
    }

    #[test]
    fn plugins_json_flag_and_payload() {
        let cli = Cli::try_parse_from(["minsvg", "plugins", "--json"]).unwrap();
        match cli.command {
            Some(Commands::Plugins { json }) => assert!(json),
            other => panic!("expected Plugins, got {other:?}"),
        }
        let cli = Cli::try_parse_from(["minsvg", "plugins"]).unwrap();
        match cli.command {
            Some(Commands::Plugins { json }) => assert!(!json),
            other => panic!("expected Plugins, got {other:?}"),
        }

        let payload = plugins_json();
        assert!(payload.starts_with("{\"wired\":["), "{payload}");
        assert!(payload.contains("\"motion_skip\":["), "{payload}");
        assert!(
            payload.contains("\"path_data\":\"convertPathData conservative minify"),
            "{payload}"
        );
        for name in DEFAULT_PLUGIN_NAMES {
            assert!(payload.contains(&format!("\"{name}\"")), "{payload}");
        }
        for name in MOTION_SKIP_PLUGINS {
            assert!(payload.contains(&format!("\"{name}\"")), "{payload}");
        }
    }

    #[test]
    fn bare_minsvg_args_are_optimize() {
        let cli = Cli::try_parse_from(["minsvg", "in.svg", "-o", "out.svg"]).unwrap();
        assert!(cli.command.is_none(), "{cli:?}");
        assert_eq!(cli.optimize.input.as_deref(), Some(Path::new("in.svg")));
        assert_eq!(cli.optimize.output.as_deref(), Some(Path::new("out.svg")));
    }

    #[test]
    fn optimize_is_not_a_subcommand() {
        let err = Cli::try_parse_from(["minsvg", "optimize", "--help"])
            .err()
            .map(|e| e.to_string())
            .unwrap_or_default();
        assert!(
            !err.contains("Usage: minsvg optimize"),
            "optimize subcommand should be gone: {err}"
        );
        let cli = Cli::try_parse_from(["minsvg", "optimize", "-o", "out.svg"]).unwrap();
        assert!(cli.command.is_none(), "{cli:?}");
        assert_eq!(cli.optimize.input.as_deref(), Some(Path::new("optimize")));
    }

    #[test]
    fn write_output_creates_parent_dirs() {
        let dir = std::env::temp_dir().join(format!("minsvg-cli-{}", std::process::id()));
        let path = dir.join("nested").join("out.svg");
        let _ = fs::remove_dir_all(&dir);
        write_output(Some(&path), b"<svg/>").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"<svg/>");
        fs::remove_dir_all(&dir).unwrap();
    }
}
