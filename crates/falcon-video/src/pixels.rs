//! Pixel conversions: the decoder's BGRA rows (with padding and a track
//! rotation) into tightly packed RGBA.

pub use falcon_ffmpeg::Frame;

/// Clockwise quarter turns (0 to 3) for a track's affine transform
/// `[a b c d]`: an iPhone portrait clip carries `[0 1 -1 0]`, one turn.
pub fn quarter_turns(a: f64, b: f64) -> u8 {
    if a == 0.0 && b == 0.0 {
        return 0;
    }
    let turns = (b.atan2(a) / std::f64::consts::FRAC_PI_2).round() as i64;
    turns.rem_euclid(4) as u8
}

/// Packs `height` rows of `width` BGRA pixels, `stride` bytes apart, into
/// RGBA, turned clockwise `turns` quarter turns. Rows missing from `src`
/// come out black rather than panicking.
pub fn from_bgra(src: &[u8], width: usize, height: usize, stride: usize, turns: u8) -> Frame {
    let (w, h) = (width, height);
    let (ow, oh) = if turns % 2 == 1 { (h, w) } else { (w, h) };
    if w == 0 || h == 0 {
        return Frame { width: ow, height: oh, rgba: Vec::new() };
    }
    let mut rgba = vec![0u8; ow * oh * 4];
    if turns.is_multiple_of(4) {
        // The common case, a row at a time.
        for (y, out) in rgba.chunks_exact_mut(w * 4).enumerate() {
            let row = src.get(y * stride..).unwrap_or(&[]);
            for (d, px) in out.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                if let Some(s) = row.get(d * 4..d * 4 + 4) {
                    *px = [s[2], s[1], s[0], 255];
                } else {
                    px[3] = 255;
                }
            }
        }
        return Frame { width: ow, height: oh, rgba };
    }
    for oy in 0..oh {
        for ox in 0..ow {
            // The source pixel that lands at (ox, oy) after the turn.
            let (sx, sy) = match turns % 4 {
                1 => (oy, h - 1 - ox),
                2 => (w - 1 - ox, h - 1 - oy),
                3 => (w - 1 - oy, ox),
                _ => (ox, oy),
            };
            let s = sy * stride + sx * 4;
            let d = (oy * ow + ox) * 4;
            if let Some(px) = src.get(s..s + 4) {
                rgba[d] = px[2];
                rgba[d + 1] = px[1];
                rgba[d + 2] = px[0];
            }
            rgba[d + 3] = 255;
        }
    }
    Frame { width: ow, height: oh, rgba }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 3×2 BGRA picture with 4 bytes of row padding; pixel n has blue n.
    fn sample() -> Vec<u8> {
        let mut v = Vec::new();
        for y in 0..2u8 {
            for x in 0..3u8 {
                v.extend_from_slice(&[y * 3 + x, 0, 200, 0]);
            }
            v.extend_from_slice(&[9, 9, 9, 9]);
        }
        v
    }

    fn blues(f: &Frame) -> Vec<u8> {
        f.rgba.as_chunks::<4>().0.iter().map(|p| p[2]).collect()
    }

    #[test]
    fn bgra_rows_become_rgba() {
        let f = from_bgra(&sample(), 3, 2, 16, 0);
        assert_eq!((f.width, f.height), (3, 2));
        assert_eq!(f.pixel(1, 1), [200, 0, 4, 255]);
        assert_eq!(blues(&f), vec![0, 1, 2, 3, 4, 5]);
        assert_eq!(f.mean(), [200.0, 0.0, 2.5]);
    }

    #[test]
    fn turns_rotate_clockwise() {
        // 0 1 2      turned once:  3 0
        // 3 4 5                    4 1
        //                          5 2
        let f = from_bgra(&sample(), 3, 2, 16, 1);
        assert_eq!((f.width, f.height), (2, 3));
        assert_eq!(blues(&f), vec![3, 0, 4, 1, 5, 2]);
        assert_eq!(blues(&from_bgra(&sample(), 3, 2, 16, 2)), vec![5, 4, 3, 2, 1, 0]);
        assert_eq!(blues(&from_bgra(&sample(), 3, 2, 16, 3)), vec![2, 5, 1, 4, 0, 3]);
    }

    #[test]
    fn short_buffers_do_not_panic() {
        let f = from_bgra(&sample()[..18], 3, 2, 16, 0);
        assert_eq!(f.pixel(0, 1), [0, 0, 0, 255]);
        assert_eq!(from_bgra(&[], 0, 0, 0, 1).rgba.len(), 0);
        assert_eq!(from_bgra(&[], 0, 5, 0, 0).rgba.len(), 0);
    }

    #[test]
    fn transforms_map_to_turns() {
        assert_eq!(quarter_turns(1.0, 0.0), 0);
        assert_eq!(quarter_turns(0.0, 1.0), 1);
        assert_eq!(quarter_turns(-1.0, 0.0), 2);
        assert_eq!(quarter_turns(0.0, -1.0), 3);
        assert_eq!(quarter_turns(0.0, 0.0), 0);
    }
}
