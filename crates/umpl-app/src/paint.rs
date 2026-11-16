//! Paints a scene with egui. Every shape is transformed by the camera and
//! skipped when it is off screen.

use egui::epaint::{CircleShape, PathShape, PathStroke};
use egui::{Align2, Color32, CornerRadius, FontFamily, FontId, Painter, Pos2, Rect, Shape as EShape, Stroke, StrokeKind};
use umpl_draw::{Color, Scene, Shape};

use crate::camera::Camera;

pub fn c32(c: Color) -> Color32 {
    Color32::from_rgba_unmultiplied(c.0, c.1, c.2, c.3)
}

pub fn family(bold: bool, italic: bool) -> FontFamily {
    FontFamily::Name(
        match (bold, italic) {
            (false, false) => "mono",
            (true, false) => "mono-bold",
            (false, true) => "mono-italic",
            (true, true) => "mono-bold-italic",
        }
        .into(),
    )
}

/// Registers the bundled font faces as the families used by [`family`].
pub fn install_fonts(ctx: &egui::Context) {
    use std::sync::Arc;
    let mut fonts = egui::FontDefinitions::default();
    let faces = [
        ("mono", umpl_draw::fonts::REGULAR),
        ("mono-bold", umpl_draw::fonts::BOLD),
        ("mono-italic", umpl_draw::fonts::ITALIC),
        ("mono-bold-italic", umpl_draw::fonts::BOLD_ITALIC),
    ];
    for (name, bytes) in faces {
        fonts
            .font_data
            .insert(name.into(), Arc::new(egui::FontData::from_static(bytes)));
        fonts.families.insert(FontFamily::Name(name.into()), vec![name.into()]);
    }
    ctx.set_fonts(fonts);
}

fn bbox(points: &[Pos2]) -> Rect {
    let mut r = Rect::NOTHING;
    for p in points {
        r.extend_with(*p);
    }
    r
}

pub fn paint_scene(painter: &Painter, scene: &Scene, cam: &Camera, clip: Rect) {
    let z = cam.zoom;
    let visible = clip.expand(20.0);
    for shape in &scene.shapes {
        match shape {
            Shape::Rect { rect, fill, stroke, radius } => {
                let r = cam.rect_to_screen(*rect);
                if !visible.intersects(r) {
                    continue;
                }
                let stroke = stroke.map_or(Stroke::NONE, |(c, w)| Stroke::new((w * z).max(0.6), c32(c)));
                painter.rect(
                    r,
                    CornerRadius::same((radius * z).round().clamp(0.0, 255.0) as u8),
                    fill.map_or(Color32::TRANSPARENT, c32),
                    stroke,
                    StrokeKind::Middle,
                );
            }
            Shape::Path { points, stroke, fill, closed, dashed } => {
                let pts: Vec<Pos2> = points.iter().map(|p| cam.to_screen(*p)).collect();
                if pts.len() < 2 || !visible.intersects(bbox(&pts)) {
                    continue;
                }
                let st = stroke.map_or(Stroke::NONE, |(c, w)| Stroke::new((w * z).max(0.6), c32(c)));
                if *closed {
                    painter.add(EShape::Path(PathShape {
                        points: pts,
                        closed: true,
                        fill: fill.map_or(Color32::TRANSPARENT, c32),
                        stroke: PathStroke::from(st),
                    }));
                } else if *dashed {
                    painter.extend(EShape::dashed_line(&pts, st, 6.0 * z, 4.0 * z));
                } else {
                    painter.add(EShape::line(pts, st));
                }
            }
            Shape::Circle { center, radius, fill } => {
                let c = cam.to_screen(*center);
                if !visible.contains(c) {
                    continue;
                }
                painter.add(EShape::Circle(CircleShape::filled(c, radius * z, c32(*fill))));
            }
            Shape::Text { pos, text, style } => {
                let size = style.size * z;
                let p = cam.to_screen(*pos);
                if size < 3.5 || !visible.contains(p) && !style.centered {
                    continue;
                }
                let anchor = if style.centered { Align2::CENTER_CENTER } else { Align2::LEFT_CENTER };
                let r = painter.text(p, anchor, text, FontId::new(size, family(style.bold, style.italic)), c32(style.color));
                if style.underline {
                    let y = r.bottom() - size * 0.12;
                    painter.line_segment([Pos2::new(r.left(), y), Pos2::new(r.right(), y)], Stroke::new((0.8 * z).max(0.5), c32(style.color)));
                }
            }
        }
    }
}
