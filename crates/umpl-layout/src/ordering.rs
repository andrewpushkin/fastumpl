//! Orders the boxes inside each layer to keep crossings few.
//!
//! Lines that skip layers get a dummy item in every layer they pass, so they
//! take part in the ordering. Barycenter sweeps give a good order quickly;
//! adjacent swaps then remove crossings the sweeps left behind.
//!
//! Positions are compared with ports: a line attaches to an item at a
//! fraction of its width, so the lines into one package box are ordered by
//! where they reach the member inside. Lines that leave the scope being
//! ordered are represented by fixed stand-in items ([`Ghost`]) on the side
//! they leave through, and removed again before coordinates are assigned.

use crate::graph::EdgeKind;
use crate::layering::FlatEdge;
use crate::rng::Rng;

/// Which edge of the scope a line leaves through.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GhostSide {
    Top,
    Bottom,
    Left,
    Right,
}

/// A line from an item of the scope being ordered to something outside
/// it. Its outer end becomes a fixed stand-in item on that side of the
/// scope, so the ordering counts the crossings the line will make.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Ghost {
    pub item: usize,
    pub side: GhostSide,
    /// Orders stand-ins on the same side (the outer end's x).
    pub key: f64,
    /// Where the line attaches along `item`'s width, in `0..1`.
    pub port: f64,
}

/// A link seen from one of its ends.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Adj {
    /// The item at the other end.
    pub other: usize,
    /// Where the link attaches along the other item's width, in `0..1`.
    pub at_other: f64,
    /// Where it attaches along this item's width.
    pub at_self: f64,
}

/// How an item may move inside its layer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pin {
    Free,
    /// Stays left of every free item.
    Left,
    /// Stays right of every free item.
    Right,
    /// In a layer of stand-ins only; never moves.
    Fixed,
}

/// The layered graph with dummy items. Items `0..real` are real boxes.
#[derive(Clone, Debug)]
pub(crate) struct Layered {
    pub real: usize,
    pub layer_of: Vec<usize>,
    pub layers: Vec<Vec<usize>>,
    /// Links between consecutive layers: `(upper, lower, weight)`.
    pub links: Vec<(usize, usize, f64)>,
    /// Ports of each link: `(at upper, at lower)`.
    link_ports: Vec<(f64, f64)>,
    /// Is the link part of an inheritance line (drawn as a shared trunk)?
    link_hier: Vec<bool>,
    /// Edges inside one layer.
    pub same: Vec<(usize, usize)>,
    /// Minimum gap between the ends of labelled same-layer edges.
    pub same_gap: std::collections::HashMap<(usize, usize), i32>,
    up: Vec<Vec<Adj>>,
    down: Vec<Vec<Adj>>,
    same_of: Vec<Vec<usize>>,
    pos: Vec<usize>,
    pin: Vec<Pin>,
    /// Order of pinned items among themselves.
    key: Vec<f64>,
    /// Items from here on are stand-ins and their dummies.
    base_total: usize,
    ghost_top: bool,
    ghost_bottom: bool,
}

/// Adds the link from `u` down to `l`, with a dummy in every layer between.
#[derive(Default)]
struct LinkLists {
    links: Vec<(usize, usize, f64)>,
    ports: Vec<(f64, f64)>,
    hier: Vec<bool>,
}

fn chain(
    layer_of: &mut Vec<usize>,
    out: &mut LinkLists,
    (u, l): (usize, usize),
    kind: EdgeKind,
    (pu, pl): (f64, f64),
) {
    let weight = if kind == EdgeKind::Hierarchy {
        2.0
    } else {
        1.0
    };
    let mut push = |a: usize, b: usize, ports: (f64, f64)| {
        out.links.push((a, b, if a == u { weight } else { 4.0 }));
        out.ports.push(ports);
        out.hier.push(kind == EdgeKind::Hierarchy);
    };
    let mut prev = u;
    for k in layer_of[u] + 1..layer_of[l] {
        let d = layer_of.len();
        layer_of.push(k);
        push(prev, d, (if prev == u { pu } else { 0.5 }, 0.5));
        prev = d;
    }
    push(prev, l, (if prev == u { pu } else { 0.5 }, pl));
}

