//! Places relation labels: multiplicities and roles beside each line end,
//! the relation name beside its longest segment. Each label takes the first
//! candidate spot that overlaps no box, no line, no earlier label and no
//! package border or title; when none is free, the nearest free spot in
//! growing rings around the best of them (up to [`REACH`] grid units).

use crate::graph::*;

/// Candidate top-left corners for a label of size `(w, h)` next to the end
/// `p` of a line whose first step goes from `p` towards `q`.
fn end_candidates(p: (i32, i32), q: (i32, i32), (w, h): (i32, i32)) -> Vec<(i32, i32)> {
    let (dx, dy) = ((q.0 - p.0).signum(), (q.1 - p.1).signum());
    let mut out = Vec::new();
    // Beside the line near the box first, then further along it.
    for step in 0..4 {
        if dx == 0 {
            let y = if dy > 0 { p.1 + 1 } else { p.1 - 1 - h } + dy * step * (h + 1);
            out.extend([(p.0 + 1, y), (p.0 - 1 - w, y)]);
        } else {
            let x = if dx > 0 { p.0 + 1 } else { p.0 - 1 - w } + dx * step * (w + 1);
            out.extend([(x, p.1 - h), (x, p.1 + 1)]);
        }
    }
    out
}

fn middle_candidates(route: &Route, (w, h): (i32, i32)) -> Vec<(i32, i32)> {
    let mut segs: Vec<((i32, i32), (i32, i32))> = route.segments().collect();
    segs.sort_by_key(|(p, q)| std::cmp::Reverse((p.0 - q.0).abs() + (p.1 - q.1).abs()));
    let mut out = Vec::new();
    for (p, q) in segs {
        let mid = ((p.0 + q.0) / 2, (p.1 + q.1) / 2);
        for shift in [0, -1, 1, -2, 2] {
            if p.1 == q.1 {
                let x = mid.0 - w / 2 + shift * (w / 2 + 1);
                out.extend([
                    (x, mid.1 - h),
                    (x, mid.1 + 1),
                    (x, mid.1 - h - 1),
                    (x, mid.1 + 2),
                ]);
            } else {
                let y = mid.1 - h / 2 + shift * (h + 1);
                out.extend([
                    (mid.0 + 1, y),
                    (mid.0 - 1 - w, y),
                    (mid.0 + 2, y),
                    (mid.0 - 2 - w, y),
                ]);
            }
        }
    }
    out
}

/// Does a segment of `route` run through the inside of `r` (not only
/// along its edge)?
fn route_through(r: &Rect, route: &Route) -> bool {
    route.segments().any(|(p, q)| {
        if p.1 == q.1 {
            let (a, b) = (p.0.min(q.0), p.0.max(q.0));
            p.1 > r.y && p.1 < r.bottom() && a < r.right() && b > r.x
        } else {
            let (a, b) = (p.1.min(q.1), p.1.max(q.1));
            p.0 > r.x && p.0 < r.right() && a < r.bottom() && b > r.y
        }
    })
}

/// The parts of a package frame text must stay clear of: its border
/// (crossed when `r` is partly inside) and its title.
fn frame_hit(r: &Rect, frame: &Rect, group: &Group) -> bool {
    let overlaps = |a: &Rect, b: &Rect| a.too_close(b, 0);
    if !overlaps(r, frame) {
        return false;
    }
    let inside = r.x >= frame.x
        && r.y >= frame.y
        && r.right() <= frame.right()
        && r.bottom() <= frame.bottom();
    let title = Rect::new(frame.x, frame.y, group.min_w.min(frame.w), group.header + 1);
    !inside || overlaps(r, &title)
}

/// How many labels are drawn over something: a box, another label, a line
/// through their text, a package's border or title.
pub fn label_overlaps(
    graph: &Graph,
    nodes: &[(i32, i32)],
    routes: &[Route],
    labels: &[[Option<(i32, i32)>; 3]],
) -> usize {
    let boxes: Vec<Rect> = graph
        .nodes
        .iter()
        .zip(nodes)
        .map(|(n, p)| Rect::new(p.0, p.1, n.w, n.h))
        .collect();
    let frames = group_rects(graph, nodes);
    let rects: Vec<Rect> = labels
        .iter()
        .enumerate()
        .flat_map(|(e, ls)| {
            (0..3).filter_map(move |k| {
                let p = ls[k]?;
                let s = graph.edges.get(e)?.labels[k]?;
                Some(Rect::new(p.0, p.1, s.0, s.1))
            })
        })
        .collect();
    rects
        .iter()
        .enumerate()
        .filter(|(i, r)| {
            boxes.iter().any(|b| b.too_close(r, 0))
                || rects
                    .iter()
                    .enumerate()
                    .any(|(j, o)| j != *i && o.too_close(r, 0))
                || routes.iter().any(|route| route_through(r, route))
                || frames
                    .iter()
                    .zip(&graph.groups)
                    .any(|(f, g)| frame_hit(r, f, g))
        })
        .count()
}

