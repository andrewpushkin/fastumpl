//! Shared test helpers: a seeded generator of random documents.

use umpl_core::*;

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

pub fn random_document(seed: u64) -> Document {
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
        let pos = rng
            .chance(70)
            .then(|| Pos::new(rng.below(100) as i32 - 20, rng.below(100) as i32));
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
