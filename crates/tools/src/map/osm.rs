//! Reads what the builders need from a region's OpenStreetMap PBF (ODbL): the elements a filter keeps, each with its tags
//! and its place: a node's position, a way's points, a relation's outer rings joined from its member ways.
//!
//! Three passes, so memory grows with what is kept, not with the file: relations first (which member ways they need),
//! then ways (which nodes they need), then nodes (only those positions).

use crate::map::geo::{self, Part};
use osmpbf::{Element, ElementReader, RelMemberType};
use std::collections::{HashMap, HashSet};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    Node,
    Way,
    Relation,
}

#[derive(Debug, Clone, Default)]
pub struct Tags(pub Vec<(String, String)>);

impl Tags {
    pub fn get(&self, k: &str) -> Option<&str> {
        self.0.iter().find(|(a, _)| a == k).map(|(_, b)| b.as_str()).filter(|v| !v.is_empty())
    }

    pub fn has(&self, k: &str) -> bool {
        self.get(k).is_some()
    }
}

/// One element kept: its tags, its rings (a way's line, a relation's outer rings; none for a node) and a point for it
/// (the node, or the middle of its largest ring).
#[derive(Debug, Clone)]
pub struct Thing {
    pub kind: Kind,
    pub id: i64,
    pub tags: Tags,
    pub rings: Vec<Part>,
    pub point: (f64, f64),
}

/// Which elements to keep, from their tags.
pub type Keep<'a> = &'a (dyn Fn(Kind, &[(&str, &str)]) -> bool + Sync);

fn owned(t: &[(&str, &str)]) -> Tags {
    Tags(t.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect())
}

pub fn read(path: &Path, keep: Keep) -> Result<Vec<Thing>, String> {
    read_with(path, keep, None)
}

/// As [`read`], keeping only what reaches into the boxes: a neighbour's extract, read for the strip of it a region's
/// boxes hold. A first pass finds the nodes inside; a way is kept if one of its nodes is, so memory grows with the
/// boxes, not with the neighbour.
pub fn read_within(path: &Path, keep: Keep, boxes: &[[f64; 4]]) -> Result<Vec<Thing>, String> {
    read_with(path, keep, Some(boxes))
}

