// An attribute, instruction to the compiler:
// Hide the black console window on Windows when build in release mode
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod anim; // Tween: values that glide to new targets
mod app; // the eframe App: takes readings, runs animations, lays out the window
mod config; // tunable constants: refresh rate, sizes, animation timings
mod format; // byte counts -> "1.2 GB"
mod sampler; // background thread that reads the OS via sysinfo
mod settings; // the few settings saved between runs
mod theme; // colours
#[cfg(windows)]
mod tray; // Windows: tray icon, show/hide, single instance
mod widgets; // custom-drawn bars, graph, core bars, process table

use eframe::egui;

// returns Result: success or error
fn main() -> eframe::Result {
    // Already running (e.g. started at sign-in)? That copy has been asked to show itself instead.
    #[cfg(windows)]
    if !tray::single_instance() {
        return Ok(());
    }
    // `--hidden` starts straight in the tray; "Start with Windows" launches it that way.
    // Without a tray there'd be no way to reveal the window, so other platforms ignore it.
    let start_hidden = cfg!(windows) && std::env::args().any(|a| a == "--hidden");

    //standard window settings e.g. normal window with title bar
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Vitals")
            .with_inner_size(config::WINDOW_SIZE)
            .with_decorations(false) // No title bar
            .with_transparent(true) // lets rounded corners show
            .with_always_on_top()
            .with_resizable(false)
            // Windows: it lives in the tray instead (see tray.rs).
            .with_taskbar(!cfg!(windows)),
        ..Default::default()
    };
    // The closure runs once the window exists. The app needs the egui context up front
    // so its sampler thread can wake the UI when a new reading arrives.
    let result = eframe::run_native(
        "Vitals",
        options,
        Box::new(move |cc| Ok(Box::new(app::Vitals::new(cc, start_hidden)))),
    );
    #[cfg(windows)]
    tray::remove();
    result
}
