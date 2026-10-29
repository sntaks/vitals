//! Human-readable byte counts. Units are 1024-based (like Windows Explorer and
//! Task Manager), labelled KB/MB/GB rather than KiB/MiB/GiB.

const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];

/// Picks the unit for a byte count and returns (scaled value, unit index).
/// Promotes to the next unit when *rounding* would print 1024, so 1023.97 KB
/// shows as "1.0 MB" rather than "1024.0 KB".
fn scale(bytes: f64) -> (f64, usize) {
    let mut v = bytes.max(0.0);
    let mut i = 0;
    while i < UNITS.len() - 1 {
        let precision = if i == 0 { 1.0 } else { 10.0 };
        if (v * precision).round() / precision < 1024.0 {
            break;
        }
        v /= 1024.0;
        i += 1;
    }
    (v, i)
}

/// Bytes print as whole numbers ("512 B"); bigger units get one decimal ("3.4 KB").
fn number(v: f64, unit: usize) -> String {
    if unit == 0 {
        format!("{v:.0}")
    } else {
        format!("{v:.1}")
    }
}

/// Turns a number of bytes into '512 B', '3.4 KB', '1.2 GB'
pub fn human(bytes: f64) -> String {
    let (v, i) = scale(bytes);
    format!("{} {}", number(v, i), UNITS[i])
}

/// "8.2 / 15.9 GB": a used/total pair, sharing the unit when both land on the same one.
pub fn human_pair(used: f64, total: f64) -> String {
    let (u, ui) = scale(used);
    let (t, ti) = scale(total);
    if ui == ti {
        format!("{} / {} {}", number(u, ui), number(t, ti), UNITS[ti])
    } else {
        format!("{} / {}", human(used), human(total))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn human_picks_units() {
        assert_eq!(human(0.0), "0 B");
        assert_eq!(human(512.0), "512 B");
        assert_eq!(human(3.4 * 1024.0), "3.4 KB");
        assert_eq!(human(1.2 * 1024.0 * 1024.0 * 1024.0), "1.2 GB");
    }

    #[test]
    fn human_promotes_instead_of_printing_1024() {
        assert_eq!(human(1023.7), "1.0 KB");
        assert_eq!(human(1023.97 * 1024.0), "1.0 MB");
    }

    #[test]
    fn human_caps_at_largest_unit_and_ignores_negatives() {
        assert_eq!(human(2048.0 * 1024f64.powi(4)), "2048.0 TB");
        assert_eq!(human(-5.0), "0 B");
    }

    #[test]
    fn human_pair_shares_unit() {
        let gb = 1024f64.powi(3);
        assert_eq!(human_pair(8.2 * gb, 15.9 * gb), "8.2 / 15.9 GB");
        assert_eq!(human_pair(512.0 * 1024f64.powi(2), 2.0 * gb), "512.0 MB / 2.0 GB");
    }
}
