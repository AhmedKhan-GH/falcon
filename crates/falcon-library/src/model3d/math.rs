//! Small vector and matrix helpers for the 3D viewer and the thumbnail
//! renderer. Vectors are `[f32; 3]`; matrices are column-major `[[f32; 4]; 4]`
//! (`m[column][row]`), as WGSL reads them. The camera space is right-handed
//! (the camera looks down −Z) and clip depth runs from 0 (near) to 1 (far),
//! as wgpu expects.

pub type V3 = [f32; 3];
pub type M4 = [[f32; 4]; 4];

pub fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

pub fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

pub fn scale(a: V3, s: f32) -> V3 {
    [a[0] * s, a[1] * s, a[2] * s]
}

pub fn dot(a: V3, b: V3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

pub fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

pub fn length(a: V3) -> f32 {
    dot(a, a).sqrt()
}

/// `a` scaled to length 1; the zero vector stays zero.
pub fn normalize(a: V3) -> V3 {
    let l = length(a);
    if l > 0.0 { scale(a, 1.0 / l) } else { a }
}

/// `p` turned by `angle` radians about the axis through `origin` along the
/// unit vector `axis`, counter-clockwise looking down the axis (Rodrigues).
pub fn rotate_about(p: V3, origin: V3, axis: V3, angle: f32) -> V3 {
    let v = sub(p, origin);
    let (s, c) = angle.sin_cos();
    let r = add(add(scale(v, c), scale(cross(axis, v), s)), scale(axis, dot(axis, v) * (1.0 - c)));
    add(origin, r)
}

pub fn mul(a: &M4, b: &M4) -> M4 {
    let mut m = [[0.0; 4]; 4];
    for (c, col) in m.iter_mut().enumerate() {
        for (r, cell) in col.iter_mut().enumerate() {
            *cell = (0..4).map(|k| a[k][r] * b[c][k]).sum();
        }
    }
    m
}

/// `m` applied to the point `p`, after the divide by w.
pub fn transform_point(m: &M4, p: V3) -> V3 {
    let v = [p[0], p[1], p[2], 1.0];
    let mut o = [0.0f32; 4];
    for (r, out) in o.iter_mut().enumerate() {
        *out = (0..4).map(|k| m[k][r] * v[k]).sum();
    }
    let w = if o[3] != 0.0 { o[3] } else { 1.0 };
    [o[0] / w, o[1] / w, o[2] / w]
}

/// The view matrix of a camera at `eye` looking at `center` with `up`
/// pointing up the screen.
pub fn look_at(eye: V3, center: V3, up: V3) -> M4 {
    let f = normalize(sub(center, eye));
    let s = normalize(cross(f, up));
    let u = cross(s, f);
    [
        [s[0], u[0], -f[0], 0.0],
        [s[1], u[1], -f[1], 0.0],
        [s[2], u[2], -f[2], 0.0],
        [-dot(s, eye), -dot(u, eye), dot(f, eye), 1.0],
    ]
}

/// A perspective projection: `fovy` is the vertical field of view in
/// radians, `aspect` width over height.
pub fn perspective(fovy: f32, aspect: f32, near: f32, far: f32) -> M4 {
    let f = 1.0 / (fovy / 2.0).tan();
    [
        [f / aspect, 0.0, 0.0, 0.0],
        [0.0, f, 0.0, 0.0],
        [0.0, 0.0, far / (near - far), -1.0],
        [0.0, 0.0, near * far / (near - far), 0.0],
    ]
}

/// An orthographic projection showing `half_h` units above and below the
/// centre of the screen.
pub fn orthographic(half_h: f32, aspect: f32, near: f32, far: f32) -> M4 {
    let half_w = half_h * aspect;
    [
        [1.0 / half_w, 0.0, 0.0, 0.0],
        [0.0, 1.0 / half_h, 0.0, 0.0],
        [0.0, 0.0, 1.0 / (near - far), 0.0],
        [0.0, 0.0, near / (near - far), 1.0],
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: V3, b: V3) -> bool {
        (0..3).all(|i| (a[i] - b[i]).abs() < 1e-4)
    }

    #[test]
    fn rotation_turns_counter_clockwise() {
        let p = rotate_about([1.0, 0.0, 0.0], [0.0; 3], [0.0, 0.0, 1.0], std::f32::consts::FRAC_PI_2);
        assert!(close(p, [0.0, 1.0, 0.0]));
        // About an axis through another point.
        let p = rotate_about([2.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], std::f32::consts::PI);
        assert!(close(p, [0.0, 0.0, 0.0]));
    }

    #[test]
    fn look_at_puts_the_target_in_front() {
        let v = look_at([0.0, 0.0, 5.0], [0.0; 3], [0.0, 1.0, 0.0]);
        assert!(close(transform_point(&v, [0.0; 3]), [0.0, 0.0, -5.0]));
        assert!(close(transform_point(&v, [0.0, 1.0, 0.0]), [0.0, 1.0, -5.0]));
        assert!(close(transform_point(&v, [1.0, 0.0, 0.0]), [1.0, 0.0, -5.0]));
    }

    #[test]
    fn projections_map_near_and_far_to_zero_and_one() {
        let p = perspective(1.0, 1.5, 0.5, 50.0);
        assert!((transform_point(&p, [0.0, 0.0, -0.5])[2]).abs() < 1e-5);
        assert!((transform_point(&p, [0.0, 0.0, -50.0])[2] - 1.0).abs() < 1e-4);
        // The edge of the field of view lands on the edge of the screen.
        let y = (0.5f32).tan() * 10.0;
        assert!((transform_point(&p, [0.0, y, -10.0])[1] - 1.0).abs() < 1e-4);
        let o = orthographic(2.0, 2.0, 1.0, 9.0);
        assert!(close(transform_point(&o, [4.0, 2.0, -1.0]), [1.0, 1.0, 0.0]));
        assert!((transform_point(&o, [0.0, 0.0, -9.0])[2] - 1.0).abs() < 1e-5);
        let id = mul(&o, &[[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]]);
        assert_eq!(id, o);
    }
}
