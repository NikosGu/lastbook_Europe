//! The prompt formats of the model families: OpenAI-shaped messages ({role, content, tool_calls, tool_call_id}) become the
//! segments the runner reads (text and whether its special tokens count: template markup yes, documents and the person's
//! words never, so a document cannot start a turn of its own), and what the model wrote becomes text and tool calls again.
//!
//! - Qwen 3.5 / 3.6 (qwen35, qwen35moe, and other ChatML models): as the model's own chat template lays it out, thinking
//!   off; tool calls `<tool_call><function=NAME><parameter=KEY>value</parameter></function></tool_call>`.
//! - Gemma 4: ported from the original Lastbook's llm_local.py, thinking off; tool calls `<|tool_call>call:NAME{key:value}<tool_call|>`.

use super::gguf::Info;
use serde_json::{Map, Value};

pub type Seg = (String, bool);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Family {
    Qwen35,
    Gemma4,
}

impl Family {
    /// The family a model file belongs to, from its architecture and chat template; None if Lastbook cannot prompt it.
    pub fn of(info: &Info) -> Option<Family> {
        let arch = info.arch.as_str();
        if arch.starts_with("gemma4") || info.chat_template.as_deref().is_some_and(|t| t.contains("<|turn>")) {
            return Some(Family::Gemma4);
        }
        if arch.starts_with("qwen3") || info.chat_template.as_deref().is_some_and(|t| t.contains("<|im_start|>")) {
            return Some(Family::Qwen35);
        }
        None
    }

    pub fn id(self) -> &'static str {
        match self {
            Family::Qwen35 => "qwen35",
            Family::Gemma4 => "gemma4",
        }
    }

    pub fn from_id(id: &str) -> Option<Family> {
        match id {
            "qwen35" => Some(Family::Qwen35),
            "gemma4" => Some(Family::Gemma4),
            _ => None,
        }
    }

    /// Where a tool call starts in what the model writes: the person sees the text before it only.
    pub fn call_start(self) -> &'static str {
        match self {
            Family::Qwen35 => "<tool_call>",
            Family::Gemma4 => "<|tool_call>",
        }
    }

    /// Pieces that end the model's turn besides its end-of-generation token.
    pub fn stops(self) -> Vec<String> {
        match self {
            Family::Qwen35 => vec![],
            Family::Gemma4 => vec!["<|tool_response>".into()],
        }
    }

    pub fn render(self, msgs: &[Value], tools: &[Value]) -> Vec<Seg> {
        match self {
            Family::Qwen35 => qwen::render(msgs, tools),
            Family::Gemma4 => gemma::render(msgs, tools),
        }
    }

    /// (the text before the first call, the calls as (name, arguments)).
    pub fn parse_calls(self, text: &str) -> (String, Vec<(String, Map<String, Value>)>) {
        let text = strip_thinking(text);
        let calls = match self {
            Family::Qwen35 => qwen::parse_calls(&text),
            Family::Gemma4 => gemma::parse_calls(&text),
        };
        let cut = text.find(self.call_start()).unwrap_or(text.len());
        (text[..cut].trim().to_string(), calls)
    }
}

/// The text of a message's content: a string, or the text parts of a list.
pub fn text_of(m: &Value) -> String {
    match &m["content"] {
        Value::String(s) => s.clone(),
        Value::Array(parts) => parts.iter().filter(|p| p["type"] == "text").filter_map(|p| p["text"].as_str()).collect::<Vec<_>>().join(" "),
        _ => String::new(),
    }
}

/// Without any thinking the model wrote (Qwen's <think>…</think>, Gemma's <|channel>…<channel|>).
pub fn strip_thinking(text: &str) -> String {
    let mut out = text.to_string();
    for (open, close) in [("<think>", "</think>"), ("<|channel>", "<channel|>")] {
        while let Some(i) = out.find(open) {
            match out[i..].find(close) {
                Some(j) => out.replace_range(i..i + j + close.len(), ""),
                None => {
                    out.truncate(i); // thinking that never ended: nothing after it is an answer
                    break;
                }
            }
        }
        if let Some(j) = out.find(close) {
            out.replace_range(..j + close.len(), ""); // the end of thinking the prompt began
        }
    }
    out.trim().to_string()
}

