//! Orders the boxes inside each layer to keep crossings few.
//!
//! Lines that skip layers get a dummy item in every layer they pass, so they
//! take part in the ordering. Barycenter sweeps give a good order quickly;
//! adjacent swaps then remove crossings the sweeps left behind.

use crate::graph::EdgeKind;
use crate::layering::FlatEdge;
use crate::rng::Rng;

/// The layered graph with dummy items. Items `0..real` are real boxes.
#[derive(Clone, Debug)]
pub(crate) struct Layered {
    pub real: usize,
    pub layer_of: Vec<usize>,
    pub layers: Vec<Vec<usize>>,
    /// Links between consecutive layers: `(upper, lower, weight)`.
    pub links: Vec<(usize, usize, f64)>,
    /// Edges inside one layer.
    pub same: Vec<(usize, usize)>,
    /// Minimum gap between the ends of labelled same-layer edges.
    pub same_gap: std::collections::HashMap<(usize, usize), i32>,
    up: Vec<Vec<usize>>,
    down: Vec<Vec<usize>>,
    same_of: Vec<Vec<usize>>,
    pos: Vec<usize>,
}

impl Layered {
    pub fn build(real: usize, layer: &[usize], edges: &[FlatEdge]) -> Layered {
        let mut layer_of = layer.to_vec();
        let mut links = Vec::new();
        let mut same = Vec::new();
        let mut same_gap = std::collections::HashMap::new();
        for e in edges {
            if e.a == e.b {
                continue;
            }
            let (la, lb) = (layer_of[e.a], layer_of[e.b]);
            if la == lb {
                same.push((e.a, e.b));
                for key in [(e.a, e.b), (e.b, e.a)] {
                    let g = same_gap.entry(key).or_insert(0);
                    *g = (*g).max(e.gap);
                }
                continue;
            }
            let (u, l) = if la < lb { (e.a, e.b) } else { (e.b, e.a) };
            let weight = if e.kind == EdgeKind::Hierarchy {
                2.0
            } else {
                1.0
            };
            let mut prev = u;
            for k in layer_of[u] + 1..layer_of[l] {
                let d = layer_of.len();
                layer_of.push(k);
                links.push((prev, d, if prev == u { weight } else { 4.0 }));
                prev = d;
            }
            links.push((prev, l, if prev == u { weight } else { 4.0 }));
        }
        let total = layer_of.len();
        let n_layers = layer_of.iter().max().map_or(0, |m| m + 1);
        let mut layers = vec![Vec::new(); n_layers];
        for (i, &l) in layer_of.iter().enumerate() {
            layers[l].push(i);
        }
        let mut up = vec![Vec::new(); total];
        let mut down = vec![Vec::new(); total];
        for &(u, l, _) in &links {
            down[u].push(l);
            up[l].push(u);
        }
        let mut same_of = vec![Vec::new(); total];
        for &(a, b) in &same {
            same_of[a].push(b);
            same_of[b].push(a);
        }
        let mut lay = Layered {
            real,
            layer_of,
            layers,
            links,
            same,
            same_gap,
            up,
            down,
            same_of,
            pos: vec![0; total],
        };
        lay.update_pos();
        lay
    }

    pub fn total(&self) -> usize {
        self.layer_of.len()
    }

    pub fn is_dummy(&self, i: usize) -> bool {
        i >= self.real
    }

    pub fn neighbors_up(&self, i: usize) -> &[usize] {
        &self.up[i]
    }

    pub fn neighbors_down(&self, i: usize) -> &[usize] {
        &self.down[i]
    }

    fn update_pos(&mut self) {
        for layer in &self.layers {
            for (p, &i) in layer.iter().enumerate() {
                self.pos[i] = p;
            }
        }
    }

