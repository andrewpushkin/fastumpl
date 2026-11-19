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
    /// What the action did, for menus: "Move Order", "Delete 3 items".
    label: String,
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

    /// Applies `ops` as one undo step named `label`.
    pub fn transact(&mut self, label: &str, ops: Vec<Op>) {
        self.push(ops, None, label);
    }

    /// Applies `ops`, merging with the previous step if it has the same key
    /// and has not been sealed. Used for drags: one drag is one undo step,
    /// keeping the label of its first part.
    pub fn transact_merge(&mut self, key: &str, label: &str, ops: Vec<Op>) {
        self.push(ops, Some(key.to_string()), label);
    }

    /// Ends merging: the next `transact_merge` starts a new step.
    pub fn seal(&mut self) {
        if let Some(g) = self.undo.last_mut() {
            g.key = None;
        }
    }

    fn push(&mut self, ops: Vec<Op>, key: Option<String>, label: &str) {
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
        self.undo.push(Group {
            inverse,
            key,
            label: label.to_string(),
        });
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
            None => self.undo.push(Group {
                inverse,
                key: None,
                label: "Edit".into(),
            }),
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

    /// What [`Editor::undo`] would undo, such as "Move Order".
    pub fn undo_label(&self) -> Option<&str> {
        self.undo.last().map(|g| g.label.as_str())
    }

    /// What [`Editor::redo`] would redo.
    pub fn redo_label(&self) -> Option<&str> {
        self.redo.last().map(|g| g.label.as_str())
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
            label: g.label,
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
            label: g.label,
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
        self.transact(&format!("Add {}", kind.label().to_lowercase()), ops);
        id
    }

    /// Adds an empty package inside `parent` (or the root).
    pub fn add_package(&mut self, name: &str, parent: Option<PackageId>) -> PackageId {
        let id = self.doc.new_package_id();
        let pkg = Package::new(self.doc.unique_name(name, parent));
        let mut body = self.doc.body(parent).to_vec();
        body.push(Decl::Package(id));
        let ops = vec![Op::Package(id, Some(pkg)), self.container_op(parent, body)];
        self.transact("Add package", ops);
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
        self.delete_items(elements, packages, &[]);
    }

    /// Deletes elements, packages (with their contents) and relations, and
    /// every relation touching a deleted element, as one undo step.
    pub fn delete_items(
        &mut self,
        elements: &[ElementId],
        packages: &[PackageId],
        relations: &[RelationId],
    ) {
        let label = self.delete_label(elements, packages, relations);
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
            .filter(|(id, r)| {
                let gone = |t: Endpoint| match t {
                    Endpoint::Element(e) => els.contains(&e),
                    Endpoint::Package(p) => pkgs.contains(&p),
                };
                relations.contains(id) || gone(r.from.target) || gone(r.to.target)
            })
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
        self.transact(&label, ops);
    }

    /// "Delete Order", "Delete package billing", "Delete association", or
    /// "Delete 3 items". Relations that go with a deleted element are not
    /// counted.
    fn delete_label(
        &self,
        elements: &[ElementId],
        packages: &[PackageId],
        relations: &[RelationId],
    ) -> String {
        let els: Vec<&Element> = elements
            .iter()
            .filter_map(|e| self.doc.elements.get(e))
            .collect();
        let pkgs: Vec<&Package> = packages
            .iter()
            .filter_map(|p| self.doc.packages.get(p))
            .collect();
        let rels: Vec<&Relation> = relations
            .iter()
            .filter_map(|r| self.doc.relations.get(r))
            .filter(|r| {
                let with = |t: Endpoint| match t {
                    Endpoint::Element(e) => elements.contains(&e),
                    Endpoint::Package(p) => packages.contains(&p),
                };
                !with(r.from.target) && !with(r.to.target)
            })
            .collect();
        match (&els[..], &pkgs[..], &rels[..]) {
            ([e], [], []) => format!("Delete {}", e.name),
            ([], [p], []) => format!("Delete package {}", p.name),
            ([], [], [r]) => format!("Delete {}", r.kind.label().to_lowercase()),
            _ => format!("Delete {} items", els.len() + pkgs.len() + rels.len()),
        }
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
            return Err("names cannot hold tabs or other control characters".into());
        }
        let pkg = self.doc.element_package(id);
        if !self.doc.name_is_free(name, pkg) {
            return Err(format!("'{name}' already exists here"));
        }
        let label = format!("Rename {} to {name}", el.name);
        let mut el = el.clone();
        el.name = name.to_string();
        self.transact(&label, vec![Op::Element(id, Some(el))]);
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
            return Err("names cannot be empty or hold control characters".into());
        }
        let parent = self.doc.container_of(&Decl::Package(id));
        if !self.doc.name_is_free(name, parent) {
            return Err(format!("'{name}' already exists here"));
        }
        let label = format!("Rename package {} to {name}", p.name);
        let mut p = p.clone();
        p.name = name.to_string();
        self.transact(&label, vec![Op::Package(id, Some(p))]);
        Ok(())
    }

    /// Replaces an element's content (members, kind, stereotype...) but keeps
    /// its name check to [`Editor::rename`].
    pub fn set_element(&mut self, id: ElementId, el: Element) {
        if self.doc.elements.get(&id) != Some(&el) {
            let label = format!("Edit {}", el.name);
            self.transact(&label, vec![Op::Element(id, Some(el))]);
        }
    }

    /// Adds a relation at the end of the root list.
    pub fn connect(&mut self, from: ElementId, to: ElementId, kind: RelationKind) -> RelationId {
        let id = self.doc.new_relation_id();
        let mut root = self.doc.root.clone();
        root.push(Decl::Relation(id));
        self.transact(
            &format!("Add {}", kind.label().to_lowercase()),
            vec![
                Op::Relation(id, Some(Relation::new(kind, from, to))),
                Op::Root(root),
            ],
        );
        id
    }

    pub fn set_relation(&mut self, id: RelationId, rel: Relation) {
        if self.doc.relations.get(&id) != Some(&rel) {
            let label = format!("Edit {}", rel.kind.label().to_lowercase());
            self.transact(&label, vec![Op::Relation(id, Some(rel))]);
        }
    }

    pub fn delete_relations(&mut self, ids: &[RelationId]) {
        self.delete_items(&[], &[], ids);
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
        let label = match moves {
            [(id, _)] => match self.doc.elements.get(id) {
                Some(el) => format!("Move {}", el.name),
                None => "Move".into(),
            },
            _ => format!("Move {} items", moves.len()),
        };
        let moves: Vec<_> = moves.iter().map(|(id, p)| (*id, Some(*p))).collect();
        let ops = self.position_ops(&moves);
        match merge_key {
            Some(k) => self.transact_merge(k, &label, ops),
            None => self.transact(&label, ops),
        }
    }

    /// Sets many positions as one undo step named `label` (Tidy).
    pub fn set_positions(&mut self, label: &str, positions: &[(ElementId, Option<Pos>)]) {
        let ops = self.position_ops(positions);
        self.transact(label, ops);
    }

    /// Resizes element `id` to at least `size` grid units (`None`: as big
    /// as its text needs), moving it to `pos`. With a `merge_key`, repeated
    /// calls merge into one undo step until [`Editor::seal`] (a drag).
    pub fn resize_element(
        &mut self,
        id: ElementId,
        pos: Pos,
        size: Option<(i32, i32)>,
        merge_key: Option<&str>,
    ) {
        let Some(el) = self.doc.elements.get(&id) else {
            return;
        };
        if el.size == size && el.pos == Some(pos) {
            return;
        }
        let label = format!("Resize {}", el.name);
        let mut new = el.clone();
        new.size = size;
        new.pos = Some(pos);
        let ops = vec![Op::Element(id, Some(new))];
        match merge_key {
            Some(k) => self.transact_merge(k, &label, ops),
            None => self.transact(&label, ops),
        }
    }

    /// Sets the fill colour of elements and packages (`None`: the usual
    /// colour) as one undo step.
    pub fn set_color(
        &mut self,
        elements: &[ElementId],
        packages: &[PackageId],
        color: Option<String>,
    ) {
        let mut ops = Vec::new();
        for id in elements {
            if let Some(el) = self.doc.elements.get(id).filter(|e| e.color != color) {
                let mut new = el.clone();
                new.color = color.clone();
                ops.push(Op::Element(*id, Some(new)));
            }
        }
        for id in packages {
            if let Some(p) = self.doc.packages.get(id).filter(|p| p.color != color) {
                let mut new = p.clone();
                new.color = color.clone();
                ops.push(Op::Package(*id, Some(new)));
            }
        }
        let label = match color {
            Some(_) => "Change colour",
            None => "Reset colour",
        };
        self.transact(label, ops);
    }

    pub fn set_title(&mut self, title: Option<String>) {
        if self.doc.title != title {
            self.transact("Change title", vec![Op::Title(title)]);
        }
    }

    /// Lays the diagram out left to right, or top to bottom.
    pub fn set_left_to_right(&mut self, on: bool) {
        if self.doc.left_to_right != on {
            let label = match on {
                true => "Left to right",
                false => "Top to bottom",
            };
            self.transact(label, vec![Op::LeftToRight(on)]);
        }
    }
}

