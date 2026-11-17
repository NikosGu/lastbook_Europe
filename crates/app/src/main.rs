//! Lastbook Europe's window: the interface in the system's own webview (WKWebView on a Mac, WebView2 on Windows,
//! WebKitGTK on Linux), talking to the engine through a custom scheme, `lastbook://app/` (on Windows
//! `http://lastbook.localhost/`, which WebView2 serves to the handler without any network). No port is opened.
//!
//!   lastbook [--data FOLDER] [--home FOLDER] [--url URL] [--snapshot FILE.png] [--after SECONDS] [--size WIDTHxHEIGHT] [--eval JS]...
//!
//! Which library opens: the one named with --data (or LASTBOOK_DATA) as given, else the one remembered from last time,
//! else the app's own LastbookData; with none, the window opens on setup. A switch setup staged is made first, before
//! anything is open. --home keeps the settings and the app's data in another folder (for tests).
//!
//! --snapshot takes a picture of the page after --after seconds (default 3), writes it as PNG and quits: the app's own
//! screenshot, which needs no screen-recording permission (macOS only for now). Each --eval runs in the page, one a second
//! from --after on, before the picture: a test drives the page this way. With --snapshot, the page's script errors are
//! printed. --url opens another address instead of the app (a local test server, for development); everything else stays
//! blocked. Without --snapshot the page has no channel to the app at all.

// A release build on Windows is a window program: no console opens beside it.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use lastbook_engine::http::{percent_decode, Request, Response};
use lastbook_engine::paths::Paths;
use lastbook_engine::router::Engine;
use lastbook_engine::setup::api::{self as setup_api, Mode, Options, Setup};
use lastbook_engine::setup::install;
use lastbook_engine::{appdirs, dist};
use std::borrow::Cow;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tao::dpi::LogicalSize;
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tao::window::WindowBuilder;
use wry::{NewWindowResponse, WebViewBuilder};

#[cfg(target_os = "macos")]
mod snapshot_macos;

const SCHEME: &str = "lastbook";

#[derive(Debug)]
enum UserEvent {
    Eval(String),
    Snapshot,
    Quit(i32),
}

struct Args {
    data: Option<PathBuf>,
    home: Option<PathBuf>,
    url: Option<String>,
    snapshot: Option<PathBuf>,
    after: f64,
    size: (f64, f64),
    evals: Vec<String>,
}

fn parse_args() -> Args {
    let mut a = Args { data: None, home: None, url: None, snapshot: None, after: 3.0, size: (1280.0, 860.0), evals: Vec::new() };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--data" => a.data = it.next().map(PathBuf::from),
            "--home" => a.home = it.next().map(PathBuf::from),
            "--url" => a.url = it.next(),
            "--snapshot" => a.snapshot = it.next().map(PathBuf::from),
            "--after" => a.after = it.next().and_then(|v| v.parse().ok()).unwrap_or(3.0),
            "--eval" => a.evals.extend(it.next()),
            "--size" => {
                if let Some((w, h)) = it.next().and_then(|v| v.split_once('x').map(|(w, h)| (w.parse().ok(), h.parse().ok()))) {
                    if let (Some(w), Some(h)) = (w, h) {
                        a.size = (w, h);
                    }
                }
            }
            other => {
                eprintln!("lastbook: unknown option {other}");
                std::process::exit(2);
            }
        }
    }
    a
}

/// Where the app's own pages live, as the webview names them.
fn app_origin() -> &'static str {
    if cfg!(windows) {
        "http://lastbook.localhost/"
    } else {
        "lastbook://app/"
    }
}

fn to_engine(req: &wry::http::Request<Vec<u8>>) -> Request {
    let uri = req.uri();
    Request {
        method: req.method().as_str().to_string(),
        path: percent_decode(uri.path()),
        query: uri.query().unwrap_or("").to_string(),
        headers: req.headers().iter().map(|(k, v)| (k.as_str().to_string(), v.to_str().unwrap_or("").to_string())).collect(),
        body: req.body().clone(),
    }
}

