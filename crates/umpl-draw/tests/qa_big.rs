//! QA findings of 2026-11-20 (big and extreme inputs, docs/qa/2026-11-20-big-session.md)
//! about how diagrams are laid out and drawn.

use umpl_core::Editor;
use umpl_draw::{build_scene, Diagram, SceneOptions, Shape, Theme};

/// QA-33: a saved diagram reopens with the lines it was saved with. Only
/// positions are stored, so reopening must route them as opening did.
/// On this 40-class file opening gives 88 crossings, reopening 81; on
/// tests/big/class-many.umpl 3815 and 4301.
#[test]
#[ignore = "QA-33: reopening routes the lines differently from opening"]
fn a_saved_diagram_reopens_with_the_same_lines() {
    let doc = umpl_text::parse(include_str!("../../../tests/big/class-crossings.umpl")).doc;
    let opened = Diagram::arrange(&doc);
    let mut ed = Editor::new(doc);
    ed.set_positions("Save", &opened.positions());
    let saved = umpl_text::print(&ed.doc);
    let reopened = Diagram::arrange(&umpl_text::parse(&saved).doc);
    assert_eq!(reopened.layout.nodes, opened.layout.nodes, "positions");
    let differ = opened
        .layout
        .routes
        .iter()
        .zip(&reopened.layout.routes)
        .filter(|(a, b)| a != b)
        .count();
    assert_eq!(
        differ,
        0,
        "{differ} of {} lines routed differently; crossings {} then {}",
        opened.layout.routes.len(),
        opened.layout.stats.crossings,
        reopened.layout.stats.crossings
    );
}

/// The vertical centre of each text of a sequence diagram's drawing.
fn text_rows(src: &str) -> Vec<(String, f32)> {
    let doc = umpl_text::read_as(umpl_text::Format::PlantUml, src).doc;
    assert!(doc.sequence.is_some(), "a sequence diagram");
    let diagram = Diagram::arrange(&doc);
    let scene = build_scene(&doc, &diagram, &Theme::light(), &SceneOptions::default());
    scene
        .shapes
        .iter()
        .filter_map(|s| match s {
            Shape::Text { pos, text, .. } => Some((text.clone(), pos.1)),
            _ => None,
        })
        .collect()
}

/// QA-29: rows after a large space (`||N||`, N up to PlantUML's int maximum)
/// stay apart. PlantUML keeps them 29 px apart even after `||2147483647||`;
/// here they fall on one line from about 2^28, past f32's whole numbers.
#[test]
#[ignore = "QA-29: rows after a space of 2^28 px or more fall on one line"]
fn rows_after_a_large_space_stay_apart() {
    let src = "@startuml\nparticipant A\nparticipant B\nA -> B : before\n||268435456||\nA -> B : after 0\nB -> A : after 1\nA -> B : after 2\n@enduml\n";
    let rows = text_rows(src);
    let y = |label: &str| {
        rows.iter()
            .find(|(t, _)| t.ends_with(label))
            .unwrap_or_else(|| panic!("no text {label:?} in {rows:?}"))
            .1
    };
    assert!(
        y("after 0") < y("after 1") && y("after 1") < y("after 2"),
        "{rows:?}"
    );
}

/// QA-35: `autonumber` at the top of the 64-bit range does not overflow
/// (a panic in debug builds, a wrap to a negative number in release).
#[test]
#[ignore = "QA-35: autonumber past i64::MAX overflows"]
fn autonumber_at_the_largest_number_does_not_overflow() {
    let src = "@startuml\nparticipant A\nparticipant B\nautonumber 9223372036854775807\nA -> B : a\nA -> B : b\n@enduml\n";
    let rows = text_rows(src);
    assert!(
        rows.iter().all(|(t, _)| !t.starts_with('-')),
        "a negative number: {rows:?}"
    );
}
