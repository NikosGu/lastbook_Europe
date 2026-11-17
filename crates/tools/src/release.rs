//! What a release passes before it ships. The release workflow runs both on GitHub's machines.
//!
//!   lastbook-tools check-roots
//!       Setup's downloader trusts only the roots built into the app (crates/engine/src/setup/roots.pem), never the
//!       system's store. This connects to each host it downloads from through those roots alone (a TLS handshake and
//!       one request; nothing is downloaded), and to a site whose certificate chains to a root outside them, which must
//!       be refused. A host that fails means its chain has moved to a root outside the set: refresh roots.pem first.
//!
//!   lastbook-tools scan-package FOLDER
//!       Looks through a built package (the .app, the folder the .deb installs, the unpacked .zip) for what must not
//!       ship: a local AI runner that could reach the network (it must import no network function, and llama.cpp's
//!       libraries beside it neither, nor an RPC backend), a person's folders or a home network's addresses in the
//!       app's own files, development files (tests, scripts, sources lists, model files, keys), and a bundled manifest
//!       naming an address setup may not reach.

use lastbook_engine::setup::net::{self, Allow, NetError, Tls};
use std::path::{Path, PathBuf};
use std::time::Duration;

const ROOTS_HELP: &str = "lastbook-tools check-roots

Connects to each host setup downloads from, trusting only the roots built into the app, and to a site outside them,
which must be refused. Exit 0 when all is as it should be, 1 otherwise. Needs the network.";

const SCAN_HELP: &str = "lastbook-tools scan-package FOLDER

Looks through a built package for what must not ship (see the top of crates/tools/src/release.rs). Exit 0 when nothing
is found, 1 otherwise.";

/// Hugging Face's other download hosts (*.hf.co, which setup allows), where its files have been served from.
const MODEL_CDN_HOSTS: &[&str] = &["us.aws.cdn.hf.co", "us.gcp.cdn.hf.co", "cdn-lfs.hf.co"];
/// A site whose certificate chains to a root roots.pem does not hold (DigiCert): it must be refused.
const OUTSIDE: &str = "www.digicert.com";

pub fn check_roots(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("{ROOTS_HELP}");
        return 0;
    }
    let tls = match Tls::product() {
        Ok(t) => t,
        Err(e) => {
            eprintln!("check-roots: the built-in roots could not be read: {e}");
            return 1;
        }
    };
    let mut bad = Vec::new();
    let mut hosts: Vec<(&str, bool)> = net::allowed_hosts().into_iter().map(|h| (h, true)).collect();
    hosts.extend(MODEL_CDN_HOSTS.iter().map(|h| (*h, false)));
    for (host, needed) in hosts {
        if host.ends_with(".invalid") {
            println!("  skip {host}: the download site is still a placeholder (crates/engine/src/dist.rs)");
            continue;
        }
        match reach(host, &tls) {
            Ok(said) => println!("  ok   {host} ({said})"),
            Err(NetError::Certificate(m)) => {
                println!("  FAIL {host}: its certificate does not verify through roots.pem: {m}");
                bad.push(host.to_string());
            }
            Err(e) if needed => {
                println!("  FAIL {host}: could not be reached: {e}");
                bad.push(host.to_string());
            }
            Err(e) => println!("  note {host}: could not be reached ({e}); Hugging Face may no longer use it"),
        }
    }
    match reach(OUTSIDE, &tls) {
        Err(NetError::Certificate(_)) => println!("  ok   {OUTSIDE} refused, as it should be: only roots.pem is trusted"),
        Ok(_) => {
            println!("  FAIL {OUTSIDE} was accepted, though its root is not in roots.pem: another store is in use");
            bad.push(OUTSIDE.to_string());
        }
        Err(e) => {
            println!("  FAIL {OUTSIDE} could not be reached ({e}), so the refusal could not be checked");
            bad.push(OUTSIDE.to_string());
        }
    }
    if bad.is_empty() {
        println!("roots: every host verifies through roots.pem alone");
        0
    } else {
        println!("roots: FAILED for {}", bad.join(", "));
        1
    }
}

/// One request to the host's root through the built-in roots: what it answered, or why there was no answer. A redirect to
/// another host counts as reached: the handshake with this one had verified.
fn reach(host: &str, tls: &Tls) -> Result<String, NetError> {
    match net::get(&format!("https://{host}/"), 0, None, &Allow::Only(vec![host.to_string()]), tls, Duration::from_secs(20)) {
        Ok(r) => Ok(format!("HTTP {}", r.status)),
        Err(NetError::Refused(_)) => Ok("sent elsewhere".into()),
        Err(e) => Err(e),
    }
}

