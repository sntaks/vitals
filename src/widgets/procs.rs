//! The "top processes" table.
//!
//! Each reading delivers up to TOP_N process groups, sorted busiest first. Rather
//! than redrawing the list in its new order, every row is an animated object that
//! survives between readings:
//!
//! - its vertical position (`slot`, measured in rows) glides to its new rank,
//! - its opacity (`alpha`) fades in when it enters the top N and out when it leaves,
//! - its numbers glide to their new values like everything else in the app.
//!
//! Rows are matched between readings by process name. The sampler already merges
//! processes that share a name (e.g. a browser's helpers) into one group.

use super::text::{fit, paint};
use crate::anim::Tween;
use crate::config::TOP_N;
use crate::format::human;
use crate::sampler::ProcRow;
use crate::theme::{level_color, MUTED, TEXT};
use eframe::egui::{self, Align2, Color32, FontId, Rect, Sense, TextStyle, Vec2};

const HEADER_H: f32 = 16.0;
const ROW_H: f32 = 18.0;
const COL_GAP: f32 = 8.0;
const NUM_SIZE: f32 = 11.0;
// New rows rise into place from this far below (in rows); departing rows sink this far.
const SLIDE: f32 = 0.5;
// Space between a name and its "×12" process count.
const COUNT_GAP: f32 = 4.0;

/// A process group's row mid-animation. Keyed by name so it keeps its identity while ranks change.
struct Row {
    name: String,
    /// How many processes share this name.
    count: usize,
    /// Vertical position in rows: 0.0 is the top row. Fractional while moving.
    slot: Tween,
    /// Opacity, 0.0..=1.0.
    alpha: Tween,
    cpu: Tween,
    mem: Tween,
    io: Tween,
    /// Still in the latest top N; otherwise it's fading out.
    live: bool,
}

/// The "top processes" table. Rows glide to their new rank when the order
/// changes, fade in when they enter the top N and fade out when they leave.
#[derive(Default)]
pub struct ProcTable {
    /// Every row currently on screen: the live top N plus any still fading out.
    rows: Vec<Row>,
}

impl ProcTable {
    /// Retarget every row at a fresh reading.
    pub fn update(&mut self, procs: &[ProcRow]) {
        // Assume every row has left the top N; the loop below revives the ones that are still in it.
        for row in &mut self.rows {
            if row.live {
                row.live = false;
                row.alpha.set(0.0);
                row.slot.set(row.slot.target() + SLIDE);
            }
        }
        for (rank, p) in procs.iter().enumerate() {
            let slot = rank as f32;
            match self.rows.iter_mut().find(|r| r.name == p.name) {
                // Known group (on screen or still fading out): glide to its new rank and values.
                Some(row) => {
                    row.live = true;
                    row.count = p.count;
                    row.slot.set(slot);
                    row.alpha.set(1.0);
                    row.cpu.set(p.cpu);
                    row.mem.set(p.mem as f32);
                    row.io.set(p.io as f32);
                }
                // New group: start half a row low and transparent, then rise and fade in.
                // Its numbers start at their real values; counting up from zero would be misleading.
                None => {
                    let mut row = Row {
                        name: p.name.clone(),
                        count: p.count,
                        slot: Tween::new(slot + SLIDE),
                        alpha: Tween::new(0.0),
                        cpu: Tween::new(p.cpu),
                        mem: Tween::new(p.mem as f32),
                        io: Tween::new(p.io as f32),
                        live: true,
                    };
                    row.slot.set(slot);
                    row.alpha.set(1.0);
                    self.rows.push(row);
                }
            }
        }
    }

    /// Advance all animations by `dt` seconds of `transition`-second moves and
    /// `fade`-second fades. Returns true while anything is still moving.
    pub fn step(&mut self, dt: f32, transition: f32, fade: f32) -> bool {
        let mut moving = false;
        for row in &mut self.rows {
            // `|`, not `||`: every tween must step.
            moving |= row.slot.step(dt, transition)
                | row.alpha.step(dt, fade)
                | row.cpu.step(dt, transition)
                | row.mem.step(dt, transition)
                | row.io.step(dt, transition);
        }
        // Forget rows that have finished fading out.
        self.rows.retain(|r| r.live || r.alpha.value() > 0.0);
        moving
    }

