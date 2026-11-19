//! Brings a document to the state of another one, such as the same diagram
//! read again from edited text, while keeping the ids of everything that is
//! still there. Positions, the selection and undo history refer to ids, so a
//! class renamed in the text keeps its place in the diagram.

use std::collections::{HashMap, HashSet, VecDeque};
use std::hash::Hash;

use crate::model::*;
use crate::ops::Op;

/// Elements removed by earlier reconciles, by full name, with their id and
/// position. While text is half typed a class can vanish for a moment (its
/// name is being retyped); when the name comes back, so do its id and place.
pub type Removed = HashMap<String, (ElementId, Option<Pos>)>;

/// The ops that turn `cur` into `target`.
///
/// Items of `target` are matched with items of `cur`: by name (or, for
/// relations, by ends and kind) first; then the leftovers, if there are as
/// many on each side, in declaration order, which treats them as renamed.
/// Matched items keep their ids; an element keeps its position unless the
/// target has one. Unmatched target elements get back an id and position from
/// `removed`, if their name is there, or a new id and no position.
///
/// `cur` is only changed to reserve ids for new items.
pub fn reconcile(cur: &mut Document, target: &Document, removed: &mut Removed) -> Vec<Op> {
    // ----- packages: by full name, then in order -----------------------------
    let cur_pkgs = cur.packages_in_order();
    let tgt_pkgs = target.packages_in_order();
    let mut pick = Picker::new(cur_pkgs.len(), tgt_pkgs.len());
    pick.by_key(
        &keys(&cur_pkgs, |p| cur.full_package_name(*p)),
        &keys(&tgt_pkgs, |p| target.full_package_name(*p)),
    );
    pick.in_order();
    let mut pkg_map: HashMap<PackageId, PackageId> = HashMap::new();
    for (j, t) in tgt_pkgs.iter().enumerate() {
        let id = match pick.out[j] {
            Some(i) => cur_pkgs[i],
            None => cur.new_package_id(),
        };
        pkg_map.insert(*t, id);
    }

    // ----- elements: by (package, name), by name, in order, then memory ----
    let cur_parent = parents(cur);
    let tgt_parent = parents(target);
    let cur_els = cur.elements_in_order();
    let tgt_els = target.elements_in_order();
    let mut pick = Picker::new(cur_els.len(), tgt_els.len());
    let name_of = |doc: &Document, e: &ElementId| doc.elements[e].name.clone();
    pick.by_key(
        &keys(&cur_els, |e| {
            (
                cur_parent.get(&Decl::Element(*e)).copied().flatten(),
                name_of(cur, e),
            )
        }),
        &keys(&tgt_els, |e| {
            let parent = tgt_parent.get(&Decl::Element(*e)).copied().flatten();
            (parent.map(|p| pkg_map[&p]), name_of(target, e))
        }),
    );
    pick.by_key(
        &keys(&cur_els, |e| name_of(cur, e)),
        &keys(&tgt_els, |e| name_of(target, e)),
    );
    pick.in_order();
    let mut el_map: HashMap<ElementId, ElementId> = HashMap::new();
    let mut el_pos: HashMap<ElementId, Option<Pos>> = HashMap::new();
    let mut taken: HashSet<ElementId> = HashSet::new();
    for (j, t) in tgt_els.iter().enumerate() {
        if let Some(i) = pick.out[j] {
            let id = cur_els[i];
            el_map.insert(*t, id);
            el_pos.insert(*t, cur.elements[&id].pos);
            taken.insert(id);
        }
    }
    for t in &tgt_els {
        if el_map.contains_key(t) {
            continue;
        }
        let back = removed
            .get(&target.full_name(*t))
            .filter(|(id, _)| !cur.elements.contains_key(id) && !taken.contains(id))
            .copied();
        let (id, pos) = match back {
            Some((id, pos)) => {
                removed.remove(&target.full_name(*t));
                (id, pos)
            }
            None => (cur.new_element_id(), None),
        };
        el_map.insert(*t, id);
        el_pos.insert(*t, pos);
        taken.insert(id);
    }

    // ----- relations: by (ends, kind), then in order -----------------------
    let map_end = |t: Endpoint| match t {
        Endpoint::Element(e) => Endpoint::Element(el_map[&e]),
        Endpoint::Package(p) => Endpoint::Package(pkg_map[&p]),
    };
    let cur_rels = cur.relations_in_order();
    let tgt_rels = target.relations_in_order();
    let mut pick = Picker::new(cur_rels.len(), tgt_rels.len());
    pick.by_key(
        &keys(&cur_rels, |r| {
            let r = &cur.relations[r];
            (r.from.target, r.to.target, r.kind)
        }),
        &keys(&tgt_rels, |r| {
            let r = &target.relations[r];
            (map_end(r.from.target), map_end(r.to.target), r.kind)
        }),
    );
    pick.in_order();
    let mut rel_map: HashMap<RelationId, RelationId> = HashMap::new();
    for (j, t) in tgt_rels.iter().enumerate() {
        let id = match pick.out[j] {
            Some(i) => cur_rels[i],
            None => cur.new_relation_id(),
        };
        rel_map.insert(*t, id);
    }

    // ----- ops -------------------------------------------------------------
    let map_decls = |decls: &[Decl]| -> Vec<Decl> {
        decls
            .iter()
            .map(|d| match d {
                Decl::Element(e) => Decl::Element(el_map[e]),
                Decl::Relation(r) => Decl::Relation(rel_map[r]),
                Decl::Package(p) => Decl::Package(pkg_map[p]),
                Decl::Raw(s) => Decl::Raw(s.clone()),
            })
            .collect()
    };
    let mut ops = Vec::new();

    for (t, p) in &target.packages {
        let id = pkg_map[t];
        let new = Package {
            name: p.name.clone(),
            body: map_decls(&p.body),
            color: p.color.clone(),
            comments: p.comments.clone(),
        };
        if cur.packages.get(&id) != Some(&new) {
            ops.push(Op::Package(id, Some(new)));
        }
    }
    for (t, el) in &target.elements {
        let id = el_map[t];
        let mut new = el.clone();
        new.pos = el.pos.or(el_pos[t]);
        // The size, like the position, is not in the text.
        if new.size.is_none() {
            new.size = cur.elements.get(&id).and_then(|e| e.size);
        }
        if cur.elements.get(&id) != Some(&new) {
            ops.push(Op::Element(id, Some(new)));
        }
    }
    for (t, r) in &target.relations {
        let id = rel_map[t];
        let mut new = r.clone();
        new.from.target = map_end(r.from.target);
        new.to.target = map_end(r.to.target);
        if cur.relations.get(&id) != Some(&new) {
            ops.push(Op::Relation(id, Some(new)));
        }
    }

    let kept_pkgs: HashSet<PackageId> = pkg_map.values().copied().collect();
    let kept_rels: HashSet<RelationId> = rel_map.values().copied().collect();
    for id in cur.relations.keys().filter(|r| !kept_rels.contains(r)) {
        ops.push(Op::Relation(*id, None));
    }
    for (id, el) in &cur.elements {
        if !taken.contains(id) {
            removed.insert(cur.full_name(*id), (*id, el.pos));
            ops.push(Op::Element(*id, None));
        }
    }
    for id in cur.packages.keys().filter(|p| !kept_pkgs.contains(p)) {
        ops.push(Op::Package(*id, None));
    }

    let root = map_decls(&target.root);
    if cur.root != root {
        ops.push(Op::Root(root));
    }
    if cur.title != target.title {
        ops.push(Op::Title(target.title.clone()));
    }
    if cur.left_to_right != target.left_to_right {
        ops.push(Op::LeftToRight(target.left_to_right));
    }
    if cur.trailing_comments != target.trailing_comments {
        ops.push(Op::TrailingComments(target.trailing_comments.clone()));
    }
    ops
}

