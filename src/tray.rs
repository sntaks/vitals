//! Windows only: the notification-area ("hidden icons") icon, showing and hiding
//! the window, and making sure only one copy runs.
//!
//! "Hidden" here means *cloaked*: the compositor stops drawing the window (and it
//! stops taking clicks), the way Windows hides windows on other virtual desktops.
//! Actually hiding it (`ShowWindow(SW_HIDE)` or `ViewportCommand::Visible(false)`)
//! breaks eframe 0.31: a hidden window gets no paint messages, so a repaint that
//! comes due is never served and eframe's event loop spins at 100% of a core
//! waiting for it. A cloaked window is still "shown" as far as Win32, winit and
//! eframe know, so all of that keeps working; it just isn't on screen.
//!
//! Only the owning process can cloak or uncloak a window, so a second launch
//! doesn't touch our window itself: it signals a named event, and a thread here
//! that waits on it shows the window.

use crate::sampler::Pause;
use crate::settings;
use eframe::egui;
use std::cell::RefCell;
use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use windows_sys::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS, HWND};
use windows_sys::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DwmSetWindowAttribute, DWMWA_CLOAK, DWMWA_CLOAKED};
use windows_sys::Win32::System::Threading::{
    CreateEventW, CreateMutexW, OpenEventW, SetEvent, WaitForSingleObject, EVENT_MODIFY_STATE, INFINITE,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{AllowSetForegroundWindow, SetForegroundWindow, ASFW_ANY};

const TITLE: &str = "Vitals";
// Named per sign-in session ("Local\"), so other users get their own copy.
const MUTEX_NAME: &str = r"Local\Vitals.SingleInstance";
/// Signalled by a second launch to ask the running copy to show itself.
const SHOW_EVENT_NAME: &str = r"Local\Vitals.Show";

/// UTF-16 with a terminating NUL, for Win32.
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain([0]).collect()
}

fn set_cloaked(hwnd: HWND, cloaked: bool) {
    let value = cloaked as i32; // a Win32 BOOL
    unsafe { DwmSetWindowAttribute(hwnd, DWMWA_CLOAK as u32, (&raw const value).cast(), size_of::<i32>() as u32) };
}

fn is_cloaked(hwnd: HWND) -> bool {
    // Non-zero if cloaked by anyone: us, or Windows (e.g. on another virtual desktop).
    let mut value = 0u32;
    unsafe { DwmGetWindowAttribute(hwnd, DWMWA_CLOAKED as u32, (&raw mut value).cast(), size_of::<u32>() as u32) };
    value != 0
}

/// Returns false if Vitals is already running, after asking that copy to show itself.
pub fn single_instance() -> bool {
    // The handles are never closed: Windows releases them when this process exits.
    unsafe {
        CreateMutexW(std::ptr::null(), 0, wide(MUTEX_NAME).as_ptr());
        if GetLastError() != ERROR_ALREADY_EXISTS {
            return true;
        }
        let show = OpenEventW(EVENT_MODIFY_STATE, 0, wide(SHOW_EVENT_NAME).as_ptr());
        if !show.is_null() {
            // We were just launched by the user, so we may take the foreground; pass
            // that right on, or the running copy's window would only flash in the taskbar.
            AllowSetForegroundWindow(ASFW_ANY);
            SetEvent(show);
        }
    }
    false
}

/// Shows the window whenever a second launch signals [`SHOW_EVENT_NAME`]. The thread
/// sleeps in the kernel until then, so it costs nothing.
fn listen_for_second_launch(window: Window) {
    let event = unsafe { CreateEventW(std::ptr::null(), 0, 0, wide(SHOW_EVENT_NAME).as_ptr()) };
    if event.is_null() {
        return;
    }
    // Raw handles aren't `Send`; pass it as an integer.
    let event = event as isize;
    let _ = std::thread::Builder::new().name("vitals-show".into()).spawn(move || loop {
        unsafe { WaitForSingleObject(event as _, INFINITE) };
        window.show();
    });
}

/// Shows and hides the app window, pausing the sampler while nobody can see it.
/// Plain data, so the tray's callbacks (which must be `Send + Sync`) can hold a copy.
#[derive(Clone)]
pub struct Window {
    /// The HWND as an integer: raw pointers aren't `Send`.
    hwnd: isize,
    pause: Pause,
    ctx: egui::Context,
}

impl Window {
    fn hwnd(&self) -> HWND {
        self.hwnd as HWND
    }

    pub fn is_visible(&self) -> bool {
        !is_cloaked(self.hwnd())
    }

    /// Uncloaks the window and brings it to the front. Safe from any thread of this process.
    pub fn show(&self) {
        self.pause.wake();
        set_cloaked(self.hwnd(), false);
        unsafe { SetForegroundWindow(self.hwnd()) };
        self.ctx.request_repaint();
    }

    pub fn hide(&self) {
        set_cloaked(self.hwnd(), true);
        self.pause.pause();
    }

    fn toggle(&self) {
        if self.is_visible() {
            self.hide();
        } else {
            self.show();
        }
    }
}

/// What has to stay alive for the icon to stay in the tray. Lives in a
/// thread-local on the main thread: `TrayIcon` isn't `Send`.
struct Installed {
    _icon: TrayIcon,
    autostart: CheckMenuItem,
}