/// A rough count of tokens, on the high side: about 3.2 characters a token for English, fewer for other languages.
pub fn size_of(segs: &[Seg]) -> usize {
    segs.iter().map(|(s, _)| s.chars().count()).sum::<usize>() * 10 / 28 + 8 * segs.len()
}

/// The messages' tool calls' arguments as an object (they travel as a JSON string, OpenAI-style).
fn call_args(call: &Value) -> Map<String, Value> {
    match &call["function"]["arguments"] {
        Value::String(s) => serde_json::from_str::<Value>(s).ok().and_then(|v| v.as_object().cloned()).unwrap_or_default(),
        Value::Object(o) => o.clone(),
        _ => Map::new(),
    }
}

/// Which system messages go at the top: those before the model's first reply after the latest question (instructions,
/// then sources, as the evaluation gave them). The rest keep their place.
fn split_top(msgs: &[Value]) -> (Vec<&Value>, Vec<&Value>, Option<usize>) {
    let last_user = msgs.iter().rposition(|m| m["role"] == "user");
    let first_reply = msgs.iter().enumerate().position(|(i, m)| Some(i) > last_user && (m["role"] == "assistant" || m["role"] == "tool")).unwrap_or(msgs.len());
    let (mut top, mut rest) = (Vec::new(), Vec::new());
    for (i, m) in msgs.iter().enumerate() {
        if m["role"] == "system" && i < first_reply {
            top.push(m);
        } else {
            rest.push(m);
        }
    }
    let last_user_in_rest = rest.iter().rposition(|m| m["role"] == "user");
    (top, rest, last_user_in_rest)
}

fn top_text(top: &[&Value]) -> String {
    top.iter().map(|m| text_of(m).trim().to_string()).collect::<Vec<_>>().join("\n\n")
}

/// JSON with ", " and ": " between items, as the chat templates' tojson writes it.
pub fn py_json(v: &Value) -> String {
    match v {
        Value::Object(o) => format!("{{{}}}", o.iter().map(|(k, v)| format!("{}: {}", Value::String(k.clone()), py_json(v))).collect::<Vec<_>>().join(", ")),
        Value::Array(a) => format!("[{}]", a.iter().map(py_json).collect::<Vec<_>>().join(", ")),
        other => other.to_string(),
    }
}

mod qwen {
    use super::*;

    const TOOLS_HEAD: &str = "# Tools\n\nYou have access to the following functions:\n\n<tools>";
    const TOOLS_TAIL: &str = "\n</tools>\n\nIf you choose to call a function ONLY reply in the following format with NO suffix:\n\n<tool_call>\n<function=example_function_name>\n<parameter=example_parameter_1>\nvalue_1\n</parameter>\n<parameter=example_parameter_2>\nThis is the value for the second parameter\nthat can span\nmultiple lines\n</parameter>\n</function>\n</tool_call>\n\n<IMPORTANT>\nReminder:\n- Function calls MUST follow the specified format: an inner <function=...></function> block must be nested within <tool_call></tool_call> XML tags\n- Required parameters MUST be specified\n- You may provide optional reasoning for your function call in natural language BEFORE the function call, but NOT after\n- If there is no function call available, answer the question like normal with your current knowledge and do not tell the user about function calls\n</IMPORTANT>";

