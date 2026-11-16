use std::collections::HashSet;

use crate::model::*;
use crate::ops::{Op, Slot};

/// A group of inverse ops that undo one user action.
#[derive(Debug)]
struct Group {
    /// Inverses in the order they were produced; undo applies them reversed.
    inverse: Vec<Op>,
    /// Actions with the same key merge into one group until [`Editor::seal`].
    key: Option<String>,
}

/// A document plus its undo history. All edits go through here.
#[derive(Debug)]
pub struct Editor {
    pub doc: Document,
    undo: Vec<Group>,
    redo: Vec<Group>,
    /// Increases on every change, including undo and redo.
    pub revision: u64,
}

impl Editor {
    pub fn new(doc: Document) -> Editor {
        Editor {
            doc,
            undo: Vec::new(),
            redo: Vec::new(),
            revision: 0,
        }
    }

    /// Applies `ops` as one undo step.
    pub fn transact(&mut self, ops: Vec<Op>) {
        self.push(ops, None);
    }

    /// Applies `ops`, merging with the previous step if it has the same key
    /// and has not been sealed. Used for drags: one drag is one undo step.
    pub fn transact_merge(&mut self, key: &str, ops: Vec<Op>) {
        self.push(ops, Some(key.to_string()));
    }

    /// Ends merging: the next `transact_merge` starts a new step.
    pub fn seal(&mut self) {
        if let Some(g) = self.undo.last_mut() {
            g.key = None;
        }
    }

    fn push(&mut self, ops: Vec<Op>, key: Option<String>) {
        if ops.is_empty() {
            return;
        }
        let inverse: Vec<Op> = ops.into_iter().map(|o| self.doc.apply(o)).collect();
        self.redo.clear();
        self.revision += 1;
        if let (Some(k), Some(last)) = (&key, self.undo.last_mut()) {
            if last.key.as_ref() == Some(k) {
                // Undo restores each slot to its value before the group, so
                // only the first inverse per slot matters.
                let seen: HashSet<Slot> = last.inverse.iter().map(Op::slot).collect();
                last.inverse
                    .extend(inverse.into_iter().filter(|o| !seen.contains(&o.slot())));
                return;
            }
        }
        self.undo.push(Group { inverse, key });
    }

    /// Applies `ops` as part of the most recent undo step, for follow-up
    /// changes such as positions computed for elements that step created.
    pub fn amend(&mut self, ops: Vec<Op>) {
        if ops.is_empty() {
            return;
        }
        let inverse: Vec<Op> = ops.into_iter().map(|o| self.doc.apply(o)).collect();
        self.revision += 1;
        match self.undo.last_mut() {
            Some(last) => {
                let seen: HashSet<Slot> = last.inverse.iter().map(Op::slot).collect();
                last.inverse
                    .extend(inverse.into_iter().filter(|o| !seen.contains(&o.slot())));
            }
            None => self.undo.push(Group { inverse, key: None }),
        }
    }

    /// Changes the document without recording an undo step (used when a
    /// file is opened and positions are filled in).
    pub fn apply_silently(&mut self, ops: Vec<Op>) {
        for o in ops {
            self.doc.apply(o);
        }
        self.revision += 1;
    }

