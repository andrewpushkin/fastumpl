//! Assigns each box to a layer (row).
//!
//! Inheritance and whole/part edges are hard rules: the parent (or whole)
//! sits in a higher layer. Associations, dependencies and notes only pull
//! their ends towards the same layer, so related classes end up side by
//! side. Layers that grow too wide push loosely bound boxes down.

use crate::graph::{EdgeKind, LAYER_GAP, NODE_GAP};

/// An edge inside one scope. For `Hierarchy`, `a` is the child and `b` the
/// parent; for `Whole`, `a` is the whole and `b` the part.
#[derive(Clone, Copy, Debug)]
pub(crate) struct FlatEdge {
    pub a: usize,
    pub b: usize,
    pub kind: EdgeKind,
    /// Room the edge's labels need when its ends sit side by side.
    pub gap: i32,
    /// Where the line attaches along the width of its `a` and `b` item, as
    /// a fraction in `0..1`. A package item has one port per member line;
    /// a plain box attaches in the middle (0.5).
    pub ports: (f64, f64),
}

impl FlatEdge {
    pub fn new(a: usize, b: usize, kind: EdgeKind, gap: i32) -> FlatEdge {
        FlatEdge {
            a,
            b,
            kind,
            gap,
            ports: (0.5, 0.5),
        }
    }
}

/// The `(upper, lower)` pair an edge asks for, if it is a hard rule.
fn constraint(e: &FlatEdge) -> Option<(usize, usize)> {
    match e.kind {
        EdgeKind::Hierarchy => Some((e.b, e.a)),
        EdgeKind::Whole => Some((e.a, e.b)),
        _ => None,
    }
}

fn soft_weight(kind: EdgeKind) -> f64 {
    match kind {
        EdgeKind::Assoc => 1.0,
        EdgeKind::Note => 1.5,
        EdgeKind::Dep => 0.6,
        _ => 0.0,
    }
}

fn reaches(succ: &[Vec<usize>], from: usize, to: usize) -> bool {
    let mut seen = vec![false; succ.len()];
    let mut stack = vec![from];
    while let Some(v) = stack.pop() {
        if v == to {
            return true;
        }
        if std::mem::replace(&mut seen[v], true) {
            continue;
        }
        stack.extend(succ[v].iter().copied());
    }
    false
}

/// New layers when `v` moves to `to` and everything below it is pushed
/// down just enough to stay below.
fn push_down(succ: &[Vec<usize>], layer: &[usize], v: usize, to: usize) -> Vec<(usize, usize)> {
    let mut new: std::collections::HashMap<usize, usize> = std::collections::HashMap::new();
    new.insert(v, to);
    let mut stack = vec![v];
    while let Some(u) = stack.pop() {
        let lu = new[&u];
        for &s in &succ[u] {
            let ls = new.get(&s).copied().unwrap_or(layer[s]);
            if ls <= lu {
                new.insert(s, lu + 1);
                stack.push(s);
            }
        }
    }
    let mut out: Vec<(usize, usize)> = new.into_iter().collect();
    out.sort_unstable();
    out
}

/// Returns a layer (0 = top) for each of the `n` items.
#[cfg(test)]
pub(crate) fn assign_layers(n: usize, edges: &[FlatEdge], sizes: &[(i32, i32)]) -> Vec<usize> {
    assign_layers_with(n, edges, sizes, crate::place::Target::usual())
}