/// Is `name` usable as an element or package name? Any text but empty or
/// with control characters: names that are not plain identifiers are
/// quoted in a file (see [`crate::name_in_path`]). Line breaks only come
/// from imports.
pub fn valid_name(name: &str) -> bool {
    !name.trim().is_empty() && !name.chars().any(|c| c.is_control() && c != '\n')
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
    fn deleting_a_package_deletes_lines_to_it() {
        let (mut ed, a, _) = editor_with_two();
        let p = ed.add_package("shop", None);
        let mut r = Relation::new(RelationKind::Dependency, a, a);
        r.to = End::at(Endpoint::Package(p));
        let id = ed.doc.new_relation_id();
        ed.doc.relations.insert(id, r);
        ed.doc.root.push(Decl::Relation(id));
        assert_eq!(ed.doc.endpoint_name(Endpoint::Package(p)), "shop");
        assert_eq!(ed.doc.find_endpoint("shop"), Some(Endpoint::Package(p)));
        ed.delete(&[], &[p]);
        assert!(ed.doc.relations.is_empty());
        assert!(!ed.doc.root.contains(&Decl::Relation(id)));
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
    fn steps_are_named_for_undo_and_redo() {
        let mut ed = Editor::new(Document::new());
        assert_eq!((ed.undo_label(), ed.redo_label()), (None, None));
        let a = ed.add_element(ElementKind::Class, "Order", None, Some(Pos::new(0, 0)));
        assert_eq!(ed.undo_label(), Some("Add class"));
        let b = ed.add_element(ElementKind::Note, "N", None, Some(Pos::new(9, 0)));
        assert_eq!(ed.undo_label(), Some("Add note"));
        // A drag is one step, named after what moved.
        ed.move_elements(&[(a, Pos::new(5, 0))], Some("drag"));
        ed.move_elements(&[(a, Pos::new(6, 0))], Some("drag"));
        assert_eq!(ed.undo_label(), Some("Move Order"));
        ed.seal();
        ed.move_elements(&[(a, Pos::new(1, 1)), (b, Pos::new(2, 2))], None);
        assert_eq!(ed.undo_label(), Some("Move 2 items"));
        ed.rename(a, "Invoice").unwrap();
        assert_eq!(ed.undo_label(), Some("Rename Order to Invoice"));
        let mut el = ed.doc.elements[&a].clone();
        el.members.push(Member::plain("id"));
        ed.set_element(a, el);
        assert_eq!(ed.undo_label(), Some("Edit Invoice"));
        let r = ed.connect(a, b, RelationKind::Composition);
        assert_eq!(ed.undo_label(), Some("Add composition"));
        let mut rel = ed.doc.relations[&r].clone();
        rel.name = Some("owns".into());
        ed.set_relation(r, rel);
        assert_eq!(ed.undo_label(), Some("Edit composition"));
        ed.set_positions("Tidy", &[(a, Some(Pos::new(30, 30)))]);
        assert_eq!(ed.undo_label(), Some("Tidy"));
        ed.set_title(Some("T".into()));
        assert_eq!(ed.undo_label(), Some("Change title"));
        let p = ed.add_package("billing", None);
        assert_eq!(ed.undo_label(), Some("Add package"));
        ed.rename_package(p, "pay").unwrap();
        assert_eq!(ed.undo_label(), Some("Rename package billing to pay"));
        ed.delete_relations(&[r]);
        assert_eq!(ed.undo_label(), Some("Delete composition"));
        ed.delete(&[], &[p]);
        assert_eq!(ed.undo_label(), Some("Delete package pay"));
        ed.delete(&[a], &[]);
        assert_eq!(ed.undo_label(), Some("Delete Invoice"));
        ed.delete(&[b], &[]);
        ed.undo();
        ed.undo();
        assert_eq!(ed.redo_label(), Some("Delete Invoice"));
        assert_eq!(ed.undo_label(), Some("Delete package pay"));
        ed.redo();
        assert_eq!(ed.undo_label(), Some("Delete Invoice"));
        assert_eq!(ed.redo_label(), Some("Delete N"));
    }

    #[test]
    fn deleting_elements_and_other_relations_is_one_step() {
        let mut ed = Editor::new(Document::new());
        let a = ed.add_element(ElementKind::Class, "A", None, None);
        let b = ed.add_element(ElementKind::Class, "B", None, None);
        let c = ed.add_element(ElementKind::Class, "C", None, None);
        let ab = ed.connect(a, b, RelationKind::Association);
        let bc = ed.connect(b, c, RelationKind::Association);
        let before = ed.doc.clone();
        // C (with B--C) and the unrelated A--B go together.
        ed.delete_items(&[c], &[], &[ab]);
        assert!(ed.doc.relations.is_empty());
        assert!(!ed.doc.elements.contains_key(&c));
        assert_eq!(ed.undo_label(), Some("Delete 2 items"));
        assert!(ed.undo());
        assert_eq!(ed.doc, before);
        assert!(ed.doc.relations.contains_key(&bc));
        assert_eq!(ed.undo_label(), Some("Add association"));
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
        assert!(ed.rename(a, "  ").is_err());
        assert!(ed.rename(a, "a\tb").is_err());
        assert!(ed.rename(a, "Order").is_ok());
        assert_eq!(ed.doc.elements[&a].name, "Order");
        // Any other text is a name too; files quote it.
        assert!(ed.rename(a, "Order line").is_ok());
        assert_eq!(ed.doc.full_name(a), "\"Order line\"");
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
