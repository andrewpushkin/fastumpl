//! Opens every PlantUML file in one or more folders the way the editor
//! does, and reports how it went.
//!
//! Not run by default: it needs a corpus. The dev image has three sample
//! repositories in /opt/puml-samples, and `scripts/puml-corpus.py` builds a
//! large one from GitHub in target/puml-corpus:
//!
//! ```text
//! PUML_CORPUS=/opt/puml-samples:target/puml-corpus \
//!     cargo test --release -p umpl-draw --test puml_corpus -- --ignored --nocapture
//! ```
//!
//! Every file that holds a diagram (`@start…`) is imported, following its
//! `!include`s. Class diagrams (and other box-and-line diagrams read as
//! boxes) are then laid out as on open. The test fails on a panic, on a
//! layout that breaks a hard rule (overlapping boxes), and on a file that
//! did not open within a minute. Everything else is reported:
//!
//! - files by diagram kind, and for the ones read: how many opened without
//!   any problem and how many have lines that were not understood;
//! - problems by kind, most frequent first, with an example of each;
//! - the shape of the laid-out drawings (width : height), and the slowest.
//!
//! A table of every file goes to target/puml-corpus-report.tsv.
//! `PUML_CORPUS_SHOW=<text>` lists every problem containing `<text>`;
//! `PUML_CORPUS_LAYOUT=0` skips layout (import only), and
//! `PUML_CORPUS_LAYOUT=<n>` lays out every n-th file only (all are imported).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use umpl_draw::Diagram;
use umpl_layout::Options;
use umpl_text::plantuml;

const EXTENSIONS: [&str; 5] = ["puml", "plantuml", "pu", "wsd", "iuml"];
const SLOW: Duration = Duration::from_secs(60);

fn files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = rd.flatten().collect();
    entries.sort_by_key(|e| e.path());
    for e in entries {
        let p = e.path();
        if p.file_name().is_some_and(|n| n == ".git") {
            continue;
        }
        if p.is_dir() {
            files(&p, out);
        } else if p
            .extension()
            .and_then(|x| x.to_str())
            .is_some_and(|x| EXTENSIONS.contains(&x.to_ascii_lowercase().as_str()))
        {
            out.push(p);
        }
    }
}

/// A problem message with names and quoted text replaced by `…`, so that
/// problems of one kind count together.
fn category(message: &str) -> String {
    let message = match message.find(": ") {
        // "in `x.iuml` line 3: …" — the place does not matter here.
        Some(i) if message.starts_with("in `") => &message[i + 2..],
        _ => message,
    };
    let mut out = String::new();
    let mut quoted = false;
    for c in message.chars() {
        if c == '`' {
            quoted = !quoted;
            if quoted {
                out.push_str("`…`");
            }
        } else if !quoted {
            out.push(c);
        }
    }
    out
}

/// What became of one file.
struct Outcome {
    path: PathBuf,
    kind: String,
    elements: usize,
    relations: usize,
    problems: Vec<(usize, String)>,
    not_understood: usize,
    layout: Option<Layout>,
    panic: Option<String>,
}

struct Layout {
    ms: u128,
    crossings: usize,
    overlaps: usize,
    width: f32,
    height: f32,
}

