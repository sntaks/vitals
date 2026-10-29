//! The app itself. Each frame it:
//!
//! 1. picks up any new readings from the sampler thread and points every animated
//!    value at them,
//! 2. advances the animations by the time since the last frame,
//! 3. decides whether another frame is needed soon (only while something moves),
//! 4. lays out and paints the window.
//!
//! All the OS work happens in `sampler.rs`; all the drawing detail lives in `widgets/`.

use crate::anim::{ease_out, Tween};
use crate::config::{FRAME, HISTORY, TRANSITION};
use crate::format::{human, human_pair};
use crate::sampler::{self, Sample};
use crate::theme::{BG, BORDER, MUTED, TEXT};
use crate::widgets::{core_bars, kv_row, metric_row, section_title, sparkline, ProcTable};
use eframe::egui::{self, CornerRadius, RichText, Sense, Stroke};
use std::collections::VecDeque;
use std::sync::mpsc::Receiver;

/// The on-screen value of every animated figure. Widgets draw `tween.value()`,
/// which glides toward the latest reading, never the raw reading itself.
#[derive(Default)]
struct Gauges {
    cpu: Tween,
    cores: Vec<Tween>,
    mem_used: Tween,
    disk_pct: Tween,
    disk_read: Tween,
    disk_write: Tween,
    net_down: Tween,
    net_up: Tween,
}

impl Gauges {
    /// Starts a transition from whatever is on screen now to the new reading.
    fn retarget(&mut self, s: &Sample) {
        self.cpu.set(s.cpu);
        // Sized on the first reading; new cores start at 0 and animate up.
        self.cores.resize(s.cores.len(), Tween::default());
        for (core, &c) in self.cores.iter_mut().zip(&s.cores) {
            core.set(c);
        }
        self.mem_used.set(s.mem_used as f32);
        self.disk_pct.set(s.disk_used as f32 / s.disk_total.max(1) as f32 * 100.0);
        self.disk_read.set(s.disk_read as f32);
        self.disk_write.set(s.disk_write as f32);
        self.net_down.set(s.net_down as f32);
        self.net_up.set(s.net_up as f32);
    }

    /// Returns true while anything is still moving.
    fn step(&mut self, dt: f32) -> bool {
        let mut moving = false;
        for tween in [
            &mut self.cpu,
            &mut self.mem_used,
            &mut self.disk_pct,
            &mut self.disk_read,
            &mut self.disk_write,
            &mut self.net_down,
            &mut self.net_up,
        ]
        .into_iter()
        .chain(self.cores.iter_mut())
        {
            moving |= tween.step(dt, TRANSITION);
        }
        moving
    }
}

// The App
pub struct Vitals {
    /// Readings arrive here from the sampler thread.
    samples: Receiver<Sample>,
    /// The newest reading; `None` until the first one arrives (~200 ms after start).
    /// Used for values that aren't animated, like totals, and for the graph's timing.
    latest: Option<Sample>,
    /// Raw CPU readings, oldest first. One more than the graph spans, so its
    /// left edge stays covered while it scrolls.
    cpu_history: VecDeque<f32>,
    gauges: Gauges,
    /// The process table keeps its own per-row animations.
    procs: ProcTable,
}

impl Vitals {
    pub fn new(ctx: &egui::Context) -> Self {
        Self {
            samples: sampler::spawn(ctx.clone()),
            latest: None,
            cpu_history: VecDeque::with_capacity(HISTORY + 1),
            gauges: Gauges::default(),
            procs: ProcTable::default(),
        }
    }

    /// Records a new reading and starts every transition toward it.
    fn ingest(&mut self, sample: Sample) {
        if self.cpu_history.len() == HISTORY + 1 {
            self.cpu_history.pop_front();
        }
        self.cpu_history.push_back(sample.cpu);
        self.gauges.retarget(&sample);
        self.procs.update(&sample.procs);
        self.latest = Some(sample);
    }

    /// How far the graph is through its transition to the latest reading, 0..=1 (linear).
    /// Based on the reading's timestamp rather than frame time, so it stays correct
    /// even if frames were skipped.
    fn graph_progress(&self) -> f32 {
        self.latest.as_ref().map_or(1.0, |s| (s.at.elapsed().as_secs_f32() / TRANSITION).min(1.0))
    }
}

// Draws window every frame
impl eframe::App for Vitals {
    /// called everytime the windows needs drawing
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Several readings can be waiting if we weren't drawing (e.g. minimised): take them all
        // so the graph has no gaps.
        while let Ok(sample) = self.samples.try_recv() {
            self.ingest(sample);
        }

