//! LSP feature layer over [`rvh_session::Session`].
//!
//! The server owns no semantics: it converts protocol positions to byte offsets,
//! asks the session, and converts the answer back. That keeps the editor path
//! identical to the playground path.
//!
//! Transport is JSON-RPC over stdio today. [`Server::handle`] takes one message
//! body and returns the bodies to send, so it can be driven by a test, by stdio,
//! or (later) by the wasm host without changes.

#![forbid(unsafe_code)]

use rvh_isa::Target;
use rvh_json as json;
use rvh_json::{number, object, string, Json};
use rvh_session::{CompletionKind, Session, TokenKind};

/// `window/logMessage` notification. Types: 1 error, 2 warning, 3 info, 4 log.
fn log_notification(message_type: u8, message: &str) -> String {
    object(vec![
        ("jsonrpc", string("2.0")),
        ("method", string("window/logMessage")),
        (
            "params",
            object(vec![
                ("type", number(f64::from(message_type))),
                ("message", string(message)),
            ]),
        ),
    ])
    .to_string()
}

fn maybe_log(enabled: bool, message_type: u8, text: impl Into<String>) -> Vec<String> {
    if enabled {
        vec![log_notification(message_type, &text.into())]
    } else {
        Vec::new()
    }
}

fn short_uri(uri: &str) -> &str {
    uri.rsplit('/').next().unwrap_or(uri)
}

/// One line describing a request, for the client's output channel.
fn summarize(method: &str, params: &Json, response: &str) -> String {
    let uri = uri_of(params).unwrap_or("?");
    let file = short_uri(uri);
    let position = params.get("position").map_or(String::new(), |position| {
        format!(
            ":{}:{}",
            position.get("line").and_then(Json::as_u32).unwrap_or(0),
            position
                .get("character")
                .and_then(Json::as_u32)
                .unwrap_or(0)
        )
    });
    let detail = match json::parse(response)
        .ok()
        .and_then(|value| value.get("result").cloned())
    {
        None => "error".to_string(),
        Some(result) => match method {
            "completion" | "diagnostic" => count_of(&result, "items")
                .map_or_else(|| "-".to_string(), |count| format!("{count} items")),
            "definition" => {
                if result == Json::Null {
                    "none".to_string()
                } else {
                    "found".to_string()
                }
            }
            "hover" => {
                if result.get("contents").is_some() {
                    "text".to_string()
                } else {
                    "none".to_string()
                }
            }
            _ => String::new(),
        },
    };
    format!("{method} {file}{position} -> {detail}")
}

fn count_of(value: &Json, key: &str) -> Option<usize> {
    value.get(key)?.as_array().map(<[Json]>::len)
}

/// How much the server says in `window/logMessage`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    /// Only problems.
    Quiet,
    /// Lifecycle and low-frequency requests.
    Normal,
    /// Everything, including per-edit traffic.
    Verbose,
}

impl LogLevel {
    fn parse(text: &str) -> Self {
        match text {
            "quiet" => Self::Quiet,
            "verbose" => Self::Verbose,
            _ => Self::Normal,
        }
    }

    fn at_least_normal(self) -> bool {
        self != Self::Quiet
    }
}

/// Semantic token types, in legend order.
const TOKEN_TYPES: [&str; 8] = [
    "keyword", "variable", "function", "number", "string", "comment", "operator", "property",
];
/// Semantic token modifiers, in legend order.
const TOKEN_MODIFIERS: [&str; 1] = ["declaration"];

/// LSP error code for a method this server does not implement.
const METHOD_NOT_FOUND: f64 = -32601.0;
/// LSP error code for a malformed request.
const INVALID_PARAMS: f64 = -32602.0;

/// Run the stdio loop until the client sends `exit`.
pub fn run_stdio(target: Target) -> std::io::Result<()> {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut reader = stdin.lock();
    let mut writer = stdout.lock();
    let mut server = Server::new(target);

    while let Some(body) = rvh_rpc::read_message(&mut reader)? {
        for response in server.handle(&body) {
            rvh_rpc::write_message(&mut writer, &response)?;
        }
        if server.exit_requested() {
            break;
        }
    }
    Ok(())
}

/// One open document.
#[derive(Debug)]
struct Document {
    uri: String,
    session: Session,
    /// Bumped on every change; sent back as the diagnostics `resultId`.
    generation: u64,
}

/// The message-level server.
#[derive(Debug)]
pub struct Server {
    target: Target,
    documents: Vec<Document>,
    shutdown: bool,
    exit: bool,
    level: LogLevel,
    /// Toggles the `$/setTrace` verbose traces on top of [`LogLevel`].
    tracing: bool,
    /// Last diagnostic count per document, so an unchanged pull stays quiet.
    diagnostic_counts: Vec<(String, usize)>,
    /// Per-document target overrides: URI to architecture string.
    pinned: Vec<(String, String)>,
    next_request_id: u64,
}

impl Server {
    #[must_use]
    pub fn new(target: Target) -> Self {
        Self {
            target,
            documents: Vec::new(),
            shutdown: false,
            exit: false,
            level: LogLevel::Normal,
            tracing: false,
            diagnostic_counts: Vec::new(),
            pinned: Vec::new(),
            next_request_id: 0,
        }
    }

    /// True once the client asked the server to exit.
    #[must_use]
    pub fn exit_requested(&self) -> bool {
        self.exit
    }

