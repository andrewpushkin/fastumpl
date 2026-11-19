//! QA: save-and-reopen keeps a diagram exactly, for values a user can type
//! into the editor (any name without control characters, any member, label,
//! stereotype or relation name text).
//!
//! The existing `roundtrip.rs` generator uses only plain names and fixed
//! members; this one uses hostile ones and compares the whole model, field
//! by field, after `print` → `parse`. Found QA-02, QA-03 and QA-19.

use std::collections::BTreeMap;

use umpl_core::*;
use umpl_text::{parse, print};

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
    fn chance(&mut self, percent: u64) -> bool {
        self.next() % 100 < percent
    }
    fn pick<'a>(&mut self, xs: &[&'a str]) -> &'a str {
        xs[self.below(xs.len())]
    }
}

/// Names the Name box of the inspector accepts (`valid_name`).
const NAMES: &[&str] = &[
    "Order",
    "Order line",
    "a.b",
    "say \"hi\"",
    "back\\slash",
    "Заказ",
    "注文",
    "#tag",
    "{",
    "}",
    "a:b",
    "layout",
    "class",
    "note",
    "diagram",
    "abstract",
    "1st",
    "-dash",
    "o",
    "x -- y",
    "a, b",
    "<<x>>",
];

/// Lines typed into a class body in the inline editor or inspector.
const MEMBERS: &[&str] = &[
    "+id: OrderId",
    "}",
    "\\escaped",
    "{",
    "+x: int }",
    "# protected thing",
    "{static}",
    "+run() {abstract} now",
    "layout {",
    "a, b",
    "\"quoted\"",
    "-map: Map<K, V>",
];

const ENUM_LITERALS: &[&str] = &[
    "RED",
    "A_B",
    "Ünï",
    "x y",
    "{",
    "}",
    "RED(255, 0, 0)",
    "\\x",
];
const STEREOTYPES: &[&str] = &["entity", "a b", "a>>b", "«x»", "x\"y"];
const TYPE_PARAMS: &[&str] = &["T", "K, V", "T extends List<K>", "a>b", "x\"y"];
const LABELS: &[&str] = &["Purchase order", "a \"b\" c", "x\\y", "", "as"];
const REL_NAMES: &[&str] = &[
    "uses",
    "C:\\new\\file",
    "a\\\\b",
    "a : b",
    "#hash",
    "two\nlines",
    "ends with \\",
];
const END_LABELS: &[&str] = &["1", "0..* items", "\"q\"", "a\\b", "x : y"];
const NOTE_LINES: &[&str] = &["text", "say \"hi\"", "back\\slash", "a\\nb", ""];

fn hostile_document(seed: u64) -> Document {
    let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    let mut ed = Editor::new(Document::new());
    let mut packages = vec![None];
    for _ in 0..rng.below(3) {
        let parent = packages[rng.below(packages.len())];
        let name = rng.pick(NAMES);
        packages.push(Some(ed.add_package(name, parent)));
    }
    let mut ids = Vec::new();
    for _ in 0..1 + rng.below(8) {
        let kind = ElementKind::ALL[rng.below(ElementKind::ALL.len())];
        let pkg = packages[rng.below(packages.len())];
        let pos = Some(Pos::new(rng.below(100) as i32, rng.below(100) as i32));
        let name = rng.pick(NAMES);
        let id = ed.add_element(kind, name, pkg, pos);
        let mut el = ed.doc.elements[&id].clone();
        for _ in 0..rng.below(4) {
            el.members.push(match kind {
                ElementKind::Enum => Member::plain(rng.pick(ENUM_LITERALS)),
                ElementKind::Note => Member::plain(rng.pick(NOTE_LINES)),
                _ => Member::parse_line(rng.pick(MEMBERS)),
            });
        }
        if kind != ElementKind::Note {
            if rng.chance(30) {
                el.stereotype = Some(rng.pick(STEREOTYPES).into());
            }
            if rng.chance(30) {
                el.type_params = Some(rng.pick(TYPE_PARAMS).into());
            }
            if rng.chance(30) {
                el.label = Some(rng.pick(LABELS).into());
            }
        }
        ed.set_element(id, el);
        ids.push(id);
    }
    for _ in 0..rng.below(6) {
        let a = ids[rng.below(ids.len())];
        let b = ids[rng.below(ids.len())];
        let kind = RelationKind::ALL[rng.below(RelationKind::ALL.len())];
        let r = ed.connect(a, b, kind);
        let mut rel = ed.doc.relations[&r].clone();
        if rng.chance(40) {
            rel.from.label = Some(rng.pick(END_LABELS).into());
        }
        if rng.chance(40) {
            rel.to.label = Some(rng.pick(END_LABELS).into());
        }
        if rng.chance(50) {
            rel.name = Some(rng.pick(REL_NAMES).into());
        }
        ed.set_relation(r, rel);
    }
    ed.doc
}

/// Everything a reader of the diagram can see, keyed by full name.
#[derive(Debug, PartialEq)]
struct Shape {
    elements: BTreeMap<String, String>,
    relations: Vec<String>,
    packages: Vec<String>,
}

