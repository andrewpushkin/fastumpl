//! `umpl`: work with .umpl diagrams without a window.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

use umpl_core::Editor;
use umpl_draw::{build_scene, svg::to_svg, Diagram, SceneOptions, Theme};
use umpl_layout::{Goal, Options};
use umpl_text::{parse, print, read_path, write_as, write_atomic, Format, Parsed};

/// Release version (0.1.N, N = commits on main), set by the build as
/// UMPL_VERSION; plain builds show the Cargo version.
const VERSION: &str = match option_env!("UMPL_VERSION") {
    Some(v) => v,
    None => env!("CARGO_PKG_VERSION"),
};

const HELP: &str = "\
umpl - render, format and check fastumpl diagrams

Usage:
  umpl render <file> -o <out.svg|out.png> [--tidy] [--fill] [--dark] [--scale N]
  umpl convert <file> -o <out.umpl|out.puml|out.mmd|out.md|out.ump> [--tidy] [--fill]
  umpl convert <file> --to <umpl|plantuml|mermaid|markdown|umple>   to stdout
  umpl tidy [--fill] <file.umpl>...  lay out again and save positions
  umpl fmt [--check] <file.umpl>...
  umpl check <file>...             list problems; exit 1 if any
  umpl stats <file> [--tidy] [--fill]  crossings, bends and score
  umpl --version

A PlantUML file may hold several diagrams (and pages split by newpage);
--diagram N reads the N-th, counting from 1 (default: the first).

