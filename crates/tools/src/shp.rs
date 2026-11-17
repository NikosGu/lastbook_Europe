//! Reads an ESRI shapefile from its zip, as Natural Earth publishes them: the shapes of the `.shp` (points, lines,
//! polygon rings) and the attributes of the `.dbf`, as text.

use crate::geo::Part;
use std::collections::HashMap;
use std::io::Read;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Point,
    Line,
    Polygon,
}

#[derive(Debug, Clone)]
pub struct Shape {
    pub kind: Kind,
    pub parts: Vec<Part>,
    pub attrs: HashMap<String, String>,
}

impl Shape {
    pub fn get(&self, k: &str) -> Option<&str> {
        self.attrs.get(k).map(String::as_str).filter(|v| !v.is_empty())
    }

    pub fn num(&self, k: &str) -> Option<f64> {
        self.get(k)?.parse().ok()
    }
}

pub fn read_zip(path: &Path) -> Result<Vec<Shape>, String> {
    let e = |x: String| format!("{}: {x}", path.display());
    let mut z = zip::ZipArchive::new(std::fs::File::open(path).map_err(|x| e(x.to_string()))?).map_err(|x| e(x.to_string()))?;
    let names: Vec<String> = z.file_names_raw().map(|n| String::from_utf8_lossy(n).into_owned()).collect();
    let mut member = |ext: &str| -> Result<Vec<u8>, String> {
        let name = names.iter().find(|n| n.to_lowercase().ends_with(ext)).ok_or_else(|| e(format!("no {ext} in the zip")))?;
        let mut b = Vec::new();
        z.by_name(name).map_err(|x| e(x.to_string()))?.read_to_end(&mut b).map_err(|x| e(x.to_string()))?;
        Ok(b)
    };
    let shp = member(".shp")?;
    let dbf = member(".dbf")?;
    let shapes = shapes(&shp);
    let attrs = attributes(&dbf);
    if shapes.len() != attrs.len() {
        return Err(e(format!("{} shapes and {} records", shapes.len(), attrs.len())));
    }
    Ok(shapes.into_iter().zip(attrs).filter_map(|(s, attrs)| s.map(|(kind, parts)| Shape { kind, parts, attrs })).collect())
}

fn i32_le(b: &[u8], o: usize) -> Option<i32> {
    Some(i32::from_le_bytes(b.get(o..o + 4)?.try_into().ok()?))
}

fn f64_le(b: &[u8], o: usize) -> Option<f64> {
    Some(f64::from_le_bytes(b.get(o..o + 8)?.try_into().ok()?))
}

/// One entry per record; None for a null or unread shape, so records stay in step with the attributes.
pub fn shapes(b: &[u8]) -> Vec<Option<(Kind, Vec<Part>)>> {
    let mut out = Vec::new();
    let mut o = 100;
    while o + 8 <= b.len() {
        let len = u32::from_be_bytes(b[o + 4..o + 8].try_into().unwrap()) as usize * 2;
        let c = o + 8;
        o = c + len;
        let shape = (|| {
            let t = i32_le(b, c)?;
            match t {
                1 | 11 | 21 => Some((Kind::Point, vec![vec![(f64_le(b, c + 4)?, f64_le(b, c + 12)?)]])),
                3 | 5 | 13 | 15 | 23 | 25 => {
                    let (np, n) = (i32_le(b, c + 36)? as usize, i32_le(b, c + 40)? as usize);
                    let starts: Vec<usize> = (0..np).map(|i| i32_le(b, c + 44 + 4 * i).map(|v| v as usize)).collect::<Option<_>>()?;
                    let p0 = c + 44 + 4 * np;
                    let mut parts = Vec::with_capacity(np);
                    for (i, &s) in starts.iter().enumerate() {
                        let e = starts.get(i + 1).copied().unwrap_or(n).min(n);
                        parts.push((s..e).map(|k| Some((f64_le(b, p0 + 16 * k)?, f64_le(b, p0 + 16 * k + 8)?))).collect::<Option<Part>>()?);
                    }
                    Some((if t % 10 == 3 { Kind::Line } else { Kind::Polygon }, parts))
                }
                _ => None,
            }
        })();
        out.push(shape);
    }
    out
}