impl Layered {
    #[cfg(test)]
    pub fn build(real: usize, layer: &[usize], edges: &[FlatEdge]) -> Layered {
        Self::build_with_ghosts(real, layer, edges, &[])
    }

    pub fn build_with_ghosts(
        real: usize,
        layer: &[usize],
        edges: &[FlatEdge],
        ghosts: &[Ghost],
    ) -> Layered {
        let mut layer_of = layer.to_vec();
        let mut ll = LinkLists::default();
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
            let (ends, pts) = if la < lb {
                ((e.a, e.b), e.ports)
            } else {
                ((e.b, e.a), (e.ports.1, e.ports.0))
            };
            chain(&mut layer_of, &mut ll, ends, e.kind, pts);
        }

        // Stand-ins: a layer above and below the scope, or a pinned item at
        // either end of the inner item's own layer.
        let base_total = layer_of.len();
        let ghost_top = ghosts.iter().any(|g| g.side == GhostSide::Top);
        let ghost_bottom = ghosts.iter().any(|g| g.side == GhostSide::Bottom);
        if ghost_top {
            for l in &mut layer_of {
                *l += 1;
            }
        }
        let bottom = layer_of.iter().max().map_or(0, |m| m + 1);
        let mut pinned = Vec::new();
        for g in ghosts {
            let gi = layer_of.len();
            let (l, p) = match g.side {
                GhostSide::Top => (0, Pin::Fixed),
                GhostSide::Bottom => (bottom, Pin::Fixed),
                GhostSide::Left => (layer_of[g.item], Pin::Left),
                GhostSide::Right => (layer_of[g.item], Pin::Right),
            };
            layer_of.push(l);
            pinned.push((gi, p, g.key));
            match g.side {
                GhostSide::Top => chain(
                    &mut layer_of,
                    &mut ll,
                    (gi, g.item),
                    EdgeKind::Assoc,
                    (0.5, g.port),
                ),
                GhostSide::Bottom => chain(
                    &mut layer_of,
                    &mut ll,
                    (g.item, gi),
                    EdgeKind::Assoc,
                    (g.port, 0.5),
                ),
                GhostSide::Left | GhostSide::Right => same.push((g.item, gi)),
            }
        }

