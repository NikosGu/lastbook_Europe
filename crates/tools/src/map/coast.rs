//! The coast, from the PBF's `natural=coastline` ways, which OpenStreetMap draws with the land on their left.
//!
//! The page paints the sea first, then a coarse outline of the world's land (Natural Earth 1:110m, which has no Malta),
//! then the tiles, where an `island` is drawn in the land's colour and an `ocean` ring in the sea's: so a coast is where
//! OpenStreetMap has it, at every zoom. A coast the extract cuts (it runs on past the extract's edge, as along a river
//! mouth) cannot be closed from the extract alone; it is counted and left to the world outline.

use crate::map::geo::{self, Part};
use crate::map::osm::{Kind, Thing};
use std::collections::HashMap;

pub fn wanted(kind: Kind, t: &[(&str, &str)]) -> bool {
    kind == Kind::Way && t.iter().any(|&(k, v)| k == "natural" && v == "coastline")
}

#[derive(Debug, Default)]
pub struct Coast {
    /// Rings with land inside: islands, and a mainland the extract holds whole.
    pub islands: Vec<Part>,
    /// Rings with water inside.
    pub seas: Vec<Part>,
    /// Pieces of coast that close nothing within the extract.
    pub open: usize,
}

/// Twice the ring's area in square degrees, positive when it runs counter-clockwise (north up).
fn turning(r: &Part) -> f64 {
    r.windows(2).map(|w| w[0].0 * w[1].1 - w[1].0 * w[0].1).sum()
}

/// The coast's rings, from the coastline ways among `things`: each piece is joined to the one that starts where it ends.
pub fn coast(things: &[Thing]) -> Coast {
    let key = |p: (f64, f64)| (p.0.to_bits(), p.1.to_bits());
    let pieces: Vec<&Part> = things.iter().filter(|t| t.kind == Kind::Way && t.tags.get("natural") == Some("coastline"))
        .filter_map(|t| t.rings.first()).filter(|r| r.len() >= 2).collect();
    let mut starting: HashMap<(u64, u64), Vec<usize>> = HashMap::new();
    for (i, p) in pieces.iter().enumerate() {
        starting.entry(key(p[0])).or_default().push(i);
    }
    let ends: std::collections::HashSet<(u64, u64)> = pieces.iter().map(|p| key(p[p.len() - 1])).collect();
    let mut used = vec![false; pieces.len()];
    let mut out = Coast::default();
    // pieces no other piece leads into begin the open stretches; whatever is left goes round in rings
    let heads: Vec<usize> = (0..pieces.len()).filter(|&i| !ends.contains(&key(pieces[i][0]))).collect();
    let order: Vec<usize> = heads.into_iter().chain(0..pieces.len()).collect();
    for i in order {
        if used[i] {
            continue;
        }
        used[i] = true;
        let mut ring: Part = pieces[i].clone();
        while ring.first() != ring.last() {
            let next = starting.get(&key(ring[ring.len() - 1])).and_then(|c| c.iter().copied().find(|&j| !used[j]));
            let Some(j) = next else { break };
            used[j] = true;
            ring.extend_from_slice(&pieces[j][1..]);
        }
        if ring.len() < 4 || ring.first() != ring.last() {
            out.open += 1;
        } else if turning(&ring) > 0.0 {
            out.islands.push(ring);
        } else {
            out.seas.push(ring);
        }
    }
    out
}

/// The rings that reach into the boxes, simplified as the map's water is.
pub fn kept(rings: &[Part], boxes: &[[f64; 4]]) -> Vec<Part> {
    rings.iter().filter(|r| geo::bbox(std::slice::from_ref(*r)).is_some_and(|b| boxes.iter().any(|x| b.overlaps(x))))
        .map(|r| geo::simplify(r, 1e-5)).filter(|r| r.len() >= 4).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::osm::Tags;

    fn way(id: i64, pts: &[(f64, f64)]) -> Thing {
        Thing { kind: Kind::Way, id, tags: Tags(vec![("natural".into(), "coastline".into())]), rings: vec![pts.to_vec()], point: pts[0] }
    }

    #[test]
    fn islands_seas_and_open_coast() {
        // an island in two pieces, counter-clockwise (land on the left); a lagoon, clockwise; a coast the extract cuts
        let island = [(14.20, 36.00), (14.30, 36.00), (14.30, 36.08)];
        let island_back = [(14.30, 36.08), (14.20, 36.08), (14.20, 36.00)];
        let lagoon = [(14.40, 35.90), (14.40, 35.95), (14.45, 35.95), (14.45, 35.90), (14.40, 35.90)];
        let cut = [(8.50, 53.55), (8.55, 53.56), (8.57, 53.60)];
        let c = coast(&[way(1, &island_back), way(2, &lagoon), way(3, &cut), way(4, &island)]);
        assert_eq!((c.islands.len(), c.seas.len(), c.open), (1, 1, 1));
        assert_eq!(c.islands[0].len(), 5, "two pieces joined end to start, closed");
        assert!(geo::contains(&c.islands, 14.25, 36.04));
        // reversed, the same ring has the water inside
        let mut reversed: Vec<(f64, f64)> = c.islands[0].clone();
        reversed.reverse();
        assert_eq!(coast(&[way(5, &reversed)]).seas.len(), 1);
        // only what reaches into the boxes is kept
        assert_eq!(kept(&c.islands, &[[14.03, 35.65, 14.73, 36.24]]).len(), 1);
        assert!(kept(&c.islands, &[[9.32, 46.90, 9.79, 47.43]]).is_empty());
    }
}
