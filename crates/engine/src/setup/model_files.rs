//! Model files the person already has: the GGUF files other programs downloaded (LM Studio, Ollama, Jan, GPT4All,
//! llama.cpp, Hugging Face's cache) or that lie in the usual folders, each read from its header to say whether Lastbook
//! Europe can talk to it (crate::ai::format::Family::of). Setup offers them beside the catalogue's models; one chosen is
//! used where it is, never copied. Nothing here leaves the computer: folders are read, files' headers opened.

use crate::ai::format::Family;
use crate::ai::gguf;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Smaller than this, a GGUF file is a projector, an adapter or a test, not a model worth offering.
const MIN_BYTES: u64 = 200_000_000;
/// How deep the walk goes below each folder, and how many entries it looks at in all: a large folder never stalls setup.
const DEPTH: usize = 6;
const ENTRIES: usize = 40_000;
const MAX_FOUND: usize = 40;

/// The folders other programs keep models in, on this system, with the name setup shows for each.
pub fn places(home: &Path) -> Vec<(PathBuf, &'static str)> {
    let h = |rel: &str| home.join(rel);
    let mut out = vec![
        (h(".lmstudio/models"), "LM Studio"),
        (h(".cache/lm-studio/models"), "LM Studio"),
        (h("jan/models"), "Jan"),
        (h(".cache/huggingface/hub"), "Hugging Face"),
        (h("models"), ""),
        (h("llama-models"), ""),
        (h("Models"), ""),
        (h("Downloads"), ""),
        (h("Documents"), ""),
        (h("Desktop"), ""),
    ];
    if cfg!(target_os = "macos") {
        out.extend([
            (h("Library/Application Support/Jan/data/models"), "Jan"),
            (h("Library/Application Support/nomic.ai/GPT4All"), "GPT4All"),
            (h("Library/Caches/llama.cpp"), "llama.cpp"),
        ]);
    } else if cfg!(windows) {
        let env = |k: &str| std::env::var_os(k).map(PathBuf::from);
        if let Some(a) = env("APPDATA") {
            out.push((a.join("Jan/data/models"), "Jan"));
        }
        if let Some(l) = env("LOCALAPPDATA") {
            out.push((l.join("nomic.ai/GPT4All"), "GPT4All"));
            out.push((l.join("llama.cpp"), "llama.cpp"));
        }
    } else {
        out.extend([
            (h(".local/share/Jan/data/models"), "Jan"),
            (h(".local/share/nomic.ai/GPT4All"), "GPT4All"),
            (h(".cache/llama.cpp"), "llama.cpp"),
        ]);
    }
    out
}

/// Ollama's models: its manifests name each model's layers; the model's own layer is a GGUF file among its blobs.
pub fn ollama(home: &Path) -> Vec<(PathBuf, String)> {
    let mut roots = vec![home.join(".ollama/models")];
    if let Some(m) = std::env::var_os("OLLAMA_MODELS") {
        roots.insert(0, PathBuf::from(m));
    }
    if cfg!(target_os = "linux") {
        roots.push(PathBuf::from("/usr/share/ollama/.ollama/models"));
    }
    let mut out = Vec::new();
    for root in roots {
        let mut stack = vec![(root.join("manifests"), 0usize)];
        while let Some((dir, depth)) = stack.pop() {
            for e in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
                let p = e.path();
                let Ok(t) = e.file_type() else { continue };
                if t.is_dir() && depth < 5 {
                    stack.push((p, depth + 1));
                } else if t.is_file() {
                    let Some(m) = std::fs::read(&p).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok()) else { continue };
                    let Some(digest) = m["layers"].as_array().into_iter().flatten()
                        .find(|l| l["mediaType"] == "application/vnd.ollama.image.model").and_then(|l| l["digest"].as_str()) else { continue };
                    let blob = root.join("blobs").join(digest.replace(':', "-"));
                    // registry.ollama.ai/library/llama3.2/3b: "llama3.2:3b"
                    let tag = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                    let model = p.parent().and_then(|d| d.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                    out.push((blob, format!("{model}:{tag}")));
                }
            }
        }
    }
    out
}

/// Is this a GGUF file worth reading: the name says so (or it is one of Ollama's blobs), it is large enough, and it is not
/// a vision projector or a later part of a split model.
fn candidate(p: &Path, size: u64) -> bool {
    let name = p.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
    if size < MIN_BYTES || !name.ends_with(".gguf") || name.contains("mmproj") {
        return false;
    }
    // "…-00002-of-00003.gguf": only the first part opens a split model
    !regex_split_part(&name)
}

fn regex_split_part(name: &str) -> bool {
    match name.rfind("-of-") {
        Some(i) if i >= 6 => {
            let part = &name[i - 5..i];
            part.chars().all(|c| c.is_ascii_digit()) && part != "00001"
        }
        _ => false,
    }
}

/// One model file, as setup lists it: None if it is not a model Lastbook can talk to.
pub fn describe(p: &Path, from: &str, label: Option<&str>) -> Option<Value> {
    let size = std::fs::metadata(p).ok().filter(|m| m.is_file())?.len();
    let info = gguf::read(p).ok()?;
    let family = Family::of(&info)?;
    let file = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let title = label.map(str::to_string).or_else(|| Some(info.name.trim().to_string()).filter(|n| !n.is_empty())).unwrap_or_else(|| file.clone());
    Some(json!({"kind": "model_file", "path": p.to_string_lossy(), "name": file, "title": title, "size": size, "from": from,
                "family": family.id(), "tools": family.tools(), "arch": info.arch, "experts": info.expert_count}))
}

