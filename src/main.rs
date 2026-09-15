use clap::{Args, Parser, Subcommand};
use minsvg::{
    apply_plugin_cli, config_from_cwd_and_cli, load_config_path, optimize, parse_plugin_spec,
    CliOverrides, Config, DataUri, DEFAULT_PLUGIN_NAMES, MOTION_SKIP_PLUGINS, OPTIN_PLUGIN_NAMES,
};
use std::fs;
use std::io::{self, IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

#[cfg(feature = "serve")]
mod serve;

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
    /// SVG file, directory, or `-` for stdin
    input: Option<PathBuf>,
    /// Write file or folder (`-` = stdout)
    #[arg(short, long)]
    output: Option<PathBuf>,
    /// Folder of `*.svg` files (in-place, or `-o outdir/`)
    #[arg(short, long, value_name = "DIR", conflicts_with_all = ["input", "stdin"])]
    folder: Option<PathBuf>,
    /// Recurse into subfolders (`-f` or a directory input)
    #[arg(short, long)]
    recursive: bool,
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
    /// Skip a named pass (SVGO `{ name, active: false }`)
    #[arg(long, value_name = "NAME")]
    skip: Vec<String>,
    /// Enable a named pass / unskip (SVGO `{ name, active: true }`).
    /// `name:{"foo":1}` also sets [`minsvg::Config::plugin_params`].
    #[arg(long, value_name = "NAME")]
    plugin: Vec<String>,
    /// Per-plugin JSON (`--param cleanupListOfValues={"floatPrecision":2}`).
    /// Does not enable a default-off pass; pair with `--plugin`.
    #[arg(long, value_name = "NAME=JSON")]
    param: Vec<String>,
    /// Re-run plugins until size is stable (max 10)
    #[arg(long)]
    multipass: bool,
    /// Decimal places for path `d` and numeric attrs (SVGO `--precision`)
    #[arg(long, value_name = "N", value_parser = clap::value_parser!(u8).range(0..=20))]
    precision: Option<u8>,
    /// Config file (default: minsvg.config.toml / .json in cwd)
    #[arg(long, value_name = "PATH")]
    config: Option<PathBuf>,
    /// Pretty-print with indentation (SVGO `--pretty`)
    #[arg(long)]
    pretty: bool,
    /// Spaces per indent when `--pretty` (SVGO `--indent`, default 4)
    #[arg(long, value_name = "N", value_parser = clap::value_parser!(u8).range(0..=32))]
    indent: Option<u8>,
    /// Output as data URI: `base64`, `enc`, or `unenc` (SVGO `--datauri`)
    #[arg(
        long,
        value_name = "FORMAT",
        num_args = 0..=1,
        default_missing_value = "base64"
    )]
    datauri: Option<DataUri>,
    /// Suppress informational stderr (SVGO `--quiet`)
    #[arg(short, long)]
    quiet: bool,
    /// Line endings: `lf` or `crlf` (SVGO `--eol`)
    #[arg(long, value_name = "lf|crlf", value_parser = Eol::parse_cli)]
    eol: Option<Eol>,
    /// Ensure written output ends with a newline (SVGO `--final-newline`)
    #[arg(long)]
    final_newline: bool,
}

/// SVGO `--eol lf|crlf`. Applied to written SVG (not data-URI payloads).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Eol {
    Lf,
    Crlf,
}

impl Eol {
    fn parse_cli(s: &str) -> Result<Self, String> {
        match s {
            "lf" => Ok(Self::Lf),
            "crlf" => Ok(Self::Crlf),
            _ => Err("option '--eol' must have one of the following values: 'lf' or 'crlf'".into()),
        }
    }
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// List wired pass names
    Plugins {
        /// JSON
        #[arg(long)]
        json: bool,
    },
    /// Local optimize HTTP (loopback default; no auth)
    Serve {
        /// Listen address. Default `127.0.0.1:8765`. `0.0.0.0:8080` only in a container you deploy.
        #[arg(long, default_value = "127.0.0.1:8765")]
        bind: String,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Some(Commands::Plugins { json }) => print_plugins(json),
        Some(Commands::Serve { bind }) => return run_serve(&bind),
        None => {
            if let Err(err) = run_from_args(cli.optimize) {
                eprintln!("minsvg: {err}");
                return ExitCode::FAILURE;
            }
        }
    }
    ExitCode::SUCCESS
}

