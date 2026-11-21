//! A minimal Debug Adapter Protocol server.
//!
//! One thread, one frame: the machine executes instructions and the adapter maps
//! them to source lines. There is no call stack to walk, because the simulator
//! models registers and memory, not functions; `stackTrace` therefore reports the
//! current instruction.
//!
//! The transport is Content-Length framed JSON over stdio, the same framing as
//! the language server, so `rvh-json` is reused unchanged.

#![forbid(unsafe_code)]

use rvh_json::{number, object, string, Json};
use rvh_session::Session;

const THREAD_ID: f64 = 1.0;

/// Run the stdio loop until `disconnect`.
pub fn run_stdio() -> std::io::Result<()> {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut reader = stdin.lock();
    let mut writer = stdout.lock();
    let mut server = Server::new();

    while let Some(body) = rvh_rpc::read_message(&mut reader)? {
        for response in server.handle(&body) {
            rvh_rpc::write_message(&mut writer, &response)?;
        }
        if server.disconnect_requested() {
            break;
        }
    }
    Ok(())
}

/// The adapter state.
#[derive(Debug, Default)]
pub struct Server {
    session: Option<Session>,
    /// Source path of the launched program.
    program: Option<String>,
    /// Addresses where execution should stop.
    breakpoints: Vec<u32>,
    stop_on_entry: bool,
    /// Bytes of console output already forwarded to the client.
    printed: usize,
    disconnected: bool,
    terminated: bool,
    seq: u64,
}

impl Server {
    #[must_use]
    pub fn new() -> Self {
        Self {
            stop_on_entry: true,
            ..Self::default()
        }
    }

    #[must_use]
    pub fn disconnect_requested(&self) -> bool {
        self.disconnected
    }

    /// Handle one message body. Returns the bodies to send back.
    pub fn handle(&mut self, body: &str) -> Vec<String> {
        let message = match rvh_json::parse(body) {
            Ok(message) => message,
            Err(error) => {
                // A malformed message cannot be answered: there is no seq to
                // answer to. Say so on an event, which the client shows.
                return vec![self.event(
                    "output",
                    object(vec![
                        ("category", string("stderr")),
                        ("output", string(format!("rvh-dap: {error}\n"))),
                    ]),
                )];
            }
        };
        let Some(command) = message.get("command").and_then(Json::as_str) else {
            return Vec::new();
        };
        let request_seq = message.get("seq").cloned().unwrap_or(Json::Null);
        let arguments = message.get("arguments").cloned().unwrap_or(Json::Null);
        let command = command.to_string();

        match command.as_str() {
            "initialize" => vec![self.respond(
                request_seq,
                &command,
                Some(object(vec![
                    ("supportsConfigurationDoneRequest", Json::Bool(true)),
                    ("supportsStepBack", Json::Bool(false)),
                    ("supportsSetVariable", Json::Bool(false)),
                    ("supportsEvaluateForHovers", Json::Bool(false)),
                    ("supportsTerminateRequest", Json::Bool(false)),
                    ("supportsReadMemoryRequest", Json::Bool(true)),
                ])),
            )],
            "launch" => self.launch(request_seq, &arguments),
            "setBreakpoints" => self.set_breakpoints(request_seq, &arguments),
            "configurationDone" => self.configuration_done(request_seq),
            "threads" => vec![self.respond(
                request_seq,
                "threads",
                Some(object(vec![(
                    "threads",
                    Json::Array(vec![object(vec![
                        ("id", number(THREAD_ID)),
                        ("name", string("main")),
                    ])]),
                )])),
            )],
            "stackTrace" => vec![self.stack_trace(request_seq)],
            "scopes" => vec![self.scopes(request_seq)],
            "variables" => vec![self.variables(request_seq, &arguments)],
            "continue" => self.resume(request_seq, "continue"),
            "next" | "stepIn" | "stepOut" => self.step_one(request_seq, &command),
            "pause" => vec![self.respond(request_seq, &command, None)],
            "readMemory" => vec![self.read_memory(request_seq, &arguments)],
            "disconnect" | "terminate" => {
                self.disconnected = true;
                vec![self.respond(request_seq, &command, None)]
            }
            other => vec![self.error(request_seq, other, &format!("`{other}` is not implemented"))],
        }
    }

