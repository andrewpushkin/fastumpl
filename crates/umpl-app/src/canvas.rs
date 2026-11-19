//! The diagram canvas: painting, selection, moving, connecting, creating
//! and editing boxes in place.

use egui::{
    Align2, Color32, CursorIcon, FontId, Id, Key, Order, PointerButton, Pos2, Rect, Response,
    Sense, Shape, Stroke, StrokeKind, Ui, Vec2,
};
use umpl_core::{
    Element, ElementId, ElementKind, Member, PackageId, Pos, RelationId, RelationKind,
};
use umpl_draw::{RectF, GRID};

use crate::app::App;
use crate::commands::Tool;
use crate::paint::{c32, paint_scene};
use crate::view::Selection;

/// What is under the pointer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Hit {
    Handle(ElementId),
    Element(ElementId),
    Relation(RelationId),
    Package(PackageId),
    Empty,
}

pub enum Drag {
    Move {
        start: (f32, f32),
        origin: Vec<(ElementId, Pos)>,
        last: (i32, i32),
    },
    Rubber {
        start: (f32, f32),
        current: (f32, f32),
        additive: bool,
    },
    Connect {
        from: ElementId,
        current: (f32, f32),
    },
}

/// Editing a box's name and members over the canvas.
pub struct InlineEdit {
    pub id: ElementId,
    pub name: String,
    pub body: String,
    pub error: Option<String>,
    pub focus: bool,
}

/// Choosing the kind of a new relation.
pub struct ConnectMenu {
    pub from: ElementId,
    pub to: Option<ElementId>,
    pub at: (f32, f32),
}

pub struct CanvasState {
    pub rect: Rect,
    pub drag: Option<Drag>,
    pub edit: Option<InlineEdit>,
    pub connect: Option<ConnectMenu>,
    hover: Option<ElementId>,
    /// The scroll bar thumbs as last painted: horizontal, then vertical.
    pub scroll_thumbs: [Option<Rect>; 2],
    /// The scroll bar thumb being dragged.
    pub scroll_drag: Option<crate::scrollbar::ScrollDrag>,
}

impl Default for CanvasState {
    fn default() -> Self {
        CanvasState {
            rect: Rect::NOTHING,
            drag: None,
            edit: None,
            connect: None,
            hover: None,
            scroll_thumbs: [None, None],
            scroll_drag: None,
        }
    }
}

fn toggle<T: PartialEq>(v: &mut Vec<T>, x: T) {
    if let Some(i) = v.iter().position(|y| *y == x) {
        v.remove(i);
    } else {
        v.push(x);
    }
}

impl CanvasState {
    pub fn busy(&self) -> bool {
        self.drag.is_some() || self.edit.is_some() || self.connect.is_some()
    }

    pub fn cancel(&mut self) {
        self.drag = None;
        self.edit = None;
        self.connect = None;
    }
}

/// Geometry needed for hit testing, copied out of the scene.
struct Geometry {
    elements: Vec<(ElementId, RectF)>,
    relations: Vec<(RelationId, Vec<(f32, f32)>)>,
    packages: Vec<(PackageId, RectF)>,
}

fn dist_to_segment(p: (f32, f32), a: (f32, f32), b: (f32, f32)) -> f32 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len2 = dx * dx + dy * dy;
    let t = if len2 == 0.0 {
        0.0
    } else {
        (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / len2).clamp(0.0, 1.0)
    };
    let (x, y) = (a.0 + t * dx, a.1 + t * dy);
    ((p.0 - x).powi(2) + (p.1 - y).powi(2)).sqrt()
}

/// Edge midpoints where a new line can be dragged out of a box.
pub fn handles(r: RectF) -> [(f32, f32); 4] {
    let (cx, cy) = r.center();
    [(cx, r.y), (r.right(), cy), (cx, r.bottom()), (r.x, cy)]
}