fn run_serve(bind: &str) -> ExitCode {
    #[cfg(feature = "serve")]
    {
        if let Err(err) = serve::run(bind) {
            eprintln!("minsvg: {err}");
            return ExitCode::FAILURE;
        }
        ExitCode::SUCCESS
    }
    #[cfg(not(feature = "serve"))]
    {
        let _ = bind;
        eprintln!(
            "minsvg: this binary was built without HTTP serve.\n\
             cargo install --git https://github.com/Gromsi/minSVG --locked --features serve"
        );
        ExitCode::FAILURE
    }
}

fn run_from_args(args: OptimizeArgs) -> Result<(), String> {
    let plugin_specs = args
        .plugin
        .iter()
        .map(|s| parse_plugin_spec(s))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    let overrides = CliOverrides {
        no_animation_aware: args.no_animation_aware,
        skip: args.skip.clone(),
        plugin: plugin_specs.iter().map(|s| s.name.clone()).collect(),
        precision: args.precision,
        multipass: args.multipass,
    };
    let extras = load_extras(&args.extra)?;
    let mut cfg = if let Some(ref path) = args.config {
        let mut cfg = load_config_path(path).map_err(|e| e.to_string())?;
        overrides.apply(&mut cfg);
        cfg
    } else {
        config_from_cwd_and_cli(&overrides).map_err(|e| e.to_string())?
    };
    cfg.extra_sources = extras;
    if args.pretty {
        cfg.pretty = true;
    }
    if let Some(n) = args.indent {
        cfg.indent = n;
    }
    cfg.datauri = args.datauri;
    apply_plugin_cli(&mut cfg, &plugin_specs, &args.param).map_err(|e| e.to_string())?;

    let folder = match args.folder.as_ref() {
        Some(dir) => Some(dir.clone()),
        None => match args.input.as_ref() {
            Some(path) if path.as_os_str() != "-" && path.is_dir() => Some(path.clone()),
            _ => None,
        },
    };
    if let Some(dir) = folder {
        return run_folder(dir, &args, cfg);
    }
    if args.recursive {
        return Err("--recursive requires a directory (`minsvg -f dir/` or `minsvg dir/`)".into());
    }
    run_optimize(&args, cfg)
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
    println!("# opt-in (default OFF; --plugin NAME)");
    for name in OPTIN_PLUGIN_NAMES {
        println!("{name}");
    }
    println!("# convertPathData is a conservative d minify (rel/abs, H/V/S/T/Z, leading zeros; optional --precision; dest-count/glue guards; skipped on motion)");
    println!("# skipped on motion-sensitive docs (animation-aware default)");
    for name in MOTION_SKIP_PLUGINS {
        println!("{name}");
    }
}

