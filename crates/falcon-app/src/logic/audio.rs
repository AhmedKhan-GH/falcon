//! Audio playback: seek bar and playhead maths, time labels.

use super::filters::format_duration;

/// The fraction of a seek bar at pointer `x`, clamped to 0..=1.
pub fn seek_fraction(x: f32, left: f32, width: f32) -> f32 {
    if width <= 0.0 { 0.0 } else { ((x - left) / width).clamp(0.0, 1.0) }
}

/// The time a click at `x` on a seek bar seeks to.
pub fn seek_time(x: f32, left: f32, width: f32, duration: f64) -> f64 {
    f64::from(seek_fraction(x, left, width)) * duration.max(0.0)
}

/// Where the playhead sits on a seek bar for position `pos` (seconds).
pub fn playhead_x(pos: f64, duration: f64, left: f32, width: f32) -> f32 {
    let f = if duration > 0.0 { (pos / duration).clamp(0.0, 1.0) } else { 0.0 };
    left + f as f32 * width
}

/// How many of `n` waveform peaks are already played at `pos`.
pub fn played_peaks(pos: f64, duration: f64, n: usize) -> usize {
    if duration <= 0.0 { 0 } else { ((pos / duration).clamp(0.0, 1.0) * n as f64).round() as usize }
}

/// The elapsed / total text beside the waveform: `0:12 / 3:04`.
pub fn time_label(pos: f64, duration: f64) -> String {
    format!("{} / {}", format_duration(pos.min(duration.max(0.0))), format_duration(duration))
}

#[cfg(test)]
mod audio_tests {
    use super::*;

    #[test]
    fn seek_bar_mapping() {
        assert_eq!(seek_fraction(50.0, 0.0, 200.0), 0.25);
        assert_eq!(seek_fraction(-10.0, 0.0, 200.0), 0.0);
        assert_eq!(seek_fraction(500.0, 0.0, 200.0), 1.0);
        assert_eq!(seek_fraction(5.0, 0.0, 0.0), 0.0);
        assert_eq!(seek_time(150.0, 100.0, 100.0, 60.0), 30.0);
        assert_eq!(playhead_x(30.0, 60.0, 100.0, 100.0), 150.0);
        assert_eq!(playhead_x(90.0, 60.0, 100.0, 100.0), 200.0);
        assert_eq!(playhead_x(5.0, 0.0, 100.0, 100.0), 100.0);
        // A click puts the playhead where it was clicked.
        let t = seek_time(173.0, 100.0, 400.0, 245.0);
        assert!((playhead_x(t, 245.0, 100.0, 400.0) - 173.0).abs() < 1e-3);
        assert_eq!(played_peaks(30.0, 60.0, 1000), 500);
        assert_eq!(played_peaks(1.0, 0.0, 1000), 0);
        assert_eq!(time_label(12.4, 184.0), "0:12 / 3:04");
        assert_eq!(time_label(200.0, 184.0), "3:04 / 3:04");
    }
}
