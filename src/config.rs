//! Every tunable number in one place. Change these and rebuild.

use std::time::Duration;

// Refresh Rate: How often numbers are sampled from the OS
pub const REFRESH: Duration = Duration::from_millis(1000);
// Seconds of CPU History in the graph
pub const HISTORY: usize = 60;
// How many processes to list
pub const TOP_N: usize = 5;
// Window Size
pub const WINDOW_SIZE: [f32; 2] = [310.0, 480.0];

// Frame pacing while animating (~60 fps). Repainting faster burns CPU for no visible gain.
pub const FRAME: Duration = Duration::from_millis(16);

// Animation timings, in seconds. Each new reading kicks off a transition this long; when
// it's done the app stops repainting until the next reading. Every frame costs CPU
// (rendering, not our code), so keep these well under REFRESH.
pub const TRANSITION: f32 = 0.5;
pub const FADE: f32 = 0.3;