fn open(path: &Path, with_layout: bool) -> Outcome {
    let mut out = Outcome {
        path: path.to_path_buf(),
        kind: String::new(),
        elements: 0,
        relations: 0,
        problems: Vec::new(),
        not_understood: 0,
        layout: None,
        panic: None,
    };
    let result = std::panic::catch_unwind(|| {
        let parsed = plantuml::import_path(path).expect("readable");
        let kind = plantuml::diagram_kind(&std::fs::read_to_string(path).unwrap_or_default());
        let layout = (with_layout && !parsed.doc.elements.is_empty()).then(|| {
            let start = Instant::now();
            let d = Diagram::tidy(&parsed.doc, &Options::default());
            let ms = start.elapsed().as_millis();
            let b = d.layout.bounds(&d.graph);
            Layout {
                ms,
                crossings: d.layout.stats.crossings,
                overlaps: d.layout.stats.overlaps,
                width: b.w as f32 * umpl_draw::GRID,
                height: b.h as f32 * umpl_draw::GRID,
            }
        });
        (parsed, kind, layout)
    });
    match result {
        Ok((parsed, kind, layout)) => {
            out.kind = kind.name().to_string();
            out.elements = parsed.doc.elements.len();
            out.relations = parsed.doc.relations.len();
            out.not_understood = parsed
                .problems
                .iter()
                .filter(|p| p.message.contains("not understood"))
                .count();
            out.problems = parsed
                .problems
                .into_iter()
                .map(|p| (p.line, p.message))
                .collect();
            out.layout = layout;
        }
        Err(e) => {
            out.panic = Some(
                e.downcast_ref::<String>()
                    .cloned()
                    .or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string()))
                    .unwrap_or_default(),
            )
        }
    }
    out
}

