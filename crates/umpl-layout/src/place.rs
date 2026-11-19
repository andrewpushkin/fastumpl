//! The placement pipeline and the public entry points.
//!
//! Groups are laid out innermost first; each finished group then acts as one
//! box in its parent. Inside a scope, each connected part is layered,
//! ordered and given coordinates, and the parts are packed in rows. Several
//! candidate layouts (different starting orders) are routed and scored in
//! parallel; the best one wins, ties going to the lower index.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::coords::assign_coords;
use crate::graph::*;
use crate::labels::place_labels;
use crate::layering::{assign_layers_with, FlatEdge};
use crate::ordering::{Ghost, GhostSide, Layered};
use crate::rng::Rng;
use crate::router::{polish, route_all_with};
use crate::score::measure;

#[derive(Clone, Debug)]
pub struct Options {
    /// How many candidate layouts compete.
    pub candidates: usize,
    pub seed: u64,
    /// Set to stop early; the best layout found so far is returned.
    pub cancel: Option<Arc<AtomicBool>>,
    /// Width : height of the view the drawing should fill, such as the
    /// editor's canvas. Layers and loose parts are arranged to fit it.
    pub aspect: f64,
    /// What to favour when fewer crossings and filling the view pull
    /// apart.
    pub goal: Goal,
}

/// What a layout favours when fewer line crossings and a drawing of the
/// view's shape pull apart.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Goal {
    /// The fewest crossings: the drawing may come out wider or taller
    /// than the view (loose parts still fill it).
    #[default]
    FewestCrossings,
    /// A drawing of the view's shape, with no blank band beside or below
    /// it, for a few more crossings where need be.
    FillView,
}

/// The usual drawing shape, 16:10.
pub const DEFAULT_ASPECT: f64 = 1.6;

impl Default for Options {
    fn default() -> Self {
        Options {
            candidates: 12,
            seed: 1,
            cancel: None,
            aspect: DEFAULT_ASPECT,
            goal: Goal::FewestCrossings,
        }
    }
}

/// The shape a candidate aims for: how wide layers may grow (a factor of
/// the usual) and the width : height of the view.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Target {
    pub width: f64,
    pub aspect: f64,
    /// Loose parts fill a view of the target shape (the top level), or
    /// pack tight (inside a package, whose box should stay compact).
    pub fill: bool,
    /// A package that is all there is at its level fills the view too
    /// ([`Goal::FillView`]).
    pub whole: bool,
    /// Parts without inheritance may run left to right where that is
    /// closer to the view's shape (some candidates when filling the view).
    pub turn: bool,
}

