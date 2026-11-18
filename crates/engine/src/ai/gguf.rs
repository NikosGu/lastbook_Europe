//! What a model file says about itself: the GGUF header's metadata (architecture, name, experts, context, chat template).
//! Only the header is read, a few kilobytes; tensors are not touched.

use std::io::{BufReader, Read};
use std::path::Path;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Info {
    pub arch: String,
    pub name: String,
    pub block_count: u64,
    pub context_length: u64,
    pub expert_count: u64,
    pub expert_used_count: u64,
    pub chat_template: Option<String>,
    /// general.type: "model" for a model; "imatrix", "mmproj" (a vision projector) and the like are not one.
    pub kind: String,
    /// tokenizer.ggml.model: the tokenizer the file carries ("gpt2", "llama"…); empty without one.
    pub tokenizer: String,
    /// The texts of the beginning- and end-of-text tokens, as a chat template writes them (bos_token, eos_token).
    pub bos_token: String,
    pub eos_token: String,
    /// tokenizer.ggml.add_bos_token: whether the tokenizer puts the beginning-of-text token first itself (as the runner
    /// does when it is so); None when the file does not say.
    pub add_bos: Option<bool>,
}

impl Info {
    pub fn is_moe(&self) -> bool {
        self.expert_count > 1
    }
}

struct R<T: Read> {
    r: T,
    read: u64,
}

impl<T: Read> R<T> {
    fn bytes<const N: usize>(&mut self) -> std::io::Result<[u8; N]> {
        let mut b = [0u8; N];
        self.r.read_exact(&mut b)?;
        self.read += N as u64;
        Ok(b)
    }
    fn u32(&mut self) -> std::io::Result<u32> {
        Ok(u32::from_le_bytes(self.bytes()?))
    }
    fn u64(&mut self) -> std::io::Result<u64> {
        Ok(u64::from_le_bytes(self.bytes()?))
    }
    fn string(&mut self) -> std::io::Result<String> {
        let n = self.u64()?;
        if n > 1 << 24 {
            return Err(bad("a string longer than 16 MB"));
        }
        let mut b = vec![0u8; n as usize];
        self.r.read_exact(&mut b)?;
        self.read += n;
        Ok(String::from_utf8_lossy(&b).into_owned())
    }
    fn skip(&mut self, n: u64) -> std::io::Result<()> {
        let got = std::io::copy(&mut (&mut self.r).take(n), &mut std::io::sink())?;
        self.read += got;
        if got != n {
            return Err(bad("the file ends inside its header"));
        }
        Ok(())
    }
}

fn bad(what: &str) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, format!("not a GGUF model file: {what}"))
}

enum V {
    Num(u64),
    Str(String),
    Strs(Vec<String>),
    Other,
}

/// Size in bytes of one value of a fixed-size GGUF type.
fn fixed(t: u32) -> Option<u64> {
    match t {
        0 | 1 | 7 => Some(1),
        2 | 3 => Some(2),
        4..=6 => Some(4),
        10..=12 => Some(8),
        _ => None,
    }
}

/// A value; a list of strings is kept only when `keep` (the tokenizer's tokens), else read past.
fn value<T: Read>(r: &mut R<T>, t: u32, keep: bool) -> std::io::Result<V> {
    Ok(match t {
        0 | 7 => V::Num(r.bytes::<1>()?[0] as u64),
        1 => V::Num(r.bytes::<1>()?[0] as i8 as i64 as u64),
        2 => V::Num(u16::from_le_bytes(r.bytes()?) as u64),
        3 => V::Num(i16::from_le_bytes(r.bytes()?) as i64 as u64),
        4 => V::Num(r.u32()? as u64),
        5 => V::Num(i32::from_le_bytes(r.bytes()?) as i64 as u64),
        6 => {
            r.skip(4)?;
            V::Other
        }
        8 => V::Str(r.string()?),
        9 => {
            let (it, n) = (r.u32()?, r.u64()?);
            match fixed(it) {
                Some(size) => r.skip(size.checked_mul(n).ok_or_else(|| bad("an array too large"))?)?,
                None if it == 8 => {
                    if keep {
                        let mut all = Vec::with_capacity(n.min(1 << 20) as usize);
                        for _ in 0..n {
                            all.push(r.string()?);
                        }
                        return Ok(V::Strs(all));
                    }
                    for _ in 0..n {
                        r.string()?;
                    }
                }
                None => return Err(bad("an array of arrays")),
            }
            V::Other
        }
        10 => V::Num(r.u64()?),
        11 => V::Num(r.u64()?),
        12 => {
            r.skip(8)?;
            V::Other
        }
        _ => return Err(bad("an unknown value type")),
    })
}