fn to_webview(r: Response) -> wry::http::Response<Cow<'static, [u8]>> {
    let mut b = wry::http::Response::builder().status(r.status);
    for (k, v) in &r.headers {
        b = b.header(k.as_str(), v.as_str());
    }
    b.body(Cow::Owned(r.body)).unwrap_or_else(|_| wry::http::Response::builder().status(500).body(Cow::Borrowed(&b""[..])).unwrap())
}

/// Start again, as the person would by opening the app: it then finds the library setup has just put in place. Only --home
/// is kept. A test run (--snapshot) quits instead, and the test starts the app again itself.
fn relaunch(test_run: bool) -> ! {
    if test_run {
        println!("lastbook: setup finished");
        std::process::exit(0);
    }
    let exe = std::env::current_exe().expect("this program's path");
    let mut keep: Vec<String> = Vec::new();
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        if a == "--home" {
            keep.push(a);
            keep.extend(it.next());
        }
    }
    std::env::remove_var("LASTBOOK_DATA"); // the next start must find the library setup staged, not this run's
    eprintln!("lastbook: setup finished: starting again");
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let e = std::process::Command::new(&exe).args(&keep).exec();
        eprintln!("lastbook: could not start again: {e}");
        std::process::exit(1);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let _ = std::process::Command::new(&exe).args(&keep).creation_flags(0x00000008 | 0x00000200).spawn(); // DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP
        std::process::exit(0);
    }
}

/// The system's folder or file picker, opened only at the person's request.
fn pick(files: bool, start: &std::path::Path) -> Option<PathBuf> {
    let dialog = rfd::AsyncFileDialog::new().set_directory(start);
    let handle = if files { pollster::block_on(dialog.set_title("Choose a library zip or AI model file").pick_file()) } else { pollster::block_on(dialog.set_title("Choose a folder").pick_folder()) };
    handle.map(|h| h.path().to_path_buf())
}

fn show_error(message: &str) {
    eprintln!("lastbook: {message}");
    let _ = rfd::MessageDialog::new().set_title(dist::NAME).set_description(message).set_level(rfd::MessageLevel::Error).show();
}