    /// Handle one JSON-RPC message body. Returns the bodies to send back.
    pub fn handle(&mut self, body: &str) -> Vec<String> {
        let message = match json::parse(body) {
            Ok(message) => message,
            Err(error) => {
                return vec![error_response(
                    Json::Null,
                    -32700.0,
                    &format!("parse error: {error}"),
                )];
            }
        };
        let id = message.get("id").cloned();
        let Some(method) = message.get("method").and_then(Json::as_str) else {
            // A response to a request this server sent (workspace/diagnostic/
            // refresh). It carries an id and a result, and needs no answer.
            if message.get("id").is_some()
                && (message.get("result").is_some() || message.get("error").is_some())
            {
                return Vec::new();
            }
            return vec![error_response(
                id.unwrap_or(Json::Null),
                INVALID_PARAMS,
                "missing method",
            )];
        };
        let params = message.get("params").cloned().unwrap_or(Json::Null);
        let method = method.to_string();

        match method.as_str() {
            "initialize" => {
                let warning = self.initialize(&params);
                let mut messages = maybe_log(
                    self.level.at_least_normal(),
                    3,
                    format!("initialize, target {}", self.target),
                );
                // A target the machine cannot run is a problem, so it is reported
                // even when the client asked for quiet.
                if let Some(warning) = warning {
                    messages.push(log_notification(2, &warning));
                }
                messages.push(result(id, initialize_result()));
                messages
            }
            "initialized" => Vec::new(),
            // Protocol notifications. Answering one is a protocol violation, so
            // accept them and stay quiet.
            "$/cancelRequest" => Vec::new(),
            "$/setTrace" => {
                self.tracing = params.get("value").and_then(Json::as_str) == Some("verbose");
                Vec::new()
            }
            "shutdown" => {
                self.shutdown = true;
                vec![result(id, Json::Null)]
            }
            "exit" => {
                self.exit = true;
                Vec::new()
            }
            "textDocument/didOpen" => self.did_open(&params),
            "textDocument/didChange" => self.did_change(&params),
            "textDocument/didClose" => {
                self.did_close(&params);
                maybe_log(
                    self.level.at_least_normal(),
                    3,
                    format!("didClose {}", uri_of(&params).unwrap_or("?")),
                )
            }
            "textDocument/diagnostic" => {
                let response = self.diagnostic(id, &params);
                let mut messages = Vec::new();
                if let Some(line) = self.diagnostic_log(&params, &response) {
                    messages.push(log_notification(4, &line));
                }
                messages.push(response);
                messages
            }
            "rvh/setDocumentTarget" => self.set_document_target(&params),
            "textDocument/completion" => {
                let response = self.completion(id, &params);
                let mut messages = maybe_log(
                    self.level.at_least_normal(),
                    4,
                    summarize("completion", &params, &response),
                );
                messages.push(response);
                messages
            }
            "textDocument/definition" => {
                let response = self.definition(id, &params);
                let mut messages = maybe_log(
                    self.level.at_least_normal(),
                    4,
                    summarize("definition", &params, &response),
                );
                messages.push(response);
                messages
            }
            "textDocument/semanticTokens/full" => {
                let mut messages = maybe_log(
                    self.tracing,
                    4,
                    summarize(
                        "semanticTokens",
                        &params,
                        "textDocument/semanticTokens/full",
                    ),
                );
                messages.push(self.semantic_tokens(id, &params));
                messages
            }
            "textDocument/hover" => {
                let response = self.hover(id, &params);
                let mut messages = maybe_log(
                    self.level.at_least_normal(),
                    4,
                    summarize("hover", &params, &response),
                );
                messages.push(response);
                messages
            }
            "textDocument/codeAction" => {
                let mut messages = maybe_log(
                    self.tracing,
                    4,
                    summarize("codeAction", &params, "textDocument/codeAction"),
                );
                messages.push(self.code_action(id, &params));
                messages
            }
            other => match id {
                Some(id) => vec![error_response(
                    id,
                    METHOD_NOT_FOUND,
                    &format!("method `{other}` is not implemented"),
                )],
                // A notification has no id; a response would be rejected by the
                // client and logged as "response message without id".
                None => Vec::new(),
            },
        }
    }

    /// Read the client's `initializationOptions.target`, if it sent one.
    ///
    /// Documents are dropped on a target change: they were analysed against the
    /// old ISA, and the client re-opens them anyway.
    /// Apply the client's options. Returns a warning when the requested target
    /// cannot be used, which the caller forwards to the client.
    fn initialize(&mut self, params: &Json) -> Option<String> {
        if let Some(level) = params
            .at(&["initializationOptions", "logLevel"])
            .and_then(Json::as_str)
        {
            self.level = LogLevel::parse(level);
        }
        if let Some(map) = params
            .at(&["initializationOptions", "documentTargets"])
            .and_then(Json::as_object)
        {
            for (uri, target) in map {
                if let Some(target) = target.as_str() {
                    self.pinned.push((uri.clone(), target.to_string()));
                }
            }
        }
        let requested = params
            .at(&["initializationOptions", "target"])
            .and_then(Json::as_str)?;
        match Target::parse(requested) {
            Ok(target) => {
                if target != self.target {
                    self.target = target;
                    self.documents.clear();
                }
                None
            }
            Err(error) => Some(format!(
                "target `{requested}` is not usable: {error}; staying on `{}`",
                self.target
            )),
        }
    }

    /// The target a URI should use: its pin, or the session default.
    fn target_for(&self, uri: &str) -> Target {
        self.pinned
            .iter()
            .find(|(pinned, _)| pinned == uri)
            .and_then(|(_, target)| Target::parse(target).ok())
            .unwrap_or_else(|| self.target.clone())
    }

    /// Pin a target to one document and re-analyse it.
    fn set_document_target(&mut self, params: &Json) -> Vec<String> {
        // A custom notification carries the uri at the top level; a client that
        // sends the standard shape works too.
        let Some(uri) = params
            .get("uri")
            .and_then(Json::as_str)
            .or_else(|| uri_of(params))
        else {
            return Vec::new();
        };
        let target = params.get("target").and_then(Json::as_str);
        self.pinned.retain(|(pinned, _)| pinned != uri);
        if let Some(target) = target {
            if Target::parse(target).is_err() {
                return vec![log_notification(
                    2,
                    &format!("target `{target}` is not usable; keeping `{}`", self.target),
                )];
            }
            self.pinned.push((uri.to_string(), target.to_string()));
        }

        // Re-analyse an open document, then ask the client to pull again: the
        // diagnostics changed while the text did not.
        if let Some(index) = self
            .documents
            .iter()
            .position(|document| document.uri == uri)
        {
            let text = self.documents[index].session.source().to_string();
            let mut session = Session::new(self.target_for(uri));
            session.set_source(&text);
            self.documents[index].session = session;
            self.documents[index].generation += 1;
            self.diagnostic_counts.retain(|(key, _)| key != uri);
        }
        let label = self.target_for(uri).name();
        let mut messages = vec![log_notification(
            3,
            &format!("target for {}: {label}", short_uri(uri)),
        )];
        messages.push(self.request("workspace/diagnostic/refresh", Json::Null));
        messages
    }

