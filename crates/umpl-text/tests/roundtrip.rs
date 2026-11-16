use umpl_core::*;
use umpl_text::{parse, print};

const DESIGN_EXAMPLE: &str = include_str!("../../../examples/shop.umpl");

/// Small deterministic generator so failures reproduce.
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
}

const MEMBERS: [&str; 8] = [
    "+id: OrderId",
    "-lines: List<OrderLine>",
    "+total(): Money",
    "#count: int {static}",
    "+pay(): Receipt {abstract}",
    "~helper(a: int, b: String): Map<K, V>",
    "+name: String",
    "-cache: Option<Vec<u8>>",
];

fn random_document(seed: u64) -> Document {
    let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    let mut ed = Editor::new(Document::new());
    if rng.chance(50) {
        ed.set_title(Some(format!("Diagram \"{seed}\"")));
    }
    let mut packages = vec![None];
    for i in 0..rng.below(3) {
        let parent = packages[rng.below(packages.len())];
        packages.push(Some(ed.add_package(&format!("pkg{i}"), parent)));
    }
    let mut ids = Vec::new();
    for i in 0..1 + rng.below(12) {
        let kind = ElementKind::ALL[rng.below(ElementKind::ALL.len())];
        let pkg = packages[rng.below(packages.len())];
        let pos = rng.chance(70).then(|| Pos::new(rng.below(100) as i32 - 20, rng.below(100) as i32));
        let id = ed.add_element(kind, &format!("E{i}"), pkg, pos);
        let mut el = ed.doc.elements[&id].clone();
        for _ in 0..rng.below(5) {
            el.members.push(match kind {
                ElementKind::Enum => Member::plain(format!("LIT{}", rng.below(100))),
                ElementKind::Note => Member::plain(format!("note text {}", rng.below(100))),
                _ => Member::parse_line(MEMBERS[rng.below(MEMBERS.len())]),
            });
        }
        if rng.chance(20) && kind != ElementKind::Note {
            el.stereotype = Some("entity".into());
        }
        if rng.chance(30) {
            el.comments.push(format!("comment for {i}"));
        }
        ed.set_element(id, el);
        ids.push(id);
    }
    for _ in 0..rng.below(15) {
        let a = ids[rng.below(ids.len())];
        let b = ids[rng.below(ids.len())];
        let kind = RelationKind::ALL[rng.below(RelationKind::ALL.len())];
        let r = ed.connect(a, b, kind);
        let mut rel = ed.doc.relations[&r].clone();
        if rng.chance(40) {
            rel.from.label = Some("1".into());
        }
        if rng.chance(40) {
            rel.to.label = Some("0..* items".into());
        }
        if rng.chance(30) {
            rel.name = Some("uses".into());
        }
        ed.set_relation(r, rel);
    }
    ed.doc
}

fn assert_stable(src: &str) {
    let first = print(&parse(src).doc);
    let second = print(&parse(&first).doc);
    assert_eq!(first, second, "printing is not stable for:\n{src}");
}

#[test]
fn print_parse_print_is_stable_for_design_example() {
    assert_stable(DESIGN_EXAMPLE);
}

#[test]
fn print_parse_print_is_stable_for_random_documents() {
    for seed in 0..300 {
        let doc = random_document(seed);
        let text = print(&doc);
        let parsed = parse(&text);
        assert_eq!(parsed.problems, vec![], "seed {seed}:\n{text}");
        assert_eq!(print(&parsed.doc), text, "seed {seed}");
        assert_eq!(parsed.doc.elements.len(), doc.elements.len(), "seed {seed}");
        assert_eq!(parsed.doc.relations.len(), doc.relations.len(), "seed {seed}");
        let mut names: Vec<_> = doc.elements.keys().map(|id| doc.full_name(*id)).collect();
        let mut parsed_names: Vec<_> = parsed.doc.elements.keys().map(|id| parsed.doc.full_name(*id)).collect();
        names.sort();
        parsed_names.sort();
        assert_eq!(names, parsed_names, "seed {seed}");
    }
}

#[test]
fn canonical_output_of_design_example() {
    let out = print(&parse(DESIGN_EXAMPLE).doc);
    assert!(out.starts_with("diagram \"Shop\"\n\npackage billing {\n  abstract class Payment {\n"));
    assert!(out.contains("enum Status { New, Paid, Shipped }\n"));
    assert!(out.contains("# an order owns its lines\nOrder \"1\" *-- \"1..*\" OrderLine : contains\nbilling.CardPayment --|> billing.Payment\n"));
    assert!(out.ends_with("layout {\n  Order               at 24, 8\n  OrderLine           at 24, 22\n  billing.CardPayment at 48, 22\n  billing.Payment     at 48, 8\n}\n"));
}

#[test]
fn raw_lines_survive() {
    let src = "class A\n\n# about the junk\n!!! junk line\nclass B\n";
    let out = print(&parse(src).doc);
    assert!(out.contains("# about the junk\n!!! junk line\n"), "{out}");
    assert_stable(src);
}

#[test]
fn self_relation_round_trip() {
    assert_stable("class Node\nNode \"1\" --> \"0..1 next\" Node : links\n");
}

#[test]
fn reversed_and_spaced_input_is_normalised() {
    let out = print(&parse("class A\nclass B\n  A   <|--   B\n").doc);
    assert!(out.contains("B --|> A\n"), "{out}");
}