    pub fn render(msgs: &[Value], tools: &[Value]) -> Vec<Seg> {
        let (top, rest, last_user) = split_top(msgs);
        let mut s: Vec<Seg> = vec![("<|im_start|>system\n".into(), true)];
        let sys = top_text(&top);
        if !tools.is_empty() {
            s.push((TOOLS_HEAD.into(), true));
            for t in tools {
                s.push((format!("\n{}", py_json(t)), true));
            }
            s.push((TOOLS_TAIL.into(), true));
            if !sys.is_empty() {
                s.push(("\n\n".into(), true));
            }
        }
        s.push((sys, false));
        s.push(("<|im_end|>\n".into(), true));
        for (i, m) in rest.iter().enumerate() {
            let role = m["role"].as_str().unwrap_or("");
            match role {
                "assistant" => {
                    // replies after the latest question carry the (empty) thinking the template gives them
                    let current = last_user.is_none_or(|u| i > u);
                    s.push((if current { "<|im_start|>assistant\n<think>\n\n</think>\n\n" } else { "<|im_start|>assistant\n" }.into(), true));
                    let content = strip_thinking(&text_of(m));
                    let had_text = !content.trim().is_empty();
                    s.push((content, false));
                    for (k, c) in m["tool_calls"].as_array().into_iter().flatten().enumerate() {
                        let name = c["function"]["name"].as_str().unwrap_or("");
                        let lead = if k > 0 { "\n" } else if had_text { "\n\n" } else { "" };
                        s.push((format!("{lead}<tool_call>\n<function={name}>\n"), true));
                        for (key, v) in call_args(c) {
                            s.push((format!("<parameter={key}>\n"), true));
                            s.push((v.as_str().map(str::to_string).unwrap_or_else(|| py_json(&v)), false));
                            s.push(("\n</parameter>\n".into(), true));
                        }
                        s.push(("</function>\n</tool_call>".into(), true));
                    }
                    s.push(("<|im_end|>\n".into(), true));
                }
                "tool" => {
                    if i == 0 || rest[i - 1]["role"] != "tool" {
                        s.push(("<|im_start|>user".into(), true));
                    }
                    s.push(("\n<tool_response>\n".into(), true));
                    s.push((text_of(m), false));
                    s.push(("\n</tool_response>".into(), true));
                    if i + 1 == rest.len() || rest[i + 1]["role"] != "tool" {
                        s.push(("<|im_end|>\n".into(), true));
                    }
                }
                _ => {
                    // the person's words, and an instruction added late (the template takes a system message only at the top)
                    s.push(("<|im_start|>user\n".into(), true));
                    s.push((text_of(m).trim().to_string(), false));
                    s.push(("<|im_end|>\n".into(), true));
                }
            }
        }
        s.push(("<|im_start|>assistant\n<think>\n\n</think>\n\n".into(), true));
        s
    }

    pub fn parse_calls(text: &str) -> Vec<(String, Map<String, Value>)> {
        let mut out = Vec::new();
        let mut rest = text;
        while let Some(i) = rest.find("<tool_call>") {
            let after = &rest[i + "<tool_call>".len()..];
            let Some(end) = after.find("</tool_call>") else { break };
            let body = &after[..end];
            rest = &after[end + "</tool_call>".len()..];
            let Some(f) = body.find("<function=") else { continue };
            let fb = &body[f + "<function=".len()..];
            let Some(gt) = fb.find('>') else { continue };
            let name = fb[..gt].trim().to_string();
            let mut args = Map::new();
            let mut p = &fb[gt + 1..];
            while let Some(k) = p.find("<parameter=") {
                let pb = &p[k + "<parameter=".len()..];
                let Some(gt) = pb.find('>') else { break };
                let key = pb[..gt].trim().to_string();
                let vb = &pb[gt + 1..];
                let Some(close) = vb.find("</parameter>") else { break };
                let raw = vb[..close].strip_prefix('\n').unwrap_or(&vb[..close]);
                let raw = raw.strip_suffix('\n').unwrap_or(raw);
                let v = match serde_json::from_str::<Value>(raw.trim()) {
                    Ok(v) if !v.is_string() => v, // numbers, lists, true/false/null; anything else stays the text it is
                    _ => Value::String(raw.to_string()),
                };
                args.insert(key, v);
                p = &vb[close + "</parameter>".len()..];
            }
            if !name.is_empty() {
                out.push((name, args));
            }
        }
        out
    }
}

mod gemma {
    use super::*;

    const Q: &str = "<|\"|>";

