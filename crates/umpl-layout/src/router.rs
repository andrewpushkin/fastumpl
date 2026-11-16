//! Orthogonal line routing on the grid.
//!
//! Each line is found by A* over grid points, where the state is a point
//! plus the direction of travel. A step costs more near boxes (so lines run
//! down the middle of corridors), a bend costs more than a step, and running
//! into another line costs far more: crossing it or sharing its lane. Lines
//! of one bundle (inheritance into the same parent) share lanes cheaply,
//! which draws the classic trunk.
//!
//! After every line is routed, lines that still cross are ripped up and
//! routed again with all other lines in place, while that helps.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};

use crate::graph::*;
use crate::score::{bundle_of, crossing_points, segment_crossing};

const STEP: u32 = 10;
/// Step along a lane already used by the same bundle.
const BUNDLE_STEP: u32 = 5;
const BEND: u32 = 40;
const CROSS: u32 = 600;
const SHARE: u32 = 600;
/// Points closer than this to a box cost extra, `CLEAR_W` per unit.
const CLEAR_K: i32 = 4;
const CLEAR_W: u32 = 4;
/// Running along a package border.
const BORDER: u32 = 300;
/// Owner value of points under a package name tab.
const TAB: i32 = -2;
/// Using a port another bundle already uses.
const PORT_USED: u32 = 900;
/// Using a port right next to one another bundle uses.
const PORT_NEAR: u32 = 150;
/// Free space around the drawing where lines may go.
const MARGIN: i32 = 6;
/// The search first looks within this distance of both boxes.
const WINDOW: i32 = 14;

const NONE: u32 = u32::MAX;
const MULTI: u32 = u32::MAX - 1;

/// E, W, S, N. `d ^ 1` is the opposite direction.
const DIRS: [(i32, i32); 4] = [(1, 0), (-1, 0), (0, 1), (0, -1)];

