//! Lastbook Europe's engine: everything the window asks for, answered in-process. There is no network port: the desktop
//! app hands each request from its webview to [`router::Engine::handle`] and gives the answer back, as the original Lastbook's
//! bridge did. The only code that reaches the internet is setup's downloader, and only when the person starts a download.

pub mod ai;
pub mod appdirs;
pub mod db;
pub mod dist;
pub mod http;
pub mod library;
pub mod map;
pub mod paths;
pub mod regions;
pub mod router;
pub mod setup;
pub mod static_files;