fn plugins_json() -> String {
    format!(
        "{{\"wired\":{},\"optin\":{},\"motion_skip\":{},\"path_data\":\"convertPathData conservative minify (rel/abs, H/V/S/T/Z, leading zeros; optional --precision; dest-count/glue guards)\"}}",
        json_string_array(DEFAULT_PLUGIN_NAMES),
        json_string_array(OPTIN_PLUGIN_NAMES),
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

fn run_folder(dir: PathBuf, args: &OptimizeArgs, cfg: Config) -> Result<(), String> {
    if !dir.is_dir() {
        return Err(format!("not a directory: {}", dir.display()));
    }
    if let Some(ref out) = args.output {
        if out.as_os_str() == "-" {
            return Err(
                "folder mode cannot write to stdout (pass -o <dir> or omit -o for in-place)".into(),
            );
        }
        if out.is_file() {
            return Err(format!(
                "folder mode -o must be a directory ({} is a file)",
                out.display()
            ));
        }
    }

    let files = collect_svg_files(&dir, args.recursive)?;
    if files.is_empty() {
        if !args.quiet {
            eprintln!("minsvg: no *.svg files in {}", dir.display());
        }
        return Ok(());
    }

    for file in files {
        let bytes = fs::read(&file).map_err(|e| format!("read {}: {e}", file.display()))?;
        if bytes.is_empty() {
            return Err(format!("input is empty ({})", file.display()));
        }
        let name = file
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("input.svg")
            .to_string();
        let result = optimize_bytes(&bytes, name, cfg.clone())?;
        let dest = match args.output.as_deref() {
            Some(out_dir) => {
                let rel = file
                    .strip_prefix(&dir)
                    .map_err(|_| format!("{} is not under {}", file.display(), dir.display()))?;
                out_dir.join(rel)
            }
            None => file.clone(),
        };
        write_output(Some(&dest), &finish_output(&result.svg, args))?;
        if should_print_summary(args.report, args.output.is_some(), args.quiet) {
            eprintln!("{}", result.summary);
        }
    }
    Ok(())
}

fn run_optimize(args: &OptimizeArgs, cfg: Config) -> Result<(), String> {
    let source = resolve_input(args.input.clone(), args.stdin, io::stdin().is_terminal())?;
    let bytes = read_input(&source)?;
    if bytes.is_empty() {
        return Err(empty_input_error(&source));
    }

    let name = match &source {
        InputSource::Stdin => "stdin.svg".into(),
        InputSource::File(path) => path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("input.svg")
            .to_string(),
    };
    let result = optimize_bytes(&bytes, name, cfg)?;
    let writing_file = matches!(args.output, Some(ref p) if p.as_os_str() != "-");
    if should_print_summary(args.report, writing_file, args.quiet) {
        eprintln!("{}", result.summary);
    }
    write_output(args.output.as_deref(), &finish_output(&result.svg, args))
}

fn load_extras(paths: &[PathBuf]) -> Result<Vec<(String, String)>, String> {
    let mut extra = Vec::with_capacity(paths.len());
    for path in paths {
        let text =
            fs::read_to_string(&path).map_err(|e| format!("read extra {}: {e}", path.display()))?;
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("extra")
            .to_string();
        extra.push((name, text));
    }
    Ok(extra)
}

fn optimize_bytes(
    bytes: &[u8],
    source_name: String,
    mut cfg: Config,
) -> Result<minsvg::OptimizeOutput, String> {
    cfg.source_name = Some(source_name);
    optimize(bytes, &cfg).map_err(|e| e.to_string())
}

fn collect_svg_files(dir: &Path, recursive: bool) -> Result<Vec<PathBuf>, String> {
    let mut files = Vec::new();
    walk_svg_files(dir, recursive, &mut files)?;
    files.sort();
    Ok(files)
}

fn walk_svg_files(dir: &Path, recursive: bool, out: &mut Vec<PathBuf>) -> Result<(), String> {
    let entries = fs::read_dir(dir).map_err(|e| format!("read {}: {e}", dir.display()))?;
    let mut subdirs = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| format!("read {}: {e}", dir.display()))?;
        let path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|e| format!("stat {}: {e}", path.display()))?;
        if file_type.is_dir() {
            if recursive {
                subdirs.push(path);
            }
        } else if is_svg_path(&path) && (file_type.is_file() || file_type.is_symlink()) {
            out.push(path);
        }
    }
    subdirs.sort();
    for sub in subdirs {
        walk_svg_files(&sub, true, out)?;
    }
    Ok(())
}

fn is_svg_path(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("svg"))
        .unwrap_or(false)
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

fn should_print_summary(report: bool, writing_dest: bool, quiet: bool) -> bool {
    !quiet && (report || writing_dest)
}

