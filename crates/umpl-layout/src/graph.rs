/// Padding between a group's border and its content, in grid units.
pub const GROUP_PAD: i32 = 2;
/// Minimum gap between boxes.
pub const NODE_GAP: i32 = 5;
/// Minimum gap between layers.
pub const LAYER_GAP: i32 = 10;

/// How an edge influences placement.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EdgeKind {
    /// `from` is a child of `to` (inheritance): `to` goes above `from`,
    /// and the line enters `to` from below.
    Hierarchy,
    /// `from` is the whole, `to` the part: whole preferably above.
    Whole,
    /// Association: ends prefer to sit side by side.
    Assoc,
    /// Dependency: like an association, weaker.
    Dep,
    /// Note attachment: the note sits next to what it describes.
    Note,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Node {
    pub w: i32,
    pub h: i32,
    /// The innermost group containing this node.
    pub group: Option<usize>,
    /// Current top-left corner. For [`crate::place_new`] and
    /// [`crate::route_only`] this is fixed; for [`crate::layout_all`] it is a
    /// hint for the starting order.
    pub pos: Option<(i32, i32)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Edge {
    pub from: usize,
    pub to: usize,
    pub kind: EdgeKind,
    /// Sizes (w, h) of the label at the `from` end, the `to` end, and the
    /// middle, if any.
    pub labels: [Option<(i32, i32)>; 3],
}

impl Edge {
    pub fn new(from: usize, to: usize, kind: EdgeKind) -> Edge {
        Edge {
            from,
            to,
            kind,
            labels: [None; 3],
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Group {
    pub parent: Option<usize>,
    /// Height of the name tab above the content.
    pub header: i32,
    /// Minimum width (the name tab must fit).
    pub min_w: i32,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Graph {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    pub groups: Vec<Group>,
}

impl Graph {
    pub fn add_node(&mut self, w: i32, h: i32) -> usize {
        self.nodes.push(Node {
            w,
            h,
            group: None,
            pos: None,
        });
        self.nodes.len() - 1
    }

    pub fn add_edge(&mut self, from: usize, to: usize, kind: EdgeKind) -> usize {
        self.edges.push(Edge::new(from, to, kind));
        self.edges.len() - 1
    }

    /// Is `g` equal to or nested inside `ancestor`?
    pub fn group_within(&self, g: usize, ancestor: usize) -> bool {
        let mut cur = Some(g);
        let mut steps = 0;
        while let Some(c) = cur {
            if c == ancestor {
                return true;
            }
            steps += 1;
            if steps > self.groups.len() {
                return false;
            }
            cur = self.groups[c].parent;
        }
        false
    }

    /// The chain of groups from `g` outwards.
    pub fn group_chain(&self, g: Option<usize>) -> Vec<usize> {
        let mut out = Vec::new();
        let mut cur = g;
        while let Some(c) = cur {
            if out.len() > self.groups.len() {
                break;
            }
            out.push(c);
            cur = self.groups[c].parent;
        }
        out
    }
}

/// An axis-aligned rectangle in grid units.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub fn new(x: i32, y: i32, w: i32, h: i32) -> Rect {
        Rect { x, y, w, h }
    }

    pub fn right(&self) -> i32 {
        self.x + self.w
    }

    pub fn bottom(&self) -> i32 {
        self.y + self.h
    }

    pub fn center(&self) -> (i32, i32) {
        (self.x + self.w / 2, self.y + self.h / 2)
    }

    /// Point on the border or inside.
    pub fn contains(&self, (x, y): (i32, i32)) -> bool {
        x >= self.x && x <= self.right() && y >= self.y && y <= self.bottom()
    }

    /// Do the rectangles overlap or come closer than `gap`?
    pub fn too_close(&self, o: &Rect, gap: i32) -> bool {
        self.x < o.right() + gap && o.x < self.right() + gap && self.y < o.bottom() + gap && o.y < self.bottom() + gap
    }

    pub fn union(&self, o: &Rect) -> Rect {
        let x = self.x.min(o.x);
        let y = self.y.min(o.y);
        Rect::new(x, y, self.right().max(o.right()) - x, self.bottom().max(o.bottom()) - y)
    }

    pub fn expand(&self, by: i32) -> Rect {
        Rect::new(self.x - by, self.y - by, self.w + 2 * by, self.h + 2 * by)
    }
}

/// An orthogonal polyline from the `from` box border to the `to` box border.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Route {
    pub points: Vec<(i32, i32)>,
}

impl Route {
    pub fn segments(&self) -> impl Iterator<Item = ((i32, i32), (i32, i32))> + '_ {
        self.points.windows(2).map(|w| (w[0], w[1]))
    }

    pub fn bends(&self) -> usize {
        self.points.len().saturating_sub(2)
    }

    pub fn length(&self) -> i32 {
        self.segments().map(|(a, b)| (a.0 - b.0).abs() + (a.1 - b.1).abs()).sum()
    }

    /// Removes zero-length segments and merges collinear ones.
    pub fn simplify(&mut self) {
        let mut out: Vec<(i32, i32)> = Vec::with_capacity(self.points.len());
        for &p in &self.points {
            if out.last() == Some(&p) {
                continue;
            }
            if out.len() >= 2 {
                let a = out[out.len() - 2];
                let b = out[out.len() - 1];
                if (a.0 == b.0 && b.0 == p.0) || (a.1 == b.1 && b.1 == p.1) {
                    out.pop();
                }
            }
            out.push(p);
        }
        self.points = out;
    }
}

/// The result of a layout.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Layout {
    /// Top-left corner of each node.
    pub nodes: Vec<(i32, i32)>,
    /// One route per edge.
    pub routes: Vec<Route>,
    /// Bounds of each group, including padding and header.
    pub groups: Vec<Rect>,
    /// Top-left corner of each edge's labels (`from` end, `to` end, middle).
    pub labels: Vec<[Option<(i32, i32)>; 3]>,
    pub stats: crate::Stats,
}

