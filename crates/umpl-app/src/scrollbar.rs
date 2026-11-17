//! Scroll bars along the bottom and right edges of the canvas.
//!
//! The canvas has no fixed size, so the scrollable range is the diagram
//! with a margin, extended to whatever is in view (as in other design
//! tools): scrolling past the diagram is allowed, and the bars always show
//! where the view is relative to the diagram.

use egui::{Pos2, Rect, Sense, StrokeKind, Ui};
use umpl_draw::RectF;

use crate::app::App;

/// Width of a scroll bar, in points.
pub const THICKNESS: f32 = 10.0;
/// The shortest thumb, in points, so it stays easy to grab.
pub const MIN_THUMB: f32 = 24.0;
/// Room to scroll past the diagram on every side, in diagram pixels.
const MARGIN: f32 = 120.0;
/// A click on the track moves this much of a view.
const PAGE: f32 = 0.9;

/// A thumb being dragged.
#[derive(Clone, Copy, Debug)]
pub struct ScrollDrag {
    pub axis: usize,
    /// The scroll range when the drag started, kept so the thumb follows
    /// the pointer even as the view leaves the diagram.
    content: Span,
    /// Pointer and camera origin along `axis` at the press.
    press: f32,
    origin: f32,
}

/// A range along one axis.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Span {
    pub lo: f32,
    pub hi: f32,
}

impl Span {
    pub fn new(lo: f32, hi: f32) -> Span {
        Span { lo, hi }
    }

    pub fn len(self) -> f32 {
        self.hi - self.lo
    }

    fn union(self, o: Span) -> Span {
        Span::new(self.lo.min(o.lo), self.hi.max(o.hi))
    }
}