    pub fn show(&self, ui: &mut egui::Ui) {
        // Always reserve room for TOP_N rows, so the table's height (and everything
        // below it) never jumps when there are fewer processes.
        let width = ui.available_width();
        let (rect, _) = ui.allocate_exact_size(Vec2::new(width, HEADER_H + TOP_N as f32 * ROW_H), Sense::hover());
        let painter = ui.painter_at(rect);

        // Fixed numeric columns sized for their widest plausible value; the name gets the rest.
        // Fixed widths matter: if columns sized to their content, they'd shift every time a value animates.
        let num_font = FontId::monospace(NUM_SIZE);
        let measure = |s: &str| painter.layout_no_wrap(s.to_owned(), num_font.clone(), TEXT).size().x;
        let num_cols = [measure("100.0%"), measure("1023.9 MB"), measure("1023.9 MB")];
        // Lay the columns out right to left as (left x, right x) pairs, then flip to reading order.
        let mut cols = Vec::with_capacity(4);
        let mut right = rect.right();
        for w in num_cols.iter().rev() {
            cols.push((right - w, right));
            right -= w + COL_GAP;
        }
        cols.push((rect.left(), right));
        cols.reverse();
        // The rect of column `col` for a row starting at `top`.
        let cell = |col: usize, top: f32, h: f32| {
            let (l, r) = cols[col];
            Rect::from_min_max(egui::pos2(l, top), egui::pos2(r.max(l), top + h))
        };

        let small = TextStyle::Small.resolve(ui.style());
        for (i, h) in ["Name", "CPU", "Mem", "I/O /s"].into_iter().enumerate() {
            let r = cell(i, rect.top(), HEADER_H);
            let galley = fit(&painter, h.to_owned(), small.clone(), MUTED, r.width(), small.size);
            let align = if i == 0 { Align2::LEFT_CENTER } else { Align2::RIGHT_CENTER };
            paint(&painter, r, align, galley);
        }

        // Rows are clipped to the body, so one sliding in from below the last slot
        // (or sinking out of it) is cut off instead of drawing past the table.
        let body = Rect::from_min_max(egui::pos2(rect.left(), rect.top() + HEADER_H), rect.max);
        let painter = painter.with_clip_rect(body);
        let name_font = TextStyle::Body.resolve(ui.style());
        // Departing rows first, so arriving ones draw on top of them.
        let order = self.rows.iter().filter(|r| !r.live).chain(self.rows.iter().filter(|r| r.live));
        for row in order {
            let alpha = row.alpha.value().clamp(0.0, 1.0);
            let fade = |c: Color32| c.gamma_multiply(alpha);
            let top = body.top() + row.slot.value() * ROW_H;

            paint_name(&painter, cell(0, top, ROW_H), row, &name_font, &small, alpha);

            let cpu = row.cpu.value();
            let cells = [
                (format!("{cpu:.1}%"), fade(level_color(cpu))),
                (human(row.mem.value() as f64), fade(TEXT)),
                (human(row.io.value() as f64), fade(MUTED)),
            ];
            for (i, (text, color)) in cells.into_iter().enumerate() {
                let r = cell(i + 1, top, ROW_H);
                // Numbers may shrink a little before being cut off.
                let galley = fit(&painter, text, num_font.clone(), color, r.width(), 9.0);
                paint(&painter, r, Align2::RIGHT_CENTER, galley);
            }
        }
    }
}

/// "brave.exe ×14": the name, elided if needed, then the group size in muted text.
/// The count is laid out first so a long name is cut instead of the count.
fn paint_name(painter: &egui::Painter, r: Rect, row: &Row, font: &FontId, small: &FontId, alpha: f32) {
    let count = (row.count > 1).then(|| {
        let text = format!("×{}", row.count);
        painter.layout_no_wrap(text, small.clone(), MUTED.gamma_multiply(alpha))
    });
    let count_w = count.as_ref().map_or(0.0, |g| g.size().x + COUNT_GAP);
    let name_w = (r.width() - count_w).max(0.0);
    let name = fit(painter, row.name.clone(), font.clone(), TEXT.gamma_multiply(alpha), name_w, font.size);
    let name_right = r.left() + name.size().x;
    paint(painter, r, Align2::LEFT_CENTER, name);
    if let Some(count) = count {
        let r = Rect::from_min_max(egui::pos2(name_right + COUNT_GAP, r.top()), r.max);
        paint(painter, r, Align2::LEFT_CENTER, count);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{FADE, TRANSITION};

    fn proc(name: &str, cpu: f32) -> ProcRow {
        ProcRow { name: name.to_owned(), count: 1, cpu, mem: 0, io: 0.0 }
    }

    fn step(t: &mut ProcTable) -> bool {
        t.step(1.0 / 60.0, TRANSITION, FADE)
    }

    fn settle(t: &mut ProcTable) {
        for _ in 0..600 {
            step(t);
        }
    }

    fn slot(t: &ProcTable, name: &str) -> f32 {
        t.rows.iter().find(|r| r.name == name).unwrap().slot.value()
    }

    #[test]
    fn rows_follow_rank_changes() {
        let mut t = ProcTable::default();
        t.update(&[proc("a", 50.0), proc("b", 10.0)]);
        settle(&mut t);
        t.update(&[proc("b", 60.0), proc("a", 5.0)]);
        settle(&mut t);
        assert_eq!(slot(&t, "b"), 0.0);
        assert_eq!(slot(&t, "a"), 1.0);
    }

    #[test]
    fn departed_rows_fade_out_then_disappear() {
        let mut t = ProcTable::default();
        t.update(&[proc("a", 50.0), proc("b", 10.0)]);
        settle(&mut t);
        t.update(&[proc("a", 50.0)]);
        step(&mut t);
        assert_eq!(t.rows.len(), 2, "should still be fading");
        settle(&mut t);
        assert_eq!(t.rows.len(), 1);
        assert!(!step(&mut t), "should be at rest");
    }

    #[test]
    fn returning_group_is_revived_not_duplicated() {
        let mut t = ProcTable::default();
        t.update(&[proc("a", 50.0)]);
        t.update(&[]);
        step(&mut t);
        t.update(&[proc("a", 50.0)]);
        assert_eq!(t.rows.len(), 1);
        settle(&mut t);
        assert_eq!(t.rows[0].alpha.value(), 1.0);
    }

    #[test]
    fn group_size_updates_in_place() {
        let mut t = ProcTable::default();
        t.update(&[ProcRow { count: 3, ..proc("brave.exe", 5.0) }]);
        t.update(&[ProcRow { count: 7, ..proc("brave.exe", 5.0) }]);
        assert_eq!(t.rows.len(), 1);
        assert_eq!(t.rows[0].count, 7);
    }
}
