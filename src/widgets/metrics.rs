//! Simple rows: "label [bar] value", "label .... value", and section titles.
//!
//! Each row allocates a fixed-height strip and paints into it with a painter
//! clipped to that strip, so nothing can draw outside its row.

use super::text::{fit, paint};
use crate::theme::{level_color, MUTED, TEXT, TRACK};
use eframe::egui::{self, Align2, CornerRadius, FontId, Painter, Rect, RichText, Sense, TextStyle, Vec2};

const ROW_H: f32 = 16.0;
const LABEL_W: f32 = 52.0;
const BAR_W: f32 = 90.0;
const BAR_H: f32 = 6.0;
const GAP: f32 = 8.0;
// Numbers may shrink this far before being elided.
const MIN_VALUE_SIZE: f32 = 9.0;

fn mono(ui: &egui::Ui) -> FontId {
    TextStyle::Monospace.resolve(ui.style())
}

/// A rounded progress bar
fn bar(painter: &Painter, rect: Rect, pct: f32) {
    painter.rect_filled(rect, CornerRadius::same(3), TRACK);
    let mut fill = rect;
    fill.set_width(rect.width() * (pct / 100.0).clamp(0.0, 1.0));
    if fill.width() > 0.5 {
        painter.rect_filled(fill, CornerRadius::same(3), level_color(pct));
    }
}

/// One row: LABEL [bar] value. Column widths are fixed so animating values never shift the bar.
pub fn metric_row(ui: &mut egui::Ui, label: &str, pct: f32, value: String) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), ROW_H), Sense::hover());
    let painter = ui.painter_at(rect);

    // Layout, left to right:  | label (52) | bar (90) | gap | value, right-aligned |
    let label_w = LABEL_W.min(rect.width() * 0.3);
    // On a narrow row the bar gives up space before the value does.
    let bar_w = BAR_W.min((rect.width() - label_w) * 0.45).max(0.0);
    let label_rect = Rect::from_min_size(rect.min, Vec2::new(label_w, ROW_H));
    let bar_rect = Rect::from_center_size(
        egui::pos2(label_rect.right() + bar_w / 2.0, rect.center().y),
        Vec2::new(bar_w, BAR_H),
    );
    let value_rect = Rect::from_min_max(egui::pos2(bar_rect.right() + GAP, rect.top()), rect.max);

    let font = FontId::proportional(11.0);
    let label = fit(&painter, label.to_owned(), font.clone(), MUTED, label_w - 4.0, font.size);
    paint(&painter, label_rect, Align2::LEFT_CENTER, label);
    bar(&painter, bar_rect, pct);
    let value = fit(&painter, value, mono(ui), TEXT, value_rect.width(), MIN_VALUE_SIZE);
    paint(&painter, value_rect, Align2::RIGHT_CENTER, value);
}

/// One row: label on the left, value right-aligned in whatever space is left.
/// The label is measured first, so the value gets exactly the remaining width.
pub fn kv_row(ui: &mut egui::Ui, label: &str, value: String) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), ROW_H), Sense::hover());
    let painter = ui.painter_at(rect);

    let font = TextStyle::Small.resolve(ui.style());
    let label = fit(&painter, label.to_owned(), font.clone(), MUTED, rect.width() * 0.4, font.size);
    let label_w = label.size().x;
    paint(&painter, rect, Align2::LEFT_CENTER, label);

    let value = fit(&painter, value, mono(ui), TEXT, rect.width() - label_w - GAP, MIN_VALUE_SIZE);
    paint(&painter, rect, Align2::RIGHT_CENTER, value);
}

pub fn section_title(ui: &mut egui::Ui, text: &str) {
    ui.add_space(6.0);
    ui.label(RichText::new(text).color(MUTED).small().strong());
}
