//! Colours, and the green/amber/red scale used for load levels.

use eframe::egui::Color32;

// Colors
pub const BG: Color32 = Color32::from_rgba_premultiplied(16, 18, 24, 236);
pub const BORDER: Color32 = Color32::from_rgb(44, 48, 60);
pub const TEXT: Color32 = Color32::from_rgb(230, 233, 240);
pub const MUTED: Color32 = Color32::from_rgb(130, 137, 155);
pub const ACCENT: Color32 = Color32::from_rgb(96, 165, 250);
pub const TRACK: Color32 = Color32::from_rgb(38, 42, 54);

const CALM: Color32 = Color32::from_rgb(74, 222, 128); // Green
const BUSY: Color32 = Color32::from_rgb(251, 191, 36); // Amber
const OVERLOADED: Color32 = Color32::from_rgb(248, 113, 113); // Red

/// calm: Green (< 50%), busy: Amber (< 80%), overloaded: Red.
/// Blends across a narrow band around each threshold, so an animating value
/// slides between colors instead of snapping.
pub fn level_color(pct: f32) -> Color32 {
    let to_busy = smoothstep(45.0, 55.0, pct);
    let to_overloaded = smoothstep(75.0, 85.0, pct);
    lerp_color(lerp_color(CALM, BUSY, to_busy), OVERLOADED, to_overloaded)
}

/// Mixes two colours: `t = 0` gives `a`, `t = 1` gives `b`.
/// Mixes channel by channel in sRGB, which is plenty for colours this close together.
pub fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let mix = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgba_premultiplied(mix(a.r(), b.r()), mix(a.g(), b.g()), mix(a.b(), b.b()), mix(a.a(), b.a()))
}

/// 0 below `edge0`, 1 above `edge1`, and an S-curve in between (no sharp corners at either end).
fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn level_color_keeps_pure_colors_away_from_thresholds() {
        assert_eq!(level_color(10.0), CALM);
        assert_eq!(level_color(65.0), BUSY);
        assert_eq!(level_color(95.0), OVERLOADED);
    }

    #[test]
    fn level_color_blends_at_thresholds() {
        let mid = level_color(50.0);
        assert_ne!(mid, CALM);
        assert_ne!(mid, BUSY);
    }
}
