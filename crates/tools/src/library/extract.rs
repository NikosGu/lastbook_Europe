//! The words of a document, page by page, for the search index and the reading page's text view. A PDF's through
//! PDFium (the engine's own loading: beside the program, or the folder in LASTBOOK_PDFIUM), with what a tagged PDF says
//! its pictures show (their alt text) on a page that has few words of its own; a saved web page's through lol_html,
//! without what is not its text (scripts, styles, menus, headers, footers and forms), as the original Lastbook took it
//! with BeautifulSoup.

use lol_html::{doc_text, element, HtmlRewriter, Settings};
use pdfium_render::prelude::*;
use std::cell::RefCell;
use std::ffi::{c_int, c_ulong, c_void};
use std::path::Path;
use std::rc::Rc;

/// What is left out of a saved page's text.
const NOT_TEXT: &str = "script, style, nav, footer, header, form, noscript, template";

/// One page's text as the library keeps it: lines as they came, with the line ends of every system as "\n", no form
/// feed (it separates pages), and no other control character. PDFium marks a word hyphenated at the end of a line with
/// U+0002: the word is joined again, so "approvvi-gionamento" is found as approvvigionamento.
pub fn clean(s: &str) -> String {
    let s = s.replace("\r\n", "\n").replace(['\r', '\u{c}'], "\n").replace("\u{2}\n", "").replace('\u{2}', "");
    s.chars().map(|c| if c.is_control() && c != '\n' && c != '\t' { ' ' } else { c }).collect::<String>().trim_end().to_string()
}

/// A page with fewer letters and digits of its own than this is mostly a picture: an infographic, a map, a table drawn
/// as an image. What its tagged PDF says of its pictures is added to its words.
pub const FEW_LETTERS: usize = 200;

/// One page of a document: its own text and, when it has little, the alt text of its pictures.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Page {
    pub text: String,
    pub alt: Vec<String>,
}

impl Page {
    /// A page that is only its text.
    pub fn of(text: String) -> Page {
        Page { text, alt: Vec::new() }
    }

    fn joined(&self, alt: impl Fn(&str) -> String) -> String {
        let own = Some(self.text.clone()).filter(|t| !t.is_empty());
        own.into_iter().chain(self.alt.iter().map(|a| alt(a))).collect::<Vec<_>>().join("\n")
    }

    /// As the page texts keep it: the page's own text, then each alt text on a line of its own, marked
    /// `[Alt text: …]` so a reader can tell a picture's description from the words printed on the page.
    pub fn kept(&self) -> String {
        self.joined(|a| format!("[Alt text: {a}]"))
    }

    /// As the search index has it: the same words without the marks, so "alt" and "text" find nothing they do not say.
    pub fn words(&self) -> String {
        self.joined(str::to_string)
    }
}

/// How many letters and digits a text has.
pub fn letters(s: &str) -> usize {
    s.chars().filter(|c| c.is_alphanumeric()).count()
}

/// Each page. An error says what is wrong in words: no PDFium here, or a file PDFium cannot open.
pub fn pdf_pages(path: &Path) -> Result<Vec<Page>, String> {
    let pdfium = lastbook_engine::library::pdf::pdfium()
        .ok_or("PDFium is needed to read a PDF's text: put its library beside this program, or name its folder in LASTBOOK_PDFIUM")?;
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let doc = pdfium.load_pdf_from_byte_slice(&bytes, None).map_err(|e| format!("{}: PDFium could not open it ({e})", path.display()))?;
    let mut pages: Vec<Page> = doc.pages().iter().map(|page| Page::of(page.text().map(|t| clean(&t.all())).unwrap_or_default())).collect();
    let few: Vec<usize> = (0..pages.len()).filter(|&i| letters(&pages[i].text) < FEW_LETTERS).collect();
    if !few.is_empty() {
        for (i, alt) in alt_texts(pdfium, &bytes, &few) {
            pages[i].alt = alt;
        }
    }
    Ok(pages)
}

/// The alt text a sources list writes out for pages whose PDF gives none (its `alt_text`: pages from 1), added to them.
/// A page the document does not have, or one whose PDF has alt text of its own, means the list and the file disagree.
pub fn add_alt_text(pages: &mut [Page], alt: &std::collections::BTreeMap<usize, Vec<String>>) -> Result<(), String> {
    let count = pages.len();
    for (&n, texts) in alt {
        let page = n.checked_sub(1).and_then(|i| pages.get_mut(i)).ok_or(format!("\"alt_text\" names page {n}, and the document has {count}"))?;
        if !page.alt.is_empty() {
            return Err(format!("page {n} has alt text of its own in the PDF: \"alt_text\" is only for pages that have none"));
        }
        page.alt = texts.clone();
    }
    Ok(())
}