    // -- requests ----------------------------------------------------------

    fn launch(&mut self, request_seq: Json, arguments: &Json) -> Vec<String> {
        let Some(path) = arguments.get("program").and_then(Json::as_str) else {
            return vec![self.error(request_seq, "launch", "`program` is required")];
        };
        self.stop_on_entry = arguments
            .get("stopOnEntry")
            .and_then(Json::as_bool)
            .unwrap_or(true);

        let bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) => {
                return vec![self.error(
                    request_seq,
                    "launch",
                    &format!("cannot read {path}: {error}"),
                )];
            }
        };
        let mut session = Session::new(match rvh_isa::Target::parse("rv32i_zicsr") {
            Ok(target) => target,
            Err(error) => return vec![self.error(request_seq, "launch", &error.to_string())],
        });
        if bytes.starts_with(b"\x7fELF") {
            if let Err(error) = session.load_elf(&bytes) {
                return vec![self.error(request_seq, "launch", &error)];
            }
        } else {
            let source = String::from_utf8_lossy(&bytes);
            session.set_source(&source);
            if !session.diagnostics().is_empty() {
                let text = session.diagnostics_text();
                return vec![self.error(request_seq, "launch", &format!("diagnostics:\n{text}"))];
            }
        }

        self.session = Some(session);
        self.program = Some(path.to_string());
        self.printed = 0;
        self.terminated = false;
        vec![
            self.respond(request_seq, "launch", None),
            self.event("initialized", object(vec![])),
        ]
    }

    fn set_breakpoints(&mut self, request_seq: Json, arguments: &Json) -> Vec<String> {
        let lines: Vec<usize> = arguments
            .get("breakpoints")
            .and_then(Json::as_array)
            .map(|breakpoints| {
                breakpoints
                    .iter()
                    .filter_map(|breakpoint| breakpoint.get("line")?.as_u32())
                    .map(|line| line as usize)
                    .collect()
            })
            .unwrap_or_default();

        self.breakpoints.clear();
        let mut verified = Vec::new();
        for line in lines {
            let address = self
                .session
                .as_ref()
                .and_then(|session| session.address_of_line(line));
            match address {
                Some(address) => {
                    self.breakpoints.push(address);
                    verified.push(object(vec![
                        ("verified", Json::Bool(true)),
                        ("line", number(line as f64)),
                        ("instructionReference", string(format!("{address:#010x}"))),
                    ]));
                }
                None => verified.push(object(vec![
                    ("verified", Json::Bool(false)),
                    ("line", number(line as f64)),
                    ("message", string("no instruction on this line")),
                ])),
            }
        }
        vec![self.respond(
            request_seq,
            "setBreakpoints",
            Some(object(vec![("breakpoints", Json::Array(verified))])),
        )]
    }

    fn configuration_done(&mut self, request_seq: Json) -> Vec<String> {
        let mut messages = vec![self.respond(request_seq, "configurationDone", None)];
        if self.stop_on_entry {
            messages.push(self.stopped("entry", "stopped at entry"));
            messages.push(self.state_event());
        } else {
            messages.extend(self.run_to_stop());
        }
        messages
    }

    fn stack_trace(&mut self, request_seq: Json) -> String {
        let Some(session) = self.session.as_ref() else {
            return self.respond(
                request_seq,
                "stackTrace",
                Some(object(vec![("stackFrames", Json::Array(Vec::new()))])),
            );
        };
        let Some(machine) = session.machine() else {
            return self.respond(
                request_seq,
                "stackTrace",
                Some(object(vec![("stackFrames", Json::Array(Vec::new()))])),
            );
        };
        let (line, column) = session.line_of_pc().unwrap_or((0, 0));
        let frame = object(vec![
            ("id", number(1.0)),
            ("name", string(format!("pc {:#010x}", machine.pc()))),
            ("line", number(line as f64)),
            ("column", number(column as f64)),
            (
                "source",
                object(vec![
                    (
                        "name",
                        string(short_name(self.program.as_deref().unwrap_or(""))),
                    ),
                    ("path", string(self.program.clone().unwrap_or_default())),
                ]),
            ),
        ]);
        self.respond(
            request_seq,
            "stackTrace",
            Some(object(vec![
                ("stackFrames", Json::Array(vec![frame])),
                ("totalFrames", number(1.0)),
            ])),
        )
    }

    fn scopes(&mut self, request_seq: Json) -> String {
        self.respond(
            request_seq,
            "scopes",
            Some(object(vec![(
                "scopes",
                Json::Array(vec![object(vec![
                    ("name", string("Registers")),
                    ("variablesReference", number(1.0)),
                    ("expensive", Json::Bool(false)),
                ])]),
            )])),
        )
    }

    fn variables(&mut self, request_seq: Json, arguments: &Json) -> String {
        let Some(reference) = arguments.get("variablesReference").and_then(Json::as_u32) else {
            return self.error(request_seq, "variables", "`variablesReference` is required");
        };
        if reference != 1 {
            return self.respond(
                request_seq,
                "variables",
                Some(object(vec![("variables", Json::Array(Vec::new()))])),
            );
        }
        let Some(machine) = self.session.as_ref().and_then(Session::machine) else {
            return self.respond(
                request_seq,
                "variables",
                Some(object(vec![("variables", Json::Array(Vec::new()))])),
            );
        };
        let variables = (0..32)
            .map(|index| {
                let name = match rvh_sem::register_name(index as u8) {
                    Some(abi) => format!("x{index} ({abi})"),
                    None => format!("x{index}"),
                };
                object(vec![
                    ("name", string(name)),
                    ("value", string(format!("{:#010x}", machine.reg(index)))),
                    ("type", string("u32")),
                    ("variablesReference", number(0.0)),
                ])
            })
            .collect();
        self.respond(
            request_seq,
            "variables",
            Some(object(vec![("variables", Json::Array(variables))])),
        )
    }

    /// Read bytes for the memory view. `data` is base64, as the protocol says.
    fn read_memory(&mut self, request_seq: Json, arguments: &Json) -> String {
        let reference = arguments
            .get("memoryReference")
            .and_then(Json::as_str)
            .unwrap_or("0");
        let base = parse_address(reference);
        let offset = arguments
            .get("offset")
            .and_then(Json::as_f64)
            .unwrap_or(0.0) as i32;
        let count = arguments
            .get("count")
            .and_then(Json::as_u32)
            .unwrap_or(64)
            .min(4096);

        let address = base.saturating_add_signed(offset);
        let limit = rvh_sim::RAM_SIZE as u32;
        if address >= limit {
            return self.error(
                request_seq,
                "readMemory",
                &format!("{address:#010x} is past the end of memory"),
            );
        }
        // A read near the end of RAM returns what is there rather than failing:
        // the stack pointer sits 16 bytes below the top, and a 256-byte window
        // from `sp` would otherwise always be an error.
        let count = count.min(limit - address);
        let Some(machine) = self.session.as_ref().and_then(Session::machine) else {
            return self.error(request_seq, "readMemory", "no program is loaded");
        };
        let Some(bytes) = machine.read_mem(address, count) else {
            return self.error(request_seq, "readMemory", "address is outside memory");
        };
        self.respond(
            request_seq,
            "readMemory",
            Some(object(vec![
                ("address", string(format!("{address:#010x}"))),
                ("data", string(base64(bytes))),
            ])),
        )
    }

    fn resume(&mut self, request_seq: Json, command: &str) -> Vec<String> {
        if self.terminated {
            return vec![
                self.respond(request_seq, command, None),
                self.event("terminated", object(vec![])),
            ];
        }
        let mut messages = vec![self.respond(request_seq, command, None)];
        messages.extend(self.run_to_stop());
        messages
    }

    fn step_one(&mut self, request_seq: Json, command: &str) -> Vec<String> {
        let mut messages = vec![self.respond(request_seq, command, None)];
        let Some(session) = self.session.as_mut() else {
            return messages;
        };
        if session.machine().is_none() {
            return messages;
        }
        session.step(1);
        messages.extend(self.after_stop("step"));
        messages
    }

    // -- execution ---------------------------------------------------------

    /// Run until a breakpoint or a halt. Returns the events to send.
    fn run_to_stop(&mut self) -> Vec<String> {
        enum Outcome {
            Breakpoint,
            Halt(String, bool),
        }

        let mut outcome = Outcome::Halt("no program".to_string(), true);
        if let Some(session) = self.session.as_mut() {
            let mut steps = 0u64;
            loop {
                let halted = session.machine().and_then(|machine| machine.halt());
                if let Some(halt) = halted {
                    outcome = Outcome::Halt(halt_text(halt), halt_is_fault(halt));
                    break;
                }
                if steps >= 10_000_000 {
                    outcome = Outcome::Halt("step limit reached".to_string(), true);
                    break;
                }
                if !session.step(1) {
                    let halt = session.machine().and_then(|machine| machine.halt());
                    outcome = Outcome::Halt(
                        halt.map_or_else(|| "halted".to_string(), halt_text),
                        halt.is_some_and(halt_is_fault),
                    );
                    break;
                }
                steps += 1;
                let address = session.machine().map_or(0, |machine| machine.pc());
                if self.breakpoints.contains(&address) {
                    outcome = Outcome::Breakpoint;
                    break;
                }
            }
        }

        match outcome {
            Outcome::Breakpoint => self.after_stop("breakpoint"),
            Outcome::Halt(reason, fault) => self.after_stop_with_halt(&reason, fault),
        }
    }

    fn after_stop(&mut self, reason: &str) -> Vec<String> {
        let mut messages = self.output_events();
        messages.push(self.stopped(reason, ""));
        messages.push(self.state_event());
        messages
    }

    fn after_stop_with_halt(&mut self, halt: &str, fault: bool) -> Vec<String> {
        let mut messages = self.output_events();
        // `ebreak` and `ecall` are how a program ends on purpose. Reporting them
        // as exceptions paints a red banner over a successful run.
        let reason = if fault { "exception" } else { "pause" };
        messages.push(self.stopped(reason, &format!("program ended: {halt}")));
        messages.push(self.state_event());
        messages
    }

    /// Forward console output produced since the last stop.
    fn output_events(&mut self) -> Vec<String> {
        let Some(machine) = self.session.as_ref().and_then(Session::machine) else {
            return Vec::new();
        };
        let output = machine.output();
        if output.len() <= self.printed {
            return Vec::new();
        }
        let fresh = String::from_utf8_lossy(&output[self.printed..]).into_owned();
        self.printed = output.len();
        vec![self.event(
            "output",
            object(vec![
                ("category", string("stdout")),
                ("output", string(fresh)),
            ]),
        )]
    }

    // -- message building --------------------------------------------------

    fn next_seq(&mut self) -> u64 {
        self.seq += 1;
        self.seq
    }

    fn respond(&mut self, request_seq: Json, command: &str, body: Option<Json>) -> String {
        let seq = self.next_seq();
        let mut entries = vec![
            ("seq", number(seq as f64)),
            ("type", string("response")),
            ("request_seq", request_seq),
            ("success", Json::Bool(true)),
            ("command", string(command)),
        ];
        if let Some(body) = body {
            entries.push(("body", body));
        }
        object(entries).to_string()
    }

    fn error(&mut self, request_seq: Json, command: &str, message: &str) -> String {
        let seq = self.next_seq();
        object(vec![
            ("seq", number(seq as f64)),
            ("type", string("response")),
            ("request_seq", request_seq),
            ("success", Json::Bool(false)),
            ("command", string(command)),
            ("message", string(message)),
        ])
        .to_string()
    }

    fn event(&mut self, event: &str, body: Json) -> String {
        let seq = self.next_seq();
        object(vec![
            ("seq", number(seq as f64)),
            ("type", string("event")),
            ("event", string(event)),
            ("body", body),
        ])
        .to_string()
    }

    /// Tell the client where execution is, so a memory view can follow it.
    fn state_event(&mut self) -> String {
        let (pc, sp, steps) = match self.session.as_ref().and_then(Session::machine) {
            Some(machine) => (machine.pc(), machine.reg(2), machine.steps()),
            None => (0, 0, 0),
        };
        self.event(
            "rvh/memory",
            object(vec![
                ("pc", string(format!("{pc:#010x}"))),
                ("sp", string(format!("{sp:#010x}"))),
                ("steps", number(steps as f64)),
            ]),
        )
    }

    fn stopped(&mut self, reason: &str, text: &str) -> String {
        let mut entries = vec![
            ("reason", string(reason)),
            ("threadId", number(THREAD_ID)),
            ("allThreadsStopped", Json::Bool(true)),
        ];
        if !text.is_empty() {
            entries.push(("description", string(text)));
            entries.push(("text", string(text)));
        }
        self.event("stopped", object(entries))
    }
}

