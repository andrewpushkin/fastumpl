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
//!
//! With target/puml-check.tsv from `scripts/dev.sh scripts/puml-check.sh`
//! (PlantUML's own verdict on every file) the report also says how many of
//! the files PlantUML reads without error open here with no problem; the
//! problem kinds count those files only (files PlantUML rejects are broken
//! in their own source). It also lists files where PlantUML sees another
//! kind of diagram, or another number of classes and notes.
//! `PUML_CORPUS_SHOW=<text>` lists every problem containing `<text>`;
//! `PUML_CORPUS_LAYOUT=0` skips layout (import only), and
//! `PUML_CORPUS_LAYOUT=<n>` lays out every n-th file only (all are imported).
//! `PUML_CORPUS_GOAL=fill` lays out to fill the view instead of for the
//! fewest crossings.
//! `PUML_CORPUS_NAMES=<file>` writes the name and kind of every element
//! read, `path<TAB>kind<TAB>name`, to compare with what PlantUML read
//! (`scripts/PumlCheck.java --dump`).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use umpl_draw::Diagram;
use umpl_layout::{Goal, Options};
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

/// PlantUML's verdict on the first diagram of a file (scripts/PumlCheck.java).
struct Verdict {
    /// PlantUML's class name for the diagram: `ClassDiagram`, …
    kind: String,
    ok: bool,
    /// Classes and notes, from PlantUML's "(N entities)".
    entities: Option<usize>,
    message: String,
}

/// target/puml-check.tsv, by path relative to the workspace root (or
/// absolute, for files outside it).
fn verdicts(root: &Path) -> BTreeMap<String, Verdict> {
    let mut out = BTreeMap::new();
    let Ok(text) = std::fs::read(root.join("target/puml-check.tsv")) else {
        return out;
    };
    for line in String::from_utf8_lossy(&text).lines() {
        let f: Vec<&str> = line.split('\t').collect();
        if f.len() < 6 || !(f[1] == "1" || f[1] == "0") || out.contains_key(f[0]) {
            continue;
        }
        let entities = f[5]
            .strip_prefix('(')
            .and_then(|d| d.split(' ').next())
            .and_then(|n| n.parse().ok());
        out.insert(
            f[0].to_string(),
            Verdict {
                kind: f[2].to_string(),
                ok: f[3] == "ok",
                entities: entities.filter(|_| f[3] == "ok"),
                message: f[5].to_string(),
            },
        );
    }
    out
}

/// The key of `path` in [`verdicts`].
fn key(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .into_owned()
}

/// What became of one file.
struct Outcome {
    path: PathBuf,
    kind: String,
    elements: usize,
    relations: usize,
    problems: Vec<(usize, String)>,
    /// Kind and name of every element.
    names: Vec<(String, String)>,
    not_understood: usize,
    layout: Option<Layout>,
    panic: Option<String>,
}

