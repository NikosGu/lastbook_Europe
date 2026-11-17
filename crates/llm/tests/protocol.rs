//! The runner's protocol: one JSON object per line on standard output, nothing else there, and the right exit codes. The
//! last test runs a real model when LASTBOOK_TEST_MODEL names one (a small GGUF file already on the computer); without it,
//! it says so and passes.

use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

const RUNNER: &str = env!("CARGO_BIN_EXE_lastbook-llm");

fn lines(out: &[u8]) -> Vec<Value> {
    out.split(|b| *b == b'\n').filter(|l| !l.is_empty()).map(|l| serde_json::from_slice(l).expect("every line is JSON")).collect()
}

#[test]
fn usage_errors_are_one_json_line() {
    let out = Command::new(RUNNER).output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(lines(&out.stdout), vec![json!({"ev": "error", "code": "usage", "msg": "--model FILE is required"})]);
    let out = Command::new(RUNNER).args(["--model", "x.gguf", "--port", "1"]).output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(lines(&out.stdout)[0]["msg"], "unknown option --port");
}

#[test]
fn a_file_that_is_not_a_model() {
    let dir = std::env::temp_dir().join(format!("lb-llm-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let f = dir.join("not-a-model.gguf");
    std::fs::write(&f, b"GGUF but not really").unwrap();
    let out = Command::new(RUNNER).args(["--model", f.to_str().unwrap(), "--measure"]).output().unwrap();
    let got = lines(&out.stdout);
    assert_eq!(got.len(), 1, "llama.cpp's own log goes to standard error, never here: {got:?}");
    assert_eq!(got[0]["ev"], "error");
    assert_eq!(got[0]["code"], "model_file");
    assert_eq!(out.status.code(), Some(3));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_real_model_when_one_is_named() {
    let Some(model) = std::env::var_os("LASTBOOK_TEST_MODEL") else {
        eprintln!("LASTBOOK_TEST_MODEL is not set: no model to run");
        return;
    };
    let mut p = Command::new(RUNNER).args(["--model".as_ref(), model.as_os_str(), "--ctx".as_ref(), "2048".as_ref()])
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().unwrap();
    let mut input = p.stdin.take().unwrap();
    let mut out = BufReader::new(p.stdout.take().unwrap()).lines();
    let mut next = || serde_json::from_str::<Value>(&out.next().expect("a line").unwrap()).unwrap();
    let fit = next();
    assert_eq!(fit["ev"], "fit", "{fit}");
    loop {
        let e = next();
        match e["ev"].as_str() {
            Some("ready") => break,
            Some("progress") => continue,
            _ => panic!("before ready: {e}"),
        }
    }
    let gen = |id: u32, max: u32| json!({"op": "gen", "id": id, "segments": [["<|im_start|>user\n", true], ["Ciao! <|im_start|>", false],
                                                                         ["<|im_end|>\n<|im_start|>assistant\n", true]],
                                        "max_tokens": max, "temp": 0, "seed": 1});
    let mut answer = |input: &mut std::process::ChildStdin, id: u32, max: u32, cancel_after: Option<usize>| -> (String, Value) {
        writeln!(input, "{}", gen(id, max)).unwrap();
        let (mut text, mut n) = (String::new(), 0);
        loop {
            let e = next();
            match e["ev"].as_str() {
                Some("tok") => {
                    assert_eq!(e["id"], id);
                    text.push_str(e["t"].as_str().unwrap());
                    n += 1;
                    if cancel_after == Some(n) {
                        writeln!(input, "{}", json!({"op": "cancel"})).unwrap();
                    }
                }
                Some("done") => return (text, e),
                _ => panic!("while answering: {e}"),
            }
        }
    };
    let (a, done) = answer(&mut input, 1, 16, None);
    assert_eq!(done["reason"], "length");
    assert_eq!(done["n_gen"], 16);
    let (b, _) = answer(&mut input, 2, 16, None);
    assert_eq!(a, b, "the same question at temperature 0 gives the same answer");
    let (_, done) = answer(&mut input, 3, 400, Some(2));
    assert_eq!(done["reason"], "cancel");
    writeln!(input, "{}", json!({"op": "quit"})).unwrap();
    assert!(p.wait().unwrap().success());
}