    fn request(&mut self, method: &str, params: Json) -> String {
        self.next_request_id += 1;
        object(vec![
            ("jsonrpc", string("2.0")),
            ("id", number(self.next_request_id as f64)),
            ("method", string(method)),
            ("params", params),
        ])
        .to_string()
    }

    fn document(&self, uri: &str) -> Option<&Document> {
        self.documents.iter().find(|document| document.uri == uri)
    }

    fn upsert(&mut self, uri: &str, text: &str) -> Option<&Document> {
        if let Some(index) = self
            .documents
            .iter()
            .position(|document| document.uri == uri)
        {
            let document = &mut self.documents[index];
            document.session.set_source(text);
            document.generation += 1;
            return self.documents.get(index);
        }
        let mut session = Session::new(self.target_for(uri));
        session.set_source(text);
        self.documents.push(Document {
            uri: uri.to_string(),
            session,
            generation: 1,
        });
        self.documents.last()
    }

    fn did_open(&mut self, params: &Json) -> Vec<String> {
        let Some(uri) = uri_of(params) else {
            return Vec::new();
        };
        let Some(text) = params.at(&["textDocument", "text"]).and_then(Json::as_str) else {
            return Vec::new();
        };
        self.upsert(uri, text);
        maybe_log(
            self.level.at_least_normal(),
            3,
            format!("didOpen {uri} ({} bytes)", text.len()),
        )
    }

    fn did_change(&mut self, params: &Json) -> Vec<String> {
        let Some(uri) = uri_of(params) else {
            return Vec::new();
        };
        let Some(text) = change_text(params) else {
            return Vec::new();
        };
        self.upsert(uri, text);
        maybe_log(
            self.tracing,
            4,
            format!("didChange {uri} ({} bytes)", text.len()),
        )
    }

    fn did_close(&mut self, params: &Json) {
        if let Some(uri) = uri_of(params) {
            self.documents.retain(|document| document.uri != uri);
        }
    }

    /// One line per *change* in the diagnostic count, not per pull.
    fn diagnostic_log(&mut self, params: &Json, response: &str) -> Option<String> {
        if !self.level.at_least_normal() {
            return None;
        }
        let uri = uri_of(params)?.to_string();
        let count = json::parse(response)
            .ok()
            .and_then(|value| value.get("result").cloned())
            .and_then(|result| count_of(&result, "items"))?;
        let previous = self
            .diagnostic_counts
            .iter()
            .find(|(key, _)| *key == uri)
            .map(|(_, count)| *count);
        if previous == Some(count) {
            return None;
        }
        if let Some(entry) = self
            .diagnostic_counts
            .iter_mut()
            .find(|(key, _)| *key == uri)
        {
            entry.1 = count;
        } else {
            self.diagnostic_counts.push((uri.clone(), count));
        }
        Some(format!("diagnostic {} -> {count} items", short_uri(&uri)))
    }

    fn diagnostic(&self, id: Option<Json>, params: &Json) -> String {
        let Some(uri) = uri_of(params) else {
            return error_response(id.unwrap_or(Json::Null), INVALID_PARAMS, "missing uri");
        };
        let Some(document) = self.document(uri) else {
            return result(
                id,
                object(vec![
                    ("kind", string("full")),
                    ("items", Json::Array(Vec::new())),
                ]),
            );
        };
        let result_id = document.generation.to_string();
        if params.get("previousResultId").and_then(Json::as_str) == Some(result_id.as_str()) {
            return result(
                id,
                object(vec![
                    ("kind", string("unchanged")),
                    ("resultId", string(result_id)),
                ]),
            );
        }
        result(
            id,
            object(vec![
                ("kind", string("full")),
                ("items", Json::Array(diagnostic_items(&document.session))),
                ("resultId", string(result_id)),
            ]),
        )
    }

    fn completion(&self, id: Option<Json>, params: &Json) -> String {
        let Some(uri) = uri_of(params) else {
            return error_response(id.unwrap_or(Json::Null), INVALID_PARAMS, "missing uri");
        };
        let Some(document) = self.document(uri) else {
            return result(
                id,
                object(vec![
                    ("isIncomplete", Json::Bool(false)),
                    ("items", Json::Array(Vec::new())),
                ]),
            );
        };
        let Some(offset) = offset_from_params(document.session.source(), params) else {
            return result(
                id,
                object(vec![
                    ("isIncomplete", Json::Bool(false)),
                    ("items", Json::Array(Vec::new())),
                ]),
            );
        };

        let items = document
            .session
            .completions(offset)
            .into_iter()
            .map(|item| {
                object(vec![
                    ("label", string(item.label)),
                    ("kind", number(f64::from(completion_kind(item.kind)))),
                    ("detail", string(item.detail)),
                ])
            })
            .collect();
        result(
            id,
            object(vec![
                ("isIncomplete", Json::Bool(false)),
                ("items", Json::Array(items)),
            ]),
        )
    }

    fn definition(&self, id: Option<Json>, params: &Json) -> String {
        let Some(uri) = uri_of(params) else {
            return error_response(id.unwrap_or(Json::Null), INVALID_PARAMS, "missing uri");
        };
        let Some(document) = self.document(uri) else {
            return result(id, Json::Null);
        };
        let Some(offset) = offset_from_params(document.session.source(), params) else {
            return result(id, Json::Null);
        };
        let Some(span) = document.session.definition(offset) else {
            return result(id, Json::Null);
        };
        let source = document.session.source();
        let target = range_json(position_of(source, span.0), position_of(source, span.1));
        // A LocationLink lets the client underline the word under the cursor and
        // the name it jumps to, which plain Locations cannot express.
        let origin = document
            .session
            .token_span(offset)
            .map_or(Json::Null, |span| {
                range_json(position_of(source, span.0), position_of(source, span.1))
            });
        result(
            id,
            Json::Array(vec![object(vec![
                ("targetUri", string(uri)),
                ("targetRange", target.clone()),
                ("targetSelectionRange", target),
                ("originSelectionRange", origin),
            ])]),
        )
    }