/// Where the thumb sits on `track` when `view` shows part of `content`.
pub fn thumb(content: Span, view: Span, track: Span) -> Span {
    let total = content.len().max(1e-3);
    let min = MIN_THUMB.min(track.len());
    let len = (view.len() / total * track.len()).clamp(min, track.len());
    let free = content.len() - view.len();
    let t = if free > 0.0 {
        ((view.lo - content.lo) / free).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let lo = track.lo + t * (track.len() - len);
    Span::new(lo, lo + len)
}

/// How far the view moves, in world units, per point the thumb moves.
pub fn world_per_point(content: Span, view: Span, track: Span) -> f32 {
    let travel = track.len() - thumb(content, view, track).len();
    let free = content.len() - view.len();
    if travel <= 0.0 || free <= 0.0 {
        0.0
    } else {
        free / travel
    }
}

/// `r` along `axis` (0 = x, 1 = y).
fn span_of(r: RectF, axis: usize) -> Span {
    if axis == 0 {
        Span::new(r.x, r.right())
    } else {
        Span::new(r.y, r.bottom())
    }
}

impl App {
    /// Paints the scroll bars over the edges of the canvas `rect` and
    /// handles dragging, paging and wheel scrolling on them.
    pub fn scroll_bars(&mut self, ui: &mut Ui, rect: Rect, bounds: RectF) {
        let view = self.camera.world_rect(rect);
        for axis in 0..2 {
            // The bottom bar stops short of the right one, leaving a corner.
            let track_rect = if axis == 0 {
                Rect::from_min_max(
                    Pos2::new(rect.left(), rect.bottom() - THICKNESS),
                    Pos2::new(rect.right() - THICKNESS, rect.bottom()),
                )
            } else {
                Rect::from_min_max(
                    Pos2::new(rect.right() - THICKNESS, rect.top()),
                    Pos2::new(rect.right(), rect.bottom() - THICKNESS),
                )
            };
            let track = if axis == 0 {
                Span::new(track_rect.left(), track_rect.right())
            } else {
                Span::new(track_rect.top(), track_rect.bottom())
            };
            let view_span = span_of(view, axis);
            let content = match self.canvas.scroll_drag {
                Some(d) if d.axis == axis => d.content,
                _ => {
                    let b = span_of(bounds, axis);
                    Span::new(b.lo - MARGIN, b.hi + MARGIN).union(view_span)
                }
            };

            let resp = ui.interact(
                track_rect,
                ui.id().with(("scroll bar", axis)),
                Sense::click_and_drag(),
            );
            let zoom = self.camera.zoom;
            let th = thumb(content, view_span, track);
            let on_thumb = |p: Pos2| {
                let v = if axis == 0 { p.x } else { p.y };
                v >= th.lo && v <= th.hi
            };
            let (press, latest) = ui
                .ctx()
                .input(|i| (i.pointer.press_origin(), i.pointer.latest_pos()));
            if resp.drag_started() {
                if let Some(p) = press {
                    self.canvas.scroll_drag = Some(ScrollDrag {
                        axis,
                        content,
                        press: p[axis],
                        origin: self.camera.origin[axis],
                    });
                }
            }
            // Measured from the press, so the thumb stays under the pointer.
            if let (true, Some(d), Some(p)) = (
                resp.dragged() || resp.drag_stopped(),
                self.canvas.scroll_drag.filter(|d| d.axis == axis),
                latest,
            ) {
                let moved = (p[axis] - d.press) * world_per_point(content, view_span, track);
                self.camera.origin[axis] = d.origin - moved * zoom;
            }
            if resp.drag_stopped() {
                self.canvas.scroll_drag = None;
            }
            if resp.clicked() {
                if let Some(p) = resp.interact_pointer_pos().filter(|p| !on_thumb(*p)) {
                    let toward_end = (if axis == 0 { p.x } else { p.y }) > th.hi;
                    let page = PAGE * view_span.len() * zoom;
                    self.camera.origin[axis] += if toward_end { -page } else { page };
                }
            }
            if resp.hovered() {
                let wheel = ui.ctx().input(|i| i.smooth_scroll_delta);
                self.camera.pan(wheel);
            }

            // Paint with the view as it is now.
            let view_now = span_of(self.camera.world_rect(rect), axis);
            let th = thumb(content, view_now, track);
            let thumb_rect = if axis == 0 {
                Rect::from_min_max(
                    Pos2::new(th.lo, track_rect.top()),
                    Pos2::new(th.hi, track_rect.bottom()),
                )
            } else {
                Rect::from_min_max(
                    Pos2::new(track_rect.left(), th.lo),
                    Pos2::new(track_rect.right(), th.hi),
                )
            };
            self.canvas.scroll_thumbs[axis] = Some(thumb_rect);
            let visuals = ui.visuals();
            let fill = if resp.dragged() {
                visuals.widgets.active.bg_fill
            } else if resp.hovered() {
                visuals.widgets.hovered.bg_fill
            } else {
                visuals.widgets.inactive.bg_fill
            };
            let painter = ui.painter_at(rect);
            painter.rect_filled(
                track_rect,
                0.0,
                visuals.extreme_bg_color.gamma_multiply(0.6),
            );
            painter.rect(
                thumb_rect.shrink(2.0),
                3.0,
                fill,
                visuals.widgets.inactive.bg_stroke,
                StrokeKind::Inside,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: Span, b: Span) -> bool {
        (a.lo - b.lo).abs() < 1e-3 && (a.hi - b.hi).abs() < 1e-3
    }

    #[test]
    fn thumb_shows_the_viewed_part() {
        let track = Span::new(0.0, 100.0);
        let content = Span::new(0.0, 1000.0);
        // All in view: the thumb fills the track.
        assert!(close(thumb(content, content, track), track));
        // The middle half in view.
        let t = thumb(content, Span::new(250.0, 750.0), track);
        assert!(close(t, Span::new(25.0, 75.0)), "{t:?}");
        // The last fifth, on a track that does not start at zero.
        let t = thumb(content, Span::new(700.0, 1000.0), Span::new(50.0, 150.0));
        assert!(close(t, Span::new(120.0, 150.0)), "{t:?}");
    }

    #[test]
    fn thumb_keeps_its_minimum_length_and_reaches_both_ends() {
        let track = Span::new(0.0, 100.0);
        let content = Span::new(0.0, 10_000.0);
        let start = thumb(content, Span::new(0.0, 10.0), track);
        assert!(close(start, Span::new(0.0, MIN_THUMB)), "{start:?}");
        let end = thumb(content, Span::new(9_990.0, 10_000.0), track);
        assert!(close(end, Span::new(100.0 - MIN_THUMB, 100.0)), "{end:?}");
    }

    #[test]
    fn dragging_the_thumb_moves_the_view_proportionally() {
        let track = Span::new(0.0, 100.0);
        let content = Span::new(0.0, 1000.0);
        // Thumb 25 long, 75 points of travel for 750 units of view travel.
        assert!((world_per_point(content, Span::new(0.0, 250.0), track) - 10.0).abs() < 1e-3);
        // With the minimum thumb, travel is shorter and each point moves more.
        let tiny = Span::new(0.0, 10.0);
        let wpp = world_per_point(Span::new(0.0, 10_000.0), tiny, track);
        assert!((wpp - 9_990.0 / (100.0 - MIN_THUMB)).abs() < 1e-2, "{wpp}");
        // Nothing to scroll.
        assert_eq!(world_per_point(content, content, track), 0.0);
    }
}