    /// Position ops for `positions`, skipping unchanged ones.
    pub fn position_changes(&self, positions: &[(ElementId, Option<Pos>)]) -> Vec<Op> {
        self.position_ops(positions)
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn undo(&mut self) -> bool {
        let Some(g) = self.undo.pop() else {
            return false;
        };
        let redo = g
            .inverse
            .into_iter()
            .rev()
            .map(|o| self.doc.apply(o))
            .collect();
        self.redo.push(Group {
            inverse: redo,
            key: None,
        });
        self.revision += 1;
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(g) = self.redo.pop() else {
            return false;
        };
        let undo = g
            .inverse
            .into_iter()
            .rev()
            .map(|o| self.doc.apply(o))
            .collect();
        self.undo.push(Group {
            inverse: undo,
            key: None,
        });
        self.revision += 1;
        true
    }

    // ----- helpers that build ops -------------------------------------------

    fn container_op(&self, pkg: Option<PackageId>, body: Vec<Decl>) -> Op {
        match pkg {
            Some(p) => {
                let mut package = self.doc.packages[&p].clone();
                package.body = body;
                Op::Package(p, Some(package))
            }
            None => Op::Root(body),
        }
    }

    /// Adds an element at the end of `pkg` (or the root).
    pub fn add_element(
        &mut self,
        kind: ElementKind,
        name: &str,
        pkg: Option<PackageId>,
        pos: Option<Pos>,
    ) -> ElementId {
        let id = self.doc.new_element_id();
        let mut el = Element::new(kind, self.doc.unique_name(name, pkg));
        el.pos = pos;
        let mut body = self.doc.body(pkg).to_vec();
        body.push(Decl::Element(id));
        let ops = vec![Op::Element(id, Some(el)), self.container_op(pkg, body)];
        self.transact(ops);
        id
    }

    /// Adds an empty package inside `parent` (or the root).
    pub fn add_package(&mut self, name: &str, parent: Option<PackageId>) -> PackageId {
        let id = self.doc.new_package_id();
        let pkg = Package::new(self.doc.unique_name(name, parent));
        let mut body = self.doc.body(parent).to_vec();
        body.push(Decl::Package(id));
        let ops = vec![Op::Package(id, Some(pkg)), self.container_op(parent, body)];
        self.transact(ops);
        id
    }

    /// Ops that remove `decls` from whatever lists contain them.
    fn removal_ops(&self, decls: &HashSet<Decl>, removed_pkgs: &HashSet<PackageId>) -> Vec<Op> {
        let mut ops = Vec::new();
        if self.doc.root.iter().any(|d| decls.contains(d)) {
            ops.push(Op::Root(
                self.doc
                    .root
                    .iter()
                    .filter(|d| !decls.contains(d))
                    .cloned()
                    .collect(),
            ));
        }
        for (pid, p) in &self.doc.packages {
            if removed_pkgs.contains(pid) {
                continue;
            }
            if p.body.iter().any(|d| decls.contains(d)) {
                let mut np = p.clone();
                np.body.retain(|d| !decls.contains(d));
                ops.push(Op::Package(*pid, Some(np)));
            }
        }
        ops
    }

    /// Deletes elements, every relation touching them, and `packages`
    /// with all their contents, as one undo step.
    pub fn delete(&mut self, elements: &[ElementId], packages: &[PackageId]) {
        let mut els: HashSet<ElementId> = elements.iter().copied().collect();
        let mut pkgs: HashSet<PackageId> = HashSet::new();
        for p in packages {
            if self.doc.packages.contains_key(p) {
                pkgs.insert(*p);
                els.extend(self.doc.elements_in(*p));
                let mut stack = vec![*p];
                while let Some(q) = stack.pop() {
                    for d in self.doc.body(Some(q)) {
                        if let Decl::Package(r) = d {
                            if pkgs.insert(*r) {
                                stack.push(*r);
                            }
                        }
                    }
                }
            }
        }
        let rels: Vec<RelationId> = self
            .doc
            .relations
            .iter()
            .filter(|(_, r)| els.contains(&r.from.element) || els.contains(&r.to.element))
            .map(|(id, _)| *id)
            .collect();
        let mut decls: HashSet<Decl> = HashSet::new();
        decls.extend(els.iter().map(|e| Decl::Element(*e)));
        decls.extend(rels.iter().map(|r| Decl::Relation(*r)));
        decls.extend(pkgs.iter().map(|p| Decl::Package(*p)));

        let mut ops = self.removal_ops(&decls, &pkgs);
        let mut rels_sorted = rels;
        rels_sorted.sort();
        ops.extend(rels_sorted.into_iter().map(|r| Op::Relation(r, None)));
        let mut els_sorted: Vec<_> = els
            .into_iter()
            .filter(|e| self.doc.elements.contains_key(e))
            .collect();
        els_sorted.sort();
        ops.extend(els_sorted.into_iter().map(|e| Op::Element(e, None)));
        let mut pkgs_sorted: Vec<_> = pkgs.into_iter().collect();
        pkgs_sorted.sort();
        ops.extend(pkgs_sorted.into_iter().map(|p| Op::Package(p, None)));
        self.transact(ops);
    }

    pub fn delete_elements(&mut self, ids: &[ElementId]) {
        self.delete(ids, &[]);
    }

    /// Renames an element. Fails if another element in the same package
    /// already has that name.
    pub fn rename(&mut self, id: ElementId, name: &str) -> Result<(), String> {
        let name = name.trim();
        let Some(el) = self.doc.elements.get(&id) else {
            return Err("no such element".into());
        };
        if el.name == name {
            return Ok(());
        }
        if name.is_empty() {
            return Err("name cannot be empty".into());
        }
        if !valid_name(name) {
            return Err("names may use letters, digits and _ only".into());
        }
        let pkg = self.doc.element_package(id);
        if !self.doc.name_is_free(name, pkg) {
            return Err(format!("'{name}' already exists here"));
        }
        let mut el = el.clone();
        el.name = name.to_string();
        self.transact(vec![Op::Element(id, Some(el))]);
        Ok(())
    }

    pub fn rename_package(&mut self, id: PackageId, name: &str) -> Result<(), String> {
        let name = name.trim();
        let Some(p) = self.doc.packages.get(&id) else {
            return Err("no such package".into());
        };
        if p.name == name {
            return Ok(());
        }
        if !valid_name(name) {
            return Err("names may use letters, digits and _ only".into());
        }
        let parent = self.doc.container_of(&Decl::Package(id));
        if !self.doc.name_is_free(name, parent) {
            return Err(format!("'{name}' already exists here"));
        }
        let mut p = p.clone();
        p.name = name.to_string();
        self.transact(vec![Op::Package(id, Some(p))]);
        Ok(())
    }

    /// Replaces an element's content (members, kind, stereotype...) but keeps
    /// its name check to [`Editor::rename`].
    pub fn set_element(&mut self, id: ElementId, el: Element) {
        if self.doc.elements.get(&id) != Some(&el) {
            self.transact(vec![Op::Element(id, Some(el))]);
        }
    }

    /// Adds a relation at the end of the root list.
    pub fn connect(&mut self, from: ElementId, to: ElementId, kind: RelationKind) -> RelationId {
        let id = self.doc.new_relation_id();
        let mut root = self.doc.root.clone();
        root.push(Decl::Relation(id));
        self.transact(vec![
            Op::Relation(id, Some(Relation::new(kind, from, to))),
            Op::Root(root),
        ]);
        id
    }

    pub fn set_relation(&mut self, id: RelationId, rel: Relation) {
        if self.doc.relations.get(&id) != Some(&rel) {
            self.transact(vec![Op::Relation(id, Some(rel))]);
        }
    }

    pub fn delete_relations(&mut self, ids: &[RelationId]) {
        let decls: HashSet<Decl> = ids.iter().map(|r| Decl::Relation(*r)).collect();
        let mut ops = self.removal_ops(&decls, &HashSet::new());
        ops.extend(ids.iter().map(|r| Op::Relation(*r, None)));
        self.transact(ops);
    }

    fn position_ops(&self, moves: &[(ElementId, Option<Pos>)]) -> Vec<Op> {
        moves
            .iter()
            .filter_map(|(id, pos)| {
                let el = self.doc.elements.get(id)?;
                if el.pos == *pos {
                    return None;
                }
                let mut el = el.clone();
                el.pos = *pos;
                Some(Op::Element(*id, Some(el)))
            })
            .collect()
    }

    /// Moves elements. With a `merge_key`, repeated calls merge into one
    /// undo step until [`Editor::seal`].
    pub fn move_elements(&mut self, moves: &[(ElementId, Pos)], merge_key: Option<&str>) {
        let moves: Vec<_> = moves.iter().map(|(id, p)| (*id, Some(*p))).collect();
        let ops = self.position_ops(&moves);
        match merge_key {
            Some(k) => self.transact_merge(k, ops),
            None => self.transact(ops),
        }
    }

    /// Sets many positions as one undo step (Tidy).
    pub fn set_positions(&mut self, positions: &[(ElementId, Option<Pos>)]) {
        let ops = self.position_ops(positions);
        self.transact(ops);
    }

    pub fn set_title(&mut self, title: Option<String>) {
        if self.doc.title != title {
            self.transact(vec![Op::Title(title)]);
        }
    }
}

/// Names are identifiers: letters, digits and `_`, not starting with a digit.
pub fn valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_alphanumeric() || c == '_')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn editor_with_two() -> (Editor, ElementId, ElementId) {
        let mut ed = Editor::new(Document::new());
        let a = ed.add_element(ElementKind::Class, "A", None, None);
        let b = ed.add_element(ElementKind::Class, "B", None, None);
        (ed, a, b)
    }

    #[test]
    fn undo_redo_add_element() {
        let mut ed = Editor::new(Document::new());
        let a = ed.add_element(ElementKind::Class, "A", None, Some(Pos::new(1, 2)));
        assert_eq!(ed.doc.root, vec![Decl::Element(a)]);
        assert!(ed.undo());
        assert!(ed.doc.elements.is_empty());
        assert!(ed.doc.root.is_empty());
        assert!(ed.redo());
        assert_eq!(ed.doc.elements[&a].name, "A");
        assert_eq!(ed.doc.root, vec![Decl::Element(a)]);
        assert!(!ed.can_redo());
    }

    #[test]
    fn add_element_picks_unique_name() {
        let mut ed = Editor::new(Document::new());
        ed.add_element(ElementKind::Class, "Class", None, None);
        let b = ed.add_element(ElementKind::Class, "Class", None, None);
        assert_eq!(ed.doc.elements[&b].name, "Class2");
    }

    #[test]
    fn delete_element_removes_its_relations_and_undo_restores_them() {
        let (mut ed, a, b) = editor_with_two();
        let r = ed.connect(a, b, RelationKind::Association);
        let snapshot = ed.doc.clone();
        ed.delete_elements(&[a]);
        assert!(!ed.doc.elements.contains_key(&a));
        assert!(!ed.doc.relations.contains_key(&r));
        assert_eq!(ed.doc.root, vec![Decl::Element(b)]);
        ed.undo();
        assert_eq!(ed.doc, snapshot);
    }

    #[test]
    fn delete_package_removes_contents() {
        let mut ed = Editor::new(Document::new());
        let p = ed.add_package("billing", None);
        let a = ed.add_element(ElementKind::Class, "A", Some(p), None);
        let b = ed.add_element(ElementKind::Class, "B", None, None);
        ed.connect(b, a, RelationKind::Dependency);
        ed.delete(&[], &[p]);
        assert!(ed.doc.packages.is_empty());
        assert_eq!(ed.doc.elements.len(), 1);
        assert!(ed.doc.relations.is_empty());
        assert_eq!(ed.doc.root, vec![Decl::Element(b)]);
    }

    #[test]
    fn transact_merge_coalesces_drag() {
        let (mut ed, a, _) = editor_with_two();
        let before = ed.doc.clone();
        for x in 1..10 {
            ed.move_elements(&[(a, Pos::new(x, 0))], Some("drag"));
        }
        ed.seal();
        assert_eq!(ed.doc.elements[&a].pos, Some(Pos::new(9, 0)));
        ed.undo();
        assert_eq!(ed.doc, before);
        // The step before the drag is still there.
        assert!(ed.can_undo());
    }

    #[test]
    fn amend_joins_last_step() {
        let mut ed = Editor::new(Document::new());
        let a = ed.add_element(ElementKind::Class, "A", None, None);
        let ops = ed.position_changes(&[(a, Some(Pos::new(3, 4)))]);
        ed.amend(ops);
        assert_eq!(ed.doc.elements[&a].pos, Some(Pos::new(3, 4)));
        ed.undo();
        assert!(ed.doc.elements.is_empty());
        assert!(!ed.can_undo());
    }

    #[test]
    fn sealed_drags_are_separate_steps() {
        let (mut ed, a, _) = editor_with_two();
        ed.move_elements(&[(a, Pos::new(1, 0))], Some("drag"));
        ed.seal();
        ed.move_elements(&[(a, Pos::new(2, 0))], Some("drag"));
        ed.seal();
        ed.undo();
        assert_eq!(ed.doc.elements[&a].pos, Some(Pos::new(1, 0)));
    }

    #[test]
    fn rename_rejects_duplicates_and_bad_names() {
        let (mut ed, a, _) = editor_with_two();
        assert!(ed.rename(a, "B").is_err());
        assert!(ed.rename(a, "1x").is_err());
        assert!(ed.rename(a, "Order").is_ok());
        assert_eq!(ed.doc.elements[&a].name, "Order");
    }

    #[test]
    fn full_name_includes_packages() {
        let mut ed = Editor::new(Document::new());
        let p = ed.add_package("shop", None);
        let q = ed.add_package("billing", Some(p));
        let a = ed.add_element(ElementKind::Class, "Payment", Some(q), None);
        assert_eq!(ed.doc.full_name(a), "shop.billing.Payment");
        assert_eq!(ed.doc.find("shop.billing.Payment"), Some(a));
        assert_eq!(ed.doc.elements_in(p), vec![a]);
    }

    #[test]
    fn member_line_round_trip() {
        for line in [
            "+total(): Money",
            "+pay(): Receipt {abstract}",
            "#count: int {static}",
            "Paid",
        ] {
            assert_eq!(Member::parse_line(line).to_line(), line);
        }
        let m = Member::parse_line("-{static} count: int");
        assert_eq!(m.vis, Some(Visibility::Private));
        assert!(m.is_static);
        assert_eq!(m.text, "count: int");
        assert_eq!(m.to_line(), "-count: int {static}");
        assert!(Member::parse_line("+total(): Money").is_operation());
    }
}
