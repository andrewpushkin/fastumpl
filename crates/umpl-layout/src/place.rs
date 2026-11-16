//! The placement pipeline and the public entry points.
//!
//! Groups are laid out innermost first; each finished group then acts as one
//! box in its parent. Inside a scope, each connected part is layered,
//! ordered and given coordinates, and the parts are packed in rows. Several
//! candidate layouts (different starting orders) are routed and scored in
//! parallel; the best one wins, ties going to the lower index.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::coords::assign_coords;
use crate::graph::*;
use crate::labels::place_labels;
use crate::layering::{assign_layers, FlatEdge};
use crate::ordering::Layered;
use crate::rng::Rng;
use crate::router::route_all;
use crate::score::measure;

#[derive(Clone, Debug)]
pub struct Options {
    /// How many candidate layouts compete.
    pub candidates: usize,
    pub seed: u64,
    /// Set to stop early; the best layout found so far is returned.
    pub cancel: Option<Arc<AtomicBool>>,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            candidates: 8,
            seed: 1,
            cancel: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Item {
    Node(usize),
    Group(usize),
}

/// How a candidate picks its starting order.
#[derive(Clone, Copy, Debug)]
enum Start {
    Hint,
    DepthFirst,
    Random(u64),
}

/// The item that stands for node `n` inside `scope`, if `n` is in it.
fn item_in_scope(graph: &Graph, n: usize, scope: Option<usize>) -> Option<Item> {
    let chain = graph.group_chain(graph.nodes[n].group);
    match scope {
        None => Some(chain.last().map_or(Item::Node(n), |&g| Item::Group(g))),
        Some(s) => match chain.iter().position(|&g| g == s) {
            Some(0) => Some(Item::Node(n)),
            Some(p) => Some(Item::Group(chain[p - 1])),
            None => None,
        },
    }
}

fn scope_items(graph: &Graph, scope: Option<usize>) -> Vec<Item> {
    let mut items: Vec<Item> = (0..graph.nodes.len())
        .filter(|&n| graph.nodes[n].group == scope)
        .map(Item::Node)
        .collect();
    items.extend(
        (0..graph.groups.len())
            .filter(|&g| graph.groups[g].parent == scope)
            .map(Item::Group),
    );
    items
}

/// Centre x of every item from current positions, if all have one.
fn hints(graph: &Graph, items: &[Item]) -> Option<Vec<f64>> {
    items
        .iter()
        .map(|it| match *it {
            Item::Node(n) => graph.nodes[n].pos.map(|p| p.0 as f64 + graph.nodes[n].w as f64 / 2.0),
            Item::Group(g) => {
                let xs: Vec<f64> = (0..graph.nodes.len())
                    .filter(|&n| graph.nodes[n].group.is_some_and(|ng| graph.group_within(ng, g)))
                    .filter_map(|n| graph.nodes[n].pos.map(|p| p.0 as f64 + graph.nodes[n].w as f64 / 2.0))
                    .collect();
                (!xs.is_empty()).then(|| xs.iter().sum::<f64>() / xs.len() as f64)
            }
        })
        .collect()
}

fn find(parent: &mut [usize], x: usize) -> usize {
    let mut r = x;
    while parent[r] != r {
        r = parent[r];
    }
    let mut c = x;
    while parent[c] != r {
        let next = parent[c];
        parent[c] = r;
        c = next;
    }
    r
}

/// Lays out one scope; returns item top-left corners, normalised to (0, 0).
fn layout_scope(sizes: &[(i32, i32)], edges: &[FlatEdge], hint: Option<&[f64]>, start: Start) -> Vec<(i32, i32)> {
    let n = sizes.len();
    let mut parent: Vec<usize> = (0..n).collect();
    for e in edges {
        let (a, b) = (find(&mut parent, e.a), find(&mut parent, e.b));
        if a != b {
            parent[a.max(b)] = a.min(b);
        }
    }
    let mut comps: Vec<Vec<usize>> = Vec::new();
    let mut comp_of = vec![usize::MAX; n];
    for i in 0..n {
        let r = find(&mut parent, i);
        if comp_of[r] == usize::MAX {
            comp_of[r] = comps.len();
            comps.push(Vec::new());
        }
        comps[comp_of[r]].push(i);
    }

    let mut placed: Vec<(Vec<usize>, Vec<(i32, i32)>, Rect)> = Vec::new();
    for (ci, comp) in comps.iter().enumerate() {
        let mut local = vec![usize::MAX; n];
        for (li, &i) in comp.iter().enumerate() {
            local[i] = li;
        }
        let sub_edges: Vec<FlatEdge> = edges
            .iter()
            .filter(|e| local[e.a] != usize::MAX)
            .map(|e| FlatEdge {
                a: local[e.a],
                b: local[e.b],
                kind: e.kind,
            })
            .collect();
        let sub_sizes: Vec<(i32, i32)> = comp.iter().map(|&i| sizes[i]).collect();
        let layers = assign_layers(comp.len(), &sub_edges, &sub_sizes);
        let mut lay = Layered::build(comp.len(), &layers, &sub_edges);
        let sub_hint: Option<Vec<f64>> = hint.map(|h| comp.iter().map(|&i| h[i]).collect());
        match start {
            Start::Hint => lay.order(sub_hint.as_deref(), None),
            Start::DepthFirst => lay.order(None, None),
            Start::Random(seed) => {
                let mut rng = Rng::new(seed.wrapping_add(ci as u64 * 7919));
                lay.order(None, Some(&mut rng))
            }
        }
        let mut widths: Vec<i32> = sub_sizes.iter().map(|s| s.0).collect();
        let mut heights: Vec<i32> = sub_sizes.iter().map(|s| s.1).collect();
        widths.resize(lay.total(), 0);
        heights.resize(lay.total(), 0);
        let pos = assign_coords(&lay, &widths, &heights);
        let mut bounds: Option<Rect> = None;
        for (li, p) in pos.iter().enumerate() {
            let r = Rect::new(p.0, p.1, sub_sizes[li].0, sub_sizes[li].1);
            bounds = Some(bounds.map_or(r, |b| b.union(&r)));
        }
        placed.push((comp.clone(), pos, bounds.unwrap_or_default()));
    }

    // Pack the parts in rows, biggest first, trying several row widths and
    // keeping the packing closest to a 16:10 drawing.
    let mut order: Vec<usize> = (0..placed.len()).collect();
    order.sort_by_key(|&i| (std::cmp::Reverse(placed[i].2.w as i64 * placed[i].2.h as i64), placed[i].0[0]));
    let rects: Vec<Rect> = order.iter().map(|&i| placed[i].2).collect();
    let offsets = best_packing(&rects);
    let mut out = vec![(0, 0); n];
    for (k, &i) in order.iter().enumerate() {
        let (comp, pos, b) = &placed[i];
        let (x, y) = offsets[k];
        for (li, &item) in comp.iter().enumerate() {
            out[item] = (pos[li].0 - b.x + x, pos[li].1 - b.y + y);
        }
    }
    out
}

/// Shelf packing of `rects` (in order) into rows no wider than `row_width`.
fn pack_rows(rects: &[Rect], row_width: i32) -> (Vec<(i32, i32)>, i32, i32) {
    let gap = 2 * NODE_GAP;
    let (mut x, mut y, mut row_h, mut width) = (0, 0, 0, 0);
    let mut out = Vec::with_capacity(rects.len());
    for b in rects {
        if x > 0 && x + b.w > row_width {
            x = 0;
            y += row_h + LAYER_GAP;
            row_h = 0;
        }
        out.push((x, y));
        width = width.max(x + b.w);
        x += b.w + gap;
        row_h = row_h.max(b.h);
    }
    (out, width, y + row_h)
}

fn best_packing(rects: &[Rect]) -> Vec<(i32, i32)> {
    let widest = rects.iter().map(|r| r.w).max().unwrap_or(0);
    let all: i32 = rects.iter().map(|r| r.w + 2 * NODE_GAP).sum();
    let mut best: Option<(f64, Vec<(i32, i32)>)> = None;
    for k in 0..=12 {
        let row_width = widest + (all - widest) * k / 12;
        let (offsets, w, h) = pack_rows(rects, row_width);
        let (w, h) = (w.max(1) as f64, h.max(1) as f64);
        let cost = w * h * (1.0 + ((w / h) / 1.6).ln().abs());
        if best.as_ref().is_none_or(|(c, _)| cost < *c - 1e-9) {
            best = Some((cost, offsets));
        }
    }
    best.map(|b| b.1).unwrap_or_default()
}

/// Computes node positions for one candidate.
fn place_all(graph: &Graph, start: Start) -> Vec<(i32, i32)> {
    let ng = graph.groups.len();
    // Deepest groups first, root last.
    let mut scopes: Vec<Option<usize>> = (0..ng).map(Some).collect();
    scopes.sort_by_key(|g| std::cmp::Reverse(graph.group_chain(*g).len()));
    scopes.push(None);

    let mut group_size = vec![(0, 0); ng];
    // Per scope: its items and their relative positions.
    let mut rel: Vec<(Option<usize>, Vec<Item>, Vec<(i32, i32)>)> = Vec::new();
    for scope in scopes {
        let items = scope_items(graph, scope);
        let sizes: Vec<(i32, i32)> = items
            .iter()
            .map(|it| match *it {
                Item::Node(n) => (graph.nodes[n].w, graph.nodes[n].h),
                Item::Group(g) => group_size[g],
            })
            .collect();
        let index = |it: Item| items.iter().position(|&x| x == it);
        let mut edges = Vec::new();
        for e in &graph.edges {
            let (Some(a), Some(b)) = (item_in_scope(graph, e.from, scope), item_in_scope(graph, e.to, scope)) else {
                continue;
            };
            if a == b {
                continue;
            }
            edges.push(FlatEdge {
                a: index(a).unwrap(),
                b: index(b).unwrap(),
                kind: e.kind,
            });
        }
        let hint = hints(graph, &items);
        let start = match (start, &hint) {
            (Start::Hint, None) => Start::DepthFirst,
            (s, _) => s,
        };
        let pos = layout_scope(&sizes, &edges, hint.as_deref(), start);
        if let Some(g) = scope {
            let mut b: Option<Rect> = None;
            for (i, p) in pos.iter().enumerate() {
                let r = Rect::new(p.0, p.1, sizes[i].0, sizes[i].1);
                b = Some(b.map_or(r, |x| x.union(&r)));
            }
            let b = b.unwrap_or_default();
            group_size[g] = (
                (b.w + 2 * GROUP_PAD).max(graph.groups[g].min_w),
                b.h + 2 * GROUP_PAD + graph.groups[g].header,
            );
        }
        rel.push((scope, items, pos));
    }

    // Top-down: absolute positions.
    let mut nodes = vec![(0, 0); graph.nodes.len()];
    let mut origin: Vec<Option<(i32, i32)>> = vec![None; ng];
    for (scope, items, pos) in rel.iter().rev() {
        let base = match scope {
            None => (0, 0),
            Some(g) => {
                let o = origin[*g].unwrap_or((0, 0));
                (o.0 + GROUP_PAD, o.1 + GROUP_PAD + graph.groups[*g].header)
            }
        };
        for (it, p) in items.iter().zip(pos) {
            let abs = (base.0 + p.0, base.1 + p.1);
            match *it {
                Item::Node(n) => nodes[n] = abs,
                Item::Group(g) => origin[g] = Some(abs),
            }
        }
    }
    nodes
}

/// Routes, labels and measures a layout with nodes at `nodes`.
pub(crate) fn finalize(graph: &Graph, nodes: Vec<(i32, i32)>) -> Layout {
    let routes = route_all(graph, &nodes);
    let groups = group_rects(graph, &nodes);
    let labels = place_labels(graph, &nodes, &routes);
    let stats = measure(graph, &nodes, &routes);
    Layout {
        nodes,
        routes,
        groups,
        labels,
        stats,
    }
}

/// Lays out the whole diagram (the Tidy command). Current positions, when
/// every node has one, seed the first candidate so a good diagram changes
/// little.
pub fn layout_all(graph: &Graph, opts: &Options) -> Layout {
    if graph.nodes.is_empty() {
        return Layout::default();
    }
    let mut starts = Vec::new();
    if graph.nodes.iter().all(|n| n.pos.is_some()) {
        starts.push(Start::Hint);
    }
    starts.push(Start::DepthFirst);
    let mut k = 1;
    while starts.len() < opts.candidates.max(1) {
        starts.push(Start::Random(opts.seed.wrapping_add(k)));
        k += 1;
    }
    let cancelled = || opts.cancel.as_ref().is_some_and(|c| c.load(Ordering::Relaxed));
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get()).min(starts.len());

