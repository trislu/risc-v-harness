//! The `rvh` command line tool.

use std::process::ExitCode;

use rvh_isa::{Insn, Operand};
use rvh_json::{number, object, string, Json};
use rvh_sem::REGISTER_NAMES;
use rvh_session::line_col;

const USAGE: &str = "usage: rvh <command>

commands:
  isa lookup <mnemonic> [--json]   show one instruction
  isa list [--ext <ext>]           list instructions
  isa extensions                   list known extensions
  isa decode <word>                decode a 32-bit word (hex or decimal)
  parse <file> [--tree] [--json]   parse assembly and print diagnostics
  asm <file> [-o <path>] [--text <path>] [--symbols] [--target <arch>]
                                   assemble and report, or write an image
  run <file> [--max-steps <n>] [--target <arch>] [--board <name>]
                                   assemble and run, then report the result
  hover <file> <line> <column>     explain the token at a position
  lsp [--stdio]                    speak LSP on stdin/stdout
  dap                              speak DAP on stdin/stdout
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    match args.as_slice() {
        ["isa", "lookup", mnemonic] => lookup(mnemonic, false),
        ["isa", "lookup", mnemonic, "--json"] => lookup(mnemonic, true),
        ["isa", "list"] => list(None),
        ["isa", "list", "--ext", ext] => list(Some(ext)),
        ["isa", "extensions"] => extensions(),
        ["isa", "decode", word] => decode(word),
        ["parse", path] => parse_file(path, false, false),
        ["parse", path, "--tree"] => parse_file(path, true, false),
        ["parse", path, "--json"] => parse_file(path, false, true),
        ["asm", rest @ ..] if !rest.is_empty() => asm_command(rest),
        ["run", rest @ ..] if !rest.is_empty() => run_command(rest),
        ["hover", file, line, column] => hover_command(file, line, column, "en"),
        ["hover", file, line, column, "--locale", locale] => {
            hover_command(file, line, column, locale)
        }
        ["lsp", rest @ ..] => lsp_command(rest),
        ["dap"] => dap_command(),
        [] | ["help"] | ["--help"] => {
            print!("{USAGE}");
            ExitCode::SUCCESS
        }
        other => {
            eprintln!("rvh: unknown command {other:?}\n\n{USAGE}");
            ExitCode::from(2)
        }
    }
}

fn lookup(mnemonic: &str, json: bool) -> ExitCode {
    let found: Vec<&Insn> = rvh_isa::lookups(mnemonic).collect();
    if found.is_empty() {
        eprintln!("rvh: unknown instruction `{mnemonic}`");
        return ExitCode::FAILURE;
    }
    for insn in found {
        if json {
            println!("{}", to_json(insn));
        } else {
            print_insn(insn);
        }
    }
    ExitCode::SUCCESS
}

fn print_insn(insn: &Insn) {
    println!(
        "{} — format {}, extension {}, class {}",
        insn.mnemonic,
        insn.format.as_str(),
        insn.ext,
        insn.class
    );

    let mut encoding = format!("opcode 0b{:07b}", insn.opcode);
    if let Some(funct3) = insn.funct3 {
        encoding.push_str(&format!("  funct3 0b{funct3:03b}"));
    }
    if let Some(funct7) = insn.funct7 {
        encoding.push_str(&format!("  funct7 0b{funct7:07b}"));
    }
    if let Some(imm12) = insn.imm12 {
        encoding.push_str(&format!("  imm12 0b{imm12:012b}"));
    }
    println!("  encoding   {encoding}");

    let (bits, mask) = insn.fixed();
    println!("  fixed      bits 0x{bits:08x}  mask 0x{mask:08x}");

    let layout = insn
        .format
        .fields()
        .iter()
        .map(|(name, hi, lo)| format!("{name}[{hi}:{lo}]"))
        .collect::<Vec<_>>()
        .join(" ");
    println!("  layout     {layout}");

    let operands = if insn.operands.is_empty() {
        "none".to_string()
    } else {
        insn.operands
            .iter()
            .map(render_operand)
            .collect::<Vec<_>>()
            .join(" ")
    };
    println!("  operands   {operands}");
    println!("  content    {}", insn.content_key("summary"));
}

fn render_operand(operand: &Operand) -> String {
    let mut out = format!("{}:{}", operand.kind.as_str(), operand.bits);
    if operand.signed {
        out.push('s');
    }
    if operand.align > 1 {
        out.push_str(&format!(" align {}", operand.align));
    }
    out
}

