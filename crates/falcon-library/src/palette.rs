//! Dominant-colour palettes, matching Eagle's `palettes` field.
//!
//! Eagle runs modified median cut quantization (MMCQ, the algorithm behind
//! color-thief) on a copy of the image at most 360 px wide, then turns box
//! populations into percentages. This port keeps Eagle's exact control flow,
//! quirks included, so the same pixels give the same palette:
//!
//! - the colour bounds are tracked with `if min .. else if max`, so a channel's
//!   first value only ever sets its minimum;
//! - each of the two splitting passes stops after 5 iterations, which caps a
//!   palette at about 11 colours;
//! - boxes are kept in a lazily sorted queue whose tie order matters.

use image::{DynamicImage, RgbaImage};
use serde_json::Number;

use crate::model::Palette;

const SIGBITS: u32 = 5;
const RSHIFT: u32 = 8 - SIGBITS;
const BINS: usize = 1 << SIGBITS;
const MAX_ITERATIONS: u32 = 5;
const FRACT_BY_POPULATIONS: f64 = 0.75;
const MAX_COLORS: usize = 12;
const ALPHA_THRESHOLD: u8 = 170;
/// Width of the image Eagle analyses.
pub const ANALYSIS_WIDTH: u32 = 360;

/// Extracts the palette Eagle would store for `img`.
///
/// `img` is the image Eagle analyses: the item's thumbnail, or the original
/// when the item has no thumbnail. It is shrunk to [`ANALYSIS_WIDTH`] first.
pub fn extract(img: &DynamicImage) -> Vec<Palette> {
    from_rgba(analysis_image(img).as_raw())
}

/// Extracts a palette from an RGBA buffer that is already analysis-sized.
pub fn from_rgba(rgba: &[u8]) -> Vec<Palette> {
    let Some(swatches) = quantize(rgba, skip_for(rgba.len()), ALPHA_THRESHOLD, MAX_COLORS) else {
        return Vec::new();
    };
    to_palettes(swatches)
}

/// The image Eagle hands to the quantizer: `img` scaled down (never up) to at
/// most [`ANALYSIS_WIDTH`] wide, keeping the aspect ratio.
///
/// Eagle draws onto a canvas with default smoothing, which Chromium does as a
/// single bilinear sample per pixel; this mimics that rather than using an
/// area filter.
pub fn analysis_image(img: &DynamicImage) -> RgbaImage {
    let src = img.to_rgba8();
    let (w, h) = src.dimensions();
    if w <= ANALYSIS_WIDTH {
        return src;
    }
    let dw = ANALYSIS_WIDTH;
    let dh = ((h as f64 / (w as f64 / dw as f64)).round() as u32).max(1);
    bilinear(&src, dw, dh)
}

/// Every Nth pixel is sampled, depending on the buffer's size in bytes.
fn skip_for(bytes: usize) -> usize {
    match bytes {
        0..100_000 => 1,
        100_000..1_000_000 => 10,
        1_000_000..10_000_000 => 100,
        10_000_000..100_000_000 => 1000,
        _ => 10000,
    }
}

/// One MMCQ colour and the number of sampled pixels it stands for, with the
/// total number of sampled pixels.
struct Swatches {
    colors: Vec<([u8; 3], u64)>,
    pixel_count: u64,
}

/// Turns box populations into Eagle's rounded, filtered percentages.
fn to_palettes(s: Swatches) -> Vec<Palette> {
    let mut colors: Vec<([u8; 3], f64)> = s
        .colors
        .into_iter()
        .map(|(color, count)| {
            let ratio = count as f64 / s.pixel_count as f64 * 100.0;
            let ratio = (ratio * 100.0).round() / 100.0;
            // Large shares are truncated to whole percents.
            let ratio = if ratio >= 5.0 { ratio.trunc() } else { ratio };
            (color, ratio)
        })
        .filter(|&(_, ratio)| ratio >= 0.25)
        .collect();
    // Stable, like Array.prototype.sort, so equal ratios keep box order.
    colors.sort_by(|a, b| b.1.total_cmp(&a.1));

    if colors.len() > 4 {
        let mut kept = Vec::new();
        for (i, c) in colors.into_iter().enumerate() {
            let threshold = if i < 6 { 0.1 } else { 0.3 };
            if kept.len() < 5 || c.1 > threshold {
                kept.push(c);
            }
        }
        colors = kept;
    }
    colors.truncate(MAX_COLORS);

    colors
        .into_iter()
        .map(|(color, ratio)| Palette { color, ratio: js_number(ratio), extra: Default::default() })
        .collect()
}