    /// Crossings between layer `k` and `k + 1`.
    fn crossings_between(&self, k: usize) -> usize {
        let mut pairs: Vec<(usize, usize)> = Vec::new();
        for &u in &self.layers[k] {
            for &l in &self.down[u] {
                pairs.push((self.pos[u], self.pos[l]));
            }
        }
        pairs.sort_unstable();
        let mut c = 0;
        for i in 0..pairs.len() {
            for j in i + 1..pairs.len() {
                if pairs[i].0 < pairs[j].0 && pairs[i].1 > pairs[j].1 {
                    c += 1;
                }
            }
        }
        c
    }

    /// Boxes standing between the ends of same-layer edges.
    fn same_layer_cost(&self) -> usize {
        self.same
            .iter()
            .map(|&(a, b)| self.pos[a].abs_diff(self.pos[b]).saturating_sub(1))
            .sum()
    }

    pub fn cost(&self) -> usize {
        (0..self.layers.len().saturating_sub(1))
            .map(|k| self.crossings_between(k))
            .sum::<usize>()
            + self.same_layer_cost()
    }

    /// Crossings among the lines of `u` and `v` (adjacent, `u` left of `v`)
    /// towards `nbrs`.
    fn pair_crossings(&self, u: usize, v: usize, nbrs: &[Vec<usize>]) -> usize {
        let mut c = 0;
        for &a in &nbrs[u] {
            for &b in &nbrs[v] {
                if self.pos[a] > self.pos[b] {
                    c += 1;
                }
            }
        }
        c
    }

    fn same_cost_of(&self, i: usize) -> usize {
        self.same_of[i]
            .iter()
            .map(|&o| self.pos[i].abs_diff(self.pos[o]).saturating_sub(1))
            .sum()
    }

    /// Change in cost if adjacent items at `p` and `p + 1` of layer `k` swap.
    fn swap_gain(&mut self, k: usize, p: usize) -> isize {
        let (u, v) = (self.layers[k][p], self.layers[k][p + 1]);
        let before = self.pair_crossings(u, v, &self.up) + self.pair_crossings(u, v, &self.down);
        let after = self.pair_crossings(v, u, &self.up) + self.pair_crossings(v, u, &self.down);
        let same_before = self.same_cost_of(u) + self.same_cost_of(v);
        self.pos[u] = p + 1;
        self.pos[v] = p;
        let same_after = self.same_cost_of(u) + self.same_cost_of(v);
        self.pos[u] = p;
        self.pos[v] = p + 1;
        (after + same_after) as isize - (before + same_before) as isize
    }

    fn transpose(&mut self) {
        for _ in 0..30 {
            let mut improved = false;
            for k in 0..self.layers.len() {
                for p in 0..self.layers[k].len().saturating_sub(1) {
                    if self.swap_gain(k, p) < 0 {
                        self.layers[k].swap(p, p + 1);
                        let (u, v) = (self.layers[k][p], self.layers[k][p + 1]);
                        self.pos[u] = p;
                        self.pos[v] = p + 1;
                        improved = true;
                    }
                }
            }
            if !improved {
                break;
            }
        }
    }

    fn barycenter_sweep(&mut self, downwards: bool) {
        let n = self.layers.len();
        let ks: Vec<usize> = if downwards {
            (1..n).collect()
        } else {
            (0..n.saturating_sub(1)).rev().collect()
        };
        for k in ks {
            let mut keyed: Vec<(f64, usize, usize)> = self.layers[k]
                .iter()
                .enumerate()
                .map(|(p, &i)| {
                    let nb = if downwards {
                        &self.up[i]
                    } else {
                        &self.down[i]
                    };
                    let mut sum = 0.0;
                    let mut cnt = 0.0;
                    for &o in nb {
                        sum += self.pos[o] as f64;
                        cnt += 1.0;
                    }
                    // Same-layer partners pull too, weakly.
                    for &o in &self.same_of[i] {
                        sum += 0.5 * self.pos[o] as f64;
                        cnt += 0.5;
                    }
                    let key = if cnt > 0.0 { sum / cnt } else { p as f64 };
                    (key, p, i)
                })
                .collect();
            keyed.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
            self.layers[k] = keyed.into_iter().map(|(_, _, i)| i).collect();
            for (p, &i) in self.layers[k].iter().enumerate() {
                self.pos[i] = p;
            }
        }
    }

