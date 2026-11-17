//! Saved web pages keep their content and run none of their code. What their scripts used to unfold, nothing unfolds
//! any more, so it is unfolded here as the page is served: no library file changes and no page needs code of its own.
//! Encyclopedia articles lose their scripts, event handlers, script addresses and links to the web as they are served.
//! A convenience, not a defense: the Content-Security-Policy is what stops scripts. Ported from the original Lastbook's
//! engine/saved_pages.py, with lol_html in place of its byte patterns: the page is read as HTML, so a tag inside a
//! comment, a script or a textarea is not taken for one, and every byte not changed comes out as it went in.

use super::text::unescape;
use lol_html::html_content::{ContentType, Element};
use lol_html::{element, end_tag, ElementContentHandlers, HtmlRewriter, Selector, Settings};
use regex::bytes::Regex as BytesRegex;
use regex::Regex;
use std::borrow::Cow;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::LazyLock;

/// A larger page is served as it is, unless taking out its blank spreadsheet rows brings it under this.
pub const MAX_PAGE: usize = 16 * 1024 * 1024;
/// Larger than this and a page is not even read to find out.
pub const MAX_READ: usize = 64 * 1024 * 1024;
pub const MAX_TRANSCRIPT: usize = 4 * 1024 * 1024;

/// A saved spreadsheet whose rows name their cells and hold nothing: the largest page in the library is 99.7% these.
static BLANK_ROW: LazyLock<BytesRegex> = LazyLock::new(|| BytesRegex::new(r"<tr>(?:<td><small>[A-Z]+\d+</small>\s*</td>)+</tr>").unwrap());
static HINT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)preconnect|dns-prefetch|prefetch|prerender").unwrap());
static SCHEME: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^([a-z][a-z0-9+.-]*:|//)").unwrap());
static SCRIPT_URL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^[\x00-\x20]*(javascript|vbscript)[\x00-\x20]*:").unwrap());
static WEB_URL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^[\x00-\x20]*([a-z][a-z0-9+.\-]*:|[/\\][/\\])").unwrap());
static PAGE_ID: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^#[\w-]+$").unwrap());
static CLOSER_ID: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^close-mobile-[\w-]+$").unwrap());
static DOCTYPE: LazyLock<BytesRegex> = LazyLock::new(|| BytesRegex::new(r"(?i)<!doctype\b[^>]*>").unwrap());
static VTT_TAG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<[^>]+>").unwrap());

type Handlers<'h> = Vec<(Cow<'static, Selector>, ElementContentHandlers<'h>)>;

/// UTF-16: none of this would match, and the style block would break it.
fn utf16(data: &[u8]) -> bool {
    data[..data.len().min(512)].contains(&0)
}

/// The page through lol_html with these handlers; the page as it came if it cannot be read as HTML.
fn run(data: &[u8], handlers: Handlers) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() + 4096);
    // not strict: on the few constructs a streaming parser cannot be sure of, it goes on with its best reading
    let settings = handlers.into_iter().fold(Settings::new().with_strict(false), |s, h| s.append_element_content_handler(h));
    let mut rw = HtmlRewriter::new(settings, |c: &[u8]| out.extend_from_slice(c));
    let ok = rw.write(data).is_ok() && rw.end().is_ok();
    if ok {
        out
    } else {
        data.to_vec()
    }
}

/// `<link>` tags that ask the browser to reach for another address before anything loads: WebKit opens a socket for
/// preconnect even under a policy of default-src 'none'.
fn hints<'h>() -> (Cow<'static, Selector>, ElementContentHandlers<'h>) {
    element!("link[rel]", |el| {
        if el.get_attribute("rel").is_some_and(|r| HINT.is_match(&r)) {
            el.remove();
        }
        Ok(())
    })
}

/// A page with its connection hints taken out.
pub fn no_hints(data: &[u8]) -> Vec<u8> {
    if utf16(data) {
        return data.to_vec();
    }
    run(data, vec![hints()])
}

/// Whether an attribute value, once its entities are read, is a script address.
fn script_url(raw: &str) -> bool {
    (raw.contains(':') || raw.contains('&')) && SCRIPT_URL.is_match(&unescape(raw.trim_matches(|c| " \t\r\n\"'".contains(c))))
}

