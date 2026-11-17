//! Encyclopedias: ZIM files in `library-data/collections/`, read here (openzim.org's ZIM format: a header, the
//! entries sorted by path and by title, and clusters of blobs, stored or compressed with xz or zstd). `/zim/<file>/<path>`
//! serves an entry as the original Lastbook did with libzim (engine/library.py), with its scripts taken out
//! (saved_pages.rs article) and the app's look added. `/api/collection-search` matches article titles, not their text:
//! the files' full-text indexes are Xapian databases, which nothing here reads.

use super::api::{json, json_status};
use super::files::{self, quote, refresh, ZIM_CSP};
use super::saved_pages;
use super::words::{query_words, Matcher};
use super::Library;
use crate::http::{Request, Response};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

const MAGIC: u32 = 72173914;
const NO_PAGE: u32 = 0xffff_ffff;
const REDIRECT: u16 = 0xffff;
/// Redirects followed before giving up (a loop).
const HOPS: usize = 8;
/// The most bytes a compressed cluster may grow to.
const CLUSTER_MAX: u64 = 512 * 1024 * 1024;
const CLUSTERS_KEPT: usize = 8;
/// Articles found per file, as libzim's search gave them.
const HITS_PER_FILE: usize = 8;

#[derive(Debug, Clone)]
pub enum Target {
    Blob { cluster: u32, blob: u32 },
    Redirect(u32),
    /// A link target or a deleted entry: nothing to read.
    Nothing,
}

/// One directory entry.
#[derive(Debug, Clone)]
pub struct Dirent {
    pub mime: u16,
    pub ns: u8,
    pub target: Target,
    pub path: String,
    pub title: String,
}

/// An open ZIM file.
pub struct Zim {
    file: File,
    size: u64,
    entries: u32,
    clusters: u32,
    path_ptr: u64,
    title_ptr: u64,
    cluster_ptr: u64,
    main_page: u32,
    checksum: u64,
    mimes: Vec<String>,
    /// Version 6.1 and later keep articles in `C`, with paths that do not name it.
    new_ns: bool,
    /// Decompressed clusters, the last few used.
    kept: Mutex<Vec<(u32, Arc<Vec<u8>>)>>,
    /// Entry numbers in title order: the front articles' listing, else the header's list of every entry.
    titles: std::sync::OnceLock<Option<Vec<u32>>>,
}

fn u16_at(b: &[u8], i: usize) -> u16 {
    u16::from_le_bytes([b[i], b[i + 1]])
}
fn u32_at(b: &[u8], i: usize) -> u32 {
    u32::from_le_bytes(b[i..i + 4].try_into().unwrap())
}
fn u64_at(b: &[u8], i: usize) -> u64 {
    u64::from_le_bytes(b[i..i + 8].try_into().unwrap())
}

fn bad(why: &str) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, why.to_string())
}

/// Bytes from a position in the file, without moving a shared cursor.
fn read_at(f: &File, off: u64, buf: &mut [u8]) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::FileExt;
        f.read_exact_at(buf, off)
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::FileExt;
        let mut done = 0;
        while done < buf.len() {
            let n = f.seek_read(&mut buf[done..], off + done as u64)?;
            if n == 0 {
                return Err(std::io::ErrorKind::UnexpectedEof.into());
            }
            done += n;
        }
        Ok(())
    }
}

/// A path written with its namespace, `A/Foo` or `/A/Foo`, as (namespace, path).
fn long_path(p: &str) -> Option<(u8, &str)> {
    let b = p.as_bytes();
    let i = usize::from(b.first() == Some(&b'/'));
    if b.len() < i + 1 || b[i] == b'/' || (b.len() > i + 1 && b[i + 1] != b'/') {
        return None;
    }
    Some((b[i], p.get((i + 2).min(p.len())..).unwrap_or("")))
}