    fn semantic_tokens(&self, id: Option<Json>, params: &Json) -> String {
        let Some(uri) = uri_of(params) else {
            return error_response(id.unwrap_or(Json::Null), INVALID_PARAMS, "missing uri");
        };
        let Some(document) = self.document(uri) else {
            return result(id, object(vec![("data", Json::Array(Vec::new()))]));
        };

        let source = document.session.source();
        let mut data = Vec::new();
        let mut previous_line = 0;
        let mut previous_start = 0;
        for token in document.session.tokens() {
            let (line, start) = position_of(source, token.span.0);
            let length = source[token.span.0..token.span.1].encode_utf16().count() as u32;
            let delta_line = line - previous_line;
            let delta_start = if delta_line == 0 {
                start - previous_start
            } else {
                start
            };
            let (token_type, modifiers) = token_encoding(token.kind);
            for value in [delta_line, delta_start, length, token_type, modifiers] {
                data.push(number(f64::from(value)));
            }
            previous_line = line;
            previous_start = start;
        }
        result(id, object(vec![("data", Json::Array(data))]))
    }

    fn hover(&self, id: Option<Json>, params: &Json) -> String {
        let Some(uri) = uri_of(params) else {
            return error_response(id.unwrap_or(Json::Null), INVALID_PARAMS, "missing uri");
        };
        let Some(document) = self.document(uri) else {
            return result(id, Json::Null);
        };
        let Some(offset) = offset_from_params(document.session.source(), params) else {
            return result(id, Json::Null);
        };
        let Some(hover) = document.session.hover(offset) else {
            return result(id, Json::Null);
        };
        let source = document.session.source();
        let start = position_of(source, hover.span.0);
        let end = position_of(source, hover.span.1);
        let contents = Json::Object(vec![
            ("kind".to_string(), Json::String("markdown".to_string())),
            ("value".to_string(), Json::String(hover.text)),
        ]);
        result(
            id,
            Json::Object(vec![
                ("contents".to_string(), contents),
                ("range".to_string(), range_json(start, end)),
            ]),
        )
    }

    fn code_action(&self, id: Option<Json>, params: &Json) -> String {
        let Some(uri) = uri_of(params) else {
            return error_response(id.unwrap_or(Json::Null), INVALID_PARAMS, "missing uri");
        };
        let Some(document) = self.document(uri) else {
            return result(id, Json::Array(Vec::new()));
        };
        let source = document.session.source();
        let Some(range) = params.get("range") else {
            return result(id, Json::Array(Vec::new()));
        };
        let Some(start) = offset_of_range_end(source, range, "start") else {
            return result(id, Json::Array(Vec::new()));
        };
        let Some(end) = offset_of_range_end(source, range, "end") else {
            return result(id, Json::Array(Vec::new()));
        };

        let mut actions = Vec::new();
        for diagnostic in document.session.diagnostics() {
            if diagnostic.span.1 < start || diagnostic.span.0 > end {
                continue;
            }
            for fix in diagnostic.fixes() {
                let edit = Json::Object(vec![(
                    "changes".to_string(),
                    Json::Object(vec![(
                        uri.to_string(),
                        Json::Array(vec![Json::Object(vec![
                            (
                                "range".to_string(),
                                range_json(
                                    position_of(source, fix.span.0),
                                    position_of(source, fix.span.1),
                                ),
                            ),
                            ("newText".to_string(), Json::String(fix.replacement.clone())),
                        ])]),
                    )]),
                )]);
                actions.push(Json::Object(vec![
                    ("title".to_string(), Json::String(fix.title.clone())),
                    ("kind".to_string(), Json::String("quickfix".to_string())),
                    ("edit".to_string(), edit),
                ]));
            }
        }
        result(id, Json::Array(actions))
    }
}

fn initialize_result() -> Json {
    Json::Object(vec![
        (
            "capabilities".to_string(),
            Json::Object(vec![
                // 1 = full synchronisation: the client sends the whole document.
                ("textDocumentSync".to_string(), Json::Number(1.0)),
                ("hoverProvider".to_string(), Json::Bool(true)),
                ("definitionProvider".to_string(), Json::Bool(true)),
                (
                    "completionProvider".to_string(),
                    object(vec![(
                        "triggerCharacters",
                        Json::Array(vec![string("."), string("%")]),
                    )]),
                ),
                // Pull only: an assembly file is self-contained, so there is no
                // cross-file state and nothing to refresh.
                (
                    "diagnosticProvider".to_string(),
                    object(vec![
                        ("interFileDependencies", Json::Bool(false)),
                        ("workspaceDiagnostics", Json::Bool(false)),
                    ]),
                ),
                (
                    "semanticTokensProvider".to_string(),
                    object(vec![
                        (
                            "legend",
                            object(vec![
                                (
                                    "tokenTypes",
                                    Json::Array(
                                        TOKEN_TYPES.iter().map(|name| string(*name)).collect(),
                                    ),
                                ),
                                (
                                    "tokenModifiers",
                                    Json::Array(
                                        TOKEN_MODIFIERS.iter().map(|name| string(*name)).collect(),
                                    ),
                                ),
                            ]),
                        ),
                        ("full", Json::Bool(true)),
                    ]),
                ),
                (
                    "codeActionProvider".to_string(),
                    Json::Object(vec![(
                        "codeActionKinds".to_string(),
                        Json::Array(vec![Json::String("quickfix".to_string())]),
                    )]),
                ),
            ]),
        ),
        (
            "serverInfo".to_string(),
            Json::Object(vec![
                ("name".to_string(), Json::String("rvh-lsp".to_string())),
                (
                    "version".to_string(),
                    Json::String(env!("CARGO_PKG_VERSION").to_string()),
                ),
            ]),
        ),
    ])
}

/// `CompletionItemKind`: 14 keyword, 6 variable, 21 constant, 3 function.
fn completion_kind(kind: CompletionKind) -> u32 {
    match kind {
        CompletionKind::Instruction | CompletionKind::Pseudo | CompletionKind::Directive => 14,
        CompletionKind::Register => 6,
        CompletionKind::Csr => 21,
        CompletionKind::Label => 3,
    }
}

/// Legend index and modifier bitset for a token class.
fn token_encoding(kind: TokenKind) -> (u32, u32) {
    match kind {
        TokenKind::Mnemonic | TokenKind::Pseudo | TokenKind::Directive => (0, 0),
        TokenKind::Register | TokenKind::Csr => (1, 0),
        TokenKind::Label => (2, 1),
        TokenKind::LabelRef => (2, 0),
        TokenKind::Number => (3, 0),
        TokenKind::String => (4, 0),
        TokenKind::Comment => (5, 0),
        TokenKind::Operator => (6, 0),
        TokenKind::Property => (7, 0),
    }
}

