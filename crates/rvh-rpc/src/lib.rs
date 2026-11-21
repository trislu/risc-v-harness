//! Content-Length framing, as both the Language Server Protocol and the Debug
//! Adapter Protocol use it: headers, a blank line, then a UTF-8 body.

#![forbid(unsafe_code)]

use std::io::{BufRead, Write};

/// Read one message body. `Ok(None)` means the stream ended.
pub fn read_message(reader: &mut impl BufRead) -> std::io::Result<Option<String>> {
    let mut length: Option<usize> = None;
    loop {
        let mut header = String::new();
        if reader.read_line(&mut header)? == 0 {
            return Ok(None);
        }
        let header = header.trim_end();
        if header.is_empty() {
            break;
        }
        if let Some(rest) = header.strip_prefix("Content-Length:") {
            length = rest.trim().parse::<usize>().ok();
        }
    }
    let Some(length) = length else {
        return Ok(None);
    };
    let mut body = vec![0u8; length];
    reader.read_exact(&mut body)?;
    Ok(Some(String::from_utf8_lossy(&body).into_owned()))
}

/// Write one message body with its header, and flush.
pub fn write_message(writer: &mut impl Write, body: &str) -> std::io::Result<()> {
    write!(writer, "Content-Length: {}\r\n\r\n{body}", body.len())?;
    writer.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_a_message() {
        let mut framed = Vec::new();
        write_message(&mut framed, "{\"a\":1}").expect("write");
        let mut reader = std::io::Cursor::new(framed);
        assert_eq!(
            read_message(&mut reader).expect("read"),
            Some("{\"a\":1}".to_string())
        );
        assert_eq!(read_message(&mut reader).expect("eof"), None);
    }
}