/// The records of a dBASE table, field name to trimmed text; deleted records are kept (as empty) to stay in step.
pub fn attributes(b: &[u8]) -> Vec<HashMap<String, String>> {
    if b.len() < 32 {
        return Vec::new();
    }
    let n = u32::from_le_bytes(b[4..8].try_into().unwrap()) as usize;
    let head = u16::from_le_bytes(b[8..10].try_into().unwrap()) as usize;
    let size = u16::from_le_bytes(b[10..12].try_into().unwrap()) as usize;
    let mut fields = Vec::new();
    let mut o = 32;
    while o + 32 <= b.len() && b[o] != 0x0d {
        let name = String::from_utf8_lossy(&b[o..o + 11]).trim_end_matches('\0').to_string();
        fields.push((name, b[o + 16] as usize));
        o += 32;
    }
    (0..n)
        .map(|i| {
            let r = head + i * size;
            let mut m = HashMap::new();
            let mut p = r + 1;
            for (name, len) in &fields {
                if let Some(v) = b.get(p..p + len) {
                    m.insert(name.clone(), String::from_utf8_lossy(v).trim().to_string());
                }
                p += len;
            }
            m
        })
        .collect()
}

#[cfg(test)]
pub mod write {
    //! A shapefile zip for tests: lines, polygons or points with text fields.
    use super::*;
    use std::io::Write;

    pub fn zip(path: &Path, kind: Kind, rows: &[(Vec<Part>, Vec<(&str, &str)>)]) {
        let t: i32 = match kind {
            Kind::Point => 1,
            Kind::Line => 3,
            Kind::Polygon => 5,
        };
        let mut shp = vec![0u8; 100];
        for (i, (parts, _)) in rows.iter().enumerate() {
            let mut c = t.to_le_bytes().to_vec();
            if kind == Kind::Point {
                c.extend(parts[0][0].0.to_le_bytes());
                c.extend(parts[0][0].1.to_le_bytes());
            } else {
                c.extend([0u8; 32]);
                c.extend((parts.len() as i32).to_le_bytes());
                c.extend((parts.iter().map(|p| p.len()).sum::<usize>() as i32).to_le_bytes());
                let mut s = 0;
                for p in parts {
                    c.extend((s as i32).to_le_bytes());
                    s += p.len();
                }
                for p in parts.iter().flatten() {
                    c.extend(p.0.to_le_bytes());
                    c.extend(p.1.to_le_bytes());
                }
            }
            shp.extend(((i + 1) as u32).to_be_bytes());
            shp.extend(((c.len() / 2) as u32).to_be_bytes());
            shp.extend(c);
        }
        let names: Vec<&str> = rows.first().map(|r| r.1.iter().map(|f| f.0).collect()).unwrap_or_default();
        let width = 40usize;
        let mut dbf = vec![3u8, 26, 1, 1];
        dbf.extend((rows.len() as u32).to_le_bytes());
        dbf.extend(((32 + 32 * names.len() + 1) as u16).to_le_bytes());
        dbf.extend(((1 + width * names.len()) as u16).to_le_bytes());
        dbf.extend([0u8; 20]);
        for n in &names {
            let mut f = [0u8; 32];
            f[..n.len()].copy_from_slice(n.as_bytes());
            f[11] = b'C';
            f[16] = width as u8;
            dbf.extend(f);
        }
        dbf.push(0x0d);
        for (_, vals) in rows {
            dbf.push(b' ');
            for (_, v) in vals {
                let mut f = vec![b' '; width];
                f[..v.len()].copy_from_slice(v.as_bytes());
                dbf.extend(f);
            }
        }
        let mut z = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
        let o = zip::write::SimpleFileOptions::default();
        for (n, b) in [("x.shp", &shp), ("x.dbf", &dbf)] {
            z.start_file(n, o).unwrap();
            z.write_all(b).unwrap();
        }
        z.finish().unwrap();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shapes_and_attributes_in_step() {
        let dir = std::env::temp_dir().join(format!("lb-tools-shp-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("t.zip");
        write::zip(&p, Kind::Line, &[(vec![vec![(9.0, 47.0), (10.0, 48.0)], vec![(1.0, 2.0), (3.0, 4.0), (5.0, 6.0)]], vec![("type", "Major Highway"), ("min_zoom", "5")]),
                                     (vec![vec![(0.0, 0.0), (1.0, 1.0)]], vec![("type", "Road"), ("min_zoom", "7.5")])]);
        let s = read_zip(&p).unwrap();
        assert_eq!(s.len(), 2);
        assert_eq!((s[0].kind, s[0].parts.len(), s[0].parts[1].len()), (Kind::Line, 2, 3));
        assert_eq!((s[0].get("type"), s[1].num("min_zoom")), (Some("Major Highway"), Some(7.5)));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