fn parse_address(text: &str) -> u32 {
    let trimmed = text.trim();
    if let Some(hex) = trimmed
        .strip_prefix("0x")
        .or_else(|| trimmed.strip_prefix("0X"))
    {
        u32::from_str_radix(hex, 16).unwrap_or(0)
    } else {
        trimmed.parse::<u32>().unwrap_or(0)
    }
}

/// Base64, standard alphabet, padded. The protocol encodes memory with it.
fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let triple = u32::from(chunk[0]) << 16
            | u32::from(*chunk.get(1).unwrap_or(&0)) << 8
            | u32::from(*chunk.get(2).unwrap_or(&0));
        out.push(ALPHABET[(triple >> 18) as usize & 0x3f] as char);
        out.push(ALPHABET[(triple >> 12) as usize & 0x3f] as char);
        out.push(if chunk.len() > 1 {
            ALPHABET[(triple >> 6) as usize & 0x3f] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            ALPHABET[triple as usize & 0x3f] as char
        } else {
            '='
        });
    }
    out
}

/// A fault stops with an exception banner; a deliberate end does not.
fn halt_is_fault(halt: rvh_sim::HaltReason) -> bool {
    matches!(
        halt,
        rvh_sim::HaltReason::Illegal { .. }
            | rvh_sim::HaltReason::Fault { .. }
            | rvh_sim::HaltReason::StepLimit
    )
}