fn keys<T, K>(items: &[T], f: impl Fn(&T) -> K) -> Vec<K> {
    items.iter().map(f).collect()
}

/// The package that directly contains each declaration (`None`: the root).
fn parents(doc: &Document) -> HashMap<Decl, Option<PackageId>> {
    let mut out: HashMap<Decl, Option<PackageId>> =
        doc.root.iter().map(|d| (d.clone(), None)).collect();
    for (id, p) in &doc.packages {
        out.extend(p.body.iter().map(|d| (d.clone(), Some(*id))));
    }
    out
}

/// Pairs target items (by index) with current items, pass by pass.
struct Picker {
    /// For each target item, the current item it is paired with.
    out: Vec<Option<usize>>,
    used: Vec<bool>,
}

impl Picker {
    fn new(cur: usize, tgt: usize) -> Picker {
        Picker {
            out: vec![None; tgt],
            used: vec![false; cur],
        }
    }

    /// Pairs unpaired items with equal keys, first come first served.
    fn by_key<K: Eq + Hash>(&mut self, cur: &[K], tgt: &[K]) {
        let mut free: HashMap<&K, VecDeque<usize>> = HashMap::new();
        for (i, k) in cur.iter().enumerate() {
            if !self.used[i] {
                free.entry(k).or_default().push_back(i);
            }
        }
        for (j, k) in tgt.iter().enumerate() {
            if self.out[j].is_some() {
                continue;
            }
            if let Some(i) = free.get_mut(k).and_then(VecDeque::pop_front) {
                self.out[j] = Some(i);
                self.used[i] = true;
            }
        }
    }

