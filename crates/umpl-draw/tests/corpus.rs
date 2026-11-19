//! Layout quality corpus: lays out every example and some generated class
//! diagrams, checks the hard rules, and fails if any crossing count or
//! score got worse than recorded in `tests/corpus/scores.txt`.
//!
//! After an intended change, re-record with:
//!     UPDATE_SCORES=1 cargo test -p umpl-draw --test corpus

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Instant;

use umpl_core::{Document, Editor, ElementKind, Member, RelationKind};
use umpl_draw::Diagram;
use umpl_layout::Options;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

/// A class diagram that looks like real code: a few inheritance trees,
/// associations mostly between nearby classes, some packages.
pub fn generated(seed: u64, n: usize) -> Document {
    generated_with(seed, n, true)
}

pub fn generated_with(seed: u64, n: usize, with_packages: bool) -> Document {
    let mut rng = Rng(seed * 2654435761 + 7);
    let mut ed = Editor::new(Document::new());
    let package_count = if with_packages { n / 25 } else { 0 };
    let packages: Vec<_> = (0..package_count)
        .map(|i| ed.add_package(&format!("pkg{i}"), None))
        .collect();
    let mut ids = Vec::new();
    for i in 0..n {
        // Packages hold runs of related classes, as in real code.
        let pkg = packages.get(i / 25).copied().filter(|_| i % 25 < 15);
        let kind = match rng.below(10) {
            0 => ElementKind::Interface,
            1 => ElementKind::Abstract,
            2 => ElementKind::Enum,
            _ => ElementKind::Class,
        };
        let id = ed.add_element(kind, &format!("Class{i}"), pkg, None);
        let mut el = ed.doc.elements[&id].clone();
        for m in 0..rng.below(5) {
            el.members.push(match kind {
                ElementKind::Enum => Member::plain(format!("V{m}")),
                _ if m % 2 == 0 => Member::parse_line(&format!("+field{m}: Type{m}")),
                _ => Member::parse_line(&format!("+op{m}(x: int): bool")),
            });
        }
        ed.set_element(id, el);
        ids.push(id);
    }
    for i in 1..n {
        // Mostly link to a recent class, like modules in real code.
        let j = i.saturating_sub(1 + rng.below(6.min(i)));
        let kind = match rng.below(10) {
            0..=2 => RelationKind::Generalization,
            3 => RelationKind::Realization,
            4 => RelationKind::Composition,
            5 => RelationKind::Aggregation,
            6 => RelationKind::Dependency,
            _ => RelationKind::Association,
        };
        ed.connect(ids[i], ids[j], kind);
        if rng.below(4) == 0 {
            let k = rng.below(n);
            if k != i {
                ed.connect(ids[i], ids[k], RelationKind::Association);
            }
        }
    }
    ed.doc
}

fn corpus() -> Vec<(String, Document)> {
    let mut out = Vec::new();
    let mut files: Vec<PathBuf> = std::fs::read_dir(root().join("examples"))
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "umpl"))
        .collect();
    files.sort();
    for f in files {
        let name = f.file_stem().unwrap().to_string_lossy().into_owned();
        let doc = umpl_text::parse(&std::fs::read_to_string(&f).unwrap()).doc;
        out.push((name, doc));
    }
    for (seed, n) in [(1, 20), (2, 20), (3, 80), (4, 200)] {
        out.push((format!("gen-{n}-{seed}"), generated(seed, n)));
    }
    out
}