fn to_json(insn: &Insn) -> String {
    let (bits, mask) = insn.fixed();
    let operands = insn
        .operands
        .iter()
        .map(|operand| {
            object(vec![
                ("kind", string(operand.kind.as_str())),
                ("bits", number(operand.bits as f64)),
                ("signed", Json::Bool(operand.signed)),
                ("align", number(operand.align as f64)),
            ])
        })
        .collect();
    object(vec![
        ("id", string(insn.id)),
        ("mnemonic", string(insn.mnemonic)),
        ("ext", string(insn.ext)),
        ("format", string(insn.format.as_str())),
        ("opcode", number(insn.opcode as f64)),
        (
            "funct3",
            insn.funct3.map_or(Json::Null, |value| number(value as f64)),
        ),
        (
            "funct7",
            insn.funct7.map_or(Json::Null, |value| number(value as f64)),
        ),
        (
            "imm12",
            insn.imm12.map_or(Json::Null, |value| number(value as f64)),
        ),
        ("class", string(insn.class)),
        ("bits", number(bits as f64)),
        ("mask", number(mask as f64)),
        ("content_key", string(insn.content_key("summary"))),
        ("operands", Json::Array(operands)),
    ])
    .to_string()
}

fn list(only_ext: Option<&str>) -> ExitCode {
    if let Some(ext) = only_ext {
        if !rvh_isa::extensions().contains(&ext) {
            eprintln!("rvh: unknown extension `{ext}`");
            return ExitCode::FAILURE;
        }
    }
    println!("{:<8} {:<6} {:<7} class", "mnemonic", "ext", "format");
    let mut count = 0;
    for insn in rvh_isa::all() {
        if only_ext.is_some_and(|ext| ext != insn.ext) {
            continue;
        }
        println!(
            "{:<8} {:<6} {:<7} {}",
            insn.mnemonic,
            insn.ext,
            insn.format.as_str(),
            insn.class
        );
        count += 1;
    }
    println!("\n{count} instructions");
    ExitCode::SUCCESS
}

fn extensions() -> ExitCode {
    println!("{:<8} instructions", "ext");
    for ext in rvh_isa::extensions() {
        let count = rvh_isa::insns_in(ext).count();
        println!("{:<8} {}", ext, count);
    }
    ExitCode::SUCCESS
}

fn decode(text: &str) -> ExitCode {
    let digits = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X"));
    let parsed = match digits {
        Some(hex) => u32::from_str_radix(hex, 16),
        None => text.parse::<u32>(),
    };
    let Ok(word) = parsed else {
        eprintln!("rvh: `{text}` is not a 32-bit word");
        return ExitCode::from(2);
    };
    // A compressed instruction is half a word, so it is tried first (ADR-0018).
    let printed = if rvh_isa::Insn::is_compressed(word) {
        match rvh_sim::compressed::decode(word as u16) {
            Some(decoded) => {
                println!(
                    "0x{:04x}  {}  (extension {}, format {})",
                    word as u16,
                    rvh_sim::disasm::word(word)
                        .unwrap_or_else(|| decoded.insn.mnemonic.to_string()),
                    decoded.insn.ext,
                    decoded.insn.format.as_str()
                );
                true
            }
            None => false,
        }
    } else {
        let found = rvh_isa::decode(word);
        if found.is_empty() {
            false
        } else {
            let rendered = rvh_sim::disasm::word(word);
            for insn in found {
                println!(
                    "0x{word:08x}  {}  (extension {}, format {})",
                    rendered
                        .clone()
                        .unwrap_or_else(|| insn.mnemonic.to_string()),
                    insn.ext,
                    insn.format.as_str()
                );
            }
            true
        }
    };
    if !printed {
        println!("0x{word:08x}  <no match>");
    }
    ExitCode::SUCCESS
}

