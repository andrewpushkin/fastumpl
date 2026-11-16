//! `umpl`: work with .umpl diagrams without a window.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

use umpl_core::Editor;
use umpl_draw::{build_scene, svg::to_svg, Diagram, SceneOptions, Theme};
use umpl_layout::Options;
use umpl_text::{parse, print, write_atomic, Parsed};

const HELP: &str = "\
umpl - render, format and check fastumpl diagrams

Usage:
  umpl render <file.umpl> -o <out.svg|out.png> [--tidy] [--dark] [--scale N]
  umpl tidy <file.umpl>...         lay out again and save positions
  umpl fmt [--check] <file.umpl>...
  umpl check <file.umpl>...        list problems; exit 1 if any
  umpl stats <file.umpl> [--tidy]  crossings, bends and score
  umpl --version

Elements without a stored position are placed automatically; --tidy lays
out the whole diagram from scratch.";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("render") => render(&args[1..]),
        Some("tidy") => tidy(&args[1..]),
        Some("fmt") => fmt(&args[1..]),
        Some("check") => check(&args[1..]),
        Some("stats") => stats(&args[1..]),
        Some("--version" | "-V") => {
            println!("umpl {}", env!("CARGO_PKG_VERSION"));
            Ok(true)
        }
        Some("--help" | "-h" | "help") | None => {
            println!("{HELP}");
            Ok(true)
        }
        Some(other) => Err(format!("unknown command `{other}`\n\n{HELP}")),
    };
    match result {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(e) => {
            eprintln!("umpl: {e}");
            ExitCode::from(2)
        }
    }
}

/// Parsed command-line flags and file arguments.
#[derive(Default)]
struct Args {
    files: Vec<PathBuf>,
    out: Option<PathBuf>,
    tidy: bool,
    dark: bool,
    check: bool,
    scale: f32,
}

fn parse_args(args: &[String]) -> Result<Args, String> {
    let mut a = Args {
        scale: 2.0,
        ..Args::default()
    };
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "-o" | "--output" => a.out = Some(it.next().ok_or("-o needs a file")?.into()),
            "--tidy" => a.tidy = true,
            "--dark" => a.dark = true,
            "--check" => a.check = true,
            "--scale" => {
                a.scale = it
                    .next()
                    .and_then(|s| s.parse().ok())
                    .filter(|s: &f32| *s > 0.0 && *s <= 8.0)
                    .ok_or("--scale needs a number between 0 and 8")?
            }
            s if s.starts_with('-') => return Err(format!("unknown option `{s}`")),
            s => a.files.push(s.into()),
        }
    }
    if a.files.is_empty() {
        return Err("no input file".into());
    }
    Ok(a)
}

fn read(path: &Path) -> Result<Parsed, String> {
    let src = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(parse(&src))
}

fn arrange(parsed: &Parsed, tidy: bool) -> Diagram {
    if tidy {
        Diagram::tidy(&parsed.doc, &Options::default())
    } else {
        Diagram::arrange(&parsed.doc)
    }
}

fn report(path: &Path, parsed: &Parsed) {
    for p in &parsed.problems {
        eprintln!("{}:{}:{}: {}", path.display(), p.line, p.col, p.message);
    }
}

fn render(args: &[String]) -> Result<bool, String> {
    let a = parse_args(args)?;
    let [input] = &a.files[..] else {
        return Err("render takes one input file".into());
    };
    let out = a.out.clone().ok_or("render needs -o <file>")?;
    let parsed = read(input)?;
    report(input, &parsed);
    let diagram = arrange(&parsed, a.tidy);
    let theme = if a.dark {
        Theme::dark()
    } else {
        Theme::light()
    };
    let scene = build_scene(&parsed.doc, &diagram, &theme, &SceneOptions::default());
    let svg = to_svg(&scene, &theme);
    let bytes = match out
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("svg") => svg.into_bytes(),
        Some("png") => umpl_draw::png::svg_to_png(&svg, a.scale)?,
        _ => return Err("output must end in .svg or .png".into()),
    };
    write_atomic(&out, &bytes).map_err(|e| format!("{}: {e}", out.display()))?;
    Ok(true)
}

fn tidy(args: &[String]) -> Result<bool, String> {
    let a = parse_args(args)?;
    for path in &a.files {
        let parsed = read(path)?;
        report(path, &parsed);
        let diagram = Diagram::tidy(&parsed.doc, &Options::default());
        let mut ed = Editor::new(parsed.doc);
        ed.set_positions(&diagram.positions());
        write_atomic(path, print(&ed.doc).as_bytes())
            .map_err(|e| format!("{}: {e}", path.display()))?;
        println!(
            "{}: {} crossings",
            path.display(),
            diagram.layout.stats.crossings
        );
    }
    Ok(true)
}

fn fmt(args: &[String]) -> Result<bool, String> {
    let a = parse_args(args)?;
    let mut ok = true;
    for path in &a.files {
        let src = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let formatted = print(&parse(&src).doc);
        if formatted == src {
            continue;
        }
        if a.check {
            println!("{}: not formatted", path.display());
            ok = false;
        } else {
            write_atomic(path, formatted.as_bytes())
                .map_err(|e| format!("{}: {e}", path.display()))?;
        }
    }
    Ok(ok)
}

fn check(args: &[String]) -> Result<bool, String> {
    let a = parse_args(args)?;
    let mut ok = true;
    for path in &a.files {
        let parsed = read(path)?;
        report(path, &parsed);
        ok &= parsed.problems.is_empty();
    }
    Ok(ok)
}

fn stats(args: &[String]) -> Result<bool, String> {
    let a = parse_args(args)?;
    for path in &a.files {
        let parsed = read(path)?;
        let start = Instant::now();
        let d = arrange(&parsed, a.tidy);
        let s = d.layout.stats;
        println!(
            "{}: {} boxes, {} lines, {} crossings, {} bends, length {}, {} upward, {} overlaps, {} shared lanes, score {:.1}, {} ms",
            path.display(),
            d.graph.nodes.len(),
            d.graph.edges.len(),
            s.crossings,
            s.bends,
            s.length,
            s.upward_violations,
            s.overlaps,
            s.shared_lanes,
            s.score(),
            start.elapsed().as_millis()
        );
    }
    Ok(true)
}
