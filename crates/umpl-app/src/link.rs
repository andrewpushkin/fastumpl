//! Links the text panel to the drawing: the lines that declare an item of
//! the document, and the item declared where the text cursor is.
//!
//! The text is parsed into a document of its own, with ids of its own, so
//! items are matched by name: elements and packages by full name, relations
//! by their ends, their kind and which of the relations with those is meant.
//! While the text and the document are in step every item matches; text not
//! yet applied simply matches less.

use std::collections::HashMap;

use umpl_core::{Document, PackageId, RelationId, RelationKind};
use umpl_text::{Item, Parsed};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum Key {
    Element(String),
    Package(String),
    /// From, to, kind, and the index among relations with these three.
    Relation(String, String, RelationKind, usize),
}

/// The key of every relation, in declaration order.
fn relation_keys(doc: &Document) -> Vec<(RelationId, Key)> {
    let mut seen: HashMap<(String, String, RelationKind), usize> = HashMap::new();
    doc.relations_in_order()
        .into_iter()
        .filter_map(|id| {
            let r = doc.relations.get(&id)?;
            let ends = (
                doc.endpoint_name(r.from.target),
                doc.endpoint_name(r.to.target),
                r.kind,
            );
            let n = seen.entry(ends.clone()).or_default();
            let key = Key::Relation(ends.0, ends.1, ends.2, *n);
            *n += 1;
            Some((id, key))
        })
        .collect()
}

fn key_of(doc: &Document, item: Item) -> Option<Key> {
    match item {
        Item::Element(id) => doc
            .elements
            .contains_key(&id)
            .then(|| Key::Element(doc.full_name(id))),
        Item::Package(id) => doc
            .packages
            .contains_key(&id)
            .then(|| Key::Package(doc.full_package_name(id))),
        Item::Relation(id) => relation_keys(doc)
            .into_iter()
            .find(|(r, _)| *r == id)
            .map(|(_, k)| k),
    }
}

fn find_package(doc: &Document, name: &str) -> Option<PackageId> {
    doc.packages_in_order()
        .into_iter()
        .find(|p| doc.full_package_name(*p) == name)
}

fn item_of(doc: &Document, key: &Key) -> Option<Item> {
    match key {
        Key::Element(name) => doc.find(name).map(Item::Element),
        Key::Package(name) => find_package(doc, name).map(Item::Package),
        Key::Relation(..) => relation_keys(doc)
            .into_iter()
            .find(|(_, k)| k == key)
            .map(|(id, _)| Item::Relation(id)),
    }
}

/// The declarations of a text, by key: first and last line (1-based).
#[derive(Default)]
pub struct Links {
    spans: Vec<(Key, usize, usize)>,
}

impl Links {
    pub fn new(parsed: &Parsed) -> Links {
        let doc = &parsed.doc;
        let rels: HashMap<RelationId, Key> = relation_keys(doc).into_iter().collect();
        let spans = parsed
            .spans
            .iter()
            .filter_map(|s| {
                let key = match s.item {
                    Item::Relation(id) => rels.get(&id).cloned()?,
                    other => key_of(doc, other)?,
                };
                Some((key, s.first, s.last))
            })
            .collect();
        Links { spans }
    }

    /// The lines that declare `item` of `doc`.
    pub fn lines_of(&self, doc: &Document, item: Item) -> Option<(usize, usize)> {
        let key = key_of(doc, item)?;
        self.spans
            .iter()
            .find(|(k, _, _)| *k == key)
            .map(|(_, a, b)| (*a, *b))
    }

    /// The item of `doc` declared at `line`, the innermost one, with its
    /// lines.
    pub fn item_at(&self, doc: &Document, line: usize) -> Option<(Item, (usize, usize))> {
        let (key, a, b) = self
            .spans
            .iter()
            .filter(|(_, a, b)| *a <= line && line <= *b)
            .min_by_key(|(_, a, b)| b - a)?;
        item_of(doc, key).map(|item| (item, (*a, *b)))
    }
}

/// The 1-based line holding character `char_index` of `text`.
pub fn line_of_char(text: &str, char_index: usize) -> usize {
    1 + text.chars().take(char_index).filter(|c| *c == '\n').count()
}

/// The character index of the first non-blank character on `line`
/// (1-based), or of the line's end.
pub fn line_start_char(text: &str, line: usize) -> usize {
    let mut chars = 0;
    for (n, l) in text.split_inclusive('\n').enumerate() {
        if n + 1 == line {
            let indent = l.chars().take_while(|c| *c == ' ' || *c == '\t').count();
            return chars + indent;
        }
        chars += l.chars().count();
    }
    chars
}

#[cfg(test)]
mod tests {
    use super::*;
    use umpl_text::parse;

    const TEXT: &str = "package p {\n  class A {\n    +x: int\n  }\n}\n\nclass B\n\np.A --> B\np.A --> B : twice\nB ..> p.A\n";

    #[test]
    fn items_and_lines_match_across_documents() {
        // The document has ids of its own: built from other text, in
        // another order.
        let doc =
            parse("class B\npackage p {\n  class A\n}\nB ..> p.A\np.A --> B\np.A --> B\n").doc;
        let links = Links::new(&parse(TEXT));
        let a = Item::Element(doc.find("p.A").unwrap());
        let b = Item::Element(doc.find("B").unwrap());
        let p = Item::Package(*doc.packages.keys().next().unwrap());
        assert_eq!(links.lines_of(&doc, a), Some((2, 4)));
        assert_eq!(links.lines_of(&doc, b), Some((7, 7)));
        assert_eq!(links.lines_of(&doc, p), Some((1, 5)));
        let rels = doc.relations_in_order();
        assert_eq!(
            links.lines_of(&doc, Item::Relation(rels[0])),
            Some((11, 11))
        );
        assert_eq!(links.lines_of(&doc, Item::Relation(rels[1])), Some((9, 9)));
        assert_eq!(
            links.lines_of(&doc, Item::Relation(rels[2])),
            Some((10, 10))
        );

        assert_eq!(links.item_at(&doc, 3), Some((a, (2, 4))));
        assert_eq!(links.item_at(&doc, 5), Some((p, (1, 5))));
        assert_eq!(
            links.item_at(&doc, 10),
            Some((Item::Relation(rels[2]), (10, 10)))
        );
        assert_eq!(links.item_at(&doc, 6), None);
        // A class only the text has yet matches nothing.
        let links = Links::new(&parse("class New\n"));
        assert_eq!(links.item_at(&doc, 1), None);
    }

    #[test]
    fn lines_and_characters() {
        let t = "ab\n  cd\n\né\n";
        assert_eq!(line_of_char(t, 0), 1);
        assert_eq!(line_of_char(t, 2), 1);
        assert_eq!(line_of_char(t, 3), 2);
        assert_eq!(line_of_char(t, 9), 4);
        assert_eq!(line_start_char(t, 2), 5);
        assert_eq!(line_start_char(t, 4), 9);
        assert_eq!(line_start_char(t, 9), 11);
    }
}