#[test]
#[ignore = "needs PUML_CORPUS"]
fn opens_the_corpus() {
    let dirs = std::env::var("PUML_CORPUS").expect("set PUML_CORPUS to folders, : between");
    // Every n-th file is laid out; 0 for none.
    let every: usize = std::env::var("PUML_CORPUS_LAYOUT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1);
    let show = std::env::var("PUML_CORPUS_SHOW").ok();
    let mut all = Vec::new();
    // Relative folders are taken from the workspace root, not from the
    // crate folder cargo runs tests in.
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    for d in std::env::split_paths(&dirs) {
        files(&root.join(d), &mut all);
    }
    // Files without a diagram are only there to be included.
    let all: Vec<PathBuf> = all
        .into_iter()
        .filter(|p| {
            std::fs::read(p).is_ok_and(|b| {
                String::from_utf8_lossy(&b)
                    .lines()
                    .any(|l| l.trim_start().starts_with("@start"))
            })
        })
        .collect();
    assert!(!all.is_empty(), "no PlantUML diagrams in {dirs}");
    println!("{} files with diagrams", all.len());

    // Files in parallel; each layout runs its candidates in parallel too,
    // within its own memory bound, so only a few files at once.
    let workers = std::thread::available_parallelism().map_or(2, |n| (n.get() / 8).clamp(1, 3));
    let next = AtomicUsize::new(0);
    let done = AtomicUsize::new(0);
    let outcomes: Mutex<Vec<(Outcome, Duration)>> = Mutex::new(Vec::new());
    std::thread::scope(|s| {
        for _ in 0..workers {
            s.spawn(|| loop {
                let i = next.fetch_add(1, Ordering::Relaxed);
                let Some(path) = all.get(i) else { break };
                let start = Instant::now();
                let o = open(path, every > 0 && i.is_multiple_of(every.max(1)));
                let took = start.elapsed();
                let n = done.fetch_add(1, Ordering::Relaxed) + 1;
                if n.is_multiple_of(500) {
                    println!("  {n} / {}", all.len());
                }
                outcomes.lock().unwrap().push((o, took));
            });
        }
    });
    let mut outcomes = outcomes.into_inner().unwrap();
    outcomes.sort_by(|a, b| a.0.path.cmp(&b.0.path));

    // ----- report ----------------------------------------------------------
    let mut kinds: BTreeMap<String, usize> = BTreeMap::new();
    let mut categories: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    let mut examples: BTreeMap<String, String> = BTreeMap::new();
    let (mut read, mut clean, mut unclear, mut elements, mut relations) = (0, 0, 0, 0, 0);
    let mut failures = Vec::new();
    let mut shapes: Vec<(f32, PathBuf)> = Vec::new();
    let mut slow: Vec<(u128, PathBuf, usize)> = Vec::new();
    let mut table = vec![
        "path\tkind\telements\trelations\tproblems\tnot_understood\tlayout_ms\tcrossings\twidth\theight"
            .to_string(),
    ];
    for (o, took) in &outcomes {
        let p = o.path.display();
        if let Some(msg) = &o.panic {
            failures.push(format!("{p}: panicked: {msg}"));
            continue;
        }
        if *took > SLOW {
            failures.push(format!("{p}: took {} s to open", took.as_secs()));
        }
        *kinds.entry(o.kind.clone()).or_default() += 1;
        let l = o.layout.as_ref();
        table.push(format!(
            "{p}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            o.kind,
            o.elements,
            o.relations,
            o.problems.len(),
            o.not_understood,
            l.map_or(String::new(), |l| l.ms.to_string()),
            l.map_or(String::new(), |l| l.crossings.to_string()),
            l.map_or(String::new(), |l| format!("{:.0}", l.width)),
            l.map_or(String::new(), |l| format!("{:.0}", l.height)),
        ));
        if o.elements == 0 && o.relations == 0 {
            // Not read (another kind of diagram) or empty.
            for (line, m) in &o.problems {
                if show.as_deref().is_some_and(|s| m.contains(s)) {
                    println!("{p}:{line}: {m}");
                }
            }
            continue;
        }
        read += 1;
        elements += o.elements;
        relations += o.relations;
        if o.problems.is_empty() {
            clean += 1;
        }
        if o.not_understood > 0 {
            unclear += 1;
        }
        let mut seen_here = std::collections::HashSet::new();
        for (line, m) in &o.problems {
            let c = category(m);
            if show.as_deref().is_some_and(|s| m.contains(s)) {
                println!("{p}:{line}: {m}");
            }
            let e = categories.entry(c.clone()).or_default();
            e.0 += 1;
            if seen_here.insert(c.clone()) {
                e.1 += 1;
            }
            examples
                .entry(c)
                .or_insert_with(|| format!("{p}:{line}: {m}"));
        }
        if let Some(l) = l {
            if l.overlaps > 0 {
                failures.push(format!("{p}: {} overlapping boxes", l.overlaps));
            }
            shapes.push((l.width / l.height.max(1.0), o.path.clone()));
            slow.push((l.ms, o.path.clone(), o.elements));
        }
    }

    let report = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/puml-corpus-report.tsv");
    let _ = std::fs::write(&report, table.join("\n") + "\n");

    println!("\nfiles by kind:");
    for (k, n) in &kinds {
        println!("{n:6}  {k}");
    }
    println!(
        "\n{read} diagrams read: {clean} without any problem, {unclear} with lines not understood; \
         {elements} elements, {relations} relations"
    );
    println!("\nproblems (count, files, kind):");
    let mut by_count: Vec<_> = categories.into_iter().collect();
    by_count.sort_by(|a, b| b.1 .1.cmp(&a.1 .1).then(b.1 .0.cmp(&a.1 .0)));
    for (c, (n, f)) in &by_count {
        println!("{n:6} {f:5}  {c}\n              e.g. {}", examples[c]);
    }
    if !shapes.is_empty() {
        shapes.sort_by(|a, b| a.0.total_cmp(&b.0));
        let q = |f: f64| shapes[((shapes.len() - 1) as f64 * f) as usize].0;
        let wide = shapes.iter().filter(|s| s.0 > 3.0).count();
        let tall = shapes.iter().filter(|s| s.0 < 1.0 / 3.0).count();
        println!(
            "\nwidth : height of {} drawings: median {:.2}, 10% {:.2}, 90% {:.2}; \
             {wide} wider than 3:1, {tall} taller than 1:3",
            shapes.len(),
            q(0.5),
            q(0.1),
            q(0.9)
        );
        for (r, p) in shapes.iter().rev().take(5) {
            println!("  {r:6.2}  {}", p.display());
        }
        slow.sort_by(|a, b| b.0.cmp(&a.0));
        println!("slowest layouts:");
        for (ms, p, n) in slow.iter().take(8) {
            println!("  {ms:6} ms  {n:4} elements  {}", p.display());
        }
    }
    println!("\ntable of every file: {}", report.display());
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