/// A JSON number written the way JavaScript prints it: whole values have no
/// fractional part.
fn js_number(x: f64) -> Number {
    if x.fract() == 0.0 {
        Number::from(x as u64)
    } else {
        Number::from_f64(x).expect("ratios are finite")
    }
}

/// Population histogram over 5-bit RGB.
struct Histogram(Vec<u64>);

impl Histogram {
    fn at(&self, r: i32, g: i32, b: i32) -> u64 {
        self.0[((r << (2 * SIGBITS)) + (g << SIGBITS) + b) as usize]
    }
}

/// An axis of colour space.
#[derive(Clone, Copy, PartialEq)]
enum Axis {
    R,
    G,
    B,
}

/// A box in 5-bit RGB space. Bounds are inclusive and can be inverted (empty)
/// because of the bounds quirk in [`quantize`].
#[derive(Clone)]
struct VBox {
    lo: [i32; 3],
    hi: [i32; 3],
    count: u64,
}

impl VBox {
    fn new(lo: [i32; 3], hi: [i32; 3], histo: &Histogram) -> VBox {
        let mut count = 0;
        for r in lo[0]..=hi[0] {
            for g in lo[1]..=hi[1] {
                for b in lo[2]..=hi[2] {
                    count += histo.at(r, g, b);
                }
            }
        }
        VBox { lo, hi, count }
    }

    fn width(&self, axis: Axis) -> i32 {
        let i = axis as usize;
        self.hi[i] - self.lo[i] + 1
    }

    fn volume(&self) -> i64 {
        [Axis::R, Axis::G, Axis::B].iter().map(|&a| self.width(a) as i64).product()
    }

    /// Population-weighted mean colour of the bins' centres, truncated.
    fn avg(&self, histo: &Histogram) -> [u8; 3] {
        let mult = (1 << RSHIFT) as f64;
        let (mut total, mut sum) = (0u64, [0f64; 3]);
        for r in self.lo[0]..=self.hi[0] {
            for g in self.lo[1]..=self.hi[1] {
                for b in self.lo[2]..=self.hi[2] {
                    let n = histo.at(r, g, b);
                    total += n;
                    for (s, v) in sum.iter_mut().zip([r, g, b]) {
                        *s += n as f64 * (v as f64 + 0.5) * mult;
                    }
                }
            }
        }
        if total > 0 {
            sum.map(|s| (s / total as f64) as u8)
        } else {
            std::array::from_fn(|i| (mult * (self.lo[i] + self.hi[i] + 1) as f64 / 2.0) as u8)
        }
    }
}

/// The JS queue Eagle uses: pushes append, and the contents are stably sorted
/// in ascending order only when popping after a push. `pop` takes the last
/// (largest) box.
struct LazyQueue {
    boxes: Vec<VBox>,
    sorted: bool,
    key: fn(&VBox) -> i64,
}

impl LazyQueue {
    fn new(key: fn(&VBox) -> i64) -> LazyQueue {
        LazyQueue { boxes: Vec::new(), sorted: false, key }
    }

    fn push(&mut self, b: VBox) {
        self.boxes.push(b);
        self.sorted = false;
    }

    fn pop(&mut self) -> Option<VBox> {
        if !self.sorted {
            let key = self.key;
            self.boxes.sort_by_key(key);
            self.sorted = true;
        }
        self.boxes.pop()
    }

    fn len(&self) -> usize {
        self.boxes.len()
    }
}

fn by_count(b: &VBox) -> i64 {
    b.count as i64
}

fn by_count_times_volume(b: &VBox) -> i64 {
    b.count as i64 * b.volume()
}