fn parse_file(path: &str, show_tree: bool, json: bool) -> ExitCode {
    let source = match std::fs::read_to_string(path) {
        Ok(source) => source,
        Err(error) => {
            eprintln!("rvh: cannot read {path}: {error}");
            return ExitCode::FAILURE;
        }
    };
    let tree = rvh_syntax::parse(&source);
    let diagnostics = tree.diagnostics();

    if show_tree {
        print!("{}", tree.render());
    }

    if json {
        let items = diagnostics
            .iter()
            .map(|diagnostic| {
                let (line, column) = line_col(&source, diagnostic.span.0);
                let fixes = diagnostic
                    .fixes()
                    .iter()
                    .map(|fix| {
                        object(vec![
                            ("title", string(fix.title.clone())),
                            ("start", number(fix.span.0 as f64)),
                            ("end", number(fix.span.1 as f64)),
                            ("replacement", string(fix.replacement.clone())),
                        ])
                    })
                    .collect();
                object(vec![
                    ("code", string(diagnostic.code)),
                    ("message", string(diagnostic.message.clone())),
                    ("line", number(line as f64)),
                    ("column", number(column as f64)),
                    ("start", number(diagnostic.span.0 as f64)),
                    ("end", number(diagnostic.span.1 as f64)),
                    ("fixes", Json::Array(fixes)),
                ])
            })
            .collect();
        println!("{}", object(vec![("diagnostics", Json::Array(items))]));
    } else {
        print_diagnostics(path, &source, diagnostics);
        if !show_tree {
            println!("{path}: {count} diagnostic(s)", count = diagnostics.len());
        }
    }

    if diagnostics.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn print_diagnostics(path: &str, source: &str, diagnostics: &[rvh_syntax::Diagnostic]) {
    for diagnostic in diagnostics {
        let (line, column) = line_col(source, diagnostic.span.0);
        println!(
            "{path}:{line}:{column}: {}: {}",
            diagnostic.code, diagnostic.message
        );
        for fix in diagnostic.fixes() {
            let (fix_line, fix_column) = line_col(source, fix.span.0);
            println!("{path}:{fix_line}:{fix_column}: help: {}", fix.title);
        }
    }
}

fn asm_command(args: &[&str]) -> ExitCode {
    let path = args[0];
    let mut output: Option<&str> = None;
    let mut text_output: Option<&str> = None;
    let mut elf_output: Option<&str> = None;
    let mut show_symbols = false;
    let mut target_name = "rv32i_zicsr";

    let mut index = 1;
    while index < args.len() {
        match args[index] {
            "-o" => match args.get(index + 1) {
                Some(value) => {
                    output = Some(value);
                    index += 1;
                }
                None => {
                    eprintln!("rvh: `-o` needs a path");
                    return ExitCode::from(2);
                }
            },
            "--text" => match args.get(index + 1) {
                Some(value) => {
                    text_output = Some(value);
                    index += 1;
                }
                None => {
                    eprintln!("rvh: `--text` needs a path");
                    return ExitCode::from(2);
                }
            },
            "--elf" => match args.get(index + 1) {
                Some(value) => {
                    elf_output = Some(value);
                    index += 1;
                }
                None => {
                    eprintln!("rvh: `--elf` needs a path");
                    return ExitCode::from(2);
                }
            },
            "--symbols" => show_symbols = true,
            "--target" => match args.get(index + 1) {
                Some(value) => {
                    target_name = value;
                    index += 1;
                }
                None => {
                    eprintln!("rvh: `--target` needs an architecture");
                    return ExitCode::from(2);
                }
            },
            other => {
                eprintln!("rvh: unknown flag `{other}`");
                return ExitCode::from(2);
            }
        }
        index += 1;
    }

    let source = match std::fs::read_to_string(path) {
        Ok(source) => source,
        Err(error) => {
            eprintln!("rvh: cannot read {path}: {error}");
            return ExitCode::FAILURE;
        }
    };
    let target = match rvh_isa::Target::parse(target_name) {
        Ok(target) => target,
        Err(error) => {
            eprintln!("rvh: {error}");
            return ExitCode::from(2);
        }
    };

    let mut session = rvh_session::Session::new(target);
    session.set_source(&source);
    print_diagnostics(path, &source, session.diagnostics());

    // Do not write bytes for a file we could not fully resolve: a silently
    // wrong image is worse than no image.
    if !session.diagnostics().is_empty() && (output.is_some() || text_output.is_some()) {
        eprintln!("rvh: refusing to write output while diagnostics remain");
        return ExitCode::FAILURE;
    }

    if let Some(path) = elf_output {
        let bytes = session.image().to_elf();
        if let Err(error) = std::fs::write(path, &bytes) {
            eprintln!("rvh: cannot write {path}: {error}");
            return ExitCode::FAILURE;
        }
        println!("wrote {path} ({} bytes)", bytes.len());
    }

    if let Some(output) = output {
        let bytes = session.image().to_bytes();
        if let Err(error) = std::fs::write(output, &bytes) {
            eprintln!("rvh: cannot write {output}: {error}");
            return ExitCode::FAILURE;
        }
        println!("wrote {output} ({} bytes)", bytes.len());
    }
    if let Some(output) = text_output {
        let bytes = session.image().text();
        if let Err(error) = std::fs::write(output, bytes) {
            eprintln!("rvh: cannot write {output}: {error}");
            return ExitCode::FAILURE;
        }
        println!("wrote {output} ({} bytes of .text)", bytes.len());
    }
    if show_symbols {
        for symbol in &session.image().symbols {
            println!("0x{:08x}  {}", symbol.addr, symbol.name);
        }
    }

    println!(
        "{path}: {} word(s) in .text, entry 0x{:08x}, {} diagnostic(s)",
        session.image().text().len() / 4,
        session.image().entry,
        session.diagnostics().len()
    );

    if session.diagnostics().is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn run_command(args: &[&str]) -> ExitCode {
    let path = args[0];
    let mut max_steps = 1_000_000u64;
    let mut target_name = "rv32i_zicsr";
    let mut board_name = "generic";
    let mut trace = false;

    let mut index = 1;
    while index < args.len() {
        match args[index] {
            "--max-steps" => match args
                .get(index + 1)
                .and_then(|text| text.parse::<u64>().ok())
            {
                Some(value) => {
                    max_steps = value;
                    index += 1;
                }
                None => {
                    eprintln!("rvh: `--max-steps` needs a number");
                    return ExitCode::from(2);
                }
            },
            "--target" => match args.get(index + 1) {
                Some(value) => {
                    target_name = value;
                    index += 1;
                }
                None => {
                    eprintln!("rvh: `--target` needs an architecture");
                    return ExitCode::from(2);
                }
            },
            "--trace" => trace = true,
            "--board" => match args.get(index + 1) {
                Some(value) => {
                    board_name = value;
                    index += 1;
                }
                None => {
                    eprintln!("rvh: `--board` needs a name");
                    return ExitCode::from(2);
                }
            },
            other => {
                eprintln!("rvh: unknown flag `{other}`");
                return ExitCode::from(2);
            }
        }
        index += 1;
    }

    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) => {
            eprintln!("rvh: cannot read {path}: {error}");
            return ExitCode::FAILURE;
        }
    };
    let is_elf = bytes.starts_with(b"\x7fELF");
    let target = match rvh_isa::Target::parse(target_name) {
        Ok(target) => target,
        Err(error) => {
            eprintln!("rvh: {error}");
            return ExitCode::from(2);
        }
    };

    let mut session = rvh_session::Session::new(target);
    match rvh_sim::board(board_name) {
        Some(board) => session.set_board(board),
        None => {
            let names: Vec<&str> = rvh_sim::BOARDS.iter().map(|board| board.name).collect();
            eprintln!(
                "rvh: unknown board `{board_name}`; try one of {}",
                names.join(", ")
            );
            return ExitCode::from(2);
        }
    }
    if is_elf {
        if let Err(error) = session.load_elf(&bytes) {
            eprintln!("rvh: {path}: {error}");
            return ExitCode::FAILURE;
        }
    } else {
        let source = String::from_utf8_lossy(&bytes);
        session.set_source(&source);
        if !session.diagnostics().is_empty() {
            print_diagnostics(path, &source, session.diagnostics());
            eprintln!("rvh: refusing to run a program with diagnostics");
            return ExitCode::FAILURE;
        }
    }
    if trace {
        // One line per step, in the shape spike's commit log can be reduced to.
        let mut steps = 0u64;
        while steps < max_steps {
            let before = session.machine().map_or(0, rvh_sim::Machine::steps);
            if !session.step(1) {
                break;
            }
            steps += 1;
            if let Some(machine) = session.machine() {
                if let Some(step) = machine.trace().last() {
                    if machine.steps() > before {
                        println!("{}", machine.describe_step(step));
                    }
                }
            }
        }
        let halt = session.machine().and_then(|machine| machine.halt());
        eprintln!("{}", describe_halt(halt));
        return ExitCode::SUCCESS;
    }
    let halt = session.run(max_steps);
    let Some(machine) = session.machine() else {
        eprintln!("rvh: no machine state");
        return ExitCode::FAILURE;
    };

    if !machine.output().is_empty() {
        println!("--- output ---");
        print!("{}", machine.output_text());
        if !machine.output_text().ends_with('\n') {
            println!();
        }
    }

    println!("--- halt ---");
    println!("{} after {} step(s)", describe_halt(halt), machine.steps());

    println!("--- registers ---");
    for (number, name) in REGISTER_NAMES.iter().enumerate() {
        let value = machine.reg(number);
        if value != 0 {
            println!("{name:<4} x{number:<2} 0x{value:08x}  {}", value as i32);
        }
    }
    println!("pc   0x{:08x}", machine.pc());

    match halt {
        Some(rvh_sim::HaltReason::Ebreak { .. } | rvh_sim::HaltReason::Ecall { .. }) => {
            ExitCode::SUCCESS
        }
        _ => ExitCode::FAILURE,
    }
}

