use crate::model::*;

/// One undoable change: replace a single slot of the document.
///
/// `None` in an element, relation or package slot means "absent", so the same
/// op both inserts and removes. Applying an op returns the op that restores
/// the old value.
#[derive(Clone, Debug, PartialEq)]
pub enum Op {
    Element(ElementId, Option<Element>),
    Relation(RelationId, Option<Relation>),
    Package(PackageId, Option<Package>),
    Root(Vec<Decl>),
    Title(Option<String>),
    TrailingComments(Vec<String>),
}

/// Which slot an op writes to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Slot {
    Element(ElementId),
    Relation(RelationId),
    Package(PackageId),
    Root,
    Title,
    TrailingComments,
}

impl Op {
    pub fn slot(&self) -> Slot {
        match self {
            Op::Element(id, _) => Slot::Element(*id),
            Op::Relation(id, _) => Slot::Relation(*id),
            Op::Package(id, _) => Slot::Package(*id),
            Op::Root(_) => Slot::Root,
            Op::Title(_) => Slot::Title,
            Op::TrailingComments(_) => Slot::TrailingComments,
        }
    }
}

impl Document {
    /// Applies `op` and returns its inverse.
    pub fn apply(&mut self, op: Op) -> Op {
        match op {
            Op::Element(id, value) => {
                self.reserve(id.0);
                let old = match value {
                    Some(v) => self.elements.insert(id, v),
                    None => self.elements.remove(&id),
                };
                Op::Element(id, old)
            }
            Op::Relation(id, value) => {
                self.reserve(id.0);
                let old = match value {
                    Some(v) => self.relations.insert(id, v),
                    None => self.relations.remove(&id),
                };
                Op::Relation(id, old)
            }
            Op::Package(id, value) => {
                self.reserve(id.0);
                let old = match value {
                    Some(v) => self.packages.insert(id, v),
                    None => self.packages.remove(&id),
                };
                Op::Package(id, old)
            }
            Op::Root(value) => Op::Root(std::mem::replace(&mut self.root, value)),
            Op::Title(value) => Op::Title(std::mem::replace(&mut self.title, value)),
            Op::TrailingComments(value) => {
                Op::TrailingComments(std::mem::replace(&mut self.trailing_comments, value))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_returns_inverse_round_trip() {
        let mut doc = Document::new();
        let before = doc.clone();
        let id = doc.new_element_id();
        let ops = vec![
            Op::Element(id, Some(Element::new(ElementKind::Class, "A"))),
            Op::Root(vec![Decl::Element(id)]),
            Op::Title(Some("T".into())),
        ];
        let inverses: Vec<Op> = ops.into_iter().map(|o| doc.apply(o)).collect();
        assert_eq!(doc.elements.len(), 1);
        assert_eq!(doc.title.as_deref(), Some("T"));
        for inv in inverses.into_iter().rev() {
            doc.apply(inv);
        }
        assert_eq!(doc.elements, before.elements);
        assert_eq!(doc.root, before.root);
        assert_eq!(doc.title, before.title);
    }
}
