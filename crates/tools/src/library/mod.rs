//! The library's data tools: `build-library` makes a LastbookData folder from documents and their sources list, and
//! `build-pack` turns one into the zip setup downloads. What they write is what the engine reads
//! (crates/engine/src/library/) and what setup checks and unpacks (crates/engine/src/setup/install.rs).
//! In the original Lastbook these were engine/library.py's build and packaging/build_packs.py.

pub mod build;
pub mod extract;
pub mod pack;
pub mod sources;
#[cfg(test)]
mod tests;

use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::Read;
use std::path::Path;

/// A subcommand's options: `--name value` pairs and `--flag`s.
pub struct Args {
    values: HashMap<String, String>,
    flags: Vec<String>,
}

impl Args {
    /// The options, or the reason they cannot be read. `flags` are the options that take no value.
    pub fn parse(args: &[String], flags: &[&str]) -> Result<Args, String> {
        let mut values = HashMap::new();
        let mut on = Vec::new();
        let mut it = args.iter();
        while let Some(a) = it.next() {
            let Some(name) = a.strip_prefix("--") else { return Err(format!("unexpected argument {a:?}")) };
            if flags.contains(&name) {
                on.push(name.to_string());
                continue;
            }
            match it.next() {
                Some(v) => {
                    values.insert(name.to_string(), v.clone());
                }
                None => return Err(format!("--{name} needs a value")),
            }
        }
        Ok(Args { values, flags: on })
    }

    pub fn get(&self, name: &str) -> Option<&str> {
        self.values.get(name).map(String::as_str)
    }

    pub fn need(&self, name: &str) -> Result<&str, String> {
        self.get(name).ok_or_else(|| format!("--{name} is needed"))
    }

    pub fn flag(&self, name: &str) -> bool {
        self.flags.iter().any(|f| f == name)
    }
}

/// A file's SHA-256, in hex.
pub fn sha256_file(path: &Path) -> std::io::Result<String> {
    let mut f = std::fs::File::open(path)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(hex::encode(h.finalize()))
}

/// Today as YYYY-MM-DD (UTC).
pub fn today() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    let (y, m, d) = lastbook_engine::setup::install::civil_from_days(secs.div_euclid(86400));
    format!("{y:04}-{m:02}-{d:02}")
}