/// The model files on this computer, largest first, at most MAX_FOUND; `skip`: files already listed (the catalogue's).
pub fn find(home: &Path, skip: &HashSet<String>) -> Vec<Value> {
    let mut out: Vec<Value> = Vec::new();
    let mut seen: HashSet<PathBuf> = HashSet::new();
    let mut add = |p: PathBuf, from: &str, label: Option<&str>, out: &mut Vec<Value>| {
        let key = p.canonicalize().unwrap_or_else(|_| p.clone());
        if out.len() >= MAX_FOUND || !seen.insert(key) || skip.contains(&p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()) {
            return;
        }
        if let Some(v) = describe(&p, from, label) {
            out.push(v);
        }
    };
    for (blob, name) in ollama(home) {
        if std::fs::metadata(&blob).is_ok_and(|m| m.is_file() && m.len() >= MIN_BYTES) {
            add(blob, "Ollama", Some(&name), &mut out);
        }
    }
    let mut looked = 0usize;
    for (root, from) in places(home) {
        let mut stack = vec![(root, 0usize)];
        while let Some((dir, depth)) = stack.pop() {
            for e in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
                looked += 1;
                if looked > ENTRIES {
                    break;
                }
                let p = e.path();
                let Ok(t) = e.file_type() else { continue };
                if t.is_dir() {
                    // a folder's own links are not followed (Hugging Face's cache links files, not folders); hidden ones are skipped
                    if depth < DEPTH && !e.file_name().to_string_lossy().starts_with('.') {
                        stack.push((p, depth + 1));
                    }
                } else if let Ok(m) = std::fs::metadata(&p) {
                    if m.is_file() && candidate(&p, m.len()) {
                        add(p, from, None, &mut out);
                    }
                }
            }
        }
    }
    out.sort_by_key(|v| std::cmp::Reverse(v["size"].as_u64().unwrap_or(0)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::gguf::tests::{header, Kv};

    fn model(dir: &Path, name: &str, kv: &[(&str, Kv)]) -> PathBuf {
        let p = dir.join(name);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        let mut b = header(kv);
        b.resize(MIN_BYTES as usize + 16, 0); // as large as a model, all zeros after its header
        std::fs::write(&p, b).unwrap();
        p
    }

    #[test]
    fn the_models_other_programs_downloaded() {
        let home = std::env::temp_dir().join(format!("lb-model-files-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        let chat = |arch: &'static str| vec![("general.architecture", Kv::Str(arch)), ("general.name", Kv::Str("Llama 3.2 3B Instruct")),
                                             ("tokenizer.ggml.model", Kv::Str("gpt2")), ("tokenizer.chat_template", Kv::Str("{{ messages }}"))];
        model(&home, ".lmstudio/models/bartowski/Llama-3.2-3B-Instruct-GGUF/Llama-3.2-3B-Instruct-Q4_K_M.gguf", &chat("llama"));
        model(&home, ".lmstudio/models/x/mmproj-model-f16.gguf", &chat("clip"));
        model(&home, "Downloads/base-model.gguf", &[("general.architecture", Kv::Str("llama")), ("tokenizer.ggml.model", Kv::Str("gpt2"))]);
        model(&home, "models/qwen3-30b-a3b-00002-of-00003.gguf", &chat("qwen3moe"));
        let first = model(&home, "models/qwen3-30b-a3b-00001-of-00003.gguf", &[("general.architecture", Kv::Str("qwen3moe")), ("tokenizer.ggml.model", Kv::Str("gpt2"))]);
        // Ollama: a manifest names the model's blob, which has no extension
        let blob = model(&home, ".ollama/models/blobs/sha256-abc123", &chat("gemma3"));
        std::fs::create_dir_all(home.join(".ollama/models/manifests/registry.ollama.ai/library/gemma3")).unwrap();
        std::fs::write(home.join(".ollama/models/manifests/registry.ollama.ai/library/gemma3/4b"),
                       r#"{"layers": [{"mediaType": "application/vnd.ollama.image.model", "digest": "sha256:abc123"}]}"#).unwrap();
        let found = find(&home, &HashSet::new());
        let titles: Vec<&str> = found.iter().map(|f| f["title"].as_str().unwrap()).collect();
        assert!(titles.contains(&"Llama 3.2 3B Instruct"), "{found:?}");
        assert!(titles.contains(&"gemma3:4b"), "Ollama's, by its own name: {found:?}");
        // by file name: the walk writes the system's own separators (Windows' \\), the test's paths may mix them
        let named = |p: &Path, f: &Value| Path::new(f["path"].as_str().unwrap_or("")).file_name() == p.file_name();
        assert!(found.iter().any(|f| named(&first, f) && f["family"] == "qwen35"), "the first part of a split model, Qwen's: {found:?}");
        assert_eq!(found.len(), 3, "no projector, no base model, no later part of a split model: {found:?}");
        let ollama = found.iter().find(|f| named(&blob, f)).unwrap();
        assert_eq!((ollama["from"].as_str(), ollama["family"].as_str(), ollama["tools"].as_bool()), (Some("Ollama"), Some("template"), Some(false)));
        // the catalogue's own files are listed as the catalogue's, not twice
        let skip: HashSet<String> = ["Llama-3.2-3B-Instruct-Q4_K_M.gguf".to_string()].into();
        assert_eq!(find(&home, &skip).len(), 2);
        let _ = std::fs::remove_dir_all(&home);
    }
}
