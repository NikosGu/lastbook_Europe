//! Who this copy is, in one place: the project's name, where setup downloads from, where the update check looks, and the
//! addresses shown to people. Everything that names a download host reads it from here.
//!
//! No download site is chosen yet. Until one is:
//!   - [`DOWNLOAD_HOST`] is a name under `.invalid`, a domain reserved never to exist (RFC 6761). A manifest written for
//!     testing on this computer may name it; its files are then used through setup's "Use the file you already have",
//!     and nothing can be fetched from it.
//!   - [`PUBLISHED`] is false: setup fetches no manifest, and the update check answers that there is nothing to check yet,
//!     without opening a connection.
//!
//! The local AI's models come from Hugging Face, which works today.

/// Working name, until the project's name is chosen.
pub const NAME: &str = "Lastbook Europe";
pub const DOWNLOAD_HOST: &str = "downloads.lastbook-europe.invalid";
/// True once DOWNLOAD_HOST serves manifest-v1.json and app-v1.json.
pub const PUBLISHED: bool = false;
/// Where the update check sends people for a new version, once there is one.
pub const WEBSITE: Option<&str> = None;
/// Where to ask for a source to be removed (each pack's Sources page).
pub const CONTACT: Option<&str> = None;

/// The local AI's model files.
pub const MODEL_HOST: &str = "huggingface.co";
/// Where Hugging Face redirects a download, and the rest of its download hosts (`*.hf.co`).
pub const MODEL_REDIRECT_HOSTS: &[&str] = &["cas-bridge.xethub.hf.co"];
pub const MODEL_SUFFIXES: &[&str] = &[".hf.co"];

pub fn download_base() -> String {
    format!("https://{DOWNLOAD_HOST}")
}

pub fn manifest_url() -> Option<String> {
    PUBLISHED.then(|| format!("{}/manifest-v1.json", download_base()))
}

pub fn update_url() -> Option<String> {
    PUBLISHED.then(|| format!("{}/app-v1.json", download_base()))
}

/// True while no download site is chosen.
pub fn placeholder() -> bool {
    DOWNLOAD_HOST.rsplit('.').next() == Some("invalid")
}