/// Text for the inline member editor.
pub fn members_text(el: &Element) -> String {
    el.members
        .iter()
        .map(|m| {
            if el.kind == ElementKind::Note || el.kind == ElementKind::Enum {
                m.text.clone()
            } else {
                m.to_line()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Members from the inline editor's text.
pub fn parse_members(kind: ElementKind, text: &str) -> Vec<Member> {
    match kind {
        ElementKind::Note => text.lines().map(|l| Member::plain(l.trim_end())).collect(),
        ElementKind::Enum => text
            .split(['\n', ','])
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(Member::plain)
            .collect(),
        _ => text
            .lines()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(Member::parse_line)
            .collect(),
    }
}

impl App {
    fn geometry(&self) -> Geometry {
        // Scene geometry from the layout without building shapes.
        let d = &self.view.diagram;
        let mut g = Geometry {
            elements: Vec::new(),
            relations: Vec::new(),
            packages: Vec::new(),
        };
        for (i, r) in d.mapping.nodes.iter().enumerate() {
            if let (umpl_draw::NodeRef::Element(id), Some(p)) = (r, d.layout.nodes.get(i)) {
                let n = &d.graph.nodes[i];
                g.elements.push((
                    *id,
                    RectF::new(
                        p.0 as f32 * GRID,
                        p.1 as f32 * GRID,
                        n.w as f32 * GRID,
                        n.h as f32 * GRID,
                    ),
                ));
            }
        }
        for (e, id) in d.mapping.relations.iter().enumerate() {
            if let Some(route) = d.layout.routes.get(e) {
                g.relations.push((
                    *id,
                    route
                        .points
                        .iter()
                        .map(|p| (p.0 as f32 * GRID, p.1 as f32 * GRID))
                        .collect(),
                ));
            }
        }
        for (gi, id) in d.mapping.packages.iter().enumerate() {
            if let Some(r) = d.layout.groups.get(gi) {
                g.packages.push((
                    *id,
                    RectF::new(
                        r.x as f32 * GRID,
                        r.y as f32 * GRID,
                        r.w as f32 * GRID,
                        r.h as f32 * GRID,
                    ),
                ));
            }
        }
        g
    }

    /// Where `item` is drawn, in world pixels.
    pub fn item_rect(&self, item: umpl_text::Item) -> Option<RectF> {
        let d = &self.view.diagram;
        let grid = |x: i32, y: i32, w: i32, h: i32| {
            RectF::new(
                x as f32 * GRID,
                y as f32 * GRID,
                w as f32 * GRID,
                h as f32 * GRID,
            )
        };
        match item {
            umpl_text::Item::Element(id) => {
                let i = d.mapping.node_of(id)?;
                let p = d.layout.nodes.get(i)?;
                let n = &d.graph.nodes[i];
                Some(grid(p.0, p.1, n.w, n.h))
            }
            umpl_text::Item::Relation(id) => {
                let pts = &d.layout.routes.get(d.mapping.edge_of(id)?)?.points;
                let (x0, y0) = pts
                    .iter()
                    .fold((i32::MAX, i32::MAX), |a, p| (a.0.min(p.0), a.1.min(p.1)));
                let (x1, y1) = pts
                    .iter()
                    .fold((i32::MIN, i32::MIN), |a, p| (a.0.max(p.0), a.1.max(p.1)));
                (!pts.is_empty()).then(|| grid(x0, y0, x1 - x0, y1 - y0))
            }
            umpl_text::Item::Package(id) => {
                let g = d.mapping.packages.iter().position(|p| *p == id)?;
                let r = d.layout.groups.get(g)?;
                Some(grid(r.x, r.y, r.w, r.h))
            }
        }
    }

    /// Pans the canvas so `item` is in view, unless it already is.
    pub fn reveal(&mut self, item: umpl_text::Item) {
        let Some(r) = self.item_rect(item) else {
            return;
        };
        let view = self.canvas.rect;
        if view.width() < 10.0 {
            return;
        }
        let on_screen = self.camera.rect_to_screen(r);
        if view.shrink(8.0).contains_rect(on_screen) {
            return;
        }
        let target = view.center() - self.camera.to_screen(r.center());
        self.camera.pan(target);
    }

    fn hit(&self, geo: &Geometry, world: (f32, f32), with_handles: bool) -> Hit {
        let z = self.camera.zoom;
        // Any box's edge dots count, not only the hovered box's: by the
        // time a drag registers, the pointer has often left the box.
        if with_handles {
            for (id, r) in geo.elements.iter().rev() {
                for h in handles(*r) {
                    if ((h.0 - world.0).powi(2) + (h.1 - world.1).powi(2)).sqrt() * z < 8.0 {
                        return Hit::Handle(*id);
                    }
                }
            }
        }
        if let Some((id, _)) = geo.elements.iter().rev().find(|(_, r)| r.contains(world)) {
            return Hit::Element(*id);
        }
        let tol = 6.0 / z;
        for (id, pts) in &geo.relations {
            if pts
                .windows(2)
                .any(|w| dist_to_segment(world, w[0], w[1]) < tol)
            {
                return Hit::Relation(*id);
            }
        }
        // Packages are picked by their name tab or border, innermost first.
        for (id, r) in geo.packages.iter().rev() {
            let tab = RectF::new(r.x, r.y, r.w.min(160.0), 24.0);
            let near_border = r.expand(tol).contains(world) && !r.expand(-tol).contains(world);
            if tab.contains(world) || near_border {
                return Hit::Package(*id);
            }
        }
        Hit::Empty
    }

    fn select_hit(&mut self, hit: Hit, shift: bool) {
        match hit {
            Hit::Element(id) | Hit::Handle(id) => {
                if shift {
                    toggle(&mut self.sel.elements, id);
                } else {
                    self.sel = Selection::only_element(id);
                }
            }
            Hit::Relation(id) => {
                if shift {
                    toggle(&mut self.sel.relations, id);
                } else {
                    self.sel = Selection {
                        relations: vec![id],
                        ..Selection::default()
                    };
                }
            }
            Hit::Package(id) => {
                if shift {
                    toggle(&mut self.sel.packages, id);
                } else {
                    self.sel = Selection {
                        packages: vec![id],
                        ..Selection::default()
                    };
                }
            }
            Hit::Empty => {
                if !shift {
                    self.sel.clear();
                }
            }
        }
    }

    /// Adds an element centred on `world` and opens the inline editor.
    fn create_at(
        &mut self,
        kind: ElementKind,
        world: (f32, f32),
        pkg: Option<PackageId>,
    ) -> Option<ElementId> {
        let s = self.session.as_mut()?;
        let pos = Pos::new(
            (world.0 / GRID).round() as i32 - 5,
            (world.1 / GRID).round() as i32 - 2,
        );
        let base = match kind {
            ElementKind::Note => "Note",
            ElementKind::Interface => "Interface",
            ElementKind::Enum => "Enum",
            ElementKind::Abstract => "Abstract",
            ElementKind::Class => "Class",
        };
        let id = s.editor.add_element(kind, base, pkg, Some(pos));
        if kind == ElementKind::Note {
            let mut el = s.editor.doc.elements[&id].clone();
            el.members = vec![Member::plain("Note")];
            s.editor.amend(vec![umpl_core::Op::Element(id, Some(el))]);
        }
        self.sel = Selection::only_element(id);
        self.start_edit(id);
        Some(id)
    }

    pub fn start_edit(&mut self, id: ElementId) {
        let Some(s) = &self.session else { return };
        let Some(el) = s.editor.doc.elements.get(&id) else {
            return;
        };
        self.canvas.edit = Some(InlineEdit {
            id,
            name: el.name.clone(),
            body: members_text(el),
            error: None,
            focus: true,
        });
    }

    /// Applies the inline editor. Returns false (and keeps it open) when the
    /// name is not acceptable.
    fn commit_edit(&mut self) -> bool {
        let Some(edit) = &mut self.canvas.edit else {
            return true;
        };
        let Some(s) = &mut self.session else {
            return true;
        };
        let Some(el) = s.editor.doc.elements.get(&edit.id) else {
            self.canvas.edit = None;
            return true;
        };
        let name = edit.name.trim().to_string();
        if name != el.name {
            if !umpl_core::valid_name(&name) {
                edit.error = Some("Use letters, digits and _ (not starting with a digit)".into());
                return false;
            }
            let pkg = s.editor.doc.element_package(edit.id);
            if !s.editor.doc.name_is_free(&name, pkg) {
                edit.error = Some(format!("'{name}' already exists here"));
                return false;
            }
        }
        let mut new = el.clone();
        new.name = name;
        new.members = parse_members(el.kind, &edit.body);
        s.editor.set_element(edit.id, new);
        self.canvas.edit = None;
        true
    }

    fn connect(
        &mut self,
        from: ElementId,
        to: Option<ElementId>,
        at: (f32, f32),
        kind: RelationKind,
    ) {
        let to = match to {
            Some(t) => t,
            None => {
                let Some(s) = &mut self.session else { return };
                let pkg = s.editor.doc.element_package(from);
                let pos = Pos::new(
                    (at.0 / GRID).round() as i32 - 5,
                    (at.1 / GRID).round() as i32 - 2,
                );
                s.editor
                    .add_element(ElementKind::Class, "Class", pkg, Some(pos))
            }
        };
        let Some(s) = &mut self.session else { return };
        let r = s.editor.connect(from, to, kind);
        self.sel = Selection {
            relations: vec![r],
            ..Selection::default()
        };
    }

    pub fn canvas_ui(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let (rect, resp) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
        self.canvas.rect = rect;
        let theme = self.diagram_theme(&ctx);
        if self.session.is_none() {
            return;
        }

        // Camera: fit, zoom, pan.
        if self.fit_pending && rect.width() > 50.0 {
            let s = self.session.as_ref().unwrap();
            if s.editor.doc.elements.is_empty() && s.editor.doc.packages.is_empty() {
                // Nothing to fill the view with: start at 100 %.
                self.camera
                    .fit(umpl_draw::RectF::new(0.0, 0.0, 0.0, 0.0), rect);
            } else {
                let bounds = self.view.scene(&s.editor, &self.sel, &theme, false).bounds;
                self.camera.fit(bounds, rect);
            }
            self.fit_pending = false;
        }
        if resp.hovered() {
            let (zoom, scroll) = ctx.input(|i| (i.zoom_delta(), i.smooth_scroll_delta));
            if zoom != 1.0 {
                if let Some(p) = resp.hover_pos() {
                    self.camera.zoom_at(p, zoom);
                }
            } else if scroll != Vec2::ZERO {
                self.camera.pan(scroll);
            }
        }
        if resp.dragged_by(PointerButton::Middle) || resp.dragged_by(PointerButton::Secondary) {
            self.camera.pan(resp.drag_delta());
        }

        // Paint.
        let painter = ui.painter_at(rect);
        if self.show_grid {
            self.paint_grid(&painter, rect, c32(theme.grid));
        }
        let bounds = {
            let s = self.session.as_ref().unwrap();
            let hide = self.camera.zoom < 0.4;
            let scene = self.view.scene(&s.editor, &self.sel, &theme, hide);
            paint_scene(&painter, scene, &self.camera, rect);
            scene.bounds
        };

        let geo = self.geometry();
        let pointer_world = resp.hover_pos().map(|p| self.camera.to_world(p));
        let shift = ctx.input(|i| i.modifiers.shift);
        self.canvas.hover = pointer_world.and_then(|w| match self.hit(&geo, w, false) {
            Hit::Element(id) => Some(id),
            _ => None,
        });
        let hit_now =
            pointer_world.map_or(Hit::Empty, |w| self.hit(&geo, w, self.tool == Tool::Select));
        let accent = c32(theme.accent);

        // Cursor.
        match (&self.canvas.drag, hit_now, self.tool) {
            (Some(Drag::Move { .. }), _, _) => ctx.set_cursor_icon(CursorIcon::Grabbing),
            (Some(Drag::Connect { .. }), _, _) | (None, Hit::Handle(_), _) => {
                ctx.set_cursor_icon(CursorIcon::Crosshair)
            }
            (None, _, Tool::Element(_) | Tool::Package) => ctx.set_cursor_icon(CursorIcon::Cell),
            (None, Hit::Element(_), Tool::Relation) => ctx.set_cursor_icon(CursorIcon::Crosshair),
            (None, Hit::Element(_) | Hit::Package(_), Tool::Select) => {
                ctx.set_cursor_icon(CursorIcon::Grab)
            }
            _ => {}
        }

        self.handle_drag(&ctx, &resp, &geo, shift);
        self.handle_clicks(&resp, &geo, shift);

        // Overlays: handles, drag feedback, guides.
        if self.tool == Tool::Select && self.canvas.drag.is_none() && self.canvas.edit.is_none() {
            let show: Vec<ElementId> = self.canvas.hover.into_iter().collect();
            for id in show {
                if let Some((_, r)) = geo.elements.iter().find(|(e, _)| *e == id) {
                    for h in handles(*r) {
                        let p = self.camera.to_screen(h);
                        painter.circle(p, 4.5, c32(theme.box_fill), Stroke::new(1.5, accent));
                    }
                }
            }
        }
        match &self.canvas.drag {
            Some(Drag::Rubber { start, current, .. }) => {
                let r = Rect::from_two_pos(
                    self.camera.to_screen(*start),
                    self.camera.to_screen(*current),
                );
                painter.rect(
                    r,
                    2.0,
                    accent.gamma_multiply(0.12),
                    Stroke::new(1.0, accent),
                    StrokeKind::Inside,
                );
            }
            Some(Drag::Connect { from, current }) => {
                if let Some((_, r)) = geo.elements.iter().find(|(e, _)| e == from) {
                    let a = self.camera.to_screen(r.center());
                    let b = self.camera.to_screen(*current);
                    painter.extend(Shape::dashed_line(
                        &[a, b],
                        Stroke::new(1.5, accent),
                        6.0,
                        4.0,
                    ));
                    painter.circle_filled(b, 4.0, accent);
                }
            }
            Some(Drag::Move { .. }) => self.paint_guides(&painter, &geo, accent),
            None => {}
        }

        self.paint_found(&painter);
        self.scroll_bars(ui, rect, bounds);
        self.inline_editor(&ctx, &geo);
        self.connect_menu(&ctx);
    }

    /// Outlines what Find matched; the current match more strongly.
    fn paint_found(&self, painter: &egui::Painter) {
        if !self.find.active() {
            return;
        }
        let color = Color32::from_rgb(0xf0, 0xa0, 0x20);
        for (k, item) in self.find.matches.iter().enumerate() {
            let current = self.find.current == Some(k);
            let width = if current { 3.0 } else { 2.0 };
            match item {
                umpl_text::Item::Relation(id) => {
                    let d = &self.view.diagram;
                    let Some(route) = d.mapping.edge_of(*id).and_then(|e| d.layout.routes.get(e))
                    else {
                        continue;
                    };
                    let pts: Vec<Pos2> = route
                        .points
                        .iter()
                        .map(|p| {
                            self.camera
                                .to_screen((p.0 as f32 * GRID, p.1 as f32 * GRID))
                        })
                        .collect();
                    painter.add(Shape::line(
                        pts,
                        Stroke::new(width + 3.0, color.gamma_multiply(0.55)),
                    ));
                }
                other => {
                    let Some(r) = self.item_rect(*other) else {
                        continue;
                    };
                    let r = self.camera.rect_to_screen(r).expand(4.0);
                    let fill = if current {
                        Color32::TRANSPARENT
                    } else {
                        color.gamma_multiply(0.08)
                    };
                    painter.rect(r, 6.0, fill, Stroke::new(width, color), StrokeKind::Outside);
                }
            }
        }
    }

    fn paint_grid(&self, painter: &egui::Painter, rect: Rect, color: Color32) {
        let mut step = GRID;
        while step * self.camera.zoom < 14.0 {
            step *= 2.0;
        }
        let w = self.camera.world_rect(rect);
        let stroke = Stroke::new(1.0, color);
        let mut x = (w.x / step).floor() * step;
        while x <= w.right() {
            let sx = self.camera.to_screen((x, 0.0)).x;
            painter.line_segment(
                [Pos2::new(sx, rect.top()), Pos2::new(sx, rect.bottom())],
                stroke,
            );
            x += step;
        }
        let mut y = (w.y / step).floor() * step;
        while y <= w.bottom() {
            let sy = self.camera.to_screen((0.0, y)).y;
            painter.line_segment(
                [Pos2::new(rect.left(), sy), Pos2::new(rect.right(), sy)],
                stroke,
            );
            y += step;
        }
    }

    /// Dashed lines where a moved box lines up with another box.
    fn paint_guides(&self, painter: &egui::Painter, geo: &Geometry, color: Color32) {
        let moving: Vec<RectF> = geo
            .elements
            .iter()
            .filter(|(id, _)| self.sel.elements.contains(id))
            .map(|(_, r)| *r)
            .collect();
        let others: Vec<RectF> = geo
            .elements
            .iter()
            .filter(|(id, _)| !self.sel.elements.contains(id))
            .map(|(_, r)| *r)
            .collect();
        let stroke = Stroke::new(1.0, color.gamma_multiply(0.8));
        for m in moving.iter().take(1) {
            for o in &others {
                let xs = [
                    (m.x, o.x),
                    (m.right(), o.right()),
                    (m.center().0, o.center().0),
                ];
                for (a, b) in xs {
                    if (a - b).abs() < 0.5 {
                        let (y1, y2) = (m.y.min(o.y), m.bottom().max(o.bottom()));
                        let pts = [
                            self.camera.to_screen((a, y1)),
                            self.camera.to_screen((a, y2)),
                        ];
                        painter.extend(Shape::dashed_line(&pts, stroke, 4.0, 4.0));
                    }
                }
                let ys = [
                    (m.y, o.y),
                    (m.bottom(), o.bottom()),
                    (m.center().1, o.center().1),
                ];
                for (a, b) in ys {
                    if (a - b).abs() < 0.5 {
                        let (x1, x2) = (m.x.min(o.x), m.right().max(o.right()));
                        let pts = [
                            self.camera.to_screen((x1, a)),
                            self.camera.to_screen((x2, a)),
                        ];
                        painter.extend(Shape::dashed_line(&pts, stroke, 4.0, 4.0));
                    }
                }
            }
        }
    }

    fn handle_drag(&mut self, ctx: &egui::Context, resp: &Response, geo: &Geometry, shift: bool) {
        if resp.drag_started_by(PointerButton::Primary) && self.canvas.edit.is_none() {
            self.canvas.connect = None;
            let Some(origin) = ctx.input(|i| i.pointer.press_origin()) else {
                return;
            };
            let world = self.camera.to_world(origin);
            let hit = self.hit(geo, world, self.tool == Tool::Select);
            match (self.tool, hit) {
                (Tool::Select, Hit::Element(id)) if !self.sel.elements.contains(&id) => {
                    self.select_hit(hit, shift)
                }
                (Tool::Select, Hit::Package(_)) => self.select_hit(hit, false),
                _ => {}
            }
            let s = self.session.as_ref().unwrap();
            self.canvas.drag = match (self.tool, hit) {
                (Tool::Relation, Hit::Element(id) | Hit::Handle(id))
                | (Tool::Select, Hit::Handle(id)) => Some(Drag::Connect {
                    from: id,
                    current: world,
                }),
                (Tool::Select, Hit::Element(_)) => {
                    let origin = self
                        .sel
                        .elements
                        .iter()
                        .filter_map(|e| Some((*e, s.editor.doc.elements.get(e)?.pos?)))
                        .collect();
                    Some(Drag::Move {
                        start: world,
                        origin,
                        last: (0, 0),
                    })
                }
                (Tool::Select, Hit::Package(p)) => {
                    let origin = s
                        .editor
                        .doc
                        .elements_in(p)
                        .into_iter()
                        .filter_map(|e| Some((e, s.editor.doc.elements.get(&e)?.pos?)))
                        .collect();
                    Some(Drag::Move {
                        start: world,
                        origin,
                        last: (0, 0),
                    })
                }
                (Tool::Select, Hit::Empty | Hit::Relation(_)) => Some(Drag::Rubber {
                    start: world,
                    current: world,
                    additive: shift,
                }),
                _ => None,
            };
        }

        let Some(pointer) = resp.interact_pointer_pos().or(resp.hover_pos()) else {
            return;
        };
        let world = self.camera.to_world(pointer);
        if resp.dragged_by(PointerButton::Primary) {
            match &mut self.canvas.drag {
                Some(Drag::Move {
                    start,
                    origin,
                    last,
                }) => {
                    let d = (
                        ((world.0 - start.0) / GRID).round() as i32,
                        ((world.1 - start.1) / GRID).round() as i32,
                    );
                    if d != *last {
                        *last = d;
                        let moves: Vec<(ElementId, Pos)> = origin
                            .iter()
                            .map(|(id, p)| (*id, Pos::new(p.x + d.0, p.y + d.1)))
                            .collect();
                        if let Some(s) = &mut self.session {
                            s.editor.move_elements(&moves, Some("drag"));
                            self.view.sync(&mut s.editor);
                        }
                    }
                }
                Some(Drag::Rubber { current, .. }) | Some(Drag::Connect { current, .. }) => {
                    *current = world
                }
                None => {}
            }
            // Keep the pointer in view by panning at the edges.
            let r = self.canvas.rect.shrink(16.0);
            if self.canvas.drag.is_some() && !r.contains(pointer) {
                let push = Vec2::new(
                    if pointer.x < r.left() {
                        8.0
                    } else if pointer.x > r.right() {
                        -8.0
                    } else {
                        0.0
                    },
                    if pointer.y < r.top() {
                        8.0
                    } else if pointer.y > r.bottom() {
                        -8.0
                    } else {
                        0.0
                    },
                );
                self.camera.pan(push);
            }
        }

        if resp.drag_stopped_by(PointerButton::Primary) {
            match self.canvas.drag.take() {
                Some(Drag::Move { last, .. }) => {
                    if let Some(s) = &mut self.session {
                        s.editor.seal();
                        self.view.sync(&mut s.editor);
                        if last != (0, 0) {
                            self.view.reroute_all();
                        }
                    }
                }
                Some(Drag::Rubber {
                    start,
                    current,
                    additive,
                }) => {
                    let area = RectF::new(
                        start.0.min(current.0),
                        start.1.min(current.1),
                        (start.0 - current.0).abs(),
                        (start.1 - current.1).abs(),
                    );
                    let picked: Vec<ElementId> = geo
                        .elements
                        .iter()
                        .filter(|(_, r)| r.intersects(&area))
                        .map(|(id, _)| *id)
                        .collect();
                    if !additive {
                        self.sel.clear();
                    }
                    for id in picked {
                        if !self.sel.elements.contains(&id) {
                            self.sel.elements.push(id);
                        }
                    }
                }
                Some(Drag::Connect { from, current }) => {
                    let to = match self.hit(geo, current, false) {
                        Hit::Element(id) => Some(id),
                        _ => None,
                    };
                    let is_note = |id: Option<ElementId>| {
                        id.and_then(|i| {
                            self.session
                                .as_ref()?
                                .editor
                                .doc
                                .elements
                                .get(&i)
                                .map(|e| e.kind == ElementKind::Note)
                        })
                        .unwrap_or(false)
                    };
                    if to.is_some() && (is_note(Some(from)) || is_note(to)) {
                        self.connect(from, to, current, RelationKind::NoteLink);
                    } else {
                        self.canvas.connect = Some(ConnectMenu {
                            from,
                            to,
                            at: current,
                        });
                    }
                }
                None => {}
            }
        }
    }

    fn handle_clicks(&mut self, resp: &Response, geo: &Geometry, shift: bool) {
        let Some(pointer) = resp.interact_pointer_pos() else {
            return;
        };
        let world = self.camera.to_world(pointer);
        if resp.double_clicked() {
            match self.hit(geo, world, false) {
                Hit::Element(id) => {
                    self.sel = Selection::only_element(id);
                    self.start_edit(id);
                }
                Hit::Empty if self.tool == Tool::Select => {
                    let pkg = self.package_at(geo, world);
                    self.create_at(ElementKind::Class, world, pkg);
                }
                _ => {}
            }
            return;
        }
        if !resp.clicked() {
            return;
        }
        if self.canvas.edit.is_some() && !self.commit_edit() {
            return;
        }
        self.canvas.connect = None;
        let hit = self.hit(geo, world, self.tool == Tool::Select);
        match self.tool {
            Tool::Element(kind) => {
                let pkg = self.package_at(geo, world);
                self.create_at(kind, world, pkg);
                if !shift {
                    self.tool = Tool::Select;
                }
            }
            Tool::Package => {
                let parent = self.package_at(geo, world);
                if let Some(s) = &mut self.session {
                    let p = s.editor.add_package("package", parent);
                    let pos = Pos::new(
                        (world.0 / GRID).round() as i32 - 5,
                        (world.1 / GRID).round() as i32 - 2,
                    );
                    let el = s
                        .editor
                        .add_element(ElementKind::Class, "Class", Some(p), Some(pos));
                    self.sel = Selection {
                        packages: vec![p],
                        ..Selection::default()
                    };
                    self.start_edit(el);
                    self.notify("Package created; rename it in the inspector");
                }
                self.tool = Tool::Select;
            }
            _ => self.select_hit(hit, shift),
        }
    }

    /// The innermost package whose area contains `world`.
    fn package_at(&self, geo: &Geometry, world: (f32, f32)) -> Option<PackageId> {
        geo.packages
            .iter()
            .rev()
            .find(|(_, r)| r.contains(world))
            .map(|(id, _)| *id)
    }

    fn inline_editor(&mut self, ctx: &egui::Context, geo: &Geometry) {
        let Some(edit) = &self.canvas.edit else {
            return;
        };
        let Some((_, r)) = geo.elements.iter().find(|(id, _)| *id == edit.id) else {
            // A box created this frame has no geometry yet; it shows next
            // frame. Close only if the element is gone.
            let exists = self
                .session
                .as_ref()
                .is_some_and(|s| s.editor.doc.elements.contains_key(&edit.id));
            if !exists {
                self.canvas.edit = None;
            }
            return;
        };
        let kind = self
            .session
            .as_ref()
            .and_then(|s| s.editor.doc.elements.get(&edit.id))
            .map(|e| e.kind);
        let screen = self.camera.rect_to_screen(*r);
        let width = screen.width().max(300.0);
        let mut commit = false;
        let mut cancel = false;
        // The click that opened the editor must not also close it.
        let just_opened = edit.focus;
        let area = egui::Area::new(Id::new("inline-edit"))
            .order(Order::Foreground)
            .fixed_pos(screen.min - Vec2::new(4.0, 4.0))
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.set_width(width);
                    let edit = self.canvas.edit.as_mut().unwrap();
                    let name = ui.add(
                        egui::TextEdit::singleline(&mut edit.name)
                            .font(FontId::new(15.0, crate::paint::family(true, false)))
                            .desired_width(f32::INFINITY)
                            .hint_text("Name"),
                    );
                    if edit.focus {
                        name.request_focus();
                        edit.focus = false;
                    }
                    if let Some(err) = &edit.error {
                        ui.colored_label(Color32::from_rgb(0xd9, 0x48, 0x3b), err);
                    }
                    let hint = match kind {
                        Some(ElementKind::Enum) => "One literal per line",
                        Some(ElementKind::Note) => "Note text",
                        _ => "+name: Type\n+method(arg: Type): Result\n-{static} count: int",
                    };
                    ui.label(
                        egui::RichText::new(match kind {
                            Some(ElementKind::Note) => "Text",
                            Some(ElementKind::Enum) => "Literals",
                            _ => "Members, one per line",
                        })
                        .small()
                        .weak(),
                    );
                    ui.add(
                        egui::TextEdit::multiline(&mut edit.body)
                            .font(FontId::new(13.0, crate::paint::family(false, false)))
                            .desired_rows(5)
                            .desired_width(f32::INFINITY)
                            .hint_text(hint),
                    );
                    if name.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                        commit = true;
                    }
                    ui.horizontal(|ui| {
                        if ui.button("Apply").on_hover_text("Ctrl+Enter").clicked() {
                            commit = true;
                        }
                        if ui.button("Cancel").on_hover_text("Esc").clicked() {
                            cancel = true;
                        }
                    });
                });
            });
        ctx.input(|i| {
            if i.key_pressed(Key::Enter) && i.modifiers.command {
                commit = true;
            }
            if i.key_pressed(Key::Escape) {
                cancel = true;
            }
        });
        if area.response.clicked_elsewhere() && !commit && !cancel && !just_opened {
            commit = true;
        }
        if cancel {
            self.canvas.edit = None;
        } else if commit {
            self.commit_edit();
        }
    }

    fn connect_menu(&mut self, ctx: &egui::Context) {
        let Some(menu) = &self.canvas.connect else {
            return;
        };
        let (from, to, at) = (menu.from, menu.to, menu.at);
        let pos = self.camera.to_screen(at) + Vec2::new(8.0, 8.0);
        let mut chosen = None;
        let area = egui::Area::new(Id::new("connect-menu"))
            .order(Order::Foreground)
            .fixed_pos(pos)
            .pivot(Align2::LEFT_TOP)
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.label(
                        egui::RichText::new(if to.is_some() {
                            "Connect as"
                        } else {
                            "New class, connected as"
                        })
                        .weak(),
                    );
                    for (n, kind) in RelationKind::ALL
                        .iter()
                        .enumerate()
                        .filter(|(_, k)| **k != RelationKind::NoteLink)
                    {
                        let text = format!("{}  {}   {}", n + 1, kind.arrow(), kind.label());
                        if ui
                            .add(
                                egui::Button::new(egui::RichText::new(text).monospace())
                                    .frame(false),
                            )
                            .clicked()
                        {
                            chosen = Some(*kind);
                        }
                    }
                });
            });
        let keys = [
            Key::Num1,
            Key::Num2,
            Key::Num3,
            Key::Num4,
            Key::Num5,
            Key::Num6,
            Key::Num7,
        ];
        for (n, k) in keys.iter().enumerate() {
            if ctx.input(|i| i.key_pressed(*k)) {
                chosen = Some(RelationKind::ALL[n]);
            }
        }
        if let Some(kind) = chosen {
            self.canvas.connect = None;
            self.connect(from, to, at, kind);
        } else if area.response.clicked_elsewhere() || ctx.input(|i| i.key_pressed(Key::Escape)) {
            self.canvas.connect = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn member_text_round_trip() {
        let text = "+id: OrderId\n-{static} count: int\n+total(): Money {abstract}";
        let members = parse_members(ElementKind::Class, text);
        assert_eq!(members.len(), 3);
        assert!(members[1].is_static);
        assert!(members[2].is_abstract);
        let mut el = Element::new(ElementKind::Class, "A");
        el.members = members;
        assert_eq!(
            members_text(&el),
            "+id: OrderId\n-count: int {static}\n+total(): Money {abstract}"
        );
    }

    #[test]
    fn enum_literals_split_on_commas_and_lines() {
        let m = parse_members(ElementKind::Enum, "A, B\nC");
        assert_eq!(
            m.iter().map(|m| m.text.as_str()).collect::<Vec<_>>(),
            ["A", "B", "C"]
        );
    }
}