#[cfg(test)]
impl Target {
    pub(crate) fn usual() -> Target {
        Target {
            width: 1.0,
            aspect: DEFAULT_ASPECT,
            fill: true,
            whole: false,
            turn: false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Item {
    Node(usize),
    Group(usize),
}

/// A laid-out component: item indices, their positions, its bounds.
type Placed = (Vec<usize>, Vec<(i32, i32)>, Rect);
/// A laid-out scope: the group (or root), its items, their positions.
type ScopeLayout = (Option<usize>, Vec<Item>, Vec<(i32, i32)>);

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

/// Centre x of every item, from `prev` positions or else the nodes' own
/// positions, if all have one.
fn hints(graph: &Graph, items: &[Item], prev: Option<&[(i32, i32)]>) -> Option<Vec<f64>> {
    let pos = |n: usize| prev.map_or(graph.nodes[n].pos, |p| Some(p[n]));
    items
        .iter()
        .map(|it| match *it {
            Item::Node(n) => pos(n).map(|p| p.0 as f64 + graph.nodes[n].w as f64 / 2.0),
            Item::Group(g) => {
                let xs: Vec<f64> = (0..graph.nodes.len())
                    .filter(|&n| {
                        graph.nodes[n]
                            .group
                            .is_some_and(|ng| graph.group_within(ng, g))
                    })
                    .filter_map(|n| pos(n).map(|p| p.0 as f64 + graph.nodes[n].w as f64 / 2.0))
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

/// Layers, orders and places one connected part: the positions of its
/// items and their bounds.
fn layered(
    sizes: &[(i32, i32)],
    edges: &[FlatEdge],
    ghosts: &[Ghost],
    hint: Option<&[f64]>,
    start: Start,
    ci: usize,
    target: Target,
) -> (Vec<(i32, i32)>, Rect) {
    let n = sizes.len();
    let layers = assign_layers_with(n, edges, sizes, target);
    let mut lay = Layered::build_with_ghosts(n, &layers, edges, ghosts);
    match start {
        Start::Hint => lay.order(hint, None),
        Start::DepthFirst => lay.order(None, None),
        Start::Random(seed) => {
            let mut rng = Rng::new(seed.wrapping_add(ci as u64 * 7919));
            lay.order(None, Some(&mut rng))
        }
    }
    lay.strip_ghosts();
    let mut widths: Vec<i32> = sizes.iter().map(|s| s.0).collect();
    let mut heights: Vec<i32> = sizes.iter().map(|s| s.1).collect();
    widths.resize(lay.total(), 0);
    heights.resize(lay.total(), 0);
    let pos = assign_coords(&lay, &widths, &heights);
    let mut bounds: Option<Rect> = None;
    for (li, p) in pos.iter().enumerate().take(n) {
        let r = Rect::new(p.0, p.1, sizes[li].0, sizes[li].1);
        bounds = Some(bounds.map_or(r, |b| b.union(&r)));
    }
    (pos, bounds.unwrap_or_default())
}

/// Lays out one scope; returns item top-left corners, normalised to (0, 0).
fn layout_scope(
    sizes: &[(i32, i32)],
    edges: &[FlatEdge],
    hint: Option<&[f64]>,
    start: Start,
    ghosts: &[Ghost],
    target: Target,
) -> Vec<(i32, i32)> {
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

    // Per component: its items, their positions, and its bounds.
    let mut placed: Vec<Placed> = Vec::new();
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
                ..*e
            })
            .collect();
        let sub_sizes: Vec<(i32, i32)> = comp.iter().map(|&i| sizes[i]).collect();
        let sub_ghosts: Vec<Ghost> = ghosts
            .iter()
            .filter(|g| local[g.item] != usize::MAX)
            .map(|g| Ghost {
                item: local[g.item],
                ..*g
            })
            .collect();
        let sub_hint: Option<Vec<f64>> = hint.map(|h| comp.iter().map(|&i| h[i]).collect());
        let (mut pos, mut bounds) = layered(
            &sub_sizes,
            &sub_edges,
            &sub_ghosts,
            sub_hint.as_deref(),
            start,
            ci,
            target,
        );
        // To fill the view, a part without inheritance (which keeps
        // parents above children) may also run left to right: a chain of
        // boxes becomes a row instead of a tall column. The direction
        // closer to the view's shape wins.
        let free = !sub_edges.iter().any(|e| e.kind == EdgeKind::Hierarchy);
        if target.turn && free && comp.len() > 2 {
            let shape = |b: &Rect| b.w.max(1) as f64 / b.h.max(1) as f64;
            let t_sizes: Vec<(i32, i32)> = sub_sizes.iter().map(|&(w, h)| (h, w)).collect();
            let t_edges: Vec<FlatEdge> = sub_edges
                .iter()
                .map(|e| FlatEdge {
                    ports: (0.5, 0.5),
                    ..*e
                })
                .collect();
            let t_ghosts: Vec<Ghost> = sub_ghosts
                .iter()
                .map(|g| Ghost {
                    side: match g.side {
                        GhostSide::Top => GhostSide::Left,
                        GhostSide::Bottom => GhostSide::Right,
                        GhostSide::Left => GhostSide::Top,
                        GhostSide::Right => GhostSide::Bottom,
                    },
                    port: 0.5,
                    ..*g
                })
                .collect();
            let (t_pos, t_bounds) = layered(&t_sizes, &t_edges, &t_ghosts, None, start, ci, target);
            let t_bounds = Rect::new(t_bounds.y, t_bounds.x, t_bounds.h, t_bounds.w);
            if misfit(shape(&t_bounds), target.aspect) + 0.05
                < misfit(shape(&bounds), target.aspect)
            {
                pos = t_pos.iter().map(|&(x, y)| (y, x)).collect();
                bounds = t_bounds;
            }
        }
        placed.push((comp.clone(), pos, bounds));
    }

    // Pack the parts in rows, biggest first, trying many row widths and
    // keeping the packing that best fills a view of the target shape.
    let mut order: Vec<usize> = (0..placed.len()).collect();
    order.sort_by_key(|&i| {
        (
            std::cmp::Reverse(placed[i].2.w as i64 * placed[i].2.h as i64),
            placed[i].0[0],
        )
    });
    // Inside a package, parts go left to right by where the lines leaving
    // them go; parts without such lines take the middle.
    if !ghosts.is_empty() {
        let mean = |ks: &[f64]| ks.iter().sum::<f64>() / ks.len() as f64;
        let all: Vec<f64> = ghosts.iter().map(|g| g.key).collect();
        let key: Vec<f64> = comps
            .iter()
            .map(|comp| {
                let ks: Vec<f64> = ghosts
                    .iter()
                    .filter(|g| comp.contains(&g.item))
                    .map(|g| g.key)
                    .collect();
                if ks.is_empty() {
                    mean(&all)
                } else {
                    mean(&ks)
                }
            })
            .collect();
        order.sort_by(|&a, &b| key[a].total_cmp(&key[b]));
    }
    let rects: Vec<Rect> = order.iter().map(|&i| placed[i].2).collect();
    let offsets = best_packing(&rects, target);
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

/// Packs `rects` in rows, trying many row widths.
///
/// At the top level the drawing should fill a view of the target shape:
/// the packing wins whose bounding box needs the least zoom to fit such a
/// view, `max(w, h * aspect)`, then the smaller area. (Smallest area alone
/// puts many small parts in one long row, since a second row costs a whole
/// gap between rows; the editor then shows a thin strip.) Inside a package
/// the package box should stay compact: smallest area, nudged towards
/// 16:10.
/// A compact packing of more than `STRIP_PARTS` parts, longer than
/// `STRIP` (width : height), is a strip. A few boxes in a row are not.
const STRIP: f64 = 6.0;
const STRIP_PARTS: usize = 8;

fn best_packing(rects: &[Rect], target: Target) -> Vec<(i32, i32)> {
    let (offsets, w, h) = packing(rects, target);
    // Compact, but not a strip: many loose parts in a package (58 classes
    // implementing an interface outside it) would otherwise make one row.
    let shape = w.max(1) as f64 / h.max(1) as f64;
    if !target.fill && rects.len() > STRIP_PARTS && !(1.0 / STRIP..=STRIP).contains(&shape) {
        let fill = Target {
            fill: true,
            aspect: DEFAULT_ASPECT,
            ..target
        };
        return packing(rects, fill).0;
    }
    offsets
}

/// The best row packing for `target`, with its width and height.
fn packing(rects: &[Rect], target: Target) -> (Vec<(i32, i32)>, i32, i32) {
    let widest = rects.iter().map(|r| r.w).max().unwrap_or(0);
    let all: i32 = rects.iter().map(|r| r.w + 2 * NODE_GAP).sum();
    let steps: i32 = if target.fill { 48 } else { 12 };
    type Packing = ((f64, f64), (Vec<(i32, i32)>, i32, i32));
    let mut best: Option<Packing> = None;
    let mut last_width = -1;
    for k in 0..=steps {
        let row_width = widest + (all - widest) * k / steps;
        if row_width == last_width {
            continue;
        }
        last_width = row_width;
        let (offsets, w, h) = pack_rows(rects, row_width);
        let (wf, hf) = (w.max(1) as f64, h.max(1) as f64);
        let cost = if target.fill {
            (wf.max(hf * target.aspect), wf * hf)
        } else {
            (
                wf * hf * (1.0 + 0.3 * ((wf / hf) / DEFAULT_ASPECT).ln().abs()),
                0.0,
            )
        };
        let better = best.as_ref().is_none_or(|(c, _)| {
            cost.0 < c.0 - 1e-9 || (cost.0 < c.0 + 1e-9 && cost.1 < c.1 - 1e-9)
        });
        if better {
            best = Some((cost, (offsets, w, h)));
        }
    }
    best.map(|b| b.1).unwrap_or_default()
}

/// Where node `n`'s lines attach along the width of `item`, which stands
/// for `n` (a package's member lines attach under that member).
fn port_of(item: Item, n: usize, member_x: &[HashMap<usize, i32>], size: &[(i32, i32)]) -> f64 {
    match item {
        Item::Node(_) => 0.5,
        Item::Group(g) => member_x[g].get(&n).map_or(0.5, |&x| {
            (x as f64 / size[g].0.max(1) as f64).clamp(0.02, 0.98)
        }),
    }
}

/// The side of `r` that a line to a point at `c` leaves through.
fn side_towards(r: &Rect, c: (f64, f64)) -> GhostSide {
    if c.1 < r.y as f64 {
        GhostSide::Top
    } else if c.1 > r.bottom() as f64 {
        GhostSide::Bottom
    } else if c.0 < r.x as f64 + r.w as f64 / 2.0 {
        GhostSide::Left
    } else {
        GhostSide::Right
    }
}

/// Whether `scope` should fill the view: the top, or a package that is
/// all there is in a scope that fills it (a diagram drawn inside one
/// package). Other packages pack tight, to keep their boxes compact.
fn fills_view(graph: &Graph, scope: Option<usize>) -> bool {
    let mut scope = scope;
    while let Some(g) = scope {
        let parent = graph.groups[g].parent;
        if scope_items(graph, parent).len() != 1 {
            return false;
        }
        scope = parent;
    }
    true
}

/// Computes node positions for one candidate. With `prev` (positions from
/// an earlier round), the order starts from those positions and each
/// package sees where its outside lines go.
fn place_all(
    graph: &Graph,
    start: Start,
    target: Target,
    prev: Option<&[(i32, i32)]>,
) -> Vec<(i32, i32)> {
    let ng = graph.groups.len();
    let prev_groups = prev.map(|p| group_rects(graph, p));
    // Centre x of every member node, relative to its group's left edge.
    let mut member_x: Vec<HashMap<usize, i32>> = vec![HashMap::new(); ng];
    // Deepest groups first, root last.
    let mut scopes: Vec<Option<usize>> = (0..ng).map(Some).collect();
    scopes.sort_by_key(|g| std::cmp::Reverse(graph.group_chain(*g).len()));
    scopes.push(None);

    let mut group_size = vec![(0, 0); ng];
    // Per scope: its items and their relative positions.
    let mut rel: Vec<ScopeLayout> = Vec::new();
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
        let mut ghosts = Vec::new();
        for e in &graph.edges {
            let ends = (
                item_in_scope(graph, e.from, scope),
                item_in_scope(graph, e.to, scope),
            );
            match ends {
                (Some(a), Some(b)) if a != b => {
                    // Labels laid end to end along the line, with a little air.
                    let gap = e.labels.iter().flatten().map(|l| l.0 + 2).sum::<i32>() + 2;
                    let mut fe = FlatEdge::new(index(a).unwrap(), index(b).unwrap(), e.kind, gap);
                    fe.ports = (
                        port_of(a, e.from, &member_x, &group_size),
                        port_of(b, e.to, &member_x, &group_size),
                    );
                    edges.push(fe);
                }
                (Some(a), None) | (None, Some(a)) => {
                    let (Some(g), Some(p), Some(rects)) = (scope, prev, &prev_groups) else {
                        continue;
                    };
                    let (inner, outer) = if ends.0.is_some() {
                        (e.from, e.to)
                    } else {
                        (e.to, e.from)
                    };
                    let c = (
                        p[outer].0 as f64 + graph.nodes[outer].w as f64 / 2.0,
                        p[outer].1 as f64 + graph.nodes[outer].h as f64 / 2.0,
                    );
                    ghosts.push(Ghost {
                        item: index(a).unwrap(),
                        side: side_towards(&rects[g], c),
                        key: c.0,
                        port: port_of(a, inner, &member_x, &group_size),
                    });
                }
                _ => {}
            }
        }
        let hint = hints(graph, &items, prev);
        let start = match (start, &hint) {
            (Start::Hint, None) => Start::DepthFirst,
            (s, _) => s,
        };
        let scope_target = Target {
            fill: scope.is_none() || (target.whole && fills_view(graph, scope)),
            ..target
        };
        let pos = layout_scope(
            &sizes,
            &edges,
            hint.as_deref(),
            start,
            &ghosts,
            scope_target,
        );
        if let Some(g) = scope {
            let mut mx = HashMap::new();
            for (it, p) in items.iter().zip(&pos) {
                match *it {
                    Item::Node(n) => {
                        mx.insert(n, GROUP_PAD + p.0 + graph.nodes[n].w / 2);
                    }
                    Item::Group(h) => {
                        for (&n, &x) in &member_x[h] {
                            mx.insert(n, GROUP_PAD + p.0 + x);
                        }
                    }
                }
            }
            member_x[g] = mx;
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

/// Rounds of package-aware placement after the first one.
const ROUNDS: usize = 2;

/// Places everything, then again `ROUNDS` times with each package seeing
/// where its outside lines went in the round before.
fn place_rounds(graph: &Graph, start: Start, target: Target) -> Vec<(i32, i32)> {
    let mut nodes = place_all(graph, start, target, None);
    if !graph.groups.is_empty() {
        for _ in 0..ROUNDS {
            nodes = place_all(graph, Start::Hint, target, Some(&nodes));
        }
    }
    nodes
}

/// Layer widths, as factors of the usual, that some candidates try: how
/// the boxes are spread over layers changes the crossings more than
/// anything after it.
const WIDTHS: [f64; 4] = [0.75, 1.35, 0.6, 1.7];

/// Layer width of a candidate when crossings come first. The hint and
/// depth-first candidates and every other random one keep the usual width,
/// so a tidy diagram is found again; the remaining random ones try the
/// `WIDTHS`.
fn width_of(start: Start, seed: u64) -> f64 {
    match start {
        Start::Random(s) => {
            let k = s.wrapping_sub(seed);
            if k % 2 == 1 {
                1.0
            } else {
                WIDTHS[((k / 2 - 1) % WIDTHS.len() as u64) as usize]
            }
        }
        _ => 1.0,
    }
}

/// Layer width of candidate `i` when the drawing should fill the view,
/// given the width `base` at which the
/// drawing has the view's shape. The first candidate (the current layout's
/// order, or depth first) keeps the usual width, so a tidy diagram is
/// found again and the fewest crossings are on offer; the next takes
/// `base` and the one after the width halfway, so that the choice weighs
/// crossings against filling the view. The rest try every other one
/// `base`, else the `WIDTHS` around it.
fn width_for(i: usize, base: f64) -> f64 {
    match i {
        0 => 1.0,
        1 => base,
        2 => base.sqrt(),
        _ if (i - 3).is_multiple_of(2) => base,
        _ => base * WIDTHS[((i - 3) / 2) % WIDTHS.len()],
    }
}

/// Width : height of placed nodes with their packages.
fn shape_of(graph: &Graph, nodes: &[(i32, i32)]) -> f64 {
    let mut r: Option<Rect> = None;
    let mut add = |o: Rect| r = Some(r.map_or(o, |r| r.union(&o)));
    for (i, &(x, y)) in nodes.iter().enumerate() {
        add(Rect::new(x, y, graph.nodes[i].w, graph.nodes[i].h));
    }
    for g in group_rects(graph, nodes) {
        add(g);
    }
    let r = r.unwrap_or_default();
    r.w.max(1) as f64 / r.h.max(1) as f64
}

/// How far a drawing of width : height `shape` is from filling a view of
/// width : height `aspect`: 0 when it fills it, ln 2 when half the view
/// stays blank (a strip twice too wide or too tall).
fn misfit(shape: f64, aspect: f64) -> f64 {
    (shape / aspect).ln().abs()
}

/// Close enough to the view's shape: nine tenths of the view filled.
const FITS: f64 = 0.105;

/// The layer width, as a factor of the usual, at which the drawing gets
/// the view's shape. Wider layers make a wider, lower drawing: with the
/// same area, width : height grows about as the square of the factor, so
/// each step scales it by the square root of what is missing. A few
/// steps; a chain of boxes, which no width makes wider, stops them early.
fn calibrated_width(graph: &Graph, aspect: f64, cancelled: &dyn Fn() -> bool) -> f64 {
    let shape = |f: f64| {
        let target = Target {
            width: f,
            aspect,
            fill: true,
            whole: true,
            turn: false,
        };
        shape_of(graph, &place_rounds(graph, Start::DepthFirst, target))
    };
    let mut f = 1.0;
    let mut r = shape(f);
    for _ in 0..3 {
        if misfit(r, aspect) < FITS || cancelled() {
            break;
        }
        let next = (f * (aspect / r).sqrt()).clamp(0.2, 4.0);
        if (next / f).ln().abs() < 0.02 {
            break;
        }
        let rn = shape(next);
        if misfit(rn, aspect) >= misfit(r, aspect) {
            break;
        }
        f = next;
        r = rn;
    }
    f
}

/// How much worse a candidate counts for not filling the view: its
/// score times 1 + 2 × the [`misfit`] beyond nine tenths of the view
/// filled ([`FITS`]). Leaving half the view blank makes it count about
/// twice its score, at any size of diagram, so a layout that fills the
/// view wins over one with a few fewer crossings or shorter lines; near
/// the view's shape, the score alone decides.
fn fit_factor(shape: f64, aspect: f64) -> f64 {
    1.0 + 2.0 * (misfit(shape, aspect) - FITS).max(0.0)
}

/// Memory the routing of candidates may take at once, in bytes.
const MEMORY: usize = 1_500_000_000;

/// Crossings of the straight lines between box centres: a cheap stand-in
/// for the crossings of routed lines, to choose a candidate to route.
fn straight_crossings(graph: &Graph, nodes: &[(i32, i32)]) -> usize {
    let center = |n: usize| {
        (
            nodes[n].0 as f64 + graph.nodes[n].w as f64 / 2.0,
            nodes[n].1 as f64 + graph.nodes[n].h as f64 / 2.0,
        )
    };
    let segs: Vec<((f64, f64), (f64, f64))> = graph
        .edges
        .iter()
        .filter(|e| e.from != e.to)
        .map(|e| (center(e.from), center(e.to)))
        .collect();
    let mut count = 0;
    for a in 0..segs.len() {
        for b in a + 1..segs.len() {
            if segments_cross(segs[a].0, segs[a].1, segs[b].0, segs[b].1) {
                count += 1;
            }
        }
    }
    count
}

/// Routes, labels and measures a layout with nodes at `nodes`.
pub(crate) fn finalize(graph: &Graph, nodes: Vec<(i32, i32)>) -> Layout {
    finalize_with(graph, nodes, true)
}

/// As [`finalize`]; `wide` routing looks further for crossing-free routes.
fn finalize_with(graph: &Graph, nodes: Vec<(i32, i32)>, wide: bool) -> Layout {
    let routes = route_all_with(graph, &nodes, wide);
    let groups = group_rects(graph, &nodes);
    let labels = place_labels(graph, &nodes, &routes);
    let mut stats = measure(graph, &nodes, &routes);
    stats.draft = crate::router::grid_cells(graph, &nodes) > crate::router::DRAFT_CELLS;
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
    // Every candidate is routed in full, so large diagrams get fewer.
    let wanted = match graph.nodes.len() {
        0..=60 => opts.candidates,
        61..=150 => opts.candidates.min(6),
        _ => opts.candidates.min(3),
    };
    let mut k = 1;
    while starts.len() < wanted.max(1) {
        starts.push(Start::Random(opts.seed.wrapping_add(k)));
        k += 1;
    }
    let cancelled = || {
        opts.cancel
            .as_ref()
            .is_some_and(|c| c.load(Ordering::Relaxed))
    };
    let threads = std::thread::available_parallelism()
        .map_or(1, |n| n.get())
        .min(starts.len());
    let in_parallel =
        |count: usize, threads: usize, work: &(dyn Fn(usize) -> Option<Layout> + Sync)| {
            let mut out: Vec<Option<Layout>> = vec![None; count];
            let threads = threads.min(count).max(1);
            std::thread::scope(|s| {
                let handles: Vec<_> = (0..threads)
                    .map(|t| {
                        s.spawn(move || {
                            (t..count)
                                .step_by(threads)
                                .filter_map(|i| work(i).map(|l| (i, l)))
                                .collect::<Vec<_>>()
                        })
                    })
                    .collect();
                for h in handles {
                    for (i, l) in h.join().expect("layout thread panicked") {
                        out[i] = Some(l);
                    }
                }
            });
            out
        };

    // To fill the view: the layer width that gives the drawing its shape.
    let fill = opts.goal == Goal::FillView;
    let base = if fill {
        calibrated_width(graph, opts.aspect, &cancelled)
    } else {
        1.0
    };
    // Placing is cheap; routing is not, and grows with the drawing.
    let placed: Vec<Option<Layout>> = in_parallel(starts.len(), threads, &|i| {
        // The first candidate always runs.
        if i > 0 && cancelled() {
            return None;
        }
        let target = Target {
            width: if fill {
                width_for(i, base)
            } else {
                width_of(starts[i], opts.seed)
            },
            aspect: opts.aspect,
            fill: true,
            whole: fill,
            // Every other candidate from the third may turn parts sideways;
            // the fit to the view decides.
            turn: fill && i >= 2 && i % 2 == 0,
        };
        Some(Layout {
            nodes: place_rounds(graph, starts[i], target),
            ..Layout::default()
        })
    });
    // A large diagram routes one candidate only: the one whose straight
    // lines cross least. That keeps one router's tables in memory, not
    // one per candidate, and the time to that of a single routing.
    let large = graph.edges.len() > crate::router::LARGE_EDGES
        || placed
            .iter()
            .flatten()
            .any(|l| crate::router::grid_cells(graph, &l.nodes) > crate::router::LARGE_CELLS);
    let chosen: Vec<usize> = if large {
        // Straight crossings, and the fit of the boxes to the view, as the
        // routed candidates are weighed below.
        let straight: Vec<Option<usize>> = placed
            .iter()
            .map(|l| l.as_ref().map(|l| straight_crossings(graph, &l.nodes)))
            .collect();
        let cost = |i: usize| {
            let l = placed[i].as_ref()?;
            // Straight crossings, plus the lines so that none is free.
            let base = 10.0 * straight[i]? as f64 + graph.edges.len() as f64;
            Some(if fill {
                base * fit_factor(shape_of(graph, &l.nodes), opts.aspect)
            } else {
                base
            })
        };
        let best = (0..placed.len())
            .filter_map(|i| cost(i).map(|c| (c, i)))
            .min_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)))
            .map(|(_, i)| i)
            .unwrap_or(0);
        vec![best]
    } else {
        (0..placed.len()).filter(|&i| placed[i].is_some()).collect()
    };
    // As many route at once as fit in about 1.5 GB of routing tables.
    let cells = chosen
        .iter()
        .filter_map(|&i| placed[i].as_ref())
        .map(|l| crate::router::grid_cells(graph, &l.nodes))
        .max()
        .unwrap_or(0);
    let fit = MEMORY / (cells * crate::router::BYTES_PER_CELL).max(1);
    let results: Vec<Option<Layout>> = in_parallel(chosen.len(), threads.min(fit.max(1)), &|k| {
        let i = chosen[k];
        if k > 0 && cancelled() {
            return None;
        }
        let nodes = placed[i].as_ref()?.nodes.clone();
        Some(finalize_with(graph, nodes, false))
    });
    // Candidates are compared with quick routing, and by how well they
    // fill the view; the winner is routed again with the thorough router,
    // which only ever lowers its cost.
    let results: Vec<Layout> = results.into_iter().flatten().collect();
    let cost = |l: &Layout| {
        if fill {
            let b = l.bounds(graph);
            let shape = b.w.max(1) as f64 / b.h.max(1) as f64;
            l.stats.score() * fit_factor(shape, opts.aspect)
        } else {
            l.stats.score()
        }
    };
    let best = results
        .into_iter()
        .enumerate()
        .min_by(|(i, a), (j, b)| cost(a).total_cmp(&cost(b)).then(i.cmp(j)))
        .map(|(_, l)| l)
        .unwrap_or_default();
    if cancelled() {
        return best;
    }
    let mut routes = best.routes.clone();
    polish(graph, &best.nodes, &mut routes);
    let labels = place_labels(graph, &best.nodes, &routes);
    let mut stats = measure(graph, &best.nodes, &routes);
    stats.draft = best.stats.draft;
    if stats.score() <= best.stats.score() {
        Layout {
            routes,
            labels,
            stats,
            ..best
        }
    } else {
        best
    }
}

/// Do segments `p1-p2` and `q1-q2` cross properly?
fn segments_cross(p1: (f64, f64), p2: (f64, f64), q1: (f64, f64), q2: (f64, f64)) -> bool {
    let orient = |a: (f64, f64), b: (f64, f64), c: (f64, f64)| {
        (b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0)
    };
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
        .filter(|(n, _)| {
            graph.nodes[*n].group.is_some() && graph.nodes[*n].group == graph.nodes[i].group
        })
        .map(|(_, r)| *r)
        .collect();
    let bounds = placed.iter().map(|(_, r)| *r).reduce(|a, b| a.union(&b));
    let anchor = if !nbrs.is_empty() {
        let s = nbrs.iter().fold((0.0, 0.0), |acc, (r, _)| {
            (acc.0 + center(r).0, acc.1 + center(r).1)
        });
        (s.0 / nbrs.len() as f64, s.1 / nbrs.len() as f64)
    } else if !mates.is_empty() {
        let s = mates.iter().fold((0.0, 0.0), |acc, r| {
            (acc.0 + center(r).0, acc.1 + center(r).1)
        });
        (s.0 / mates.len() as f64, s.1 / mates.len() as f64)
    } else if let Some(b) = bounds {
        (
            (b.right() + 2 * NODE_GAP) as f64 + w as f64 / 2.0,
            b.y as f64 + h as f64 / 2.0,
        )
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
    let current: Vec<(i32, i32)> = nodes
        .iter()
        .map(|p| p.unwrap_or((i32::MIN / 4, i32::MIN / 4)))
        .collect();
    let group_rects_now = group_rects(graph, &current);
    let foreign: Vec<Rect> = (0..graph.groups.len())
        .filter(|&g| {
            !graph.nodes[i]
                .group
                .is_some_and(|ng| graph.group_within(ng, g))
        })
        .filter(|&g| {
            (0..graph.nodes.len()).any(|n| {
                nodes[n].is_some()
                    && graph.nodes[n]
                        .group
                        .is_some_and(|ng| graph.group_within(ng, g))
            })
        })
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
                cands.extend([
                    (ax + t, ay - r),
                    (ax + t, ay + r),
                    (ax - r, ay + t),
                    (ax + r, ay + t),
                ]);
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
                cost += 10.0
                    * lines
                        .iter()
                        .filter(|(a, b)| segments_cross(cc, nc, *a, *b))
                        .count() as f64;
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

/// After `moved` nodes changed position (all nodes must have one): keeps
/// the other lines, re-routes the affected ones, and refreshes group
/// bounds, labels and stats. Fast enough to run on every drag frame.
pub fn update_after_move(graph: &Graph, prev: &Layout, moved: &[usize]) -> Layout {
    if prev.routes.len() != graph.edges.len() || graph.nodes.iter().any(|n| n.pos.is_none()) {
        return route_only(graph);
    }
    let nodes: Vec<(i32, i32)> = graph.nodes.iter().map(|n| n.pos.unwrap()).collect();
    let mut routes = prev.routes.clone();
    crate::router::reroute(graph, &nodes, &mut routes, moved);
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
        let kinds = [
            EdgeKind::Hierarchy,
            EdgeKind::Assoc,
            EdgeKind::Whole,
            EdgeKind::Dep,
        ];
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

    fn ghost(item: usize, side: GhostSide, key: f64) -> Ghost {
        Ghost {
            item,
            side,
            key,
            port: 0.5,
        }
    }

    /// Many loose boxes at the top level fill a view of the target shape
    /// instead of making one long row.
    #[test]
    fn loose_boxes_fill_the_view() {
        for aspect in [0.8, 1.6, 2.4] {
            let t = Target {
                aspect,
                ..Target::usual()
            };
            let pos = layout_scope(&[(14, 6); 30], &[], None, Start::DepthFirst, &[], t);
            let w = pos.iter().map(|p| p.0 + 14).max().unwrap() as f64;
            let h = pos.iter().map(|p| p.1 + 6).max().unwrap() as f64;
            assert!(
                (w / h / aspect).ln().abs() < 0.5,
                "aspect {aspect}: {w} x {h}"
            );
        }
    }

    /// One connected diagram (a base class with many subclasses, some with
    /// helpers) takes the shape of the view: its layers fold or spread to
    /// fit, as the choice of candidates weighs the fit.
    #[test]
    fn a_connected_diagram_fills_the_view() {
        for aspect in [1.0, 1.6, 2.4] {
            let mut g = Graph::default();
            let base = g.add_node(16, 6);
            for k in 0..24 {
                let child = g.add_node(14, 6);
                g.add_edge(child, base, EdgeKind::Hierarchy);
                if k % 3 == 0 {
                    let helper = g.add_node(12, 4);
                    g.add_edge(child, helper, EdgeKind::Assoc);
                }
            }
            let opts = Options {
                aspect,
                goal: Goal::FillView,
                ..fast()
            };
            let l = layout_all(&g, &opts);
            let b = l.bounds(&g);
            let shape = b.w as f64 / b.h as f64;
            assert!(
                misfit(shape, aspect) < 0.4,
                "aspect {aspect}: {} x {}",
                b.w,
                b.h
            );
        }
    }

    /// Many loose parts in a package make a block, not a strip.
    #[test]
    fn many_loose_parts_in_a_package_are_not_a_strip() {
        let ghosts: Vec<Ghost> = (0..40).map(|i| ghost(i, GhostSide::Top, 50.0)).collect();
        let t = Target {
            fill: false,
            ..Target::usual()
        };
        let pos = layout_scope(&[(30, 4); 40], &[], None, Start::DepthFirst, &ghosts, t);
        let w = pos.iter().map(|p| p.0 + 30).max().unwrap() as f64;
        let h = pos.iter().map(|p| p.1 + 4).max().unwrap() as f64;
        assert!(w / h < STRIP, "{w} x {h}");
    }

    /// Parts of a package that are not connected to each other are placed
    /// left to right by where their outside neighbours are.
    #[test]
    fn package_parts_face_their_outside_neighbours() {
        let ghosts = [
            ghost(0, GhostSide::Top, 100.0),
            ghost(1, GhostSide::Top, 0.0),
        ];
        let pos = layout_scope(
            &[(10, 4); 2],
            &[],
            None,
            Start::DepthFirst,
            &ghosts,
            Target {
                fill: false,
                ..Target::usual()
            },
        );
        assert!(pos[1].0 < pos[0].0, "{pos:?}");
    }

    /// Children of one parent inside a package are ordered to match the
    /// outside boxes their lines go to.
    #[test]
    fn package_members_face_their_outside_neighbours() {
        let edges = [
            FlatEdge::new(1, 0, EdgeKind::Hierarchy, 0),
            FlatEdge::new(2, 0, EdgeKind::Hierarchy, 0),
        ];
        let ghosts = [
            ghost(1, GhostSide::Bottom, 100.0),
            ghost(2, GhostSide::Bottom, 0.0),
        ];
        let pos = layout_scope(
            &[(10, 4); 3],
            &edges,
            None,
            Start::DepthFirst,
            &ghosts,
            Target {
                fill: false,
                ..Target::usual()
            },
        );
        assert!(pos[2].0 < pos[1].0, "{pos:?}");
    }

    /// Lines from package members to outside boxes do not cross when an
    /// order without crossings exists.
    #[test]
    fn lines_leaving_a_package_do_not_cross() {
        let mut g = Graph::default();
        g.groups.push(Group {
            parent: None,
            header: 3,
            min_w: 10,
        });
        let p = g.add_node(12, 5);
        let a = g.add_node(12, 5);
        let b = g.add_node(12, 5);
        for n in [p, a, b] {
            g.nodes[n].group = Some(0);
        }
        g.add_edge(a, p, EdgeKind::Hierarchy);
        g.add_edge(b, p, EdgeKind::Hierarchy);
        // Outside the package: x is a child of b, y of a.
        let x = g.add_node(12, 5);
        let y = g.add_node(12, 5);
        g.add_edge(x, b, EdgeKind::Hierarchy);
        g.add_edge(y, a, EdgeKind::Hierarchy);
        let l = layout_all(&g, &Options::default());
        assert_eq!(crossing_points(&g, &l.routes).len(), 0);
        // Each outside child sits under its parent's half of the package.
        let cx = |n: usize| l.nodes[n].0 + g.nodes[n].w / 2;
        assert_eq!(cx(x) < cx(y), cx(b) < cx(a), "{:?}", l.nodes);
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
        assert!(
            l.nodes[c].1 >= 4 + 2,
            "child below parent: {:?}",
            l.nodes[c]
        );
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