Input files are .umpl, PlantUML (.puml, .plantuml, .pu, .iuml, .wsd) or
Umple (.ump; the class model, without state machines or code). Elements
without a stored position are placed automatically; --tidy lays out the
whole diagram from scratch, for the fewest line crossings, or with --fill
to fill a 16:10 window (a few more crossings where need be); --aspect W:H
gives the window another shape. Mermaid, PlantUML and Umple output carry
no positions.";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("render") => render(&args[1..]),
        Some("convert") => convert(&args[1..]),
        Some("tidy") => tidy(&args[1..]),
        Some("fmt") => fmt(&args[1..]),
        Some("check") => check(&args[1..]),
        Some("stats") => stats(&args[1..]),
        Some("--version" | "-V") => {
            println!("umpl {VERSION}");
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
    /// `--fill`: lay out to fill the view rather than for the fewest
    /// crossings.
    fill: bool,
    /// `--aspect W:H`: the shape of the window `--fill` fills.
    aspect: Option<f64>,
    dark: bool,
    check: bool,
    scale: f32,
    to: Option<Format>,
    /// `--diagram N`: which diagram of a PlantUML file, from 0.
    diagram: Option<usize>,
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
            "--fill" => a.fill = true,
            "--dark" => a.dark = true,
            "--check" => a.check = true,
            "--to" => {
                let name = it.next().ok_or("--to needs a format")?;
                a.to = Some(Format::from_name(name).ok_or(format!(
                    "unknown format `{name}` (umpl, plantuml, mermaid, markdown, umple)"
                ))?)
            }
            "--diagram" => {
                a.diagram = Some(
                    it.next()
                        .and_then(|s| s.parse::<usize>().ok())
                        .filter(|n| *n >= 1)
                        .ok_or("--diagram needs a number from 1")?
                        - 1,
                )
            }
            "--aspect" => {
                let s = it.next().ok_or("--aspect needs a shape such as 16:10")?;
                let ratio = match s.split_once(':') {
                    Some((w, h)) => w
                        .parse::<f64>()
                        .ok()
                        .zip(h.parse::<f64>().ok())
                        .map(|(w, h)| w / h),
                    None => s.parse::<f64>().ok(),
                };
                a.aspect = Some(
                    ratio
                        .filter(|r| (0.2..=5.0).contains(r))
                        .ok_or("--aspect needs a shape such as 16:10, between 1:5 and 5:1")?,
                );
            }
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

/// The format of an input file: PlantUML or Umple by extension, otherwise
/// `.umpl`.
fn input_format(path: &Path) -> Format {
    match Format::from_path(path) {
        Some(f) if f.can_read() => f,
        _ => Format::Umpl,
    }
}

/// Reads a file; of a PlantUML file with several diagrams, diagram `n` (the
/// first if `None`, with a note that there are more).
fn read(path: &Path, n: Option<usize>) -> Result<Parsed, String> {
    let err = |e: std::io::Error| format!("{}: {e}", path.display());
    let format = input_format(path);
    if format != Format::PlantUml {
        return read_path(format, path).map_err(err);
    }
    let src = std::fs::read(path).map_err(err)?;
    let count = umpl_text::plantuml::diagrams(&String::from_utf8_lossy(&src)).len();
    match n {
        Some(n) if n >= count => {
            return Err(format!(
                "{}: has {count} diagram(s), not {}",
                path.display(),
                n + 1
            ))
        }
        None if count > 1 => eprintln!(
            "{}: holds {count} diagrams; reading the first (choose with --diagram N)",
            path.display()
        ),
        _ => {}
    }
    umpl_text::plantuml::import_path_nth(path, n.unwrap_or(0)).map_err(err)
}

/// `fmt` and `tidy` rewrite their input as `.umpl`, which would destroy a
/// PlantUML or Umple file.
fn refuse_foreign(path: &Path, command: &str) -> Result<(), String> {
    match input_format(path) {
        Format::Umpl => Ok(()),
        _ => Err(format!(
            "{}: `{command}` rewrites .umpl files only; use `umpl convert {} -o <file.umpl>` first",
            path.display(),
            path.display()
        )),
    }
}

/// The layout options `--fill` asks for.
fn layout(a: &Args) -> Options {
    Options {
        goal: if a.fill {
            Goal::FillView
        } else {
            Goal::FewestCrossings
        },
        aspect: a.aspect.unwrap_or(umpl_layout::DEFAULT_ASPECT),
        ..Options::default()
    }
}

fn arrange(parsed: &Parsed, a: &Args) -> Diagram {
    if a.tidy {
        Diagram::tidy(&parsed.doc, &layout(a))
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
    let parsed = read(input, a.diagram)?;
    report(input, &parsed);
    let diagram = arrange(&parsed, &a);
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

fn convert(args: &[String]) -> Result<bool, String> {
    let a = parse_args(args)?;
    let [input] = &a.files[..] else {
        return Err("convert takes one input file".into());
    };
    let format = match (&a.out, a.to) {
        (_, Some(f)) => f,
        (Some(out), None) => Format::from_path(out).ok_or(format!(
            "{}: output must end in .umpl, .puml, .mmd, .md or .ump (or use --to)",
            out.display()
        ))?,
        (None, None) => return Err("convert needs -o <file> or --to <format>".into()),
    };
    let parsed = read(input, a.diagram)?;
    report(input, &parsed);
    let mut doc = parsed.doc;
    if a.tidy {
        let diagram = Diagram::tidy(&doc, &layout(&a));
        let mut ed = Editor::new(doc);
        ed.set_positions("Tidy", &diagram.positions());
        doc = ed.doc;
    }
    let text = write_as(format, &doc);
    match &a.out {
        Some(out) => {
            write_atomic(out, text.as_bytes()).map_err(|e| format!("{}: {e}", out.display()))?
        }
        None => print!("{text}"),
    }
    Ok(true)
}

fn tidy(args: &[String]) -> Result<bool, String> {
    let a = parse_args(args)?;
    for path in &a.files {
        refuse_foreign(path, "tidy")?;
        let parsed = read(path, a.diagram)?;
        report(path, &parsed);
        let diagram = Diagram::tidy(&parsed.doc, &layout(&a));
        let mut ed = Editor::new(parsed.doc);
        ed.set_positions("Tidy", &diagram.positions());
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
        refuse_foreign(path, "fmt")?;
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
        let parsed = read(path, a.diagram)?;
        report(path, &parsed);
        ok &= parsed.problems.is_empty();
    }
    Ok(ok)
}

fn stats(args: &[String]) -> Result<bool, String> {
    let a = parse_args(args)?;
    for path in &a.files {
        let parsed = read(path, a.diagram)?;
        let start = Instant::now();
        let d = arrange(&parsed, &a);
        let s = d.layout.stats;
        let b = d.layout.bounds(&d.graph);
        println!(
            "{}: {} boxes, {} lines, size {}x{}{}, {} crossings, {} bends, length {}, {} upward, {} overlaps, {} shared lanes, score {:.1}, {} ms",
            path.display(),
            d.graph.nodes.len(),
            d.graph.edges.len(),
            b.w,
            b.h,
            if s.draft { " (draft lines)" } else { "" },
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