/// One encyclopedia article or drawing, with its script elements, event handlers and script addresses taken out, a
/// link to the web made plain text (its tag and end tag go, its words stay), and every other byte left as it came. A
/// `<script>` that never closes runs to the end of the page in a browser too, so what it would have hidden goes with it.
pub fn article(data: &[u8]) -> Vec<u8> {
    if utf16(data) {
        return data.to_vec();
    }
    run(
        data,
        vec![
            hints(),
            element!("script", |el| {
                el.remove();
                Ok(())
            }),
            element!("a, area", |el| {
                let web = el.attributes().iter().any(|a| matches!(a.name().as_str(), "href" | "xlink:href") && WEB_URL.is_match(&unescape(&a.value())));
                if web {
                    if el.tag_name() == "a" {
                        unwrap(el, None);
                    } else {
                        el.remove();
                    }
                }
                Ok(())
            }),
            element!("*", |el| {
                if el.removed() {
                    return Ok(());
                }
                let bad: Vec<String> = el
                    .attributes()
                    .iter()
                    .filter(|a| {
                        let n = a.name();
                        (n.starts_with("on") && n.len() > 2) || script_url(&a.value())
                    })
                    .map(|a| a.name())
                    .collect();
                for n in bad {
                    el.remove_attribute(&n);
                }
                Ok(())
            }),
        ],
    )
}

/// An encyclopedia page (HTML) with a style block added at the end of its head, or at its end when it has no head.
pub fn article_look(data: &[u8], look: &str) -> Vec<u8> {
    let upto = data.iter().rposition(|&b| b == b'>').map(|i| i + 1).unwrap_or(data.len());
    let at = find_ci(&data[..upto], b"</head>");
    let mut out = Vec::with_capacity(data.len() + look.len());
    match at {
        Some(i) => {
            out.extend_from_slice(&data[..i]);
            out.extend_from_slice(look.as_bytes());
            out.extend_from_slice(&data[i..]);
        }
        None => {
            out.extend_from_slice(data);
            out.extend_from_slice(look.as_bytes());
        }
    }
    out
}

/// One of Lastbook's own pages, with its look.
pub fn own_look(data: &[u8]) -> Vec<u8> {
    article_look(data, OWN_LOOK)
}

/// The first place a word is, in any case.
fn find_ci(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w.eq_ignore_ascii_case(needle))
}

/// A WebVTT file's words, one line each time it changes.
pub fn transcript_text(vtt: &[u8]) -> String {
    let text = String::from_utf8_lossy(vtt);
    let mut out: Vec<String> = Vec::new();
    let mut last: Option<String> = None;
    for line in text.lines() {
        let s = line.trim();
        if s.is_empty() || ["WEBVTT", "NOTE", "Kind:", "Language:"].iter().any(|p| s.starts_with(p)) || s.contains("-->") || s.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        let s = VTT_TAG.replace_all(s, "").into_owned();
        if last.as_deref() != Some(s.as_str()) {
            out.push(s.clone());
        }
        last = Some(s);
    }
    out.join(" ")
}

/// Text for HTML, escaped as the original Lastbook escaped it.
pub fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&#x27;")
}

/// What a saved page is rewritten with: the bytes of a file it names (its transcript), and the library's own copy of an
/// address it links to.
pub struct Local<'a> {
    pub read: &'a dyn Fn(&str) -> Option<Vec<u8>>,
    pub link: &'a dyn Fn(&str) -> Option<String>,
}

/// The first `<track>` with a `.vtt` source.
fn first_track(data: &[u8]) -> Option<String> {
    find_ci(data, b"<track")?;
    let found: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
    let f = found.clone();
    run(
        data,
        vec![element!("track[src]", move |el| {
            if f.borrow().is_none() {
                if let Some(src) = el.get_attribute("src").filter(|s| s.to_ascii_lowercase().ends_with(".vtt") && !s.contains(['"', '\'', '>'])) {
                    *f.borrow_mut() = Some(src);
                }
            }
            Ok(())
        })],
    );
    found.take()
}

