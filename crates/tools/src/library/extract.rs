//! The words of a document, page by page, for the search index and the reading page's text view. A PDF's through
//! PDFium (the engine's own loading: beside the program, or the folder in LASTBOOK_PDFIUM); a saved web page's through
//! lol_html, without what is not its text (scripts, styles, menus, headers, footers and forms), as the original
//! Lastbook took it with BeautifulSoup.

use lol_html::{doc_text, element, HtmlRewriter, Settings};
use std::cell::RefCell;
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

/// Each page's text. An error says what is wrong in words: no PDFium here, or a file PDFium cannot open.
pub fn pdf_pages(path: &Path) -> Result<Vec<String>, String> {
    let pdfium = lastbook_engine::library::pdf::pdfium()
        .ok_or("PDFium is needed to read a PDF's text: put its library beside this program, or name its folder in LASTBOOK_PDFIUM")?;
    let doc = pdfium.load_pdf_from_file(path, None).map_err(|e| format!("{}: PDFium could not open it ({e})", path.display()))?;
    Ok(doc.pages().iter().map(|page| page.text().map(|t| clean(&t.all())).unwrap_or_default()).collect())
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