        // Seconds since the last frame. Capped so a stall (window dragged, machine busy)
        // slows the animation down instead of skipping it.
        let dt = ctx.input(|i| i.stable_dt).min(0.1);
        let moving = self.gauges.step(dt) | self.procs.step(dt);
        // Keep painting while a transition runs. Once everything is at rest, sleep;
        // the sampler thread wakes us with the next reading. Every frame costs CPU in
        // egui's renderer whether or not anything changed, so idling matters.
        // (Mouse movement and clicks also wake egui on their own.)
        if moving || self.graph_progress() < 1.0 {
            ctx.request_repaint_after(FRAME);
        }

        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ctx, |ui| {
                // Drag anywhere to move
                // Right click for the menu
                // An invisible interactive area under everything. Widgets added later sit on
                // top of it, so buttons still get their clicks first.
                let bg = ui.interact(ui.max_rect(), ui.id().with("drag"), Sense::click_and_drag());
                if bg.drag_started() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
                }
                bg.context_menu(|ui| {
                    if ui.button("Quit").clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });

                egui::Frame::new()
                    .fill(BG)
                    .stroke(Stroke::new(1.0, BORDER))
                    .corner_radius(CornerRadius::same(14))
                    .inner_margin(egui::Margin::same(14))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        // The header stays put; only the body scrolls.
                        self.header(ui, ctx);
                        // Anything taller than the window scrolls instead of being cut off.
                        // Wheel only: dragging the body still moves the window.
                        // auto_shrink([false, true]): fill the width, but only be as tall as the
                        // content, so the card hugs short content and caps at the window height.
                        egui::ScrollArea::vertical()
                            .auto_shrink([false, true])
                            .drag_to_scroll(false)
                            .show(ui, |ui| self.body(ui));
                    })
            });
    }

    /// Fully transparent window background, so only the rounded card shows
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0; 4]
    }
}

impl Vitals {
    /// Title and close button.
    fn header(&self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Vitals").color(TEXT).strong().size(15.0));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let close = ui.add(egui::Button::new(RichText::new("×").size(18.0).color(MUTED)).frame(false));
                if close.clicked() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
        });
    }

    /// Everything below the header. Animated figures come from `self.gauges`;
    /// totals don't change, so they're read straight from the latest reading.
    fn body(&self, ui: &mut egui::Ui) {
        let g = &self.gauges;
        let (mem_total, disk_total) = self.latest.as_ref().map_or((0, 0), |s| (s.mem_total, s.disk_total));
        ui.spacing_mut().item_spacing.y = 6.0;

        // CPU
        section_title(ui, "CPU");
        // right-aligned in 5 characters with 1 decimal, so " 4.2%" and "48.0%" take the same width.
        let cpu = g.cpu.value();
        metric_row(ui, "Total", cpu, format!("{cpu:>5.1}%"));
        // The graph's timing gets the same ease-out curve as the tweens, so everything moves together.
        sparkline(ui, &self.cpu_history, ease_out(self.graph_progress()));
        let cores: Vec<f32> = g.cores.iter().map(Tween::value).collect();
        core_bars(ui, &cores);

        // Memory
        section_title(ui, "SYSTEM");
        let mem_used = g.mem_used.value() as f64;
        let mem_pct = (mem_used / mem_total.max(1) as f64 * 100.0) as f32;
        metric_row(ui, "Memory", mem_pct, human_pair(mem_used, mem_total as f64));

        // Disk
        // "n/a" if no drive was found (disk_total of 0), rather than a misleading "0% full".
        let disk_pct = g.disk_pct.value();
        let disk_value = if disk_total == 0 { "n/a".to_owned() } else { format!("{disk_pct:.0}% full") };
        metric_row(ui, "Disk", disk_pct, disk_value);

        // Disk I/O
        // On Windows this counts all process I/O, not just disk: see docs/sampling.md.
        let rate = |t: &Tween| human(t.value() as f64);
        kv_row(ui, "Disk I/O", format!("R {}/s  W {}/s", rate(&g.disk_read), rate(&g.disk_write)));

        // Network
        kv_row(ui, "Network", format!("↓ {}/s  ↑ {}/s", rate(&g.net_down), rate(&g.net_up)));

        // Top processes table
        section_title(ui, "TOP PROCESSES");
        self.procs.show(ui);
    }
}
