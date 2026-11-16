//! Raster output: renders the SVG with resvg and the bundled fonts.

use resvg::{tiny_skia, usvg};

use crate::fonts;

/// An RGBA image, rows top to bottom, premultiplied alpha removed.
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

fn options() -> usvg::Options<'static> {
    let mut opt = usvg::Options::default();
    let db = opt.fontdb_mut();
    for face in [
        fonts::REGULAR,
        fonts::BOLD,
        fonts::ITALIC,
        fonts::BOLD_ITALIC,
    ] {
        db.load_font_data(face.to_vec());
    }
    db.set_monospace_family(fonts::FAMILY);
    opt
}

fn render(svg: &str, scale: f32) -> Result<tiny_skia::Pixmap, String> {
    let tree = usvg::Tree::from_str(svg, &options()).map_err(|e| e.to_string())?;
    let size = tree.size();
    let w = (size.width() * scale).ceil().max(1.0) as u32;
    let h = (size.height() * scale).ceil().max(1.0) as u32;
    let mut pixmap = tiny_skia::Pixmap::new(w, h).ok_or("image too large")?;
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    Ok(pixmap)
}

/// Renders an SVG produced by [`crate::svg::to_svg`] to PNG bytes.
pub fn svg_to_png(svg: &str, scale: f32) -> Result<Vec<u8>, String> {
    render(svg, scale)?.encode_png().map_err(|e| e.to_string())
}

/// Renders an SVG to straight-alpha RGBA pixels (for the clipboard).
pub fn svg_to_rgba(svg: &str, scale: f32) -> Result<Image, String> {
    let pixmap = render(svg, scale)?;
    let mut rgba = Vec::with_capacity(pixmap.data().len());
    for p in pixmap.pixels() {
        let c = p.demultiply();
        rgba.extend_from_slice(&[c.red(), c.green(), c.blue(), c.alpha()]);
    }
    Ok(Image {
        width: pixmap.width(),
        height: pixmap.height(),
        rgba,
    })
}

/// Encodes opaque or straight-alpha RGBA pixels as PNG.
pub fn rgba_to_png(width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>, String> {
    let size = tiny_skia::IntSize::from_wh(width, height).ok_or("empty image")?;
    let mut data = Vec::with_capacity(rgba.len());
    for px in rgba.chunks_exact(4) {
        let c = tiny_skia::ColorU8::from_rgba(px[0], px[1], px[2], px[3]).premultiply();
        data.extend_from_slice(&[c.red(), c.green(), c.blue(), c.alpha()]);
    }
    let pixmap = tiny_skia::Pixmap::from_vec(data, size).ok_or("bad image size")?;
    pixmap.encode_png().map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{build_scene, svg::to_svg, Diagram, SceneOptions, Theme};

    #[test]
    fn renders_png() {
        let p = umpl_text::parse(include_str!("../../../examples/shop.umpl"));
        let d = Diagram::arrange(&p.doc);
        let theme = Theme::light();
        let svg = to_svg(
            &build_scene(&p.doc, &d, &theme, &SceneOptions::default()),
            &theme,
        );
        let png = svg_to_png(&svg, 1.0).unwrap();
        assert_eq!(&png[1..4], b"PNG");
        let img = svg_to_rgba(&svg, 0.5).unwrap();
        assert_eq!(img.rgba.len(), (img.width * img.height * 4) as usize);
    }
}
