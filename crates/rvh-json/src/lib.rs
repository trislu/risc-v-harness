//! A small JSON reader and writer.
//!
//! LSP is JSON-RPC, so the server has to parse incoming messages. The workspace
//! builds without external crates, so this is a hand-written parser for the
//! subset LSP uses: objects, arrays, strings, numbers, booleans and null. It is
//! strict about syntax and reports the byte offset on failure.

use std::fmt;

/// A JSON value. Objects keep their key order so that output stays stable.
#[derive(Debug, Clone, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Array(Vec<Json>),
    Object(Vec<(String, Json)>),
}

impl Json {
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Self::Object(entries) => entries
                .iter()
                .find(|(name, _)| name == key)
                .map(|(_, value)| value),
            _ => None,
        }
    }

    #[must_use]
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(value) => Some(value),
            _ => None,
        }
    }

    #[must_use]
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Self::Number(value) => Some(*value),
            _ => None,
        }
    }

    /// Integer access, rejecting fractional values.
    #[must_use]
    pub fn as_u32(&self) -> Option<u32> {
        let value = self.as_f64()?;
        if value < 0.0 || value.fract() != 0.0 || value > f64::from(u32::MAX) {
            return None;
        }
        Some(value as u32)
    }

    /// The entries of an object, for callers that iterate a map.
    #[must_use]
    pub fn as_object(&self) -> Option<&[(String, Json)]> {
        match self {
            Self::Object(entries) => Some(entries),
            _ => None,
        }
    }

    #[must_use]
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(value) => Some(*value),
            _ => None,
        }
    }

    #[must_use]
    pub fn as_array(&self) -> Option<&[Json]> {
        match self {
            Self::Array(items) => Some(items),
            _ => None,
        }
    }

    /// Path lookup: `value.at(&["params", "textDocument", "uri"])`.
    #[must_use]
    pub fn at(&self, path: &[&str]) -> Option<&Json> {
        let mut current = self;
        for key in path {
            current = current.get(key)?;
        }
        Some(current)
    }
}

/// Build an object from borrowed keys. Keeps call sites readable.
#[must_use]
pub fn object(entries: Vec<(&str, Json)>) -> Json {
    Json::Object(
        entries
            .into_iter()
            .map(|(key, value)| (key.to_string(), value))
            .collect(),
    )
}

/// Short string constructor.
#[must_use]
pub fn string(value: impl Into<String>) -> Json {
    Json::String(value.into())
}

/// Short number constructor. Callers convert to `f64`; JSON has one number type.
#[must_use]
pub fn number(value: impl Into<f64>) -> Json {
    Json::Number(value.into())
}

impl fmt::Display for Json {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Null => formatter.write_str("null"),
            Self::Bool(value) => write!(formatter, "{value}"),
            Self::Number(value) => {
                if value.fract() == 0.0 && value.abs() < 1e15 {
                    write!(formatter, "{}", *value as i64)
                } else {
                    write!(formatter, "{value}")
                }
            }
            Self::String(value) => write!(formatter, "\"{}\"", escape_string(value)),
            Self::Array(items) => {
                formatter.write_str("[")?;
                for (index, item) in items.iter().enumerate() {
                    if index > 0 {
                        formatter.write_str(",")?;
                    }
                    write!(formatter, "{item}")?;
                }
                formatter.write_str("]")
            }
            Self::Object(entries) => {
                formatter.write_str("{")?;
                for (index, (key, value)) in entries.iter().enumerate() {
                    if index > 0 {
                        formatter.write_str(",")?;
                    }
                    write!(formatter, "\"{}\":{value}", escape_string(key))?;
                }
                formatter.write_str("}")
            }
        }
    }
}

/// Escape a string for JSON output, without the surrounding quotes.
#[must_use]
pub fn escape_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            other if (other as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", other as u32)),
            other => out.push(other),
        }
    }
    out
}