/// Cell size of [`Obstacles`]' grid, in grid units.
const CELL: i32 = 16;

/// The boxes, labels and line segments touching one grid cell, by index.
type Cell = (Vec<usize>, Vec<usize>, Vec<usize>);

/// What a label must keep clear of, filed by grid cell so a spot is
/// checked against what is near it only.
struct Obstacles {
    boxes: Vec<Rect>,
    labels: Vec<Rect>,
    /// Line segments, as `(from, to)`.
    segments: Vec<((i32, i32), (i32, i32))>,
    frames: Vec<(Rect, Rect)>,
    /// Per cell: boxes, labels and segments touching it.
    cells: std::collections::HashMap<(i32, i32), Cell>,
}

impl Obstacles {
    fn cells_of(r: &Rect) -> impl Iterator<Item = (i32, i32)> {
        let (x0, x1) = (r.x.div_euclid(CELL), r.right().div_euclid(CELL));
        let (y0, y1) = (r.y.div_euclid(CELL), r.bottom().div_euclid(CELL));
        (x0..=x1).flat_map(move |x| (y0..=y1).map(move |y| (x, y)))
    }

    fn new(graph: &Graph, nodes: &[(i32, i32)], routes: &[Route]) -> Obstacles {
        let mut o = Obstacles {
            boxes: Vec::new(),
            labels: Vec::new(),
            segments: Vec::new(),
            frames: Vec::new(),
            cells: Default::default(),
        };
        for (n, p) in graph.nodes.iter().zip(nodes) {
            let r = Rect::new(p.0, p.1, n.w, n.h);
            for c in Self::cells_of(&r) {
                o.cells.entry(c).or_default().0.push(o.boxes.len());
            }
            o.boxes.push(r);
        }
        for route in routes {
            for (a, b) in route.segments() {
                let r = Rect::new(
                    a.0.min(b.0),
                    a.1.min(b.1),
                    (a.0 - b.0).abs(),
                    (a.1 - b.1).abs(),
                );
                for c in Self::cells_of(&r) {
                    o.cells.entry(c).or_default().2.push(o.segments.len());
                }
                o.segments.push((a, b));
            }
        }
        for (f, g) in group_rects(graph, nodes).iter().zip(&graph.groups) {
            let title = Rect::new(f.x, f.y, g.min_w.min(f.w), g.header + 1);
            o.frames.push((*f, title));
        }
        o
    }

    fn add_label(&mut self, r: Rect) {
        for c in Self::cells_of(&r) {
            self.cells.entry(c).or_default().1.push(self.labels.len());
        }
        self.labels.push(r);
    }

    /// How bad a label at `r` is: over a box or a package's border or title
    /// is worst, then over another label, then each line through it.
    fn badness(&self, r: &Rect) -> usize {
        let mut seen_b = std::collections::HashSet::new();
        let mut seen_l = std::collections::HashSet::new();
        let mut seen_s = std::collections::HashSet::new();
        let mut bad = 0;
        for c in Self::cells_of(r) {
            let Some((bs, ls, ss)) = self.cells.get(&c) else {
                continue;
            };
            for &b in bs {
                if seen_b.insert(b) && self.boxes[b].too_close(r, 0) {
                    bad += 100;
                }
            }
            for &l in ls {
                if seen_l.insert(l) && self.labels[l].too_close(r, 0) {
                    bad += 10;
                }
            }
            for &k in ss {
                if seen_s.insert(k) {
                    let route = Route {
                        points: vec![self.segments[k].0, self.segments[k].1],
                    };
                    if route_through(r, &route) {
                        bad += 1;
                    }
                }
            }
        }
        for (f, title) in &self.frames {
            if f.too_close(r, 0) {
                let inside =
                    r.x >= f.x && r.y >= f.y && r.right() <= f.right() && r.bottom() <= f.bottom();
                if !inside || title.too_close(r, 0) {
                    bad += 100;
                }
            }
        }
        bad
    }
}

/// Spots around `(x, y)` in rings growing one grid unit at a time, up
/// to `reach` away: where to look when no spot beside the line is free.
fn rings(x: i32, y: i32, reach: i32) -> impl Iterator<Item = (i32, i32)> {
    (1..=reach).flat_map(move |k| {
        let top = (-k..=k).map(move |d| (x + d, y - k));
        let bottom = (-k..=k).map(move |d| (x + d, y + k));
        let left = (1 - k..k).map(move |d| (x - k, y + d));
        let right = (1 - k..k).map(move |d| (x + k, y + d));
        top.chain(right).chain(bottom).chain(left)
    })
}

/// How far from its line a label may go to find a free spot.
const REACH: i32 = 16;

