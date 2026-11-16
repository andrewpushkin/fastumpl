use std::collections::HashSet;

use crate::graph::*;

/// Layout quality numbers. Lower is better everywhere.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Stats {
    pub crossings: usize,
    pub bends: usize,
    pub length: i64,
    /// Inheritance lines whose child is not below its parent.
    pub upward_violations: usize,
    /// Pairs of boxes that overlap or touch (hard rule).
    pub overlaps: usize,
    /// Grid points where two unrelated lines run along the same lane.
    pub shared_lanes: usize,
    /// Area of the bounding box, in square grid units.
    pub area: i64,
}

impl Stats {
    /// One number to compare layouts; design §6.7.
    pub fn score(&self) -> f64 {
        10.0 * self.crossings as f64
            + 20.0 * self.upward_violations as f64
            + 1.0 * self.bends as f64
            + 0.1 * self.length as f64
            + 1000.0 * self.overlaps as f64
            + 5.0 * self.shared_lanes as f64
            + 0.0005 * self.area as f64
    }
}

/// Lines in the same bundle may share lanes and do not cross each other:
/// all inheritance lines into one parent form one bundle (the trunk).
pub(crate) fn bundle_of(graph: &Graph, e: usize) -> u32 {
    let edge = &graph.edges[e];
    if edge.kind == EdgeKind::Hierarchy {
        (1 << 30) | edge.to as u32
    } else {
        e as u32
    }
}

type Seg = ((i32, i32), (i32, i32));

fn horizontal(s: &Seg) -> bool {
    s.0 .1 == s.1 .1 && s.0 .0 != s.1 .0
}

fn vertical(s: &Seg) -> bool {
    s.0 .0 == s.1 .0 && s.0 .1 != s.1 .1
}

/// Where a horizontal and a vertical segment cross at interior points of
/// both. Touching ends and T-junctions are not crossings.
fn cross_point(h: &Seg, v: &Seg) -> Option<(i32, i32)> {
    let y = h.0 .1;
    let (x1, x2) = (h.0 .0.min(h.1 .0), h.0 .0.max(h.1 .0));
    let x = v.0 .0;
    let (y1, y2) = (v.0 .1.min(v.1 .1), v.0 .1.max(v.1 .1));
    (x > x1 && x < x2 && y > y1 && y < y2).then_some((x, y))
}

/// Every crossing point between routes of different bundles, each counted
/// once per pair of bundles. Returns `(point, edge_a, edge_b)`.
pub fn crossing_points(graph: &Graph, routes: &[Route]) -> Vec<((i32, i32), usize, usize)> {
    let segs: Vec<Vec<Seg>> = routes.iter().map(|r| r.segments().collect()).collect();
    let mut seen: HashSet<((i32, i32), u32, u32)> = HashSet::new();
    let mut out = Vec::new();
    for a in 0..routes.len() {
        for b in a + 1..routes.len() {
            let (ba, bb) = (bundle_of(graph, a), bundle_of(graph, b));
            if ba == bb {
                continue;
            }
            for sa in &segs[a] {
                for sb in &segs[b] {
                    let p = if horizontal(sa) && vertical(sb) {
                        cross_point(sa, sb)
                    } else if vertical(sa) && horizontal(sb) {
                        cross_point(sb, sa)
                    } else {
                        None
                    };
                    if let Some(p) = p {
                        if seen.insert((p, ba.min(bb), ba.max(bb))) {
                            out.push((p, a, b));
                        }
                    }
                }
            }
        }
    }
    out
}