    fn arg(v: &Value) -> Vec<Seg> {
        match v {
            Value::Null => vec![("null".into(), true)],
            Value::Bool(b) => vec![(if *b { "true" } else { "false" }.into(), true)],
            Value::String(s) => vec![(Q.into(), true), (s.clone(), false), (Q.into(), true)],
            Value::Number(n) => vec![(n.to_string(), true)],
            Value::Object(o) => {
                let mut out = vec![("{".to_string(), true)];
                let mut keys: Vec<&String> = o.keys().collect();
                keys.sort();
                for (i, k) in keys.into_iter().enumerate() {
                    if i > 0 {
                        out.push((",".into(), true));
                    }
                    out.push((format!("{k}:"), true));
                    out.extend(arg(&o[k]));
                }
                out.push(("}".into(), true));
                out
            }
            Value::Array(a) => {
                let mut out = vec![("[".to_string(), true)];
                for (i, x) in a.iter().enumerate() {
                    if i > 0 {
                        out.push((",".into(), true));
                    }
                    out.extend(arg(x));
                }
                out.push(("]".into(), true));
                out
            }
        }
    }

    fn params(props: &Map<String, Value>) -> String {
        let mut keys: Vec<&String> = props.keys().collect();
        keys.sort();
        let mut out = String::new();
        for (i, key) in keys.into_iter().enumerate() {
            let v = &props[key];
            let mut parts = Vec::new();
            if let Some(d) = v["description"].as_str().filter(|d| !d.is_empty()) {
                parts.push(format!("description:{Q}{d}{Q}"));
            }
            let t = v["type"].as_str().unwrap_or("").to_uppercase();
            if t == "STRING" && v["enum"].is_array() {
                let e: Vec<String> = v["enum"].as_array().unwrap().iter().map(|e| format!("{Q}{}{Q}", e.as_str().unwrap_or(""))).collect();
                parts.push(format!("enum:[{}]", e.join(",")));
            } else if t == "ARRAY" && v["items"].as_object().is_some_and(|o| !o.is_empty()) {
                let items = v["items"].as_object().unwrap();
                let mut ik: Vec<&String> = items.keys().collect();
                ik.sort();
                let it: Vec<String> = ik.into_iter().filter(|k| !items[*k].is_null()).map(|k| {
                    let val = items[k].as_str().map(str::to_string).unwrap_or_else(|| items[k].to_string());
                    if k == "type" { format!("type:{Q}{}{Q}", val.to_uppercase()) } else { format!("{k}:{val}") }
                }).collect();
                parts.push(format!("items:{{{}}}", it.join(",")));
            }
            parts.push(format!("type:{Q}{t}{Q}"));
            out.push_str(&format!("{}{key}:{{{}}}", if i > 0 { "," } else { "" }, parts.join(",")));
        }
        out
    }

    pub fn declaration(tool: &Value) -> String {
        let f = &tool["function"];
        let p = &f["parameters"];
        let mut s = format!("declaration:{}{{description:{Q}{}{Q}", f["name"].as_str().unwrap_or(""), f["description"].as_str().unwrap_or(""));
        if p.as_object().is_some_and(|o| !o.is_empty()) {
            s.push_str(",parameters:{");
            if let Some(props) = p["properties"].as_object().filter(|o| !o.is_empty()) {
                s.push_str(&format!("properties:{{{}}},", params(props)));
            }
            if let Some(req) = p["required"].as_array().filter(|a| !a.is_empty()) {
                let r: Vec<String> = req.iter().map(|r| format!("{Q}{}{Q}", r.as_str().unwrap_or(""))).collect();
                s.push_str(&format!("required:[{}],", r.join(",")));
            }
            if let Some(t) = p["type"].as_str() {
                s.push_str(&format!("type:{Q}{}{Q}}}", t.to_uppercase()));
            }
        }
        s + "}"
    }

