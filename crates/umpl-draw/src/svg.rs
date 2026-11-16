//! Writes a [`Scene`] as an SVG document.

use std::fmt::Write;

use crate::fonts::{CENTER_TO_BASELINE, FAMILY};
use crate::scene::{Scene, Shape};
use crate::theme::Theme;

fn num(v: f32) -> String {
    let s = format!("{:.2}", v);
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" {
        "0".into()
    } else {
        s.to_string()
    }
}

fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            c => out.push(c),
        }
    }
    out
}

fn path_d(points: &[(f32, f32)], closed: bool) -> String {
    let mut d = String::new();
    for (i, p) in points.iter().enumerate() {
        let _ = write!(d, "{}{} {}", if i == 0 { "M" } else { " L" }, num(p.0), num(p.1));
    }
    if closed {
        d.push_str(" Z");
    }
    d
}

/// The whole scene as a standalone SVG file.
pub fn to_svg(scene: &Scene, theme: &Theme) -> String {
    let b = scene.bounds;
    let mut out = String::new();
    let _ = writeln!(
        out,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="{x} {y} {w} {h}" font-family="{FAMILY}, monospace">"#,
        x = num(b.x),
        y = num(b.y),
        w = num(b.w),
        h = num(b.h),
    );
    let _ = writeln!(
        out,
        r#"<rect x="{}" y="{}" width="{}" height="{}" fill="{}"/>"#,
        num(b.x),
        num(b.y),
        num(b.w),
        num(b.h),
        theme.background.css()
    );
    for shape in &scene.shapes {
        match shape {
            Shape::Rect { rect, fill, stroke, radius } => {
                let _ = write!(
                    out,
                    r#"<rect x="{}" y="{}" width="{}" height="{}" rx="{}" fill="{}""#,
                    num(rect.x),
                    num(rect.y),
                    num(rect.w),
                    num(rect.h),
                    num(*radius),
                    fill.map_or("none".into(), |c| c.css())
                );
                if let Some((c, w)) = stroke {
                    let _ = write!(out, r#" stroke="{}" stroke-width="{}""#, c.css(), num(*w));
                }
                out.push_str("/>\n");
            }
            Shape::Path { points, stroke, fill, closed, dashed } => {
                let _ = write!(
                    out,
                    r#"<path d="{}" fill="{}""#,
                    path_d(points, *closed),
                    fill.map_or("none".into(), |c| c.css())
                );
                if let Some((c, w)) = stroke {
                    let _ = write!(
                        out,
                        r#" stroke="{}" stroke-width="{}" stroke-linejoin="round" stroke-linecap="round""#,
                        c.css(),
                        num(*w)
                    );
                }
                if *dashed {
                    out.push_str(r#" stroke-dasharray="6 4""#);
                }
                out.push_str("/>\n");
            }
            Shape::Circle { center, radius, fill } => {
                let _ = writeln!(
                    out,
                    r#"<circle cx="{}" cy="{}" r="{}" fill="{}"/>"#,
                    num(center.0),
                    num(center.1),
                    num(*radius),
                    fill.css()
                );
            }
            Shape::Text { pos, text, style } => {
                if text.is_empty() {
                    continue;
                }
                let _ = write!(
                    out,
                    r#"<text x="{}" y="{}" font-size="{}" fill="{}""#,
                    num(pos.0),
                    num(pos.1 + CENTER_TO_BASELINE * style.size),
                    num(style.size),
                    style.color.css()
                );
                if style.bold {
                    out.push_str(r#" font-weight="bold""#);
                }
                if style.italic {
                    out.push_str(r#" font-style="italic""#);
                }
                if style.underline {
                    out.push_str(r#" text-decoration="underline""#);
                }
                if style.centered {
                    out.push_str(r#" text-anchor="middle""#);
                }
                let _ = writeln!(out, ">{}</text>", escape(text));
            }
        }
    }
    out.push_str("</svg>\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{build_scene, Diagram, SceneOptions};

    #[test]
    fn svg_is_well_formed() {
        let p = umpl_text::parse(include_str!("../../../examples/shop.umpl"));
        let d = Diagram::arrange(&p.doc);
        let theme = Theme::light();
        let svg = to_svg(&build_scene(&p.doc, &d, &theme, &SceneOptions::default()), &theme);
        assert!(svg.starts_with("<svg ") && svg.trim_end().ends_with("</svg>"));
        assert_eq!(svg.matches("<text").count(), svg.matches("</text>").count());
        for name in ["Order", "Payment", "CardPayment", "Priced", "Status", "OrderLine"] {
            assert!(svg.contains(&format!(">{name}</text>")), "missing {name}");
        }
        assert!(svg.contains("List&lt;OrderLine&gt;"));
    }
}