impl Zim {
    pub fn open(path: &Path) -> std::io::Result<Zim> {
        let file = File::open(path)?;
        let size = file.metadata()?.len();
        let mut h = [0u8; 80];
        read_at(&file, 0, &mut h)?;
        if u32_at(&h, 0) != MAGIC {
            return Err(bad("not a ZIM file"));
        }
        let (major, minor) = (u16_at(&h, 4), u16_at(&h, 6));
        let mut z = Zim {
            size,
            entries: u32_at(&h, 24),
            clusters: u32_at(&h, 28),
            path_ptr: u64_at(&h, 32),
            title_ptr: u64_at(&h, 40),
            cluster_ptr: u64_at(&h, 48),
            main_page: u32_at(&h, 64),
            checksum: u64_at(&h, 72),
            mimes: Vec::new(),
            new_ns: major >= 6 && minor >= 1,
            kept: Mutex::new(Vec::new()),
            titles: std::sync::OnceLock::new(),
            file,
        };
        let mime_pos = u64_at(&h, 56);
        let room = z.path_ptr.saturating_sub(mime_pos).min(1 << 20) as usize;
        if mime_pos >= size || room == 0 || z.path_ptr + 8 * z.entries as u64 > size || z.cluster_ptr + 8 * z.clusters as u64 > size {
            return Err(bad("damaged ZIM header"));
        }
        let mut list = vec![0u8; room];
        read_at(&z.file, mime_pos, &mut list)?;
        for m in list.split(|&b| b == 0) {
            if m.is_empty() {
                break;
            }
            z.mimes.push(String::from_utf8_lossy(m).into_owned());
        }
        Ok(z)
    }

    fn ptr(&self, base: u64, i: u64) -> std::io::Result<u64> {
        let mut b = [0u8; 8];
        read_at(&self.file, base + 8 * i, &mut b)?;
        Ok(u64::from_le_bytes(b))
    }

    /// The entry with this number, in path order.
    pub fn dirent(&self, i: u32) -> std::io::Result<Dirent> {
        if i >= self.entries {
            return Err(bad("no such entry"));
        }
        let at = self.ptr(self.path_ptr, i as u64)?;
        let mut want = 512usize;
        loop {
            let n = want.min(self.size.saturating_sub(at) as usize);
            let mut b = vec![0u8; n];
            read_at(&self.file, at, &mut b)?;
            if b.len() < 12 {
                return Err(bad("damaged entry"));
            }
            let mime = u16_at(&b, 0);
            let ns = b[3];
            let (target, start) = match mime {
                REDIRECT => (Target::Redirect(u32_at(&b, 8)), 12),
                0xfffe | 0xfffd => (Target::Nothing, 8),
                _ if b.len() >= 16 => (Target::Blob { cluster: u32_at(&b, 8), blob: u32_at(&b, 12) }, 16),
                _ => return Err(bad("damaged entry")),
            };
            let mut parts = b[start..].splitn(3, |&c| c == 0);
            let (p, t) = (parts.next(), parts.next());
            if parts.next().is_some() {
                let path = String::from_utf8_lossy(p.unwrap_or_default()).into_owned();
                let title = String::from_utf8_lossy(t.unwrap_or_default()).into_owned();
                let title = if title.is_empty() { path.clone() } else { title };
                return Ok(Dirent { mime, ns, target, path, title });
            }
            if n < want || want >= 64 * 1024 {
                return Err(bad("damaged entry"));
            }
            want *= 8;
        }
    }