/// One line a person can read in the editor.
fn halt_text(halt: rvh_sim::HaltReason) -> String {
    match halt {
        rvh_sim::HaltReason::Ebreak { pc } => format!("ebreak at {pc:#010x}"),
        rvh_sim::HaltReason::Ecall { pc } => format!("ecall at {pc:#010x}"),
        rvh_sim::HaltReason::Illegal { pc, word } => {
            format!("illegal word {word:#010x} at {pc:#010x}")
        }
        rvh_sim::HaltReason::Fault { pc, addr } => {
            format!("memory fault at {addr:#010x} while executing {pc:#010x}")
        }
        rvh_sim::HaltReason::StepLimit => "step limit reached".to_string(),
    }
}

fn short_name(path: &str) -> String {
    path.rsplit('/').next().unwrap_or(path).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_program(source: &str) -> std::path::PathBuf {
        // Tests run in parallel: one file per call, or they race on the content.
        use std::sync::atomic::{AtomicU32, Ordering};
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let mut path = std::env::temp_dir();
        path.push(format!(
            "rvh-dap-{}-{}.s",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::write(&path, source).expect("write fixture");
        path
    }

    fn send(server: &mut Server, message: &str) -> Vec<Json> {
        server
            .handle(message)
            .into_iter()
            .map(|body| rvh_json::parse(&body).expect("valid json"))
            .collect()
    }

    fn request(seq: u32, command: &str, arguments: &str) -> String {
        format!(r#"{{"seq":{seq},"type":"request","command":"{command}","arguments":{arguments}}}"#)
    }

    fn find<'a>(messages: &'a [Json], key: &str) -> &'a Json {
        messages
            .iter()
            .find(|message| message.get(key).is_some())
            .unwrap_or_else(|| panic!("no message with `{key}`"))
    }

    #[test]
    fn a_session_stops_at_a_breakpoint_and_steps() {
        let path = temp_program(
            "_start:\n    addi a0, zero, 1\n    addi a0, a0, 1\n    addi a1, zero, 0\n    ebreak\n",
        );
        let mut server = Server::new();

        let messages = send(&mut server, &request(1, "initialize", "{}"));
        assert!(messages[0].get("body").is_some(), "initialize answers");

        let launch = format!(
            r#"{{"seq":2,"type":"request","command":"launch","arguments":{{"program":"{}"}}}}"#,
            path.display()
        );
        let messages = send(&mut server, &launch);
        assert!(
            messages.iter().any(|m| m.get("event").is_some()),
            "initialized event"
        );

        send(
            &mut server,
            &request(3, "setBreakpoints", r#"{"breakpoints":[{"line":3}]}"#),
        );
        let messages = send(&mut server, &request(4, "configurationDone", "{}"));
        assert_eq!(
            find(&messages, "event")
                .at(&["body", "reason"])
                .and_then(Json::as_str),
            Some("entry")
        );

        let messages = send(&mut server, &request(5, "continue", "{}"));
        assert_eq!(
            find(&messages, "event")
                .at(&["body", "reason"])
                .and_then(Json::as_str),
            Some("breakpoint")
        );

        let messages = send(&mut server, &request(6, "stackTrace", r#"{"threadId":1}"#));
        let frames = find(&messages, "body")
            .at(&["body", "stackFrames"])
            .and_then(Json::as_array)
            .expect("frames");
        let frame = &frames[0];
        assert_eq!(frame.get("line").and_then(Json::as_u32), Some(3));

        let messages = send(&mut server, &request(7, "next", r#"{"threadId":1}"#));
        assert_eq!(
            find(&messages, "event")
                .at(&["body", "reason"])
                .and_then(Json::as_str),
            Some("step")
        );

        let messages = send(
            &mut server,
            &request(8, "variables", r#"{"variablesReference":1}"#),
        );
        let variables = find(&messages, "body")
            .at(&["body", "variables"])
            .and_then(Json::as_array)
            .expect("variables");
        assert_eq!(variables.len(), 32);
    }

    #[test]
    fn a_read_near_the_end_of_ram_is_short_not_an_error() {
        let path = temp_program("_start:\n    ebreak\n");
        let mut server = Server::new();
        send(&mut server, &request(1, "initialize", "{}"));
        let launch = format!(
            r#"{{"seq":2,"type":"request","command":"launch","arguments":{{"program":"{}"}}}}"#,
            path.display()
        );
        send(&mut server, &launch);
        let end = rvh_sim::RAM_SIZE as u32 - 16;
        let arguments = format!(r#"{{"memoryReference":"{end:#010x}","count":256}}"#);
        let messages = send(&mut server, &request(3, "readMemory", &arguments));
        let response = find(&messages, "request_seq");
        assert_eq!(response.get("success").and_then(Json::as_bool), Some(true));
        assert_eq!(
            response.at(&["body", "data"]).and_then(Json::as_str),
            Some("AAAAAAAAAAAAAAAAAAAAAA=="),
            "16 bytes of zero, not an error"
        );
    }

    #[test]
    fn a_missing_program_is_an_error_response() {
        let mut server = Server::new();
        send(&mut server, &request(1, "initialize", "{}"));
        let messages = send(
            &mut server,
            &request(2, "launch", r#"{"program":"/nonexistent/program.elf"}"#),
        );
        let response = find(&messages, "request_seq");
        assert_eq!(response.get("success").and_then(Json::as_bool), Some(false));
    }

    #[test]
    fn a_breakpoint_on_a_blank_line_is_not_verified() {
        let path = temp_program("_start:\n\n    ebreak\n");
        let mut server = Server::new();
        send(&mut server, &request(1, "initialize", "{}"));
        let launch = format!(
            r#"{{"seq":2,"type":"request","command":"launch","arguments":{{"program":"{}"}}}}"#,
            path.display()
        );
        send(&mut server, &launch);
        let messages = send(
            &mut server,
            &request(3, "setBreakpoints", r#"{"breakpoints":[{"line":2}]}"#),
        );
        let breakpoints = find(&messages, "body")
            .at(&["body", "breakpoints"])
            .and_then(Json::as_array)
            .expect("breakpoints");
        let breakpoint = &breakpoints[0];
        assert_eq!(
            breakpoint.get("verified").and_then(Json::as_bool),
            Some(false)
        );
    }
}

#[cfg(test)]
mod memory_tests {
    use super::*;

    #[test]
    fn base64_matches_the_reference_encoding() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foob"), "Zm9vYg==");
    }

    #[test]
    fn addresses_parse_in_hex_and_decimal() {
        assert_eq!(parse_address("0x1000"), 0x1000);
        assert_eq!(parse_address("4096"), 4096);
        assert_eq!(parse_address("nonsense"), 0);
    }
}
