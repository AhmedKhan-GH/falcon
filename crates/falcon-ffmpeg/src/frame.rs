//! Decoded pictures, shared by both of falcon's video back ends.

/// A decoded picture: `width * height` RGBA pixels, rows top to bottom,
/// fully opaque.
#[derive(Clone, Debug, PartialEq)]
pub struct Frame {
    pub width: usize,
    pub height: usize,
    pub rgba: Vec<u8>,
}

impl Frame {
    /// The RGBA of the pixel at `(x, y)`.
    pub fn pixel(&self, x: usize, y: usize) -> [u8; 4] {
        let i = (y * self.width + x) * 4;
        [self.rgba[i], self.rgba[i + 1], self.rgba[i + 2], self.rgba[i + 3]]
    }

    /// The average RGB over the whole picture.
    pub fn mean(&self) -> [f32; 3] {
        let n = (self.width * self.height).max(1) as f32;
        let mut sum = [0.0f32; 3];
        for px in self.rgba.as_chunks::<4>().0 {
            for c in 0..3 {
                sum[c] += px[c] as f32;
            }
        }
        sum.map(|s| s / n)
    }

    /// Whether the picture is (nearly) black all over: a fade-in or a
    /// title card's gap, not a picture worth using as a thumbnail.
    pub fn is_black(&self) -> bool {
        let [r, g, b] = self.mean();
        if r.max(g).max(b) > BLACK_MEAN {
            return false;
        }
        // A dark frame with something bright in it (stars, a logo) counts.
        let bright = self.rgba.as_chunks::<4>().0.iter().filter(|p| p[0].max(p[1]).max(p[2]) > BLACK_PIXEL).count();
        bright * 100 < self.width * self.height
    }

    /// The picture turned `turns` clockwise quarter turns.
    pub fn turned(self, turns: u8) -> Frame {
        let (w, h) = (self.width, self.height);
        if turns.is_multiple_of(4) || w == 0 || h == 0 {
            return self;
        }
        let (ow, oh) = if turns % 2 == 1 { (h, w) } else { (w, h) };
        let mut rgba = vec![0u8; ow * oh * 4];
        for oy in 0..oh {
            for ox in 0..ow {
                let (sx, sy) = match turns % 4 {
                    1 => (oy, h - 1 - ox),
                    2 => (w - 1 - ox, h - 1 - oy),
                    _ => (w - 1 - oy, ox),
                };
                let (s, d) = ((sy * w + sx) * 4, (oy * ow + ox) * 4);
                rgba[d..d + 4].copy_from_slice(&self.rgba[s..s + 4]);
            }
        }
        Frame { width: ow, height: oh, rgba }
    }
}

/// Mean channel level (0 to 255) at or under which a picture may be black.
const BLACK_MEAN: f32 = 12.0;
/// A pixel brighter than this is not black.
const BLACK_PIXEL: u8 = 40;

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(w: usize, h: usize, v: u8) -> Frame {
        Frame { width: w, height: h, rgba: [v, v, v, 255].repeat(w * h) }
    }

    #[test]
    fn black_frames_are_noticed() {
        assert!(solid(8, 8, 0).is_black());
        assert!(solid(8, 8, 10).is_black());
        assert!(!solid(8, 8, 60).is_black());
        // A few bright pixels in the dark are a picture.
        let mut stars = solid(10, 10, 0);
        for i in [0, 9, 44, 77, 98] {
            stars.rgba[i * 4..i * 4 + 3].copy_from_slice(&[255, 255, 255]);
        }
        assert!(!stars.is_black());
    }

    #[test]
    fn turns_rotate_clockwise() {
        // 0 1 2   once:  3 0
        // 3 4 5          4 1
        //                5 2
        let f = Frame { width: 3, height: 2, rgba: (0..6u8).flat_map(|n| [n, 0, 0, 255]).collect() };
        let reds = |f: &Frame| f.rgba.as_chunks::<4>().0.iter().map(|p| p[0]).collect::<Vec<_>>();
        let once = f.clone().turned(1);
        assert_eq!((once.width, once.height), (2, 3));
        assert_eq!(reds(&once), vec![3, 0, 4, 1, 5, 2]);
        assert_eq!(reds(&f.clone().turned(2)), vec![5, 4, 3, 2, 1, 0]);
        assert_eq!(reds(&f.clone().turned(3)), vec![2, 5, 1, 4, 0, 3]);
        assert_eq!(f.clone().turned(4), f);
    }
}