/// A saved page as it is served: no connection hints; local links open where they are (no target, no download); a
/// link to an address the library holds goes to its copy; image galleries show their pictures; slide-in drawers open by
/// a plain in-page link; the transcript its player loaded is shown under the video; and the style that unfolds what
/// its scripts used to. Nothing here adds a script or an address outside the library.
pub fn rewrite(data: &[u8], local: Option<&Local>) -> Vec<u8> {
    if utf16(data) {
        return data.to_vec();
    }
    if data.len() > MAX_PAGE {
        let data = BLANK_ROW.replace_all(data, &b""[..]);
        if data.len() > MAX_PAGE {
            return no_hints(&data);
        }
        return rewrite(&data, local);
    }
    let block = local.and_then(|l| first_track(data).and_then(|src| (l.read)(&src))).map(|vtt| {
        format!("<details class=\"lb-transcript\" open><summary>Transcript</summary><p>{}</p></details>", escape(&transcript_text(&vtt)))
    });
    let gallery: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
    let placed = Rc::new(RefCell::new(block.is_none()));
    let head = Rc::new(RefCell::new(false));
    let mut handlers: Handlers = vec![hints()];
    {
        let gallery = gallery.clone();
        handlers.push(element!("div.image-gallery[data-base-url]", move |el| {
            gallery.borrow_mut().push(el.get_attribute("data-base-url").unwrap_or_default());
            let g = gallery.clone();
            let _ = el.on_end_tag(end_tag!(move |_| {
                g.borrow_mut().pop();
                Ok(())
            }));
            Ok(())
        }));
    }
    {
        let gallery = gallery.clone();
        handlers.push(element!("a", move |el| {
            let Some(mut href) = el.get_attribute("href") else { return Ok(()) };
            if let Some(base) = gallery.borrow().last() {
                // a gallery's link to a picture: the picture, with the link's words under it
                el.before(&format!("<figure style=\"margin:1em 0\"><img style=\"max-width:100%\" alt=\"\" src=\"{}\"><figcaption>", attr(&format!("{base}{href}"))), ContentType::Html);
                unwrap(el, Some("</figcaption></figure>"));
                return Ok(());
            }
            if let Some(l) = local {
                let decoded = unescape(&href);
                let (base, frag) = decoded.split_once('#').unwrap_or((&decoded, ""));
                if let Some(path) = (l.link)(base) {
                    href = format!("/{path}{}", if frag.is_empty() { String::new() } else { format!("#{frag}") });
                    el.set_attribute("href", &href)?;
                }
            }
            if !SCHEME.is_match(href.trim_start()) {
                // a local link opens where it is: in a saved page target= and download= only make it do nothing
                for a in ["target", "download"] {
                    if el.has_attribute(a) {
                        el.remove_attribute(a);
                    }
                }
            }
            Ok(())
        }));
    }
    handlers.push(element!("button", drawer));
    if let Some(block) = block.clone() {
        // at the start of the video tab's section where the page has one, else after the first video
        let placed = placed.clone();
        if find_ci(data, b"video-tab-content-section").is_some() {
            handlers.push(element!("*", move |el| {
                if !*placed.borrow() && el.attributes().iter().any(|a| a.value().contains("video-tab-content-section")) {
                    el.prepend(&block, ContentType::Html);
                    *placed.borrow_mut() = true;
                }
                Ok(())
            }));
        } else {
            handlers.push(element!("video", move |el| {
                if !*placed.borrow() {
                    el.after(&block, ContentType::Html);
                    *placed.borrow_mut() = true;
                }
                Ok(())
            }));
        }
    }
    {
        let head = head.clone();
        handlers.push(element!("head", move |el| {
            if !*head.borrow() {
                el.prepend(CSS, ContentType::Html);
                *head.borrow_mut() = true;
            }
            Ok(())
        }));
    }
    let out = run(data, handlers);
    if *head.borrow() {
        return out;
    }
    // no head: after the doctype, else at the start
    let at = DOCTYPE.find(&out).map(|m| m.end()).unwrap_or(0);
    let mut v = Vec::with_capacity(out.len() + CSS.len());
    v.extend_from_slice(&out[..at]);
    v.extend_from_slice(CSS.as_bytes());
    v.extend_from_slice(&out[at..]);
    v
}

/// Takes off an element's tags and keeps what is inside, with `close` where its end tag was. Only its own end tag
/// goes: lol_html gives an element closed by its parent's end tag (an `<a>` by `</p>`) that end tag, which stays.
fn unwrap(el: &mut Element, close: Option<&'static str>) {
    let name = el.tag_name();
    el.start_tag().remove();
    let _ = el.on_end_tag(end_tag!(move |end| {
        if let Some(c) = close {
            end.before(c, ContentType::Html);
        }
        if end.name() == name {
            end.remove();
        }
        Ok(())
    }));
}

/// A value for a double-quoted attribute this file writes.
fn attr(s: &str) -> String {
    s.replace('"', "&quot;").replace('<', "&lt;").replace('>', "&gt;")
}

/// A slide-in drawer's button becomes a plain in-page link the page's CSS opens by :target, as does its close button.
fn drawer(el: &mut Element) -> lol_html::HandlerResult {
    let opens = el.get_attribute("data-toggle").is_some_and(|v| v.eq_ignore_ascii_case("offcanvas")).then(|| el.get_attribute("data-target")).flatten().filter(|t| PAGE_ID.is_match(t));
    let closer = el.get_attribute("id").is_some_and(|i| CLOSER_ID.is_match(&i));
    if opens.is_none() && !closer {
        return Ok(());
    }
    // href first, then the button's own attributes as they were
    let attrs: Vec<(String, String)> = el.attributes().iter().map(|a| (a.name(), a.value())).collect();
    for (n, _) in &attrs {
        el.remove_attribute(n);
    }
    el.start_tag().set_name("a");
    let _ = el.on_end_tag(end_tag!(|end| {
        if end.name() == "button" {
            end.set_name("a");
        }
        Ok(())
    }));
    el.set_attribute("href", opens.as_deref().unwrap_or("#"))?;
    for (n, v) in &attrs {
        el.set_attribute(n, v)?;
    }
    Ok(())
}

