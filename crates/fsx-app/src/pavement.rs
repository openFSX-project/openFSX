//! Airport aprons and taxiways as world-space triangles: apron outlines are
//! ear-clipped, taxi paths become strips with round joints and a centreline.

/// Triangulate a simple polygon (x, z) by ear clipping; either winding.
pub fn triangulate(points: &[[f32; 2]]) -> Vec<[usize; 3]> {
    let n = points.len();
    if n < 3 {
        return Vec::new();
    }
    let area: f32 = (0..n)
        .map(|i| {
            let (a, b) = (points[i], points[(i + 1) % n]);
            a[0] * b[1] - b[0] * a[1]
        })
        .sum();
    let ccw = area > 0.;
    let mut remaining: Vec<usize> = (0..n).collect();
    let mut out = Vec::new();
    let cross = |o: [f32; 2], a: [f32; 2], b: [f32; 2]| {
        (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0])
    };
    let mut guard = 0;
    while remaining.len() > 3 && guard < n * n {
        guard += 1;
        let m = remaining.len();
        let mut clipped = false;
        for i in 0..m {
            let (ia, ib, ic) = (
                remaining[(i + m - 1) % m],
                remaining[i],
                remaining[(i + 1) % m],
            );
            let (a, b, c) = (points[ia], points[ib], points[ic]);
            let turn = cross(a, b, c);
            if (turn > 0.) != ccw || turn.abs() < 1e-9 {
                continue;
            }
            let inside = remaining.iter().any(|&j| {
                if j == ia || j == ib || j == ic {
                    return false;
                }
                let p = points[j];
                let (d1, d2, d3) = (cross(a, b, p), cross(b, c, p), cross(c, a, p));
                if ccw {
                    d1 >= 0. && d2 >= 0. && d3 >= 0.
                } else {
                    d1 <= 0. && d2 <= 0. && d3 <= 0.
                }
            });
            if inside {
                continue;
            }
            out.push([ia, ib, ic]);
            remaining.remove(i);
            clipped = true;
            break;
        }
        if !clipped {
            // Self-intersecting or degenerate outline: fan the rest.
            break;
        }
    }
    for k in 1..remaining.len().saturating_sub(1) {
        out.push([remaining[0], remaining[k], remaining[k + 1]]);
    }
    out
}
/// A flat strip of `width` from a to b plus a disc at each end, at height y.
pub fn strip(out: &mut Vec<[f32; 3]>, a: [f32; 2], b: [f32; 2], width: f32, y: f32) {
    let (dx, dz) = (b[0] - a[0], b[1] - a[1]);
    let len = (dx * dx + dz * dz).sqrt();
    if len < 0.01 || width <= 0. {
        return;
    }
    let (nx, nz) = (-dz / len * width / 2., dx / len * width / 2.);
    let quad = [
        [a[0] + nx, a[1] + nz],
        [b[0] + nx, b[1] + nz],
        [b[0] - nx, b[1] - nz],
        [a[0] - nx, a[1] - nz],
    ];
    for i in [0, 1, 2, 0, 2, 3] {
        out.push([quad[i][0], y, quad[i][1]]);
    }
}
pub fn disc(out: &mut Vec<[f32; 3]>, c: [f32; 2], radius: f32, y: f32) {
    const SIDES: usize = 10;
    for k in 0..SIDES {
        let (t0, t1) = (
            k as f32 / SIDES as f32 * std::f32::consts::TAU,
            (k + 1) as f32 / SIDES as f32 * std::f32::consts::TAU,
        );
        out.push([c[0], y, c[1]]);
        out.push([c[0] + radius * t0.cos(), y, c[1] + radius * t0.sin()]);
        out.push([c[0] + radius * t1.cos(), y, c[1] + radius * t1.sin()]);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ear_clipping_covers_concave_polygons_in_either_winding() {
        // An L shape of area 3.
        let l = [[0., 0.], [2., 0.], [2., 1.], [1., 1.], [1., 2.], [0., 2.]];
        for points in [l.to_vec(), l.iter().rev().copied().collect()] {
            let t = triangulate(&points);
            assert_eq!(t.len(), 4);
            let area: f32 = t
                .iter()
                .map(|[a, b, c]| {
                    let (a, b, c) = (points[*a], points[*b], points[*c]);
                    ((b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])).abs() / 2.
                })
                .sum();
            assert!((area - 3.).abs() < 1e-5);
        }
        assert!(triangulate(&[[0., 0.], [1., 0.]]).is_empty());
    }
}