/// Functions a program needs to reach the network, by the names the systems' libraries give them.
const NET_FUNCTIONS: &[&str] = &[
    "socket", "connect", "bind", "listen", "accept", "accept4", "getaddrinfo", "gethostbyname", "gethostbyname2", "getnameinfo",
    "sendto", "recvfrom", "sendmsg", "recvmsg", "WSAStartup", "WSASocketW", "WSAConnect", "WSAIoctl", "InternetOpenW",
    "InternetOpenA", "WinHttpOpen", "URLDownloadToFileW", "DnsQuery_W",
    // libcurl, which llama.cpp links when it is built to download models
    "curl_easy_init", "curl_easy_perform", "curl_global_init",
];
/// Windows libraries that are the network, whatever is imported from them.
const NET_DLLS: &[&str] = &["ws2_32.dll", "wsock32.dll", "mswsock.dll", "winhttp.dll", "wininet.dll", "urlmon.dll", "dnsapi.dll",
                            "iphlpapi.dll", "httpapi.dll", "webio.dll"];

/// Why a program or library may reach the network: the network functions or libraries it imports.
pub fn network_imports(data: &[u8]) -> Result<Vec<String>, String> {
    use object::read::Object;
    let file = object::File::parse(data).map_err(|e| e.to_string())?;
    let mut found = Vec::new();
    for imp in file.imports().map_err(|e| e.to_string())? {
        let imp = imp.map_err(|e| e.to_string())?;
        // a function imported by name (a Windows library's functions can also be imported by number: the library says it)
        let name = match imp.name() {
            object::read::NameOrOrdinal::Name(n) => String::from_utf8_lossy(n).into_owned(),
            object::read::NameOrOrdinal::Ordinal(o) => format!("#{o}"),
        };
        let name = name.trim_start_matches('_'); // Mach-O's leading underscore
        let lib = String::from_utf8_lossy(imp.library()).to_ascii_lowercase();
        if NET_FUNCTIONS.contains(&name) || NET_DLLS.contains(&lib.as_str()) {
            found.push(if lib.is_empty() { name.to_string() } else { format!("{name} ({lib})") });
        }
    }
    found.sort();
    found.dedup();
    Ok(found)
}

pub fn scan_package(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("{SCAN_HELP}");
        return 0;
    }
    let [folder] = args else {
        eprintln!("{SCAN_HELP}");
        return 2;
    };
    let root = PathBuf::from(folder);
    match scan(&root) {
        Ok(found) if found.is_empty() => {
            println!("scan-package: nothing that must not ship in {}", root.display());
            0
        }
        Ok(found) => {
            for f in &found {
                println!("  FAIL {f}");
            }
            println!("scan-package: {} things that must not ship in {}", found.len(), root.display());
            1
        }
        Err(e) => {
            eprintln!("scan-package: {e}");
            1
        }
    }
}

/// Everything in the package that must not ship, as sentences; empty when it is clean.
pub fn scan(root: &Path) -> Result<Vec<String>, String> {
    if !root.is_dir() {
        return Err(format!("{} is not a folder", root.display()));
    }
    let mut files = Vec::new();
    walk(root, &mut files);
    let rel = |p: &Path| p.strip_prefix(root).unwrap_or(p).to_string_lossy().replace('\\', "/");
    let mut found = Vec::new();

    // the local AI's runner, and llama.cpp's libraries beside it: no way to the network
    let runners: Vec<&PathBuf> = files.iter().filter(|p| matches!(file_name(p), "lastbook-llm" | "lastbook-llm.exe")).collect();
    if runners.is_empty() {
        found.push("no local AI runner (lastbook-llm) in the package".to_string());
    }
    for p in files.iter().filter(|p| {
        let n = file_name(p).to_ascii_lowercase();
        matches!(n.as_str(), "lastbook-llm" | "lastbook-llm.exe")
            || ((n.contains("llama") || n.contains("ggml")) && (n.contains(".so") || n.ends_with(".dylib") || n.ends_with(".dll")))
    }) {
        let n = file_name(p).to_ascii_lowercase();
        if n.contains("ggml-rpc") {
            found.push(format!("{}: llama.cpp's RPC backend, which talks to other computers", rel(p)));
            continue;
        }
        if std::fs::symlink_metadata(p).is_ok_and(|m| m.file_type().is_symlink()) {
            continue; // its target is scanned under its own name
        }
        let data = std::fs::read(p).map_err(|e| format!("{}: {e}", rel(p)))?;
        match network_imports(&data) {
            Ok(net) if net.is_empty() => {}
            Ok(net) => found.push(format!("{}: imports the network: {}", rel(p), net.join(", "))),
            Err(e) => found.push(format!("{}: could not be read as a program or library: {e}", rel(p))),
        }
    }

    // development files, and what is never part of the app
    for p in &files {
        let r = rel(p);
        let lower = r.to_ascii_lowercase();
        let name = file_name(p).to_ascii_lowercase();
        let why = if lower.split('/').any(|part| matches!(part, "tests" | "test" | "fixtures" | ".git" | "derived" | "target" | "node_modules")) {
            Some("a development folder")
        } else if lower.contains("content/library/") {
            Some("the library's sources lists, which only lastbook-tools reads")
        } else if [".py", ".pyc", ".mjs", ".sh", ".ps1", ".rs", ".gguf", ".key", ".p12", ".pfx", ".env"].iter().any(|s| name.ends_with(s)) {
            Some("a script, source, model or key file")
        } else if name.ends_with(".pem") {
            Some("a certificate file (the roots are built into the program)")
        } else {
            None
        };
        if let Some(why) = why {
            found.push(format!("{r}: {why}"));
        }
    }

    // the app's own files: no person's folder, no home network's address
    let user_path = regex::Regex::new(r"(?i)\b[a-z]:[\\/]+users[\\/]+[a-z0-9_.-]+|/Users/[A-Za-z0-9_.-]+|/home/[a-z0-9_.-]+|/private/tmp/").unwrap();
    let lan = regex::Regex::new(r"\b(?:192\.168|10\.\d{1,3}|172\.(?:1[6-9]|2\d|3[01]))\.\d{1,3}\.\d{1,3}\b").unwrap();
    for p in files.iter().filter(|p| {
        let n = file_name(p).to_ascii_lowercase();
        [".html", ".js", ".css", ".json", ".txt", ".md", ".svg"].iter().any(|s| n.ends_with(s))
    }) {
        let Ok(text) = std::fs::read_to_string(p) else { continue };
        if let Some(m) = user_path.find(&text) {
            found.push(format!("{}: names a person's folder ({})", rel(p), m.as_str()));
        }
        if let Some(m) = lan.find(&text) {
            found.push(format!("{}: names a home network's address ({})", rel(p), m.as_str()));
        }
    }

    // a bundled manifest: only addresses setup may reach
    for p in files.iter().filter(|p| file_name(p) == "manifest-v1.json") {
        let text = std::fs::read_to_string(p).map_err(|e| format!("{}: {e}", rel(p)))?;
        let v: serde_json::Value = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", rel(p)))?;
        let mut urls = Vec::new();
        collect_urls(&v, &mut urls);
        for u in urls.iter().filter(|u| !net::url_allowed(u, &Allow::Product)) {
            found.push(format!("{}: names an address setup may not reach: {u}", rel(p)));
        }
    }
    Ok(found)
}