/// Quantizes every `skip`th pixel of `rgba` with alpha at least
/// `alpha_threshold` into at most `max_colors` boxes.
fn quantize(rgba: &[u8], skip: usize, alpha_threshold: u8, max_colors: usize) -> Option<Swatches> {
    let mut histo = Histogram(vec![0; BINS * BINS * BINS]);
    let (mut lo, mut hi) = ([31i32; 3], [0i32; 3]);
    let mut pixel_count = 0u64;
    for px in rgba.chunks_exact(4).step_by(skip) {
        if px[3] < alpha_threshold {
            continue;
        }
        let v = [px[0], px[1], px[2]].map(|c| (c >> RSHIFT) as i32);
        histo.0[((v[0] << (2 * SIGBITS)) + (v[1] << SIGBITS) + v[2]) as usize] += 1;
        pixel_count += 1;
        for i in 0..3 {
            // Eagle's quirk: a value that lowers the minimum never raises the
            // maximum.
            if v[i] < lo[i] {
                lo[i] = v[i];
            } else if v[i] > hi[i] {
                hi[i] = v[i];
            }
        }
    }
    if pixel_count == 0 {
        return None;
    }

    let mut pq = LazyQueue::new(by_count);
    pq.push(VBox::new(lo, hi, &histo));
    split(&mut pq, FRACT_BY_POPULATIONS * max_colors as f64, &histo);

    let mut pq2 = LazyQueue::new(by_count_times_volume);
    while let Some(b) = pq.pop() {
        pq2.push(b);
    }
    let target = max_colors as f64 - pq2.len() as f64;
    split(&mut pq2, target, &histo);

    let mut colors = Vec::new();
    while let Some(b) = pq2.pop() {
        colors.push((b.avg(&histo), b.count));
    }
    Some(Swatches { colors, pixel_count })
}

/// Splits the largest box repeatedly until `target` boxes have been added or
/// the iteration budget runs out.
fn split(queue: &mut LazyQueue, target: f64, histo: &Histogram) {
    let mut ncolors = 1.0;
    let mut iterations = 0;
    while iterations < MAX_ITERATIONS {
        let Some(vbox) = queue.pop() else { return };
        if vbox.count == 0 {
            queue.push(vbox);
            iterations += 1;
            continue;
        }
        let Some((first, second)) = median_cut(histo, &vbox) else {
            return;
        };
        queue.push(first);
        if let Some(second) = second {
            queue.push(second);
            ncolors += 1.0;
        }
        if ncolors >= target {
            return;
        }
        iterations += 1;
    }
}

/// Cuts `vbox` across its longest side near the population median.
fn median_cut(histo: &Histogram, vbox: &VBox) -> Option<(VBox, Option<VBox>)> {
    if vbox.count == 0 {
        return None;
    }
    if vbox.count == 1 {
        return Some((vbox.clone(), None));
    }
    let (rw, gw, bw) = (vbox.width(Axis::R), vbox.width(Axis::G), vbox.width(Axis::B));
    let maxw = rw.max(gw).max(bw);
    let axis = if maxw == rw {
        Axis::R
    } else if maxw == gw {
        Axis::G
    } else {
        Axis::B
    };
    let a = axis as usize;
    let (start, end) = (vbox.lo[a], vbox.hi[a]);

    // Running population along the axis. Out-of-range slots read as 0,
    // standing in for the `undefined` holes of the JS array.
    let mut partial = vec![0u64; BINS];
    let mut total = 0;
    for i in start..=end {
        let mut slab = vbox.clone();
        slab.lo[a] = i;
        slab.hi[a] = i;
        total += VBox::new(slab.lo, slab.hi, histo).count;
        partial[i as usize] = total;
    }
    let ps = |i: i32| if (start..=end).contains(&i) { partial[i as usize] } else { 0 };
    let lookahead = |i: i32| total - ps(i);

    let i = (start..=end).find(|&i| 2 * ps(i) > total)?;
    let (left, right) = (i - start, end - i);
    let mut d2 = if left <= right {
        (end - 1).min((i as f64 + right as f64 / 2.0) as i32)
    } else {
        start.max((i as f64 - 1.0 - left as f64 / 2.0) as i32)
    };
    while ps(d2) == 0 {
        d2 += 1;
    }
    let mut count2 = lookahead(d2);
    while count2 == 0 && ps(d2 - 1) != 0 {
        d2 -= 1;
        count2 = lookahead(d2);
    }

    let mut hi1 = vbox.hi;
    let mut lo2 = vbox.lo;
    hi1[a] = d2;
    lo2[a] = d2 + 1;
    Some((VBox::new(vbox.lo, hi1, histo), Some(VBox::new(lo2, vbox.hi, histo))))
}