    /// If as many items are unpaired on each side, pairs them in order.
    fn in_order(&mut self) {
        let cur: Vec<usize> = (0..self.used.len()).filter(|i| !self.used[*i]).collect();
        let tgt: Vec<usize> = (0..self.out.len())
            .filter(|j| self.out[*j].is_none())
            .collect();
        if cur.len() == tgt.len() {
            for (i, j) in cur.into_iter().zip(tgt) {
                self.out[j] = Some(i);
                self.used[i] = true;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Editor;

    /// A document built like the parser does: fresh ids in file order.
    fn doc(classes: &[&str], rels: &[(&str, &str, RelationKind)]) -> Document {
        let mut d = Document::new();
        for name in classes {
            let id = d.new_element_id();
            d.elements
                .insert(id, Element::new(ElementKind::Class, *name));
            d.root.push(Decl::Element(id));
        }
        for (a, b, k) in rels {
            let id = d.new_relation_id();
            let r = Relation::new(*k, d.find(a).unwrap(), d.find(b).unwrap());
            d.relations.insert(id, r);
            d.root.push(Decl::Relation(id));
        }
        d
    }

    fn placed(mut d: Document) -> Document {
        for (i, el) in d.elements.values_mut().enumerate() {
            el.pos = Some(Pos::new(10 * i as i32, 0));
        }
        d
    }

    /// Applies the reconcile as one undo step and checks the result has the
    /// target's content.
    fn sync(ed: &mut Editor, target: &Document, removed: &mut Removed) {
        let ops = reconcile(&mut ed.doc, target, removed);
        ed.transact("Edit text", ops);
        let names = |d: &Document| -> Vec<String> {
            d.elements_in_order()
                .iter()
                .map(|e| d.full_name(*e))
                .collect()
        };
        assert_eq!(names(&ed.doc), names(target));
        assert_eq!(ed.doc.relations.len(), target.relations.len());
        assert_eq!(ed.doc.packages.len(), target.packages.len());
    }

    #[test]
    fn same_content_gives_no_ops() {
        let d = placed(doc(&["A", "B"], &[("A", "B", RelationKind::Association)]));
        let mut cur = d.clone();
        let mut target = d.clone();
        for el in target.elements.values_mut() {
            el.pos = None;
        }
        assert!(reconcile(&mut cur, &target, &mut Removed::new()).is_empty());
    }

    #[test]
    fn renamed_class_keeps_id_and_position() {
        let mut ed = Editor::new(placed(doc(
            &["A", "B"],
            &[("A", "B", RelationKind::Association)],
        )));
        let b = ed.doc.find("B").unwrap();
        let b_pos = ed.doc.elements[&b].pos;
        let rel = *ed.doc.relations.keys().next().unwrap();
        let before = ed.doc.clone();
        sync(
            &mut ed,
            &doc(&["A", "Bee"], &[("A", "Bee", RelationKind::Association)]),
            &mut Removed::new(),
        );
        assert_eq!(ed.doc.find("Bee"), Some(b));
        assert_eq!(ed.doc.elements[&b].pos, b_pos);
        assert_eq!(ed.doc.relations[&rel].to.element(), Some(b));
        ed.undo();
        assert_eq!(ed.doc, before);
    }

    #[test]
    fn added_class_is_new_and_unplaced() {
        let mut ed = Editor::new(placed(doc(&["A"], &[])));
        let a = ed.doc.find("A").unwrap();
        sync(&mut ed, &doc(&["A", "B"], &[]), &mut Removed::new());
        assert_eq!(ed.doc.find("A"), Some(a));
        let b = ed.doc.find("B").unwrap();
        assert_ne!(a, b);
        assert_eq!(ed.doc.elements[&b].pos, None);
    }

    #[test]
    fn class_that_comes_back_gets_its_place_back() {
        let mut ed = Editor::new(placed(doc(&["A", "B", "C"], &[])));
        let b = ed.doc.find("B").unwrap();
        let b_pos = ed.doc.elements[&b].pos;
        let mut removed = Removed::new();
        sync(&mut ed, &doc(&["A", "C"], &[]), &mut removed);
        assert!(ed.doc.find("B").is_none());
        sync(&mut ed, &doc(&["A", "B", "C"], &[]), &mut removed);
        assert_eq!(ed.doc.find("B"), Some(b));
        assert_eq!(ed.doc.elements[&b].pos, b_pos);
        assert!(removed.is_empty());
    }

    #[test]
    fn positions_in_target_win() {
        let mut ed = Editor::new(placed(doc(&["A"], &[])));
        let mut target = doc(&["A"], &[]);
        target.elements.values_mut().next().unwrap().pos = Some(Pos::new(7, 8));
        sync(&mut ed, &target, &mut Removed::new());
        let a = ed.doc.find("A").unwrap();
        assert_eq!(ed.doc.elements[&a].pos, Some(Pos::new(7, 8)));
    }

    #[test]
    fn renamed_package_keeps_ids_of_its_contents() {
        let mut ed = Editor::new(Document::new());
        let p = ed.add_package("billing", None);
        let a = ed.add_element(ElementKind::Class, "Pay", Some(p), Some(Pos::new(3, 3)));
        let b = ed.add_element(ElementKind::Class, "Order", None, Some(Pos::new(9, 3)));
        ed.connect(b, a, RelationKind::Directed);

        let mut target = Editor::new(Document::new());
        let q = target.add_package("pay", None);
        let ta = target.add_element(ElementKind::Class, "Pay", Some(q), None);
        let tb = target.add_element(ElementKind::Class, "Order", None, None);
        target.connect(tb, ta, RelationKind::Directed);
        target.set_title(Some("Shop".into()));

        sync(&mut ed, &target.doc, &mut Removed::new());
        assert_eq!(ed.doc.find("pay.Pay"), Some(a));
        assert_eq!(ed.doc.packages[&p].name, "pay");
        assert_eq!(ed.doc.elements[&a].pos, Some(Pos::new(3, 3)));
        assert_eq!(ed.doc.title.as_deref(), Some("Shop"));
    }

    #[test]
    fn relation_changes_keep_other_relations() {
        let mut ed = Editor::new(placed(doc(
            &["A", "B", "C"],
            &[
                ("A", "B", RelationKind::Association),
                ("B", "C", RelationKind::Composition),
            ],
        )));
        let bc = ed.doc.relations_in_order()[1];
        sync(
            &mut ed,
            &doc(&["A", "B", "C"], &[("B", "C", RelationKind::Composition)]),
            &mut Removed::new(),
        );
        assert_eq!(ed.doc.relations.keys().copied().collect::<Vec<_>>(), [bc]);
    }

    #[test]
    fn moved_between_packages_keeps_id() {
        let mut ed = Editor::new(Document::new());
        let a = ed.add_element(ElementKind::Class, "A", None, Some(Pos::new(1, 1)));
        let mut target = Editor::new(Document::new());
        let p = target.add_package("pkg", None);
        target.add_element(ElementKind::Class, "A", Some(p), None);
        target.add_element(ElementKind::Class, "B", None, None);
        sync(&mut ed, &target.doc, &mut Removed::new());
        assert_eq!(ed.doc.find("pkg.A"), Some(a));
    }

    #[test]
    fn trailing_comments_and_raw_lines_follow() {
        let mut ed = Editor::new(doc(&["A"], &[]));
        let mut target = doc(&["A"], &[]);
        target.root.push(Decl::Raw("???".into()));
        target.trailing_comments = vec!["end".into()];
        sync(&mut ed, &target, &mut Removed::new());
        assert_eq!(ed.doc.trailing_comments, ["end"]);
        assert_eq!(ed.doc.root[1], Decl::Raw("???".into()));
        ed.undo();
        assert!(ed.doc.trailing_comments.is_empty());
    }
}