/// The alt text of the pages asked for (from 0), from the PDF's structure tree: what a tagged (accessible) PDF says its
/// pictures show. pdfium-render does not walk the tree, so this is PDFium's own calls, on the same bytes opened again.
/// The same text twice on a page is kept once; a page with none is left out. Nothing is read from a PDF that has no
/// structure tree.
pub fn alt_texts(pdfium: &Pdfium, bytes: &[u8], pages: &[usize]) -> Vec<(usize, Vec<String>)> {
    let b = pdfium.bindings();
    let mut out = Vec::new();
    // SAFETY: every handle comes from PDFium and is used only while what it belongs to is open: the page's tree is
    // closed before the page, the page before the document, and the document before `bytes` can go.
    unsafe {
        let doc = b.FPDF_LoadMemDocument64(bytes, None);
        if doc.is_null() {
            return out;
        }
        for &i in pages {
            let page = b.FPDF_LoadPage(doc, i as c_int);
            if page.is_null() {
                continue;
            }
            let tree = b.FPDF_StructTree_GetForPage(page);
            if !tree.is_null() {
                let mut alt = Vec::new();
                for k in 0..b.FPDF_StructTree_CountChildren(tree).max(0) {
                    element_alt(b, b.FPDF_StructTree_GetChildAtIndex(tree, k), 0, &mut alt);
                }
                b.FPDF_StructTree_Close(tree);
                if !alt.is_empty() {
                    out.push((i, alt));
                }
            }
            b.FPDF_ClosePage(page);
        }
        b.FPDF_CloseDocument(doc);
    }
    out
}

/// An element's alt text, else its children's: an element's alt text stands for everything in it (ISO 32000, 14.9.3),
/// so what is under it is not read again. A child on another page is not in this page's tree (PDFium gives no handle).
///
/// SAFETY: `el` is null or an element of a structure tree that is open.
unsafe fn element_alt(b: &dyn PdfiumLibraryBindings, el: FPDF_STRUCTELEMENT, depth: usize, out: &mut Vec<String>) {
    if el.is_null() || depth > 100 {
        return;
    }
    let bytes = b.FPDF_StructElement_GetAltText(el, std::ptr::null_mut(), 0) as usize; // UTF-16LE, its NUL counted
    if bytes > 2 {
        let mut buf = vec![0u16; bytes.div_ceil(2)];
        b.FPDF_StructElement_GetAltText(el, buf.as_mut_ptr() as *mut c_void, (buf.len() * 2) as c_ulong);
        let text = clean(String::from_utf16_lossy(&buf).trim_end_matches('\0')).trim().to_string();
        if !text.is_empty() {
            if !out.contains(&text) {
                out.push(text);
            }
            return;
        }
    }
    for k in 0..b.FPDF_StructElement_CountChildren(el).max(0) {
        element_alt(b, b.FPDF_StructElement_GetChildAtIndex(el, k), depth + 1, out);
    }
}

/// A saved page's text, one line per piece of text, as one page.
pub fn html_text(data: &[u8]) -> String {
    let mut kept = Vec::with_capacity(data.len());
    {
        let settings = Settings::new().with_strict(false).append_element_content_handler(element!(NOT_TEXT, |el| {
            el.remove();
            Ok(())
        }));
        let mut rw = HtmlRewriter::new(settings, |c: &[u8]| kept.extend_from_slice(c));
        if rw.write(data).is_err() || rw.end().is_err() {
            kept = data.to_vec();
        }
    }
    let lines: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
    let node: Rc<RefCell<String>> = Rc::new(RefCell::new(String::new()));
    {
        let (l, n) = (lines.clone(), node.clone());
        let settings = Settings::new().with_strict(false).append_document_content_handler(doc_text!(move |t| {
            n.borrow_mut().push_str(t.as_str());
            if t.last_in_text_node() {
                let raw = std::mem::take(&mut *n.borrow_mut());
                let words = html_escape::decode_html_entities(&raw).split_whitespace().collect::<Vec<_>>().join(" ");
                if !words.is_empty() {
                    l.borrow_mut().push(words);
                }
            }
            Ok(())
        }));
        let mut rw = HtmlRewriter::new(settings, |_: &[u8]| {});
        let _ = rw.write(&kept).and_then(|_| rw.end());
    }
    let out = lines.borrow().join("\n");
    out
}
