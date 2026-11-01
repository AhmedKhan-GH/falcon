//! The 3D model viewer's camera and scene maths, after Eagle's embedded
//! model viewer (observed behaviour, written fresh):
//!
//! - The camera is an eye looking at a centre, with an up direction and a
//!   45° vertical field of view. A model opens with Y up, seen from above,
//!   front and left (the eye along (−1.5, 2, 3) from the centre), and is
//!   fitted to the view.
//! - Left drag orbits at half a degree per point. With the up vector fixed
//!   (the default) the eye turns about the up axis and tilts about the
//!   camera's right axis, never past straight up or down; in free orbit it
//!   turns about the camera's own axes and the up vector follows.
//! - Right or middle drag, or Shift with the left button, pans by 0.001 of
//!   the eye–centre distance per point; Control with the left button
//!   zooms by 0.005 of the distance per point dragged up.
//! - Each wheel step moves the eye a tenth of the way to the centre (or
//!   back).
//! - Fitting keeps the viewing direction and puts the eye where the
//!   bounding sphere just fills the shorter side of the view.
//! - Lighting: see `falcon_library::model3d::geom::shade`; the
//!   directional light comes from the eye.
//!
//! Matrices are column-major with clip depth 0..1 (`model3d::math`).

use falcon_library::model3d::math::{self, M4, V3};

/// The vertical field of view, in degrees.
pub const FOV_DEG: f32 = 45.0;
/// Degrees of orbit per point dragged.
pub const ORBIT_DEG_PER_POINT: f32 = 0.5;
/// Pan per point dragged, as a share of the eye–centre distance.
pub const PAN_PER_POINT: f32 = 0.001;
/// Zoom per point dragged with Control, as a share of the distance.
pub const ZOOM_PER_POINT: f32 = 0.005;
/// One wheel step's zoom, as a share of the distance.
pub const WHEEL_STEP: f32 = 0.1;
/// Points of wheel movement counted as one step.
pub const WHEEL_POINTS: f32 = 40.0;

/// Which world axis points up.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum UpAxis {
    #[default]
    Y,
    Z,
}

impl UpAxis {
    pub fn vector(self) -> V3 {
        match self {
            UpAxis::Y => [0.0, 1.0, 0.0],
            UpAxis::Z => [0.0, 0.0, 1.0],
        }
    }

