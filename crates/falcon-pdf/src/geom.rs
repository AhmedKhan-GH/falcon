//! Converting between PDFKit's page space (points, origin at the
//! bottom-left of the unrotated page box) and display space (points,
//! origin at the top-left of the page as shown, after its rotation).

use crate::Rect;

/// A page's crop box and rotation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PageGeom {
    pub x0: f64,
    pub y0: f64,
    pub w: f64,
    pub h: f64,
    /// Clockwise quarter turns, 0 to 3.
    pub turns: u8,
}

/// Clockwise quarter turns for a PDF `/Rotate` value in degrees (any
/// multiple of 90, negative included); other values round down.
pub fn turns(degrees: i64) -> u8 {
    (degrees.div_euclid(90).rem_euclid(4)) as u8
}

impl PageGeom {
    /// The size as shown.
    pub fn size(self) -> (f64, f64) {
        if self.turns % 2 == 1 { (self.h, self.w) } else { (self.w, self.h) }
    }

    pub fn to_display(self, px: f64, py: f64) -> (f64, f64) {
        // Unrotated, top-left origin.
        let (ux, uy) = (px - self.x0, self.y0 + self.h - py);
        match self.turns {
            1 => (self.h - uy, ux),
            2 => (self.w - ux, self.h - uy),
            3 => (uy, self.w - ux),
            _ => (ux, uy),
        }
    }

    pub fn to_page(self, dx: f64, dy: f64) -> (f64, f64) {
        let (ux, uy) = match self.turns {
            1 => (dy, self.h - dx),
            2 => (self.w - dx, self.h - dy),
            3 => (self.w - dy, dx),
            _ => (dx, dy),
        };
        (ux + self.x0, self.y0 + self.h - uy)
    }

    /// A page-space rectangle (origin, size) as a display rectangle.
    pub fn rect_to_display(self, x: f64, y: f64, w: f64, h: f64) -> Rect {
        let (ax, ay) = self.to_display(x, y);
        let (bx, by) = self.to_display(x + w, y + h);
        Rect { x: ax.min(bx), y: ay.min(by), w: (ax - bx).abs(), h: (ay - by).abs() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn g(turns: u8) -> PageGeom {
        PageGeom { x0: 10.0, y0: 20.0, w: 100.0, h: 200.0, turns }
    }

    #[test]
    fn turns_normalise() {
        assert_eq!(turns(0), 0);
        assert_eq!(turns(90), 1);
        assert_eq!(turns(-90), 3);
        assert_eq!(turns(450), 1);
        assert_eq!(turns(180), 2);
    }

    #[test]
    fn corners_land_where_the_rotation_puts_them() {
        // The page's top-left corner in page space is (x0, y0 + h).
        let top_left = (10.0, 220.0);
        assert_eq!(g(0).to_display(top_left.0, top_left.1), (0.0, 0.0));
        // Turned clockwise, the top-left goes to the top-right.
        assert_eq!(g(1).to_display(top_left.0, top_left.1), (200.0, 0.0));
        assert_eq!(g(2).to_display(top_left.0, top_left.1), (100.0, 200.0));
        assert_eq!(g(3).to_display(top_left.0, top_left.1), (0.0, 100.0));
        assert_eq!(g(1).size(), (200.0, 100.0));
        assert_eq!(g(2).size(), (100.0, 200.0));
    }

    #[test]
    fn round_trips() {
        for t in 0..4 {
            let geom = g(t);
            for &(x, y) in &[(10.0, 20.0), (55.5, 120.25), (110.0, 220.0)] {
                let (dx, dy) = geom.to_display(x, y);
                let (px, py) = geom.to_page(dx, dy);
                assert!((px - x).abs() < 1e-9 && (py - y).abs() < 1e-9, "turns {t}: {x},{y} -> {px},{py}");
            }
        }
    }

    #[test]
    fn rects_flip_to_top_left_origin() {
        let r = g(0).rect_to_display(20.0, 200.0, 30.0, 10.0);
        assert_eq!(r, Rect { x: 10.0, y: 10.0, w: 30.0, h: 10.0 });
        let r = g(1).rect_to_display(20.0, 200.0, 30.0, 10.0);
        assert_eq!(r, Rect { x: 180.0, y: 10.0, w: 10.0, h: 30.0 });
    }
}