fn describe_halt(halt: Option<rvh_sim::HaltReason>) -> String {
    match halt {
        Some(rvh_sim::HaltReason::Ebreak { pc }) => format!("ebreak at 0x{pc:08x}"),
        Some(rvh_sim::HaltReason::Ecall { pc }) => {
            format!("ecall at 0x{pc:08x} (no syscall table until M7)")
        }
        Some(rvh_sim::HaltReason::Illegal { pc, word }) => {
            format!("illegal word 0x{word:08x} at 0x{pc:08x}")
        }
        Some(rvh_sim::HaltReason::Fault { pc, addr }) => {
            format!("memory fault at 0x{addr:08x} from 0x{pc:08x}")
        }
        Some(rvh_sim::HaltReason::StepLimit) => "step limit reached".to_string(),
        None => "still running".to_string(),
    }
}

fn hover_command(path: &str, line: &str, column: &str, locale: &str) -> ExitCode {
    let (Ok(line), Ok(column)) = (line.parse::<usize>(), column.parse::<usize>()) else {
        eprintln!("rvh: line and column must be numbers");
        return ExitCode::from(2);
    };
    let source = match std::fs::read_to_string(path) {
        Ok(source) => source,
        Err(error) => {
            eprintln!("rvh: cannot read {path}: {error}");
            return ExitCode::FAILURE;
        }
    };
    let Some(offset) = offset_of(&source, line, column) else {
        eprintln!("rvh: {path}: no such position {line}:{column}");
        return ExitCode::from(2);
    };

    let mut session =
        rvh_session::Session::new(rvh_isa::Target::parse("rv32i_zicsr").expect("default target"));
    session.set_source(&source);
    session.set_locale(locale);

    match session.hover(offset) {
        Some(hover) => {
            println!("{} ({})", hover.title, hover.kind);
            println!();
            println!("{}", hover.text);
            ExitCode::SUCCESS
        }
        None => {
            println!("nothing at {line}:{column}");
            ExitCode::SUCCESS
        }
    }
}