/// What a saved page's scripts used to unfold, unfolded by style. :where() keeps the specificity at zero, so a page's own responsive
/// !important rules still win; the description's ids need their weight to beat .d-none!important.
pub const CSS: &str = r###"<style>
:where(.collapse:not(.navbar-collapse),.collapsing,.tab-pane,.accordion-collapse){display:block!important;height:auto!important;visibility:visible!important}
:where(.partial-collapse){height:auto!important;overflow:visible!important}
:where(.partial-collapse-overlay){display:none!important}
:where(button)[data-toggle="collapse"]:not(:has(span)),:where(button)[data-bs-toggle="collapse"]:not(:has(span)){display:none!important}
:where(button[data-toggle],button[data-bs-toggle]){pointer-events:none}
:where(a[data-toggle="collapse"],a[data-bs-toggle="collapse"]){pointer-events:none;color:inherit;text-decoration:none}
.navbar-offcanvas:target{left:0!important;transform:none!important;opacity:1!important;box-shadow:0 0 20px #0000004d}
:where(.navbar-toggler){display:none!important}
#expanded-description{display:block!important}
#collapsed-description:has(~#expanded-description),#collapse-description{display:none!important}
:where(a.offcanvas-toggle,a[id^="close-mobile-"]){pointer-events:auto;cursor:pointer}
:where(.video-tab-download-popup){position:static!important;visibility:visible!important;width:auto!important;box-shadow:none!important;border:0!important}
:where(.download-menu-submenu){display:block!important}
:where(.video-download-icons,.download-transcript-submenu-btn,.download-submenu-back-btn,.video-tab-toggle-section .tab-toggle-button){display:none!important}
:where(.download-menu-main a[href^="http"]){display:none!important}
:where(.download-menu-submenu a)::before{content:"Transcript PDF: "}
.lb-transcript{margin:.8em 0;font:inherit;line-height:1.5}
.lb-transcript summary{cursor:pointer;font-weight:600}
</style>"###;

/// How an encyclopedia article looks inside the app: its title, text and links in the window's own font, colours and accent,
/// its left edge on the page's, and the window's colour behind it. The page showing the article sets the --lb-* values
/// (web/app.js, lookOf); these are what shows before it does, and in a page opened on its own.
pub const ARTICLE_LOOK: &str = r###"<style id="lb-article">
:root{--lb-ink:#1d1d1f;--lb-ink2:#55555a;--lb-line:#e6e6e9;--lb-link:#10805c;--lb-font:-apple-system,BlinkMacSystemFont,"Segoe UI Variable Text","Segoe UI",system-ui,sans-serif;--lb-display:-apple-system,BlinkMacSystemFont,"Segoe UI Variable Display","Segoe UI",system-ui,sans-serif}
@media (prefers-color-scheme:dark){:root{--lb-ink:#f2f2f3;--lb-ink2:#c2c2c7;--lb-line:#313134;--lb-link:#3cc690}}
html:root:not(#lb){--color-base:var(--lb-ink)!important;--color-emphasized:var(--lb-ink)!important;--color-subtle:var(--lb-ink2)!important;--color-progressive:var(--lb-link)!important;--color-progressive--hover:var(--lb-link)!important;--color-progressive--active:var(--lb-link)!important;--color-progressive--focus:var(--lb-link)!important;--color-visited:var(--lb-link)!important;--color-link:var(--lb-link)!important;--color-link--visited:var(--lb-link)!important;--border-color-base:var(--lb-line)!important;--border-color-subtle:var(--lb-line)!important;--background-color-base:transparent!important;--background-color-interactive-subtle:transparent!important}
html:not(#lb),html:not(#lb) body,.mw-page-container,.mw-page-container-inner,.mw-content-container,.mw-body,.vector-body{background:transparent!important}
html:not(#lb) body{margin:0!important;padding:0!important;color:var(--lb-ink)!important;font-family:var(--lb-font)!important}
.mw-page-container,.mw-page-container-inner,.mw-content-container,.mw-body,#content,.vector-body,#bodyContent,.mw-body-header,.vector-page-titlebar{margin-left:0!important;margin-right:0!important;padding-left:0!important;padding-right:0!important;max-width:none!important;border:0!important;box-shadow:none!important}
.mw-page-container{padding-top:0!important}
.vector-page-titlebar::after,.mw-body-header::after{display:none!important}
.mw-body,.mw-body-content,.vector-body,.mw-parser-output,#content,#bodyContent{font-family:var(--lb-font)!important;color:var(--lb-ink)}
.mw-body-content,.vector-body{font-size:18px!important;line-height:1.6!important}
h1,h2,h3,h4,h5,h6,.mw-body h1,.mw-body-content h1,.mw-body-content h2,.mw-body-content h3,.mw-heading h2,.mw-heading h3{font-family:var(--lb-display)!important;color:var(--lb-ink)!important}
#firstHeading,.firstHeading{font-weight:700!important;font-size:34px!important;line-height:1.15!important;letter-spacing:-.018em;margin:.1em 0 .3em!important;padding:0!important;border:0!important}
.mw-body-content h2,.mw-heading2 h2,.mw-heading2{font-weight:650!important;font-size:22px!important;line-height:1.25!important;border-bottom-color:var(--lb-line)!important}
.mw-body-content h3,.mw-heading3 h3{font-weight:650!important;font-size:19px!important}
a,a:visited,a.new,a.extiw,.mw-body-content a:not(.mw-selflink),.mw-body-content a:visited{color:var(--lb-link)!important}
.hatnote,.dablink,.rellink{background:none!important;color:var(--lb-ink2)!important}
html:not(#lb) .hatnote,html:not(#lb) .dablink,html:not(#lb) .rellink{color:var(--lb-ink2)!important}
.mw-parser-output table{font-size:15.5px}
.navbox{font-size:14px!important}
.mw-parser-output .navbox table{font-size:inherit}
div.navbox{overflow-x:auto}
ul.gallery{margin-left:0!important;margin-right:0!important;max-width:100%}
li.gallerybox,li.gallerybox .thumb,li.gallerybox .gallerytextwrapper{max-width:100%!important;box-sizing:border-box}
.mw-parser-output img:not(table img){max-width:100%;height:auto}   /* not in a table: there a share of the cell's width collapses a picture in a cell sized by its content, and the table's box scrolls instead */
.smooth-pie{overflow:hidden}
.mw-parser-output{overflow-wrap:break-word}
.references,.reflist,.reference-text{overflow-wrap:anywhere}
/* a table or picture group wider than the column scrolls inside its own box, so the article never scrolls sideways. A table keeps its
   table layout: web/app.js (tidyArticle) puts it in a .lb-wide box, which fades at an edge that has more to scroll to */
.lb-wide{max-width:100%;overflow-x:auto;overflow-y:hidden;position:relative}
div.navbox{position:relative}   /* a screen-reader label placed absolutely inside stays inside the box, and does not widen the page */
@media (max-width:639px){html[data-lb-tidy] table{display:table}}   /* the skin makes every table a block on a narrow page, which fixes its width; the box scrolls it instead */
/* a narrow page: a formula set on one line may break at its spaces, and a box set to a share of the width counts its padding in it, so neither runs past the column */
@media (max-width:639px){.mw-parser-output .chemf,.mw-parser-output .texhtml{white-space:normal!important}.mw-parser-output div[style*="width"][style*="%"]{box-sizing:border-box}}
.lb-wide.lb-r{-webkit-mask-image:linear-gradient(to left,transparent,#000 36px);mask-image:linear-gradient(to left,transparent,#000 36px)}
.lb-wide.lb-l{-webkit-mask-image:linear-gradient(to right,transparent,#000 36px);mask-image:linear-gradient(to right,transparent,#000 36px)}
.lb-wide.lb-l.lb-r{-webkit-mask-image:linear-gradient(to right,transparent,#000 36px,#000 calc(100% - 36px),transparent);mask-image:linear-gradient(to right,transparent,#000 36px,#000 calc(100% - 36px),transparent)}
/* the web page's editing links do nothing offline: [edit], and a template's view / talk / edit row */
.mw-editsection,.mw-editsection-like,.mw-parser-output .navbar,.mw-parser-output .sidebar-navbar{display:none!important}
.mw-parser-output img:is([src*="OOjs_UI_icon_edit"],[src$="Red_Pencil_Icon.png"],[src$="Blue_pencil.svg.png"]){display:none!important}   /* the pencil that edited an infobox or taxobox */
/* a paragraph set in small type (the front page's licence note) gets line spacing for that type, not the body's */
.mw-parser-output p:has(>span:is([style*="font-size:10px"],[style*="font-size: 10px"],[style*="font-size:11px"],[style*="font-size: 11px"],[style*="font-size:12px"],[style*="font-size: 12px"],[style*="font-size:13px"],[style*="font-size: 13px"],[style*="font-size:small"],[style*="font-size: small"],[style*="font-size:x-small"],[style*="font-size: x-small"],[style*="font-size:80%"],[style*="font-size: 80%"],[style*="font-size:85%"],[style*="font-size: 85%"],[style*="font-size:90%"],[style*="font-size: 90%"]):only-child){font-size:14.5px;line-height:1.5;color:var(--lb-ink2);margin-top:1.75em}
.mw-parser-output p>span:is([style*="font-size:10px"],[style*="font-size: 10px"],[style*="font-size:11px"],[style*="font-size: 11px"],[style*="font-size:12px"],[style*="font-size: 12px"],[style*="font-size:13px"],[style*="font-size: 13px"],[style*="font-size:small"],[style*="font-size: small"],[style*="font-size:x-small"],[style*="font-size: x-small"],[style*="font-size:80%"],[style*="font-size: 80%"],[style*="font-size:85%"],[style*="font-size: 85%"],[style*="font-size:90%"],[style*="font-size: 90%"]):only-child{font-size:inherit!important}
.mw-parser-output .thumbinner{max-width:100%!important;overflow-x:auto}
/* the encyclopedia's licence line at the end of every page, in the same small secondary type as the front page's own licence note */
html:not(#lb) .zim-footer{font-size:14.5px!important;line-height:1.5!important;color:var(--lb-ink2)!important;border-top:1px solid var(--lb-line)!important;margin:2.5em 0 1em!important;padding:.9em 0 0!important}
/* the front page's logo is a picture on white: in dark mode it shows as a tile, with rounded corners, not a white square */
@media (prefers-color-scheme:dark){.mw-parser-output img[src$="/MedLogoNoWiFi.png"]{border-radius:18%}}
.mw-parser-output div[style*="inline-table"]{overflow-wrap:anywhere;display:inline-block!important;max-width:100%}   /* tables set side by side: each fits the column, and one wider scrolls in its box */
.mw-parser-output math:not([display=block]){display:inline-block;vertical-align:middle}
.mw-parser-output img.mwe-math-fallback-image-display:not(table img){max-width:calc(100% - 1.6em)}   /* a formula set on its own line is indented 1.6em by the skin: with that, it still fits the column */
.mw-parser-output dd dd{margin-left:min(1.6em,8%)}   /* indentation many levels deep (':::: ...') takes a share of what is left, so the text never runs out of room and past the column */
.mw-parser-output math{max-width:100%;overflow-x:auto;overflow-y:hidden}
@media (min-width:760px){.mw-parser-output .thumb.tright{float:right!important;clear:right;width:auto!important;margin:.3em 0 1em 1.5em!important}}
</style>"###;

/// Lastbook's own pages in a pack (Sources and permissions): the window's font and colours, light and dark, clear so the window
/// shows behind, and disclosure rows with the app's chevron.
pub const OWN_LOOK: &str = r###"<style id="lb-own">
:root{color-scheme:light dark;--ink:#1d1d1f;--ink2:#55555a;--line:#e6e6e9;--link:#10805c}
@media (prefers-color-scheme:dark){:root{--ink:#f2f2f3;--ink2:#c2c2c7;--line:#313134;--link:#3cc690}}
html{background:transparent}
body{font:16px/1.55 -apple-system,BlinkMacSystemFont,"Segoe UI Variable Text","Segoe UI",system-ui,sans-serif!important;color:var(--ink);background:transparent;max-width:46em;margin:4px 0 24px!important;padding:0 2px!important;-webkit-font-smoothing:antialiased}
body>h1:first-child{display:none}
h2{font:650 19px/1.3 -apple-system,BlinkMacSystemFont,"Segoe UI Variable Display","Segoe UI",system-ui,sans-serif;margin:1.5em 0 .3em}
p{margin:.4em 0}a{color:var(--link)}
details{border-bottom:1px solid var(--line);padding:4px 0 8px}
summary{list-style:none;color:var(--ink2);font-size:15px;padding:4px 0}summary::-webkit-details-marker{display:none}
summary::before{content:"";display:inline-block;width:7px;height:7px;margin:0 10px 0 2px;border-right:2px solid currentColor;border-bottom:2px solid currentColor;transform:rotate(-45deg);vertical-align:2px;transition:transform .15s}
details[open]>summary::before{transform:rotate(45deg)}
ul{color:var(--ink2);font-size:14.5px;overflow-wrap:anywhere;padding-left:22px}
</style>"###;

#[cfg(test)]
mod tests {
    use super::*;

    fn has(hay: &[u8], needle: &[u8]) -> bool {
        hay.windows(needle.len()).any(|w| w == needle)
    }

    #[test]
    fn article_scripts_go_and_the_rest_stays() {
        // test_what_ships.py Article
        let page = concat!(
            "<!doctype html><html><head><title>x <script> y</title><script src=\"/-/a.js\"></script><script>var a=\"</p>\"</script></head>",
            "<body><p onclick=\"go()\" class=\"k\" data-on=\"1\">Text <a href=\"javascript:alert(1)\">one</a> <a href=\"java&#115;cript:x\">two</a> ",
            "<a href=\"Other_page\">three</a></p><!-- <script>kept</script> --><table><tr><td>cell</td></tr></table><img src=\"I/p.png\" alt=\"\">",
            "<svg><script/><circle onload=\"x\" r=\"1\"/></svg><textarea><script>as text</script></textarea></body></html>"
        );
        let out = article(page.as_bytes());
        for gone in ["/-/a.js", "var a", "onclick", "onload", "javascript", "cript:x"] {
            assert!(!has(&out, gone.as_bytes()), "{gone}: {}", String::from_utf8_lossy(&out));
        }
        for kept in ["<title>x <script> y</title>", "class=\"k\" data-on=\"1\"", "<a href=\"Other_page\">three</a>", "<!-- <script>kept</script> -->", "<td>cell</td>",
                     "<img src=\"I/p.png\" alt=\"\">", "<textarea><script>as text</script></textarea>", "Text one two "] {
            assert!(has(&out, kept.as_bytes()), "{kept}: {}", String::from_utf8_lossy(&out));
        }
    }

    #[test]
    fn article_web_links_become_plain_text() {
        let page = concat!(
            "<p>See <a rel=\"nofollow\" class=\"external text\" href=\"https://www.who.int/x\">the WHO page</a>, ",
            "<a href='HTTP://a.b'>b</a> <a href=\"//c.d/e\">c</a> <a href=\"mailto:x@y.z\">mail</a> <a href=\"&#104;ttps://f.g\">h</a> ",
            "<a href=\"Local_page\">local</a> <a href=\"../A/Other#s\">other</a> <a href=\"#cite_note-1\">[1]</a> ",
            "<map><area shape=\"rect\" href=\"https://m.n\" alt=\"m\"></map><svg><a xlink:href=\"http://s.t\"><text>s</text></a></svg>",
            "<a href=\"https://u.v\">never closed</p><p>next</p></body>"
        );
        let out = article(page.as_bytes());
        for gone in ["who.int", "a.b", "c.d", "mailto", "f.g", "m.n", "s.t", "u.v"] {
            assert!(!has(&out, gone.as_bytes()), "{gone}: {}", String::from_utf8_lossy(&out));
        }
        for kept in ["<p>See the WHO page, b c mail h ", "<a href=\"Local_page\">local</a>", "<a href=\"../A/Other#s\">other</a>", "<a href=\"#cite_note-1\">[1]</a>",
                     "<map></map>", "<svg><text>s</text></svg>", "never closed</p><p>next</p>"] {
            assert!(has(&out, kept.as_bytes()), "{kept}: {}", String::from_utf8_lossy(&out));
        }
    }

    #[test]
    fn a_script_that_never_closes_takes_the_rest_as_a_browser_would() {
        assert_eq!(article(b"<p>before</p><script>never closed <p>hidden"), b"<p>before</p>");
        assert_eq!(article(b"<p>x</p><script>a</script"), b"<p>x</p>");
    }

    #[test]
    fn a_page_with_nothing_to_take_is_unchanged() {
        let page = b"<!doctype html><p class=a>plain &amp; simple <a href=\"x\">x</a></p><p title=\">\">";
        assert_eq!(article(page), page);
        let utf16: Vec<u8> = "<script>x</script>".encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
        assert_eq!(article(&utf16), utf16);
        assert_eq!(rewrite(&utf16, None), utf16);
    }

    #[test]
    fn looks_go_at_the_end_of_the_head() {
        let out = article_look(b"<html><HEAD><title>t</title></HEAD><body>x</body></html>", ARTICLE_LOOK);
        assert!(has(&out, format!("{ARTICLE_LOOK}</HEAD>").as_bytes()));
        assert!(own_look(b"<p>no head").ends_with(OWN_LOOK.as_bytes()));
    }

    #[test]
    fn connection_hints_go() {
        let page = b"<!doctype html><html><head><link rel=preconnect href=\"http://x.invalid\"><link href=\"//y.invalid\" rel=\"dns-prefetch\"><LINK REL='Prefetch' href=/z>\n\
                     <link rel=\"prerender\" href=/q><link rel=\"stylesheet\" href=\"keep.css\"><link rel=\"canonical\" href=\"/keep\"></head><body><p>rel=preconnect stays as text</p></body></html>";
        let out = rewrite(page, None).to_ascii_lowercase();
        for gone in ["x.invalid", "y.invalid", "prerender", "'prefetch'"] {
            assert!(!has(&out, gone.as_bytes()), "{gone}");
        }
        for kept in ["keep.css", "href=\"/keep\"", "rel=preconnect stays as text"] {
            assert!(has(&out, kept.as_bytes()), "{kept}");
        }
    }

    #[test]
    fn a_saved_page_comes_with_what_its_scripts_unfolded() {
        let vtt = b"WEBVTT\n\n00:01.000 --> 00:02.000\nHello <b>there</b> & welcome\n\n00:02.000 --> 00:03.000\nHello <b>there</b> & welcome\n".to_vec();
        let read = move |src: &str| if src == "talk.vtt" { Some(vtt.clone()) } else { None };
        let link = |u: &str| if u == "https://example.org/guide" { Some("pages/guide.html".to_string()) } else { None };
        let local = Local { read: &read, link: &link };
        let page = concat!(
            "<!doctype html><html><head><title>t</title></head><body><a href=\"x.pdf\" target=\"_blank\" download>f</a><a href=\"https://e.example/\" target=\"_blank\">w</a>",
            "<a href=\"https://example.org/guide#part\" target=\"_blank\">copy</a>",
            "<button data-toggle=\"offcanvas\" data-target=\"#menu\">m</button><button id=\"close-mobile-menu\">x</button><video><track src=\"talk.vtt\"></video>",
            "<video><track src=\"../../../secret.vtt\"></video><div class=\"image-gallery\" data-base-url=\"/img/\"><a href=\"p1.jpg\">First</a></div></body></html>"
        );
        let out = rewrite(page.as_bytes(), Some(&local));
        let s = String::from_utf8_lossy(&out);
        assert!(s.to_lowercase().find("<head>").unwrap() < s.find("<style>").unwrap());
        assert!(s.contains(".collapse"));
        assert!(Regex::new(r#"<a href="x.pdf"\s*>"#).unwrap().is_match(&s), "{s}");
        assert!(s.contains("https://e.example/\" target=\"_blank\""), "still does nothing, but the page is not changed for it");
        assert!(s.contains("<a href=\"/pages/guide.html#part\">copy</a>"), "the library's own copy, opened where it is: {s}");
        assert!(s.contains("<a href=\"#menu\" data-toggle=\"offcanvas\" data-target=\"#menu\">m</a>"), "{s}");
        assert!(s.contains("<a href=\"#\" id=\"close-mobile-menu\">x</a>"), "{s}");
        assert_eq!(s.matches("Hello there &amp; welcome").count(), 1);
        assert!(s.contains("</video><details class=\"lb-transcript\" open>"));
        assert!(s.contains("<figure style=\"margin:1em 0\"><img style=\"max-width:100%\" alt=\"\" src=\"/img/p1.jpg\"><figcaption>First</figcaption></figure>"), "{s}");
        assert!(!s.contains("<script"));
        // with no head the style goes after the doctype
        let out = rewrite(b"<!DOCTYPE html><p>x", None);
        assert!(out.starts_with(format!("<!DOCTYPE html>{CSS}<p>x").as_bytes()));
    }

    #[test]
    fn blank_rows_go_only_when_the_page_is_too_large() {
        let blank = b"<tr><td><small>A1</small> </td><td><small>B1</small> </td></tr>";
        let real = b"<tr><td><small>A2</small> water</td><td><small>B2</small> </td></tr>";
        let small = [&b"<table>"[..], blank, real, b"</table>"].concat();
        assert!(has(&rewrite(&small, None), blank));
        let mut big = b"<html><head></head><table>".to_vec();
        for _ in 0..MAX_PAGE / blank.len() + 1 {
            big.extend_from_slice(blank);
        }
        big.extend_from_slice(real);
        big.extend_from_slice(b"</table>");
        let out = rewrite(&big, None);
        assert!(!has(&out, blank));
        assert!(has(&out, real));
        assert!(has(&out, CSS.as_bytes()));
        let mut full = b"<html><head></head>".to_vec();
        full.resize(full.len() + MAX_PAGE, b'x');
        assert_eq!(rewrite(&full, None), full);
    }

    #[test]
    fn linear_time_on_tags_that_never_close() {
        // test_bridge.py: a whole page against 1/16 of one. In step with its size it takes 16 times as long; quadratic, 256.
        let read = |_: &str| Some([&b"WEBVTT\n"[..], &vec![b'<'; 1024 * 1024]].concat());
        let link = |_: &str| None;
        let local = Local { read: &read, link: &link };
        let size = 2 * 1024 * 1024;
        let took = |unit: &[u8], n: usize| {
            let data: Vec<u8> = unit.iter().copied().cycle().take(n / unit.len() * unit.len()).collect();
            let t = std::time::Instant::now();
            rewrite(&data, Some(&local));
            article(&data);
            t.elapsed().as_secs_f64()
        };
        for unit in [&b"<button x>"[..], b"<a x>", b"<a ", b"<head ", b"<track src=\"a.vtt ", b"<track ", b"<div class=\"image-gallery\" data-base-url=\"x\">",
                     b"<div class=\"image-gallery ", b"<!doctype ", b"<a href=x target=y download ", b"<div class=\"image-gallery\" data-base-url=\"x\"><a ",
                     b"<tr><td><small>A1</small> </td>", b"<a \"", b"<script>"] {
            let small = (0..3).map(|_| took(unit, size / 16)).fold(f64::MAX, f64::min);
            let whole = took(unit, size);
            assert!(whole < 4.0 * 16.0 * small + 0.05, "{}: {small} {whole}", String::from_utf8_lossy(unit));
        }
    }

    #[test]
    fn transcript_lines() {
        assert_eq!(transcript_text(b"WEBVTT\nKind: captions\n\n1\n00:01.000 --> 00:02.000\n<v A>Hi</v> there\nHi there\nNext"), "Hi there Next");
    }
}