fn main() {
    let args = parse_args();
    let (settings, appdata) = match &args.home {
        Some(h) => (h.join("settings"), h.join("data")),
        None => appdirs::app_folders(),
    };
    let given = args.data.clone().or_else(|| std::env::var_os("LASTBOOK_DATA").filter(|v| !v.is_empty()).map(PathBuf::from));
    let (lib, note) = setup_api::startup(&settings, given.as_deref()); // before anything is open
    if let (Some(lib), None) = (&lib, &given) {
        appdirs::remember(&settings, lib);
    }
    let data = appdirs::data_folder(args.data.as_deref(), &settings, &appdata);
    let data = match data {
        None => None,
        Some(d) if install::is_library(&d) => Some(d.canonicalize().unwrap_or(d)),
        Some(d) => {
            show_error(&format!("Lastbook could not find its library.\n\nIt looked in:\n{}\n\nThat folder must hold library-data/catalog.json.", d.display()));
            std::process::exit(2);
        }
    };
    let mode = if data.is_some() { Mode::Manage } else { Mode::FirstRun };
    if data.is_some() && (given.is_none() || note.is_some()) {
        setup_api::opened(&settings, note.as_ref(), None);
    }
    let (parent, why) = appdirs::default_choice(&appdata);
    let mut opts = Options::new(mode, settings.clone(), parent, data.clone());
    opts.default_why = why;
    let (s2, a2) = (settings.clone(), appdata.clone());
    opts.look_for_libraries = Some(Box::new(move || appdirs::libraries_here(&s2, &a2)));
    let a3 = appdata.clone();
    opts.pick_folder = Some(Box::new(move |files| pick(files, &a3)));
    let s3 = settings.clone();
    let test_run = args.snapshot.is_some();
    opts.on_finish = Some(Box::new(move |folder| {
        appdirs::remember(&s3, &folder);
        relaunch(test_run);
    }));
    let engine = Arc::new(Engine::new(Paths::discover(data)).with_setup(Setup::new(opts)));
    let start = args.url.clone().unwrap_or_else(|| app_origin().to_string());
    let allowed: Vec<String> = [Some(app_origin().to_string()), args.url.as_ref().map(|u| origin_of(u))].into_iter().flatten().collect();

    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    let proxy = event_loop.create_proxy();
    let window = WindowBuilder::new()
        .with_title(dist::NAME)
        .with_inner_size(LogicalSize::new(args.size.0, args.size.1))
        .build(&event_loop)
        .expect("the window could not be made");

    let handler_engine = engine.clone();
    let mut builder = WebViewBuilder::new()
        .with_asynchronous_custom_protocol(SCHEME.into(), move |_id, req, responder| {
            // each request on a thread of its own: a long one (setup's state, a page image) never holds up the window
            let engine = handler_engine.clone();
            let req = to_engine(&req);
            std::thread::spawn(move || responder.respond(to_webview(engine.handle(&req))));
        })
        .with_url(start)
        .with_navigation_handler(move |url| {
            let ok = url == "about:blank" || allowed.iter().any(|o| url.starts_with(o.as_str()));
            if !ok {
                eprintln!("lastbook: blocked a page load: {url}");
            }
            ok
        })
        .with_new_window_req_handler(|url, _| {
            eprintln!("lastbook: blocked a new window: {url}");
            NewWindowResponse::Deny
        });
    if args.snapshot.is_some() {     // test runs only: the page's errors come back to be printed
        builder = builder
            .with_initialization_script(
                "addEventListener('error', e => window.ipc.postMessage('error: ' + e.message + ' at ' + e.filename + ':' + e.lineno));\
                 addEventListener('unhandledrejection', e => window.ipc.postMessage('unhandled rejection: ' + (e.reason && e.reason.message || e.reason)));",
            )
            .with_ipc_handler(|req| eprintln!("lastbook: page {}", req.body()));
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    let webview = builder.build(&window).expect("the webview could not be made");
    #[cfg(target_os = "linux")]
    let webview = {
        use tao::platform::unix::WindowExtUnix;
        use wry::WebViewBuilderExtUnix;
        builder.build_gtk(window.default_vbox().expect("no GTK box")).expect("the webview could not be made")
    };

    if args.snapshot.is_some() {
        let p = proxy.clone();
        let after = args.after;
        let evals = args.evals.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs_f64(after));
            for js in evals {
                let _ = p.send_event(UserEvent::Eval(js));
                std::thread::sleep(Duration::from_secs(1));
            }
            let _ = p.send_event(UserEvent::Snapshot);
        });
    }

    let snapshot = args.snapshot.clone();
    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        match event {
            Event::WindowEvent { event: WindowEvent::CloseRequested, .. } => *control_flow = ControlFlow::Exit,
            Event::UserEvent(UserEvent::Eval(js)) => {
                if let Err(e) = webview.evaluate_script(&js) {
                    eprintln!("lastbook: script failed: {e}");
                }
            }
            Event::UserEvent(UserEvent::Snapshot) => {
                let path = snapshot.clone().expect("snapshot path");
                let p = proxy.clone();
                take_snapshot(&webview, path, move |result| {
                    let code = match result {
                        Ok(path) => {
                            println!("lastbook: snapshot written to {}", path.display());
                            0
                        }
                        Err(e) => {
                            eprintln!("lastbook: snapshot failed: {e}");
                            1
                        }
                    };
                    let _ = p.send_event(UserEvent::Quit(code));
                });
            }
            Event::UserEvent(UserEvent::Quit(code)) => std::process::exit(code),
            _ => {}
        }
    });
}

fn origin_of(url: &str) -> String {
    match url.find("://") {
        Some(i) => match url[i + 3..].find('/') {
            Some(j) => url[..i + 3 + j + 1].to_string(),
            None => format!("{url}/"),
        },
        None => url.to_string(),
    }
}

#[cfg(target_os = "macos")]
fn take_snapshot(webview: &wry::WebView, path: PathBuf, done: impl FnOnce(Result<PathBuf, String>) + 'static) {
    snapshot_macos::take(webview, path, done);
}

#[cfg(not(target_os = "macos"))]
fn take_snapshot(_webview: &wry::WebView, _path: PathBuf, done: impl FnOnce(Result<PathBuf, String>) + 'static) {
    done(Err("snapshots are made on macOS only for now".into()));
}