    /// The entry with this namespace and path.
    pub fn find(&self, ns: u8, path: &str) -> Option<u32> {
        let key = (ns, path.as_bytes());
        let (mut lo, mut hi) = (0u32, self.entries);
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            let d = self.dirent(mid).ok()?;
            match (d.ns, d.path.as_bytes()).cmp(&key) {
                std::cmp::Ordering::Less => lo = mid + 1,
                std::cmp::Ordering::Greater => hi = mid,
                std::cmp::Ordering::Equal => return Some(mid),
            }
        }
        None
    }

    /// An entry as libzim's getEntryByPath finds it: in `C` (a path that names its namespace is read as one in `C`) for
    /// a newer file; by its namespace, else in `A`, for an older one.
    pub fn entry_by_path(&self, path: &str) -> Option<u32> {
        if self.new_ns {
            self.find(b'C', path).or_else(|| long_path(path).and_then(|(_, p)| self.find(b'C', p)))
        } else {
            long_path(path).and_then(|(ns, p)| self.find(ns, p)).or_else(|| self.find(b'A', path))
        }
    }

    /// The entry's address as libzim gives it: without its namespace in a newer file, with it in an older one.
    pub fn path_of(&self, d: &Dirent) -> String {
        if self.new_ns {
            d.path.clone()
        } else {
            format!("{}/{}", d.ns as char, d.path)
        }
    }

    /// Where a redirect leads, a few hops at most.
    pub fn follow(&self, mut i: u32) -> Option<(u32, Dirent)> {
        for _ in 0..HOPS {
            let d = self.dirent(i).ok()?;
            match d.target {
                Target::Redirect(to) => i = to,
                _ => return Some((i, d)),
            }
        }
        None
    }

    /// The front page's address (libzim getMainEntry, its redirects followed).
    pub fn main_path(&self) -> Option<String> {
        let start = self.find(b'W', "mainPage").or(Some(self.main_page).filter(|p| *p != NO_PAGE))?;
        let (_, d) = self.follow(start)?;
        Some(self.path_of(&d))
    }

    fn cluster(&self, c: u32) -> std::io::Result<Arc<Vec<u8>>> {
        if let Some((_, data)) = self.kept.lock().unwrap_or_else(|e| e.into_inner()).iter().find(|(k, _)| *k == c) {
            return Ok(data.clone());
        }
        if c >= self.clusters {
            return Err(bad("no such cluster"));
        }
        let start = self.ptr(self.cluster_ptr, c as u64)?;
        let next = if c + 1 < self.clusters { self.ptr(self.cluster_ptr, c as u64 + 1)? } else { self.checksum };
        let end = if next > start && next <= self.size { next } else { self.size };
        let mut raw = vec![0u8; (end - start) as usize];
        read_at(&self.file, start, &mut raw)?;
        let (info, body) = raw.split_first().ok_or_else(|| bad("empty cluster"))?;
        let mut data = Vec::new();
        match info & 0x0f {
            0 | 1 => data = body.to_vec(),
            4 => lzma_rs::xz_decompress(&mut std::io::BufReader::new(body), &mut Limited(&mut data)).map_err(|_| bad("damaged xz cluster"))?,
            5 => {
                let dec = ruzstd::decoding::StreamingDecoder::new_with_max_window_size(body, 1 << 27).map_err(|_| bad("damaged zstd cluster"))?;
                dec.take(CLUSTER_MAX).read_to_end(&mut data)?;
            }
            _ => return Err(bad("a cluster compressed with zlib or bzip2, which ZIM no longer uses")),
        }
        // the extended flag (8-byte offsets) is kept in front of the data
        let mut out = Vec::with_capacity(data.len() + 1);
        out.push(info & 0x10);
        out.extend_from_slice(&data);
        let out = Arc::new(out);
        let mut kept = self.kept.lock().unwrap_or_else(|e| e.into_inner());
        if kept.len() >= CLUSTERS_KEPT {
            kept.remove(0);
        }
        kept.push((c, out.clone()));
        Ok(out)
    }

    /// One blob's bytes.
    pub fn blob(&self, cluster: u32, blob: u32) -> std::io::Result<Vec<u8>> {
        let data = self.cluster(cluster)?;
        let (wide, data) = (data[0] != 0, &data[1..]);
        let w = if wide { 8 } else { 4 };
        let at = |i: usize| -> Option<usize> {
            let p = i * w;
            let b = data.get(p..p + w)?;
            Some(if wide { u64_at(b, 0) as usize } else { u32_at(b, 0) as usize })
        };
        let first = at(0).ok_or_else(|| bad("damaged cluster"))?;
        let count = first / w;
        let b = blob as usize;
        if b + 1 >= count {
            return Err(bad("no such blob"));
        }
        let (s, e) = (at(b).ok_or_else(|| bad("damaged cluster"))?, at(b + 1).ok_or_else(|| bad("damaged cluster"))?);
        data.get(s..e).map(<[u8]>::to_vec).ok_or_else(|| bad("damaged cluster"))
    }

    /// An entry's content and type, its redirects followed.
    pub fn content(&self, i: u32) -> Option<(Vec<u8>, String)> {
        let (_, d) = self.follow(i)?;
        let Target::Blob { cluster, blob } = d.target else { return None };
        let mime = self.mimes.get(d.mime as usize).cloned().unwrap_or_else(|| "application/octet-stream".into());
        Some((self.blob(cluster, blob).ok()?, mime))
    }

    /// Entry numbers in title order: the newer files' list of front articles, else every entry.
    fn titles(&self) -> Option<&Vec<u32>> {
        self.titles
            .get_or_init(|| {
                if let Some((data, _)) = self.find(b'X', "listing/titleOrdered/v1").and_then(|i| self.content(i)) {
                    return Some(data.chunks_exact(4).map(|c| u32::from_le_bytes(c.try_into().unwrap())).collect());
                }
                if self.title_ptr == 0 || self.title_ptr + 4 * self.entries as u64 > self.size {
                    return None;
                }
                let mut b = vec![0u8; 4 * self.entries as usize];
                read_at(&self.file, self.title_ptr, &mut b).ok()?;
                Some(b.chunks_exact(4).map(|c| u32::from_le_bytes(c.try_into().unwrap())).collect())
            })
            .as_ref()
    }

    /// Articles whose title starts with `prefix` (as written), at most `max`.
    fn titles_starting(&self, prefix: &str, max: usize) -> Vec<(u32, Dirent)> {
        let Some(list) = self.titles() else { return Vec::new() };
        let ns = if self.new_ns { b'C' } else { b'A' };
        let key = (ns, prefix.as_bytes());
        let at = |k: usize| list.get(k).and_then(|i| self.dirent(*i).ok().map(|d| (*i, d)));
        let (mut lo, mut hi) = (0usize, list.len());
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            match at(mid) {
                Some((_, d)) if (d.ns, d.title.as_bytes()) < key => lo = mid + 1,
                Some(_) => hi = mid,
                None => return Vec::new(),
            }
        }
        let mut out = Vec::new();
        let mut k = lo;
        while out.len() < max {
            let Some((i, d)) = at(k) else { break };
            if d.ns != ns || !d.title.as_bytes().starts_with(prefix.as_bytes()) {
                break;
            }
            out.push((i, d));
            k += 1;
        }
        out
    }

    /// Articles for a query, by their titles: titles starting with the query or one of its words (as typed, capitalised
    /// or in capitals), those holding more of the words first.
    pub fn search_titles(&self, q: &str, words: &[String]) -> Vec<(String, String)> {
        let mut seeds: Vec<String> = Vec::new();
        let cap = |s: &str| s.chars().next().map(|c| c.to_uppercase().collect::<String>() + &s[c.len_utf8()..]).unwrap_or_default();
        let q = q.trim();
        for s in std::iter::once(q.to_string()).chain(words.iter().cloned()) {
            for v in [s.clone(), s.to_lowercase(), cap(&s.to_lowercase()), s.split(' ').map(cap).collect::<Vec<_>>().join(" "), s.to_uppercase()] {
                if !v.is_empty() && !seeds.contains(&v) {
                    seeds.push(v);
                }
            }
        }
        let mut seen = HashSet::new();
        let mut found: Vec<(usize, bool, usize, String, String)> = Vec::new();
        for seed in &seeds {
            for (i, d) in self.titles_starting(seed, 50) {
                let Some((to, _)) = self.follow(i) else { continue };
                if !seen.insert(to) {
                    continue;
                }
                let m = Matcher::new(&d.title);
                let score = words.iter().filter(|w| m.has(w)).count();
                if score == 0 && !words.is_empty() {
                    continue;
                }
                let whole = d.title.to_lowercase().starts_with(&q.to_lowercase());
                found.push((score, whole, d.title.chars().count(), d.title.clone(), self.path_of(&d)));
            }
        }
        found.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)).then(a.2.cmp(&b.2)));
        found.into_iter().take(HITS_PER_FILE).map(|f| (f.3, f.4)).collect()
    }
}

