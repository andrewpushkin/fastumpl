//! PlantUML and Mermaid: export, import, and the round trip between them.

mod common;

use common::random_document;
use umpl_core::*;
use umpl_text::{mermaid, parse, plantuml, print};

const EXAMPLES: [(&str, &str); 4] = [
    ("banking", include_str!("../../../examples/banking.umpl")),
    ("library", include_str!("../../../examples/library.umpl")),
    ("shapes", include_str!("../../../examples/shapes.umpl")),
    ("shop", include_str!("../../../examples/shop.umpl")),
];

/// The document as `.umpl` text, without positions: what must survive a
/// trip through a format that cannot store positions.
fn model_text(doc: &Document) -> String {
    let mut doc = doc.clone();
    for el in doc.elements.values_mut() {
        el.pos = None;
    }
    print(&doc)
}

fn assert_plantuml_round_trip(name: &str, doc: &Document) {
    // Normalise through `.umpl` first, as the editor would.
    let doc = parse(&print(doc)).doc;
    let puml = plantuml::export(&doc);
    let back = plantuml::import(&puml);
    assert_eq!(back.problems, vec![], "{name}:\n{puml}");
    assert!(!back.has_layout);
    assert_eq!(model_text(&back.doc), model_text(&doc), "{name}:\n{puml}");
}

#[test]
fn examples_survive_plantuml_round_trip() {
    for (name, src) in EXAMPLES {
        let parsed = parse(src);
        assert_eq!(parsed.problems, vec![], "{name}");
        assert_plantuml_round_trip(name, &parsed.doc);
    }
}

#[test]
fn random_documents_survive_plantuml_round_trip() {
    for seed in 0..300 {
        assert_plantuml_round_trip(&format!("seed {seed}"), &random_document(seed));
    }
}

#[test]
fn mermaid_export_names_every_class_and_relation() {
    for (name, src) in EXAMPLES {
        let doc = parse(src).doc;
        let out = mermaid::export(&doc);
        assert!(out.contains("classDiagram\n"), "{name}");
        for el in doc.elements.values() {
            if el.kind != ElementKind::Note {
                assert!(out.contains(&el.name), "{name}: {} missing", el.name);
            }
        }
        let relation_lines = out
            .lines()
            .filter(|l| {
                RelationKind::ALL
                    .iter()
                    .any(|k| l.contains(&format!(" {} ", k.arrow())))
            })
            .count();
        let expected = doc
            .relations
            .values()
            .filter(|r| {
                ![r.from.element, r.to.element]
                    .iter()
                    .any(|e| doc.elements[e].kind == ElementKind::Note)
            })
            .count();
        assert_eq!(relation_lines, expected, "{name}:\n{out}");
        // Braces balance, so Mermaid sees every body and namespace closed.
        assert_eq!(out.matches('{').count(), out.matches('}').count(), "{name}");
    }
}

#[test]
fn mermaid_export_of_random_documents_is_balanced() {
    for seed in 0..300 {
        let doc = random_document(seed);
        let out = mermaid::export(&doc);
        assert_eq!(
            out.matches('{').count(),
            out.matches('}').count(),
            "seed {seed}:\n{out}"
        );
        // Every line inside the diagram is indented, a comment, or a
        // known statement.
        for line in out.lines().skip_while(|l| *l != "classDiagram").skip(1) {
            assert!(line.starts_with("    "), "seed {seed}: {line:?}");
        }
    }
}