/// Parse one JSON document. Requires the `parse` feature.
#[cfg(feature = "parse")]
pub fn parse(text: &str) -> Result<Json, String> {
    let mut parser = Parser {
        bytes: text.as_bytes(),
        position: 0,
    };
    parser.skip_whitespace();
    let value = parser.value()?;
    parser.skip_whitespace();
    if parser.position != parser.bytes.len() {
        return Err(format!("trailing data at byte {}", parser.position));
    }
    Ok(value)
}

#[cfg(feature = "parse")]
struct Parser<'a> {
    bytes: &'a [u8],
    position: usize,
}

#[cfg(feature = "parse")]
impl Parser<'_> {
    fn skip_whitespace(&mut self) {
        while matches!(
            self.bytes.get(self.position),
            Some(b' ' | b'\t' | b'\n' | b'\r')
        ) {
            self.position += 1;
        }
    }

    fn value(&mut self) -> Result<Json, String> {
        match self.bytes.get(self.position) {
            None => Err("unexpected end of input".to_string()),
            Some(b'n') => self.literal("null", Json::Null),
            Some(b't') => self.literal("true", Json::Bool(true)),
            Some(b'f') => self.literal("false", Json::Bool(false)),
            Some(b'"') => self.string().map(Json::String),
            Some(b'[') => self.array(),
            Some(b'{') => self.object(),
            Some(b'-' | b'0'..=b'9') => self.number(),
            Some(other) => Err(format!(
                "unexpected byte `{}` at {}",
                *other as char, self.position
            )),
        }
    }

    fn literal(&mut self, text: &str, value: Json) -> Result<Json, String> {
        if self.bytes[self.position..].starts_with(text.as_bytes()) {
            self.position += text.len();
            Ok(value)
        } else {
            Err(format!("expected `{text}` at {}", self.position))
        }
    }

    fn string(&mut self) -> Result<String, String> {
        self.position += 1; // opening quote
        let mut out = String::new();
        loop {
            let Some(&byte) = self.bytes.get(self.position) else {
                return Err("unterminated string".to_string());
            };
            match byte {
                b'"' => {
                    self.position += 1;
                    return Ok(out);
                }
                b'\\' => {
                    self.position += 1;
                    self.escape(&mut out)?;
                }
                _ => {
                    let start = self.position;
                    let mut end = start;
                    while matches!(self.bytes.get(end), Some(byte) if *byte != b'"' && *byte != b'\\')
                    {
                        end += 1;
                    }
                    let text = std::str::from_utf8(&self.bytes[start..end])
                        .map_err(|_| format!("invalid UTF-8 at {start}"))?;
                    out.push_str(text);
                    self.position = end;
                }
            }
        }
    }

    fn escape(&mut self, out: &mut String) -> Result<(), String> {
        let Some(&byte) = self.bytes.get(self.position) else {
            return Err("unterminated escape".to_string());
        };
        self.position += 1;
        match byte {
            b'"' => out.push('"'),
            b'\\' => out.push('\\'),
            b'/' => out.push('/'),
            b'b' => out.push('\u{8}'),
            b'f' => out.push('\u{c}'),
            b'n' => out.push('\n'),
            b'r' => out.push('\r'),
            b't' => out.push('\t'),
            b'u' => {
                let first = self.hex4()?;
                let code = if (0xd800..0xdc00).contains(&first) {
                    if !self.bytes[self.position..].starts_with(b"\\u") {
                        return Err("unpaired surrogate".to_string());
                    }
                    self.position += 2;
                    let second = self.hex4()?;
                    if !(0xdc00..0xe000).contains(&second) {
                        return Err("invalid surrogate pair".to_string());
                    }
                    0x1_0000 + ((first - 0xd800) << 10) + (second - 0xdc00)
                } else {
                    first
                };
                out.push(char::from_u32(code).ok_or_else(|| "invalid code point".to_string())?);
            }
            other => return Err(format!("unknown escape `\\{}`", other as char)),
        }
        Ok(())
    }

    fn hex4(&mut self) -> Result<u32, String> {
        let Some(slice) = self.bytes.get(self.position..self.position + 4) else {
            return Err("truncated \\u escape".to_string());
        };
        let text = std::str::from_utf8(slice).map_err(|_| "invalid \\u escape".to_string())?;
        let value = u32::from_str_radix(text, 16).map_err(|_| "invalid \\u escape".to_string())?;
        self.position += 4;
        Ok(value)
    }

    fn array(&mut self) -> Result<Json, String> {
        self.position += 1;
        let mut items = Vec::new();
        self.skip_whitespace();
        if self.bytes.get(self.position) == Some(&b']') {
            self.position += 1;
            return Ok(Json::Array(items));
        }
        loop {
            self.skip_whitespace();
            items.push(self.value()?);
            self.skip_whitespace();
            match self.bytes.get(self.position) {
                Some(b',') => self.position += 1,
                Some(b']') => {
                    self.position += 1;
                    return Ok(Json::Array(items));
                }
                _ => return Err(format!("expected `,` or `]` at {}", self.position)),
            }
        }
    }

    fn object(&mut self) -> Result<Json, String> {
        self.position += 1;
        let mut entries = Vec::new();
        self.skip_whitespace();
        if self.bytes.get(self.position) == Some(&b'}') {
            self.position += 1;
            return Ok(Json::Object(entries));
        }
        loop {
            self.skip_whitespace();
            if self.bytes.get(self.position) != Some(&b'"') {
                return Err(format!("expected a key at {}", self.position));
            }
            let key = self.string()?;
            self.skip_whitespace();
            if self.bytes.get(self.position) != Some(&b':') {
                return Err(format!("expected `:` at {}", self.position));
            }
            self.position += 1;
            self.skip_whitespace();
            let value = self.value()?;
            entries.push((key, value));
            self.skip_whitespace();
            match self.bytes.get(self.position) {
                Some(b',') => self.position += 1,
                Some(b'}') => {
                    self.position += 1;
                    return Ok(Json::Object(entries));
                }
                _ => return Err(format!("expected `,` or `}}` at {}", self.position)),
            }
        }
    }

    fn number(&mut self) -> Result<Json, String> {
        let start = self.position;
        if self.bytes.get(self.position) == Some(&b'-') {
            self.position += 1;
        }
        while matches!(
            self.bytes.get(self.position),
            Some(b'0'..=b'9' | b'.' | b'e' | b'E' | b'+' | b'-')
        ) {
            self.position += 1;
        }
        let text = std::str::from_utf8(&self.bytes[start..self.position])
            .map_err(|_| "invalid number".to_string())?;
        text.parse::<f64>()
            .map(Json::Number)
            .map_err(|_| format!("invalid number `{text}`"))
    }
}