        let total = layer_of.len();
        let mut pin = vec![Pin::Free; total];
        let mut key = vec![0.0; total];
        for (gi, p, k) in pinned {
            pin[gi] = p;
            key[gi] = k;
        }
        let n_layers = layer_of.iter().max().map_or(0, |m| m + 1);
        let mut layers = vec![Vec::new(); n_layers];
        for (i, &l) in layer_of.iter().enumerate() {
            layers[l].push(i);
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
            links: ll.links,
            link_ports: ll.ports,
            link_hier: ll.hier,
            same,
            same_gap,
            up: Vec::new(),
            down: Vec::new(),
            same_of,
            pos: vec![0; total],
            pin,
            key,
            base_total,
            ghost_top,
            ghost_bottom,
        };
        lay.rebuild_adjacency();
        lay.repin_all();
        lay
    }

    /// Rebuilds the per-item link lists from `links` and their ports.
    fn rebuild_adjacency(&mut self) {
        let total = self.total();
        let mut up = vec![Vec::new(); total];
        let mut down = vec![Vec::new(); total];
        for (&(u, l, _), &(pu, pl)) in self.links.iter().zip(&self.link_ports) {
            down[u].push(Adj {
                other: l,
                at_other: pl,
                at_self: pu,
            });
            up[l].push(Adj {
                other: u,
                at_other: pu,
                at_self: pl,
            });
        }
        self.up = up;
        self.down = down;
    }

    /// Inheritance lines into a parent are drawn as one trunk: a stem and
    /// a bar over the children. Any other line of the parent (or of a
    /// child) that leaves through that bar crosses it, which links sharing
    /// an end cannot show. So the parent's other downward lines (and a
    /// child's other upward lines) attach towards the side where their far
    /// end lies, given by `x`.
    fn side_ports(&mut self, x: &[f64]) {
        let total = self.total();
        let mut parent = vec![false; total];
        let mut child = vec![false; total];
        for (&(u, l, _), &h) in self.links.iter().zip(&self.link_hier) {
            if h {
                parent[u] = true;
                child[l] = true;
            }
        }
        let side = |from: usize, to: usize| if x[to] >= x[from] { 0.85 } else { 0.15 };
        for li in 0..self.links.len() {
            if self.link_hier[li] {
                continue;
            }
            let (u, l, _) = self.links[li];
            let real = self.real;
            let p = &mut self.link_ports[li];
            if parent[u] && u < real && p.0 == 0.5 {
                p.0 = side(u, l);
            }
            if child[l] && l < real && p.1 == 0.5 {
                p.1 = side(l, u);
            }
        }
        self.rebuild_adjacency();
    }

    /// Removes stand-in items and their lines, leaving the plain layers.
    pub fn strip_ghosts(&mut self) {
        let keep = self.base_total;
        if self.total() == keep {
            return;
        }
        for layer in &mut self.layers {
            layer.retain(|&i| i < keep);
        }
        if self.ghost_bottom {
            self.layers.pop();
        }
        if self.ghost_top {
            self.layers.remove(0);
        }
        self.layer_of.truncate(keep);
        for (k, layer) in self.layers.iter().enumerate() {
            for &i in layer {
                self.layer_of[i] = k;
            }
        }
        let kept: Vec<usize> = (0..self.links.len())
            .filter(|&i| self.links[i].0 < keep && self.links[i].1 < keep)
            .collect();
        self.links = kept.iter().map(|&i| self.links[i]).collect();
        self.link_ports = kept.iter().map(|&i| self.link_ports[i]).collect();
        self.link_hier = kept.iter().map(|&i| self.link_hier[i]).collect();
        self.same.retain(|&(a, b)| a < keep && b < keep);
        self.same_gap.retain(|&(a, b), _| a < keep && b < keep);
        for adj in [&mut self.up, &mut self.down] {
            adj.truncate(keep);
            for v in adj.iter_mut() {
                v.retain(|a| a.other < keep);
            }
        }
        self.same_of.truncate(keep);
        for v in &mut self.same_of {
            v.retain(|&o| o < keep);
        }
        self.pin.truncate(keep);
        self.key.truncate(keep);
        self.pos.truncate(keep);
        self.ghost_top = false;
        self.ghost_bottom = false;
        self.update_pos();
    }

    pub fn total(&self) -> usize {
        self.layer_of.len()
    }

    pub fn is_dummy(&self, i: usize) -> bool {
        i >= self.real
    }

    /// Links to the layer above, as seen from `i`.
    pub fn up_links(&self, i: usize) -> &[Adj] {
        &self.up[i]
    }

    /// Links to the layer below, as seen from `i`.
    pub fn down_links(&self, i: usize) -> &[Adj] {
        &self.down[i]
    }

    pub fn neighbors_up(&self, i: usize) -> impl Iterator<Item = usize> + '_ {
        self.up[i].iter().map(|a| a.other)
    }

    fn update_pos(&mut self) {
        for layer in &self.layers {
            for (p, &i) in layer.iter().enumerate() {
                self.pos[i] = p;
            }
        }
    }

    /// Position of a port on item `i`: its place in the layer, refined by
    /// where along its width the line attaches.
    fn at(&self, i: usize, port: f64) -> f64 {
        self.pos[i] as f64 + port
    }

    /// Puts pinned items of layer `k` back at their side, keeping the order
    /// of the free items.
    fn repin(&mut self, k: usize) {
        let (pin, key) = (&self.pin, &self.key);
        let layer = &mut self.layers[k];
        if layer.iter().all(|&i| pin[i] == Pin::Free) {
            return;
        }
        let rank = |i: usize| match pin[i] {
            Pin::Left | Pin::Fixed => 0,
            Pin::Free => 1,
            Pin::Right => 2,
        };
        layer.sort_by(|&a, &b| {
            rank(a).cmp(&rank(b)).then_with(|| {
                if pin[a] == Pin::Free {
                    std::cmp::Ordering::Equal
                } else {
                    key[a].total_cmp(&key[b])
                }
            })
        });
        for (p, &i) in self.layers[k].iter().enumerate() {
            self.pos[i] = p;
        }
    }

    fn repin_all(&mut self) {
        self.update_pos();
        for k in 0..self.layers.len() {
            self.repin(k);
        }
    }

    /// Crossings between layer `k` and `k + 1`, in O(m log m) for m links
    /// (Barth, Jünger and Mutzel): with the links sorted by their upper
    /// end, two cross exactly when their lower ends are inverted, and the
    /// inversions are counted with a Fenwick tree over the lower ends.
    /// Links sharing an end point do not cross.
    fn crossings_between(&self, k: usize) -> usize {
        let mut pairs: Vec<(f64, f64)> = Vec::new();
        for &u in &self.layers[k] {
            for a in &self.down[u] {
                pairs.push((self.at(u, a.at_self), self.at(a.other, a.at_other)));
            }
        }
        if pairs.len() < 2 {
            return 0;
        }
        // Equal upper ends sort by lower end, so they are never inverted.
        pairs.sort_unstable_by(|x, y| x.0.total_cmp(&y.0).then(x.1.total_cmp(&y.1)));
        let mut lows: Vec<f64> = pairs.iter().map(|p| p.1).collect();
        lows.sort_unstable_by(f64::total_cmp);
        lows.dedup();
        let m = lows.len();
        let mut tree = vec![0usize; m + 1];
        let mut crossings = 0;
        for (seen, p) in pairs.iter().enumerate() {
            let rank = lows.partition_point(|x| *x < p.1) + 1;
            // Earlier links with a lower end at or left of this one.
            let mut not_after = 0;
            let mut j = rank;
            while j > 0 {
                not_after += tree[j];
                j &= j - 1;
            }
            crossings += seen - not_after;
            let mut j = rank;
            while j <= m {
                tree[j] += 1;
                j += j & j.wrapping_neg();
            }
        }
        crossings
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
    fn pair_crossings(&self, u: usize, v: usize, nbrs: &[Vec<Adj>]) -> usize {
        let mut c = 0;
        for a in &nbrs[u] {
            let pa = self.at(a.other, a.at_other);
            for b in &nbrs[v] {
                if pa > self.at(b.other, b.at_other) {
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

    /// Swaps the adjacent items at `p` and `p + 1` of layer `k`.
    fn swap_at(&mut self, k: usize, p: usize) {
        self.layers[k].swap(p, p + 1);
        self.pos[self.layers[k][p]] = p;
        self.pos[self.layers[k][p + 1]] = p + 1;
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
                    let (u, v) = (self.layers[k][p], self.layers[k][p + 1]);
                    if self.pin[u] != Pin::Free || self.pin[v] != Pin::Free {
                        continue;
                    }
                    if self.swap_gain(k, p) < 0 {
                        self.swap_at(k, p);
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
                    for a in nb {
                        sum += self.at(a.other, a.at_other) - 0.5;
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
            self.repin(k);
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
                    .map(|a| a.other)
                    .chain(self.same_of[v].iter().copied())
                    .chain(self.up[v].iter().map(|a| a.other))
                    .collect();
                next.reverse();
                stack.extend(next);
            }
        }
        self.layers = layers;
        self.repin_all();
    }

    /// Orders every layer. `hint_x` gives the current x of real items (used
    /// for the first candidate, so a good diagram changes little); `rng`
    /// shuffles the starting order for the other candidates. A few extra
    /// random restarts run as well, and the best order wins.
    pub fn order(&mut self, hint_x: Option<&[f64]>, rng: Option<&mut Rng>) {
        let mut restart_rng = Rng::new(self.total() as u64 * 31 + self.layers.len() as u64);
        match (hint_x, rng) {
            (Some(hx), _) => {
                // Dummies take the mean x of their chain's real ends.
                let x = self.dummy_hints(hx);
                self.side_ports(&x);
                for layer in &mut self.layers {
                    layer.sort_by(|a, b| x[*a].total_cmp(&x[*b]).then(a.cmp(b)));
                }
                self.repin_all();
            }
            (None, Some(rng)) => {
                restart_rng = Rng::new(rng.next_u64());
                self.dfs_order();
                for layer in &mut self.layers {
                    rng.shuffle(layer);
                }
                self.repin_all();
            }
            (None, None) => self.dfs_order(),
        }
        self.improve_order();
        // Restarts are cheap next to routing; fewer for big diagrams.
        let restarts = match self.total() {
            0..=200 => 8,
            _ => 3,
        };
        let mut best = (self.cost(), self.layers.clone());
        for _ in 0..restarts {
            if best.0 == 0 {
                break;
            }
            for layer in &mut self.layers {
                restart_rng.shuffle(layer);
            }
            self.repin_all();
            self.improve_order();
            let c = self.cost();
            if c < best.0 {
                best = (c, self.layers.clone());
            }
        }
        self.layers = best.1;
        self.update_pos();
    }

    /// Barycenter sweeps, adjacent swaps and sifting from the current order.
    fn improve_order(&mut self) {
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
        for _ in 0..4 {
            if !self.sift() {
                break;
            }
            self.transpose();
        }
    }

    /// Cost that changes when items move inside layer `k`.
    fn local_cost(&self, k: usize) -> usize {
        let mut c = self.same_layer_cost();
        if k > 0 {
            c += self.crossings_between(k - 1);
        }
        if k + 1 < self.layers.len() {
            c += self.crossings_between(k);
        }
        c
    }

    /// Sifting: moves each item to the position in its layer where it
    /// crosses the fewest lines. Escapes local optima that adjacent swaps
    /// cannot. Returns whether anything improved.
    fn sift(&mut self) -> bool {
        let mut improved = false;
        for k in 0..self.layers.len() {
            let len = self.layers[k].len();
            if !(2..=80).contains(&len) {
                continue;
            }
            let items = self.layers[k].clone();
            for u in items {
                if self.pin[u] != Pin::Free {
                    continue;
                }
                let before = self.local_cost(k);
                let old_layer = self.layers[k].clone();
                let mut rest = old_layer.clone();
                rest.retain(|&x| x != u);
                // Crossings between u and each other item, with u left of it
                // (`left`) or right of it (`right`).
                let left: Vec<usize> = rest
                    .iter()
                    .map(|&v| {
                        self.pair_crossings(u, v, &self.up) + self.pair_crossings(u, v, &self.down)
                    })
                    .collect();
                let right: Vec<usize> = rest
                    .iter()
                    .map(|&v| {
                        self.pair_crossings(v, u, &self.up) + self.pair_crossings(v, u, &self.down)
                    })
                    .collect();
                // Inserting u at index p: items before p are left of u.
                let mut cost: usize = left.iter().sum();
                let mut best = (cost, 0);
                for p in 0..rest.len() {
                    cost = cost - left[p] + right[p];
                    if cost < best.0 {
                        best = (cost, p + 1);
                    }
                }
                rest.insert(best.1, u);
                if rest == old_layer {
                    continue;
                }
                self.layers[k] = rest;
                self.update_pos();
                self.repin(k);
                if self.local_cost(k) < before {
                    improved = true;
                } else {
                    self.layers[k] = old_layer;
                    self.update_pos();
                }
            }
        }
        improved
    }

    /// Hint x for every item: real items from `hx`, stand-ins at their key,
    /// dummies at the mean x of their chain's ends.
    fn dummy_hints(&self, hx: &[f64]) -> Vec<f64> {
        let total = self.total();
        let mut x = vec![0.0; total];
        x[..self.real].copy_from_slice(&hx[..self.real]);
        let anchor = |i: usize| i < self.real || self.pin[i] != Pin::Free;
        for i in self.real..total {
            if self.pin[i] != Pin::Free {
                x[i] = self.key[i];
            }
        }
        // Dummies are created top to bottom along each chain, after its
        // upper end, so walking in index order finds the upper end first.
        for i in self.real..total {
            if anchor(i) {
                continue;
            }
            let up = self.up[i].first().map_or(i, |a| a.other);
            let mut low = i;
            while !anchor(low) {
                match self.down[low].first() {
                    Some(a) => low = a.other,
                    None => break,
                }
            }
            x[i] = (x[up] + if anchor(low) { x[low] } else { x[up] }) / 2.0;
        }
        x
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layering::assign_layers;

    fn e(a: usize, b: usize) -> FlatEdge {
        FlatEdge::new(a, b, EdgeKind::Hierarchy, 0)
    }

    fn permutations(items: &[usize]) -> Vec<Vec<usize>> {
        if items.len() <= 1 {
            return vec![items.to_vec()];
        }
        let mut out = Vec::new();
        for i in 0..items.len() {
            let mut rest = items.to_vec();
            let x = rest.remove(i);
            for mut p in permutations(&rest) {
                p.insert(0, x);
                out.push(p);
            }
        }
        out
    }

    /// On small two-layer graphs, the ordering finds the true minimum
    /// number of crossings (checked against every possible order).
    #[test]
    fn finds_minimum_crossings_on_small_graphs() {
        for seed in 0..40 {
            let mut rng = Rng::new(seed);
            let (top, bottom) = (2 + rng.below(3), 2 + rng.below(4));
            let n = top + bottom;
            let mut edges = Vec::new();
            for b in top..n {
                // Every child has one or two parents.
                edges.push(e(b, rng.below(top)));
                if rng.below(2) == 0 {
                    edges.push(e(b, rng.below(top)));
                }
            }
            edges.sort_by_key(|x| (x.a, x.b));
            edges.dedup_by_key(|x| (x.a, x.b));
            let layers: Vec<usize> = (0..n).map(|i| usize::from(i >= top)).collect();
            let mut lay = Layered::build(n, &layers, &edges);
            lay.order(None, Some(&mut Rng::new(seed + 100)));
            let found = lay.cost();

            let mut best = usize::MAX;
            let tops: Vec<usize> = (0..top).collect();
            let bottoms: Vec<usize> = (top..n).collect();
            for pt in permutations(&tops) {
                for pb in permutations(&bottoms) {
                    lay.layers = vec![pt.clone(), pb.clone()];
                    lay.update_pos();
                    best = best.min(lay.cost());
                }
            }
            assert_eq!(found, best, "seed {seed}: found {found}, best {best}");
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

    /// Two lines into one package attach at different points along its
    /// width; their other ends are ordered to match, so they do not cross.
    #[test]
    fn ports_order_lines_into_one_box() {
        // 0 is a package above 1 and 2; 1 attaches at its right, 2 at its left.
        let mut to_right = e(1, 0);
        to_right.ports = (0.5, 0.8);
        let mut to_left = e(2, 0);
        to_left.ports = (0.5, 0.2);
        let mut lay = Layered::build(3, &[0, 1, 1], &[to_right, to_left]);
        assert_eq!(lay.cost(), 1, "index order crosses");
        lay.order(None, None);
        assert_eq!(lay.cost(), 0);
        assert_eq!(lay.layers[1], vec![2, 1]);
    }

    fn ghost(item: usize, side: GhostSide, key: f64) -> Ghost {
        Ghost {
            item,
            side,
            key,
            port: 0.5,
        }
    }

    #[test]
    fn ghost_on_the_right_pulls_its_item_right() {
        for seed in 0..10 {
            let ghosts = [ghost(0, GhostSide::Right, 100.0)];
            let mut lay = Layered::build_with_ghosts(4, &[0; 4], &[], &ghosts);
            lay.order(None, Some(&mut Rng::new(seed)));
            lay.strip_ghosts();
            assert_eq!(lay.layers[0].last(), Some(&0), "seed {seed}");
        }
    }

    #[test]
    fn ghosts_above_order_the_top_layer() {
        // 0 connects to something far right above the scope, 1 far left.
        let ghosts = [
            ghost(0, GhostSide::Top, 100.0),
            ghost(1, GhostSide::Top, 0.0),
        ];
        let mut lay = Layered::build_with_ghosts(2, &[0, 0], &[], &ghosts);
        lay.order(None, None);
        lay.strip_ghosts();
        assert_eq!(lay.layers, vec![vec![1, 0]]);
    }

    #[test]
    fn ghost_lines_reach_deep_items_and_strip_cleanly() {
        // Parent 0, child 1; a line from above the scope goes to the child.
        let ghosts = [ghost(1, GhostSide::Top, 0.0)];
        let mut lay = Layered::build_with_ghosts(2, &[0, 1], &[e(1, 0)], &ghosts);
        assert_eq!(lay.layers.len(), 3, "a stand-in layer on top");
        assert_eq!(lay.total(), 4, "two boxes, a stand-in, a dummy");
        lay.order(None, None);
        lay.strip_ghosts();
        assert_eq!(lay.layers, vec![vec![0], vec![1]]);
        assert_eq!(lay.total(), 2);
        assert_eq!(lay.layer_of, vec![0, 1]);
        assert_eq!(lay.links.len(), 1);
    }

    /// A line from a parent down past its children leaves beside the
    /// inheritance bar, on the side its other end lies, not through it.
    #[test]
    fn own_line_passes_beside_the_inheritance_bar() {
        // 0 is the parent of 1, 2, 3; 4 is a child of 2, below the right
        // half of 0, and also linked to 0 (a dummy in layer 1).
        let mut assoc = e(4, 0);
        assoc.kind = EdgeKind::Assoc;
        let edges = [e(1, 0), e(2, 0), e(3, 0), e(4, 2), assoc];
        let mut lay = Layered::build(5, &[0, 1, 1, 1, 2], &edges);
        let dummy = 5;
        assert!(lay.is_dummy(dummy) && lay.layer_of[dummy] == 1);
        lay.order(Some(&[50.0, 0.0, 50.0, 100.0, 60.0]), None);
        assert_eq!(lay.layers[1].last(), Some(&dummy), "{:?}", lay.layers);
    }

    /// Crossings between two layers counted the slow way: every pair.
    fn crossings_pairwise(lay: &Layered, k: usize) -> usize {
        let mut pairs: Vec<(f64, f64)> = Vec::new();
        for &u in &lay.layers[k] {
            for a in &lay.down[u] {
                pairs.push((lay.at(u, a.at_self), lay.at(a.other, a.at_other)));
            }
        }
        let mut c = 0;
        for i in 0..pairs.len() {
            for j in 0..pairs.len() {
                if pairs[i].0 < pairs[j].0 && pairs[i].1 > pairs[j].1 {
                    c += 1;
                }
            }
        }
        c
    }

    #[test]
    fn crossing_count_matches_pairwise_count() {
        for seed in 0..60 {
            let mut rng = Rng::new(seed);
            let (top, bottom) = (1 + rng.below(8), 1 + rng.below(8));
            let n = top + bottom;
            let ports = [0.2, 0.5, 0.5, 0.8];
            let edges: Vec<FlatEdge> = (0..rng.below(3 * n + 1))
                .map(|_| {
                    let mut f = e(top + rng.below(bottom), rng.below(top));
                    f.ports = (ports[rng.below(4)], ports[rng.below(4)]);
                    f
                })
                .collect();
            let layers: Vec<usize> = (0..n).map(|i| usize::from(i >= top)).collect();
            let mut lay = Layered::build(n, &layers, &edges);
            for layer in &mut lay.layers {
                rng.shuffle(layer);
            }
            lay.update_pos();
            assert_eq!(
                lay.crossings_between(0),
                crossings_pairwise(&lay, 0),
                "seed {seed}"
            );
        }
    }

    #[test]
    fn long_edges_get_dummies() {
        let edges = [e(1, 0), e(2, 1), e(2, 0)];
        let layers = assign_layers(3, &edges, &[(8, 4); 3]);
        let lay = Layered::build(3, &layers, &edges);
        assert_eq!(lay.total(), 4);
    }
}
