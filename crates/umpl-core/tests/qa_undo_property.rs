//! QA: any sequence of editor actions can be undone back to the start and
//! redone to the end, with the document equal at every step.

use umpl_core::*;

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

/// The parts of a document an undo must restore (ids included).
fn snapshot(doc: &Document) -> String {
    let mut els: Vec<_> = doc
        .elements
        .iter()
        .map(|(k, v)| format!("{k:?}={v:?}"))
        .collect();
    els.sort();
    let mut rels: Vec<_> = doc
        .relations
        .iter()
        .map(|(k, v)| format!("{k:?}={v:?}"))
        .collect();
    rels.sort();
    let mut pkgs: Vec<_> = doc
        .packages
        .iter()
        .map(|(k, v)| format!("{k:?}={v:?}"))
        .collect();
    pkgs.sort();
    format!(
        "{:?}|{:?}|{:?}|{els:?}|{rels:?}|{pkgs:?}",
        doc.title, doc.left_to_right, doc.root
    )
}

fn random_action(ed: &mut Editor, rng: &mut Rng, merge: bool) {
    let els: Vec<ElementId> = ed.doc.elements.keys().copied().collect();
    let pkgs: Vec<PackageId> = ed.doc.packages.keys().copied().collect();
    let rels: Vec<RelationId> = ed.doc.relations.keys().copied().collect();
    let pick_pkg = |rng: &mut Rng| {
        if pkgs.is_empty() || rng.below(2) == 0 {
            None
        } else {
            Some(pkgs[rng.below(pkgs.len())])
        }
    };
    match rng.below(13) {
        0 | 1 => {
            let p = pick_pkg(rng);
            ed.add_element(
                ElementKind::Class,
                "C",
                p,
                Some(Pos::new(rng.below(50) as i32, 0)),
            );
        }
        2 => {
            let p = pick_pkg(rng);
            ed.add_package("P", p);
        }
        3 if els.len() >= 2 => {
            ed.connect(
                els[rng.below(els.len())],
                els[rng.below(els.len())],
                RelationKind::Directed,
            );
        }
        4 if !els.is_empty() => ed.delete_elements(&[els[rng.below(els.len())]]),
        5 if !pkgs.is_empty() => ed.delete(&[], &[pkgs[rng.below(pkgs.len())]]),
        6 if !rels.is_empty() => ed.delete_relations(&[rels[rng.below(rels.len())]]),
        7 if !els.is_empty() => {
            let _ = ed.rename(els[rng.below(els.len())], &format!("N{}", rng.below(5)));
        }
        8 if !els.is_empty() => {
            let id = els[rng.below(els.len())];
            let p = Pos::new(rng.below(80) as i32, rng.below(80) as i32);
            // A drag: several moves merged into one step.
            ed.move_elements(&[(id, p)], merge.then_some("drag"));
        }
        9 => ed.seal(),
        10 if !els.is_empty() => {
            let id = els[rng.below(els.len())];
            ed.resize_element(
                id,
                Pos::new(1, 1),
                Some((20, 10)),
                merge.then_some("resize"),
            );
        }
        11 => ed.set_left_to_right(rng.below(2) == 0),
        12 if !els.is_empty() || !pkgs.is_empty() => {
            ed.set_color(&els, &pkgs, Some("red".into()));
        }
        _ => ed.set_title(Some(format!("T{}", rng.below(3)))),
    }
}

#[test]
fn undo_all_then_redo_all_restores_every_state() {
    for seed in 0..400u64 {
        let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
        let mut ed = Editor::new(Document::new());
        let merge = seed % 2 == 0;
        let start = snapshot(&ed.doc);
        for _ in 0..40 {
            random_action(&mut ed, &mut rng, merge);
        }
        ed.seal();
        let end = snapshot(&ed.doc);
        let mut steps = 0;
        while ed.undo() {
            steps += 1;
        }
        assert_eq!(
            snapshot(&ed.doc),
            start,
            "seed {seed}: undo all did not restore the start"
        );
        for _ in 0..steps {
            assert!(ed.redo());
        }
        assert_eq!(
            snapshot(&ed.doc),
            end,
            "seed {seed}: redo all did not restore the end"
        );
    }
}

/// A drag that is never sealed (the pointer is released outside the window,
/// say) merges into the next drag of the same kind even after other steps
/// were undone in between.
#[test]
fn an_unsealed_drag_does_not_swallow_a_later_drag_after_undo() {
    let mut ed = Editor::new(Document::new());
    let a = ed.add_element(ElementKind::Class, "A", None, Some(Pos::new(0, 0)));
    ed.move_elements(&[(a, Pos::new(5, 0))], Some("drag"));
    // Another step, then undone: the drag is the last step again.
    ed.set_title(Some("T".into()));
    ed.undo();
    ed.move_elements(&[(a, Pos::new(9, 0))], Some("drag"));
    // Undo once should take A back to 5, 0 (the second drag only).
    ed.undo();
    assert_eq!(ed.doc.elements[&a].pos, Some(Pos::new(5, 0)));
}

/// QA-24: a state id names one document. Whenever the editor reports a
/// state it reported before, the document is the same as it was then, so
/// "unsaved" can be told by comparing states.
#[test]
fn one_state_is_one_document() {
    use std::collections::HashMap;
    for seed in 0..400u64 {
        let mut rng = Rng(seed.wrapping_mul(0x2545_F491_4F6C_DD1D) | 1);
        let mut ed = Editor::new(Document::new());
        let merge = seed % 2 == 0;
        let mut seen: HashMap<u64, String> = HashMap::new();
        seen.insert(ed.state(), snapshot(&ed.doc));
        let mut returns = 0;
        for _ in 0..80 {
            match rng.below(6) {
                0 | 1 => {
                    ed.undo();
                }
                2 => {
                    ed.redo();
                }
                3 if rng.below(4) == 0 => ed.seal(),
                _ => random_action(&mut ed, &mut rng, merge),
            }
            let now = snapshot(&ed.doc);
            match seen.get(&ed.state()) {
                Some(before) => {
                    returns += 1;
                    assert_eq!(*before, now, "seed {seed}: state {} changed", ed.state());
                }
                None => {
                    seen.insert(ed.state(), now);
                }
            }
        }
        assert!(returns > 0, "seed {seed}: never came back to a state");
    }
}
