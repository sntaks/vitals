//! One small vertical bar per CPU core, filling from the bottom.

use crate::theme::{level_color, TRACK};
use eframe::egui::{self, CornerRadius, Pos2, Rect, Sense, Vec2};

const SINGLE_ROW_H: f32 = 22.0;
const MULTI_ROW_H: f32 = 12.0;
const MIN_BAR_W: f32 = 3.0;

/// Splits `n` bars into rows so none is narrower than `MIN_BAR_W`.
/// Returns (bars per row, rows, gap).
fn grid(n: usize, width: f32) -> (usize, usize, f32) {
    let n = n.max(1);
    let gap = if n > 32 { 1.0 } else { 2.0 };
    let fit_per_row = (((width + gap) / (MIN_BAR_W + gap)).floor() as usize).max(1);
    let rows = n.div_ceil(fit_per_row);
    // Balance the rows, e.g. 2 x 40 rather than 70 + 10.
    (n.div_ceil(rows), rows, gap)
}

/// Tiny vertical bars, one per CPU core. Wraps onto extra rows on machines
/// with more cores than fit across.
pub fn core_bars(ui: &mut egui::Ui, cores: &[f32]) {
    let width = ui.available_width();
    let (per_row, rows, gap) = grid(cores.len(), width);
    let row_h = if rows == 1 { SINGLE_ROW_H } else { MULTI_ROW_H };
    let height = rows as f32 * row_h + (rows - 1) as f32 * gap;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
    let painter = ui.painter_at(rect);

    // Bar width: share the row's width equally after subtracting the gaps.
    let w = ((rect.width() - gap * (per_row as f32 - 1.0)) / per_row as f32).max(1.0);
    let radius = CornerRadius::same(if w >= 4.0 { 2 } else { 1 });
    for (i, c) in cores.iter().enumerate() {
        // Bars fill left to right, then wrap to the next row.
        let (row, col) = (i / per_row, i % per_row);
        let x = rect.left() + col as f32 * (w + gap);
        let y = rect.top() + row as f32 * (row_h + gap);
        let track = Rect::from_min_size(Pos2::new(x, y), Vec2::new(w, row_h));
        painter.rect_filled(track, radius, TRACK);
        let h = row_h * (c / 100.0).clamp(0.0, 1.0);
        if h > 0.5 {
            let fill = Rect::from_min_max(Pos2::new(x, track.bottom() - h), track.max);
            painter.rect_filled(fill, radius, level_color(*c));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::grid;

    #[test]
    fn few_cores_fit_one_row() {
        assert_eq!(grid(8, 282.0), (8, 1, 2.0));
    }

    #[test]
    fn many_cores_wrap_into_balanced_rows() {
        let (per_row, rows, _) = grid(256, 282.0);
        assert!(rows > 1);
        assert!(per_row * rows >= 256);
        assert!(per_row as f32 * (3.0 + 1.0) - 1.0 <= 282.0);
    }
}
