//! Converts a document into the layout engine's graph and back.

use std::collections::HashMap;

use umpl_core::*;
use umpl_layout::{Edge, EdgeKind, Graph, Group, Layout, Node, Options};

use crate::measure::{box_content, label_size, text_width, BoxContent};

/// What a layout node stands for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodeRef {
    Element(ElementId),
    /// Keeps an empty package visible; not stored in the file.
    Placeholder(PackageId),
}

/// Links layout indices to document ids.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Mapping {
    pub nodes: Vec<NodeRef>,
    pub contents: Vec<Option<BoxContent>>,
    pub relations: Vec<RelationId>,
    pub packages: Vec<PackageId>,
    node_of: HashMap<ElementId, usize>,
    edge_of: HashMap<RelationId, usize>,
}

impl Mapping {
    pub fn node_of(&self, id: ElementId) -> Option<usize> {
        self.node_of.get(&id).copied()
    }

    pub fn edge_of(&self, id: RelationId) -> Option<usize> {
        self.edge_of.get(&id).copied()
    }
}

/// Size of an empty package's placeholder, in grid units.
const PLACEHOLDER: (i32, i32) = (12, 4);
/// Package name tab height, in grid units.
pub const PACKAGE_HEADER: i32 = 3;

fn edge_kind(k: RelationKind) -> EdgeKind {
    match k {
        RelationKind::Generalization | RelationKind::Realization => EdgeKind::Hierarchy,
        RelationKind::Composition | RelationKind::Aggregation => EdgeKind::Whole,
        RelationKind::Association | RelationKind::Directed => EdgeKind::Assoc,
        RelationKind::Dependency => EdgeKind::Dep,
        RelationKind::NoteLink => EdgeKind::Note,
    }
}

/// Builds the layout graph: one node per element (in file order), one edge
/// per relation, one group per package.
pub fn build_graph(doc: &Document) -> (Graph, Mapping) {
    let mut g = Graph::default();
    let mut m = Mapping::default();

    let packages = doc.packages_in_order();
    let group_of: HashMap<PackageId, usize> =
        packages.iter().enumerate().map(|(i, p)| (*p, i)).collect();
    for p in &packages {
        let name = &doc.packages[p].name;
        g.groups.push(Group {
            parent: doc
                .container_of(&Decl::Package(*p))
                .and_then(|q| group_of.get(&q).copied()),
            header: PACKAGE_HEADER,
            min_w: ((text_width(name, 12.0) + 24.0) / crate::GRID).ceil() as i32 + 4,
        });
    }
    m.packages = packages.clone();

    for id in doc.elements_in_order() {
        let el = &doc.elements[&id];
        let content = box_content(el);
        let (w, h) = content.size();
        m.node_of.insert(id, g.nodes.len());
        g.nodes.push(Node {
            w,
            h,
            group: doc
                .element_package(id)
                .and_then(|p| group_of.get(&p).copied()),
            pos: el.pos.map(|p| (p.x, p.y)),
        });
        m.nodes.push(NodeRef::Element(id));
        m.contents.push(Some(content));
    }
    for p in &packages {
        if doc.elements_in(*p).is_empty()
            && !doc
                .body(Some(*p))
                .iter()
                .any(|d| matches!(d, Decl::Package(_)))
        {
            g.nodes.push(Node {
                w: PLACEHOLDER.0,
                h: PLACEHOLDER.1,
                group: group_of.get(p).copied(),
                pos: None,
            });
            m.nodes.push(NodeRef::Placeholder(*p));
            m.contents.push(None);
        }
    }

    for id in doc.relations_in_order() {
        let r = &doc.relations[&id];
        let (Some(a), Some(b)) = (m.node_of(r.from.element), m.node_of(r.to.element)) else {
            continue;
        };
        let mut e = Edge::new(a, b, edge_kind(r.kind));
        e.labels = [
            r.from.label.as_deref().map(label_size),
            r.to.label.as_deref().map(label_size),
            r.name.as_deref().map(label_size),
        ];
        m.edge_of.insert(id, g.edges.len());
        g.edges.push(e);
        m.relations.push(id);
    }
    (g, m)
}

/// A document's graph together with a computed layout.
#[derive(Clone, Debug, Default)]
pub struct Diagram {
    pub graph: Graph,
    pub mapping: Mapping,
    pub layout: Layout,
}

impl Diagram {
    /// Keeps stored positions, places elements that have none (or lays out
    /// everything if nothing has a position) and routes the lines.
    pub fn arrange(doc: &Document) -> Diagram {
        let (graph, mapping) = build_graph(doc);
        let layout = umpl_layout::route_only(&graph);
        Diagram {
            graph,
            mapping,
            layout,
        }
    }

    /// Lays out everything from scratch (Tidy).
    pub fn tidy(doc: &Document, opts: &Options) -> Diagram {
        let (graph, mapping) = build_graph(doc);
        let layout = umpl_layout::layout_all(&graph, opts);
        Diagram {
            graph,
            mapping,
            layout,
        }
    }

    /// Element positions to store in the document.
    pub fn positions(&self) -> Vec<(ElementId, Option<Pos>)> {
        self.mapping
            .nodes
            .iter()
            .zip(&self.layout.nodes)
            .filter_map(|(r, p)| match r {
                NodeRef::Element(id) => Some((*id, Some(Pos::new(p.0, p.1)))),
                NodeRef::Placeholder(_) => None,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn graph_mirrors_document() {
        let p = umpl_text::parse(include_str!("../../../examples/shop.umpl"));
        let (g, m) = build_graph(&p.doc);
        assert_eq!(g.nodes.len(), 6);
        assert_eq!(g.edges.len(), 4);
        assert_eq!(g.groups.len(), 1);
        let pay = m.node_of(p.doc.find("billing.Payment").unwrap()).unwrap();
        assert_eq!(g.nodes[pay].group, Some(0));
        assert!(g.nodes[pay].pos.is_some());
        let gen = g
            .edges
            .iter()
            .find(|e| e.kind == EdgeKind::Hierarchy)
            .unwrap();
        assert_eq!(gen.to, pay);
    }

    #[test]
    fn empty_package_gets_placeholder() {
        let p = umpl_text::parse("package empty {}\nclass A\n");
        let (g, m) = build_graph(&p.doc);
        assert_eq!(g.nodes.len(), 2);
        assert!(matches!(m.nodes[1], NodeRef::Placeholder(_)));
        let d = Diagram::arrange(&p.doc);
        assert_eq!(d.positions().len(), 1);
    }
}
