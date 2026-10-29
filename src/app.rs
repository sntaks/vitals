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
use crate::config::{FADE, FRAME, HISTORY, REFRESH, TRANSITION};
use crate::format::{human, human_pair};
use crate::sampler::{self, Sample};
use crate::settings;
use crate::theme::{BG, BORDER, MUTED, TEXT};
use crate::widgets::{core_bars, kv_row, metric_row, section_title, sparkline, ProcTable};
use eframe::egui::{self, CornerRadius, RichText, Sense, Stroke};
use std::collections::VecDeque;
use std::sync::mpsc::Receiver;
use std::time::Duration;

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
    fn step(&mut self, dt: f32, transition: f32) -> bool {
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
            moving |= tween.step(dt, transition);
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
    /// Snap to new readings instead of animating: ~1 frame per reading instead of ~15.
    reduce_motion: bool,
    /// Whether the previous frame asked for another one to continue an animation.
    animating: bool,
    /// Windows: the tray icon's handle on our window. `None` elsewhere, or if the tray failed.
    #[cfg(windows)]
    window: Option<crate::tray::Window>,
}

impl Vitals {
    pub fn new(cc: &eframe::CreationContext, start_hidden: bool) -> Self {
        let ctx = &cc.egui_ctx;
        let (samples, pause) = sampler::spawn(ctx.clone(), !start_hidden);
        #[cfg(windows)]
        let window = hwnd(cc).and_then(|hwnd| crate::tray::install(hwnd, pause.clone(), ctx.clone()));
        // Hidden before eframe first shows it, so nothing flashes on screen. Without a
        // tray icon there'd be no way to bring it back, so it then starts visible.
        #[cfg(windows)]
        match (&window, start_hidden) {
            (Some(window), true) => window.hide(),
            (None, true) => pause.wake(),
            _ => {}
        }
        // Without a tray nothing ever hides the window, so the sampler never pauses.
        #[cfg(not(windows))]
        drop(pause);
        Self {
            samples,
            latest: None,
            cpu_history: VecDeque::with_capacity(HISTORY + 1),
            gauges: Gauges::default(),
            procs: ProcTable::default(),
            reduce_motion: settings::reduce_motion(),
            animating: false,
            #[cfg(windows)]
            window,
        }
    }

    /// Records a new reading and starts every transition toward it.
    fn ingest(&mut self, sample: Sample) {
        // A gap in the readings (the window was hidden, or the machine slept) would
        // otherwise draw as a straight line across time nobody measured.
        let gap = self.latest.as_ref().is_some_and(|l| sample.at.duration_since(l.at) > REFRESH * 3);
        if gap {
            self.cpu_history.clear();
        }
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
        if self.reduce_motion {
            return 1.0;
        }
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

        // Seconds since the last frame. Mid-animation that's the real time elapsed, capped
        // so a stall (window dragged, machine busy) slows the animation down instead of
        // skipping it. Starting from rest, the gap since the last frame was idle time, so
        // count one frame. (Not egui's `stable_dt`: it only trusts the real time after an
        // *immediate* repaint and otherwise assumes one monitor refresh, so at 30 fps on a
        // 60 Hz screen every animation would run at half speed and never come to rest.)
        let dt = if self.animating { ctx.input(|i| i.unstable_dt).min(0.1) } else { FRAME.as_secs_f32() };
        let (transition, fade) = if self.reduce_motion { (0.0, 0.0) } else { (TRANSITION, FADE) };
        let moving = self.gauges.step(dt, transition) | self.procs.step(dt, transition, fade);
        // Keep painting while a transition runs. Once everything is at rest, sleep;
        // the sampler thread wakes us with the next reading. Every frame costs CPU in
        // egui's renderer whether or not anything changed, so idling matters.
        // (Mouse movement and clicks also wake egui on their own.)
        self.animating = moving || self.graph_progress() < 1.0;
        if self.animating {
            // egui starts a delayed repaint one predicted frame (a monitor refresh) early,
            // assuming drawing takes that long. Ours takes about a millisecond, so add it
            // back, or FRAME = 33 ms would really run at 60 fps.
            let predicted = ctx.input(|i| i.predicted_dt);
            ctx.request_repaint_after(FRAME + Duration::from_secs_f32(predicted));
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
                    if ui.checkbox(&mut self.reduce_motion, "Reduce motion").changed() {
                        settings::set_reduce_motion(self.reduce_motion);
                    }
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
    /// Title and close button. On Windows the button hides the window to the tray;
    /// quitting is in the tray's and the right-click menus.
    fn header(&self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Vitals").color(TEXT).strong().size(15.0));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let close = ui.add(egui::Button::new(RichText::new("×").size(18.0).color(MUTED)).frame(false));
                if close.clicked() {
                    #[cfg(windows)]
                    if let Some(window) = &self.window {
                        window.hide();
                        return;
                    }
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

/// The window's Win32 handle, as an integer.
#[cfg(windows)]
fn hwnd(cc: &eframe::CreationContext) -> Option<isize> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    match cc.window_handle().ok()?.as_raw() {
        RawWindowHandle::Win32(h) => Some(h.hwnd.get()),
        _ => None,
    }
}