struct Layout {
    ms: u128,
    crossings: usize,
    overlaps: usize,
    /// Labels drawn over a box, a label, a line or a package frame.
    label_overlaps: usize,
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
        names: Vec::new(),
        not_understood: 0,
        layout: None,
        panic: None,
    };
    let result = std::panic::catch_unwind(|| {
        let parsed = plantuml::import_path(path).expect("readable");
        let kind = plantuml::diagram_kind(&std::fs::read_to_string(path).unwrap_or_default());
        // A sequence diagram is drawn straight from the document.
        if let (true, Some(seq)) = (with_layout, &parsed.doc.sequence) {
            let start = Instant::now();
            let theme = umpl_draw::Theme::light();
            let scene = umpl_draw::sequence::scene(seq, parsed.doc.title.as_deref(), &theme);
            let layout = Layout {
                ms: start.elapsed().as_millis(),
                crossings: 0,
                overlaps: 0,
                label_overlaps: 0,
                width: scene.bounds.w,
                height: scene.bounds.h,
            };
            return (parsed, kind, Some(layout));
        }
        let layout = (with_layout && !parsed.doc.elements.is_empty()).then(|| {
            let start = Instant::now();
            let goal = match std::env::var("PUML_CORPUS_GOAL").as_deref() {
                Ok("fill") => Goal::FillView,
                _ => Goal::FewestCrossings,
            };
            let opts = Options {
                goal,
                ..Options::default()
            };
            let d = Diagram::tidy(&parsed.doc, &opts);
            let ms = start.elapsed().as_millis();
            let b = d.layout.bounds(&d.graph);
            Layout {
                ms,
                crossings: d.layout.stats.crossings,
                overlaps: d.layout.stats.overlaps,
                label_overlaps: d.layout.stats.label_overlaps,
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
            // A sequence diagram: its participants and messages. Written
            // back as PlantUML, it must read the same.
            if let Some(seq) = &parsed.doc.sequence {
                out.elements = seq.participants.len();
                out.relations = seq.messages().count();
                let again = plantuml::import(&plantuml::export(&parsed.doc));
                if let Some(b) = again.doc.sequence.as_ref().filter(|b| *b != seq) {
                    let what = if b.participants != seq.participants {
                        format!(
                            "participants {:?} / {:?}",
                            seq.participants.iter().map(|p| &p.name).collect::<Vec<_>>(),
                            b.participants.iter().map(|p| &p.name).collect::<Vec<_>>()
                        )
                    } else {
                        let k = seq.steps.iter().zip(&b.steps).position(|(x, y)| x != y);
                        match k {
                            Some(k) => format!("step {k}: {:?} / {:?}", seq.steps[k], b.steps[k]),
                            None => format!("{} / {} steps", seq.steps.len(), b.steps.len()),
                        }
                    };
                    out.problems
                        .push((0, format!("written back, reads differently: {what}")));
                }
            }
            out.names = parsed
                .doc
                .elements
                .values()
                .map(|e| (format!("{:?}", e.kind), e.shown().replace('\n', "\\n")))
                .collect();
            out.not_understood = parsed
                .problems
                .iter()
                .filter(|p| p.message.contains("not understood"))
                .count();
            out.problems
                .extend(parsed.problems.into_iter().map(|p| (p.line, p.message)));
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
    let checked = verdicts(&root);
    let (mut valid, mut valid_clean) = (0, 0);
    let mut other_kind: Vec<String> = Vec::new();
    let mut other_count: Vec<(i64, String)> = Vec::new();
    let mut kinds: BTreeMap<String, usize> = BTreeMap::new();
    let mut categories: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    let mut examples: BTreeMap<String, String> = BTreeMap::new();
    let (mut read, mut clean, mut unclear, mut elements, mut relations) = (0, 0, 0, 0, 0);
    let mut failures = Vec::new();
    let mut shapes: Vec<(f32, PathBuf)> = Vec::new();
    let mut crossings = 0usize;
    let mut label_overlaps = 0usize;
    let mut with_label_overlaps: Vec<(usize, std::path::PathBuf)> = Vec::new();
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
        let verdict = checked.get(&key(&root, &o.path));
        if let Some(v) = verdict.filter(|v| v.ok) {
            valid += 1;
            if o.problems.is_empty() {
                valid_clean += 1;
            }
            if !matches!(
                v.kind.as_str(),
                "ClassDiagram" | "DescriptionDiagram" | "SequenceDiagram"
            ) {
                other_kind.push(format!("{} {} ({p})", v.kind, v.message));
            }
            if let Some(n) = v.entities {
                if n != o.elements {
                    other_count.push((o.elements as i64 - n as i64, p.to_string()));
                }
            }
        }
        // With PlantUML's verdicts, problems are counted in valid files only.
        if !checked.is_empty() && !verdict.is_some_and(|v| v.ok) {
            if o.not_understood > 0 {
                unclear += 1;
            }
            continue;
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
            crossings += l.crossings;
            label_overlaps += l.label_overlaps;
            if l.label_overlaps > 0 {
                with_label_overlaps.push((l.label_overlaps, o.path.clone()));
            }
            slow.push((l.ms, o.path.clone(), o.elements));
        }
    }

    if let Ok(file) = std::env::var("PUML_CORPUS_NAMES") {
        let mut lines = Vec::new();
        for (o, _) in &outcomes {
            for (kind, name) in &o.names {
                lines.push(format!("{}\t{kind}\t{name}", key(&root, &o.path)));
            }
        }
        let _ = std::fs::write(root.join(file), lines.join("\n") + "\n");
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
    if !checked.is_empty() {
        println!(
            "PlantUML reads {valid} of them without error; {valid_clean} of those ({:.1} %) \
             open here without any problem",
            100.0 * valid_clean as f64 / valid.max(1) as f64
        );
        println!(
            "PlantUML sees another kind of diagram in {} of them:",
            other_kind.len()
        );
        for k in other_kind.iter().take(8) {
            println!("  {k}");
        }
        other_count.sort();
        println!(
            "and another number of classes and notes in {} (here minus PlantUML's):",
            other_count.len()
        );
        let n = other_count.len();
        for (d, p) in other_count
            .iter()
            .take(5)
            .chain(other_count.iter().skip(n.saturating_sub(5).max(5)))
        {
            println!("  {d:+6}  {p}");
        }
    }
    let scope = if checked.is_empty() {
        ""
    } else {
        " in files PlantUML reads without error"
    };
    println!("\nproblems{scope} (count, files, kind):");
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
        // How much of a 16:10 view (the layout's default) each drawing
        // fills when zoomed to fit, and the crossings paid for it.
        let aspect = umpl_layout::DEFAULT_ASPECT;
        let mut fill: Vec<f64> = shapes
            .iter()
            .map(|s| (-((s.0 as f64) / aspect).ln().abs()).exp())
            .collect();
        fill.sort_by(|a, b| a.total_cmp(b));
        let most = fill.iter().filter(|&&f| f >= 0.8).count();
        println!(
            "view filled: median {:.0} %, 10% of drawings under {:.0} %; {most} of {} fill 80 % or more; \
             {crossings} crossings in all",
            100.0 * fill[fill.len() / 2],
            100.0 * fill[fill.len() / 10],
            fill.len()
        );
        with_label_overlaps.sort();
        println!(
            "labels drawn over something: {label_overlaps} in {} drawings",
            with_label_overlaps.len()
        );
        for (n, p) in with_label_overlaps.iter().rev().take(5) {
            println!("  {n:5}  {}", p.display());
        }
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
