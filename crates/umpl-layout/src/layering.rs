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

/// Returns a layer (0 = top) for each of the `n` items.
pub(crate) fn assign_layers(n: usize, edges: &[FlatEdge], sizes: &[(i32, i32)]) -> Vec<usize> {
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
        for &u in &pred[v] {
            c += 1.0 * (l - layer[u] - 1) as f64;
        }
        for &s in &succ[v] {
            c += 1.0 * (layer[s] - l - 1) as f64;
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

    // 4. Keep layers from growing much wider than a 16:10 drawing.
    let total: f64 = sizes
        .iter()
        .map(|&(w, h)| ((w + NODE_GAP) * (h + LAYER_GAP)) as f64)
        .sum();
    let widest = sizes.iter().map(|s| s.0).max().unwrap_or(0) as f64;
    let target = widest.max((total * 1.6).sqrt() * 1.8);
    for _ in 0..n * 4 {
        let max_layer = *layer.iter().max().unwrap();
        let mut width = vec![0.0f64; max_layer + 1];
        for v in 0..n {
            width[layer[v]] += (sizes[v].0 + NODE_GAP) as f64;
        }
        let Some(k) = (0..=max_layer).find(|&k| width[k] > target && layer.iter().filter(|&&l| l == k).count() > 1)
        else {
            break;
        };
        // Cheapest box to move one layer down without breaking a rule.
        let movable = (0..n)
            .filter(|&v| layer[v] == k)
            // Rows of siblings stay together: only boxes held by
            // associations alone move.
            .filter(|&v| pred[v].is_empty() && succ[v].is_empty())
            .map(|v| (cost(v, k + 1, &layer) - cost(v, k, &layer), v))
            .min_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        match movable {
            Some((_, v)) => layer[v] = k + 1,
            None => break,
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
        FlatEdge { a, b, kind, gap: 0 }
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

    #[test]
    fn wide_star_is_split() {
        let edges: Vec<FlatEdge> = (1..20).map(|i| e(0, i, EdgeKind::Assoc)).collect();
        let l = assign_layers(20, &edges, &[(14, 6); 20]);
        assert!(*l.iter().max().unwrap() >= 1);
    }
}