#[cfg(all(test, feature = "parse"))]
mod tests {
    use super::*;

    #[test]
    fn parses_a_request() {
        let value = parse(
            r#"{"jsonrpc":"2.0","id":1,"method":"textDocument/hover","params":{"position":{"line":2,"character":5}}}"#,
        )
        .expect("valid");
        assert_eq!(
            value.get("method").and_then(Json::as_str),
            Some("textDocument/hover")
        );
        assert_eq!(
            value
                .at(&["params", "position", "line"])
                .and_then(Json::as_u32),
            Some(2)
        );
        assert_eq!(value.get("id").and_then(Json::as_u32), Some(1));
    }

    #[test]
    fn round_trips_strings_and_escapes() {
        let original = "a \"quoted\" line\nwith \\ and 中文";
        let encoded = format!("\"{}\"", escape_string(original));
        assert_eq!(parse(&encoded), Ok(Json::String(original.to_string())));
    }

    #[test]
    fn handles_surrogate_pairs() {
        let value = parse(r#""\ud83d\ude00""#).expect("valid");
        assert_eq!(value, Json::String("😀".to_string()));
    }

    #[test]
    fn rejects_trailing_data() {
        assert!(parse("{}x").is_err());
    }

    #[test]
    fn displays_compact_json() {
        let value = Json::Object(vec![
            ("a".to_string(), Json::Number(1.0)),
            (
                "b".to_string(),
                Json::Array(vec![Json::Null, Json::Bool(true)]),
            ),
        ]);
        assert_eq!(value.to_string(), r#"{"a":1,"b":[null,true]}"#);
    }
}
