use crate::config::HISTORY;
use crate::theme::{ACCENT, TRACK};
use eframe::egui::{self, CornerRadius, Mesh, Pos2, Sense, Shape, Stroke, Vec2};
use std::collections::VecDeque;

const HEIGHT: f32 = 38.0;
const PAD_Y: f32 = 3.0;

/// The 60s CPU line graph.
///
/// `history` holds raw readings, oldest first. `phase` (0..=1) is how far the
/// graph is through its transition to the newest reading. Rather than jumping one
/// step per reading, it slides left while the newest segment draws itself in.
pub fn sparkline(ui: &mut egui::Ui, history: &VecDeque<f32>, phase: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), HEIGHT), Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, CornerRadius::same(6), TRACK.gamma_multiply(0.6));

    let n = history.len();
    if n < 2 {
        return;
    }
    let phase = phase.clamp(0.0, 1.0);
    let step = rect.width() / (HISTORY - 1) as f32;
    let y = |v: f32| rect.bottom() - PAD_Y - (v / 100.0).clamp(0.0, 1.0) * (rect.height() - 2.0 * PAD_Y);

    // `step` is the horizontal distance between two readings: the graph spans HISTORY readings.
    //
    // The reading `age` steps old (0 = newest) sits at `right - (age - 1 + phase) * step`.
    // The second-newest reaches the right edge exactly as the next reading arrives,
    // so the hand-over from phase 1 back to 0 is seamless.
    let mut points: Vec<Pos2> = history
        .iter()
        .take(n - 1)
        .enumerate()
        .map(|(i, &v)| {
            let age = (n - 1 - i) as f32;
            Pos2::new(rect.right() - (age - 1.0 + phase) * step, y(v))
        })
        .collect();
    // The tip travels from the second-newest reading toward the newest. Together with the
    // slide above, the newest segment appears to draw itself in from the right.
    //
    //   phase 0:  ... o---o        (second-newest at the right edge, tip on top of it)
    //   phase 1:  ... o---o---o    (everything one step left, tip at the newest value)
    let (prev, newest) = (history[n - 2], history[n - 1]);
    points.push(Pos2::new(rect.right(), y(prev + (newest - prev) * phase)));

    // Older readings that have slid past the left edge are hidden by the painter's clip rect.
    painter.add(area_under(&points, rect.bottom()));
    let tip = *points.last().unwrap();
    painter.add(Shape::line(points, Stroke::new(1.5, ACCENT)));
    painter.circle_filled(tip, 2.0, ACCENT);
}

/// A soft gradient fill between the line and the bottom of the graph.
///
/// Built as a triangle strip: each point on the line gets a partner directly below
/// it on the bottom edge, and each neighbouring pair of columns becomes two triangles.
/// The top vertices are tinted and the bottom ones transparent; the GPU blends between them.
fn area_under(points: &[Pos2], bottom: f32) -> Mesh {
    let mut mesh = Mesh::default();
    let top = ACCENT.gamma_multiply(0.22);
    let fade = ACCENT.gamma_multiply(0.0);
    for (i, p) in points.iter().enumerate() {
        mesh.colored_vertex(*p, top);
        mesh.colored_vertex(Pos2::new(p.x, bottom), fade);
        if i > 0 {
            let b = 2 * i as u32;
            mesh.add_triangle(b - 2, b - 1, b);
            mesh.add_triangle(b - 1, b, b + 1);
        }
    }
    mesh
}