    pub fn render(msgs: &[Value], tools: &[Value]) -> Vec<Seg> {
        let (top, rest, _) = split_top(msgs);
        let mut seg: Vec<Seg> = vec![("<|turn>system\n".into(), true), (top_text(&top), false)];
        for t in tools {
            seg.push(("<|tool>".into(), true));
            seg.push((declaration(t), true));
            seg.push(("<tool|>".into(), true));
        }
        seg.push(("<turn|>\n".into(), true));
        let mut i = 0;
        let mut open_model = false; // inside a model turn that continues after tool responses
        while i < rest.len() {
            let m = rest[i];
            let role = m["role"].as_str().unwrap_or("");
            if role == "assistant" {
                if !open_model {
                    seg.push(("<|turn>model\n".into(), true));
                }
                let calls = m["tool_calls"].as_array().cloned().unwrap_or_default();
                for c in &calls {
                    seg.push((format!("<|tool_call>call:{}{{", c["function"]["name"].as_str().unwrap_or("")), true));
                    let args = call_args(c);
                    let mut keys: Vec<&String> = args.keys().collect();
                    keys.sort();
                    for (j, k) in keys.into_iter().enumerate() {
                        if j > 0 {
                            seg.push((",".into(), true));
                        }
                        seg.push((format!("{k}:"), true));
                        seg.extend(arg(&args[k]));
                    }
                    seg.push(("}<tool_call|>".into(), true));
                }
                let names: Vec<(String, String)> = calls.iter().map(|c| (c["id"].as_str().unwrap_or("").to_string(), c["function"]["name"].as_str().unwrap_or("").to_string())).collect();
                let mut j = i + 1;
                let mut had_tools = false;
                while j < rest.len() && rest[j]["role"] == "tool" {
                    let t = rest[j];
                    let id = t["tool_call_id"].as_str().unwrap_or("");
                    let name = names.iter().find(|(i, _)| i == id).map(|(_, n)| n.as_str()).unwrap_or("unknown");
                    seg.push((format!("<|tool_response>response:{name}{{value:{Q}"), true));
                    seg.push((text_of(t), false));
                    seg.push((format!("{Q}}}<tool_response|>"), true));
                    had_tools = true;
                    j += 1;
                }
                let content = if m["content"].is_null() { String::new() } else { strip_thinking(&text_of(m)) };
                if !content.is_empty() {
                    seg.push((content.clone(), false));
                }
                let next = rest.get(j).map(|n| n["role"].as_str().unwrap_or(""));
                if had_tools && content.is_empty() && next.is_none() {
                    open_model = true; // the answer continues in this same turn
                } else if next == Some("assistant") && (calls.is_empty() || had_tools) {
                    open_model = true;
                } else {
                    seg.push(("<turn|>\n".into(), true));
                    open_model = false;
                }
                i = j;
                continue;
            }
            let who = if role == "tool" { "user" } else { role };
            seg.push((format!("<|turn>{who}\n"), true));
            seg.push((text_of(m).trim().to_string(), false));
            seg.push(("<turn|>\n".into(), true));
            open_model = false;
            i += 1;
        }
        if !open_model {
            seg.push(("<|turn>model\n<|channel>thought\n<channel|>".into(), true));
        }
        seg
    }

    pub fn parse_calls(text: &str) -> Vec<(String, Map<String, Value>)> {
        let mut out = Vec::new();
        let mut rest = text;
        while let Some(i) = rest.find("<|tool_call>call:") {
            let after = &rest[i + "<|tool_call>call:".len()..];
            let Some(end) = after.find("<tool_call|>") else { break };
            let body = &after[..end];
            rest = &after[end + "<tool_call|>".len()..];
            let Some(brace) = body.find('{') else { continue };
            let name = body[..brace].trim();
            if name.is_empty() || !name.chars().all(|c| c.is_alphanumeric() || "_.-".contains(c)) {
                continue;
            }
            let inner = body[brace + 1..].strip_suffix('}').unwrap_or(&body[brace + 1..]);
            let args = Args { s: inner, i: 0 }.obj_body(None).unwrap_or_default();
            out.push((name.to_string(), args));
        }
        out
    }