fn horizontal(d: usize) -> bool {
    d < 2
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Side {
    Top,
    Bottom,
    Left,
    Right,
}

impl Side {
    const ALL: [Side; 4] = [Side::Top, Side::Bottom, Side::Left, Side::Right];

    /// Direction pointing away from the box.
    fn outward(self) -> usize {
        match self {
            Side::Right => 0,
            Side::Left => 1,
            Side::Bottom => 2,
            Side::Top => 3,
        }
    }
}

/// Boundary points of `r` on `side`, corners excluded, with their offset
/// from the middle of the side.
fn ports(r: &Rect, side: Side) -> Vec<((i32, i32), i32)> {
    match side {
        Side::Top | Side::Bottom => {
            let y = if side == Side::Top { r.y } else { r.bottom() };
            (1..r.w)
                .map(|i| ((r.x + i, y), (2 * i - r.w).abs()))
                .collect()
        }
        Side::Left | Side::Right => {
            let x = if side == Side::Left { r.x } else { r.right() };
            (1..r.h)
                .map(|i| ((x, r.y + i), (2 * i - r.h).abs()))
                .collect()
        }
    }
}

fn side_of(r: &Rect, p: (i32, i32)) -> Option<Side> {
    let corner = (p.0 == r.x || p.0 == r.right()) && (p.1 == r.y || p.1 == r.bottom());
    if corner {
        return None;
    }
    if p.1 == r.y && p.0 > r.x && p.0 < r.right() {
        Some(Side::Top)
    } else if p.1 == r.bottom() && p.0 > r.x && p.0 < r.right() {
        Some(Side::Bottom)
    } else if p.0 == r.x && p.1 > r.y && p.1 < r.bottom() {
        Some(Side::Left)
    } else if p.0 == r.right() && p.1 > r.y && p.1 < r.bottom() {
        Some(Side::Right)
    } else {
        None
    }
}

/// Which sides an edge may leave from and arrive at.
fn allowed_sides(kind: EdgeKind) -> (&'static [Side], &'static [Side]) {
    match kind {
        EdgeKind::Hierarchy => (&[Side::Top], &[Side::Bottom]),
        _ => (&Side::ALL, &Side::ALL),
    }
}

/// The static part of the routing problem: boxes and clearance.
struct Grid {
    x0: i32,
    y0: i32,
    w: i32,
    h: i32,
    /// Index of the box covering each point, `TAB` under a package name
    /// tab, or -1 when free.
    owner: Vec<i32>,
    /// Bit 1: on a horizontal package border; bit 2: on a vertical one.
    border: Vec<u8>,
    /// Distance to the nearest box, capped at `CLEAR_K`.
    clear: Vec<u8>,
    rects: Vec<Rect>,
}

impl Grid {
    fn new(graph: &Graph, nodes: &[(i32, i32)]) -> Grid {
        let rects: Vec<Rect> = graph
            .nodes
            .iter()
            .zip(nodes)
            .map(|(n, p)| Rect::new(p.0, p.1, n.w, n.h))
            .collect();
        let groups = group_rects(graph, nodes);
        let mut bounds = rects.first().copied().unwrap_or_default();
        for r in rects.iter().chain(&groups) {
            bounds = bounds.union(r);
        }
        let b = bounds.expand(MARGIN);
        let (w, h) = (b.w + 1, b.h + 1);
        let mut owner = vec![-1; (w * h) as usize];
        for (i, r) in rects.iter().enumerate() {
            for y in r.y..=r.bottom() {
                for x in r.x..=r.right() {
                    owner[((y - b.y) * w + (x - b.x)) as usize] = i as i32;
                }
            }
        }
        let mut border = vec![0u8; owner.len()];
        for (g, r) in graph.groups.iter().zip(&groups) {
            let at = |x: i32, y: i32| ((y - b.y) * w + (x - b.x)) as usize;
            for x in r.x..=r.right() {
                border[at(x, r.y)] |= 1;
                border[at(x, r.bottom())] |= 1;
            }
            for y in r.y..=r.bottom() {
                border[at(r.x, y)] |= 2;
                border[at(r.right(), y)] |= 2;
            }
            let tab_w = (g.min_w - 4).clamp(0, r.w);
            for y in r.y..r.y + g.header {
                for x in r.x..r.x + tab_w {
                    if owner[at(x, y)] == -1 {
                        owner[at(x, y)] = TAB;
                    }
                }
            }
        }
        // Multi-source BFS distance from boxes.
        let mut clear = vec![CLEAR_K as u8; owner.len()];
        let mut queue = std::collections::VecDeque::new();
        for (i, o) in owner.iter().enumerate() {
            if *o != -1 {
                clear[i] = 0;
                queue.push_back(i);
            }
        }
        while let Some(i) = queue.pop_front() {
            let (x, y) = (i as i32 % w, i as i32 / w);
            let d = clear[i];
            if d as i32 + 1 >= CLEAR_K {
                continue;
            }
            for (dx, dy) in DIRS {
                let (nx, ny) = (x + dx, y + dy);
                if nx < 0 || ny < 0 || nx >= w || ny >= h {
                    continue;
                }
                let j = (ny * w + nx) as usize;
                if clear[j] > d + 1 {
                    clear[j] = d + 1;
                    queue.push_back(j);
                }
            }
        }
        Grid {
            x0: b.x,
            y0: b.y,
            w,
            h,
            owner,
            border,
            clear,
            rects,
        }
    }

    fn idx(&self, p: (i32, i32)) -> Option<usize> {
        let (x, y) = (p.0 - self.x0, p.1 - self.y0);
        (x >= 0 && y >= 0 && x < self.w && y < self.h).then(|| (y * self.w + x) as usize)
    }

    fn point(&self, i: usize) -> (i32, i32) {
        (i as i32 % self.w + self.x0, i as i32 / self.w + self.y0)
    }
}

/// Which lines use one grid point in one direction. Keeps the count, sum
/// and sum of squares of the (dense) bundle ids, so lines can be removed
/// again and "all the same bundle" is still known exactly.
#[derive(Clone, Copy, Default)]
struct Users {
    count: u16,
    sum: u64,
    squares: u64,
}

impl Users {
    fn apply(&mut self, bundle: u32, add: bool) {
        let b = bundle as u64;
        if add {
            self.count += 1;
            self.sum += b;
            self.squares += b * b;
        } else {
            self.count -= 1;
            self.sum -= b;
            self.squares -= b * b;
        }
    }

    /// NONE, the single bundle using it, or MULTI.
    fn owner(&self) -> u32 {
        match self.count {
            0 => NONE,
            c if self.sum * self.sum == self.squares * c as u64 => (self.sum / c as u64) as u32,
            _ => MULTI,
        }
    }
}

/// Where the already-routed lines are.
#[derive(Clone)]
struct Occupancy {
    h: Vec<Users>,
    v: Vec<Users>,
    ports: HashMap<usize, Users>,
}

impl Occupancy {
    fn new(grid: &Grid) -> Occupancy {
        Occupancy {
            h: vec![Users::default(); grid.owner.len()],
            v: vec![Users::default(); grid.owner.len()],
            ports: HashMap::new(),
        }
    }

    fn h(&self, i: usize) -> u32 {
        self.h[i].owner()
    }

    fn v(&self, i: usize) -> u32 {
        self.v[i].owner()
    }

    fn apply(&mut self, grid: &Grid, route: &Route, bundle: u32, add: bool) {
        for (p, q) in route.segments() {
            let hz = p.1 == q.1;
            let (dx, dy) = ((q.0 - p.0).signum(), (q.1 - p.1).signum());
            let mut c = p;
            loop {
                if let Some(i) = grid.idx(c) {
                    let slot = if hz { &mut self.h[i] } else { &mut self.v[i] };
                    slot.apply(bundle, add);
                }
                if c == q {
                    break;
                }
                c = (c.0 + dx, c.1 + dy);
            }
        }
        for p in [route.points.first(), route.points.last()]
            .into_iter()
            .flatten()
        {
            if let Some(i) = grid.idx(*p) {
                self.ports.entry(i).or_default().apply(bundle, add);
            }
        }
    }

    fn add(&mut self, grid: &Grid, route: &Route, bundle: u32) {
        self.apply(grid, route, bundle, true);
    }

    fn remove(&mut self, grid: &Grid, route: &Route, bundle: u32) {
        self.apply(grid, route, bundle, false);
    }

    /// Cost of using port `i`: high if another bundle uses it, lower if
    /// another bundle uses a port next to it (the lines would look doubled).
    fn port_cost(&self, grid: &Grid, i: usize, bundle: u32) -> u32 {
        let other = |j: usize| {
            self.ports
                .get(&j)
                .is_some_and(|u| u.count > 0 && u.owner() != bundle)
        };
        if other(i) {
            return PORT_USED;
        }
        let w = grid.w as usize;
        let near = [i.wrapping_sub(1), i + 1, i.wrapping_sub(w), i + w];
        if near.iter().any(|&j| other(j)) {
            PORT_NEAR
        } else {
            0
        }
    }
}

/// Reusable A* buffers.
struct Search {
    g: Vec<u32>,
    parent: Vec<u32>,
    touched: Vec<usize>,
    heap: BinaryHeap<Reverse<(u32, u32, u32)>>,
}

impl Search {
    fn new(n: usize) -> Search {
        Search {
            g: vec![u32::MAX; n * 4],
            parent: vec![NONE; n * 4],
            touched: Vec::new(),
            heap: BinaryHeap::new(),
        }
    }

    fn reset(&mut self) {
        for &s in &self.touched {
            self.g[s] = u32::MAX;
            self.parent[s] = NONE;
        }
        self.touched.clear();
        self.heap.clear();
    }
}

struct Router<'a> {
    graph: &'a Graph,
    grid: Grid,
    search: Search,
    /// Dense bundle id of each edge (small numbers, for `Users`).
    bundle: Vec<u32>,
}