/// Counts grid points covered by collinear segments of different bundles.
fn shared_lanes(graph: &Graph, routes: &[Route]) -> usize {
    use std::collections::HashMap;
    // (point, is_horizontal) -> first bundle seen, or u32::MAX if several.
    let mut owner: HashMap<((i32, i32), bool), u32> = HashMap::new();
    let mut count = 0;
    for (e, r) in routes.iter().enumerate() {
        let b = bundle_of(graph, e);
        let mut mine: HashSet<((i32, i32), bool)> = HashSet::new();
        for (p, q) in r.segments() {
            let hz = p.1 == q.1;
            let (dx, dy) = ((q.0 - p.0).signum(), (q.1 - p.1).signum());
            let mut c = p;
            // Interior points only; endpoints are bends or ports.
            loop {
                c = (c.0 + dx, c.1 + dy);
                if c == q {
                    break;
                }
                mine.insert((c, hz));
            }
        }
        for key in mine {
            match owner.get(&key) {
                None => {
                    owner.insert(key, b);
                }
                Some(&o) if o != b && o != u32::MAX => {
                    count += 1;
                    owner.insert(key, u32::MAX);
                }
                Some(&o) if o == u32::MAX => count += 1,
                _ => {}
            }
        }
    }
    count
}

/// Measures a finished layout.
pub fn measure(graph: &Graph, nodes: &[(i32, i32)], routes: &[Route]) -> Stats {
    let rects: Vec<Rect> = graph
        .nodes
        .iter()
        .zip(nodes)
        .map(|(n, p)| Rect::new(p.0, p.1, n.w, n.h))
        .collect();
    let mut overlaps = 0;
    for a in 0..rects.len() {
        for b in a + 1..rects.len() {
            if rects[a].too_close(&rects[b], 1) {
                overlaps += 1;
            }
        }
    }
    let upward_violations = graph
        .edges
        .iter()
        .filter(|e| e.kind == EdgeKind::Hierarchy && e.from != e.to)
        .filter(|e| rects[e.from].y < rects[e.to].bottom())
        .count();
    let mut bounds: Option<Rect> = None;
    for r in &rects {
        bounds = Some(bounds.map_or(*r, |b| b.union(r)));
    }
    let area = bounds.map_or(0, |b| b.w as i64 * b.h as i64);
    Stats {
        crossings: crossing_points(graph, routes).len(),
        bends: routes.iter().map(Route::bends).sum(),
        length: routes.iter().map(|r| r.length() as i64).sum(),
        upward_violations,
        overlaps,
        shared_lanes: shared_lanes(graph, routes),
        area,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn graph_with_edges(n: usize) -> Graph {
        let mut g = Graph::default();
        for i in 0..n {
            g.add_node(2, 2);
            if i % 2 == 1 {
                g.add_edge(i - 1, i, EdgeKind::Assoc);
            }
        }
        g
    }

    fn route(points: &[(i32, i32)]) -> Route {
        Route {
            points: points.to_vec(),
        }
    }

    #[test]
    fn counts_single_crossing() {
        let g = graph_with_edges(4);
        let routes = vec![route(&[(0, 5), (10, 5)]), route(&[(5, 0), (5, 10)])];
        assert_eq!(crossing_points(&g, &routes).len(), 1);
    }

    #[test]
    fn touching_endpoints_not_crossing() {
        let g = graph_with_edges(4);
        let routes = vec![route(&[(0, 5), (10, 5)]), route(&[(5, 5), (5, 10)])];
        assert_eq!(crossing_points(&g, &routes).len(), 0);
    }

    #[test]
    fn bends_and_length() {
        let r = route(&[(0, 0), (0, 4), (6, 4), (6, 8)]);
        assert_eq!(r.bends(), 2);
        assert_eq!(r.length(), 14);
    }

    #[test]
    fn upward_violation_detected() {
        let mut g = Graph::default();
        let parent = g.add_node(4, 4);
        let child = g.add_node(4, 4);
        g.add_edge(child, parent, EdgeKind::Hierarchy);
        let good = measure(&g, &[(0, 0), (0, 20)], &[Route::default()]);
        assert_eq!(good.upward_violations, 0);
        let bad = measure(&g, &[(0, 20), (0, 0)], &[Route::default()]);
        assert_eq!(bad.upward_violations, 1);
    }

    #[test]
    fn overlap_and_shared_lane_detected() {
        let g = graph_with_edges(4);
        let s = measure(&g, &[(0, 0), (1, 1), (50, 50), (80, 80)], &[route(&[(0, 5), (10, 5)]), route(&[(2, 5), (8, 5)])]);
        assert_eq!(s.overlaps, 1);
        assert_eq!(s.shared_lanes, 5);
    }
}
