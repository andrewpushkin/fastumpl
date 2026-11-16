//! Keeps the computed diagram (graph, layout, shapes) in step with the
//! document.

use std::time::{Duration, Instant};

use umpl_core::{Editor, ElementId, PackageId, Pos, RelationId};
use umpl_draw::{build_graph, build_scene, Diagram, NodeRef, Scene, SceneOptions, Theme};
use umpl_layout::{group_rects, update_after_move, Graph, Layout, Route};

pub const GLIDE: Duration = Duration::from_millis(200);

#[derive(Clone, Default, PartialEq)]
pub struct Selection {
    pub elements: Vec<ElementId>,
    pub relations: Vec<RelationId>,
    pub packages: Vec<PackageId>,
}

impl Selection {
    pub fn is_empty(&self) -> bool {
        self.elements.is_empty() && self.relations.is_empty() && self.packages.is_empty()
    }

    pub fn clear(&mut self) {
        *self = Selection::default();
    }

    pub fn only_element(id: ElementId) -> Selection {
        Selection {
            elements: vec![id],
            ..Selection::default()
        }
    }
}

struct Glide {
    start: Instant,
    from: Vec<(i32, i32)>,
}

#[derive(Clone, PartialEq)]
struct SceneKey {
    revision: u64,
    selection: Selection,
    dark: bool,
    hide_members: bool,
    glide_step: Option<u32>,
}

pub struct View {
    pub diagram: Diagram,
    revision: Option<u64>,
    scene: Option<(SceneKey, Scene)>,
    glide: Option<Glide>,
}

fn same_structure(a: &Graph, b: &Graph) -> bool {
    a.edges == b.edges
        && a.groups == b.groups
        && a.nodes.len() == b.nodes.len()
        && a.nodes
            .iter()
            .zip(&b.nodes)
            .all(|(x, y)| x.w == y.w && x.h == y.h && x.group == y.group)
}

impl View {
    pub fn new() -> View {
        View {
            diagram: Diagram::default(),
            revision: None,
            scene: None,
            glide: None,
        }
    }

    /// Brings the layout up to date with the document. Elements that have
    /// no position yet are placed, and their positions are added to the
    /// last undo step.
    pub fn sync(&mut self, ed: &mut Editor) {
        if self.revision == Some(ed.revision) {
            return;
        }
        let (mut graph, mapping) = build_graph(&ed.doc);
        let old = &self.diagram;
        let layout = if self.revision.is_some()
            && same_structure(&graph, &old.graph)
            && graph.nodes.iter().all(|n| n.pos.is_some())
        {
            let moved: Vec<usize> = (0..graph.nodes.len())
                .filter(|&i| graph.nodes[i].pos != old.graph.nodes[i].pos)
                .collect();
            if moved.is_empty() {
                old.layout.clone()
            } else {
                update_after_move(&graph, &old.layout, &moved)
            }
        } else {
            umpl_layout::route_only(&graph)
        };

        let missing: Vec<(ElementId, Option<Pos>)> = mapping
            .nodes
            .iter()
            .enumerate()
            .filter_map(|(i, r)| match r {
                NodeRef::Element(id) if graph.nodes[i].pos.is_none() => {
                    let p = layout.nodes[i];
                    graph.nodes[i].pos = Some(p);
                    Some((*id, Some(Pos::new(p.0, p.1))))
                }
                _ => None,
            })
            .collect();
        for (i, r) in mapping.nodes.iter().enumerate() {
            if matches!(r, NodeRef::Placeholder(_)) {
                graph.nodes[i].pos = Some(layout.nodes[i]);
            }
        }
        if !missing.is_empty() {
            let ops = ed.position_changes(&missing);
            ed.amend(ops);
        }
        self.diagram = Diagram {
            graph,
            mapping,
            layout,
        };
        self.revision = Some(ed.revision);
    }

    /// Re-routes every line from scratch (after a drag ends).
    pub fn reroute_all(&mut self) {
        let mut g = self.diagram.graph.clone();
        for (i, p) in self.diagram.layout.nodes.iter().enumerate() {
            g.nodes[i].pos = Some(*p);
        }
        self.diagram.layout = umpl_layout::route_only(&g);
        self.scene = None;
    }

    /// Replaces the layout with a finished Tidy result and glides boxes
    /// from where they were.
    pub fn install(&mut self, diagram: Diagram, revision: u64) {
        let from = if same_structure(&diagram.graph, &self.diagram.graph) {
            Some(self.diagram.layout.nodes.clone())
        } else {
            None
        };
        let mut diagram = diagram;
        for (i, p) in diagram.layout.nodes.iter().enumerate() {
            diagram.graph.nodes[i].pos = Some(*p);
        }
        self.diagram = diagram;
        self.revision = Some(revision);
        self.glide = from.map(|from| Glide {
            start: Instant::now(),
            from,
        });
        self.scene = None;
    }

    pub fn animating(&self) -> bool {
        self.glide
            .as_ref()
            .is_some_and(|g| g.start.elapsed() < GLIDE)
    }

    /// Layout to draw right now: during a glide, boxes between their old
    /// and new places and no lines.
    fn current_layout(&self) -> (Option<u32>, Layout) {
        let Some(g) = self.glide.as_ref().filter(|_| self.animating()) else {
            return (None, self.diagram.layout.clone());
        };
        let t = (g.start.elapsed().as_secs_f32() / GLIDE.as_secs_f32()).clamp(0.0, 1.0);
        let t = 1.0 - (1.0 - t).powi(3);
        let to = &self.diagram.layout.nodes;
        let nodes: Vec<(i32, i32)> = g
            .from
            .iter()
            .zip(to)
            .map(|(a, b)| {
                (
                    (a.0 as f32 + (b.0 - a.0) as f32 * t).round() as i32,
                    (a.1 as f32 + (b.1 - a.1) as f32 * t).round() as i32,
                )
            })
            .collect();
        let edges = self.diagram.graph.edges.len();
        let layout = Layout {
            groups: group_rects(&self.diagram.graph, &nodes),
            nodes,
            routes: vec![Route::default(); edges],
            labels: vec![[None; 3]; edges],
            stats: self.diagram.layout.stats,
        };
        (Some((t * 1000.0) as u32), layout)
    }

    /// The shapes to paint, rebuilt only when something they show changed.
    pub fn scene(
        &mut self,
        ed: &Editor,
        sel: &Selection,
        theme: &Theme,
        hide_members: bool,
    ) -> &Scene {
        let (glide_step, layout) = self.current_layout();
        let key = SceneKey {
            revision: ed.revision,
            selection: sel.clone(),
            dark: theme.dark,
            hide_members,
            glide_step,
        };
        if self.scene.as_ref().is_none_or(|(k, _)| *k != key) {
            let diagram = Diagram {
                graph: self.diagram.graph.clone(),
                mapping: self.diagram.mapping.clone(),
                layout,
            };
            let opts = SceneOptions {
                hide_members,
                selected_elements: &sel.elements,
                selected_relations: &sel.relations,
                selected_packages: &sel.packages,
            };
            let scene = build_scene(&ed.doc, &diagram, theme, &opts);
            self.scene = Some((key, scene));
        }
        &self.scene.as_ref().unwrap().1
    }

    /// Forgets everything (another document was opened).
    pub fn reset(&mut self) {
        *self = View::new();
    }
}
