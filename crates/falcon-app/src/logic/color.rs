//! RGB and HSV conversion for the colour picker.

/// Converts RGB to `[hue, saturation, value]`, hue in degrees `0..360`,
/// saturation and value in `0..=1`. Greys have hue 0.
pub fn rgb_to_hsv(rgb: [u8; 3]) -> [f32; 3] {
    let [r, g, b] = rgb.map(|c| c as f32 / 255.0);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    let h = if d == 0.0 {
        0.0
    } else if max == r {
        60.0 * ((g - b) / d).rem_euclid(6.0)
    } else if max == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    let s = if max == 0.0 { 0.0 } else { d / max };
    [h.rem_euclid(360.0), s, max]
}

/// Converts `[hue, saturation, value]` back to RGB, rounded. The hue wraps
/// around 360 and saturation and value are clamped to `0..=1`.
pub fn hsv_to_rgb([h, s, v]: [f32; 3]) -> [u8; 3] {
    let s = s.clamp(0.0, 1.0);
    let v = v.clamp(0.0, 1.0);
    let h = h.rem_euclid(360.0) / 60.0;
    let c = v * s;
    let x = c * (1.0 - (h.rem_euclid(2.0) - 1.0).abs());
    let (r, g, b) = match h as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = v - c;
    [r, g, b].map(|k| ((k + m) * 255.0).round().clamp(0.0, 255.0) as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn primaries() {
        assert_eq!(rgb_to_hsv([255, 0, 0]), [0.0, 1.0, 1.0]);
        assert_eq!(rgb_to_hsv([0, 255, 0]), [120.0, 1.0, 1.0]);
        assert_eq!(rgb_to_hsv([0, 0, 255]), [240.0, 1.0, 1.0]);
        assert_eq!(hsv_to_rgb([120.0, 1.0, 1.0]), [0, 255, 0]);
        assert_eq!(hsv_to_rgb([240.0, 1.0, 1.0]), [0, 0, 255]);
    }

    #[test]
    fn black_white_grey() {
        assert_eq!(rgb_to_hsv([0, 0, 0]), [0.0, 0.0, 0.0]);
        assert_eq!(rgb_to_hsv([255, 255, 255]), [0.0, 0.0, 1.0]);
        let g = rgb_to_hsv([128, 128, 128]);
        assert_eq!((g[0], g[1]), (0.0, 0.0));
        assert_eq!(hsv_to_rgb(g), [128, 128, 128]);
        assert_eq!(hsv_to_rgb([200.0, 0.5, 0.0]), [0, 0, 0]);
        assert_eq!(hsv_to_rgb([200.0, 0.0, 1.0]), [255, 255, 255]);
    }

    #[test]
    fn round_trip_grid() {
        for r in (0..=255).step_by(15) {
            for g in (0..=255).step_by(15) {
                for b in (0..=255).step_by(15) {
                    let back = hsv_to_rgb(rgb_to_hsv([r as u8, g as u8, b as u8]));
                    for (a, c) in back.iter().zip([r, g, b]) {
                        assert!((*a as i32 - c).abs() <= 1, "{r},{g},{b} -> {back:?}");
                    }
                }
            }
        }
    }

    #[test]
    fn hue_wraps() {
        assert_eq!(hsv_to_rgb([360.0, 1.0, 1.0]), [255, 0, 0]);
        assert_eq!(hsv_to_rgb([480.0, 1.0, 1.0]), hsv_to_rgb([120.0, 1.0, 1.0]));
        assert_eq!(hsv_to_rgb([-120.0, 1.0, 1.0]), [0, 0, 255]);
    }
}
