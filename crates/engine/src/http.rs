//! The shape of a request from the window and of the engine's answer. Plain data: the window turns its webview's
//! request into a [`Request`] and the [`Response`] back into the webview's, so the engine never sees a socket.

/// One request from the window.
#[derive(Debug, Clone, Default)]
pub struct Request {
    pub method: String,
    /// The path, percent-decoded, without the query: `/ui/app.js`, `/api/setup/state`.
    pub path: String,
    /// The raw query string, without the `?`.
    pub query: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Request {
    pub fn get(path: &str) -> Request {
        let (path, query) = path.split_once('?').unwrap_or((path, ""));
        Request { method: "GET".into(), path: percent_decode(path), query: query.into(), ..Default::default() }
    }

    pub fn post(path: &str, body: impl Into<Vec<u8>>) -> Request {
        let mut r = Request::get(path);
        r.method = "POST".into();
        r.body = body.into();
        r.headers.push(("Content-Type".into(), "application/json".into()));
        r
    }

    /// The first header of that name, compared without case.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }

    /// A query parameter, percent-decoded (`+` is a space).
    pub fn param(&self, name: &str) -> Option<String> {
        self.query.split('&').filter_map(|kv| kv.split_once('=').or(Some((kv, "")))).find(|(k, _)| *k == name).map(|(_, v)| percent_decode(&v.replace('+', " ")))
    }
}

/// The engine's answer.
#[derive(Debug, Clone)]
pub struct Response {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Response {
    pub fn new(status: u16, content_type: &str, body: impl Into<Vec<u8>>) -> Response {
        Response { status, headers: vec![("Content-Type".into(), content_type.into())], body: body.into() }
    }

    pub fn json(value: &serde_json::Value) -> Response {
        Response::json_status(200, value)
    }

    pub fn json_status(status: u16, value: &serde_json::Value) -> Response {
        Response::new(status, "application/json", serde_json::to_vec(value).unwrap_or_default()).with("Cache-Control", "no-store")
    }

    pub fn not_found() -> Response {
        Response::json_status(404, &serde_json::json!({"error": "not found"}))
    }

    pub fn with(mut self, name: &str, value: &str) -> Response {
        self.headers.push((name.into(), value.into()));
        self
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }
}

/// `%41` to `A`, and leaves anything that is not a valid escape as it is. Bytes that do not make UTF-8 become U+FFFD.
pub fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let (Some(h), Some(l)) = (hex(b[i + 1]), hex(b[i + 2])) {
                out.push(h * 16 + l);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decoding() {
        assert_eq!(percent_decode("/ui/a%20b.js"), "/ui/a b.js");
        assert_eq!(percent_decode("%C3%BC"), "ü");
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("%zz%4"), "%zz%4");
        assert_eq!(percent_decode("%2e%2e/x"), "../x");
    }

    #[test]
    fn query() {
        let r = Request::get("/api/find?q=boil+water&kind=pdf&x");
        assert_eq!(r.path, "/api/find");
        assert_eq!(r.param("q").as_deref(), Some("boil water"));
        assert_eq!(r.param("kind").as_deref(), Some("pdf"));
        assert_eq!(r.param("x").as_deref(), Some(""));
        assert_eq!(r.param("y"), None);
    }
}