fn read_with(path: &Path, keep: Keep, within: Option<&[[f64; 4]]>) -> Result<Vec<Thing>, String> {
    let err = |e: osmpbf::Error| format!("{}: {e}", path.display());
    let inside_box = |lon: f64, lat: f64| within.is_none_or(|b| b.iter().any(|x| x[0] <= lon && lon <= x[2] && x[1] <= lat && lat <= x[3]));
    // 0: with boxes, the nodes inside them
    let mut inside: HashSet<i64> = HashSet::new();
    if within.is_some() {
        ElementReader::from_path(path).map_err(err)?.for_each(|e| match e {
            Element::Node(n) if inside_box(n.lon(), n.lat()) => {
                inside.insert(n.id());
            }
            Element::DenseNode(n) if inside_box(n.lon(), n.lat()) => {
                inside.insert(n.id());
            }
            _ => {}
        }).map_err(err)?;
    }
    // 1: relations kept, and the ways they are made of
    let mut rels: Vec<(i64, Tags, Vec<i64>)> = Vec::new();
    ElementReader::from_path(path).map_err(err)?.for_each(|e| {
        if let Element::Relation(r) = e {
            let t: Vec<(&str, &str)> = r.tags().collect();
            if keep(Kind::Relation, &t) {
                let ways = r.members().filter(|m| m.member_type == RelMemberType::Way && matches!(m.role(), Ok("outer") | Ok(""))).map(|m| m.member_id).collect();
                rels.push((r.id(), owned(&t), ways));
            }
        }
    }).map_err(err)?;
    let member_ways: HashSet<i64> = rels.iter().flat_map(|r| r.2.iter().copied()).collect();
    // 2: ways kept, or part of a kept relation, with their nodes
    let mut ways: Vec<(i64, Option<Tags>, Vec<i64>)> = Vec::new();
    ElementReader::from_path(path).map_err(err)?.for_each(|e| {
        if let Element::Way(w) = e {
            let t: Vec<(&str, &str)> = w.tags().collect();
            let kept = keep(Kind::Way, &t);
            if kept || member_ways.contains(&w.id()) {
                let refs: Vec<i64> = w.refs().collect();
                if within.is_none() || refs.iter().any(|r| inside.contains(r)) {
                    ways.push((w.id(), kept.then(|| owned(&t)), refs));
                }
            }
        }
    }).map_err(err)?;
    let wanted: HashSet<i64> = ways.iter().flat_map(|w| w.2.iter().copied()).collect();
    // 3: nodes kept, and the positions the ways need
    let mut at: HashMap<i64, (f64, f64)> = HashMap::with_capacity(wanted.len());
    let mut out: Vec<Thing> = Vec::new();
    {
        let mut node = |id: i64, lat: f64, lon: f64, t: Vec<(&str, &str)>| {
            if wanted.contains(&id) {
                at.insert(id, (lon, lat));
            }
            if !t.is_empty() && inside_box(lon, lat) && keep(Kind::Node, &t) {
                out.push(Thing { kind: Kind::Node, id, tags: owned(&t), rings: Vec::new(), point: (lon, lat) });
            }
        };
        ElementReader::from_path(path).map_err(err)?.for_each(|e| match e {
            Element::Node(n) => node(n.id(), n.lat(), n.lon(), n.tags().collect()),
            Element::DenseNode(n) => node(n.id(), n.lat(), n.lon(), n.tags().collect()),
            _ => {}
        }).map_err(err)?;
    }
    let line = |refs: &[i64]| -> Part { refs.iter().filter_map(|r| at.get(r).copied()).collect() };
    let by_id: HashMap<i64, &Vec<i64>> = ways.iter().map(|w| (w.0, &w.2)).collect();
    for (id, tags, refs) in &ways {
        let Some(tags) = tags else { continue };
        let ring = line(refs);
        let Some(point) = geo::centroid(std::slice::from_ref(&ring)) else { continue };
        out.push(Thing { kind: Kind::Way, id: *id, tags: tags.clone(), rings: vec![ring], point });
    }
    for (id, tags, members) in rels {
        let parts: Vec<Vec<i64>> = members.iter().filter_map(|m| by_id.get(m).map(|r| (*r).clone())).collect();
        let rings: Vec<Part> = join(parts).iter().map(|r| line(r)).filter(|r| r.len() >= 4).collect();
        let Some(point) = geo::centroid(&rings) else { continue };
        out.push(Thing { kind: Kind::Relation, id, tags, rings, point });
    }
    Ok(out)
}

/// Ways joined end to end into closed rings, by their node ids. Pieces that close nothing are left out.
pub fn join(mut ways: Vec<Vec<i64>>) -> Vec<Vec<i64>> {
    let mut rings = Vec::new();
    ways.retain(|w| w.len() >= 2);
    while let Some(mut ring) = ways.pop() {
        while ring.first() != ring.last() {
            let end = *ring.last().unwrap();
            let Some(i) = ways.iter().position(|w| w[0] == end || w[w.len() - 1] == end) else { break };
            let mut next = ways.swap_remove(i);
            if next[0] != end {
                next.reverse();
            }
            ring.extend_from_slice(&next[1..]);
        }
        if ring.len() >= 4 && ring.first() == ring.last() {
            rings.push(ring);
        }
    }
    rings
}

#[cfg(test)]
pub mod write {
    //! A small OSM PBF for tests: one header block, then nodes, ways and relations in plain (not dense) groups.

    fn varint(o: &mut Vec<u8>, mut v: u64) {
        while v >= 0x80 {
            o.push(v as u8 | 0x80);
            v >>= 7;
        }
        o.push(v as u8);
    }

