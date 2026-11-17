//! PlantUML, Mermaid and Umple: export, import, and the round trip between them.

mod common;

use common::random_document;
use umpl_core::*;
use umpl_text::{mermaid, parse, plantuml, print, umple};

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

/// What an Umple export must keep: every class, interface and enum with
/// its kind, and every generalization and realization between them.
fn umple_shape(doc: &Document) -> (Vec<String>, usize) {
    let mut kinds: Vec<String> = doc
        .elements
        .values()
        .filter(|e| e.kind != ElementKind::Note)
        .map(|e| format!("{:?}", e.kind))
        .collect();
    kinds.sort();
    let is_type = |id: &ElementId| {
        matches!(
            doc.elements[id].kind,
            ElementKind::Class | ElementKind::Abstract | ElementKind::Interface
        )
    };
    let isa = doc
        .relations
        .values()
        .filter(|r| {
            matches!(
                r.kind,
                RelationKind::Generalization | RelationKind::Realization
            ) && is_type(&r.from.element)
                && is_type(&r.to.element)
        })
        .count();
    (kinds, isa)
}

fn assert_umple_export_reads_back(name: &str, doc: &Document) {
    let ump = umple::export(doc);
    let back = umple::import(&ump);
    assert_eq!(back.problems, vec![], "{name}:\n{ump}");
    assert!(!back.has_layout);
    assert_eq!(umple_shape(&back.doc), umple_shape(doc), "{name}:\n{ump}");
    // Every class member is kept, as an Umple member or as a comment.
    let members = |d: &Document| -> usize {
        d.elements
            .values()
            .filter(|e| e.kind != ElementKind::Note)
            .map(|e| e.members.len())
            .sum()
    };
    let commented = ump
        .lines()
        .filter(|l| {
            l.starts_with("  // ") && !l.contains("depends on") && !l.contains("aggregation")
        })
        .count();
    assert!(
        members(&back.doc) + commented >= members(doc),
        "{name}:\n{ump}"
    );
}

#[test]
fn examples_survive_umple_export() {
    for (name, src) in EXAMPLES {
        assert_umple_export_reads_back(name, &parse(src).doc);
    }
}

#[test]
fn random_documents_survive_umple_export() {
    for seed in 0..300 {
        assert_umple_export_reads_back(&format!("seed {seed}"), &random_document(seed));
    }
}
