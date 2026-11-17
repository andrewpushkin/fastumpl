//! Drawn icons for the tool buttons: each shows what the tool creates,
//! coloured like that kind of box on the canvas.

use egui::{CornerRadius, Painter, Pos2, Rect, Shape, Stroke, StrokeKind, Vec2};
use umpl_core::ElementKind;
use umpl_draw::Theme;

use crate::commands::Tool;
use crate::paint::c32;

/// Paints the icon of `tool` in `r` (square), with lines in `fg`.
pub fn paint_tool_icon(p: &Painter, r: Rect, tool: Tool, fg: egui::Color32, theme: &Theme) {
    // Drawn on a 22 × 22 grid.
    let s = r.width() / 22.0;
    let at = |x: f32, y: f32| r.min + Vec2::new(x * s, y * s);
    let stroke = Stroke::new(1.3 * s, fg);
    let thin = Stroke::new(1.0 * s, fg.gamma_multiply(0.75));
    match tool {
        Tool::Select => {
            // A pointer: head and tail.
            p.add(Shape::convex_polygon(
                vec![at(6.0, 3.0), at(6.0, 17.5), at(16.5, 12.5)],
                fg,
                Stroke::NONE,
            ));
            p.line_segment([at(10.0, 13.0), at(13.0, 19.5)], Stroke::new(2.6 * s, fg));
        }
        Tool::Element(ElementKind::Note) => {
            let page = vec![
                at(4.0, 3.0),
                at(14.0, 3.0),
                at(18.0, 7.0),
                at(18.0, 19.0),
                at(4.0, 19.0),
            ];
            p.add(Shape::convex_polygon(page, c32(theme.note_fill), stroke));
            p.add(Shape::line(
                vec![at(14.0, 3.0), at(14.0, 7.0), at(18.0, 7.0)],
                stroke,
            ));
            p.line_segment([at(7.0, 11.0), at(15.0, 11.0)], thin);
            p.line_segment([at(7.0, 14.5), at(13.0, 14.5)], thin);
        }
        Tool::Element(kind) => class_box(p, &at, s, kind, stroke, thin, theme),
        Tool::Package => {
            let fill = c32(theme.package_fill);
            let tab = Rect::from_min_max(at(3.0, 4.0), at(10.0, 8.0));
            p.rect(tab, 1.0 * s, fill, stroke, StrokeKind::Middle);
            let body = Rect::from_min_max(at(3.0, 7.5), at(19.0, 18.0));
            p.rect(body, 1.0 * s, fill, stroke, StrokeKind::Middle);
        }
        Tool::Relation => {
            let (a, b) = (at(4.5, 17.5), at(17.5, 4.5));
            p.line_segment([a, b], Stroke::new(1.5 * s, fg));
            p.circle_filled(a, 2.0 * s, fg);
            let dir = (b - a).normalized();
            let side = Vec2::new(-dir.y, dir.x);
            let back = b - dir * 6.0 * s;
            p.add(Shape::line(
                vec![back + side * 3.5 * s, b, back - side * 3.5 * s],
                Stroke::new(1.5 * s, fg),
            ));
        }
    }
}

/// A class box with a tinted header and the coloured dot of its kind, with
/// a detail for each kind: an italic name (abstract), a lollipop
/// (interface) or a list (enum).
fn class_box(
    p: &Painter,
    at: &dyn Fn(f32, f32) -> Pos2,
    s: f32,
    kind: ElementKind,
    stroke: Stroke,
    thin: Stroke,
    theme: &Theme,
) {
    let mark = c32(match kind {
        ElementKind::Abstract => theme.mark_abstract,
        ElementKind::Interface => theme.mark_interface,
        ElementKind::Enum => theme.mark_enum,
        _ => theme.mark_class,
    });
    let right = if kind == ElementKind::Interface {
        15.0
    } else {
        19.0
    };
    let round = (2.0 * s) as u8;
    let body = Rect::from_min_max(at(3.0, 4.0), at(right, 18.0));
    p.rect_filled(body, round, c32(theme.box_fill));
    let header = Rect::from_min_max(at(3.0, 4.0), at(right, 9.0));
    let top = CornerRadius {
        nw: round,
        ne: round,
        sw: 0,
        se: 0,
    };
    p.rect_filled(header, top, mark.gamma_multiply(0.5));
    p.rect_stroke(body, round, stroke, StrokeKind::Middle);
    p.line_segment([at(3.0, 9.0), at(right, 9.0)], stroke);
    p.circle_filled(at(6.3, 6.5), 2.4 * s, mark);
    let name_end = right - 2.5;
    if kind == ElementKind::Abstract {
        // An italic name leans.
        p.line_segment([at(10.0, 7.5), at(name_end, 5.5)], stroke);
    } else {
        p.line_segment([at(10.0, 6.5), at(name_end, 6.5)], stroke);
    }
    match kind {
        ElementKind::Enum => {
            for y in [12.0, 15.0] {
                p.circle_filled(at(5.8, y), 0.9 * s, stroke.color);
                p.line_segment([at(8.0, y), at(14.5, y)], thin);
            }
        }
        ElementKind::Interface => {
            p.line_segment([at(5.5, 13.0), at(12.0, 13.0)], thin);
            // The provided-interface lollipop.
            p.line_segment([at(15.0, 11.0), at(17.0, 11.0)], stroke);
            p.circle_stroke(at(18.8, 11.0), 1.8 * s, stroke);
        }
        _ => {
            p.line_segment([at(5.5, 12.0), at(15.0, 12.0)], thin);
            p.line_segment([at(5.5, 15.0), at(12.5, 15.0)], thin);
        }
    }
}