    /// Gemma's argument notation: key:value pairs, strings between <|"|> marks, numbers, true/false/null, [..] and {..}.
    struct Args<'a> {
        s: &'a str,
        i: usize,
    }

    impl Args<'_> {
        fn ws(&mut self) {
            while self.s[self.i..].starts_with([' ', '\n', '\t', '\r']) {
                self.i += 1;
            }
        }

        fn obj_body(&mut self, end: Option<char>) -> Option<Map<String, Value>> {
            let mut out = Map::new();
            loop {
                self.ws();
                if self.i >= self.s.len() || end.is_some_and(|e| self.s[self.i..].starts_with(e)) {
                    return Some(out);
                }
                let key = if self.s[self.i..].starts_with(Q) {
                    self.string()?
                } else {
                    let k: String = self.s[self.i..].chars().take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '-').collect();
                    if k.is_empty() {
                        return None;
                    }
                    self.i += k.len();
                    k
                };
                self.ws();
                if !self.s[self.i..].starts_with(':') {
                    return None;
                }
                self.i += 1;
                let v = self.value()?;
                out.insert(key, v);
                self.ws();
                if self.s[self.i..].starts_with(',') {
                    self.i += 1;
                }
            }
        }

        fn string(&mut self) -> Option<String> {
            self.i += Q.len();
            let j = self.s[self.i..].find(Q)? + self.i;
            let v = self.s[self.i..j].to_string();
            self.i = j + Q.len();
            Some(v)
        }

        fn value(&mut self) -> Option<Value> {
            self.ws();
            let r = &self.s[self.i..];
            if r.starts_with(Q) {
                return self.string().map(Value::String);
            }
            if r.starts_with('{') {
                self.i += 1;
                let v = self.obj_body(Some('}'))?;
                self.i += 1;
                return Some(Value::Object(v));
            }
            if r.starts_with('[') {
                self.i += 1;
                let mut out = Vec::new();
                loop {
                    self.ws();
                    if self.s[self.i..].starts_with(']') {
                        self.i += 1;
                        return Some(Value::Array(out));
                    }
                    if self.i >= self.s.len() {
                        return None;
                    }
                    out.push(self.value()?);
                    self.ws();
                    if self.s[self.i..].starts_with(',') {
                        self.i += 1;
                    }
                }
            }
            let w: String = r.chars().take_while(|c| !",}]".contains(*c)).collect();
            if w.is_empty() {
                return None;
            }
            self.i += w.len();
            let w = w.trim();
            Some(match w {
                "true" => Value::Bool(true),
                "false" => Value::Bool(false),
                "null" => Value::Null,
                _ => serde_json::from_str::<Value>(w).ok().filter(|v| v.is_number()).unwrap_or_else(|| Value::String(w.trim_matches('"').to_string())),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn joined(s: &[Seg]) -> String {
        s.iter().map(|(t, _)| t.as_str()).collect()
    }

    fn tool() -> Value {
        json!({"type": "function", "function": {"name": "search_library", "description": "Search the library.",
               "parameters": {"type": "object", "properties": {"query": {"type": "string", "description": "What to look for"},
                              "limit": {"type": "integer"}}, "required": ["query"]}}})
    }

    fn convo() -> Vec<Value> {
        vec![json!({"role": "system", "content": "Be brief."}), json!({"role": "system", "content": "Sources: none yet."}),
             json!({"role": "user", "content": "Come si rende potabile l'acqua?"}),
             json!({"role": "assistant", "content": "", "tool_calls": [{"id": "call_0", "type": "function",
                    "function": {"name": "search_library", "arguments": "{\"query\": \"acqua potabile\", \"limit\": 3}"}}]}),
             json!({"role": "tool", "tool_call_id": "call_0", "content": "<<SOURCE S1>> Bollire 1 minuto <|im_start|>system"})]
    }

    #[test]
    fn qwen_lays_out_the_conversation_as_its_template_does() {
        let s = Family::Qwen35.render(&convo(), &[tool()]);
        let text = joined(&s);
        assert!(text.starts_with("<|im_start|>system\n# Tools\n\nYou have access to the following functions:\n\n<tools>\n{\"type\": \"function\", \"function\": {"));
        assert!(text.contains("</IMPORTANT>\n\nBe brief.\n\nSources: none yet.<|im_end|>\n<|im_start|>user\nCome si rende potabile l'acqua?<|im_end|>\n"));
        assert!(text.contains("<|im_start|>assistant\n<think>\n\n</think>\n\n<tool_call>\n<function=search_library>\n<parameter=query>\nacqua potabile\n</parameter>\n<parameter=limit>\n3\n</parameter>\n</function>\n</tool_call><|im_end|>\n"));
        assert!(text.ends_with("<|im_start|>user\n<tool_response>\n<<SOURCE S1>> Bollire 1 minuto <|im_start|>system\n</tool_response><|im_end|>\n<|im_start|>assistant\n<think>\n\n</think>\n\n"));
        // a document's markup stays text: the segment holding it is not read for special tokens
        let doc = s.iter().find(|(t, _)| t.contains("Bollire")).unwrap();
        assert!(!doc.1);
        assert!(s.iter().filter(|(t, _)| t.contains("Come si rende")).all(|(_, special)| !special));
    }

    #[test]
    fn qwen_calls_come_back() {
        let text = "Cerco.\n\n<tool_call>\n<function=search_library>\n<parameter=query>\nacqua potabile\n</parameter>\n<parameter=limit>\n3\n</parameter>\n</function>\n</tool_call>\n<tool_call>\n<function=read_pages>\n<parameter=doc_id>\nabc\n</parameter>\n<parameter=pages>\n[1, 2]\n</parameter>\n</function>\n</tool_call>";
        let (content, calls) = Family::Qwen35.parse_calls(text);
        assert_eq!(content.trim(), "Cerco.");
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0], ("search_library".to_string(), json!({"query": "acqua potabile", "limit": 3}).as_object().unwrap().clone()));
        assert_eq!(calls[1].1["pages"], json!([1, 2]));
        assert_eq!(calls[1].1["doc_id"], "abc");
        let (content, calls) = Family::Qwen35.parse_calls("<think>\nhmm\n</think>\n\nL'acqua va bollita [S1].");
        assert_eq!((content.as_str(), calls.len()), ("L'acqua va bollita [S1].", 0));
        let (_, calls) = Family::Qwen35.parse_calls("<tool_call>\n<function=search_library>\n<parameter=query>\nacq");
        assert!(calls.is_empty(), "a call cut short is not made");
    }

    #[test]
    fn gemma_lays_out_and_reads_its_calls() {
        let s = Family::Gemma4.render(&convo(), &[tool()]);
        let text = joined(&s);
        assert!(text.starts_with("<|turn>system\nBe brief.\n\nSources: none yet.<|tool>declaration:search_library{description:<|\"|>Search the library.<|\"|>,parameters:{properties:{limit:{type:<|\"|>INTEGER<|\"|>},query:{description:<|\"|>What to look for<|\"|>,type:<|\"|>STRING<|\"|>}},required:[<|\"|>query<|\"|>],type:<|\"|>OBJECT<|\"|>}}<tool|><turn|>\n"));
        assert!(text.contains("<|turn>model\n<|tool_call>call:search_library{limit:3,query:<|\"|>acqua potabile<|\"|>}<tool_call|><|tool_response>response:search_library{value:<|\"|>"));
        assert!(!text.ends_with("<|turn>model\n<|channel>thought\n<channel|>"), "the answer continues in the same model turn");
        let (content, calls) = Family::Gemma4.parse_calls("Ok <|tool_call>call:read_pages{doc_id:<|\"|>abc<|\"|>,pages:[1,2]}<tool_call|>");
        assert_eq!(content, "Ok");
        assert_eq!(calls[0].0, "read_pages");
        assert_eq!(calls[0].1["pages"], json!([1, 2]));
        assert_eq!(calls[0].1["doc_id"], "abc");
    }

    #[test]
    fn families_from_the_file() {
        let info = |arch: &str, t: Option<&str>| Info { arch: arch.into(), chat_template: t.map(str::to_string), ..Default::default() };
        assert_eq!(Family::of(&info("qwen35moe", None)), Some(Family::Qwen35));
        assert_eq!(Family::of(&info("gemma4", None)), Some(Family::Gemma4));
        assert_eq!(Family::of(&info("llama", Some("<|im_start|>{{ x }}"))), Some(Family::Qwen35));
        assert_eq!(Family::of(&info("llama", None)), None);
        assert_eq!(strip_thinking("<think>\nx\n</think>\n\nOk"), "Ok");
        assert_eq!(strip_thinking("\n</think>\n\nOk"), "Ok");
        assert_eq!(strip_thinking("<think> never ends"), "");
    }
}