fn diagnostic_items(session: &Session) -> Vec<Json> {
    let source = session.source();
    session
        .diagnostics()
        .iter()
        .map(|diagnostic| {
            object(vec![
                (
                    "range",
                    range_json(
                        position_of(source, diagnostic.span.0),
                        position_of(source, diagnostic.span.1),
                    ),
                ),
                ("severity", number(1.0)),
                ("source", string("rvh")),
                ("code", string(diagnostic.code.to_string())),
                ("message", string(diagnostic.message.clone())),
            ])
        })
        .collect()
}

fn result(id: Option<Json>, value: Json) -> String {
    Json::Object(vec![
        ("jsonrpc".to_string(), Json::String("2.0".to_string())),
        ("id".to_string(), id.unwrap_or(Json::Null)),
        ("result".to_string(), value),
    ])
    .to_string()
}

fn error_response(id: Json, code: f64, message: &str) -> String {
    Json::Object(vec![
        ("jsonrpc".to_string(), Json::String("2.0".to_string())),
        ("id".to_string(), id),
        (
            "error".to_string(),
            Json::Object(vec![
                ("code".to_string(), Json::Number(code)),
                ("message".to_string(), Json::String(message.to_string())),
            ]),
        ),
    ])
    .to_string()
}

fn uri_of(params: &Json) -> Option<&str> {
    params.at(&["textDocument", "uri"]).and_then(Json::as_str)
}

fn change_text(params: &Json) -> Option<&str> {
    params
        .get("contentChanges")?
        .as_array()?
        .first()?
        .get("text")
        .and_then(Json::as_str)
}

fn offset_from_params(source: &str, params: &Json) -> Option<usize> {
    let position = params.get("position")?;
    let line = position.get("line")?.as_u32()?;
    let character = position.get("character")?.as_u32()?;
    Some(offset_of(source, line, character))
}

fn offset_of_range_end(source: &str, range: &Json, end: &str) -> Option<usize> {
    let position = range.get(end)?;
    let line = position.get("line")?.as_u32()?;
    let character = position.get("character")?.as_u32()?;
    Some(offset_of(source, line, character))
}

fn position_json(line: u32, character: u32) -> Json {
    Json::Object(vec![
        ("line".to_string(), Json::Number(f64::from(line))),
        ("character".to_string(), Json::Number(f64::from(character))),
    ])
}

fn range_json(start: (u32, u32), end: (u32, u32)) -> Json {
    Json::Object(vec![
        ("start".to_string(), position_json(start.0, start.1)),
        ("end".to_string(), position_json(end.0, end.1)),
    ])
}

/// Byte offsets of every line start.
fn line_starts(text: &str) -> Vec<usize> {
    let mut starts = vec![0];
    for (index, character) in text.char_indices() {
        if character == '\n' {
            starts.push(index + 1);
        }
    }
    starts
}

/// LSP position (line, UTF-16 column) to byte offset.
#[must_use]
pub fn offset_of(text: &str, line: u32, character: u32) -> usize {
    let starts = line_starts(text);
    let Some(&start) = starts.get(line as usize) else {
        return text.len();
    };
    let end = text[start..]
        .find('\n')
        .map_or(text.len(), |offset| start + offset);
    let mut units = 0u32;
    for (offset, character_here) in text[start..end].char_indices() {
        if units >= character {
            return start + offset;
        }
        units += character_here.len_utf16() as u32;
    }
    end
}

