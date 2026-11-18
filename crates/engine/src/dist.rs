//! Who this copy is, in one place: the project's name, where setup downloads from, where the update check looks, and the
//! addresses shown to people. Everything that names a download host reads it from here.
//!
//! The download site is this project's GitHub repository, through its releases:
//!   - the data (the library packs, the region maps, the address indexes, and manifest-v1.json, which lists them) are
//!     the files of the release tagged [`DATA_TAG`], under [`download_base`];
//!   - each version of the app is a release of its own, and the newest one's app-v1.json says which version that is
//!     (GitHub's "latest release" address, [`update_url`]).
//!
//! On github.com setup may fetch only this repository's release files; GitHub then sends the download on to
//! [`DOWNLOAD_REDIRECT_HOSTS`]. A release kept as a draft cannot be fetched by anyone but its owners: until the maintainer
//! publishes the data release, setup finds nothing to download and uses files already on the computer.
//!
//! The local AI's models come from Hugging Face.

/// Working name, until the project's name is chosen.
pub const NAME: &str = "Lastbook Europe";
/// The project's repository on GitHub, where its releases are.
pub const REPO: &str = "NikosGu/lastbook_Europe";
pub const DOWNLOAD_HOST: &str = "github.com";
/// Where GitHub sends the download of a release's file.
pub const DOWNLOAD_REDIRECT_HOSTS: &[&str] = &["release-assets.githubusercontent.com", "objects.githubusercontent.com"];
/// The release that holds the data.
pub const DATA_TAG: &str = "data-v1";
/// True when setup fetches the manifest and the update check asks: the download site is chosen.
pub const PUBLISHED: bool = true;
/// Where the update check sends people for a new version.
pub const WEBSITE: Option<&str> = Some("https://github.com/NikosGu/lastbook_Europe/releases");
/// Who to write to about a problem, or to ask for a source to be removed (each pack's Sources page, the README, the
/// installers): the maintainer, at the email of the GitHub account that publishes the project.
pub const CONTACT: Option<&str> = Some("nikos.gu.devs@gmail.com");

/// The local AI's model files.
pub const MODEL_HOST: &str = "huggingface.co";
/// Where Hugging Face redirects a download, and the rest of its download hosts (`*.hf.co`).
pub const MODEL_REDIRECT_HOSTS: &[&str] = &["cas-bridge.xethub.hf.co"];
pub const MODEL_SUFFIXES: &[&str] = &[".hf.co"];

/// The data release's files: `<base>/<file>`.
pub fn download_base() -> String {
    format!("https://{DOWNLOAD_HOST}/{REPO}/releases/download/{DATA_TAG}")
}

/// The only paths setup may fetch on [`DOWNLOAD_HOST`]: this repository's release files.
pub fn download_path_prefix() -> String {
    format!("/{REPO}/releases/")
}

pub fn manifest_url() -> Option<String> {
    PUBLISHED.then(|| format!("{}/manifest-v1.json", download_base()))
}

/// app-v1.json of the newest published version.
pub fn update_url() -> Option<String> {
    PUBLISHED.then(|| format!("https://{DOWNLOAD_HOST}/{REPO}/releases/latest/download/app-v1.json"))
}

/// True while no download site is chosen.
pub fn placeholder() -> bool {
    DOWNLOAD_HOST.rsplit('.').next() == Some("invalid")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_addresses() {
        assert!(!placeholder());
        assert_eq!(download_base(), "https://github.com/NikosGu/lastbook_Europe/releases/download/data-v1");
        assert!(manifest_url().unwrap().starts_with(&format!("https://{DOWNLOAD_HOST}{}", download_path_prefix())));
        assert!(update_url().unwrap().starts_with(&format!("https://{DOWNLOAD_HOST}{}", download_path_prefix())));
    }
}
