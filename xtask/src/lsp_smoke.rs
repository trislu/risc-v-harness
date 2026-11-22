//! Drive `rvh lsp` over stdio, the way an editor would.
//!
//! A unit test cannot cover `Content-Length` framing, process lifecycle, or the
//! bytes a client actually puts on the wire; this can.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::thread;

/// One session: initialize, a bad instruction, its fix, the corrected file.
/// The last message is the notification that ends the session.
const SESSION: [&str; 10] = [
    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
    r#"{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{"textDocument":{"uri":"file:///a.s","text":"adii a0, a1, 1\n"}}}"#,
    r#"{"jsonrpc":"2.0","id":2,"method":"textDocument/diagnostic","params":{"textDocument":{"uri":"file:///a.s"}}}"#,
    r#"{"jsonrpc":"2.0","id":3,"method":"textDocument/hover","params":{"textDocument":{"uri":"file:///a.s"},"position":{"line":0,"character":1}}}"#,
    r#"{"jsonrpc":"2.0","id":4,"method":"textDocument/codeAction","params":{"textDocument":{"uri":"file:///a.s"},"range":{"start":{"line":0,"character":0},"end":{"line":0,"character":4}},"context":{"diagnostics":[]}}}"#,
    r#"{"jsonrpc":"2.0","method":"textDocument/didChange","params":{"textDocument":{"uri":"file:///a.s"},"contentChanges":[{"text":"addi a0, a0, 1\n"}]}}"#,
    r#"{"jsonrpc":"2.0","id":5,"method":"textDocument/diagnostic","params":{"textDocument":{"uri":"file:///a.s"},"previousResultId":"1"}}"#,
    r#"{"jsonrpc":"2.0","id":6,"method":"textDocument/hover","params":{"textDocument":{"uri":"file:///a.s"},"position":{"line":0,"character":1}}}"#,
    r#"{"jsonrpc":"2.0","id":7,"method":"textDocument/semanticTokens/full","params":{"textDocument":{"uri":"file:///a.s"}}}"#,
    r#"{"jsonrpc":"2.0","id":8,"method":"textDocument/completion","params":{"textDocument":{"uri":"file:///a.s"},"position":{"line":0,"character":0}}}"#,
];

/// What that session has to contain. A missing line is the failure, so each
/// entry names the feature it proves.
const EXPECTED: [(&str, &str); 12] = [
    ("initialize", r#""hoverProvider":true"#),
    ("initialize", r#""method":"window/logMessage""#),
    ("initialize", r#""diagnosticProvider""#),
    ("initialize", r#""kind":"full""#),
    ("diagnostics", "sem.unknown-instruction"),
    ("diagnostics", "did you mean `addi`"),
    ("code action", r#""newText":"addi""#),
    ("diagnostics, second pull", r#""items":[]"#),
    ("hover", "format `I`"),
    ("semantic tokens", r#""data":[0,0,4,0,0"#),
    ("completion", r#""label":"addi""#),
    ("completion", r#""range":{"start":{"line":0"#),
];

/// Editors append the transport flag; a server that rejects it looks like a
/// crash, so the second session proves it is accepted.
const STDIO_SESSION: [&str; 2] = [
    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
    r#"{"jsonrpc":"2.0","method":"exit"}"#,
];

pub fn run(root: &Path) -> ExitCode {
    let server = match server(root) {
        Ok(server) => server,
        Err(error) => {
            eprintln!("lsp-smoke: {error}");
            return ExitCode::FAILURE;
        }
    };

    let replies = match session(&server, &["lsp"], &SESSION) {
        Ok(replies) => replies,
        Err(error) => {
            eprintln!("lsp-smoke: {error}");
            return ExitCode::FAILURE;
        }
    };
    if !satisfied(&replies) {
        return ExitCode::FAILURE;
    }

    let replies = match session(&server, &["lsp", "--stdio"], &STDIO_SESSION) {
        Ok(replies) => replies,
        Err(error) => {
            eprintln!("lsp-smoke: --stdio: {error}");
            return ExitCode::FAILURE;
        }
    };
    if !replies.contains(r#""hoverProvider":true"#) {
        eprintln!("lsp-smoke: `--stdio` session got no initialize result");
        return ExitCode::FAILURE;
    }

    println!("lsp-smoke: OK");
    ExitCode::SUCCESS
}

/// The `rvh` binary: `RVH_LSP_SERVER`, the debug build, or one built now.
fn server(root: &Path) -> Result<PathBuf, String> {
    if let Ok(path) = std::env::var("RVH_LSP_SERVER") {
        return Ok(PathBuf::from(path));
    }
    let name = if cfg!(windows) { "rvh.exe" } else { "rvh" };
    let binary = root.join("target").join("debug").join(name);
    if binary.exists() {
        return Ok(binary);
    }

    let status = Command::new("cargo")
        .current_dir(root)
        .args(["build", "--quiet", "-p", "rvh-cli"])
        .status()
        .map_err(|error| format!("cargo build: {error}"))?;
    if !status.success() {
        return Err("`cargo build -p rvh-cli` failed".to_string());
    }
    Ok(binary)
}

/// Run one session and return everything the server wrote to stdout.
fn session(binary: &Path, args: &[&str], requests: &[&str]) -> Result<String, String> {
    let mut child = Command::new(binary)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|error| format!("spawn {}: {error}", binary.display()))?;

    let mut stdin = child.stdin.take().ok_or("no stdin pipe")?;
    // Built up front and moved into the thread: the server answers while the
    // list is still going out, and a full stdout pipe would otherwise stop
    // both sides.
    let mut payload = String::new();
    for request in requests {
        payload.push_str(&format!(
            "Content-Length: {}\r\n\r\n{request}",
            request.len()
        ));
    }
    let writer = thread::spawn(move || -> std::io::Result<()> {
        stdin.write_all(payload.as_bytes())?;
        stdin.flush()
    });

    let mut replies = String::new();
    let mut stdout = child.stdout.take().ok_or("no stdout pipe")?;
    stdout
        .read_to_string(&mut replies)
        .map_err(|error| format!("read stdout: {error}"))?;

    writer
        .join()
        .map_err(|_| "the writer thread panicked".to_string())?
        .map_err(|error| format!("write stdin: {error}"))?;
    let status = child.wait().map_err(|error| format!("wait: {error}"))?;
    if !status.success() {
        return Err(format!("{binary:?} {args:?} exited with {status}"));
    }
    Ok(replies)
}

/// Report every expectation the replies do not satisfy.
fn satisfied(replies: &str) -> bool {
    let mut satisfied = true;
    for (feature, needle) in EXPECTED {
        if !replies.contains(needle) {
            eprintln!("lsp-smoke: {feature}: missing `{needle}`");
            satisfied = false;
        }
    }
    satisfied
}