/// As [`assign_layers`], with layers allowed to grow `target.width` times as wide
/// as usual before boxes are pushed down.
pub(crate) fn assign_layers_with(
    n: usize,
    edges: &[FlatEdge],
    sizes: &[(i32, i32)],
    target: crate::place::Target,
) -> Vec<usize> {
    if n == 0 {
        return Vec::new();
    }
    // 1. Hard rules, inheritance first, skipping any that would make a cycle.
    let mut succ: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut pred: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut hard: Vec<&FlatEdge> = edges.iter().filter(|e| constraint(e).is_some()).collect();
    hard.sort_by_key(|e| e.kind != EdgeKind::Hierarchy);
    for e in hard {
        let (u, l) = constraint(e).unwrap();
        if u == l || succ[u].contains(&l) || reaches(&succ, l, u) {
            continue;
        }
        succ[u].push(l);
        pred[l].push(u);
    }

    // 2. Longest path from the top, in topological order.
    let mut indeg: Vec<usize> = pred.iter().map(Vec::len).collect();
    let mut queue: Vec<usize> = (0..n).filter(|&v| indeg[v] == 0).collect();
    let mut topo = Vec::with_capacity(n);
    let mut layer = vec![0usize; n];
    let mut qi = 0;
    while qi < queue.len() {
        let v = queue[qi];
        qi += 1;
        topo.push(v);
        for &w in &succ[v] {
            layer[w] = layer[w].max(layer[v] + 1);
            indeg[w] -= 1;
            if indeg[w] == 0 {
                queue.push(w);
            }
        }
    }

    let mut soft: Vec<Vec<(usize, f64)>> = vec![Vec::new(); n];
    for e in edges {
        let w = soft_weight(e.kind);
        if w > 0.0 && e.a != e.b {
            soft[e.a].push((e.b, w));
            soft[e.b].push((e.a, w));
        }
    }

    // 3. Relax: move each box to the best layer its hard rules allow.
    let cost = |v: usize, l: usize, layer: &[usize]| -> f64 {
        let mut c = 0.0;
        for &(o, w) in &soft[v] {
            c += w * (l as f64 - layer[o] as f64).abs();
        }
        // Stretched hard edges cost a little; broken ones a lot.
        let stretch = |d: i64| if d < 0 { 100.0 } else { d as f64 };
        for &u in &pred[v] {
            c += stretch(l as i64 - layer[u] as i64 - 1);
        }
        for &s in &succ[v] {
            c += stretch(layer[s] as i64 - l as i64 - 1);
        }
        c + 0.01 * l as f64
    };
    for _ in 0..12 {
        let mut changed = false;
        let max_layer = *layer.iter().max().unwrap();
        for &v in &topo {
            let lo = pred[v].iter().map(|&u| layer[u] + 1).max().unwrap_or(0);
            let hi = succ[v]
                .iter()
                .map(|&s| layer[s] - 1)
                .min()
                .unwrap_or(max_layer + 1);
            let mut best = layer[v];
            let mut best_cost = cost(v, best, &layer);
            for l in lo..=hi {
                let c = cost(v, l, &layer);
                if c + 1e-9 < best_cost {
                    best = l;
                    best_cost = c;
                }
            }
            if best != layer[v] {
                layer[v] = best;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }

    // 4. Keep layers from growing much wider than a drawing of the target
    // shape (16:10 unless the view is another shape).
    let total: f64 = sizes
        .iter()
        .map(|&(w, h)| ((w + NODE_GAP) * (h + LAYER_GAP)) as f64)
        .sum();
    let widest = sizes.iter().map(|s| s.0).max().unwrap_or(0) as f64;
    let fill = target.fill;
    let target = widest.max((total * target.aspect).sqrt() * 1.8 * target.width);
    // Moving a child costs more than moving a box without a parent: its
    // inheritance line stretches and its row of siblings splits. At the
    // top level, which should fill the view, children move as soon as a
    // layer is too wide; inside a package, whose target comes from its own
    // few boxes, only when a layer is twice too wide.
    let child_limit = if fill { target } else { 2.0 * target };
    // Layers where nothing can move; skipped so other layers still balance.
    let mut stuck: std::collections::HashSet<usize> = std::collections::HashSet::new();
    for _ in 0..n * 4 {
        let max_layer = *layer.iter().max().unwrap();
        let mut width = vec![0.0f64; max_layer + 1];
        for v in 0..n {
            width[layer[v]] += (sizes[v].0 + NODE_GAP) as f64;
        }
        let Some(k) = (0..=max_layer).find(|&k| {
            !stuck.contains(&k)
                && width[k] > target
                && layer.iter().filter(|&&l| l == k).count() > 1
        }) else {
            break;
        };
        // Cheapest box to move one layer down. Boxes without a parent move
        // first: association-held boxes alone, roots together with
        // everything below them. Only when none can, children move too: a
        // child need only be below its parent, not right below it, so a
        // long row of siblings folds into several rows (width-bounded
        // layering, as Coffman and Graham's) instead of one row as wide
        // as all of them.
        let pick = |roots: bool| {
            (0..n)
                .filter(|&v| layer[v] == k && pred[v].is_empty() == roots)
                .map(|v| {
                    let moved = push_down(&succ, &layer, v, k + 1);
                    let mut after = layer.clone();
                    for &(u, l) in &moved {
                        after[u] = l;
                    }
                    // Change in cost of every edge touching a moved box.
                    let mut c = 0.0;
                    for &(u, l) in &moved {
                        c += cost(u, l, &after) - cost(u, layer[u], &layer);
                    }
                    // Moving a big subtree is a bigger change.
                    (c + 0.2 * moved.len() as f64, v, moved)
                })
                .min_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)))
        };
        let movable =
            pick(true).or_else(|| (width[k] > child_limit).then(|| pick(false)).flatten());
        match movable {
            Some((_, _, moved)) => {
                for (u, l) in moved {
                    layer[u] = l;
                }
                // Layers below changed; give them another chance.
                stuck.retain(|&l| l <= k);
            }
            None => {
                stuck.insert(k);
            }
        }
    }

    // 5. Remove empty layers.
    let mut used: Vec<usize> = layer.clone();
    used.sort_unstable();
    used.dedup();
    layer
        .iter()
        .map(|l| used.binary_search(l).unwrap())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(a: usize, b: usize, kind: EdgeKind) -> FlatEdge {
        FlatEdge::new(a, b, kind, 0)
    }

    #[test]
    fn parent_above_children_and_associations_side_by_side() {
        // 0 parent, 1 and 2 children, 3 associated with 1.
        let edges = [
            e(1, 0, EdgeKind::Hierarchy),
            e(2, 0, EdgeKind::Hierarchy),
            e(1, 3, EdgeKind::Assoc),
        ];
        let l = assign_layers(4, &edges, &[(10, 4); 4]);
        assert!(l[0] < l[1] && l[0] < l[2]);
        assert_eq!(l[1], l[3]);
    }

    #[test]
    fn cycle_terminates() {
        let edges = [e(0, 1, EdgeKind::Hierarchy), e(1, 0, EdgeKind::Hierarchy)];
        let l = assign_layers(2, &edges, &[(10, 4); 2]);
        assert_ne!(l[0], l[1]);
    }

    /// A narrower width target gives more, shorter layers.
    #[test]
    fn width_factor_sets_how_wide_layers_grow() {
        let edges: Vec<FlatEdge> = (1..30).map(|i| e(0, i, EdgeKind::Assoc)).collect();
        let count = |f: f64| {
            let t = crate::place::Target {
                width: f,
                ..crate::place::Target::usual()
            };
            let l = assign_layers_with(30, &edges, &[(14, 6); 30], t);
            l.iter().max().unwrap() + 1
        };
        assert!(count(0.5) > count(1.0), "{} {}", count(0.5), count(1.0));
        assert!(count(1.0) > count(2.0), "{} {}", count(1.0), count(2.0));
    }

    /// Sixty children of one parent fold into rows instead of one row.
    #[test]
    fn many_siblings_fold_into_rows() {
        let edges: Vec<FlatEdge> = (1..61).map(|i| e(i, 0, EdgeKind::Hierarchy)).collect();
        let l = assign_layers(61, &edges, &[(14, 6); 61]);
        assert_eq!(l[0], 0);
        assert!(l[1..].iter().all(|&x| x >= 1), "children below the parent");
        let rows = *l.iter().max().unwrap();
        assert!(rows >= 3, "{rows} rows of children");
    }

    #[test]
    fn wide_star_is_split() {
        let edges: Vec<FlatEdge> = (1..20).map(|i| e(0, i, EdgeKind::Assoc)).collect();
        let l = assign_layers(20, &edges, &[(14, 6); 20]);
        assert!(*l.iter().max().unwrap() >= 1);
    }
}
