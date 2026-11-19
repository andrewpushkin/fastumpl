use egui::{Pos2, Rect, Vec2};
use umpl_draw::RectF;

pub const MIN_ZOOM: f32 = 0.05;
pub const MAX_ZOOM: f32 = 4.0;
/// Fitting a small diagram zooms in at most this far.
pub const FIT_MAX_ZOOM: f32 = 2.0;
/// Space kept free around a fitted diagram, in screen points.
const FIT_MARGIN: f32 = 16.0;

/// Maps diagram pixels ("world") to screen points.
#[derive(Clone, Copy, Debug)]
pub struct Camera {
    /// Screen position of the world origin.
    pub origin: Pos2,
    pub zoom: f32,
}

impl Default for Camera {
    fn default() -> Self {
        Camera {
            origin: Pos2::new(40.0, 40.0),
            zoom: 1.0,
        }
    }
}

impl Camera {
    pub fn to_screen(self, p: (f32, f32)) -> Pos2 {
        Pos2::new(
            self.origin.x + p.0 * self.zoom,
            self.origin.y + p.1 * self.zoom,
        )
    }

    pub fn to_world(self, p: Pos2) -> (f32, f32) {
        (
            (p.x - self.origin.x) / self.zoom,
            (p.y - self.origin.y) / self.zoom,
        )
    }

    pub fn rect_to_screen(&self, r: RectF) -> Rect {
        Rect::from_min_max(
            self.to_screen((r.x, r.y)),
            self.to_screen((r.right(), r.bottom())),
        )
    }

    pub fn world_rect(&self, screen: Rect) -> RectF {
        let a = self.to_world(screen.min);
        let b = self.to_world(screen.max);
        RectF::new(a.0, a.1, b.0 - a.0, b.1 - a.1)
    }

    /// Zooms by `factor`, keeping the world point under `anchor` still.
    pub fn zoom_at(&mut self, anchor: Pos2, factor: f32) {
        let before = self.to_world(anchor);
        self.zoom = (self.zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);
        let after = self.to_screen(before);
        self.origin += anchor - after;
    }

    pub fn pan(&mut self, delta: Vec2) {
        self.origin += delta;
    }

    /// Shows all of `bounds` filling `view`, zoomed in to at most
    /// [`FIT_MAX_ZOOM`].
    pub fn fit(&mut self, bounds: RectF, view: Rect) {
        if bounds.w <= 0.0 || bounds.h <= 0.0 {
            *self = Camera {
                origin: view.min + Vec2::splat(40.0),
                zoom: 1.0,
            };
            return;
        }
        let zx = (view.width() - 2.0 * FIT_MARGIN) / bounds.w;
        let zy = (view.height() - 2.0 * FIT_MARGIN) / bounds.h;
        self.zoom = zx.min(zy).clamp(MIN_ZOOM, FIT_MAX_ZOOM);
        let center = view.center();
        self.origin = Pos2::new(
            center.x - (bounds.x + bounds.w / 2.0) * self.zoom,
            center.y - (bounds.y + bounds.h / 2.0) * self.zoom,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_fills_the_view() {
        let view = Rect::from_min_size(Pos2::new(100.0, 50.0), Vec2::new(800.0, 600.0));
        let mut c = Camera::default();
        // Wider than the view: it fills the width.
        c.fit(RectF::new(0.0, 0.0, 1536.0, 400.0), view);
        let r = c.rect_to_screen(RectF::new(0.0, 0.0, 1536.0, 400.0));
        assert!(
            (r.width() - (800.0 - 2.0 * FIT_MARGIN)).abs() < 0.5,
            "{r:?}"
        );
        assert!((r.center() - view.center()).length() < 0.5);
        // Small: zoomed in, but not past the cap.
        c.fit(RectF::new(0.0, 0.0, 300.0, 200.0), view);
        assert!(c.zoom > 1.0 && c.zoom <= FIT_MAX_ZOOM, "{}", c.zoom);
        c.fit(RectF::new(0.0, 0.0, 30.0, 20.0), view);
        assert_eq!(c.zoom, FIT_MAX_ZOOM);
    }

    #[test]
    fn zoom_keeps_anchor() {
        let mut c = Camera::default();
        let anchor = Pos2::new(300.0, 200.0);
        let w = c.to_world(anchor);
        c.zoom_at(anchor, 2.0);
        let back = c.to_screen(w);
        assert!((back - anchor).length() < 1e-3);
    }
}