/// The header's metadata, or an error if the file is not a GGUF model.
pub fn read(path: &Path) -> std::io::Result<Info> {
    let mut r = R { r: BufReader::new(std::fs::File::open(path)?), read: 0 };
    if &r.bytes::<4>()? != b"GGUF" {
        return Err(bad("no GGUF mark"));
    }
    let version = r.u32()?;
    if !(2..=3).contains(&version) {
        return Err(bad("an unknown version"));
    }
    let _tensors = r.u64()?;
    let kv = r.u64()?;
    if kv > 100_000 {
        return Err(bad("too many metadata entries"));
    }
    let mut info = Info::default();
    let mut nums: Vec<(String, u64)> = Vec::new();
    let mut tokens: Vec<String> = Vec::new();
    for _ in 0..kv {
        let key = r.string()?;
        let t = r.u32()?;
        match value(&mut r, t, key == "tokenizer.ggml.tokens")? {
            V::Str(s) if key == "general.architecture" => info.arch = s,
            V::Str(s) if key == "general.name" => info.name = s,
            V::Str(s) if key == "general.type" => info.kind = s,
            V::Str(s) if key == "tokenizer.ggml.model" => info.tokenizer = s,
            V::Str(s) if key == "tokenizer.chat_template" => info.chat_template = Some(s),
            V::Strs(all) => tokens = all,
            V::Num(n) => nums.push((key, n)),
            _ => {}
        }
    }
    let token = |key: &str| nums.iter().find(|(k, _)| k == key).and_then(|(_, n)| tokens.get(*n as usize)).cloned().unwrap_or_default();
    info.bos_token = token("tokenizer.ggml.bos_token_id");
    info.add_bos = nums.iter().find(|(k, _)| k == "tokenizer.ggml.add_bos_token").map(|(_, n)| *n != 0);
    info.eos_token = token("tokenizer.ggml.eos_token_id");
    let get = |suffix: &str| nums.iter().find(|(k, _)| *k == format!("{}.{suffix}", info.arch)).map(|(_, n)| *n).unwrap_or(0);
    info.block_count = get("block_count");
    info.context_length = get("context_length");
    info.expert_count = get("expert_count");
    info.expert_used_count = get("expert_used_count");
    Ok(info)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub enum Kv<'a> {
        Num(u64),
        Str(&'a str),
        Strs(&'a [&'a str]),
    }

    /// A GGUF header with the given metadata and no tensors.
    pub fn header(kv: &[(&str, Kv)]) -> Vec<u8> {
        let mut b = b"GGUF".to_vec();
        b.extend(3u32.to_le_bytes());
        b.extend(0u64.to_le_bytes());
        b.extend((kv.len() as u64).to_le_bytes());
        let s = |b: &mut Vec<u8>, s: &str| {
            b.extend((s.len() as u64).to_le_bytes());
            b.extend(s.as_bytes());
        };
        for (k, v) in kv {
            s(&mut b, k);
            match v {
                Kv::Num(n) => {
                    b.extend(4u32.to_le_bytes());
                    b.extend((*n as u32).to_le_bytes());
                }
                Kv::Str(x) => {
                    b.extend(8u32.to_le_bytes());
                    s(&mut b, x);
                }
                Kv::Strs(xs) => {
                    b.extend(9u32.to_le_bytes());
                    b.extend(8u32.to_le_bytes());
                    b.extend((xs.len() as u64).to_le_bytes());
                    for x in *xs {
                        s(&mut b, x);
                    }
                }
            }
        }
        b
    }

    #[test]
    fn reads_the_header() {
        let d = std::env::temp_dir().join(format!("lb-gguf-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let f = d.join("m.gguf");
        let h = header(&[("general.architecture", Kv::Str("qwen35moe")), ("tokenizer.ggml.tokens", Kv::Strs(&["a", "<|im_start|>"])),
                         ("general.name", Kv::Str("Qwen3.6 35B A3B")), ("qwen35moe.expert_count", Kv::Num(256)),
                         ("qwen35moe.expert_used_count", Kv::Num(8)), ("qwen35moe.context_length", Kv::Num(262144)),
                         ("tokenizer.chat_template", Kv::Str("<|im_start|>"))]);
        std::fs::write(&f, &h).unwrap();
        let i = read(&f).unwrap();
        assert_eq!(i.arch, "qwen35moe");
        assert_eq!(i.name, "Qwen3.6 35B A3B");
        assert_eq!((i.expert_count, i.expert_used_count, i.context_length), (256, 8, 262144));
        assert!(i.is_moe());
        assert_eq!(i.chat_template.as_deref(), Some("<|im_start|>"));
        std::fs::write(&f, &h[..h.len() - 5]).unwrap();
        assert!(read(&f).is_err(), "cut short");
        std::fs::write(&f, b"GGUF but not really").unwrap();
        assert!(read(&f).is_err());
        std::fs::write(&f, b"PK\x03\x04").unwrap();
        assert!(read(&f).is_err());
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn reads_the_real_files_when_here() {
        // the models on the maintainer's computer, when they are there; nothing is downloaded for this
        let home = std::env::var_os("HOME").map(std::path::PathBuf::from).unwrap_or_default();
        let f = home.join("Documents/Lavoro/qw35/.gguf/fixtures/tiny-qwen35moe-f32.gguf");
        if let Ok(i) = read(&f) {
            assert_eq!(i.arch, "qwen35moe");
            assert_eq!(i.expert_count, 256);
        }
    }
}