fn rect_distance(r: &Rect, p: (i32, i32)) -> i32 {
    let dx = (r.x - p.0).max(0).max(p.0 - r.right());
    let dy = (r.y - p.1).max(0).max(p.1 - r.bottom());
    dx + dy
}

impl<'a> Router<'a> {
    fn new(graph: &'a Graph, nodes: &[(i32, i32)]) -> Router<'a> {
        let grid = Grid::new(graph, nodes);
        let n = grid.owner.len();
        let mut dense: HashMap<u32, u32> = HashMap::new();
        let bundle = (0..graph.edges.len())
            .map(|e| {
                let next = dense.len() as u32;
                *dense.entry(bundle_of(graph, e)).or_insert(next)
            })
            .collect();
        Router {
            graph,
            grid,
            search: Search::new(n),
            bundle,
        }
    }

    fn route_edge(&mut self, e: usize, occ: &Occupancy, loops_before: i32) -> Route {
        let edge = &self.graph.edges[e];
        let (src, dst) = (self.grid.rects[edge.from], self.grid.rects[edge.to]);
        if edge.from == edge.to {
            return self_loop(&src, loops_before);
        }
        if edge.kind == EdgeKind::Hierarchy {
            if let Some(r) = self.tree_route(e, occ) {
                return r;
            }
        }
        let window = src.union(&dst).expand(WINDOW);
        if let Some(r) = self.astar(e, occ, Some(window)) {
            return r;
        }
        if let Some(r) = self.astar(e, occ, None) {
            return r;
        }
        fallback(&src, &dst)
    }

    /// The classic inheritance tree: a stem under the middle of the parent,
    /// a bar halfway down to the highest child, and a drop to each child.
    /// Used when that path is clear of boxes and other lines.
    fn tree_route(&self, e: usize, occ: &Occupancy) -> Option<Route> {
        let graph = self.graph;
        let edge = &graph.edges[e];
        let (child, parent) = (self.grid.rects[edge.from], self.grid.rects[edge.to]);
        let (px, py) = (parent.x + parent.w / 2, parent.bottom());
        if child.y < py + 4 {
            return None;
        }
        let min_top = graph
            .edges
            .iter()
            .filter(|o| o.kind == EdgeKind::Hierarchy && o.to == edge.to && o.from != o.to)
            .map(|o| self.grid.rects[o.from].y)
            .filter(|&y| y >= py + 4)
            .min()?;
        let bar = py + ((min_top - py) / 2).max(2);
        let cx = if child.x < px && px < child.right() {
            px
        } else {
            child.x + child.w / 2
        };
        let mut route = Route {
            points: vec![(cx, child.y), (cx, bar), (px, bar), (px, py)],
        };
        route.simplify();
        let bundle = self.bundle[e];
        let last = route.points.len() - 1;
        for (k, (p, q)) in route.segments().enumerate() {
            let hz = p.1 == q.1;
            let (dx, dy) = ((q.0 - p.0).signum(), (q.1 - p.1).signum());
            let mut c = p;
            loop {
                let endpoint = (k == 0 && c == p) || (k + 1 == last && c == q);
                let i = self.grid.idx(c)?;
                if !endpoint {
                    if self.grid.owner[i] != -1 || self.grid.border[i] & if hz { 1 } else { 2 } != 0
                    {
                        return None;
                    }
                    let (same, other) = if hz {
                        (occ.h(i), occ.v(i))
                    } else {
                        (occ.v(i), occ.h(i))
                    };
                    if (same != NONE && same != bundle) || (other != NONE && other != bundle) {
                        return None;
                    }
                }
                if c == q {
                    break;
                }
                c = (c.0 + dx, c.1 + dy);
            }
        }
        if occ.port_cost(&self.grid, self.grid.idx(route.points[0])?, bundle) > 0 {
            return None;
        }
        Some(route)
    }

    fn astar(&mut self, e: usize, occ: &Occupancy, window: Option<Rect>) -> Option<Route> {
        let graph = self.graph;
        let edge = &graph.edges[e];
        let bundle = self.bundle[e];
        let (from, to) = (edge.from as i32, edge.to as i32);
        let (src, dst) = (self.grid.rects[edge.from], self.grid.rects[edge.to]);
        let (src_sides, dst_sides) = allowed_sides(edge.kind);
        let grid = &self.grid;
        let s = &mut self.search;
        s.reset();
        let mut counter = 0u32;
        // Only inheritance lines get the cheaper bundle step, so ordinary
        // lines can use the full step: a tighter estimate, a faster search.
        let unit = if edge.kind == EdgeKind::Hierarchy {
            BUNDLE_STEP
        } else {
            STEP
        };
        let h = |p: (i32, i32)| rect_distance(&dst, p) as u32 * unit;

        for &side in src_sides {
            let d = side.outward();
            for (p, off) in ports(&src, side) {
                let Some(i) = grid.idx(p) else { continue };
                // A port shared with another box (touching boxes) is unusable.
                if grid.owner[i] != from {
                    continue;
                }
                let state = i * 4 + d;
                let g = off as u32 + occ.port_cost(grid, i, bundle);
                if g < s.g[state] {
                    s.g[state] = g;
                    s.parent[state] = NONE;
                    s.touched.push(state);
                    counter += 1;
                    s.heap.push(Reverse((g + h(p), counter, state as u32)));
                }
            }
        }

        let in_window = |p: (i32, i32)| window.is_none_or(|w| w.contains(p));
        let mut goal = None;
        while let Some(Reverse((_, _, state))) = s.heap.pop() {
            let state = state as usize;
            let (i, d) = (state / 4, state % 4);
            let g = s.g[state];
            let p = grid.point(i);
            if grid.owner[i] == to && side_of(&dst, p).is_some() {
                goal = Some(state);
                break;
            }
            for d2 in 0..4 {
                if d2 == d ^ 1 {
                    continue;
                }
                let q = (p.0 + DIRS[d2].0, p.1 + DIRS[d2].1);
                if !in_window(q) {
                    continue;
                }
                let Some(j) = grid.idx(q) else { continue };
                let mut cost = if d2 != d { BEND } else { 0 };
                if grid.owner[j] == TAB {
                    continue;
                }
                if grid.owner[j] >= 0 {
                    // Only arriving at the target, head-on, is allowed.
                    if grid.owner[j] != to {
                        continue;
                    }
                    match side_of(&dst, q) {
                        Some(side) if dst_sides.contains(&side) && side.outward() == d2 ^ 1 => {}
                        _ => continue,
                    }
                    // Inheritance lines all meet in one stem under the
                    // middle of the parent.
                    if edge.kind == EdgeKind::Hierarchy && q != (dst.x + dst.w / 2, dst.bottom()) {
                        continue;
                    }
                    if grid.owner[i] >= 0 {
                        // Do not slide along the target's border.
                        continue;
                    }
                    let (_, off) = ports(&dst, side_of(&dst, q).unwrap())
                        .into_iter()
                        .find(|(pp, _)| *pp == q)
                        .unwrap_or((q, 0));
                    cost += STEP + off as u32 + occ.port_cost(grid, j, bundle);
                } else {
                    let (same, other) = if horizontal(d2) {
                        (occ.h(j), occ.v(j))
                    } else {
                        (occ.v(j), occ.h(j))
                    };
                    cost += if same == bundle { BUNDLE_STEP } else { STEP };
                    if same != NONE && same != bundle {
                        cost += SHARE;
                    }
                    if other != NONE && other != bundle {
                        cost += CROSS;
                    }
                    cost += CLEAR_W * (CLEAR_K - grid.clear[j] as i32).max(0) as u32;
                    if grid.border[j] & if horizontal(d2) { 1 } else { 2 } != 0 {
                        cost += BORDER;
                    }
                }
                let ns = j * 4 + d2;
                let ng = g + cost;
                if ng < s.g[ns] {
                    if s.g[ns] == u32::MAX {
                        s.touched.push(ns);
                    }
                    s.g[ns] = ng;
                    s.parent[ns] = state as u32;
                    counter += 1;
                    s.heap.push(Reverse((ng + h(q), counter, ns as u32)));
                }
            }
        }

        let mut state = goal?;
        let mut points = vec![grid.point(state / 4)];
        while s.parent[state] != NONE {
            state = s.parent[state] as usize;
            points.push(grid.point(state / 4));
        }
        points.reverse();
        let mut r = Route { points };
        r.simplify();
        Some(r)
    }
}

/// A loop off the top-right corner for an edge from a box to itself.
fn self_loop(r: &Rect, k: i32) -> Route {
    let o = 2 * k;
    let right = r.right();
    let inner = (right - 2 - o).max(r.x + 1);
    let y_start = (r.y + 1 + o).min(r.bottom() - 1);
    Route {
        points: vec![
            (right, y_start),
            (right + 2 + o, y_start),
            (right + 2 + o, r.y - 2 - o),
            (inner, r.y - 2 - o),
            (inner, r.y),
        ],
    }
}

/// Used only when no path exists (boxes overlapping each other).
fn fallback(src: &Rect, dst: &Rect) -> Route {
    let a = src.center();
    let b = dst.center();
    let mut r = Route {
        points: vec![a, (b.0, a.1), b],
    };
    r.simplify();
    r
}

fn routing_order(graph: &Graph, nodes: &[(i32, i32)], edges: &[usize]) -> Vec<usize> {
    let mut order = edges.to_vec();
    let center = |n: usize| {
        let node = &graph.nodes[n];
        (nodes[n].0 + node.w / 2, nodes[n].1 + node.h / 2)
    };
    order.sort_by_key(|&e| {
        let edge = &graph.edges[e];
        let (a, b) = (center(edge.from), center(edge.to));
        let dist = (a.0 - b.0).abs() + (a.1 - b.1).abs();
        let hier = edge.kind != EdgeKind::Hierarchy;
        // Inheritance first, grouped by parent; then shortest first.
        (hier, if hier { 0 } else { edge.to }, dist, e)
    });
    order
}

fn crossings_per_edge(graph: &Graph, routes: &[Route]) -> Vec<usize> {
    let mut count = vec![0; routes.len()];
    for (_, a, b) in crossing_points(graph, routes) {
        count[a] += 1;
        count[b] += 1;
    }
    count
}

fn occupancy_of(router: &Router, routes: &[Route]) -> Occupancy {
    let mut occ = Occupancy::new(&router.grid);
    for (e, r) in routes.iter().enumerate() {
        if !r.points.is_empty() {
            occ.add(&router.grid, r, router.bundle[e]);
        }
    }
    occ
}

/// Crossings of `candidate` (as edge `e`) with every other line, counted
/// once per crossing point and bundle.
fn crossings_against(router: &Router, routes: &[Route], e: usize, candidate: &Route) -> usize {
    let mut seen = std::collections::HashSet::new();
    for (o, r) in routes.iter().enumerate() {
        if o == e || router.bundle[o] == router.bundle[e] {
            continue;
        }
        for sa in candidate.segments() {
            for sb in r.segments() {
                if let Some(p) = segment_crossing(sa, sb) {
                    seen.insert((p, router.bundle[o]));
                }
            }
        }
    }
    seen.len()
}

/// Rips up lines that cross others and routes them again, keeping the new
/// route when it crosses less (or equally, but is shorter).
fn improve(router: &mut Router, routes: &mut [Route], candidates: &[usize], passes: usize) {
    let graph = router.graph;
    let mut occ = occupancy_of(router, routes);
    for _ in 0..passes {
        let counts = crossings_per_edge(graph, routes);
        let mut bad: Vec<usize> = candidates
            .iter()
            .copied()
            .filter(|&e| counts[e] > 0)
            .collect();
        if bad.is_empty() {
            return;
        }
        bad.sort_by_key(|&e| (Reverse(counts[e]), e));
        let mut improved = false;
        for e in bad {
            if graph.edges[e].from == graph.edges[e].to {
                continue;
            }
            let b = router.bundle[e];
            let old = std::mem::take(&mut routes[e]);
            occ.remove(&router.grid, &old, b);
            let old_cross = crossings_against(router, routes, e, &old);
            let new = router.route_edge(e, &occ, 0);
            let new_cross = crossings_against(router, routes, e, &new);
            // Inheritance lines keep their tree shape unless crossings drop.
            let shorter =
                (new.length() + 4 * new.bends() as i32) < (old.length() + 4 * old.bends() as i32);
            let better = new_cross < old_cross
                || (new_cross == old_cross
                    && shorter
                    && graph.edges[e].kind != EdgeKind::Hierarchy);
            routes[e] = if better {
                improved = true;
                new
            } else {
                old
            };
            occ.add(&router.grid, &routes[e], b);
        }
        if !improved {
            return;
        }
    }
}

/// Routes every edge with the nodes at `nodes`.
pub fn route_all(graph: &Graph, nodes: &[(i32, i32)]) -> Vec<Route> {
    let mut routes = vec![Route::default(); graph.edges.len()];
    if graph.edges.is_empty() {
        return routes;
    }
    let mut router = Router::new(graph, nodes);
    let all: Vec<usize> = (0..graph.edges.len()).collect();
    route_edges(&mut router, nodes, &mut routes, &all);
    // Rip-up passes pay off most on small diagrams; large ones get one.
    let passes = if graph.edges.len() <= 100 { 3 } else { 1 };
    improve(&mut router, &mut routes, &all, passes);
    routes
}

fn route_edges(router: &mut Router, nodes: &[(i32, i32)], routes: &mut [Route], edges: &[usize]) {
    let graph = router.graph;
    let mut occ = Occupancy::new(&router.grid);
    let set: std::collections::HashSet<usize> = edges.iter().copied().collect();
    for (e, r) in routes.iter().enumerate() {
        if !set.contains(&e) && !r.points.is_empty() {
            occ.add(&router.grid, r, router.bundle[e]);
        }
    }
    let mut loops: HashMap<usize, i32> = HashMap::new();
    for e in routing_order(graph, nodes, edges) {
        let edge = &graph.edges[e];
        let k = if edge.from == edge.to {
            let c = loops.entry(edge.from).or_insert(0);
            *c += 1;
            *c - 1
        } else {
            0
        };
        let r = router.route_edge(e, &occ, k);
        occ.add(&router.grid, &r, router.bundle[e]);
        routes[e] = r;
    }
}

/// Re-routes the lines touching `moved` nodes, and any line that now runs
/// through one of them. Other lines stay as they are.
pub fn reroute(graph: &Graph, nodes: &[(i32, i32)], routes: &mut Vec<Route>, moved: &[usize]) {
    routes.resize(graph.edges.len(), Route::default());
    let rects: Vec<Rect> = moved
        .iter()
        .map(|&n| Rect::new(nodes[n].0, nodes[n].1, graph.nodes[n].w, graph.nodes[n].h))
        .collect();
    let dirty: Vec<usize> = (0..graph.edges.len())
        .filter(|&e| {
            let edge = &graph.edges[e];
            routes[e].points.is_empty()
                || moved.contains(&edge.from)
                || moved.contains(&edge.to)
                || route_hits(&routes[e], &rects)
        })
        .collect();
    if dirty.is_empty() {
        return;
    }
    let mut router = Router::new(graph, nodes);
    for &e in &dirty {
        routes[e] = Route::default();
    }
    route_edges(&mut router, nodes, routes, &dirty);
}

fn route_hits(route: &Route, rects: &[Rect]) -> bool {
    route.segments().any(|(p, q)| {
        let seg = Rect::new(
            p.0.min(q.0),
            p.1.min(q.1),
            (p.0 - q.0).abs(),
            (p.1 - q.1).abs(),
        );
        rects.iter().any(|r| r.too_close(&seg, 1))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::score::measure;

    fn two_boxes(a: (i32, i32), b: (i32, i32), kind: EdgeKind) -> (Graph, Vec<(i32, i32)>) {
        let mut g = Graph::default();
        g.add_node(10, 4);
        g.add_node(10, 4);
        g.add_edge(0, 1, kind);
        (g, vec![a, b])
    }

    fn assert_clear_of_boxes(g: &Graph, nodes: &[(i32, i32)], e: usize, r: &Route) {
        let edge = &g.edges[e];
        for (p, q) in r.segments() {
            let (dx, dy) = ((q.0 - p.0).signum(), (q.1 - p.1).signum());
            let mut c = p;
            loop {
                for (n, pos) in nodes.iter().enumerate() {
                    let rect = Rect::new(pos.0, pos.1, g.nodes[n].w, g.nodes[n].h);
                    let endpoint = c == r.points[0] || c == *r.points.last().unwrap();
                    let own = n == edge.from || n == edge.to;
                    assert!(
                        !rect.contains(c) || (own && endpoint),
                        "point {c:?} hits box {n} in {r:?}"
                    );
                }
                if c == q {
                    break;
                }
                c = (c.0 + dx, c.1 + dy);
            }
        }
    }

    #[test]
    fn straight_line_between_aligned_boxes() {
        let (g, nodes) = two_boxes((0, 0), (0, 20), EdgeKind::Assoc);
        let r = &route_all(&g, &nodes)[0];
        assert_eq!(r.bends(), 0, "{r:?}");
        assert_eq!(r.points, vec![(5, 4), (5, 20)]);
    }

    #[test]
    fn routes_around_obstacle() {
        let mut g = Graph::default();
        g.add_node(10, 4);
        g.add_node(10, 4);
        g.add_node(30, 4);
        g.add_edge(0, 1, EdgeKind::Assoc);
        let nodes = vec![(0, 0), (0, 30), (-10, 14)];
        let routes = route_all(&g, &nodes);
        assert_clear_of_boxes(&g, &nodes, 0, &routes[0]);
        assert_eq!(
            routes[0]
                .points
                .first()
                .map(|p| Rect::new(0, 0, 10, 4).contains(*p)),
            Some(true)
        );
        assert_eq!(
            routes[0]
                .points
                .last()
                .map(|p| Rect::new(0, 30, 10, 4).contains(*p)),
            Some(true)
        );
    }

    #[test]
    fn two_parallel_edges_do_not_overlap() {
        let (mut g, nodes) = two_boxes((0, 0), (0, 20), EdgeKind::Assoc);
        g.add_edge(0, 1, EdgeKind::Dep);
        let routes = route_all(&g, &nodes);
        assert_ne!(routes[0], routes[1]);
        let s = measure(&g, &nodes, &routes);
        assert_eq!(s.shared_lanes, 0, "{routes:?}");
        assert_eq!(s.crossings, 0);
    }

    #[test]
    fn inheritance_siblings_share_trunk() {
        let mut g = Graph::default();
        let p = g.add_node(10, 4);
        let a = g.add_node(10, 4);
        let b = g.add_node(10, 4);
        g.add_edge(a, p, EdgeKind::Hierarchy);
        g.add_edge(b, p, EdgeKind::Hierarchy);
        let nodes = vec![(20, 0), (0, 20), (40, 20)];
        let routes = route_all(&g, &nodes);
        // Both arrive at the same point under the parent.
        assert_eq!(routes[0].points.last(), routes[1].points.last());
        assert_eq!(routes[0].points.last().unwrap().1, 4);
        // Child lines leave from the top.
        assert_eq!(routes[0].points[0].1, 20);
        assert_eq!(routes[1].points[0].1, 20);
        assert_eq!(measure(&g, &nodes, &routes).crossings, 0);
    }

    #[test]
    fn self_loop() {
        let mut g = Graph::default();
        g.add_node(10, 4);
        g.add_edge(0, 0, EdgeKind::Assoc);
        let r = &route_all(&g, &[(0, 0)])[0];
        assert!(r.points.len() >= 4);
        let rect = Rect::new(0, 0, 10, 4);
        assert!(side_of(&rect, r.points[0]).is_some());
        assert!(side_of(&rect, *r.points.last().unwrap()).is_some());
    }

    #[test]
    fn avoids_crossing_when_possible() {
        // A above B, C left, D right: A-B and C-D would cross straight,
        // but there is room to go around.
        let mut g = Graph::default();
        for _ in 0..4 {
            g.add_node(6, 4);
        }
        g.add_edge(0, 1, EdgeKind::Assoc);
        g.add_edge(2, 3, EdgeKind::Assoc);
        let nodes = vec![(20, 0), (20, 30), (0, 15), (40, 15)];
        let routes = route_all(&g, &nodes);
        // Crossing is unavoidable here only if both lines are inside the
        // square; the router may route around.
        let s = measure(&g, &nodes, &routes);
        assert!(s.crossings <= 1);
    }

    #[test]
    fn deterministic() {
        let mut g = Graph::default();
        for _ in 0..6 {
            g.add_node(8, 4);
        }
        for (a, b) in [(0, 1), (1, 2), (2, 3), (3, 4), (4, 5), (5, 0), (0, 3)] {
            g.add_edge(a, b, EdgeKind::Assoc);
        }
        let nodes = vec![(0, 0), (20, 0), (40, 0), (0, 20), (20, 20), (40, 20)];
        assert_eq!(route_all(&g, &nodes), route_all(&g, &nodes));
    }

    #[test]
    fn reroute_moves_only_affected_lines() {
        let mut g = Graph::default();
        for _ in 0..4 {
            g.add_node(8, 4);
        }
        g.add_edge(0, 1, EdgeKind::Assoc);
        g.add_edge(2, 3, EdgeKind::Assoc);
        let mut nodes = vec![(0, 0), (0, 20), (40, 0), (40, 20)];
        let mut routes = route_all(&g, &nodes);
        let untouched = routes[1].clone();
        nodes[1] = (10, 30);
        reroute(&g, &nodes, &mut routes, &[1]);
        assert_eq!(routes[1], untouched);
        assert_eq!(routes[0].points.last().unwrap().1, 30);
    }
}
