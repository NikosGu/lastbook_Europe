//! Geometry the builders share: parts of lon/lat points, boxes, the original Lastbook's simplification and zoom rules,
//! centroids, areas, points in rings, and the zoom-30 pixels building outlines are filed in.

use std::f64::consts::PI;

/// A line or a ring, as (lon, lat) points.
pub type Part = Vec<(f64, f64)>;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BBox {
    pub minx: f64,
    pub maxx: f64,
    pub miny: f64,
    pub maxy: f64,
}

impl BBox {
    pub fn center(&self) -> (f64, f64) {
        ((self.minx + self.maxx) / 2.0, (self.miny + self.maxy) / 2.0)
    }

    /// Does it overlap a box given as [west, south, east, north]?
    pub fn overlaps(&self, b: &[f64; 4]) -> bool {
        self.maxx >= b[0] && self.minx <= b[2] && self.maxy >= b[1] && self.miny <= b[3]
    }
}

pub fn bbox(parts: &[Part]) -> Option<BBox> {
    let mut b = BBox { minx: f64::MAX, maxx: f64::MIN, miny: f64::MAX, maxy: f64::MIN };
    let mut any = false;
    for &(x, y) in parts.iter().flatten() {
        b = BBox { minx: b.minx.min(x), maxx: b.maxx.max(x), miny: b.miny.min(y), maxy: b.maxy.max(y) };
        any = true;
    }
    any.then_some(b)
}

/// Points closer than `tol` degrees to the last one kept are dropped; the ends stay.
pub fn simplify(p: &Part, tol: f64) -> Part {
    if p.len() <= 2 || tol <= 0.0 {
        return p.clone();
    }
    let mut out = vec![p[0]];
    let mut last = p[0];
    for &q in &p[1..p.len() - 1] {
        if (q.0 - last.0).abs() > tol || (q.1 - last.1).abs() > tol {
            out.push(q);
            last = q;
        }
    }
    out.push(p[p.len() - 1]);
    out
}

/// The zoom an area appears from: small areas only when zoomed in (about a sixteenth of the screen), never before `base`.
pub fn area_z(b: &BBox, base: i64) -> i64 {
    let size = (b.maxx - b.minx).max(b.maxy - b.miny);
    if size <= 0.0 {
        return 17;
    }
    let z = (360.0 / size).log2() as i64 - 4;
    base.max(z.min(16))
}

/// A ring without its closing point.
pub fn open(r: &Part) -> &[(f64, f64)] {
    if r.len() > 1 && r[0] == r[r.len() - 1] {
        &r[..r.len() - 1]
    } else {
        r
    }
}

/// Area of a ring in square metres (flat, at its mean latitude: fine for a building, a lake, a town).
pub fn ring_area_m2(r: &Part) -> f64 {
    let pts = open(r);
    if pts.len() < 3 {
        return 0.0;
    }
    let lat0 = pts.iter().map(|p| p.1).sum::<f64>() / pts.len() as f64;
    let (kx, ky) = (111_320.0 * lat0.to_radians().cos(), 110_540.0);
    let mut a = 0.0;
    for i in 0..pts.len() {
        let (x1, y1) = pts[i];
        let (x2, y2) = pts[(i + 1) % pts.len()];
        a += x1 * kx * (y2 * ky) - x2 * kx * (y1 * ky);
    }
    (a / 2.0).abs()
}

/// The middle of a ring (its centroid; the mean of its points when it has no area).
pub fn ring_centroid(r: &Part) -> (f64, f64) {
    let pts = open(r);
    if pts.is_empty() {
        return (0.0, 0.0);
    }
    let (ox, oy) = pts[0];
    let (mut a, mut cx, mut cy) = (0.0, 0.0, 0.0);
    for i in 0..pts.len() {
        let (x1, y1) = (pts[i].0 - ox, pts[i].1 - oy);
        let (x2, y2) = (pts[(i + 1) % pts.len()].0 - ox, pts[(i + 1) % pts.len()].1 - oy);
        let c = x1 * y2 - x2 * y1;
        a += c;
        cx += (x1 + x2) * c;
        cy += (y1 + y2) * c;
    }
    if a.abs() < 1e-14 {
        let n = pts.len() as f64;
        return (pts.iter().map(|p| p.0).sum::<f64>() / n, pts.iter().map(|p| p.1).sum::<f64>() / n);
    }
    (ox + cx / (3.0 * a), oy + cy / (3.0 * a))
}