/// Byte offset to LSP position (line, UTF-16 column).
#[must_use]
pub fn position_of(text: &str, offset: usize) -> (u32, u32) {
    let clamped = offset.min(text.len());
    let starts = line_starts(text);
    let line = starts
        .iter()
        .rposition(|start| *start <= clamped)
        .unwrap_or(0);
    let column = text[starts[line]..clamped]
        .chars()
        .map(|character| character.len_utf16() as u32)
        .sum();
    (line as u32, column)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn server() -> Server {
        Server::new(Target::parse("rv32i_zicsr").expect("target"))
    }

    fn handle(server: &mut Server, body: &str) -> String {
        server.handle(body).join("\n")
    }

    #[test]
    fn initialize_advertises_hover_and_fixes() {
        let mut server = server();
        let response = handle(
            &mut server,
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
        );
        assert!(response.contains("\"hoverProvider\":true"), "{response}");
        assert!(response.contains("quickfix"), "{response}");
        assert!(response.contains("\"id\":1"), "{response}");
    }

    #[test]
    fn opening_a_document_does_not_push() {
        let mut server = server();
        let body = r#"{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{"textDocument":{"uri":"file:///a.s","text":"adii a0, a1, 1\n"}}}"#;
        let messages = server.handle(body);
        assert!(
            messages
                .iter()
                .all(|message| !message.contains("publishDiagnostics")),
            "pull servers never push diagnostics"
        );
        assert!(
            messages
                .iter()
                .any(|message| message.contains("window/logMessage")),
            "but they do say what happened"
        );
    }

    #[test]
    fn the_client_pulls_diagnostics() {
        let mut server = server();
        handle(
            &mut server,
            r#"{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{"textDocument":{"uri":"file:///a.s","text":"adii a0, a1, 1\n"}}}"#,
        );
        let response = handle(
            &mut server,
            r#"{"jsonrpc":"2.0","id":2,"method":"textDocument/diagnostic","params":{"textDocument":{"uri":"file:///a.s"}}}"#,
        );
        assert!(response.contains("\"kind\":\"full\""), "{response}");
        assert!(response.contains("sem.unknown-instruction"), "{response}");
        assert!(response.contains("\"resultId\":\"1\""), "{response}");
    }

    #[test]
    fn a_matching_result_id_is_unchanged() {
        let mut server = server();
        handle(
            &mut server,
            r#"{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{"textDocument":{"uri":"file:///a.s","text":"adii a0, a1, 1\n"}}}"#,
        );
        let response = handle(
            &mut server,
            r#"{"jsonrpc":"2.0","id":3,"method":"textDocument/diagnostic","params":{"textDocument":{"uri":"file:///a.s"},"previousResultId":"1"}}"#,
        );
        assert!(response.contains("\"kind\":\"unchanged\""), "{response}");

        // A change bumps the generation, so the same request now returns items.
        handle(
            &mut server,
            r#"{"jsonrpc":"2.0","method":"textDocument/didChange","params":{"textDocument":{"uri":"file:///a.s"},"contentChanges":[{"text":"addi a0, a0, 1\n"}]}}"#,
        );
        let response = handle(
            &mut server,
            r#"{"jsonrpc":"2.0","id":4,"method":"textDocument/diagnostic","params":{"textDocument":{"uri":"file:///a.s"},"previousResultId":"1"}}"#,
        );
        assert!(response.contains("\"kind\":\"full\""), "{response}");
        assert!(response.contains("\"items\":[]"), "{response}");
    }

    #[test]
    fn an_unsupported_target_is_reported_and_ignored() {
        let mut server = server();
        let messages = server.handle(
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"initializationOptions":{"target":"rv64i"}}}"#,
        );
        assert_eq!(messages.len(), 3, "warning, log line, response");
        let warning = messages
            .iter()
            .find(|message| message.contains("\"type\":2"))
            .expect("a warning");
        assert!(warning.contains("not usable"), "{warning}");
        assert!(
            warning.contains("rv32i_zicsr"),
            "names the target in use: {warning}"
        );

        // The session keeps working on the default target.
        let messages = server.handle(
            r#"{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{"textDocument":{"uri":"file:///t.s","text":"addi a0, zero, 1\n"}}}"#,
        );
        assert!(
            messages
                .iter()
                .all(|message| !message.contains("publishDiagnostics")),
            "no diagnostics on a valid program"
        );
    }

    #[test]
    fn the_client_chooses_the_target() {
        let mut server = server();
        handle(
            &mut server,
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"initializationOptions":{"target":"rv32i"}}}"#,
        );
        handle(
            &mut server,
            r#"{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{"textDocument":{"uri":"file:///b.s","text":"csrrw x0, mstatus, t0\n"}}}"#,
        );
        let response = handle(
            &mut server,
            r#"{"jsonrpc":"2.0","id":2,"method":"textDocument/diagnostic","params":{"textDocument":{"uri":"file:///b.s"}}}"#,
        );
        // `rv32i` lacks Zicsr, so the CSR instruction is rejected.
        assert!(response.contains("sem.extension-not-enabled"), "{response}");
    }

    #[test]
    fn low_frequency_requests_are_logged_with_their_result() {
        let mut server = server();
        handle(
            &mut server,
            r#"{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{"textDocument":{"uri":"file:///f.s","text":"addi a0, a1, 1\n"}}}"#,
        );
        let messages = server.handle(
            r#"{"jsonrpc":"2.0","id":11,"method":"textDocument/completion","params":{"textDocument":{"uri":"file:///f.s"},"position":{"line":0,"character":6}}}"#,
        );
        assert_eq!(messages.len(), 2, "a log line and a response");
        assert!(messages[0].contains("window/logMessage"), "{}", messages[0]);
        assert!(
            messages[0].contains("completion f.s:0:6 -> "),
            "{}",
            messages[0]
        );
        assert!(messages[0].contains(" items"), "{}", messages[0]);
    }

    #[test]
    fn noisy_requests_stay_out_of_the_log() {
        let mut server = server();
        handle(
            &mut server,
            r#"{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{"textDocument":{"uri":"file:///g.s","text":"addi a0, a1, 1\n"}}}"#,
        );
        let messages = server.handle(
            r#"{"jsonrpc":"2.0","id":12,"method":"textDocument/semanticTokens/full","params":{"textDocument":{"uri":"file:///g.s"}}}"#,
        );
        assert_eq!(messages.len(), 1, "semantic tokens only answer");
        assert!(
            !messages[0].contains("window/logMessage"),
            "{}",
            messages[0]
        );
    }

    #[test]
    fn completion_answers_by_context() {
        let mut server = server();
        handle(
            &mut server,
            r#"{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{"textDocument":{"uri":"file:///d.s","text":"addi a0, a1, 1\n"}}}"#,
        );
        let head = handle(
            &mut server,
            r#"{"jsonrpc":"2.0","id":8,"method":"textDocument/completion","params":{"textDocument":{"uri":"file:///d.s"},"position":{"line":0,"character":0}}}"#,
        );
        assert!(head.contains("\"label\":\"addi\""), "{head}");

        let operand = handle(
            &mut server,
            r#"{"jsonrpc":"2.0","id":9,"method":"textDocument/completion","params":{"textDocument":{"uri":"file:///d.s"},"position":{"line":0,"character":6}}}"#,
        );
        assert!(operand.contains("\"label\":\"a0\""), "{operand}");
        assert!(!operand.contains("\"label\":\"addi\""), "{operand}");
    }

    #[test]
    fn definition_returns_the_label_range() {
        let mut server = server();
        handle(
            &mut server,
            r#"{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{"textDocument":{"uri":"file:///e.s","text":"_start:\n    j _start\n"}}}"#,
        );
        let response = handle(
            &mut server,
            r#"{"jsonrpc":"2.0","id":10,"method":"textDocument/definition","params":{"textDocument":{"uri":"file:///e.s"},"position":{"line":1,"character":6}}}"#,
        );
        assert!(response.contains("\"targetUri\""), "{response}");
        assert!(response.contains("\"targetSelectionRange\""), "{response}");
        assert!(response.contains("\"originSelectionRange\""), "{response}");
        assert!(response.contains("\"line\":0"), "{response}");
    }

    /// Decode the 5-tuples into byte spans, using the server's own position
    /// conversion so the test checks what an editor would see.
    fn token_spans(source: &str, response: &str) -> Vec<(usize, usize)> {
        let value = json::parse(response).expect("json");
        let data = value
            .at(&["result", "data"])
            .and_then(Json::as_array)
            .expect("data");
        let mut spans = Vec::new();
        let (mut line, mut character) = (0u32, 0u32);
        for chunk in data.chunks(5) {
            let number = |index: usize| chunk[index].as_u32().expect("u32");
            line += number(0);
            character = if number(0) == 0 {
                character + number(1)
            } else {
                number(1)
            };
            let start = offset_of(source, line, character);
            let end = start + number(2) as usize;
            spans.push((start, end));
        }
        spans
    }

    /// Every word character of a file with no diagnostics must belong to a token.
    #[test]
    fn every_word_character_is_covered() {
        let cases = [
            include_str!("../../../tests/corpus/004_directives.s"),
            include_str!("../../../tests/corpus/006_arch_directives.s"),
            include_str!("../../../tests/corpus/007_arch_attribute.s"),
        ];
        for source in cases {
            let mut server = server();
            let body = format!(
                r#"{{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{{"textDocument":{{"uri":"file:///cover.s","text":{}}}}}}}"#,
                string(source)
            );
            handle(&mut server, &body);
            let response = handle(
                &mut server,
                r#"{"jsonrpc":"2.0","id":30,"method":"textDocument/semanticTokens/full","params":{"textDocument":{"uri":"file:///cover.s"}}}"#,
            );
            let spans = token_spans(source, &response);

            let mut uncovered = String::new();
            for (offset, character) in source.char_indices() {
                let word =
                    character.is_alphanumeric() || matches!(character, '_' | '.' | '+' | '-');
                if word
                    && !spans
                        .iter()
                        .any(|(start, end)| *start <= offset && offset < *end)
                {
                    uncovered.push(character);
                }
            }
            assert!(
                uncovered.is_empty(),
                "uncovered characters: {uncovered:?}\n{response}"
            );
        }
    }

    #[test]
    fn the_legend_covers_every_type_the_encoder_uses() {
        let mut server = server();
        let response = handle(
            &mut server,
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
        );
        for name in TOKEN_TYPES.iter() {
            assert!(
                response.contains(&format!("\"{name}\"")),
                "the legend must advertise `{name}`: {response}"
            );
        }
        // Every class the encoder can return needs a legend slot.
        for kind in [
            TokenKind::Mnemonic,
            TokenKind::Pseudo,
            TokenKind::Directive,
            TokenKind::Register,
            TokenKind::Csr,
            TokenKind::Label,
            TokenKind::LabelRef,
            TokenKind::Number,
            TokenKind::String,
            TokenKind::Comment,
            TokenKind::Operator,
            TokenKind::Property,
        ] {
            let (index, _) = token_encoding(kind);
            assert!(
                (index as usize) < TOKEN_TYPES.len(),
                "{kind:?} maps to {index}, outside the legend"
            );
        }
    }

    #[test]
    fn semantic_tokens_encode_positions() {
        let mut server = server();
        handle(
            &mut server,
            r#"{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{"textDocument":{"uri":"file:///c.s","text":"addi a0, a1, 1\n"}}}"#,
        );
        let response = handle(
            &mut server,
            r#"{"jsonrpc":"2.0","id":7,"method":"textDocument/semanticTokens/full","params":{"textDocument":{"uri":"file:///c.s"}}}"#,
        );
        // addi at 0:0 length 4 type 0; a0 at 5 type 1; a1 at +4 type 1; 1 at +4 type 3.
        assert!(
            response.contains("\"data\":[0,0,4,0,0,0,5,2,1,0,0,4,2,1,0,0,4,1,3,0]"),
            "{response}"
        );
    }

    #[test]
    fn directive_arguments_and_operators_get_types() {
        let mut server = server();
        handle(
            &mut server,
            r#"{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{"textDocument":{"uri":"file:///e.s","text":".option arch, +m\n"}}}"#,
        );
        let response = handle(
            &mut server,
            r#"{"jsonrpc":"2.0","id":21,"method":"textDocument/semanticTokens/full","params":{"textDocument":{"uri":"file:///e.s"}}}"#,
        );
        // .option keyword, arch property, + operator, m property
        assert!(
            response.contains("\"data\":[0,0,7,0,0,0,8,4,7,0,0,6,1,6,0,0,1,1,7,0]"),
            "{response}"
        );
    }

    #[test]
    fn the_capabilities_advertise_pull() {
        let mut server = server();
        let response = handle(
            &mut server,
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
        );
        assert!(response.contains("\"diagnosticProvider\""), "{response}");
        assert!(
            response.contains("\"interFileDependencies\":false"),
            "{response}"
        );
    }

    #[test]
    fn hover_returns_markdown_and_a_range() {
        let mut server = server();
        handle(
            &mut server,
            r#"{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{"textDocument":{"uri":"file:///a.s","text":"addi a0, a0, 1\n"}}}"#,
        );
        let response = handle(
            &mut server,
            r#"{"jsonrpc":"2.0","id":2,"method":"textDocument/hover","params":{"textDocument":{"uri":"file:///a.s"},"position":{"line":0,"character":1}}}"#,
        );
        assert!(response.contains("\"kind\":\"markdown\""), "{response}");
        assert!(response.contains("format `I`"), "{response}");
        assert!(
            response.contains("| operand | bits | range |"),
            "{response}"
        );
        assert!(response.contains("\"range\""), "{response}");
    }

    #[test]
    fn code_action_offers_the_fix() {
        let mut server = server();
        handle(
            &mut server,
            r#"{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{"textDocument":{"uri":"file:///a.s","text":"adii a0, a1, 1\n"}}}"#,
        );
        let response = handle(
            &mut server,
            r#"{"jsonrpc":"2.0","id":3,"method":"textDocument/codeAction","params":{"textDocument":{"uri":"file:///a.s"},"range":{"start":{"line":0,"character":0},"end":{"line":0,"character":4}},"context":{"diagnostics":[]}}}"#,
        );
        assert!(response.contains("replace with `addi`"), "{response}");
        assert!(response.contains("\"newText\":\"addi\""), "{response}");
    }

    #[test]
    fn a_missing_extension_is_fixable_from_the_editor() {
        let mut server = server();
        handle(
            &mut server,
            r#"{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{"textDocument":{"uri":"file:///m.s","text":"_start:\n    mul a0, a1, a2\n"}}}"#,
        );
        let response = handle(
            &mut server,
            r#"{"jsonrpc":"2.0","id":4,"method":"textDocument/codeAction","params":{"textDocument":{"uri":"file:///m.s"},"range":{"start":{"line":1,"character":4},"end":{"line":1,"character":7}},"context":{"diagnostics":[]}}}"#,
        );
        assert!(
            response.contains("enable extension `m` in this file"),
            "{response}"
        );
        assert!(response.contains(".option arch, +m"), "{response}");
    }

    #[test]
    fn the_client_can_quiet_the_log() {
        let mut server = server();
        handle(
            &mut server,
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"initializationOptions":{"logLevel":"quiet"}}}"#,
        );
        let messages = server.handle(
            r#"{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{"textDocument":{"uri":"file:///q.s","text":"addi a0, a1, 1\n"}}}"#,
        );
        assert!(messages.is_empty(), "quiet means quiet: {messages:?}");
    }

    #[test]
    fn set_trace_verbose_adds_the_noisy_traffic() {
        let mut server = server();
        handle(
            &mut server,
            r#"{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{"textDocument":{"uri":"file:///v.s","text":"addi a0, a1, 1\n"}}}"#,
        );
        let quiet = server.handle(
            r#"{"jsonrpc":"2.0","id":2,"method":"textDocument/semanticTokens/full","params":{"textDocument":{"uri":"file:///v.s"}}}"#,
        );
        assert_eq!(quiet.len(), 1, "no trace line by default");

        handle(
            &mut server,
            r#"{"jsonrpc":"2.0","method":"$/setTrace","params":{"value":"verbose"}}"#,
        );
        let loud = server.handle(
            r#"{"jsonrpc":"2.0","id":3,"method":"textDocument/semanticTokens/full","params":{"textDocument":{"uri":"file:///v.s"}}}"#,
        );
        assert_eq!(loud.len(), 2, "a trace line and a response");
        assert!(loud[0].contains("semanticTokens"), "{}", loud[0]);
    }

    #[test]
    fn diagnostics_log_only_when_the_count_changes() {
        let mut server = server();
        handle(
            &mut server,
            r#"{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{"textDocument":{"uri":"file:///c.s","text":"adii a0, a1, 1\n"}}}"#,
        );
        let pull = r#"{"jsonrpc":"2.0","id":4,"method":"textDocument/diagnostic","params":{"textDocument":{"uri":"file:///c.s"}}}"#;
        let first = server.handle(pull);
        assert_eq!(first.len(), 2, "one change, one line: {first:?}");
        let second = server.handle(pull);
        assert_eq!(second.len(), 1, "same count, no line: {second:?}");
    }

    #[test]
    fn unknown_notifications_are_ignored() {
        let mut server = server();
        assert!(server
            .handle(r#"{"jsonrpc":"2.0","method":"textDocument/frobnicate","params":{}}"#)
            .is_empty());
    }

    #[test]
    fn protocol_notifications_are_accepted() {
        let mut server = server();
        for method in ["$/cancelRequest", "$/setTrace"] {
            let body = format!(r#"{{"jsonrpc":"2.0","method":"{method}","params":{{}}}}"#);
            assert!(server.handle(&body).is_empty(), "{method}");
        }
    }

    #[test]
    fn an_unknown_method_is_an_error() {
        let mut server = server();
        let response = handle(
            &mut server,
            r#"{"jsonrpc":"2.0","id":4,"method":"textDocument/frobnicate","params":{}}"#,
        );
        assert!(response.contains("-32601"), "{response}");
    }

    #[test]
    fn positions_use_utf16_columns() {
        let text = "中文: nop\n";
        // Two CJK characters are three bytes but one UTF-16 unit each.
        assert_eq!(offset_of(text, 0, 2), 6, "after both characters");
        assert_eq!(offset_of(text, 0, 3), 7, "after the colon");
        assert_eq!(position_of(text, 6), (0, 2));
        assert_eq!(position_of("a\nb", 2), (1, 0));
    }
}

#[cfg(test)]
mod target_tests {
    use super::*;

    fn handle(server: &mut Server, body: &str) -> Vec<String> {
        server.handle(body)
    }

    fn open(server: &mut Server, uri: &str, text: &str) {
        let body = format!(
            r#"{{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{{"textDocument":{{"uri":"{uri}","text":"{text}"}}}}}}"#
        );
        handle(server, &body);
    }

    fn diagnostics(server: &mut Server, uri: &str) -> String {
        let body = format!(
            r#"{{"jsonrpc":"2.0","id":9,"method":"textDocument/diagnostic","params":{{"textDocument":{{"uri":"{uri}"}}}}}}"#
        );
        handle(server, &body).join("\n")
    }

    #[test]
    fn a_pinned_target_applies_to_one_document_only() {
        let mut server = Server::new(Target::parse("rv32i_zicsr").expect("target"));
        handle(
            &mut server,
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"initializationOptions":{"documentTargets":{"file:///m.s":"rv32i_m_zicsr"}}}}"#,
        );
        open(&mut server, "file:///m.s", "mul a0, a1, a2\\n");
        open(&mut server, "file:///p.s", "mul a0, a1, a2\\n");

        assert!(
            !diagnostics(&mut server, "file:///m.s").contains("extension-not-enabled"),
            "the pin enables M"
        );
        assert!(
            diagnostics(&mut server, "file:///p.s").contains("extension-not-enabled"),
            "the default target does not"
        );
    }

    #[test]
    fn changing_the_target_asks_the_client_to_refresh() {
        let mut server = Server::new(Target::parse("rv32i_zicsr").expect("target"));
        open(&mut server, "file:///m.s", "mul a0, a1, a2\\n");
        let messages = handle(
            &mut server,
            r#"{"jsonrpc":"2.0","method":"rvh/setDocumentTarget","params":{"uri":"file:///m.s","target":"rv32i_m_zicsr"}}"#,
        );
        assert!(
            messages
                .iter()
                .any(|message| message.contains("workspace/diagnostic/refresh")),
            "{messages:?}"
        );
        assert!(
            !diagnostics(&mut server, "file:///m.s").contains("extension-not-enabled"),
            "the new target is in effect"
        );
    }

    #[test]
    fn clearing_the_pin_returns_to_the_default() {
        let mut server = Server::new(Target::parse("rv32i_zicsr").expect("target"));
        open(&mut server, "file:///m.s", "mul a0, a1, a2\\n");
        handle(
            &mut server,
            r#"{"jsonrpc":"2.0","method":"rvh/setDocumentTarget","params":{"uri":"file:///m.s","target":"rv32i_m_zicsr"}}"#,
        );
        handle(
            &mut server,
            r#"{"jsonrpc":"2.0","method":"rvh/setDocumentTarget","params":{"uri":"file:///m.s"}}"#,
        );
        assert!(
            diagnostics(&mut server, "file:///m.s").contains("extension-not-enabled"),
            "back on the default"
        );
    }

    #[test]
    fn a_response_from_the_client_is_ignored() {
        let mut server = Server::new(Target::parse("rv32i_zicsr").expect("target"));
        assert!(handle(&mut server, r#"{"jsonrpc":"2.0","id":1,"result":null}"#).is_empty());
    }
}