/// Resamples `src` to `dw` x `dh` with one bilinear sample per output pixel,
/// sampling at pixel centres and clamping at the edges.
fn bilinear(src: &RgbaImage, dw: u32, dh: u32) -> RgbaImage {
    let (sw, sh) = src.dimensions();
    let (sx, sy) = (sw as f64 / dw as f64, sh as f64 / dh as f64);
    RgbaImage::from_fn(dw, dh, |x, y| {
        let fx = ((x as f64 + 0.5) * sx - 0.5).clamp(0.0, (sw - 1) as f64);
        let fy = ((y as f64 + 0.5) * sy - 0.5).clamp(0.0, (sh - 1) as f64);
        let (x0, y0) = (fx.floor() as u32, fy.floor() as u32);
        let (x1, y1) = ((x0 + 1).min(sw - 1), (y0 + 1).min(sh - 1));
        let (tx, ty) = (fx - x0 as f64, fy - y0 as f64);
        let p = |x, y| src.get_pixel(x, y).0.map(f64::from);
        let (a, b, c, d) = (p(x0, y0), p(x1, y0), p(x0, y1), p(x1, y1));
        image::Rgba(std::array::from_fn(|i| {
            let top = a[i] + (b[i] - a[i]) * tx;
            let bottom = c[i] + (d[i] - c[i]) * tx;
            (top + (bottom - top) * ty).round() as u8
        }))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(rgba: [u8; 4], n: usize) -> Vec<u8> {
        rgba.repeat(n)
    }

    #[test]
    fn ratios_follow_eagle_rounding() {
        let s = Swatches { colors: vec![([1, 2, 3], 4517), ([4, 5, 6], 222), ([7, 8, 9], 20)], pixel_count: 10000 };
        let p = to_palettes(s);
        let ratios: Vec<String> = p.iter().map(|c| c.ratio.to_string()).collect();
        // 45.17 truncates to 45, 2.22 keeps its decimals, 0.2 is dropped.
        assert_eq!(ratios, ["45", "2.22"]);
    }

    #[test]
    fn whole_small_ratios_print_without_decimals() {
        assert_eq!(js_number(3.0).to_string(), "3");
        assert_eq!(js_number(0.74).to_string(), "0.74");
    }

    #[test]
    fn transparent_pixels_are_ignored() {
        let mut buf = solid([255, 0, 0, 255], 100);
        buf.extend(solid([0, 0, 255, 0], 100));
        let p = from_rgba(&buf);
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].ratio.to_string(), "100");
    }

    #[test]
    fn single_pixel_hits_the_bounds_quirk() {
        // The only value lowers each minimum and never sets a maximum, so the
        // box is empty and Eagle stores no palette.
        assert!(from_rgba(&solid([40, 40, 40, 255], 1)).is_empty());
        // A second pixel of the same colour raises the maximum.
        let p = from_rgba(&solid([40, 40, 40, 255], 2));
        assert_eq!(p.len(), 1);
        assert_eq!((p[0].color, p[0].ratio.to_string()), ([44, 44, 44], "100".to_string()));
    }

    #[test]
    fn two_colours_split_evenly() {
        let mut buf = solid([250, 10, 10, 255], 60);
        buf.extend(solid([10, 10, 250, 255], 40));
        let p = from_rgba(&buf);
        let got: Vec<_> = p.iter().map(|c| (c.color, c.ratio.to_string())).collect();
        assert_eq!(got, [([252, 12, 12], "60".to_string()), ([12, 12, 252], "40".to_string())]);
    }

    #[test]
    fn skip_tracks_buffer_size() {
        assert_eq!(skip_for(360 * 270 * 4), 10);
        assert_eq!(skip_for(99_999), 1);
        assert_eq!(skip_for(100_000_000), 10000);
    }
}