    fn key(o: &mut Vec<u8>, field: u64, wire: u64) {
        varint(o, field << 3 | wire);
    }

    fn zz(v: i64) -> u64 {
        ((v << 1) ^ (v >> 63)) as u64
    }

    fn bytes(o: &mut Vec<u8>, field: u64, b: &[u8]) {
        key(o, field, 2);
        varint(o, b.len() as u64);
        o.extend_from_slice(b);
    }

    fn packed(o: &mut Vec<u8>, field: u64, vals: impl IntoIterator<Item = u64>) {
        let mut b = Vec::new();
        vals.into_iter().for_each(|v| varint(&mut b, v));
        bytes(o, field, &b);
    }

    pub enum El<'a> {
        Node(i64, f64, f64, &'a [(&'a str, &'a str)]),
        Way(i64, &'a [i64], &'a [(&'a str, &'a str)]),
        /// (id, members as (way id, role), tags)
        Rel(i64, &'a [(i64, &'a str)], &'a [(&'a str, &'a str)]),
    }

    fn blob(o: &mut Vec<u8>, kind: &str, data: &[u8]) {
        let mut b = Vec::new();
        bytes(&mut b, 1, data); // raw
        let mut h = Vec::new();
        bytes(&mut h, 1, kind.as_bytes());
        key(&mut h, 3, 0);
        varint(&mut h, b.len() as u64);
        o.extend((h.len() as u32).to_be_bytes());
        o.extend(h);
        o.extend(b);
    }

    pub fn pbf(elements: &[El]) -> Vec<u8> {
        let mut strings: Vec<String> = vec![String::new()];
        let mut sid = |s: &str| -> u64 {
            match strings.iter().position(|x| x == s) {
                Some(i) => i as u64,
                None => {
                    strings.push(s.to_string());
                    (strings.len() - 1) as u64
                }
            }
        };
        let (mut nodes, mut ways, mut rels) = (Vec::new(), Vec::new(), Vec::new());
        for e in elements {
            let mut m = Vec::new();
            match e {
                El::Node(id, lat, lon, tags) => {
                    key(&mut m, 1, 0);
                    varint(&mut m, zz(*id));
                    packed(&mut m, 2, tags.iter().map(|t| sid(t.0)).collect::<Vec<_>>());
                    packed(&mut m, 3, tags.iter().map(|t| sid(t.1)).collect::<Vec<_>>());
                    key(&mut m, 8, 0);
                    varint(&mut m, zz((lat * 1e7).round() as i64));
                    key(&mut m, 9, 0);
                    varint(&mut m, zz((lon * 1e7).round() as i64));
                    bytes(&mut nodes, 1, &m);
                }
                El::Way(id, refs, tags) => {
                    key(&mut m, 1, 0);
                    varint(&mut m, *id as u64);
                    packed(&mut m, 2, tags.iter().map(|t| sid(t.0)).collect::<Vec<_>>());
                    packed(&mut m, 3, tags.iter().map(|t| sid(t.1)).collect::<Vec<_>>());
                    let mut last = 0;
                    packed(&mut m, 8, refs.iter().map(|r| {
                        let d = r - last;
                        last = *r;
                        zz(d)
                    }).collect::<Vec<_>>());
                    bytes(&mut ways, 3, &m);
                }
                El::Rel(id, members, tags) => {
                    key(&mut m, 1, 0);
                    varint(&mut m, *id as u64);
                    packed(&mut m, 2, tags.iter().map(|t| sid(t.0)).collect::<Vec<_>>());
                    packed(&mut m, 3, tags.iter().map(|t| sid(t.1)).collect::<Vec<_>>());
                    packed(&mut m, 8, members.iter().map(|x| sid(x.1)).collect::<Vec<_>>());
                    let mut last = 0;
                    packed(&mut m, 9, members.iter().map(|x| {
                        let d = x.0 - last;
                        last = x.0;
                        zz(d)
                    }).collect::<Vec<_>>());
                    packed(&mut m, 10, members.iter().map(|_| 1).collect::<Vec<_>>()); // ways
                    bytes(&mut rels, 4, &m);
                }
            }
        }
        let mut st = Vec::new();
        for s in &strings {
            bytes(&mut st, 1, s.as_bytes());
        }
        let mut block = Vec::new();
        bytes(&mut block, 1, &st);
        for g in [nodes, ways, rels] {
            if !g.is_empty() {
                bytes(&mut block, 2, &g);
            }
        }
        let mut header = Vec::new();
        bytes(&mut header, 4, b"OsmSchema-V0.6");
        let mut out = Vec::new();
        blob(&mut out, "OSMHeader", &header);
        blob(&mut out, "OSMData", &block);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::write::*;
    use super::*;

    #[test]
    fn nodes_ways_and_multipolygons() {
        let dir = std::env::temp_dir().join(format!("lb-tools-osm-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.osm.pbf");
        let sq = |id0: i64, x: f64, y: f64| {
            vec![El::Node(id0, y, x, &[]), El::Node(id0 + 1, y, x + 0.001, &[]), El::Node(id0 + 2, y + 0.001, x + 0.001, &[]), El::Node(id0 + 3, y + 0.001, x, &[])]
        };
        let mut els = vec![El::Node(1, 47.14, 9.52, &[("addr:housenumber", "5"), ("addr:street", "Städtle")]), El::Node(2, 47.0, 9.0, &[("highway", "crossing")])];
        els.extend(sq(10, 9.50, 47.10));
        els.extend(sq(20, 9.60, 47.20));
        els.push(El::Way(100, &[10, 11, 12, 13, 10], &[("building", "yes"), ("addr:housenumber", "7"), ("addr:street", "Äulestrasse")]));
        els.push(El::Way(101, &[20, 21, 22], &[]));
        els.push(El::Way(102, &[22, 23, 20], &[]));
        els.push(El::Way(103, &[10, 11], &[("highway", "path")]));
        els.push(El::Rel(200, &[(101, "outer"), (102, "outer")], &[("type", "multipolygon"), ("building", "yes"), ("height", "12 m")]));
        std::fs::write(&path, pbf(&els)).unwrap();
        let keep = |_: Kind, t: &[(&str, &str)]| t.iter().any(|(k, _)| *k == "building" || *k == "addr:housenumber");
        let mut got = read(&path, &keep).unwrap();
        got.sort_by_key(|t| t.id);
        assert_eq!(got.iter().map(|t| (t.kind, t.id)).collect::<Vec<_>>(), vec![(Kind::Node, 1), (Kind::Way, 100), (Kind::Relation, 200)]);
        assert_eq!(got[0].tags.get("addr:street"), Some("Städtle"));
        assert!((got[0].point.0 - 9.52).abs() < 1e-7 && (got[0].point.1 - 47.14).abs() < 1e-7);
        assert_eq!(got[1].rings[0].len(), 5);
        assert!((got[1].point.0 - 9.5005).abs() < 1e-7 && (got[1].point.1 - 47.1005).abs() < 1e-7, "the middle of the building");
        assert_eq!(got[2].rings.len(), 1, "two member ways joined into one ring");
        assert_eq!(got[2].rings[0].len(), 5);
        assert_eq!(got[2].tags.get("height"), Some("12 m"));
        assert!((got[2].point.0 - 9.6005).abs() < 1e-7);
        std::fs::remove_dir_all(&dir).unwrap();
        let rings = join(vec![vec![1, 2], vec![3, 2], vec![3, 1], vec![7, 8]]);
        assert_eq!(rings.len(), 1, "reversed pieces fit, loose ones go");
        assert_eq!((rings[0].len(), rings[0][0] == rings[0][3]), (4, true));
    }
}