thread_local! {
    static TRAY: RefCell<Option<Installed>> = const { RefCell::new(None) };
}

/// Puts the icon in the tray. Left click shows or hides the window; right click
/// opens the menu. Returns `None` (and the app runs without a tray) if Windows refuses.
///
/// The tray's callbacks run on the main thread (from the event loop's message pump).
pub fn install(hwnd: isize, pause: Pause, ctx: egui::Context) -> Option<Window> {
    let window = Window { hwnd, pause, ctx };

    let show = MenuItem::new("Show Vitals", true, None);
    let autostart = CheckMenuItem::new("Start with Windows", true, settings::autostart(), None);
    let quit = MenuItem::new("Quit", true, None);
    let menu = Menu::with_items(&[&show, &autostart, &PredefinedMenuItem::separator(), &quit]).ok()?;

    let icon = TrayIconBuilder::new()
        .with_tooltip(TITLE)
        .with_icon(icon())
        .with_menu(Box::new(menu))
        .with_menu_on_left_click(false)
        .build()
        .ok()?;

    let w = window.clone();
    TrayIconEvent::set_event_handler(Some(move |event| {
        if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
            w.toggle();
        }
    }));

    let w = window.clone();
    let (show_id, autostart_id, quit_id) = (show.id().clone(), autostart.id().clone(), quit.id().clone());
    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        if event.id == show_id {
            w.show();
        } else if event.id == autostart_id {
            settings::set_autostart(!settings::autostart());
            // The menu already flipped the tick; correct it if the registry write failed.
            TRAY.with_borrow(|t| {
                if let Some(t) = t {
                    t.autostart.set_checked(settings::autostart());
                }
            });
        } else if event.id == quit_id {
            // A hidden window can't run the frame eframe needs to close normally,
            // and there's nothing to save, so just leave (after removing the icon).
            remove();
            std::process::exit(0);
        }
    }));

    TRAY.set(Some(Installed { _icon: icon, autostart }));
    listen_for_second_launch(window.clone());
    Some(window)
}

/// Takes the icon out of the tray. Without this, a dead icon lingers until the
/// mouse passes over it.
pub fn remove() {
    TRAY.take();
}

/// The tray icon, drawn in code so no image file or decoder is needed:
/// a pulse line in the accent colour on a dark rounded square.
fn icon() -> Icon {
    const N: usize = 32;
    // The pulse, in pixel coordinates.
    const LINE: [(f32, f32); 6] = [(4.0, 17.0), (10.0, 17.0), (13.5, 8.0), (18.0, 25.0), (21.5, 14.0), (28.0, 14.0)];
    let (bg, accent) = ([16u8, 18, 24], [96u8, 165, 250]);

    let mut rgba = Vec::with_capacity(N * N * 4);
    for y in 0..N {
        for x in 0..N {
            let p = (x as f32 + 0.5, y as f32 + 0.5);
            // Coverage of the rounded square (radius 7), anti-aliased over one pixel.
            let edge = rounded_square_distance(p, N as f32, 7.0);
            let fill = (0.5 - edge).clamp(0.0, 1.0);
            // Coverage of the line, 2.4 px wide.
            let d = LINE.windows(2).map(|s| segment_distance(p, s[0], s[1])).fold(f32::MAX, f32::min);
            let ink = (1.7 - d).clamp(0.0, 1.0);
            let mix = |b: u8, a: u8| (b as f32 + (a as f32 - b as f32) * ink).round() as u8;
            rgba.extend_from_slice(&[
                mix(bg[0], accent[0]),
                mix(bg[1], accent[1]),
                mix(bg[2], accent[2]),
                (fill * 255.0).round() as u8,
            ]);
        }
    }
    Icon::from_rgba(rgba, N as u32, N as u32).expect("icon is 32x32 RGBA")
}

/// Signed distance from `p` to the edge of a `size` square with corners of radius `r`
/// (negative inside).
fn rounded_square_distance(p: (f32, f32), size: f32, r: f32) -> f32 {
    let half = size / 2.0;
    let qx = (p.0 - half).abs() - (half - r);
    let qy = (p.1 - half).abs() - (half - r);
    let outside = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt();
    outside + qx.max(qy).min(0.0) - r
}

/// Distance from `p` to the segment `a`-`b`.
fn segment_distance(p: (f32, f32), a: (f32, f32), b: (f32, f32)) -> f32 {
    let (abx, aby) = (b.0 - a.0, b.1 - a.1);
    let t = (((p.0 - a.0) * abx + (p.1 - a.1) * aby) / (abx * abx + aby * aby)).clamp(0.0, 1.0);
    let (dx, dy) = (p.0 - (a.0 + abx * t), p.1 - (a.1 + aby * t));
    (dx * dx + dy * dy).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rounded_square_inside_and_corners() {
        assert!(rounded_square_distance((16.0, 16.0), 32.0, 7.0) < 0.0);
        // The very corner pixel is cut off by the rounding.
        assert!(rounded_square_distance((0.5, 0.5), 32.0, 7.0) > 0.0);
    }

    #[test]
    fn segment_distance_clamps_to_ends() {
        assert_eq!(segment_distance((5.0, 1.0), (0.0, 0.0), (10.0, 0.0)), 1.0);
        assert_eq!(segment_distance((-3.0, 4.0), (0.0, 0.0), (10.0, 0.0)), 5.0);
    }
}
