//! Formatting of sizes, dates and colours the way Eagle prints them.

/// Byte size the way Eagle prints it: `512 B`, `5.34 KB`, `17.19 KB`.
pub fn format_size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["KB", "MB", "GB", "TB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut v = bytes as f64 / 1024.0;
    let mut u = 0;
    while v >= 1024.0 && u < UNITS.len() - 1 {
        v /= 1024.0;
        u += 1;
    }
    let s = format!("{v:.2}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    format!("{s} {}", UNITS[u])
}

/// Local date and time the way Eagle prints it: `2026/10/26 2:15`.
pub fn format_ms(ms: u64) -> String {
    use chrono::{Local, TimeZone};
    match Local.timestamp_millis_opt(ms as i64).single() {
        Some(t) => t.format("%Y/%m/%d %-H:%M").to_string(),
        None => "-".into(),
    }
}

/// `#RRGGBB`.
pub fn hex(rgb: [u8; 3]) -> String {
    format!("#{:02X}{:02X}{:02X}", rgb[0], rgb[1], rgb[2])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logic::layout::columns;

    #[test]
    fn formatting() {
        assert_eq!(format_size(512), "512 B");
        assert_eq!(format_size(5465), "5.34 KB");
        assert_eq!(format_size(1536), "1.5 KB");
        assert_eq!(format_size(2048), "2 KB");
        assert_eq!(hex([255, 0, 16]), "#FF0010");
        assert_eq!(columns(300.0, 100.0, 8.0), 2);
    }
}