pub(crate) fn place_labels(
    graph: &Graph,
    nodes: &[(i32, i32)],
    routes: &[Route],
) -> Vec<[Option<(i32, i32)>; 3]> {
    let mut obstacles = Obstacles::new(graph, nodes, routes);
    let mut out = vec![[None; 3]; graph.edges.len()];
    for (e, edge) in graph.edges.iter().enumerate() {
        let route = &routes[e];
        if route.points.len() < 2 {
            continue;
        }
        let n = route.points.len();
        for k in 0..3 {
            let Some(size) = edge.labels[k] else { continue };
            let cands = match k {
                0 => end_candidates(route.points[0], route.points[1], size),
                1 => end_candidates(route.points[n - 1], route.points[n - 2], size),
                _ => middle_candidates(route, size),
            };
            let bad = |c: (i32, i32)| obstacles.badness(&Rect::new(c.0, c.1, size.0, size.1));
            // First free spot beside the line, else the nearest free spot
            // around the best of those, else the least bad one.
            let scored: Vec<(usize, (i32, i32))> = cands.iter().map(|&c| (bad(c), c)).collect();
            let best = scored
                .iter()
                .enumerate()
                .min_by_key(|(i, (b, _))| (*b, *i))
                .map(|(_, &(b, c))| (b, c));
            let pick = best.map(|(b, c)| match b {
                0 => c,
                _ => rings(c.0, c.1, REACH).find(|&p| bad(p) == 0).unwrap_or(c),
            });
            if let Some(p) = pick {
                obstacles.add_label(Rect::new(p.0, p.1, size.0, size.1));
                out[e][k] = Some(p);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::router::route_all;

    fn labelled_pair() -> (Graph, Vec<(i32, i32)>) {
        let mut g = Graph::default();
        g.add_node(10, 4);
        g.add_node(10, 4);
        let e = g.add_edge(0, 1, EdgeKind::Assoc);
        g.edges[e].labels = [Some((3, 2)), Some((5, 2)), Some((8, 2))];
        (g, vec![(0, 0), (0, 24)])
    }

    #[test]
    fn end_label_does_not_overlap_box() {
        let (g, nodes) = labelled_pair();
        let routes = route_all(&g, &nodes);
        let labels = place_labels(&g, &nodes, &routes);
        for k in 0..3 {
            let p = labels[0][k].unwrap();
            let size = g.edges[0].labels[k].unwrap();
            let r = Rect::new(p.0, p.1, size.0, size.1);
            for (i, n) in nodes.iter().enumerate() {
                assert!(
                    !r.too_close(&Rect::new(n.0, n.1, g.nodes[i].w, g.nodes[i].h), 0),
                    "label {k} on box {i}"
                );
            }
        }
    }

    /// What counts as drawn over something: across a package's border, on
    /// its title, or with a line through it; wholly inside a package and
    /// clear of its title is fine.
    #[test]
    fn label_overlaps_counts_borders_titles_and_lines() {
        let mut g = Graph::default();
        g.groups.push(Group {
            parent: None,
            header: 3,
            min_w: 12,
            frame: None,
        });
        let a = g.add_node(10, 4);
        g.nodes[a].group = Some(0);
        let b = g.add_node(10, 4);
        let e = g.add_edge(a, b, EdgeKind::Assoc);
        g.edges[e].labels = [None, None, Some((6, 2))];
        // a at (0, 20) in a package whose frame was made bigger: from
        // (-2, 15) to (28, 35); b well outside. The line runs along y = 22.
        g.groups[0].frame = Some(Rect::new(-2, 15, 30, 20));
        let nodes = vec![(0, 20), (40, 20)];
        let route = Route {
            points: vec![(10, 22), (40, 22)],
        };
        let count = |at: (i32, i32)| {
            label_overlaps(
                &g,
                &nodes,
                std::slice::from_ref(&route),
                &[[None, None, Some(at)]],
            )
        };
        // Across the right border.
        assert_eq!(count((25, 28)), 1);
        // On the title.
        assert_eq!(count((-1, 15)), 1);
        // The line through it.
        assert_eq!(count((30, 21)), 1);
        // Above the line, outside the package: fine.
        assert_eq!(count((30, 18)), 0);
        // Inside the package, clear of its title and box: fine.
        assert_eq!(count((14, 28)), 0);
    }

    #[test]
    fn labels_do_not_overlap_each_other() {
        let (g, nodes) = labelled_pair();
        let routes = route_all(&g, &nodes);
        let labels = place_labels(&g, &nodes, &routes);
        let rects: Vec<Rect> = (0..3)
            .map(|k| {
                let p = labels[0][k].unwrap();
                let s = g.edges[0].labels[k].unwrap();
                Rect::new(p.0, p.1, s.0, s.1)
            })
            .collect();
        for a in 0..3 {
            for b in a + 1..3 {
                assert!(!rects[a].too_close(&rects[b], 0));
            }
        }
    }
}
