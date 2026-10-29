//! Text that fits its space.
//!
//! egui turns text into a "galley" (laid-out glyphs with a known size) before
//! painting it. Measuring the galley first lets us decide how to make the text
//! fit before anything is drawn. egui caches galleys, so laying out the same text
//! every frame is cheap.

use eframe::egui::text::{LayoutJob, TextWrapping};
use eframe::egui::{Align2, Color32, FontId, Galley, Painter, Rect};
use std::sync::Arc;

/// Lays out one line of text no wider than `max_width`.
///
/// Too-wide text first shrinks, down to `min_size` (a smaller number beats a
/// cut-off one), then is elided with '…'. Pass `min_size == font.size` to only elide.
pub fn fit(painter: &Painter, text: String, font: FontId, color: Color32, max_width: f32, min_size: f32) -> Arc<Galley> {
    let galley = painter.layout_no_wrap(text.clone(), font.clone(), color);
    let width = galley.size().x;
    if width <= max_width {
        return galley;
    }
    // Text width scales roughly with font size, so start from the proportional guess and step
    // down until it fits. Whole pixels only: every new size costs glyph rasterization, and
    // animated values would otherwise create many.
    let min_size = min_size.min(font.size);
    let mut size = (font.size * max_width / width).floor().clamp(min_size, font.size);
    while size > min_size {
        let font = FontId::new(size, font.family.clone());
        let galley = painter.layout_no_wrap(text.clone(), font, color);
        if galley.size().x <= max_width {
            return galley;
        }
        size = (size - 1.0).max(min_size);
    }
    // Still too wide at the smallest allowed size: elide. One row, cut at `max_width`, ending in '…'.
    let mut job = LayoutJob::simple_singleline(text, FontId::new(size, font.family), color);
    job.wrap = TextWrapping::truncate_at_width(max_width.max(0.0));
    painter.layout_job(job)
}

/// Paints a galley aligned inside `rect`.
pub fn paint(painter: &Painter, rect: Rect, align: Align2, galley: Arc<Galley>) {
    let pos = align.align_size_within_rect(galley.size(), rect).min;
    // The galley already carries its colour; the fallback is never used.
    painter.galley(pos, galley, Color32::PLACEHOLDER);
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui;

    /// Runs `check` with a real painter, so text is measured with egui's actual fonts.
    fn with_painter(check: impl FnOnce(&Painter)) {
        let ctx = egui::Context::default();
        let mut check = Some(check);
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| (check.take().unwrap())(ui.painter()));
        });
    }

    #[test]
    fn short_text_is_untouched() {
        with_painter(|p| {
            let g = fit(p, "4.2%".into(), FontId::monospace(12.0), Color32::WHITE, 100.0, 9.0);
            assert_eq!(g.text(), "4.2%");
            assert!(!g.elided);
        });
    }

    #[test]
    fn wide_number_shrinks_before_eliding() {
        with_painter(|p| {
            let text = "1023.9 MB / 1023.9 GB";
            let natural = p.layout_no_wrap(text.into(), FontId::monospace(12.0), Color32::WHITE).size().x;
            let g = fit(p, text.into(), FontId::monospace(12.0), Color32::WHITE, natural * 0.85, 9.0);
            assert!(g.size().x <= natural * 0.85 + 0.5);
            assert!(!g.elided, "should have fit by shrinking");
        });
    }

    #[test]
    fn hopeless_text_is_elided_within_width() {
        with_painter(|p| {
            let g = fit(p, "x".repeat(200), FontId::monospace(12.0), Color32::WHITE, 80.0, 9.0);
            assert!(g.size().x <= 80.5);
            assert!(g.elided);
        });
    }
}
