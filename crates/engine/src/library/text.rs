//! The words of a document page by page, and a video's transcript as timed cues. Ported from the original Lastbook's
//! server.py (page_texts, vtt_cues, transcript).

use super::catalog::{inside, Record};
use super::Library;
use regex::Regex;
use serde_json::{json, Value};
use std::path::Path;
use std::sync::LazyLock;

static CUE_LONG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(\d+):(\d\d):(\d\d)[.,](\d+)\s+-->").unwrap());
static CUE_SHORT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(\d\d):(\d\d)[.,](\d+)\s+-->").unwrap());
static TAG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<[^>]+>").unwrap());

/// A file's text, with bytes that are not UTF-8 replaced.
fn read_text(p: &Path) -> Option<String> {
    std::fs::read(p).ok().map(|b| String::from_utf8_lossy(&b).into_owned())
}

/// HTML entities in caption text, the ones captions use, decoded as the original Lastbook did.
pub fn unescape(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let end = rest[1..].find(|c: char| !(c.is_ascii_alphanumeric() || c == '#')).map(|e| e + 1).unwrap_or(rest.len());
        let name = &rest[1..end];
        let semi = rest[end..].starts_with(';');
        let ch = if let Some(num) = name.strip_prefix('#') {
            let n = if let Some(h) = num.strip_prefix(['x', 'X']) { u32::from_str_radix(h, 16).ok() } else { num.parse::<u32>().ok() };
            n.map(|n| char::from_u32(n).filter(|c| *c != '\0').unwrap_or('\u{fffd}'))
        } else {
            match name {
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "quot" => Some('"'),
                "apos" => Some('\''),
                "nbsp" => Some('\u{a0}'),
                "colon" if semi => Some(':'),
                "Tab" if semi => Some('\t'),
                "NewLine" if semi => Some('\n'),
                "sol" if semi => Some('/'),
                "lpar" if semi => Some('('),
                "rpar" if semi => Some(')'),
                "period" if semi => Some('.'),
                "comma" if semi => Some(','),
                _ => None,
            }
        };
        match ch {
            Some(c) if !name.is_empty() => {
                out.push(c);
                rest = &rest[end + semi as usize..];
            }
            _ => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// A WebVTT file as cues `{t, text}`: auto captions repeat lines across cues, so repeats go and cues are merged
/// into chunks of about a sentence.
pub fn vtt_cues(text: &str) -> Vec<Value> {
    let mut cues: Vec<(i64, String)> = Vec::new();
    for line in text.lines() {
        let t = if let Some(m) = CUE_LONG.captures(line) {
            Some(m[1].parse::<i64>().unwrap_or(0) * 3600 + m[2].parse::<i64>().unwrap_or(0) * 60 + m[3].parse::<i64>().unwrap_or(0))
        } else {
            CUE_SHORT.captures(line).map(|m| m[1].parse::<i64>().unwrap_or(0) * 60 + m[2].parse::<i64>().unwrap_or(0))
        };
        if let Some(t) = t {
            cues.push((t, String::new()));
        } else if let Some(cur) = cues.last_mut() {
            let s = line.trim();
            if !s.is_empty() && !s.chars().all(|c| c.is_ascii_digit()) {
                let txt = unescape(&TAG.replace_all(line, "")).trim().to_string();
                if !txt.is_empty() && !cur.1.ends_with(&txt) {
                    cur.1 = format!("{} {}", cur.1, txt).trim().to_string();
                }
            }
        }
    }
    let mut out: Vec<(i64, String)> = Vec::new();
    let mut last = String::new();
    for (t, text) in &cues {
        let mut piece = text.as_str();
        if !last.is_empty() && piece.starts_with(last.as_str()) {
            piece = piece[last.len()..].trim();
        }
        if piece.is_empty() {
            continue;
        }
        last = text.clone();
        match out.last_mut() {
            Some(prev) if t - prev.0 < 12 && prev.1.chars().count() < 220 => {
                prev.1.push(' ');
                prev.1.push_str(piece);
            }
            _ => out.push((*t, piece.to_string())),
        }
    }
    out.into_iter().map(|(t, text)| json!({"t": t, "text": text})).collect()
}

impl Library {
    /// The text of each page of a document, from its text file (`text_path`, else `text/<id>.txt`). None when there is
    /// no text file; pages are split at form feeds.
    pub fn page_texts(&self, r: &Record) -> Vec<String> {
        let Some(lib) = self.lib() else { return Vec::new() };
        let rel = if r.truthy("text_path") { r.str("text_path").to_string() } else { format!("text/{}.txt", r.id) }; // the catalog may be mid-rewrite and miss text_path
        let Some(t) = inside(&lib, &rel).filter(|p| p.is_file()).and_then(|p| read_text(&p)) else { return Vec::new() };
        if t.contains("\n\u{c}\n") {
            t.split("\n\u{c}\n").map(str::to_string).collect()
        } else {
            t.split('\u{c}').map(str::to_string).collect()
        }
    }

    /// A video's transcript: from its WebVTT captions as timed cues, else from its transcript text as untimed
    /// paragraphs. None for an id that is not in the catalog.
    pub fn transcript(&self, video_id: &str) -> Option<Value> {
        let cat = self.catalog();
        let v = cat.get(video_id)?;
        let lib = self.lib()?;
        let tx = cat.transcript_of(&v.path);
        let mut vtts: Vec<String> = Vec::new();
        if let Some(tx) = tx.filter(|t| t.truthy("vtt_path")) {
            vtts.push(tx.str("vtt_path").to_string());
        }
        vtts.extend(v.get("captions").as_array().into_iter().flatten().filter_map(Value::as_str).filter(|c| c.ends_with(".vtt")).map(str::to_string));
        let caption_type = tx.map(|t| t.get("caption_type")).filter(|c| super::words::truthy(c)).cloned();
        let notes = tx.map(|t| t.get("notes").clone()).unwrap_or(Value::Null);
        for p in vtts {
            if let Some(text) = inside(&lib, &p).filter(|f| f.is_file()).and_then(|f| read_text(&f)) {
                return Some(json!({"source": caption_type.unwrap_or(json!("captions")), "notes": notes, "cues": vtt_cues(&text)}));
            }
        }
        if let Some(tx) = tx {
            if let Some(text) = inside(&lib, &tx.path).filter(|f| f.is_file()).and_then(|f| read_text(&f)) {
                let cues: Vec<Value> = text.split("\n\n").filter(|x| !x.trim().is_empty()).map(|x| json!({"t": null, "text": x})).collect();
                return Some(json!({"source": caption_type.unwrap_or(json!("transcript")), "notes": notes, "cues": cues}));
            }
        }
        Some(json!({"source": null, "cues": []}))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cues_lose_their_repeats_and_merge() {
        let vtt = "WEBVTT\n\n1\n00:00:01.000 --> 00:00:03.000\nBoil the <c>water</c>\n\n00:00:03.000 --> 00:00:05.000\nBoil the water\nfor one minute\n\n\
                   00:20.500 --> 00:22.000\nThen let it cool &amp; settle\n\n00:00:21.000 --> 00:00:23.000\n\n";
        let cues = vtt_cues(vtt);
        assert_eq!(cues, vec![json!({"t": 1, "text": "Boil the water for one minute"}), json!({"t": 20, "text": "Then let it cool & settle"})]);
    }

    #[test]
    fn entities() {
        assert_eq!(unescape("a &amp; b &lt;i&gt; &#115;&#x63; &bogus; &"), "a & b <i> sc &bogus; &");
        assert_eq!(unescape("java&#0000115;cript&colon;"), "javascript:");
    }
}