/// Turns a too-large cluster into an error instead of filling memory.
struct Limited<'a>(&'a mut Vec<u8>);

impl std::io::Write for Limited<'_> {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        if self.0.len() as u64 + b.len() as u64 > CLUSTER_MAX {
            return Err(bad("cluster too large"));
        }
        self.0.extend_from_slice(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Open ZIM files, by path and modified time.
pub(super) type Open = Mutex<Vec<(PathBuf, Option<SystemTime>, Arc<Zim>)>>;

impl Library {
    /// The file behind a `/zim/<name>/` address, after the same checks as any other file.
    pub fn zim_file(&self, name: &str) -> Option<PathBuf> {
        if name.is_empty() || name.contains(['\0', '/', '\\']) || !files::segments_ok(name) {
            return None;
        }
        let lib = self.lib()?;
        let dir = lib.join("collections").canonicalize().ok()?;
        let path = dir.join(name);
        let real = path.canonicalize().ok()?;
        let real_name = real.file_name()?.to_string_lossy().into_owned();
        let ok = real.parent() == Some(dir.as_path()) && real.is_file() && real_name.to_lowercase().ends_with(".zim") && !files::name_refused(&[real_name]) && files::segments_ok(&real.to_string_lossy());
        ok.then_some(real)
    }

    fn zim(&self, path: &Path) -> Option<Arc<Zim>> {
        let modified = std::fs::metadata(path).ok()?.modified().ok();
        let mut open = self.zims.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((_, _, z)) = open.iter().find(|(p, m, _)| p == path && *m == modified) {
            return Some(z.clone());
        }
        let z = Arc::new(Zim::open(path).ok()?);
        open.retain(|(p, _, _)| p != path);
        open.push((path.to_path_buf(), modified, z.clone()));
        Some(z)
    }

    /// `/zim/<file>/<entry path>`: the entry, or the front page for the bare address. Every answer under `/zim/` has
    /// the encyclopedia's policy, pages and errors alike.
    pub(super) fn serve_zim(&self, req: &Request) -> Response {
        let r = self.zim_answer(req);
        let r = r.with("X-Content-Type-Options", "nosniff");
        if r.header("Content-Security-Policy").is_some() {
            return r;
        }
        r.with("Content-Security-Policy", &ZIM_CSP).with("X-DNS-Prefetch-Control", "off")
    }

    fn zim_answer(&self, req: &Request) -> Response {
        let not_found = || Response::new(404, "text/plain; charset=utf-8", "Not found"); // libzim's own messages carry the full file path
        let rest = req.path.strip_prefix("/zim/").unwrap_or("");
        let (name, entry) = rest.split_once('/').unwrap_or((rest, ""));
        let Some(zim) = self.zim_file(name).and_then(|p| self.zim(&p)) else { return not_found() };
        if entry.is_empty() {
            return match zim.main_path() {
                Some(main) => refresh(&format!("/zim/{}/{}", quote(name), quote(&main))),
                None => not_found(),
            };
        }
        let Some((data, mime)) = zim.entry_by_path(entry).and_then(|i| zim.content(i)) else { return not_found() };
        if mime.to_lowercase().contains("pdf") {
            return Response::new(404, "text/html; charset=utf-8", files::PDF_NOTICE.to_vec()); // a PDF never reaches the webview
        }
        if data.len() as u64 > files::WHOLE_MAX {
            return Response::new(413, "text/html; charset=utf-8", files::TOO_LARGE.to_vec());
        }
        let low = mime.to_lowercase();
        let data = if low.contains("html") || low.contains("svg") {
            let clean = saved_pages::article(&data);
            if low.contains("html") {
                saved_pages::article_look(&clean, saved_pages::ARTICLE_LOOK)
            } else {
                clean
            }
        } else {
            data
        };
        Response::new(200, &mime, data)
    }

    /// `/api/collection-search`: articles in every encyclopedia whose title matches the words, each as the address a
    /// guide's `zim` field takes.
    pub(super) fn collection_search(&self, q: &str) -> Response {
        let stop = self.words.stop_words("en");
        let words = query_words(q, &stop);
        let (mut hits, mut tried, mut failed) = (Vec::new(), 0, 0);
        for name in self.zim_names() {
            let Some(path) = self.zim_file(&name) else { continue };
            tried += 1;
            let Some(z) = self.zim(&path) else {
                failed += 1; // one damaged file must not cost the answers from the others
                eprintln!("lastbook: collection search skipped {name}");
                continue;
            };
            let stem = name.strip_suffix(".zim").unwrap_or(&name).to_string();
            for (title, p) in z.search_titles(q, &words) {
                hits.push(json!({"title": title, "collection": stem, "path": format!("zim/{}/{}", quote(&name), quote(&p))}));
            }
        }
        if tried > 0 && failed == tried {
            return json_status(500, &json!({"error": "Encyclopedia search failed."}));
        }
        json(&Value::Array(hits))
    }
}
