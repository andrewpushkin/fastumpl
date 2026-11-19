//! The flat shape list both the screen and SVG export paint.

use umpl_core::*;
use umpl_layout::crossing_points;

use crate::diagram::{Diagram, NodeRef};
use crate::measure::*;
use crate::theme::{Color, Theme};
use crate::GRID;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RectF {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl RectF {
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> RectF {
        RectF { x, y, w, h }
    }

    pub fn right(&self) -> f32 {
        self.x + self.w
    }

    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }

    pub fn center(&self) -> (f32, f32) {
        (self.x + self.w / 2.0, self.y + self.h / 2.0)
    }

    pub fn contains(&self, p: (f32, f32)) -> bool {
        p.0 >= self.x && p.0 <= self.right() && p.1 >= self.y && p.1 <= self.bottom()
    }

    pub fn intersects(&self, o: &RectF) -> bool {
        self.x < o.right() && o.x < self.right() && self.y < o.bottom() && o.y < self.bottom()
    }

    pub fn union(&self, o: &RectF) -> RectF {
        let x = self.x.min(o.x);
        let y = self.y.min(o.y);
        RectF::new(
            x,
            y,
            self.right().max(o.right()) - x,
            self.bottom().max(o.bottom()) - y,
        )
    }

    pub fn expand(&self, by: f32) -> RectF {
        RectF::new(
            self.x - by,
            self.y - by,
            self.w + 2.0 * by,
            self.h + 2.0 * by,
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextStyle {
    pub size: f32,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub color: Color,
    /// Centred on `pos.0` instead of starting there.
    pub centered: bool,
    /// The width (in diagram pixels) the text must stay within, inside a
    /// box: drawn smaller if a font would make it wider.
    pub fit: Option<f32>,
}

impl TextStyle {
    fn new(size: f32, color: Color) -> TextStyle {
        TextStyle {
            size,
            bold: false,
            italic: false,
            underline: false,
            color,
            centered: false,
            fit: None,
        }
    }

    fn fit(mut self, width: f32) -> TextStyle {
        self.fit = Some(width.max(1.0));
        self
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Shape {
    Rect {
        rect: RectF,
        fill: Option<Color>,
        stroke: Option<(Color, f32)>,
        radius: f32,
    },
    Path {
        points: Vec<(f32, f32)>,
        stroke: Option<(Color, f32)>,
        fill: Option<Color>,
        closed: bool,
        dashed: bool,
    },
    Circle {
        center: (f32, f32),
        radius: f32,
        fill: Color,
    },
    /// `pos.1` is the vertical centre of the line of text.
    Text {
        pos: (f32, f32),
        text: String,
        style: TextStyle,
    },
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SceneOptions<'a> {
    /// Draw names only (far zoomed out).
    pub hide_members: bool,
    pub selected_elements: &'a [ElementId],
    pub selected_relations: &'a [RelationId],
    pub selected_packages: &'a [PackageId],
}

#[derive(Clone, Debug, Default)]
pub struct Scene {
    pub shapes: Vec<Shape>,
    /// Everything drawn, with a margin.
    pub bounds: RectF,
    pub elements: Vec<(ElementId, RectF)>,
    /// Full line paths, for hit testing.
    pub relations: Vec<(RelationId, Vec<(f32, f32)>)>,
    pub packages: Vec<(PackageId, RectF)>,
}

const LINE_W: f32 = 1.5;
const BOX_W: f32 = 1.5;
const CORNER: f32 = 6.0;
const GAP: f32 = 4.0;
const TRIANGLE: f32 = 12.0;
const DIAMOND: f32 = 16.0;
const ARROW: f32 = 10.0;
/// Diameter of the circled plus at the outer end of a nesting line.
const NEST: f32 = 12.0;
const TAB_H: f32 = 20.0;

fn px(p: (i32, i32)) -> (f32, f32) {
    (p.0 as f32 * GRID, p.1 as f32 * GRID)
}

fn unit(a: (f32, f32), b: (f32, f32)) -> (f32, f32) {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len = (dx * dx + dy * dy).sqrt().max(1e-6);
    (dx / len, dy / len)
}

/// The part of `pts` up to where it first enters `r`; all of it if it never
/// does, or starts inside.
fn until_inside(pts: &[(f32, f32)], r: &RectF) -> Vec<(f32, f32)> {
    let inside = |p: (f32, f32)| p.0 > r.x && p.0 < r.right() && p.1 > r.y && p.1 < r.y + r.h;
    if pts.first().is_none_or(|p| inside(*p)) {
        return pts.to_vec();
    }
    let mut out = vec![pts[0]];
    for w in pts.windows(2) {
        let (a, b) = (w[0], w[1]);
        // Where segment a→b first meets the rectangle (Liang–Barsky).
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let mut t0: f32 = 0.0;
        let mut t1: f32 = 1.0;
        let mut meets = true;
        for (p, q) in [
            (-dx, a.0 - r.x),
            (dx, r.right() - a.0),
            (-dy, a.1 - r.y),
            (dy, r.y + r.h - a.1),
        ] {
            if p == 0.0 {
                if q < 0.0 {
                    meets = false;
                }
            } else {
                let t = q / p;
                if p < 0.0 {
                    t0 = t0.max(t);
                } else {
                    t1 = t1.min(t);
                }
            }
        }
        if meets && t0 <= t1 && (t0 > 0.0 || inside(b)) {
            out.push((a.0 + dx * t0, a.1 + dy * t0));
            return out;
        }
        out.push(b);
    }
    out
}

fn dist(a: (f32, f32), b: (f32, f32)) -> f32 {
    ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt()
}

/// Replaces each corner with a short flattened curve of radius `r`.
pub fn round_corners(points: &[(f32, f32)], r: f32) -> Vec<(f32, f32)> {
    if points.len() < 3 {
        return points.to_vec();
    }
    let mut out = vec![points[0]];
    for i in 1..points.len() - 1 {
        let (a, p, b) = (points[i - 1], points[i], points[i + 1]);
        let rr = r.min(dist(a, p) / 2.0).min(dist(p, b) / 2.0);
        let u1 = unit(p, a);
        let u2 = unit(p, b);
        let s = (p.0 + u1.0 * rr, p.1 + u1.1 * rr);
        let e = (p.0 + u2.0 * rr, p.1 + u2.1 * rr);
        // Quadratic Bézier from s to e with control point p.
        for k in 0..=4 {
            let t = k as f32 / 4.0;
            let mt = 1.0 - t;
            out.push((
                mt * mt * s.0 + 2.0 * mt * t * p.0 + t * t * e.0,
                mt * mt * s.1 + 2.0 * mt * t * p.1 + t * t * e.1,
            ));
        }
    }
    out.push(*points.last().unwrap());
    out
}

/// Splits a polyline at `gaps` (points on its vertical segments), leaving
/// `GAP` pixels free on each side.
pub fn cut_gaps(points: &[(f32, f32)], gaps: &[(f32, f32)]) -> Vec<Vec<(f32, f32)>> {
    let mut pieces = vec![vec![points[0]]];
    for w in points.windows(2) {
        let (a, b) = (w[0], w[1]);
        let mut here: Vec<(f32, f32)> = gaps
            .iter()
            .copied()
            .filter(|g| {
                (a.0 - b.0).abs() < 0.01
                    && (g.0 - a.0).abs() < 0.01
                    && g.1 > a.1.min(b.1) + GAP
                    && g.1 < a.1.max(b.1) - GAP
            })
            .collect();
        here.sort_by(|p, q| dist(a, *p).total_cmp(&dist(a, *q)));
        let u = unit(a, b);
        for g in here {
            pieces
                .last_mut()
                .unwrap()
                .push((g.0 - u.0 * GAP, g.1 - u.1 * GAP));
            pieces.push(vec![(g.0 + u.0 * GAP, g.1 + u.1 * GAP)]);
        }
        pieces.last_mut().unwrap().push(b);
    }
    pieces
}

/// Moves the first point `len` pixels along the first segment.
fn shorten_start(points: &mut [(f32, f32)], len: f32) {
    if points.len() >= 2 && dist(points[0], points[1]) > len {
        let u = unit(points[0], points[1]);
        points[0] = (points[0].0 + u.0 * len, points[0].1 + u.1 * len);
    }
}

fn shorten_end(points: &mut [(f32, f32)], len: f32) {
    let n = points.len();
    if n >= 2 && dist(points[n - 1], points[n - 2]) > len {
        let u = unit(points[n - 1], points[n - 2]);
        points[n - 1] = (points[n - 1].0 + u.0 * len, points[n - 1].1 + u.1 * len);
    }
}

fn kind_letter(k: ElementKind) -> &'static str {
    match k {
        ElementKind::Class => "C",
        ElementKind::Abstract => "A",
        ElementKind::Interface => "I",
        ElementKind::Enum => "E",
        ElementKind::Note | ElementKind::Diamond => "",
    }
}

/// Builds the shape list for `diagram` (whose layout must be current).
pub fn build_scene(doc: &Document, diagram: &Diagram, theme: &Theme, opts: &SceneOptions) -> Scene {
    let (graph, mapping, layout) = (&diagram.graph, &diagram.mapping, &diagram.layout);
    let mut s = Scene::default();

    // Packages, outermost first.
    for (gi, pid) in mapping.packages.iter().enumerate() {
        let Some(g) = layout.groups.get(gi) else {
            continue;
        };
        let Some(pkg) = doc.packages.get(pid) else {
            continue;
        };
        let r = RectF::new(
            g.x as f32 * GRID,
            g.y as f32 * GRID,
            g.w as f32 * GRID,
            g.h as f32 * GRID,
        );
        let selected = opts.selected_packages.contains(pid);
        let stroke = if selected {
            (theme.accent, 2.0)
        } else {
            (theme.package_stroke, 1.0)
        };
        let tab_w = (text_width(&pkg.name, 12.0) + 20.0).min(r.w);
        let fill = theme.user_fill(theme.package_fill, pkg.color.as_deref());
        // A transparent package is a frame: the tab sits on its top edge.
        let tab_h = if fill.is_some() { TAB_H + 4.0 } else { TAB_H };
        s.shapes.push(Shape::Rect {
            rect: RectF::new(r.x, r.y, tab_w, tab_h),
            fill,
            stroke: Some(stroke),
            radius: 3.0,
        });
        s.shapes.push(Shape::Rect {
            rect: RectF::new(r.x, r.y + TAB_H, r.w, r.h - TAB_H),
            fill,
            stroke: Some(stroke),
            radius: 4.0,
        });
        // Hide the tab's bottom border where it joins the body.
        if fill.is_some() {
            s.shapes.push(Shape::Rect {
                rect: RectF::new(r.x + 1.0, r.y + TAB_H - 1.0, tab_w - 2.0, 3.0),
                fill,
                stroke: None,
                radius: 0.0,
            });
        }
        let name_color = theme.text_on(fill, pkg.color.as_deref(), true);
        let mut st = TextStyle::new(12.0, name_color);
        st.bold = true;
        s.shapes.push(Shape::Text {
            pos: (r.x + 10.0, r.y + TAB_H / 2.0 + 1.0),
            text: pkg.name.clone(),
            style: st,
        });
        s.packages.push((*pid, r));
    }

    // Lines.
    let mut gaps: Vec<Vec<(f32, f32)>> = vec![Vec::new(); graph.edges.len()];
    for (p, a, b) in crossing_points(graph, &layout.routes) {
        let vertical_in = |e: usize| {
            layout.routes[e]
                .segments()
                .any(|(u, v)| u.0 == v.0 && u.0 == p.0 && p.1 > u.1.min(v.1) && p.1 < u.1.max(v.1))
        };
        let e = if vertical_in(a) { a } else { b };
        gaps[e].push(px(p));
    }
    let mut markers = Vec::new();
    for (e, rid) in mapping.relations.iter().enumerate() {
        let Some(rel) = doc.relations.get(rid) else {
            continue;
        };
        let Some(route) = layout.routes.get(e) else {
            continue;
        };
        if route.points.len() < 2 {
            continue;
        }
        let mut full: Vec<(f32, f32)> = route.points.iter().map(|p| px(*p)).collect();
        // A line to a package ends at its frame.
        let frame = |t: Endpoint| match t {
            Endpoint::Package(p) => s.packages.iter().find(|(q, _)| *q == p).map(|(_, r)| *r),
            Endpoint::Element(_) => None,
        };
        if let Some(r) = frame(rel.to.target) {
            full = until_inside(&full, &r);
        }
        if let Some(r) = frame(rel.from.target) {
            full.reverse();
            full = until_inside(&full, &r);
            full.reverse();
        }
        if full.len() < 2 {
            continue;
        }
        s.relations.push((*rid, full.clone()));
        let selected = opts.selected_relations.contains(rid);
        let color = if selected { theme.accent } else { theme.line };
        let width = if selected { 2.5 } else { LINE_W };

        let mut pts = full.clone();
        match rel.kind {
            RelationKind::Generalization | RelationKind::Realization => {
                shorten_end(&mut pts, TRIANGLE)
            }
            _ => {}
        }
        if matches!(
            rel.kind,
            RelationKind::Composition | RelationKind::Aggregation
        ) {
            shorten_start(&mut pts, DIAMOND);
        }
        if rel.kind == RelationKind::Nesting {
            shorten_start(&mut pts, NEST);
        }
        for piece in cut_gaps(&pts, &gaps[e]) {
            s.shapes.push(Shape::Path {
                points: round_corners(&piece, CORNER),
                stroke: Some((color, width)),
                fill: None,
                closed: false,
                dashed: rel.kind.is_dashed(),
            });
        }

        let n = full.len();
        let tip = full[n - 1];
        let u = unit(full[n - 2], tip);
        let nrm = (-u.1, u.0);
        match rel.kind {
            RelationKind::Generalization | RelationKind::Realization => {
                let base = (tip.0 - u.0 * TRIANGLE, tip.1 - u.1 * TRIANGLE);
                markers.push(Shape::Path {
                    points: vec![
                        tip,
                        (base.0 + nrm.0 * 7.0, base.1 + nrm.1 * 7.0),
                        (base.0 - nrm.0 * 7.0, base.1 - nrm.1 * 7.0),
                    ],
                    stroke: Some((color, width)),
                    fill: Some(theme.box_fill),
                    closed: true,
                    dashed: false,
                });
            }
            RelationKind::Directed | RelationKind::Dependency => {
                let base = (tip.0 - u.0 * ARROW, tip.1 - u.1 * ARROW);
                markers.push(Shape::Path {
                    points: vec![
                        (base.0 + nrm.0 * 5.0, base.1 + nrm.1 * 5.0),
                        tip,
                        (base.0 - nrm.0 * 5.0, base.1 - nrm.1 * 5.0),
                    ],
                    stroke: Some((color, width)),
                    fill: None,
                    closed: false,
                    dashed: false,
                });
            }
            _ => {}
        }
        if matches!(
            rel.kind,
            RelationKind::Composition | RelationKind::Aggregation
        ) {
            let f = full[0];
            let v = unit(full[1], f);
            let m = (-v.1, v.0);
            let mid = (f.0 - v.0 * DIAMOND / 2.0, f.1 - v.1 * DIAMOND / 2.0);
            let fill = if rel.kind == RelationKind::Composition {
                color
            } else {
                theme.box_fill
            };
            markers.push(Shape::Path {
                points: vec![
                    f,
                    (mid.0 + m.0 * 5.5, mid.1 + m.1 * 5.5),
                    (f.0 - v.0 * DIAMOND, f.1 - v.1 * DIAMOND),
                    (mid.0 - m.0 * 5.5, mid.1 - m.1 * 5.5),
                ],
                stroke: Some((color, width)),
                fill: Some(fill),
                closed: true,
                dashed: false,
            });
        }

        if rel.kind == RelationKind::Nesting {
            // A circle with a plus in it, touching the outer class.
            let f = full[0];
            let v = unit(full[1], f);
            let r = NEST / 2.0;
            let c = (f.0 - v.0 * r, f.1 - v.1 * r);
            let ring: Vec<(f32, f32)> = (0..24)
                .map(|k| {
                    let a = k as f32 * std::f32::consts::TAU / 24.0;
                    (c.0 + r * a.cos(), c.1 + r * a.sin())
                })
                .collect();
            markers.push(Shape::Path {
                points: ring,
                stroke: Some((color, width)),
                fill: Some(theme.box_fill),
                closed: true,
                dashed: false,
            });
            for (dx, dy) in [(r, 0.0), (0.0, r)] {
                markers.push(Shape::Path {
                    points: vec![(c.0 - dx, c.1 - dy), (c.0 + dx, c.1 + dy)],
                    stroke: Some((color, width)),
                    fill: None,
                    closed: false,
                    dashed: false,
                });
            }
        }

        // Labels.
        let texts = [
            rel.from.label.as_ref(),
            rel.to.label.as_ref(),
            rel.name.as_ref(),
        ];
        for (k, text) in texts.iter().enumerate() {
            let (Some(text), Some(Some(p))) = (text, layout.labels.get(e).map(|l| l[k])) else {
                continue;
            };
            let (x, y) = px(p);
            let mut st = TextStyle::new(LABEL_SIZE, if k == 2 { theme.text } else { theme.muted });
            st.italic = k == 2;
            s.shapes.push(Shape::Text {
                pos: (x + 2.0, y + GRID),
                text: (*text).clone(),
                style: st,
            });
        }
    }
    s.shapes.extend(markers);

    // Boxes.
    for (i, nref) in mapping.nodes.iter().enumerate() {
        let NodeRef::Element(id) = nref else { continue };
        let Some(Some(content)) = mapping.contents.get(i) else {
            continue;
        };
        let Some(&p) = layout.nodes.get(i) else {
            continue;
        };
        let (x, y) = px(p);
        let r = RectF::new(
            x,
            y,
            graph.nodes[i].w as f32 * GRID,
            graph.nodes[i].h as f32 * GRID,
        );
        s.elements.push((*id, r));
        draw_box(&mut s.shapes, content, r, theme, opts.hide_members);
        if opts.selected_elements.contains(id) {
            s.shapes.push(Shape::Rect {
                rect: r.expand(3.0),
                fill: None,
                stroke: Some((theme.accent, 2.0)),
                radius: 6.0,
            });
        }
    }

    let b = layout.bounds(graph);
    s.bounds = RectF::new(
        b.x as f32 * GRID,
        b.y as f32 * GRID,
        b.w as f32 * GRID,
        b.h as f32 * GRID,
    )
    .expand(16.0);
    s
}

fn draw_box(shapes: &mut Vec<Shape>, c: &BoxContent, r: RectF, theme: &Theme, hide_members: bool) {
    if c.kind == ElementKind::Diamond {
        let (cx, cy) = (r.x + r.w / 2.0, r.y + r.h / 2.0);
        shapes.push(Shape::Path {
            points: vec![(cx, r.y), (r.right(), cy), (cx, r.bottom()), (r.x, cy)],
            stroke: Some((theme.box_stroke, BOX_W)),
            fill: Some(theme.box_fill),
            closed: true,
            dashed: false,
        });
        return;
    }
    if c.kind == ElementKind::Note {
        let fold = 10.0;
        shapes.push(Shape::Path {
            points: vec![
                (r.x, r.y),
                (r.right() - fold, r.y),
                (r.right(), r.y + fold),
                (r.right(), r.bottom()),
                (r.x, r.bottom()),
            ],
            stroke: Some((theme.note_stroke, 1.2)),
            fill: theme.user_fill(theme.note_fill, c.color.as_deref()),
            closed: true,
            dashed: false,
        });
        shapes.push(Shape::Path {
            points: vec![
                (r.right() - fold, r.y),
                (r.right() - fold, r.y + fold),
                (r.right(), r.y + fold),
            ],
            stroke: Some((theme.note_stroke, 1.2)),
            fill: None,
            closed: false,
            dashed: false,
        });
        let fill = theme.user_fill(theme.note_fill, c.color.as_deref());
        let color = theme.text_on(fill, c.color.as_deref(), false);
        if let Some(lines) = c.compartments.first() {
            for (k, l) in lines.iter().enumerate() {
                shapes.push(Shape::Text {
                    pos: (
                        r.x + NOTE_PAD,
                        r.y + NOTE_PAD + MEMBER_LINE * (k as f32 + 0.5),
                    ),
                    text: l.text.clone(),
                    style: TextStyle::new(MEMBER_SIZE, color).fit(r.w - 2.0 * NOTE_PAD - 4.0),
                });
            }
        }
        return;
    }

    let fill = theme.user_fill(theme.fill(c.kind), c.color.as_deref());
    let text = theme.text_on(fill, c.color.as_deref(), false);
    let muted = theme.text_on(fill, c.color.as_deref(), true);
    shapes.push(Shape::Rect {
        rect: r,
        fill,
        stroke: Some((theme.box_stroke, BOX_W)),
        radius: 4.0,
    });
    let mut y = r.y + PAD_Y;
    if let Some(st) = &c.stereotype {
        let mut style = TextStyle::new(STEREO_SIZE, muted).fit(r.w - 2.0 * PAD_X);
        style.centered = true;
        shapes.push(Shape::Text {
            pos: (r.x + r.w / 2.0, y + STEREO_LINE / 2.0),
            text: format!("«{st}»"),
            style,
        });
        y += STEREO_LINE;
    }
    let name_cy = y + NAME_LINE / 2.0;
    shapes.push(Shape::Circle {
        center: (r.x + 15.0, name_cy),
        radius: 7.5,
        fill: theme.mark(c.kind),
    });
    let mut badge = TextStyle::new(10.0, theme.mark_text);
    badge.bold = true;
    badge.centered = true;
    shapes.push(Shape::Text {
        pos: (r.x + 15.0, name_cy),
        text: kind_letter(c.kind).into(),
        style: badge,
    });
    let mut name = TextStyle::new(NAME_SIZE, text).fit(r.w - 2.0 * BADGE_ROOM);
    name.bold = true;
    name.italic = c.italic_name;
    name.centered = true;
    shapes.push(Shape::Text {
        pos: (r.x + r.w / 2.0, name_cy),
        text: c.name.clone(),
        style: name,
    });
    y += NAME_LINE + PAD_Y;

    for comp in &c.compartments {
        shapes.push(Shape::Path {
            points: vec![(r.x, y), (r.right(), y)],
            stroke: Some((theme.box_stroke, 1.0)),
            fill: None,
            closed: false,
            dashed: false,
        });
        y += 4.0;
        for l in comp {
            if !hide_members {
                let mut st = TextStyle::new(MEMBER_SIZE, text).fit(r.w - 2.0 * PAD_X);
                st.italic = l.italic;
                st.underline = l.underline;
                shapes.push(Shape::Text {
                    pos: (r.x + PAD_X, y + MEMBER_LINE / 2.0),
                    text: l.text.clone(),
                    style: st,
                });
            }
            y += MEMBER_LINE;
        }
        y += 4.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shop() -> (Document, Diagram) {
        let p = umpl_text::parse(include_str!("../../../examples/shop.umpl"));
        let d = Diagram::arrange(&p.doc);
        (p.doc, d)
    }

    #[test]
    fn lines_to_packages_end_at_the_frame() {
        let r = RectF::new(10.0, 10.0, 20.0, 20.0);
        // Into the frame from the left, then on to a box inside.
        let pts = [(0.0, 15.0), (20.0, 15.0), (20.0, 25.0)];
        assert_eq!(until_inside(&pts, &r), [(0.0, 15.0), (10.0, 15.0)]);
        // From above, after a bend outside.
        let pts = [(0.0, 0.0), (15.0, 0.0), (15.0, 20.0)];
        assert_eq!(
            until_inside(&pts, &r),
            [(0.0, 0.0), (15.0, 0.0), (15.0, 10.0)]
        );
        // Never in, or starting inside: unchanged.
        let pts = [(0.0, 0.0), (40.0, 0.0)];
        assert_eq!(until_inside(&pts, &r), pts);
        let pts = [(15.0, 15.0), (40.0, 15.0)];
        assert_eq!(until_inside(&pts, &r), pts);

        let p = umpl_text::parse("package shop {\n  class Order\n  class Line\n}\npackage web {}\nclass Client\nClient --> shop\nweb ..> shop\n");
        let d = Diagram::tidy(&p.doc, &umpl_layout::Options::default());
        let s = build_scene(&p.doc, &d, &Theme::light(), &SceneOptions::default());
        let shop = s
            .packages
            .iter()
            .find(|(q, _)| p.doc.packages[q].name == "shop")
            .unwrap()
            .1;
        assert_eq!(s.relations.len(), 2);
        for (_, pts) in &s.relations {
            // Each line stops on the frame of shop, not inside it.
            let end = *pts.last().unwrap();
            let on_frame = (end.0 - shop.x).abs() < 0.5
                || (end.0 - shop.right()).abs() < 0.5
                || (end.1 - shop.y).abs() < 0.5
                || (end.1 - shop.bottom()).abs() < 0.5;
            assert!(on_frame, "{end:?} not on {shop:?}");
        }
    }

    #[test]
    fn scene_has_one_rect_per_element() {
        let (doc, d) = shop();
        let s = build_scene(&doc, &d, &Theme::light(), &SceneOptions::default());
        assert_eq!(s.elements.len(), doc.elements.len());
        assert_eq!(s.relations.len(), doc.relations.len());
        assert_eq!(s.packages.len(), 1);
        for (_, r) in &s.elements {
            assert!(s.bounds.contains((r.x, r.y)) && s.bounds.contains((r.right(), r.bottom())));
        }
    }

    /// Each kind of box gets its own pale, opaque fill, tinted with the
    /// colour of its kind, in both themes.
    #[test]
    fn boxes_are_tinted_by_kind() {
        let (doc, d) = shop();
        for theme in [Theme::light(), Theme::dark()] {
            let s = build_scene(&doc, &d, &theme, &SceneOptions::default());
            let mut fills = std::collections::HashSet::new();
            for (id, r) in &s.elements {
                let fill = s
                    .shapes
                    .iter()
                    .find_map(|sh| match sh {
                        Shape::Rect { rect, fill, .. } if rect == r => *fill,
                        _ => None,
                    })
                    .unwrap();
                assert_eq!(fill, theme.fill(doc.elements[id].kind));
                fills.insert((fill.0, fill.1, fill.2, fill.3));
            }
            assert_eq!(fills.len(), 4, "class, abstract, interface, enum");
            for f in fills {
                assert_eq!(f.3, 255, "opaque");
                let dist = |a: u8, b: u8| (a as i32 - b as i32).abs();
                let b = theme.box_fill;
                let near = dist(f.0, b.0).max(dist(f.1, b.1)).max(dist(f.2, b.2));
                assert!((4..=40).contains(&near), "pale tint: {f:?} vs {b:?}");
            }
        }
    }

    #[test]
    fn gap_cut_at_crossing() {
        let pieces = cut_gaps(&[(0.0, 0.0), (0.0, 100.0), (50.0, 100.0)], &[(0.0, 40.0)]);
        assert_eq!(pieces.len(), 2);
        assert_eq!(pieces[0], vec![(0.0, 0.0), (0.0, 36.0)]);
        assert_eq!(pieces[1], vec![(0.0, 44.0), (0.0, 100.0), (50.0, 100.0)]);
    }

    #[test]
    fn rounded_corner_stays_near_corner() {
        let out = round_corners(&[(0.0, 0.0), (0.0, 50.0), (50.0, 50.0)], 6.0);
        assert_eq!(out.first(), Some(&(0.0, 0.0)));
        assert_eq!(out.last(), Some(&(50.0, 50.0)));
        assert!(out.iter().all(|p| p.0 >= 0.0 && p.1 <= 50.0));
    }
}