    /// The direction from the centre to the eye a model opens with.
    pub fn home_direction(self) -> V3 {
        match self {
            UpAxis::Y => [-1.5, 2.0, 3.0],
            UpAxis::Z => [-1.5, -3.0, 2.0],
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Projection {
    #[default]
    Perspective,
    Orthographic,
}

/// What a drag does, from the button held and the modifiers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DragAction {
    Orbit,
    Pan,
    Zoom,
}

/// Eagle's mouse mapping: the left button orbits, or zooms with Control,
/// or pans with Shift; the right and middle buttons pan.
pub fn drag_action(primary: bool, secondary_or_middle: bool, ctrl: bool, shift: bool) -> Option<DragAction> {
    if primary {
        Some(if ctrl {
            DragAction::Zoom
        } else if shift {
            DragAction::Pan
        } else {
            DragAction::Orbit
        })
    } else if secondary_or_middle {
        Some(DragAction::Pan)
    } else {
        None
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    pub eye: V3,
    pub center: V3,
    pub up: V3,
}

impl Camera {
    /// The camera a model opens with, before fitting: the eye along the
    /// home direction at unit scale, looking at the origin.
    pub fn home(up: UpAxis) -> Camera {
        Camera { eye: up.home_direction(), center: [0.0; 3], up: up.vector() }
    }

    pub fn distance(&self) -> f32 {
        math::length(math::sub(self.eye, self.center))
    }

    /// Unit vectors: forward (eye to centre), right, and up on screen.
    fn basis(&self) -> (V3, V3, V3) {
        let f = math::normalize(math::sub(self.center, self.eye));
        let r = math::normalize(math::cross(f, self.up));
        let u = math::cross(r, f);
        (f, r, u)
    }

    /// Turns the eye about the centre for a drag of (`dx`, `dy`) points.
    pub fn orbit(&mut self, dx: f32, dy: f32, fixed_up: bool) {
        let h = (dx * ORBIT_DEG_PER_POINT).to_radians();
        let v = (dy * ORBIT_DEG_PER_POINT).to_radians();
        let (f, r, u) = self.basis();
        if fixed_up {
            // Tilt only while the view stays strictly between straight
            // down and straight up the up axis.
            let up = math::normalize(self.up);
            let angle = math::dot(f, up).clamp(-1.0, 1.0).acos() + v;
            if angle > 1e-3 && angle < std::f32::consts::PI - 1e-3 {
                self.eye = math::rotate_about(self.eye, self.center, r, -v);
            }
            self.eye = math::rotate_about(self.eye, self.center, up, -h);
        } else {
            // Tilt about the right axis, carrying the screen's up along,
            // then turn about that new up.
            self.eye = math::rotate_about(self.eye, self.center, r, -v);
            let u = math::rotate_about(u, [0.0; 3], r, -v);
            self.eye = math::rotate_about(self.eye, self.center, u, -h);
            self.up = u;
        }
    }

    /// Slides eye and centre for a drag of (`dx`, `dy`) points: the model
    /// follows the pointer.
    pub fn pan(&mut self, dx: f32, dy: f32) {
        let k = PAN_PER_POINT * self.distance();
        let (_, r, u) = self.basis();
        let shift = math::add(math::scale(r, -dx * k), math::scale(u, dy * k));
        self.eye = math::add(self.eye, shift);
        self.center = math::add(self.center, shift);
    }

    /// Moves the eye `ratio` of the way to the centre (back out when
    /// negative). Kept below 0.9 so the eye never reaches the centre.
    pub fn zoom(&mut self, ratio: f32) {
        let ratio = ratio.clamp(-0.9, 0.9);
        let d = math::sub(self.center, self.eye);
        self.eye = math::add(self.eye, math::scale(d, ratio));
    }

    /// Fits the sphere (`center`, `radius`) in a view `aspect` wide per
    /// unit high, keeping the viewing direction.
    pub fn fit(&mut self, center: V3, radius: f32, aspect: f32) {
        if radius.is_nan() || radius <= 0.0 {
            self.center = center;
            return;
        }
        let back = math::normalize(math::sub(self.eye, self.center));
        self.center = center;
        self.eye = math::add(center, math::scale(back, fit_distance(radius, aspect)));
    }

    pub fn view(&self) -> M4 {
        math::look_at(self.eye, self.center, self.up)
    }

    /// The projection for a view `aspect` wide per unit high, with clip
    /// planes around a model of bounding radius `radius`.
    pub fn projection(&self, mode: Projection, aspect: f32, radius: f32) -> M4 {
        let (near, far) = clip_planes(self.distance(), radius);
        match mode {
            Projection::Perspective => math::perspective(FOV_DEG.to_radians(), aspect, near, far),
            // The orthographic view shows what the perspective one shows
            // at the centre's depth, so switching keeps the size.
            Projection::Orthographic => math::orthographic(self.distance() * (FOV_DEG.to_radians() / 2.0).tan(), aspect, near, far),
        }
    }

    /// The ray through the point (`x`, `y`) of the view in normalised
    /// device coordinates (−1..1, y up), for picking: (origin, unit
    /// direction). Perspective rays start at the eye; orthographic ones are
    /// parallel, starting on the eye's plane.
    pub fn ray(&self, mode: Projection, aspect: f32, x: f32, y: f32) -> (V3, V3) {
        let (f, r, u) = self.basis();
        let t = (FOV_DEG.to_radians() / 2.0).tan();
        match mode {
            Projection::Perspective => {
                let d = math::add(f, math::add(math::scale(r, x * t * aspect), math::scale(u, y * t)));
                (self.eye, math::normalize(d))
            }
            Projection::Orthographic => {
                let h = self.distance() * t;
                let o = math::add(self.eye, math::add(math::scale(r, x * h * aspect), math::scale(u, y * h)));
                (o, f)
            }
        }
    }

    /// The direction from the scene towards the eye, for the light.
    pub fn light_direction(&self) -> V3 {
        math::normalize(math::sub(self.eye, self.center))
    }

    /// The camera with up turned upside down (Eagle's "Flip up vector").
    pub fn flipped(&self) -> Camera {
        Camera { up: math::scale(self.up, -1.0), ..*self }
    }
}

/// Where the point `p` lands in `rect` (`[x0, y0, x1, y1]`, y down) through
/// the camera matrix `vp`; `None` behind the eye.
pub fn screen_point(vp: &M4, p: V3, rect: [f32; 4]) -> Option<[f32; 2]> {
    let v = [p[0], p[1], p[2], 1.0];
    let c: [f32; 4] = std::array::from_fn(|r| (0..4).map(|k| vp[k][r] * v[k]).sum());
    if c[3] <= 1e-9 {
        return None;
    }
    let (x, y) = (c[0] / c[3], c[1] / c[3]);
    Some([rect[0] + (x * 0.5 + 0.5) * (rect[2] - rect[0]), rect[1] + (0.5 - y * 0.5) * (rect[3] - rect[1])])
}

/// How far from the centre the eye sits for a sphere of `radius` to fill a
/// view `aspect` wide per unit high: the half angle is half the vertical
/// field of view, narrowed by the aspect when the view is taller than wide.
pub fn fit_distance(radius: f32, aspect: f32) -> f32 {
    let mut half = FOV_DEG / 2.0;
    if aspect < 1.0 {
        half *= aspect;
    }
    radius / half.to_radians().sin()
}

/// Near and far clip distances for an eye `distance` from a model's
/// centre, `radius` its bounding radius: they hug the model (and the grid
/// around it, out to ten radii) so depth stays precise however far one
/// zooms, and the near plane stays a little in front of the eye when it is
/// inside the model.
pub fn clip_planes(distance: f32, radius: f32) -> (f32, f32) {
    let r = if radius > 0.0 { radius } else { 1.0 };
    let reach = 10.0 * r;
    let near = (distance - reach).max(r * 1e-3);
    let far = (distance + reach).max(near * 2.0);
    (near, far)
}

/// Wheel movement of `points` (positive away from the user) as a zoom
/// ratio: a tenth of the distance per 40 points, at most a tenth per frame.
pub fn wheel_zoom(points: f32) -> f32 {
    (points / WHEEL_POINTS * WHEEL_STEP).clamp(-WHEEL_STEP, WHEEL_STEP)
}

/// A trackpad pinch by `factor` (above 1 spreads the fingers) as a zoom
/// ratio, so the model grows by that factor.
pub fn pinch_zoom(factor: f32) -> f32 {
    if factor > 0.0 { 1.0 - 1.0 / factor } else { 0.0 }
}

/// A round grid step for a model of bounding radius `radius`: a power of
/// ten times 1, 2 or 5, about a fifth of the radius.
pub fn grid_step(radius: f32) -> f32 {
    let target = if radius > 0.0 { radius / 5.0 } else { 0.2 };
    let p = 10f32.powf(target.log10().floor());
    [1.0, 2.0, 5.0, 10.0].into_iter().map(|m| m * p).find(|s| *s >= target * 0.75).unwrap_or(10.0 * p)
}

/// The grid's lines on the floor under the model: the plane across the up
/// axis through the bottom of `bounds`, centred under the model, with
/// lines `grid_step` apart reaching about three radii out.
pub fn grid_lines(bounds: (V3, V3), up: UpAxis) -> Vec<[V3; 2]> {
    let (lo, hi) = bounds;
    let center = math::scale(math::add(lo, hi), 0.5);
    let radius = math::length(math::sub(hi, center));
    let step = grid_step(radius);
    let n = ((3.0 * radius.max(1e-6)) / step).ceil().clamp(1.0, 200.0) as i32;
    let (a, b, u) = match up {
        UpAxis::Y => (0, 2, 1),
        UpAxis::Z => (0, 1, 2),
    };
    // Snap the grid's middle to the step so lines land on round values.
    let ca = (center[a] / step).round() * step;
    let cb = (center[b] / step).round() * step;
    let half = n as f32 * step;
    let point = |x: f32, y: f32| {
        let mut p = [0.0; 3];
        p[a] = x;
        p[b] = y;
        p[u] = lo[u];
        p
    };
    let mut lines = Vec::with_capacity(4 * n as usize + 2);
    for i in -n..=n {
        let o = i as f32 * step;
        lines.push([point(ca + o, cb - half), point(ca + o, cb + half)]);
        lines.push([point(ca - half, cb + o), point(ca + half, cb + o)]);
    }
    lines
}

/// The axes: X, Y and Z from the origin, each as long as the model's
/// bounding radius (at least a grid step), in red, green and blue.
pub fn axes(radius: f32) -> [([V3; 2], [f32; 3]); 3] {
    let l = radius.max(grid_step(radius));
    [
        ([[0.0; 3], [l, 0.0, 0.0]], [0.90, 0.25, 0.25]),
        ([[0.0; 3], [0.0, l, 0.0]], [0.30, 0.80, 0.30]),
        ([[0.0; 3], [0.0, 0.0, l]], [0.30, 0.50, 0.95]),
    ]
}

/// A number from the model's details as Eagle lists it: up to two decimals,
/// without trailing zeros.
pub fn detail_number(v: f64) -> String {
    if !v.is_finite() {
        return "-".into();
    }
    let s = format!("{v:.2}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".into() } else { s.to_string() }
}

#[cfg(test)]
mod viewer3d_tests {
    use super::*;

    fn close(a: V3, b: V3, eps: f32) -> bool {
        (0..3).all(|i| (a[i] - b[i]).abs() < eps)
    }

    #[test]
    fn mouse_mapping_follows_eagle() {
        assert_eq!(drag_action(true, false, false, false), Some(DragAction::Orbit));
        assert_eq!(drag_action(true, false, true, false), Some(DragAction::Zoom));
        assert_eq!(drag_action(true, false, false, true), Some(DragAction::Pan));
        assert_eq!(drag_action(false, true, false, false), Some(DragAction::Pan));
        assert_eq!(drag_action(false, false, false, false), None);
    }

    #[test]
    fn fixed_orbit_turns_about_up_and_keeps_distance() {
        let mut c = Camera { eye: [0.0, 0.0, 5.0], center: [0.0; 3], up: [0.0, 1.0, 0.0] };
        // 180 points left-right is 90 degrees about Y.
        c.orbit(180.0, 0.0, true);
        assert!(close(c.eye, [-5.0, 0.0, 0.0], 1e-4) || close(c.eye, [5.0, 0.0, 0.0], 1e-4), "{:?}", c.eye);
        assert!((c.distance() - 5.0).abs() < 1e-4);
        assert_eq!(c.up, [0.0, 1.0, 0.0]);
        // Dragging down far enough stops short of the pole.
        let mut c = Camera { eye: [0.0, 0.0, 5.0], center: [0.0; 3], up: [0.0, 1.0, 0.0] };
        for _ in 0..400 {
            c.orbit(0.0, 1.0, true);
        }
        let f = math::normalize(math::sub(c.center, c.eye));
        let cos = math::dot(f, [0.0, 1.0, 0.0]);
        assert!(cos > -1.0 + 1e-7 && cos < -0.99, "close to straight down, never on it: {cos}");
        assert!((c.distance() - 5.0).abs() < 1e-3);
    }

    #[test]
    fn orbit_direction_matches_the_drag() {
        // Dragging right brings the model's left side into view: the eye
        // moves to the left (negative X when looking down −Z).
        let mut c = Camera { eye: [0.0, 0.0, 5.0], center: [0.0; 3], up: [0.0, 1.0, 0.0] };
        c.orbit(10.0, 0.0, true);
        assert!(c.eye[0] < 0.0);
        // Dragging down lifts the eye.
        let mut c = Camera { eye: [0.0, 0.0, 5.0], center: [0.0; 3], up: [0.0, 1.0, 0.0] };
        c.orbit(0.0, 10.0, true);
        assert!(c.eye[1] > 0.0);
    }

    #[test]
    fn free_orbit_carries_the_up_vector() {
        let mut c = Camera { eye: [0.0, 0.0, 5.0], center: [0.0; 3], up: [0.0, 1.0, 0.0] };
        c.orbit(0.0, 180.0, false);
        // 90 degrees of tilt: looking straight down, up now points along −Z.
        assert!(close(c.eye, [0.0, 5.0, 0.0], 1e-4), "{:?}", c.eye);
        assert!(close(c.up, [0.0, 0.0, -1.0], 1e-4), "{:?}", c.up);
    }

    #[test]
    fn pan_moves_eye_and_centre_together() {
        let mut c = Camera { eye: [0.0, 0.0, 10.0], center: [0.0; 3], up: [0.0, 1.0, 0.0] };
        c.pan(100.0, 0.0);
        // 100 points at 0.001 of 10 units: the camera slides 1 unit left.
        assert!(close(c.center, [-1.0, 0.0, 0.0], 1e-5));
        assert!(close(c.eye, [-1.0, 0.0, 10.0], 1e-5));
        c.pan(0.0, 100.0);
        assert!(close(c.center, [-1.0, 1.0, 0.0], 1e-5));
    }

    #[test]
    fn zoom_and_wheel() {
        let mut c = Camera { eye: [0.0, 0.0, 10.0], center: [0.0; 3], up: [0.0, 1.0, 0.0] };
        c.zoom(wheel_zoom(40.0));
        assert!((c.distance() - 9.0).abs() < 1e-5);
        c.zoom(wheel_zoom(-400.0));
        assert!((c.distance() - 9.9).abs() < 1e-4, "a frame moves at most one step");
        c.zoom(5.0);
        assert!(c.distance() > 0.0);
        assert!((pinch_zoom(2.0) - 0.5).abs() < 1e-6);
        assert_eq!(pinch_zoom(1.0), 0.0);
    }

    #[test]
    fn fit_fills_the_shorter_side() {
        let mut c = Camera::home(UpAxis::Y);
        c.fit([1.0, 2.0, 3.0], 2.0, 1.5);
        assert_eq!(c.center, [1.0, 2.0, 3.0]);
        let d = 2.0 / 22.5f32.to_radians().sin();
        assert!((c.distance() - d).abs() < 1e-4);
        // The direction is kept.
        let dir = math::normalize(math::sub(c.eye, c.center));
        assert!(close(dir, math::normalize(UpAxis::Y.home_direction()), 1e-5));
        // A tall view backs off further.
        assert!(fit_distance(2.0, 0.5) > fit_distance(2.0, 1.5));
        // The centre lands in the middle of the screen.
        let p = math::mul(&c.projection(Projection::Perspective, 1.5, 2.0), &c.view());
        let mid = math::transform_point(&p, c.center);
        assert!(mid[0].abs() < 1e-4 && mid[1].abs() < 1e-4);
        // Eagle's Z-up home looks from the front-left, above.
        assert!(UpAxis::Z.home_direction()[2] > 0.0);
    }

    #[test]
    fn clip_planes_hug_the_model() {
        let (n, f) = clip_planes(100.0, 1.0);
        assert!(n > 80.0 && f >= 110.0);
        let (n, f) = clip_planes(0.5, 1.0);
        assert!(n > 0.0 && n < 0.01 && f > 10.0);
        // A model at the origin of radius 0 still gets planes.
        let (n, f) = clip_planes(1.0, 0.0);
        assert!(n > 0.0 && f > n);
    }

    #[test]
    fn orthographic_keeps_the_size_at_the_centre() {
        let c = Camera { eye: [0.0, 0.0, 10.0], center: [0.0; 3], up: [0.0, 1.0, 0.0] };
        let y = 10.0 * (FOV_DEG.to_radians() / 2.0).tan();
        for mode in [Projection::Perspective, Projection::Orthographic] {
            let p = math::mul(&c.projection(mode, 1.0, 1.0), &c.view());
            assert!((math::transform_point(&p, [0.0, y, 0.0])[1] - 1.0).abs() < 1e-4, "{mode:?}");
        }
    }

    #[test]
    fn flip_turns_up_around() {
        let c = Camera::home(UpAxis::Z).flipped();
        assert_eq!(c.up, [0.0, 0.0, -1.0]);
    }

    #[test]
    fn grid_steps_are_round() {
        assert_eq!(grid_step(5.0), 1.0);
        assert_eq!(grid_step(50.0), 10.0);
        assert!((grid_step(1.0) - 0.2).abs() < 1e-6);
        assert!((grid_step(2.5) - 0.5).abs() < 1e-6);
        assert_eq!(grid_step(0.0), 0.2);
    }

    #[test]
    fn grid_lies_under_the_model() {
        let lines = grid_lines(([-1.0, -2.0, -1.0], [1.0, 2.0, 1.0]), UpAxis::Y);
        assert!(!lines.is_empty());
        assert!(lines.iter().all(|l| l[0][1] == -2.0 && l[1][1] == -2.0));
        // Z up: the floor is at the lowest Z.
        let lines = grid_lines(([0.0, 0.0, 3.0], [1.0, 1.0, 4.0]), UpAxis::Z);
        assert!(lines.iter().all(|l| l[0][2] == 3.0));
        // Lines in both directions, an odd count each (one through the middle).
        assert_eq!(lines.len() % 2, 0);
        assert_eq!((lines.len() / 2) % 2, 1);
        let (_, colour) = axes(1.0)[0];
        assert!(colour[0] > colour[1]);
    }

    #[test]
    fn detail_numbers_drop_trailing_zeros() {
        assert_eq!(detail_number(1.0), "1");
        assert_eq!(detail_number(2.5), "2.5");
        assert_eq!(detail_number(1.23456), "1.23");
        assert_eq!(detail_number(-0.001), "0");
        assert_eq!(detail_number(f64::NAN), "-");
    }

    #[test]
    fn rays_go_through_the_points_they_project_from() {
        for mode in [Projection::Perspective, Projection::Orthographic] {
            let cam = Camera { eye: [1.0, 2.0, 5.0], center: [0.0, 0.5, 0.0], up: [0.0, 1.0, 0.0] };
            let aspect = 1.6;
            let vp = math::mul(&cam.projection(mode, aspect, 1.0), &cam.view());
            let rect = [0.0, 0.0, 160.0, 100.0];
            let p = [0.3, 0.2, -0.4];
            let s = screen_point(&vp, p, rect).unwrap();
            let (x, y) = (s[0] / 80.0 - 1.0, 1.0 - s[1] / 50.0);
            let (o, d) = cam.ray(mode, aspect, x, y);
            // The point lies on the ray: its offset from the origin is along d.
            let off = math::sub(p, o);
            let along = math::dot(off, d);
            let miss = math::length(math::sub(off, math::scale(d, along)));
            assert!(miss < 1e-3, "{mode:?}: {miss}");
        }
        let cam = Camera::home(UpAxis::Y);
        let vp = math::mul(&cam.projection(Projection::Perspective, 1.0, 1.0), &cam.view());
        assert!(screen_point(&vp, math::scale(cam.eye, 2.0), [0.0, 0.0, 1.0, 1.0]).is_none(), "behind the eye");
    }
}