#[test]
fn layout_quality_does_not_regress() {
    let scores_path = root().join("tests/corpus/scores.txt");
    let recorded: BTreeMap<String, (usize, f64)> = std::fs::read_to_string(&scores_path)
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .map(|l| {
            let p: Vec<&str> = l.split_whitespace().collect();
            (
                p[0].to_string(),
                (p[1].parse().unwrap(), p[2].parse().unwrap()),
            )
        })
        .collect();
    let update = std::env::var_os("UPDATE_SCORES").is_some();
    let mut lines = vec!["# name crossings score (lower is better)".to_string()];
    let mut failures = Vec::new();
    for (name, doc) in corpus() {
        let d = Diagram::tidy(&doc, &Options::default());
        let s = d.layout.stats;
        assert_eq!(s.overlaps, 0, "{name}: boxes overlap");
        let score = (s.score() * 10.0).round() / 10.0;
        let b = d.layout.bounds(&d.graph);
        let shape = b.w.max(1) as f64 / b.h.max(1) as f64;
        println!(
            "{name}: {} crossings, score {score}, width : height {shape:.2}",
            s.crossings
        );
        lines.push(format!("{name} {} {score:.1}", s.crossings));
        if let Some(&(c, sc)) = recorded.get(&name) {
            if s.crossings > c || score > sc + 0.05 {
                failures.push(format!(
                    "{name}: {} crossings (was {c}), score {score} (was {sc})",
                    s.crossings
                ));
            }
        } else if !update {
            failures.push(format!(
                "{name}: no recorded score; run with UPDATE_SCORES=1"
            ));
        }
    }
    if update {
        std::fs::create_dir_all(scores_path.parent().unwrap()).unwrap();
        std::fs::write(&scores_path, lines.join("\n") + "\n").unwrap();
        return;
    }
    assert!(
        failures.is_empty(),
        "layout got worse:\n{}",
        failures.join("\n")
    );
}

/// Timing against the design's speed targets (§9). Manual, because
/// shared CI machines vary too much:
///     cargo test --release -p umpl-draw --test corpus -- --ignored --nocapture tidy_timing
/// Current limits are what the grid router reaches today (design targets
/// are 100 ms and 1 s).
#[test]
#[ignore = "manual timing check"]
fn tidy_timing() {
    for (n, limit_ms) in [(100, 1000), (500, 20_000)] {
        let doc = generated(99, n);
        let start = Instant::now();
        let d = Diagram::tidy(&doc, &Options::default());
        let ms = start.elapsed().as_millis();
        println!(
            "tidy {n} classes: {ms} ms, {} crossings",
            d.layout.stats.crossings
        );
        assert!(
            ms <= limit_ms,
            "tidy of {n} classes took {ms} ms (target {limit_ms} ms)"
        );
    }
}

/// Writes generated diagrams to target/ for inspection with the CLI.
#[test]
#[ignore = "manual"]
fn dump_generated() {
    for (seed, n) in [(99, 100), (99, 500), (4, 200)] {
        let text = umpl_text::print(&generated(seed, n));
        std::fs::write(root().join(format!("target/gen-{n}-{seed}.umpl")), text).unwrap();
        let text = umpl_text::print(&generated_with(seed, n, false));
        std::fs::write(root().join(format!("target/flat-{n}-{seed}.umpl")), text).unwrap();
    }
}

/// The benchmark layout changes are judged on (design §6.8): 52 generated
/// diagrams of 40 to 150 classes, with and without packages. Single
/// diagrams swing too much with small changes; totals do not.
///     cargo test --release -p umpl-draw --test corpus -- --ignored --nocapture benchmark
#[test]
#[ignore = "manual benchmark"]
fn benchmark() {
    let (mut crossings, mut score, mut wide) = (0, 0.0, 0);
    let mut shapes = Vec::new();
    let start = Instant::now();
    for seed in 1..=26u64 {
        let n = 40 + (seed as usize * 37) % 111;
        for packages in [true, false] {
            let doc = generated_with(seed, n, packages);
            let d = Diagram::tidy(&doc, &Options::default());
            let s = d.layout.stats;
            assert_eq!(s.overlaps, 0);
            crossings += s.crossings;
            score += s.score();
            let b = d.layout.bounds(&d.graph);
            let shape = b.w as f64 / b.h.max(1) as f64;
            wide += usize::from(!(1.0 / 3.0..=3.0).contains(&shape));
            shapes.push(shape);
        }
    }
    shapes.sort_by(f64::total_cmp);
    println!(
        "52 diagrams: {crossings} crossings, score {score:.0}, width:height median {:.2} \
         (10% {:.2}, 90% {:.2}), {wide} beyond 3:1, {} s",
        shapes[26],
        shapes[5],
        shapes[46],
        start.elapsed().as_secs()
    );
}