    let mut results: Vec<Option<Layout>> = vec![None; starts.len()];
    std::thread::scope(|s| {
        let chunks: Vec<Vec<usize>> = (0..threads)
            .map(|t| (t..starts.len()).step_by(threads).collect())
            .collect();
        let handles: Vec<_> = chunks
            .into_iter()
            .map(|idx| {
                let starts = &starts;
                let cancelled = &cancelled;
                s.spawn(move || {
                    idx.into_iter()
                        .filter_map(|i| {
                            // The first candidate always runs.
                            if i > 0 && cancelled() {
                                return None;
                            }
                            Some((i, finalize(graph, place_all(graph, starts[i]))))
                        })
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        for h in handles {
            for (i, l) in h.join().expect("layout thread panicked") {
                results[i] = Some(l);
            }
        }
    });
    results
        .into_iter()
        .flatten()
        .enumerate()
        .min_by(|(i, a), (j, b)| a.stats.score().total_cmp(&b.stats.score()).then(i.cmp(j)))
        .map(|(_, l)| l)
        .unwrap()
}

/// Do segments `p1-p2` and `q1-q2` cross properly?
fn segments_cross(p1: (f64, f64), p2: (f64, f64), q1: (f64, f64), q2: (f64, f64)) -> bool {
    let orient = |a: (f64, f64), b: (f64, f64), c: (f64, f64)| (b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0);
    let d1 = orient(q1, q2, p1);
    let d2 = orient(q1, q2, p2);
    let d3 = orient(p1, p2, q1);
    let d4 = orient(p1, p2, q2);
    d1 * d2 < 0.0 && d3 * d4 < 0.0
}

/// Places node `i` near its placed neighbours, where it overlaps nothing
/// and adds the fewest crossings.
fn place_one(graph: &Graph, nodes: &mut [Option<(i32, i32)>], i: usize) {
    let (w, h) = (graph.nodes[i].w, graph.nodes[i].h);
    let rect_of = |n: usize, p: (i32, i32)| Rect::new(p.0, p.1, graph.nodes[n].w, graph.nodes[n].h);
    let placed: Vec<(usize, Rect)> = (0..graph.nodes.len())
        .filter_map(|n| nodes[n].map(|p| (n, rect_of(n, p))))
        .collect();
    let center = |r: &Rect| (r.x as f64 + r.w as f64 / 2.0, r.y as f64 + r.h as f64 / 2.0);
    let rect_by_node = |n: usize| placed.iter().find(|(m, _)| *m == n).map(|(_, r)| *r);

    // Neighbours already placed.
    let mut nbrs: Vec<(Rect, Option<bool>)> = Vec::new(); // Some(true): i is the child
    for e in &graph.edges {
        let (other, i_child) = if e.from == i && e.to != i {
            (e.to, true)
        } else if e.to == i && e.from != i {
            (e.from, false)
        } else {
            continue;
        };
        if let Some(r) = rect_by_node(other) {
            let rule = (e.kind == EdgeKind::Hierarchy).then_some(i_child);
            nbrs.push((r, rule));
        }
    }
    let mates: Vec<Rect> = placed
        .iter()
        .filter(|(n, _)| graph.nodes[*n].group.is_some() && graph.nodes[*n].group == graph.nodes[i].group)
        .map(|(_, r)| *r)
        .collect();
    let bounds = placed.iter().map(|(_, r)| *r).reduce(|a, b| a.union(&b));
    let anchor = if !nbrs.is_empty() {
        let s = nbrs.iter().fold((0.0, 0.0), |acc, (r, _)| (acc.0 + center(r).0, acc.1 + center(r).1));
        (s.0 / nbrs.len() as f64, s.1 / nbrs.len() as f64)
    } else if !mates.is_empty() {
        let s = mates.iter().fold((0.0, 0.0), |acc, r| (acc.0 + center(r).0, acc.1 + center(r).1));
        (s.0 / mates.len() as f64, s.1 / mates.len() as f64)
    } else if let Some(b) = bounds {
        ((b.right() + 2 * NODE_GAP) as f64 + w as f64 / 2.0, b.y as f64 + h as f64 / 2.0)
    } else {
        (w as f64 / 2.0, h as f64 / 2.0)
    };

    // Existing lines, approximated by straight segments between centres.
    let lines: Vec<((f64, f64), (f64, f64))> = graph
        .edges
        .iter()
        .filter_map(|e| Some((center(&rect_by_node(e.from)?), center(&rect_by_node(e.to)?))))
        .collect();
    // Groups this node is not part of must stay clear.
    let current: Vec<(i32, i32)> = nodes.iter().map(|p| p.unwrap_or((i32::MIN / 4, i32::MIN / 4))).collect();
    let group_rects_now = group_rects(graph, &current);
    let foreign: Vec<Rect> = (0..graph.groups.len())
        .filter(|&g| !graph.nodes[i].group.is_some_and(|ng| graph.group_within(ng, g)))
        .filter(|&g| (0..graph.nodes.len()).any(|n| nodes[n].is_some() && graph.nodes[n].group.is_some_and(|ng| graph.group_within(ng, g))))
        .map(|g| group_rects_now[g])
        .collect();

    let ax = (anchor.0 - w as f64 / 2.0).round() as i32;
    let ay = (anchor.1 - h as f64 / 2.0).round() as i32;
    let mut best: Option<(f64, (i32, i32))> = None;
    let mut found_ring = None;
    for ring in 0..200 {
        if let Some(r0) = found_ring {
            if ring > r0 + 8 {
                break;
            }
        }
        let r = ring * 2;
        let mut cands = Vec::new();
        if r == 0 {
            cands.push((ax, ay));
        } else {
            for t in -r..=r {
                if t % 2 != 0 {
                    continue;
                }
                cands.extend([(ax + t, ay - r), (ax + t, ay + r), (ax - r, ay + t), (ax + r, ay + t)]);
            }
        }
        for c in cands {
            let rect = Rect::new(c.0, c.1, w, h);
            if placed.iter().any(|(_, o)| rect.too_close(o, NODE_GAP)) {
                continue;
            }
            if foreign.iter().any(|g| rect.too_close(g, 2)) {
                continue;
            }
            let cc = center(&rect);
            let mut cost = ((cc.0 - anchor.0).abs() + (cc.1 - anchor.1).abs()) * 0.2;
            for (nr, rule) in &nbrs {
                let nc = center(nr);
                cost += 10.0 * lines.iter().filter(|(a, b)| segments_cross(cc, nc, *a, *b)).count() as f64;
                match rule {
                    Some(true) if rect.y < nr.bottom() + 2 => cost += 20.0,
                    Some(false) if rect.bottom() + 2 > nr.y => cost += 20.0,
                    _ => {}
                }
            }
            if best.as_ref().is_none_or(|(b, _)| cost < *b) {
                best = Some((cost, c));
            }
            found_ring.get_or_insert(ring);
        }
    }
    nodes[i] = Some(best.map_or((ax, ay), |b| b.1));
}

/// Keeps placed nodes where they are and places nodes without a position.
pub fn place_new(graph: &Graph) -> Layout {
    if graph.nodes.iter().all(|n| n.pos.is_none()) {
        return layout_all(graph, &Options::default());
    }
    let mut nodes: Vec<Option<(i32, i32)>> = graph.nodes.iter().map(|n| n.pos).collect();
    for i in 0..graph.nodes.len() {
        if nodes[i].is_none() {
            place_one(graph, &mut nodes, i);
        }
    }
    finalize(graph, nodes.into_iter().map(Option::unwrap).collect())
}

/// Keeps every node where it is (placing any without a position) and
/// routes the lines.
pub fn route_only(graph: &Graph) -> Layout {
    if graph.nodes.iter().any(|n| n.pos.is_none()) {
        return place_new(graph);
    }
    finalize(graph, graph.nodes.iter().map(|n| n.pos.unwrap()).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::score::crossing_points;

    fn random_graph(seed: u64, n: usize, m: usize) -> Graph {
        let mut rng = Rng::new(seed);
        let mut g = Graph::default();
        for _ in 0..n {
            g.add_node(8 + rng.below(10) as i32, 4 + rng.below(6) as i32);
        }
        let kinds = [EdgeKind::Hierarchy, EdgeKind::Assoc, EdgeKind::Whole, EdgeKind::Dep];
        for _ in 0..m {
            let a = rng.below(n);
            let b = rng.below(n);
            g.add_edge(a, b, kinds[rng.below(kinds.len())]);
        }
        g
    }

    fn fast() -> Options {
        Options {
            candidates: 3,
            ..Options::default()
        }
    }

    #[test]
    fn parent_above_child() {
        let mut g = Graph::default();
        let p = g.add_node(10, 4);
        let c = g.add_node(10, 4);
        g.add_edge(c, p, EdgeKind::Hierarchy);
        let l = layout_all(&g, &fast());
        assert!(l.nodes[c].1 >= l.nodes[p].1 + 4 + LAYER_GAP);
        assert_eq!(l.stats.upward_violations, 0);
    }

    #[test]
    fn no_overlaps_random_graphs() {
        for seed in 0..30 {
            let g = random_graph(seed, 3 + seed as usize % 12, seed as usize % 18);
            let l = layout_all(&g, &fast());
            assert_eq!(l.stats.overlaps, 0, "seed {seed}");
            assert_eq!(l.routes.len(), g.edges.len());
            for r in &l.routes {
                assert!(r.points.len() >= 2, "seed {seed}: {r:?}");
                for (p, q) in r.segments() {
                    assert!(p.0 == q.0 || p.1 == q.1, "not orthogonal, seed {seed}");
                }
            }
        }
    }

    #[test]
    fn cyclic_inheritance_terminates() {
        let mut g = Graph::default();
        let a = g.add_node(10, 4);
        let b = g.add_node(10, 4);
        g.add_edge(a, b, EdgeKind::Hierarchy);
        g.add_edge(b, a, EdgeKind::Hierarchy);
        let l = layout_all(&g, &fast());
        assert_eq!(l.stats.overlaps, 0);
    }

    #[test]
    fn tree_has_no_crossings() {
        // A root with three children, each with two children.
        let mut g = Graph::default();
        let root = g.add_node(12, 5);
        for _ in 0..3 {
            let c = g.add_node(12, 5);
            g.add_edge(c, root, EdgeKind::Hierarchy);
            for _ in 0..2 {
                let gc = g.add_node(12, 5);
                g.add_edge(gc, c, EdgeKind::Hierarchy);
            }
        }
        let l = layout_all(&g, &Options::default());
        assert_eq!(crossing_points(&g, &l.routes).len(), 0);
        assert_eq!(l.stats.upward_violations, 0);
    }

    #[test]
    fn groups_contain_members() {
        let mut g = Graph::default();
        g.groups.push(Group {
            parent: None,
            header: 3,
            min_w: 10,
        });
        let a = g.add_node(10, 4);
        let b = g.add_node(10, 4);
        let c = g.add_node(10, 4);
        g.nodes[a].group = Some(0);
        g.nodes[b].group = Some(0);
        g.add_edge(a, b, EdgeKind::Assoc);
        g.add_edge(c, a, EdgeKind::Hierarchy);
        let l = layout_all(&g, &fast());
        let gr = l.groups[0];
        for n in [a, b] {
            let r = l.node_rect(&g, n);
            assert!(gr.contains((r.x, r.y)) && gr.contains((r.right(), r.bottom())));
        }
        let rc = l.node_rect(&g, c);
        assert!(!gr.too_close(&rc, 0), "outsider inside group");
    }

    #[test]
    fn place_new_moves_only_new() {
        let mut g = Graph::default();
        let a = g.add_node(10, 4);
        let b = g.add_node(10, 4);
        let c = g.add_node(10, 4);
        g.nodes[a].pos = Some((0, 0));
        g.nodes[b].pos = Some((30, 0));
        g.add_edge(c, a, EdgeKind::Hierarchy);
        let l = place_new(&g);
        assert_eq!(l.nodes[a], (0, 0));
        assert_eq!(l.nodes[b], (30, 0));
        assert_eq!(l.stats.overlaps, 0);
        assert!(l.nodes[c].1 >= 4 + 2, "child below parent: {:?}", l.nodes[c]);
    }

    #[test]
    fn same_input_same_output() {
        let g = random_graph(7, 12, 16);
        assert_eq!(layout_all(&g, &fast()), layout_all(&g, &fast()));
    }

    #[test]
    fn hint_candidate_keeps_good_layout() {
        let mut g = random_graph(3, 8, 9);
        let first = layout_all(&g, &Options::default());
        for (n, p) in first.nodes.iter().enumerate() {
            g.nodes[n].pos = Some(*p);
        }
        let again = layout_all(&g, &Options::default());
        assert!(again.stats.score() <= first.stats.score() + 1e-9);
    }
}