fn collect_urls(v: &serde_json::Value, out: &mut Vec<String>) {
    match v {
        serde_json::Value::Object(m) => {
            for (k, x) in m {
                match x {
                    serde_json::Value::String(s) if k == "url" => out.push(s.clone()),
                    _ => collect_urls(x, out),
                }
            }
        }
        serde_json::Value::Array(a) => a.iter().for_each(|x| collect_urls(x, out)),
        _ => {}
    }
}

fn file_name(p: &Path) -> &str {
    p.file_name().and_then(|n| n.to_str()).unwrap_or("")
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let p = e.path();
        match e.file_type() {
            Ok(t) if t.is_dir() => walk(&p, out),
            Ok(_) => out.push(p),
            Err(_) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("lb-release-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// This test program, with the engine's downloader kept in it (check_roots reaches it): a program that imports the network.
    fn networked_program() -> Vec<u8> {
        std::hint::black_box(check_roots as fn(&[String]) -> i32);
        std::fs::read(std::env::current_exe().unwrap()).unwrap()
    }

    #[test]
    fn a_program_that_can_reach_the_network_is_seen() {
        assert!(!network_imports(&networked_program()).unwrap().is_empty());
    }

    #[test]
    fn a_package_with_what_must_not_ship() {
        let d = temp("bad");
        let me = networked_program();
        std::fs::write(d.join(if cfg!(windows) { "lastbook-llm.exe" } else { "lastbook-llm" }), &me).unwrap(); // a "runner" with the network
        std::fs::create_dir_all(d.join("web")).unwrap();
        std::fs::write(d.join("web/app.js"), "fetch('http://192.168.1.20/x'); // /Users/someone/x").unwrap();
        std::fs::create_dir_all(d.join("content/library")).unwrap();
        std::fs::write(d.join("content/library/starter.json"), "{}").unwrap();
        std::fs::write(d.join("libggml-rpc.so"), b"").unwrap();
        std::fs::create_dir_all(d.join("setup")).unwrap();
        std::fs::write(d.join("setup/manifest-v1.json"), r#"{"packs":[{"files":[{"url":"https://evil.example/x.zip"}]}]}"#).unwrap();
        let found = scan(&d).unwrap().join("\n");
        for want in ["imports the network", "a person's folder", "a home network's address", "sources lists", "RPC backend", "evil.example"] {
            assert!(found.contains(want), "{want} not found in:\n{found}");
        }
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_clean_package() {
        let d = temp("clean");
        std::fs::create_dir_all(d.join("web")).unwrap();
        std::fs::write(d.join("web/index.html"), "<p>Lastbook</p>").unwrap();
        std::fs::write(d.join("README.txt"), "made for people").unwrap();
        let found = scan(&d).unwrap();
        assert_eq!(found, vec!["no local AI runner (lastbook-llm) in the package".to_string()]);
        let _ = std::fs::remove_dir_all(&d);
    }
}
