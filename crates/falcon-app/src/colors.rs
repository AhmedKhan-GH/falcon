//! Colour strings for the swatch menu, in the formats Eagle copies. The
//! conversions follow the npm `color-convert` package; every number is
//! rounded to the nearest integer (halves away from zero).

fn unit(rgb: [u8; 3]) -> (f64, f64, f64) {
    (f64::from(rgb[0]) / 255.0, f64::from(rgb[1]) / 255.0, f64::from(rgb[2]) / 255.0)
}

/// Hue in degrees, 0 for greys.
fn hue(rgb: [u8; 3]) -> f64 {
    let (r, g, b) = unit(rgb);
    let min = r.min(g).min(b);
    let max = r.max(g).max(b);
    let delta = max - min;
    if delta == 0.0 {
        return 0.0;
    }
    let h = if r == max {
        (g - b) / delta
    } else if g == max {
        2.0 + (b - r) / delta
    } else {
        4.0 + (r - g) / delta
    };
    let h = (h * 60.0).min(360.0);
    if h < 0.0 { h + 360.0 } else { h }
}

/// `#rrggbb`, lower case.
pub fn hex(rgb: [u8; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}", rgb[0], rgb[1], rgb[2])
}

/// `rgb(r, g, b)`.
pub fn rgb(c: [u8; 3]) -> String {
    format!("rgb({}, {}, {})", c[0], c[1], c[2])
}

/// `rgba(r, g, b, 1)`.
pub fn rgba(c: [u8; 3]) -> String {
    format!("rgba({}, {}, {}, 1)", c[0], c[1], c[2])
}

/// `hsl(h, s%, l%)`.
pub fn hsl(c: [u8; 3]) -> String {
    let (r, g, b) = unit(c);
    let min = r.min(g).min(b);
    let max = r.max(g).max(b);
    let delta = max - min;
    let l = (min + max) / 2.0;
    let s = if delta == 0.0 {
        0.0
    } else if l <= 0.5 {
        delta / (max + min)
    } else {
        delta / (2.0 - max - min)
    };
    format!("hsl({}, {}%, {}%)", hue(c).round(), (s * 100.0).round(), (l * 100.0).round())
}

/// `hsv(h, s%, v%)`.
pub fn hsv(c: [u8; 3]) -> String {
    let (r, g, b) = unit(c);
    let min = r.min(g).min(b);
    let max = r.max(g).max(b);
    let s = if max == 0.0 { 0.0 } else { (max - min) / max };
    format!("hsv({}, {}%, {}%)", hue(c).round(), (s * 100.0).round(), (max * 100.0).round())
}

/// `hwb(h, w%, b%)`.
pub fn hwb(c: [u8; 3]) -> String {
    let (r, g, b) = unit(c);
    let w = r.min(g).min(b);
    let bl = 1.0 - r.max(g).max(b);
    format!("hwb({}, {}%, {}%)", hue(c).round(), (w * 100.0).round(), (bl * 100.0).round())
}

/// `cmyk(c%, m%, y%, k%)`.
pub fn cmyk(c: [u8; 3]) -> String {
    let (r, g, b) = unit(c);
    let k = (1.0 - r).min(1.0 - g).min(1.0 - b);
    let part = |v: f64| if k == 1.0 { 0.0 } else { (1.0 - v - k) / (1.0 - k) };
    format!(
        "cmyk({}%, {}%, {}%, {}%)",
        (part(r) * 100.0).round(),
        (part(g) * 100.0).round(),
        (part(b) * 100.0).round(),
        (k * 100.0).round()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn red() {
        let c = [255, 0, 0];
        assert_eq!(hex(c), "#ff0000");
        assert_eq!(rgb(c), "rgb(255, 0, 0)");
        assert_eq!(rgba(c), "rgba(255, 0, 0, 1)");
        assert_eq!(hsl(c), "hsl(0, 100%, 50%)");
        assert_eq!(hsv(c), "hsv(0, 100%, 100%)");
        assert_eq!(hwb(c), "hwb(0, 0%, 0%)");
        assert_eq!(cmyk(c), "cmyk(0%, 100%, 100%, 0%)");
    }

    #[test]
    fn greys() {
        assert_eq!(cmyk([0, 0, 0]), "cmyk(0%, 0%, 0%, 100%)");
        assert_eq!(hsl([0, 0, 0]), "hsl(0, 0%, 0%)");
        assert_eq!(hsv([0, 0, 0]), "hsv(0, 0%, 0%)");
        assert_eq!(hwb([0, 0, 0]), "hwb(0, 0%, 100%)");
        assert_eq!(hsl([255, 255, 255]), "hsl(0, 0%, 100%)");
        assert_eq!(hwb([255, 255, 255]), "hwb(0, 100%, 0%)");
        assert_eq!(cmyk([255, 255, 255]), "cmyk(0%, 0%, 0%, 0%)");
        assert_eq!(hsl([128, 128, 128]), "hsl(0, 0%, 50%)");
        assert_eq!(cmyk([128, 128, 128]), "cmyk(0%, 0%, 0%, 50%)");
    }

    #[test]
    fn mid_colour() {
        let c = [18, 115, 251];
        assert_eq!(hex(c), "#1273fb");
        assert_eq!(hsl(c), "hsl(215, 97%, 53%)");
        assert_eq!(hsv(c), "hsv(215, 93%, 98%)");
        assert_eq!(hwb(c), "hwb(215, 7%, 2%)");
        assert_eq!(cmyk(c), "cmyk(93%, 54%, 0%, 2%)");
    }

    #[test]
    fn hue_wraps_for_magenta_side() {
        assert_eq!(hsl([255, 0, 128]), "hsl(330, 100%, 50%)");
        assert_eq!(hsv([0, 255, 0]), "hsv(120, 100%, 100%)");
        assert_eq!(hsv([0, 0, 255]), "hsv(240, 100%, 100%)");
    }
}