    /// Initial order: depth-first from the top layer, so connected boxes
    /// start close together.
    fn dfs_order(&mut self) {
        let total = self.total();
        let mut seen = vec![false; total];
        let mut layers: Vec<Vec<usize>> = vec![Vec::new(); self.layers.len()];
        let mut starts: Vec<usize> = (0..total).collect();
        starts.sort_by_key(|&i| (self.layer_of[i], i));
        for s in starts {
            let mut stack = vec![s];
            while let Some(v) = stack.pop() {
                if std::mem::replace(&mut seen[v], true) {
                    continue;
                }
                layers[self.layer_of[v]].push(v);
                let mut next: Vec<usize> = self.down[v]
                    .iter()
                    .chain(&self.same_of[v])
                    .chain(&self.up[v])
                    .copied()
                    .collect();
                next.reverse();
                stack.extend(next);
            }
        }
        self.layers = layers;
        self.update_pos();
    }

    /// Orders every layer. `hint_x` gives the current x of real items (used
    /// for the first candidate, so a good diagram changes little); `rng`
    /// shuffles the starting order for the other candidates.
    pub fn order(&mut self, hint_x: Option<&[f64]>, rng: Option<&mut Rng>) {
        match (hint_x, rng) {
            (Some(hx), _) => {
                // Dummies take the mean x of their chain's real ends.
                let x = self.dummy_hints(hx);
                for layer in &mut self.layers {
                    layer.sort_by(|a, b| x[*a].total_cmp(&x[*b]).then(a.cmp(b)));
                }
                self.update_pos();
            }
            (None, Some(rng)) => {
                self.dfs_order();
                for layer in &mut self.layers {
                    rng.shuffle(layer);
                }
                self.update_pos();
            }
            (None, None) => self.dfs_order(),
        }
        let mut best = self.layers.clone();
        let mut best_cost = self.cost();
        for it in 0..24 {
            if best_cost == 0 {
                break;
            }
            self.barycenter_sweep(it % 2 == 0);
            self.transpose();
            let c = self.cost();
            if c < best_cost {
                best_cost = c;
                best = self.layers.clone();
            }
        }
        self.layers = best;
        self.update_pos();
        self.transpose();
    }

    fn dummy_hints(&self, hx: &[f64]) -> Vec<f64> {
        let total = self.total();
        let mut x = vec![0.0; total];
        x[..self.real].copy_from_slice(&hx[..self.real]);
        // Dummies are created top to bottom along each chain, so walking
        // in index order finds the real upper end first.
        for i in self.real..total {
            let up = self.up[i].first().copied().unwrap_or(i);
            let mut low = i;
            while low >= self.real {
                match self.down[low].first() {
                    Some(&d) => low = d,
                    None => break,
                }
            }
            x[i] = (x[up] + if low < self.real { x[low] } else { x[up] }) / 2.0;
        }
        x
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layering::assign_layers;

    fn e(a: usize, b: usize) -> FlatEdge {
        FlatEdge {
            a,
            b,
            kind: EdgeKind::Hierarchy,
            gap: 0,
        }
    }

    #[test]
    fn untangles_two_crossing_trees() {
        // Parents 0, 1; children 2..6, listed so the naive order crosses.
        let edges = [e(2, 0), e(3, 1), e(4, 0), e(5, 1)];
        let layers = assign_layers(6, &edges, &[(8, 4); 6]);
        let mut lay = Layered::build(6, &layers, &edges);
        lay.order(None, None);
        assert_eq!(lay.cost(), 0);
    }

    #[test]
    fn long_edges_get_dummies() {
        let edges = [e(1, 0), e(2, 1), e(2, 0)];
        let layers = assign_layers(3, &edges, &[(8, 4); 3]);
        let lay = Layered::build(3, &layers, &edges);
        assert_eq!(lay.total(), 4);
    }
}
