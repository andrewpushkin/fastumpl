//! Places relation labels: multiplicities and roles beside each line end,
//! the relation name beside its longest segment. Each label takes the first
//! candidate spot that overlaps no box, no line and no earlier label.

use crate::graph::*;

fn rect_hits_route(r: &Rect, route: &Route) -> bool {
    route.segments().any(|(p, q)| {
        let seg = Rect::new(p.0.min(q.0), p.1.min(q.1), (p.0 - q.0).abs(), (p.1 - q.1).abs());
        r.x < seg.right() + 1 && seg.x < r.right() + 1 && r.y < seg.bottom() + 1 && seg.y < r.bottom() + 1
    })
}

/// Candidate top-left corners for a label of size `(w, h)` next to the end
/// `p` of a line whose first step goes from `p` towards `q`.
fn end_candidates(p: (i32, i32), q: (i32, i32), (w, h): (i32, i32)) -> Vec<(i32, i32)> {
    let (dx, dy) = ((q.0 - p.0).signum(), (q.1 - p.1).signum());
    if dx == 0 {
        // Vertical line: beside it, a little away from the box.
        let y = if dy > 0 { p.1 + 1 } else { p.1 - 1 - h };
        vec![(p.0 + 1, y), (p.0 - 1 - w, y), (p.0 + 1, y + dy * (h + 1)), (p.0 - 1 - w, y + dy * (h + 1))]
    } else {
        let x = if dx > 0 { p.0 + 1 } else { p.0 - 1 - w };
        vec![(x, p.1 - h), (x, p.1 + 1), (x + dx * (w + 1), p.1 - h), (x + dx * (w + 1), p.1 + 1)]
    }
}

fn middle_candidates(route: &Route, (w, h): (i32, i32)) -> Vec<(i32, i32)> {
    let mut segs: Vec<((i32, i32), (i32, i32))> = route.segments().collect();
    segs.sort_by_key(|(p, q)| std::cmp::Reverse((p.0 - q.0).abs() + (p.1 - q.1).abs()));
    let mut out = Vec::new();
    for (p, q) in segs {
        let mid = ((p.0 + q.0) / 2, (p.1 + q.1) / 2);
        if p.1 == q.1 {
            out.extend([(mid.0 - w / 2, mid.1 - h), (mid.0 - w / 2, mid.1 + 1)]);
        } else {
            out.extend([(mid.0 + 1, mid.1 - h / 2), (mid.0 - 1 - w, mid.1 - h / 2)]);
        }
    }
    out
}

pub(crate) fn place_labels(graph: &Graph, nodes: &[(i32, i32)], routes: &[Route]) -> Vec<[Option<(i32, i32)>; 3]> {
    let boxes: Vec<Rect> = graph
        .nodes
        .iter()
        .zip(nodes)
        .map(|(n, p)| Rect::new(p.0, p.1, n.w, n.h))
        .collect();
    let mut taken: Vec<Rect> = Vec::new();
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
            let free = |c: &(i32, i32)| {
                let r = Rect::new(c.0, c.1, size.0, size.1);
                !boxes.iter().any(|b| b.too_close(&r, 0))
                    && !taken.iter().any(|t| t.too_close(&r, 0))
                    && !routes.iter().any(|other| rect_hits_route(&r, other))
            };
            let pick = cands.iter().copied().find(free).or(cands.first().copied());
            if let Some(p) = pick {
                taken.push(Rect::new(p.0, p.1, size.0, size.1));
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
                assert!(!r.too_close(&Rect::new(n.0, n.1, g.nodes[i].w, g.nodes[i].h), 0), "label {k} on box {i}");
            }
        }
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
