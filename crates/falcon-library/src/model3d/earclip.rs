//! Triangulating flat polygons, concave ones and ones with holes, by ear
//! clipping: each hole is joined to the outer loop by a bridge to a vertex
//! it can see, then ears (convex corners with no other vertex inside) are
//! cut off one by one. A 3D polygon is first projected onto the axis plane
//! it faces most. Used by the IFC reader for faceted faces.

pub type P2 = [f64; 2];
pub type P3 = [f64; 3];

/// Above this many vertices a face is cut as a fan of its outer loop, so
/// a huge face can't take quadratic time.
const MAX_POINTS: usize = 20_000;

fn cross(a: P2, b: P2, c: P2) -> f64 {
    (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
}

/// Twice the signed area: positive when counter-clockwise.
fn signed_area(pts: &[P2], ring: &[usize]) -> f64 {
    let n = ring.len();
    (0..n).map(|k| {
        let (a, b) = (pts[ring[k]], pts[ring[(k + 1) % n]]);
        a[0] * b[1] - b[0] * a[1]
    }).sum()
}

/// Whether segments ab and cd cross at a point inside both.
fn segments_cross(a: P2, b: P2, c: P2, d: P2) -> bool {
    let d1 = cross(a, b, c);
    let d2 = cross(a, b, d);
    let d3 = cross(c, d, a);
    let d4 = cross(c, d, b);
    ((d1 > 0.0 && d2 < 0.0) || (d1 < 0.0 && d2 > 0.0)) && ((d3 > 0.0 && d4 < 0.0) || (d3 < 0.0 && d4 > 0.0))
}

/// Whether the direction from ring vertex `k` towards `q` points into the
/// polygon (the ring runs counter-clockwise).
fn locally_inside(pts: &[P2], ring: &[usize], k: usize, q: P2) -> bool {
    let n = ring.len();
    let (a, b, c) = (pts[ring[(k + n - 1) % n]], pts[ring[k]], pts[ring[(k + 1) % n]]);
    if cross(a, b, c) >= 0.0 {
        // Convex corner: q lies left of both edges.
        cross(b, c, q) >= 0.0 && cross(a, b, q) >= 0.0
    } else {
        // Reflex corner: q is not in the outside wedge.
        cross(b, c, q) >= 0.0 || cross(a, b, q) >= 0.0
    }
}

/// Splices `hole` (clockwise) into `ring` (counter-clockwise) through a
/// bridge from the hole's rightmost vertex to a ring vertex it can see.
fn bridge(pts: &[P2], ring: &mut Vec<usize>, hole: &[usize], others: &[Vec<usize>]) {
    let (hk, &m) = hole.iter().enumerate().max_by(|a, b| pts[*a.1][0].total_cmp(&pts[*b.1][0])).expect("a hole has vertices");
    let pm = pts[m];
    let d2 = |p: P2| (p[0] - pm[0]).powi(2) + (p[1] - pm[1]).powi(2);
    let mut order: Vec<usize> = (0..ring.len()).collect();
    order.sort_by(|&a, &b| d2(pts[ring[a]]).total_cmp(&d2(pts[ring[b]])));
    let blocked = |pp: P2| {
        let edges = std::iter::once(ring.as_slice()).chain(std::iter::once(hole)).chain(others.iter().map(Vec::as_slice));
        for r in edges {
            let n = r.len();
            for e in 0..n {
                let (c, d) = (pts[r[e]], pts[r[(e + 1) % n]]);
                if segments_cross(pm, pp, c, d) {
                    return true;
                }
            }
        }
        false
    };
    let pos = order.iter().copied().find(|&k| locally_inside(pts, ring, k, pm) && !blocked(pts[ring[k]])).unwrap_or(order[0]);
    let p = ring[pos];
    let mut spliced = Vec::with_capacity(ring.len() + hole.len() + 2);
    spliced.extend_from_slice(&ring[..=pos]);
    spliced.extend_from_slice(&hole[hk..]);
    spliced.extend_from_slice(&hole[..=hk]);
    spliced.push(p);
    spliced.extend_from_slice(&ring[pos + 1..]);
    *ring = spliced;
}

/// Triangulates a flat polygon with holes. The result indexes the outer
/// loop's points followed by each hole's in turn, and runs the same way
/// round as the outer loop. Degenerate input gives fewer (or no)
/// triangles, never a panic.
pub fn triangulate_2d(outer: &[P2], holes: &[&[P2]]) -> Vec<[usize; 3]> {
    let mut pts: Vec<P2> = outer.to_vec();
    let mut hole_rings = Vec::new();
    for h in holes {
        let start = pts.len();
        pts.extend_from_slice(h);
        if h.len() >= 3 {
            hole_rings.push((start..pts.len()).collect::<Vec<usize>>());
        }
    }
    if outer.len() < 3 || pts.iter().any(|p| !p[0].is_finite() || !p[1].is_finite()) {
        return Vec::new();
    }
    let mut ring: Vec<usize> = (0..outer.len()).collect();
    let flip = signed_area(&pts, &ring) < 0.0;
    if flip {
        ring.reverse();
    }
    let fan = pts.len() > MAX_POINTS;
    let mut tris = if fan {
        (1..ring.len() - 1).map(|k| [ring[0], ring[k], ring[k + 1]]).collect()
    } else {
        for h in &mut hole_rings {
            if signed_area(&pts, h) > 0.0 {
                h.reverse();
            }
        }
        hole_rings.retain(|h| signed_area(&pts, h) != 0.0);
        hole_rings.sort_by(|a, b| {
            let mx = |r: &Vec<usize>| r.iter().map(|&i| pts[i][0]).fold(f64::MIN, f64::max);
            mx(b).total_cmp(&mx(a))
        });
        for k in 0..hole_rings.len() {
            let (done, rest) = hole_rings.split_at(k + 1);
            bridge(&pts, &mut ring, &done[k], rest);
        }
        clip(&pts, ring)
    };
    if flip {
        for t in &mut tris {
            t.swap(1, 2);
        }
    }
    tris
}

/// Cuts ears off a counter-clockwise ring until three corners are left.
fn clip(pts: &[P2], mut v: Vec<usize>) -> Vec<[usize; 3]> {
    let (mut lo, mut hi) = ([f64::MAX; 2], [f64::MIN; 2]);
    for &i in &v {
        for k in 0..2 {
            lo[k] = lo[k].min(pts[i][k]);
            hi[k] = hi[k].max(pts[i][k]);
        }
    }
    let size = (hi[0] - lo[0]).max(hi[1] - lo[1]);
    let eps = size * size * 1e-12;
    let mut tris = Vec::with_capacity(v.len().saturating_sub(2));
    let mut i = 0;
    let mut misses = 0;
    while v.len() > 3 {
        let n = v.len();
        i %= n;
        let (ia, ib, ic) = (v[(i + n - 1) % n], v[i], v[(i + 1) % n]);
        let (a, b, c) = (pts[ia], pts[ib], pts[ic]);
        let cr = cross(a, b, c);
        if cr.abs() <= eps {
            // Collinear or doubled back: the corner adds no area.
            v.remove(i);
            misses = 0;
            continue;
        }
        let ear = cr > 0.0
            && !v.iter().any(|&j| {
                let p = pts[j];
                p != a && p != b && p != c && cross(a, b, p) >= 0.0 && cross(b, c, p) >= 0.0 && cross(c, a, p) >= 0.0
            });
        if ear || misses > n {
            // No ear in a whole lap means the ring self-intersects; cutting
            // the corner anyway still ends.
            tris.push([ia, ib, ic]);
            v.remove(i);
            misses = 0;
        } else {
            i += 1;
            misses += 1;
        }
    }
    if v.len() == 3 && cross(pts[v[0]], pts[v[1]], pts[v[2]]).abs() > eps {
        tris.push([v[0], v[1], v[2]]);
    }
    tris
}

/// The Newell normal of a 3D loop (not normalised).
pub fn newell(pts: &[P3]) -> P3 {
    let mut n = [0.0; 3];
    for k in 0..pts.len() {
        let (a, b) = (pts[k], pts[(k + 1) % pts.len()]);
        n[0] += (a[1] - b[1]) * (a[2] + b[2]);
        n[1] += (a[2] - b[2]) * (a[0] + b[0]);
        n[2] += (a[0] - b[0]) * (a[1] + b[1]);
    }
    n
}

/// Triangulates a flat (or nearly flat) 3D polygon with holes, projected
/// onto the axis plane it faces most; indices as in [`triangulate_2d`],
/// running the same way round as the outer loop.
pub fn triangulate_3d(outer: &[P3], holes: &[&[P3]]) -> Vec<[usize; 3]> {
    if outer.len() == 3 {
        return vec![[0, 1, 2]];
    }
    let n = newell(outer);
    let k = (0..3).max_by(|&a, &b| n[a].abs().total_cmp(&n[b].abs())).unwrap_or(2);
    // The other two axes in cyclic order keep the loop's turning sense.
    let (u, w) = ((k + 1) % 3, (k + 2) % 3);
    let flat = |l: &[P3]| l.iter().map(|p| [p[u], p[w]]).collect::<Vec<P2>>();
    let o = flat(outer);
    let hs: Vec<Vec<P2>> = holes.iter().map(|h| flat(h)).collect();
    let hr: Vec<&[P2]> = hs.iter().map(Vec::as_slice).collect();
    triangulate_2d(&o, &hr)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all_pts(outer: &[P2], holes: &[&[P2]]) -> Vec<P2> {
        let mut v = outer.to_vec();
        for h in holes {
            v.extend_from_slice(h);
        }
        v
    }

    /// The triangles' total signed area (counter-clockwise positive).
    fn area(pts: &[P2], tris: &[[usize; 3]]) -> f64 {
        tris.iter().map(|t| cross(pts[t[0]], pts[t[1]], pts[t[2]]) / 2.0).sum()
    }

    /// Checks the area covered and the winding; at most `max_tris`
    /// triangles (fewer when collinear corners are dropped).
    fn check(outer: &[P2], holes: &[&[P2]], want_area: f64, max_tris: usize) {
        let tris = triangulate_2d(outer, holes);
        let pts = all_pts(outer, holes);
        assert!(tris.len() <= max_tris, "{tris:?}");
        assert!((area(&pts, &tris) - want_area).abs() < 1e-9, "area {} want {want_area}", area(&pts, &tris));
        // Every triangle turns the same way as the outer loop.
        let sign = want_area.signum();
        for t in &tris {
            assert!(cross(pts[t[0]], pts[t[1]], pts[t[2]]) * sign > 0.0, "{t:?}");
        }
    }

    #[test]
    fn concave_l_shape() {
        let l = [[0.0, 0.0], [2.0, 0.0], [2.0, 1.0], [1.0, 1.0], [1.0, 2.0], [0.0, 2.0]];
        check(&l, &[], 3.0, 4);
        // Clockwise input gives clockwise triangles.
        let mut cw = l;
        cw.reverse();
        check(&cw, &[], -3.0, 4);
    }

    #[test]
    fn concave_comb_and_star() {
        // A comb with three teeth.
        let comb = [[0.0, 0.0], [5.0, 0.0], [5.0, 3.0], [4.0, 3.0], [4.0, 1.0], [3.0, 1.0], [3.0, 3.0], [2.0, 3.0], [2.0, 1.0], [1.0, 1.0], [1.0, 3.0], [0.0, 3.0]];
        check(&comb, &[], 5.0 + 3.0 * 2.0, 10);
        // A five-pointed star: ten corners, half of them reflex.
        let star: Vec<P2> = (0..10)
            .map(|k| {
                let r = if k % 2 == 0 { 2.0 } else { 0.8 };
                let a = std::f64::consts::PI * 2.0 * k as f64 / 10.0;
                [r * a.cos(), r * a.sin()]
            })
            .collect();
        let tris = triangulate_2d(&star, &[]);
        assert_eq!(tris.len(), 8);
        let want = 10.0 * 0.5 * 2.0 * 0.8 * (std::f64::consts::PI / 5.0).sin();
        assert!((area(&star, &tris) - want).abs() < 1e-9);
    }

    #[test]
    fn squares_with_holes() {
        let outer = [[0.0, 0.0], [4.0, 0.0], [4.0, 4.0], [0.0, 4.0]];
        let hole = [[1.0, 1.0], [2.0, 1.0], [2.0, 2.0], [1.0, 2.0]];
        // n + h + 2 * holes - 2 triangles.
        check(&outer, &[&hole], 15.0, 8);
        // Two holes, one of them given clockwise already.
        let h2 = [[3.0, 3.5], [3.5, 3.5], [3.5, 3.0], [3.0, 3.0]];
        check(&outer, &[&hole, &h2], 16.0 - 1.0 - 0.25, 14);
        // A hole in a concave outline.
        let u = [[0.0, 0.0], [6.0, 0.0], [6.0, 4.0], [4.0, 4.0], [4.0, 2.0], [2.0, 2.0], [2.0, 4.0], [0.0, 4.0]];
        let h = [[0.5, 0.5], [5.5, 0.5], [5.5, 1.5], [0.5, 1.5]];
        check(&u, &[&h], 24.0 - 4.0 - 5.0, 12);
    }

    #[test]
    fn collinear_and_degenerate_input() {
        // A square with extra points along its sides.
        let sq = [[0.0, 0.0], [1.0, 0.0], [2.0, 0.0], [2.0, 2.0], [1.0, 2.0], [0.0, 2.0], [0.0, 1.0]];
        let tris = triangulate_2d(&sq, &[]);
        assert!((area(&sq, &tris) - 4.0).abs() < 1e-9);
        // A line has no area and no triangles.
        assert!(triangulate_2d(&[[0.0, 0.0], [1.0, 0.0], [2.0, 0.0], [3.0, 0.0]], &[]).is_empty());
        assert!(triangulate_2d(&[[0.0, 0.0], [1.0, 0.0]], &[]).is_empty());
        assert!(triangulate_2d(&[[0.0, 0.0], [f64::NAN, 0.0], [1.0, 1.0]], &[]).is_empty());
        // A bow tie still ends.
        let bow = [[0.0, 0.0], [2.0, 2.0], [2.0, 0.0], [0.0, 2.0]];
        assert!(triangulate_2d(&bow, &[]).len() <= 2);
        // Repeated points.
        let rep = [[0.0, 0.0], [0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [1.0, 1.0], [0.0, 1.0]];
        let tris = triangulate_2d(&rep, &[]);
        assert!((area(&rep, &tris) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn faces_in_3d_keep_their_winding() {
        // An L in the XZ plane (a wall), wound so its normal is −Y.
        let l: Vec<P3> = [[0.0, 0.0], [2.0, 0.0], [2.0, 1.0], [1.0, 1.0], [1.0, 2.0], [0.0, 2.0]].iter().map(|p| [p[0], 5.0, p[1]]).collect();
        let n = newell(&l);
        assert!(n[1] < 0.0);
        let tris = triangulate_3d(&l, &[]);
        assert_eq!(tris.len(), 4);
        for t in &tris {
            let [a, b, c] = t.map(|i| l[i]);
            let tn = [(b[1] - a[1]) * (c[2] - a[2]) - (b[2] - a[2]) * (c[1] - a[1]), (b[2] - a[2]) * (c[0] - a[0]) - (b[0] - a[0]) * (c[2] - a[2]), (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])];
            assert!(tn[0] * n[0] + tn[1] * n[1] + tn[2] * n[2] > 0.0);
        }
        // With a hole, in a tilted plane.
        let tilt = |p: P2| [p[0], p[1], 0.5 * p[0]];
        let outer: Vec<P3> = [[0.0, 0.0], [4.0, 0.0], [4.0, 4.0], [0.0, 4.0]].map(tilt).to_vec();
        let hole: Vec<P3> = [[1.0, 1.0], [1.0, 2.0], [2.0, 2.0], [2.0, 1.0]].map(tilt).to_vec();
        assert_eq!(triangulate_3d(&outer, &[&hole]).len(), 8);
    }

    #[test]
    fn huge_faces_fall_back_to_a_fan() {
        let n = MAX_POINTS + 10;
        let ring: Vec<P2> = (0..n).map(|k| {
            let a = std::f64::consts::TAU * k as f64 / n as f64;
            [a.cos(), a.sin()]
        }).collect();
        assert_eq!(triangulate_2d(&ring, &[]).len(), n - 2);
    }
}
