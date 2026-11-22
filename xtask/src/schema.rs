//! A strict reader for the subset of TOML used by `isa/*.toml`.
//!
//! The workspace builds without external dependencies, so this is not a general
//! TOML parser. It accepts `[[insn]]` tables with a fixed key set and rejects
//! everything else. A typo then becomes an error instead of a silent default.

use std::fmt;
use std::path::Path;

/// Every key an `[[insn]]` record may carry.
pub const FIELDS: &[&str] = &[
    "id", "mnemonic", "ext", "format", "opcode", "funct3", "funct7", "imm12", "class", "operands",
];

const FORMATS: &[&str] = &[
    "R", "I", "S", "B", "U", "J", "CI", "CR", "CSS", "CIW", "CL", "CS", "CA", "CB", "CJ",
];

#[derive(Debug)]
pub struct Error {
    pub file: String,
    pub line: usize,
    pub message: String,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}: {}", self.file, self.line, self.message)
    }
}

impl std::error::Error for Error {}

pub fn err(file: &str, line: usize, message: impl Into<String>) -> Error {
    Error {
        file: file.to_string(),
        line,
        message: message.into(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Operand {
    pub kind: String,
    pub bits: u8,
    pub signed: bool,
    pub align: u8,
}

#[derive(Debug)]
pub struct Insn {
    pub id: String,
    pub mnemonic: String,
    pub ext: String,
    pub format: String,
    pub opcode: u32,
    pub funct3: Option<u32>,
    pub funct7: Option<u32>,
    pub imm12: Option<u32>,
    pub class: String,
    pub operands: Vec<Operand>,
    pub line: usize,
}

#[derive(Debug, Default)]
struct Raw {
    line: usize,
    id: Option<String>,
    mnemonic: Option<String>,
    ext: Option<String>,
    format: Option<String>,
    opcode: Option<u32>,
    funct3: Option<u32>,
    funct7: Option<u32>,
    imm12: Option<u32>,
    class: Option<String>,
    operands: Option<Vec<String>>,
}

pub fn parse_file(path: &Path) -> Result<Vec<Insn>, Error> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| err(&path.display().to_string(), 0, e.to_string()))?;
    parse_str(&path.display().to_string(), &text)
}

pub fn parse_str(file: &str, text: &str) -> Result<Vec<Insn>, Error> {
    let mut out = Vec::new();
    let mut current: Option<Raw> = None;

    for (idx, raw_line) in text.lines().enumerate() {
        let line_no = idx + 1;
        let line = strip_comment(raw_line).trim();
        if line.is_empty() {
            continue;
        }

        if line.starts_with('[') {
            if line != "[[insn]]" {
                return Err(err(
                    file,
                    line_no,
                    format!("unsupported table `{line}`; only `[[insn]]` is allowed"),
                ));
            }
            if let Some(done) = current.take() {
                out.push(finish(file, done)?);
            }
            current = Some(Raw {
                line: line_no,
                ..Raw::default()
            });
            continue;
        }

        let Some(rec) = current.as_mut() else {
            return Err(err(
                file,
                line_no,
                format!("key outside of an `[[insn]]` table: `{line}`"),
            ));
        };

        let Some((key, value)) = line.split_once('=') else {
            return Err(err(
                file,
                line_no,
                format!("expected `key = value`, found `{line}`"),
            ));
        };
        let key = key.trim();
        let value = value.trim();
        if !FIELDS.contains(&key) {
            return Err(err(
                file,
                line_no,
                format!("unknown key `{key}`; allowed keys: {}", FIELDS.join(", ")),
            ));
        }
        assign(file, line_no, rec, key, value)?;
    }

    if let Some(done) = current.take() {
        out.push(finish(file, done)?);
    }
    if out.is_empty() {
        return Err(err(file, 1, "no `[[insn]]` records found"));
    }
    Ok(out)
}

/// Remove a trailing `#` comment without touching `#` inside a string.
fn strip_comment(line: &str) -> &str {
    let mut in_string = false;
    let mut escaped = false;
    for (i, c) in line.char_indices() {
        match c {
            '\\' if in_string => escaped = !escaped,
            '"' if !escaped => in_string = !in_string,
            '#' if !in_string => return &line[..i],
            _ => escaped = false,
        }
    }
    line
}

fn assign(file: &str, line: usize, rec: &mut Raw, key: &str, value: &str) -> Result<(), Error> {
    match key {
        "id" => set_str(file, line, &mut rec.id, key, value),
        "mnemonic" => set_str(file, line, &mut rec.mnemonic, key, value),
        "ext" => set_str(file, line, &mut rec.ext, key, value),
        "format" => set_str(file, line, &mut rec.format, key, value),
        "class" => set_str(file, line, &mut rec.class, key, value),
        "opcode" => set_int(file, line, &mut rec.opcode, key, value),
        "funct3" => set_int(file, line, &mut rec.funct3, key, value),
        "funct7" => set_int(file, line, &mut rec.funct7, key, value),
        "imm12" => set_int(file, line, &mut rec.imm12, key, value),
        "operands" => {
            if rec.operands.is_some() {
                return Err(err(file, line, "duplicate key `operands`"));
            }
            rec.operands = Some(parse_str_array(file, line, value)?);
            Ok(())
        }
        other => Err(err(file, line, format!("unknown key `{other}`"))),
    }
}

fn set_str(
    file: &str,
    line: usize,
    slot: &mut Option<String>,
    key: &str,
    value: &str,
) -> Result<(), Error> {
    if slot.is_some() {
        return Err(err(file, line, format!("duplicate key `{key}`")));
    }
    *slot = Some(parse_string(file, line, value)?);
    Ok(())
}

fn set_int(
    file: &str,
    line: usize,
    slot: &mut Option<u32>,
    key: &str,
    value: &str,
) -> Result<(), Error> {
    if slot.is_some() {
        return Err(err(file, line, format!("duplicate key `{key}`")));
    }
    *slot = Some(parse_int(file, line, value)?);
    Ok(())
}

pub fn parse_string(file: &str, line: usize, v: &str) -> Result<String, Error> {
    let text = v.trim_start();
    if !text.starts_with('"') {
        return Err(err(
            file,
            line,
            format!("expected a quoted string, found `{v}`"),
        ));
    }
    let mut out = String::new();
    let mut characters = text.char_indices();
    characters.next(); // opening quote
    while let Some((index, character)) = characters.next() {
        match character {
            '"' => {
                let rest = text[index + character.len_utf8()..].trim();
                if !rest.is_empty() {
                    return Err(err(
                        file,
                        line,
                        format!("unexpected text after string: `{rest}`"),
                    ));
                }
                return Ok(out);
            }
            '\\' => {
                let Some((_, escaped)) = characters.next() else {
                    return Err(err(file, line, "unterminated escape sequence"));
                };
                out.push(escaped);
            }
            other => out.push(other),
        }
    }
    Err(err(file, line, format!("unterminated string `{v}`")))
}

fn parse_int(file: &str, line: usize, v: &str) -> Result<u32, Error> {
    let v = v.trim();
    let (radix, digits) = if let Some(rest) = v.strip_prefix("0b") {
        (2, rest)
    } else if let Some(rest) = v.strip_prefix("0x") {
        (16, rest)
    } else {
        (10, v)
    };
    if digits.is_empty() {
        return Err(err(file, line, format!("invalid number `{v}`")));
    }
    u32::from_str_radix(digits, radix).map_err(|_| err(file, line, format!("invalid number `{v}`")))
}

fn parse_str_array(file: &str, line: usize, v: &str) -> Result<Vec<String>, Error> {
    let v = v.trim();
    let Some(inner) = v.strip_prefix('[').and_then(|s| s.strip_suffix(']')) else {
        return Err(err(
            file,
            line,
            format!("expected a string array, found `{v}`"),
        ));
    };
    if inner.trim().is_empty() {
        return Ok(Vec::new());
    }
    let bytes = inner.as_bytes();
    let mut out = Vec::new();
    let mut start = 0usize;
    let mut in_string = false;
    let mut i = 0usize;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' if in_string => i += 1,
            b'"' => in_string = !in_string,
            b',' if !in_string => {
                out.push(parse_string(file, line, inner[start..i].trim())?);
                start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    out.push(parse_string(file, line, inner[start..].trim())?);
    Ok(out)
}

fn parse_operand(file: &str, line: usize, text: &str) -> Result<Operand, Error> {
    let parts: Vec<&str> = text.split(':').collect();
    let kind = parts[0];
    let (bits, signed, align) = match kind {
        "rd" | "rs1" | "rs2" => {
            if parts.get(1) != Some(&"gpr") || parts.len() != 2 {
                return Err(err(
                    file,
                    line,
                    format!("expected `{kind}:gpr`, found `{text}`"),
                ));
            }
            (5u8, false, 1u8)
        }
        "imm" => {
            let bits = parse_bits(file, line, parts.get(1), text)?;
            let signed = match parts.get(2) {
                None | Some(&"u") => false,
                Some(&"s") => true,
                Some(other) => {
                    return Err(err(
                        file,
                        line,
                        format!("imm sign must be `s` or `u`, found `{other}`"),
                    ));
                }
            };
            let align = match parts.get(3) {
                None => 1,
                Some(a) => a
                    .parse::<u8>()
                    .map_err(|_| err(file, line, format!("bad alignment in `{text}`")))?,
            };
            (bits, signed, align)
        }
        "shamt" | "zimm" => (parse_bits(file, line, parts.get(1), text)?, false, 1),
        "csr" => (parse_bits(file, line, parts.get(1), text)?, false, 1),
        "pred" | "succ" => (parse_bits(file, line, parts.get(1), text)?, false, 1),
        other => {
            return Err(err(
                file,
                line,
                format!("unknown operand kind `{other}`; expected rd, rs1, rs2, imm, shamt, csr, zimm, pred, succ"),
            ));
        }
    };
    Ok(Operand {
        kind: kind.to_string(),
        bits,
        signed,
        align,
    })
}

fn parse_bits(file: &str, line: usize, raw: Option<&&str>, text: &str) -> Result<u8, Error> {
    let Some(raw) = raw else {
        return Err(err(file, line, format!("missing bit width in `{text}`")));
    };
    let bits = raw
        .parse::<u8>()
        .map_err(|_| err(file, line, format!("bad bit width in `{text}`")))?;
    if bits == 0 || bits > 32 {
        return Err(err(
            file,
            line,
            format!("bit width must be 1..=32 in `{text}`"),
        ));
    }
    Ok(bits)
}

fn finish(file: &str, raw: Raw) -> Result<Insn, Error> {
    let line = raw.line;
    let id = raw.id.ok_or_else(|| err(file, line, "missing `id`"))?;
    let mnemonic = raw
        .mnemonic
        .ok_or_else(|| err(file, line, format!("`{id}`: missing `mnemonic`")))?;
    let ext = raw
        .ext
        .ok_or_else(|| err(file, line, format!("`{id}`: missing `ext`")))?;
    let format = raw
        .format
        .ok_or_else(|| err(file, line, format!("`{id}`: missing `format`")))?;
    let class = raw
        .class
        .ok_or_else(|| err(file, line, format!("`{id}`: missing `class`")))?;
    let opcode = raw
        .opcode
        .ok_or_else(|| err(file, line, format!("`{id}`: missing `opcode`")))?;
    let operands = raw.operands.ok_or_else(|| {
        err(
            file,
            line,
            format!("`{id}`: missing `operands`; use `[]` for none"),
        )
    })?;

    for (field, value) in [
        ("id", &id),
        ("mnemonic", &mnemonic),
        ("ext", &ext),
        ("format", &format),
        ("class", &class),
    ] {
        if !value.is_ascii() {
            return Err(err(
                file,
                line,
                format!("`{field}` must be ASCII, found `{value}`"),
            ));
        }
    }

    if !FORMATS.contains(&format.as_str()) {
        return Err(err(
            file,
            line,
            format!(
                "`{id}`: unknown format `{format}`; expected one of {}",
                FORMATS.join(", ")
            ),
        ));
    }
    if opcode > 0x7f {
        return Err(err(
            file,
            line,
            format!("`{id}`: opcode does not fit in 7 bits"),
        ));
    }
    match format.as_str() {
        "R" => {
            if raw.funct3.is_none() || raw.funct7.is_none() {
                return Err(err(
                    file,
                    line,
                    format!("`{id}`: format R needs `funct3` and `funct7`"),
                ));
            }
        }
        "I" | "S" | "B" if raw.funct3.is_none() => {
            return Err(err(
                file,
                line,
                format!("`{id}`: format {format} needs `funct3`"),
            ));
        }
        _ => {}
    }
    if raw.imm12.is_some() && format != "I" {
        return Err(err(
            file,
            line,
            format!("`{id}`: `imm12` is only valid on format I"),
        ));
    }

    let mut parsed = Vec::with_capacity(operands.len());
    for spec in &operands {
        parsed.push(parse_operand(file, line, spec)?);
    }

    Ok(Insn {
        id,
        mnemonic,
        ext,
        format,
        opcode,
        funct3: raw.funct3,
        funct7: raw.funct7,
        imm12: raw.imm12,
        class,
        operands: parsed,
        line,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
# comment
[[insn]]
id = "addi"
mnemonic = "addi"
ext = "i"
format = "I"
opcode = 0b0010011
funct3 = 0b000
class = "alu-imm"
operands = ["rd:gpr", "rs1:gpr", "imm:12:s"]

[[insn]]
id = "ecall"
mnemonic = "ecall"
ext = "i"
format = "I"
opcode = 0b1110011
funct3 = 0b000
imm12 = 0b0
class = "system"
operands = []
"#;

    #[test]
    fn parses_two_records() {
        let got = parse_str("sample.toml", SAMPLE).expect("parses");
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].id, "addi");
        assert_eq!(got[0].operands.len(), 3);
        assert_eq!(got[0].operands[2].bits, 12);
        assert!(got[0].operands[2].signed);
        assert_eq!(got[1].imm12, Some(0));
        assert!(got[1].operands.is_empty());
    }

    #[test]
    fn rejects_unknown_key() {
        let src = SAMPLE.replace("class = \"system\"", "klass = \"system\"");
        let err = parse_str("sample.toml", &src).expect_err("must fail");
        assert!(err.message.contains("unknown key"), "{}", err.message);
    }

    #[test]
    fn rejects_missing_funct3_for_r() {
        let src = r#"
[[insn]]
id = "add"
mnemonic = "add"
ext = "i"
format = "R"
opcode = 0b0110011
class = "alu-reg"
operands = ["rd:gpr"]
"#;
        let err = parse_str("sample.toml", src).expect_err("must fail");
        assert!(err.message.contains("needs `funct3`"), "{}", err.message);
    }

    #[test]
    fn strips_comment_outside_strings_only() {
        assert_eq!(strip_comment("a = \"x#y\" # tail"), "a = \"x#y\" ");
        assert_eq!(strip_comment("# whole line"), "");
    }
}