fn shape(doc: &Document) -> Shape {
    let elements = doc
        .elements
        .keys()
        .map(|id| {
            let el = &doc.elements[id];
            // A note without lines is written `""` and reads back as one
            // empty line: the same box.
            let empty_note =
                el.kind == ElementKind::Note && el.members.iter().all(|m| m.text.is_empty());
            let members: Vec<String> = el
                .members
                .iter()
                .filter(|_| !empty_note)
                .map(|m| format!("{:?}|{}|{}|{}", m.vis, m.text, m.is_static, m.is_abstract))
                .collect();
            (
                doc.full_name(*id),
                format!(
                    "{:?} label={:?} params={:?} stereo={:?} pos={:?} members={:?}",
                    el.kind, el.label, el.type_params, el.stereotype, el.pos, members
                ),
            )
        })
        .collect();
    let mut relations: Vec<String> = doc
        .relations_in_order()
        .iter()
        .map(|id| {
            let r = &doc.relations[id];
            format!(
                "{} {:?} {} from={:?} to={:?} name={:?}",
                doc.endpoint_name(r.from.target),
                r.kind,
                doc.endpoint_name(r.to.target),
                r.from.label,
                r.to.label,
                r.name
            )
        })
        .collect();
    relations.sort();
    let packages = doc
        .packages_in_order()
        .iter()
        .map(|p| doc.full_package_name(*p))
        .collect();
    Shape {
        elements,
        relations,
        packages,
    }
}

/// Which field differs first, as a short failure signature.
fn first_difference(a: &Shape, b: &Shape) -> String {
    if a.packages != b.packages {
        return format!("packages {:?} -> {:?}", a.packages, b.packages);
    }
    for (name, want) in &a.elements {
        match b.elements.get(name) {
            None => return format!("element {name} missing after reopen"),
            Some(got) if got != want => {
                let field = ["label=", "params=", "stereo=", "pos=", "members="]
                    .iter()
                    .find(|f| field_of(want, f) != field_of(got, f))
                    .unwrap_or(&"kind");
                return format!(
                    "element field {field} changed\n   element {name}\n   saved  {want}\n   reread {got}"
                );
            }
            _ => {}
        }
    }
    if a.elements.len() != b.elements.len() {
        let extra: Vec<_> = b
            .elements
            .keys()
            .filter(|k| !a.elements.contains_key(*k))
            .collect();
        return format!("extra elements after reopen: {extra:?}");
    }
    for (x, y) in a.relations.iter().zip(&b.relations) {
        if x != y {
            let field = ["from=", "to=", "name="]
                .iter()
                .find(|f| field_of(x, f) != field_of(y, f))
                .unwrap_or(&"ends/kind");
            return format!("relation field {field} changed\n   saved  {x}\n   reread {y}");
        }
    }
    format!(
        "relation count {} -> {}",
        a.relations.len(),
        b.relations.len()
    )
}

/// The text of `field` (such as `label=`) in a shape line.
fn field_of<'a>(line: &'a str, field: &str) -> &'a str {
    match line.find(field) {
        Some(i) => {
            let rest = &line[i..];
            let end = [
                " label=",
                " params=",
                " stereo=",
                " pos=",
                " members=",
                " to=",
                " name=",
            ]
            .iter()
            .filter_map(|f| rest[1..].find(f).map(|j| j + 1))
            .min()
            .unwrap_or(rest.len());
            &rest[..end]
        }
        None => "",
    }
}

#[test]
fn saving_and_reopening_keeps_every_editor_value() {
    let mut failures: BTreeMap<String, (u64, String)> = BTreeMap::new();
    let mut failed = 0;
    let runs = 2000;
    for seed in 0..runs {
        let doc = hostile_document(seed);
        let text = print(&doc);
        let parsed = parse(&text);
        let before = shape(&doc);
        let after = shape(&parsed.doc);
        let mut sig = None;
        if !parsed.problems.is_empty() {
            sig = Some(format!(
                "problems on reopen: {}",
                parsed.problems[0].message
            ));
        } else if before != after {
            sig = Some(first_difference(&before, &after));
        }
        if let Some(s) = sig {
            failed += 1;
            // Group by the first line, so each kind of failure shows once.
            let key: String = s.lines().next().unwrap_or("").to_string();
            let key = if key.starts_with("element ") && key.ends_with(" missing after reopen") {
                "element missing after reopen".to_string()
            } else if key.starts_with("extra elements") {
                "extra elements after reopen".to_string()
            } else {
                key
            };
            failures
                .entry(key)
                .or_insert((seed, format!("{s}\n--- file:\n{text}")));
        }
    }
    if !failures.is_empty() {
        let mut report = format!("{failed} of {runs} documents changed on save and reopen\n");
        for (_, (seed, detail)) in failures.iter().take(12) {
            report.push_str(&format!("\n=== seed {seed}\n{detail}\n"));
        }
        panic!("{report}");
    }
}
