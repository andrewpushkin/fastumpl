//! QA findings of 2026-11-20 (big and extreme inputs, docs/qa/2026-11-20-big-session.md)
//! that the PlantUML reader can show without drawing.

use umpl_core::sequence::{Head, Step};
use umpl_text::{read_as, write_as, Format};

fn is_sequence(src: &str) -> bool {
    read_as(Format::PlantUml, src).doc.sequence.is_some()
}

/// QA-27: a fragment label that starts with a dash is still a fragment.
/// PlantUML draws this file as a sequence diagram with a loop frame.
#[test]
#[ignore = "QA-27: `loop - every second -` makes the file a class diagram"]
fn a_fragment_label_starting_with_a_dash_keeps_a_sequence() {
    let src = "@startuml\nparticipant A\nparticipant B\nloop - every second -\n  A -> B : tick\nend\n@enduml\n";
    assert!(is_sequence(src));
}

/// QA-28: every message arrow PlantUML accepts in a sequence diagram keeps
/// the file a sequence diagram. Each of these is drawn by PlantUML 1.2026.9
/// with lifelines.
#[test]
#[ignore = "QA-28: half arrows and a circle at the start make the file a class diagram"]
fn every_sequence_arrow_keeps_a_sequence() {
    let failing: Vec<&str> = ["-\\", "-\\\\", "-/", "-//", "//--", "o->", "o->o"]
        .into_iter()
        .filter(|arrow| {
            let src =
                format!("@startuml\nparticipant A\nparticipant B\nA {arrow} B : m\n@enduml\n");
            !is_sequence(&src)
        })
        .collect();
    assert!(failing.is_empty(), "read as a class diagram: {failing:?}");
}

/// QA-32: the circle at a message's end (`->o`, `<->o`) and the cross at its
/// start (`x->`) survive reading and writing the PlantUML text, as Save does.
#[test]
#[ignore = "QA-32: `->o`, `<->o` and `x->` lose their circle or cross"]
fn message_decorations_survive_save() {
    let src = "@startuml\nparticipant A\nparticipant B\nA ->o B : circle\nA <->o B : both\nA x-> B : cross\n@enduml\n";
    let doc = read_as(Format::PlantUml, src).doc;
    let seq = doc.sequence.as_ref().expect("a sequence diagram");
    let heads: Vec<Head> = seq
        .steps
        .iter()
        .filter_map(|s| match s {
            Step::Message(m) => Some(m.head),
            _ => None,
        })
        .collect();
    assert_eq!(heads[..2], [Head::Circle, Head::Circle], "parsed heads");
    let out = write_as(Format::PlantUml, &doc);
    for arrow in ["A ->o B", "A <->o B", "A x-> B"] {
        assert!(out.contains(arrow), "{arrow} missing from:\n{out}");
    }
}