/// The middle of a shape: the centroid of its largest ring, or of its points.
pub fn centroid(parts: &[Part]) -> Option<(f64, f64)> {
    let largest = parts.iter().filter(|p| p.len() >= 3).max_by(|a, b| ring_area_m2(a).total_cmp(&ring_area_m2(b)));
    match largest {
        Some(r) => Some(ring_centroid(r)),
        None => {
            let pts: Vec<&(f64, f64)> = parts.iter().flatten().collect();
            (!pts.is_empty()).then(|| (pts.iter().map(|p| p.0).sum::<f64>() / pts.len() as f64, pts.iter().map(|p| p.1).sum::<f64>() / pts.len() as f64))
        }
    }
}

/// Is the point inside these rings (outer rings and holes together, even-odd)?
pub fn contains(rings: &[Part], lon: f64, lat: f64) -> bool {
    let mut inside = false;
    for r in rings {
        let pts = open(r);
        let n = pts.len();
        if n < 3 {
            continue;
        }
        let mut j = n - 1;
        for i in 0..n {
            let (xi, yi) = pts[i];
            let (xj, yj) = pts[j];
            if (yi > lat) != (yj > lat) && lon < (xj - xi) * (lat - yi) / (yj - yi) + xi {
                inside = !inside;
            }
            j = i;
        }
    }
    inside
}

/// Web-mercator pixels at zoom 30 (units of 2^-30 of the world), as building outlines are stored.
pub fn world30(lon: f64, lat: f64) -> (i64, i64) {
    const WORLD: f64 = (1u64 << 30) as f64;
    let s = lat.clamp(-85.0, 85.0).to_radians().sin();
    (((lon + 180.0) / 360.0 * WORLD) as i64, ((0.5 - ((1.0 + s) / (1.0 - s)).ln() / (4.0 * PI)) * WORLD) as i64)
}

/// The tile holding a point at a zoom.
pub fn tile_of(z: u32, lon: f64, lat: f64) -> (i64, i64) {
    let n = 2f64.powi(z as i32);
    let s = lat.clamp(-85.0, 85.0).to_radians().sin();
    (((lon + 180.0) / 360.0 * n).floor() as i64, ((0.5 - ((1.0 + s) / (1.0 - s)).ln() / (4.0 * PI)) * n).floor() as i64)
}

/// The tiles of a zoom that a box [west, south, east, north] touches: (x0, x1, y0, y1), inclusive.
pub fn tile_range(z: u32, b: &[f64; 4]) -> (i64, i64, i64, i64) {
    let (x0, y0) = tile_of(z, b[0], b[3]);
    let (x1, y1) = tile_of(z, b[2], b[1]);
    (x0, x1, y0, y1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shapes() {
        let sq = vec![(0.0, 0.0), (0.001, 0.0), (0.001, 0.001), (0.0, 0.001), (0.0, 0.0)];
        let c = ring_centroid(&sq);
        assert!((c.0 - 0.0005).abs() < 1e-12 && (c.1 - 0.0005).abs() < 1e-12);
        let a = ring_area_m2(&sq);
        assert!((a - 12_305.0).abs() < 50.0, "about 111 m by 111 m: {a}");
        assert!(contains(&[sq.clone()], 0.0005, 0.0005));
        assert!(!contains(&[sq.clone()], 0.002, 0.0005));
        let hole = vec![(0.0004, 0.0004), (0.0006, 0.0004), (0.0006, 0.0006), (0.0004, 0.0006)];
        assert!(!contains(&[sq.clone(), hole], 0.0005, 0.0005), "a hole is outside");
        let b = bbox(&[sq.clone()]).unwrap();
        assert_eq!((b.minx, b.maxy), (0.0, 0.001));
        assert_eq!(area_z(&b, 9), 14);
        assert_eq!(area_z(&BBox { minx: 0.0, maxx: 1.0, miny: 0.0, maxy: 1.0 }, 7), 7);
        let line = vec![(0.0, 0.0), (0.000001, 0.0), (0.5, 0.0), (0.500001, 0.0), (1.0, 0.0)];
        assert_eq!(simplify(&line, 0.00001), vec![(0.0, 0.0), (0.5, 0.0), (1.0, 0.0)]);
        let (x, y) = world30(9.5209, 47.1410);
        assert_eq!((x >> 16, y >> 16), tile_of(14, 9.5209, 47.1410));
    }
}