fn to_lf(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

fn to_crlf(text: &str) -> String {
    to_lf(text).replace('\n', "\r\n")
}

/// Apply `--eol` / `--final-newline` to written bytes. Data-URI payloads skip
/// internal newline conversion so `data:…` stays a single URI.
fn apply_output_style(
    text: &str,
    eol: Option<Eol>,
    final_newline: bool,
    convert_newlines: bool,
) -> String {
    let mut out = if convert_newlines {
        match eol {
            Some(Eol::Crlf) => to_crlf(text),
            Some(Eol::Lf) => to_lf(text),
            None => text.to_string(),
        }
    } else {
        text.to_string()
    };
    if final_newline && !out.ends_with('\n') {
        match eol {
            Some(Eol::Crlf) => out.push_str("\r\n"),
            _ => out.push('\n'),
        }
    }
    out
}

fn finish_output(svg: &str, args: &OptimizeArgs) -> Vec<u8> {
    apply_output_style(svg, args.eol, args.final_newline, args.datauri.is_none()).into_bytes()
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
        assert!(payload.contains("\"optin\":["), "{payload}");
        assert!(payload.contains("\"motion_skip\":["), "{payload}");
        for name in OPTIN_PLUGIN_NAMES {
            assert!(payload.contains(&format!("\"{name}\"")), "{payload}");
        }
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
    fn serve_defaults_to_localhost() {
        let cli = Cli::try_parse_from(["minsvg", "serve"]).unwrap();
        match cli.command {
            Some(Commands::Serve { bind }) => assert_eq!(bind, "127.0.0.1:8765"),
            other => panic!("expected Serve, got {other:?}"),
        }
        let cli = Cli::try_parse_from(["minsvg", "serve", "--bind", "0.0.0.0:8080"]).unwrap();
        match cli.command {
            Some(Commands::Serve { bind }) => assert_eq!(bind, "0.0.0.0:8080"),
            other => panic!("expected Serve, got {other:?}"),
        }
    }

    #[test]
    fn multipass_flag_parses() {
        let cli = Cli::try_parse_from(["minsvg", "in.svg", "--multipass"]).unwrap();
        assert!(cli.command.is_none(), "{cli:?}");
        assert!(cli.optimize.multipass);
        let cli = Cli::try_parse_from(["minsvg", "in.svg"]).unwrap();
        assert!(!cli.optimize.multipass);
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

    #[test]
    fn precision_flag_parses() {
        let cli = Cli::try_parse_from(["minsvg", "in.svg", "--precision", "2"]).unwrap();
        assert_eq!(cli.optimize.precision, Some(2));
        let cli = Cli::try_parse_from(["minsvg", "in.svg", "--precision", "3"]).unwrap();
        assert_eq!(cli.optimize.precision, Some(3));
        let cli = Cli::try_parse_from(["minsvg", "in.svg"]).unwrap();
        assert_eq!(cli.optimize.precision, None);
        assert!(Cli::try_parse_from(["minsvg", "in.svg", "--precision", "21"]).is_err());
    }

    #[test]
    fn skip_and_plugin_flags_parse() {
        let cli = Cli::try_parse_from([
            "minsvg",
            "in.svg",
            "--skip",
            "removeComments",
            "--skip",
            "convertPathData",
            "--plugin",
            "cleanupIds",
        ])
        .unwrap();
        assert_eq!(
            cli.optimize.skip,
            vec!["removeComments".to_string(), "convertPathData".to_string()]
        );
        assert_eq!(cli.optimize.plugin, vec!["cleanupIds".to_string()]);
        let cli = Cli::try_parse_from(["minsvg", "in.svg"]).unwrap();
        assert!(cli.optimize.skip.is_empty());
        assert!(cli.optimize.plugin.is_empty());
        assert!(cli.optimize.param.is_empty());
    }

    #[test]
    fn param_and_plugin_json_flags_parse() {
        let cli = Cli::try_parse_from([
            "minsvg",
            "in.svg",
            "--plugin",
            r#"cleanupListOfValues:{"floatPrecision":2}"#,
            "--param",
            r#"convertStyleToAttrs={"keepImportant":true}"#,
            "--pretty",
            "--datauri",
            "enc",
            "--skip",
            "removeComments",
        ])
        .unwrap();
        assert_eq!(
            cli.optimize.plugin,
            vec![r#"cleanupListOfValues:{"floatPrecision":2}"#.to_string()]
        );
        assert_eq!(
            cli.optimize.param,
            vec![r#"convertStyleToAttrs={"keepImportant":true}"#.to_string()]
        );
        assert!(cli.optimize.pretty);
        assert_eq!(cli.optimize.datauri, Some(DataUri::Enc));
        assert_eq!(cli.optimize.skip, vec!["removeComments".to_string()]);
    }

    #[test]
    fn plugin_json_and_param_apply_through_optimize() {
        let root = unique_temp("plugin-params");
        let input = root.join("in.svg");
        let output = root.join("out.svg");
        fs::write(
            &input,
            concat!(
                r##"<svg xmlns="http://www.w3.org/2000/svg">"##,
                r##"<polygon points="0pt,12pt"/>"##,
                r##"<style>.ocean{fill:blue}</style>"##,
                r##"<rect class="ocean" style="fill:red"/>"##,
                r##"<rect class="ocean"/>"##,
                "</svg>",
            ),
        )
        .unwrap();
        run_from_args(OptimizeArgs {
            input: Some(input),
            output: Some(output.clone()),
            plugin: vec![
                r#"cleanupListOfValues:{"floatPrecision":2}"#.into(),
                "convertStyleToAttrs".into(),
            ],
            ..OptimizeArgs::default()
        })
        .expect("minsvg --plugin cleanupListOfValues --plugin convertStyleToAttrs");
        let out = fs::read_to_string(&output).unwrap();
        assert!(
            out.contains("16") || out.contains("points=\"0 16\""),
            "{out}"
        );
        assert!(!out.contains("12pt"), "{out}");
        assert!(out.contains("fill=\"red\""), "{out}");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn bad_plugin_json_is_an_error() {
        let err = run_from_args(OptimizeArgs {
            input: Some(PathBuf::from("in.svg")),
            plugin: vec!["cleanupListOfValues:{nope}".into()],
            ..OptimizeArgs::default()
        })
        .unwrap_err();
        assert!(
            err.contains("--plugin") || err.contains("nope") || err.contains("expected"),
            "{err}"
        );
    }

    #[test]
    fn skip_remove_comments_keeps_comment() {
        let root = unique_temp("skip-comment");
        let input = root.join("in.svg");
        let output = root.join("out.svg");
        fs::write(
            &input,
            r#"<svg xmlns="http://www.w3.org/2000/svg"><!-- keep --><rect width="10" height="10"/></svg>"#,
        )
        .unwrap();
        run_from_args(OptimizeArgs {
            input: Some(input),
            output: Some(output.clone()),
            skip: vec!["removeComments".into()],
            ..OptimizeArgs::default()
        })
        .expect("minsvg --skip removeComments");
        let out = fs::read_to_string(&output).unwrap();
        assert!(out.contains("keep"), "{out}");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn config_flag_parses() {
        let cli =
            Cli::try_parse_from(["minsvg", "in.svg", "--config", "minsvg.config.toml"]).unwrap();
        assert_eq!(
            cli.optimize.config.as_deref(),
            Some(Path::new("minsvg.config.toml"))
        );
    }

    #[test]
    fn folder_flags_parse() {
        let cli =
            Cli::try_parse_from(["minsvg", "-f", "icons", "-o", "out", "--recursive"]).unwrap();
        assert!(cli.command.is_none(), "{cli:?}");
        assert_eq!(cli.optimize.folder.as_deref(), Some(Path::new("icons")));
        assert_eq!(cli.optimize.output.as_deref(), Some(Path::new("out")));
        assert!(cli.optimize.recursive);
        assert!(cli.optimize.input.is_none());

        let cli = Cli::try_parse_from(["minsvg", "icons/", "-r"]).unwrap();
        assert_eq!(cli.optimize.input.as_deref(), Some(Path::new("icons/")));
        assert!(cli.optimize.recursive);
        assert!(cli.optimize.folder.is_none());
    }

    #[test]
    fn folder_mode_two_svgs_in_place_and_outdir() {
        let root = unique_temp("folder-two");
        let input = root.join("in");
        let outdir = root.join("out");
        fs::create_dir_all(input.join("nested")).unwrap();

        let one = verbose_svg("one");
        let two = verbose_svg("two");
        fs::write(input.join("a.svg"), &one).unwrap();
        fs::write(input.join("b.svg"), &two).unwrap();
        fs::write(input.join("nested").join("c.svg"), verbose_svg("nested")).unwrap();
        fs::write(input.join("readme.txt"), "not svg").unwrap();

        run_from_args(OptimizeArgs {
            folder: Some(input.clone()),
            output: Some(outdir.clone()),
            ..OptimizeArgs::default()
        })
        .expect("minsvg -f in/ -o out/");

        let out_a = fs::read(outdir.join("a.svg")).expect("out/a.svg");
        let out_b = fs::read(outdir.join("b.svg")).expect("out/b.svg");
        assert_svg_shrunk_or_valid(&one, &out_a, "out/a.svg");
        assert_svg_shrunk_or_valid(&two, &out_b, "out/b.svg");
        assert!(
            !outdir.join("nested").join("c.svg").exists(),
            "non-recursive must skip nested/"
        );

        run_from_args(OptimizeArgs {
            input: Some(input.clone()),
            recursive: true,
            ..OptimizeArgs::default()
        })
        .expect("minsvg in/ --recursive");

        let in_a = fs::read(input.join("a.svg")).expect("in/a.svg");
        let in_b = fs::read(input.join("b.svg")).expect("in/b.svg");
        let in_c = fs::read(input.join("nested").join("c.svg")).expect("in/nested/c.svg");
        assert_svg_shrunk_or_valid(&one, &in_a, "in/a.svg");
        assert_svg_shrunk_or_valid(&two, &in_b, "in/b.svg");
        assert_svg_shrunk_or_valid(&verbose_svg("nested"), &in_c, "in/nested/c.svg");

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn pretty_and_datauri_flags_parse() {
        let cli = Cli::try_parse_from(["minsvg", "in.svg", "--pretty", "--indent", "2"]).unwrap();
        assert!(cli.optimize.pretty);
        assert_eq!(cli.optimize.indent, Some(2));
        assert_eq!(cli.optimize.datauri, None);

        let cli = Cli::try_parse_from(["minsvg", "in.svg", "--datauri"]).unwrap();
        assert_eq!(cli.optimize.datauri, Some(DataUri::Base64));
        let cli = Cli::try_parse_from(["minsvg", "in.svg", "--datauri", "base64"]).unwrap();
        assert_eq!(cli.optimize.datauri, Some(DataUri::Base64));
        let cli = Cli::try_parse_from(["minsvg", "in.svg", "--datauri", "enc"]).unwrap();
        assert_eq!(cli.optimize.datauri, Some(DataUri::Enc));
        let cli = Cli::try_parse_from(["minsvg", "in.svg", "--datauri", "unenc"]).unwrap();
        assert_eq!(cli.optimize.datauri, Some(DataUri::Unenc));
        assert!(Cli::try_parse_from(["minsvg", "in.svg", "--datauri", "hex"]).is_err());
        assert!(Cli::try_parse_from(["minsvg", "in.svg", "--indent", "33"]).is_err());

        let cli = Cli::try_parse_from(["minsvg", "in.svg"]).unwrap();
        assert!(!cli.optimize.pretty);
        assert_eq!(cli.optimize.indent, None);
        assert_eq!(cli.optimize.datauri, None);
        assert_eq!(cli.optimize.precision, None);
        assert!(!cli.optimize.multipass);
        assert!(cli.optimize.config.is_none());
        assert!(cli.optimize.folder.is_none());
        assert!(!cli.optimize.quiet);
        assert_eq!(cli.optimize.eol, None);
        assert!(!cli.optimize.final_newline);
    }

    #[test]
    fn existing_svgo_flags_still_parse() {
        let help = Cli::try_parse_from(["minsvg", "--help"])
            .err()
            .map(|e| e.to_string())
            .unwrap_or_default();
        for flag in [
            "--pretty",
            "--datauri",
            "--plugin",
            "--skip",
            "--param",
            "--multipass",
            "--precision",
            "--folder",
            "--quiet",
            "--eol",
            "--final-newline",
        ] {
            assert!(help.contains(flag), "missing {flag} in help:\n{help}");
        }
        assert!(
            help.contains("-f") && help.contains("--folder"),
            "missing -f in help:\n{help}"
        );

        let cli = Cli::try_parse_from([
            "minsvg",
            "-f",
            "icons",
            "--pretty",
            "--datauri",
            "enc",
            "--plugin",
            "cleanupIds",
            "--skip",
            "removeComments",
            "--param",
            r#"convertStyleToAttrs={"keepImportant":true}"#,
            "--multipass",
            "--precision",
            "2",
            "--quiet",
            "--eol",
            "crlf",
            "--final-newline",
        ])
        .unwrap();
        assert!(cli.optimize.pretty);
        assert_eq!(cli.optimize.datauri, Some(DataUri::Enc));
        assert_eq!(cli.optimize.plugin, vec!["cleanupIds".to_string()]);
        assert_eq!(cli.optimize.skip, vec!["removeComments".to_string()]);
        assert_eq!(
            cli.optimize.param,
            vec![r#"convertStyleToAttrs={"keepImportant":true}"#.to_string()]
        );
        assert!(cli.optimize.multipass);
        assert_eq!(cli.optimize.precision, Some(2));
        assert_eq!(cli.optimize.folder.as_deref(), Some(Path::new("icons")));
        assert!(cli.optimize.quiet);
        assert_eq!(cli.optimize.eol, Some(Eol::Crlf));
        assert!(cli.optimize.final_newline);
    }

    #[test]
    fn quiet_eol_final_newline_flags_parse() {
        let cli = Cli::try_parse_from(["minsvg", "in.svg", "-q"]).unwrap();
        assert!(cli.optimize.quiet);
        let cli = Cli::try_parse_from(["minsvg", "in.svg", "--quiet"]).unwrap();
        assert!(cli.optimize.quiet);

        let cli = Cli::try_parse_from(["minsvg", "in.svg", "--eol", "lf"]).unwrap();
        assert_eq!(cli.optimize.eol, Some(Eol::Lf));
        let cli = Cli::try_parse_from(["minsvg", "in.svg", "--eol", "crlf"]).unwrap();
        assert_eq!(cli.optimize.eol, Some(Eol::Crlf));
        assert!(Cli::try_parse_from(["minsvg", "in.svg", "--eol", "cr"]).is_err());
        assert!(Cli::try_parse_from(["minsvg", "in.svg", "--eol", "unix"]).is_err());

        let cli = Cli::try_parse_from(["minsvg", "in.svg", "--final-newline"]).unwrap();
        assert!(cli.optimize.final_newline);
    }

    #[test]
    fn quiet_suppresses_summary() {
        assert!(should_print_summary(true, false, false));
        assert!(should_print_summary(false, true, false));
        assert!(!should_print_summary(true, true, true));
        assert!(!should_print_summary(false, true, true));
        assert!(!should_print_summary(false, false, false));
    }

    #[test]
    fn apply_output_style_eol_and_final_newline() {
        assert_eq!(
            apply_output_style("<svg/>\n", Some(Eol::Crlf), false, true),
            "<svg/>\r\n"
        );
        assert_eq!(
            apply_output_style("a\r\nb\r\n", Some(Eol::Lf), false, true),
            "a\nb\n"
        );
        assert_eq!(apply_output_style("<svg/>", None, true, true), "<svg/>\n");
        assert_eq!(
            apply_output_style("<svg/>", Some(Eol::Crlf), true, true),
            "<svg/>\r\n"
        );
        assert_eq!(apply_output_style("<svg/>\n", None, true, true), "<svg/>\n");
        assert_eq!(
            apply_output_style(
                "data:image/svg+xml,<svg/>\nline",
                Some(Eol::Crlf),
                true,
                false
            ),
            "data:image/svg+xml,<svg/>\nline\r\n"
        );
    }

    #[test]
    fn pretty_eol_crlf_and_final_newline_write() {
        let root = unique_temp("eol");
        let input = root.join("in.svg");
        let crlf_path = root.join("crlf.svg");
        fs::write(&input, verbose_svg("eol")).unwrap();
        run_from_args(OptimizeArgs {
            input: Some(input.clone()),
            output: Some(crlf_path.clone()),
            pretty: true,
            indent: Some(2),
            eol: Some(Eol::Crlf),
            quiet: true,
            ..OptimizeArgs::default()
        })
        .expect("minsvg --pretty --eol crlf");
        let crlf = fs::read(&crlf_path).unwrap();
        assert!(crlf.windows(2).any(|w| w == b"\r\n"), "{crlf:?}");
        let bare_lf = crlf
            .iter()
            .enumerate()
            .any(|(i, &b)| b == b'\n' && (i == 0 || crlf[i - 1] != b'\r'));
        assert!(!bare_lf, "bare LF after --eol crlf: {crlf:?}");

        let nl_path = root.join("nl.svg");
        run_from_args(OptimizeArgs {
            input: Some(input),
            output: Some(nl_path.clone()),
            final_newline: true,
            quiet: true,
            ..OptimizeArgs::default()
        })
        .expect("minsvg --final-newline");
        let nl = fs::read(&nl_path).unwrap();
        assert!(nl.ends_with(b"\n"), "{nl:?}");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn config_flag_uses_load_config_path() {
        let root = unique_temp("config-load");
        let input = root.join("in.svg");
        let output = root.join("out.svg");
        let cfg = root.join("minsvg.config.toml");
        fs::write(&cfg, "skip = [\"removeComments\"]\npretty = true\n").unwrap();
        fs::write(
            &input,
            r#"<svg xmlns="http://www.w3.org/2000/svg"><!-- keep --><rect width="10" height="10"/></svg>"#,
        )
        .unwrap();
        run_from_args(OptimizeArgs {
            input: Some(input),
            output: Some(output.clone()),
            config: Some(cfg),
            quiet: true,
            ..OptimizeArgs::default()
        })
        .expect("minsvg --config");
        let out = fs::read_to_string(&output).unwrap();
        assert!(out.contains("keep"), "{out}");
        assert!(
            out.contains('\n'),
            "config pretty = true should emit: {out}"
        );

        let err = run_from_args(OptimizeArgs {
            input: Some(PathBuf::from("in.svg")),
            config: Some(root.join("missing-minsvg.config.toml")),
            quiet: true,
            ..OptimizeArgs::default()
        })
        .unwrap_err();
        assert!(
            err.contains("read") || err.contains("missing-minsvg"),
            "{err}"
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn pretty_writes_indented_svg() {
        let root = unique_temp("pretty");
        let input = root.join("in.svg");
        let output = root.join("out.svg");
        fs::write(&input, verbose_svg("pretty")).unwrap();
        run_from_args(OptimizeArgs {
            input: Some(input),
            output: Some(output.clone()),
            pretty: true,
            indent: Some(2),
            ..OptimizeArgs::default()
        })
        .expect("minsvg --pretty --indent 2");
        let out = fs::read_to_string(&output).unwrap();
        assert!(out.contains("<svg"), "{out}");
        assert!(out.contains('\n'), "{out}");
        assert!(
            out.contains("\n  <") || out.contains("\n    <"),
            "expected indented children: {out}"
        );
        assert!(!out.starts_with("data:"), "{out}");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn datauri_base64_and_enc_write() {
        let root = unique_temp("datauri");
        let input = root.join("in.svg");
        fs::write(&input, verbose_svg("uri")).unwrap();

        let b64_path = root.join("b64.txt");
        run_from_args(OptimizeArgs {
            input: Some(input.clone()),
            output: Some(b64_path.clone()),
            datauri: Some(DataUri::Base64),
            ..OptimizeArgs::default()
        })
        .expect("minsvg --datauri base64");
        let b64 = fs::read_to_string(&b64_path).unwrap();
        assert!(b64.starts_with("data:image/svg+xml;base64,"), "{b64}");
        assert!(!b64.contains("<svg"), "{b64}");

        let enc_path = root.join("enc.txt");
        run_from_args(OptimizeArgs {
            input: Some(input),
            output: Some(enc_path.clone()),
            pretty: true,
            datauri: Some(DataUri::Enc),
            ..OptimizeArgs::default()
        })
        .expect("minsvg --pretty --datauri enc");
        let enc = fs::read_to_string(&enc_path).unwrap();
        assert!(enc.starts_with("data:image/svg+xml,"), "{enc}");
        assert!(enc.contains("%3Csvg"), "{enc}");
        let _ = fs::remove_dir_all(&root);
    }

    fn unique_temp(tag: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir =
            std::env::temp_dir().join(format!("minsvg-{}-{}-{}", tag, std::process::id(), nanos));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn verbose_svg(mark: &str) -> Vec<u8> {
        format!(
            r##"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE svg PUBLIC "-//W3C//DTD SVG 1.1//EN" "http://www.w3.org/Graphics/SVG/1.1/DTD/svg11.dtd">
<svg xmlns="http://www.w3.org/2000/svg" width="64" height="64" viewBox="0 0 64 64">
  <!-- {mark} -->
  <g>
    <circle cx="32" cy="32" r="22" fill="#3366FF" stroke="#000000" stroke-width="2"/>
  </g>
</svg>
"##
        )
        .into_bytes()
    }

    fn assert_svg_shrunk_or_valid(src: &[u8], dst: &[u8], label: &str) {
        let text = std::str::from_utf8(dst).unwrap_or("");
        let valid = text.contains("<svg");
        assert!(!dst.is_empty(), "{label} is empty");
        assert!(
            dst.len() <= src.len() || valid,
            "{label} grew ({} -> {}) and is not valid SVG: {text}",
            src.len(),
            dst.len()
        );
        assert!(valid, "{label} is not valid SVG: {text}");
    }
}