/// Byte offset of a 1-based line and column.
fn offset_of(source: &str, line: usize, column: usize) -> Option<usize> {
    if line == 0 || column == 0 {
        return None;
    }
    let mut offset = 0;
    for (current_line, text_line) in (1..).zip(source.split_inclusive('\n')) {
        if current_line == line {
            let column_offset = text_line
                .char_indices()
                .nth(column - 1)
                .map_or(text_line.len(), |(index, _)| index);
            return Some(offset + column_offset);
        }
        offset += text_line.len();
    }
    None
}

fn lsp_command(args: &[&str]) -> ExitCode {
    // Editors append the transport flag: `vscode-languageclient` adds `--stdio`
    // to every executable server it starts. Accept it, and say so loudly if a
    // transport we do not implement shows up, because stdout is the protocol.
    for argument in args {
        if *argument != "--stdio" {
            eprintln!("rvh: lsp: unsupported argument `{argument}`; only stdio is implemented");
            return ExitCode::from(2);
        }
    }

    let target = match rvh_isa::Target::parse("rv32i_zicsr") {
        Ok(target) => target,
        Err(error) => {
            eprintln!("rvh: {error}");
            return ExitCode::from(2);
        }
    };
    match rvh_lsp::run_stdio(target) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("rvh: lsp: {error}");
            ExitCode::FAILURE
        }
    }
}

fn dap_command() -> ExitCode {
    match rvh_dap::run_stdio() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("rvh: dap: {error}");
            ExitCode::FAILURE
        }
    }
}