impl Layout {
    pub fn node_rect(&self, graph: &Graph, i: usize) -> Rect {
        let (x, y) = self.nodes[i];
        Rect::new(x, y, graph.nodes[i].w, graph.nodes[i].h)
    }

    /// Bounds of everything drawn.
    pub fn bounds(&self, graph: &Graph) -> Rect {
        let mut r: Option<Rect> = None;
        let mut add = |o: Rect| r = Some(r.map_or(o, |r| r.union(&o)));
        for i in 0..self.nodes.len() {
            add(self.node_rect(graph, i));
        }
        for g in &self.groups {
            add(*g);
        }
        for route in &self.routes {
            for p in &route.points {
                add(Rect::new(p.0, p.1, 0, 0));
            }
        }
        for (e, ls) in self.labels.iter().enumerate() {
            for (k, l) in ls.iter().enumerate() {
                if let (Some(p), Some(size)) = (l, graph.edges[e].labels[k]) {
                    add(Rect::new(p.0, p.1, size.0, size.1));
                }
            }
        }
        r.unwrap_or_default()
    }
}

/// Group bounds around their nodes and nested groups.
pub fn group_rects(graph: &Graph, nodes: &[(i32, i32)]) -> Vec<Rect> {
    let n = graph.groups.len();
    let mut content: Vec<Option<Rect>> = vec![None; n];
    for (i, node) in graph.nodes.iter().enumerate() {
        if let Some(g) = node.group {
            let r = Rect::new(nodes[i].0, nodes[i].1, node.w, node.h);
            content[g] = Some(content[g].map_or(r, |c| c.union(&r)));
        }
    }
    // Children before parents: deeper groups first.
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by_key(|g| std::cmp::Reverse(graph.group_chain(Some(*g)).len()));
    let mut rects = vec![Rect::default(); n];
    for g in order {
        let inner = content[g].unwrap_or(Rect::new(0, 0, 0, 0));
        let mut r = Rect::new(
            inner.x - GROUP_PAD,
            inner.y - GROUP_PAD - graph.groups[g].header,
            inner.w + 2 * GROUP_PAD,
            inner.h + 2 * GROUP_PAD + graph.groups[g].header,
        );
        r.w = r.w.max(graph.groups[g].min_w);
        rects[g] = r;
        if let Some(p) = graph.groups[g].parent {
            content[p] = Some(content[p].map_or(r, |c| c.union(&r)));
        }
    }
    rects
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simplify_merges_collinear() {
        let mut r = Route {
            points: vec![(0, 0), (0, 0), (0, 5), (0, 9), (4, 9), (8, 9)],
        };
        r.simplify();
        assert_eq!(r.points, vec![(0, 0), (0, 9), (8, 9)]);
    }

    #[test]
    fn nested_group_rects_contain_children() {
        let mut g = Graph::default();
        g.groups.push(Group { parent: None, header: 3, min_w: 0 });
        g.groups.push(Group { parent: Some(0), header: 3, min_w: 0 });
        let a = g.add_node(10, 4);
        g.nodes[a].group = Some(1);
        let rects = group_rects(&g, &[(10, 10)]);
        assert!(rects[1].contains((10, 10)) && rects[1].contains((20, 14)));
        assert!(rects[0].contains((rects[1].x, rects[1].y)));
        assert!(rects[0].contains((rects[1].right(), rects[1].bottom())));
    }
}
