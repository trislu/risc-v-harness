//! Content overlay: flat TOML maps of short descriptions, compiled into tables.
//!
//! Files live under `content/overlay/<locale>/<kind>.toml`. Each file is a flat
//! `key = "value"` map so that a translator edits one line and nothing else.
//! Long-form prose will use Markdown with TOML front-matter (ADR-0005); these
//! one-liners do not need that machinery.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::schema::{err, Error};

/// One locale's file for one kind.
pub struct ContentFile {
    pub locale: String,
    pub kind: String,
    pub entries: Vec<(String, String)>,
}

pub fn read_dir(directory: &Path) -> Result<Vec<ContentFile>, Error> {
    let mut files = Vec::new();
    let locales = read_subdirectories(directory)?;
    for locale_dir in locales {
        let locale = file_name(&locale_dir);
        let mut kinds = Vec::new();
        for entry in std::fs::read_dir(&locale_dir)
            .map_err(|error| err(&locale_dir.display().to_string(), 0, error.to_string()))?
        {
            let path = entry
                .map_err(|error| err(&locale_dir.display().to_string(), 0, error.to_string()))?
                .path();
            if path
                .extension()
                .is_some_and(|extension| extension == "toml")
            {
                kinds.push(path);
            }
        }
        kinds.sort();
        for path in kinds {
            let kind = path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap_or_default()
                .to_string();
            let entries = parse_map(&path)?;
            files.push(ContentFile {
                locale: locale.clone(),
                kind,
                entries,
            });
        }
    }
    files.sort_by(|left, right| (&left.locale, &left.kind).cmp(&(&right.locale, &right.kind)));
    Ok(files)
}

fn read_subdirectories(directory: &Path) -> Result<Vec<PathBuf>, Error> {
    if !directory.is_dir() {
        return Ok(Vec::new());
    }
    let mut directories = Vec::new();
    for entry in std::fs::read_dir(directory)
        .map_err(|error| err(&directory.display().to_string(), 0, error.to_string()))?
    {
        let path = entry
            .map_err(|error| err(&directory.display().to_string(), 0, error.to_string()))?
            .path();
        if path.is_dir() {
            directories.push(path);
        }
    }
    directories.sort();
    Ok(directories)
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_string()
}

/// Parse a flat `key = "value"` map.
pub fn parse_map(path: &Path) -> Result<Vec<(String, String)>, Error> {
    let file = path.display().to_string();
    let text = std::fs::read_to_string(path).map_err(|error| err(&file, 0, error.to_string()))?;
    let mut entries = Vec::new();
    let mut seen = BTreeMap::new();

    for (index, raw_line) in text.lines().enumerate() {
        let line_number = index + 1;
        let line = strip_comment(raw_line).trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with('[') {
            return Err(err(
                &file,
                line_number,
                "content files are flat maps; tables are not supported",
            ));
        }
        let Some((key, value)) = split_assignment(line) else {
            return Err(err(
                &file,
                line_number,
                format!("expected `key = \"value\"`, found `{line}`"),
            ));
        };
        let key = parse_key(&file, line_number, key)?;
        let value = crate::schema::parse_string(&file, line_number, value)?;
        if seen.insert(key.clone(), line_number).is_some() {
            return Err(err(&file, line_number, format!("duplicate key `{key}`")));
        }
        entries.push((key, value));
    }

    if entries.is_empty() {
        return Err(err(&file, 1, "no entries"));
    }
    Ok(entries)
}

fn strip_comment(line: &str) -> &str {
    let mut in_string = false;
    let mut escaped = false;
    for (index, character) in line.char_indices() {
        match character {
            '\\' if in_string => escaped = !escaped,
            '"' if !escaped => in_string = !in_string,
            '#' if !in_string => return &line[..index],
            _ => escaped = false,
        }
    }
    line
}

fn split_assignment(line: &str) -> Option<(&str, &str)> {
    let mut in_string = false;
    let mut escaped = false;
    for (index, character) in line.char_indices() {
        match character {
            '\\' if in_string => escaped = !escaped,
            '"' if !escaped => in_string = !in_string,
            '=' if !in_string => return Some((line[..index].trim(), line[index + 1..].trim())),
            _ => escaped = false,
        }
    }
    None
}

fn parse_key(file: &str, line: usize, key: &str) -> Result<String, Error> {
    if key.starts_with('"') {
        return crate::schema::parse_string(file, line, key);
    }
    if key.is_empty() || key.contains(char::is_whitespace) {
        return Err(err(file, line, format!("invalid key `{key}`")));
    }
    Ok(key.to_string())
}

/// Render all content files into a Rust module.
pub fn render(files: &[ContentFile]) -> String {
    let mut out = String::new();
    out.push_str("// @generated by `cargo xtask codegen` from content/overlay; do not edit.\n");
    out.push_str("//\n// Regenerate with `cargo xtask codegen`.\n\n");

    for file in files {
        // rustfmt must not touch the tables: `codegen --check` compares bytes.
        out.push_str("#[rustfmt::skip]\n");
        out.push_str(&format!(
            "pub static {}_{}: &[(&str, &str)] = &[\n",
            locale_static(&file.locale),
            kind_static(&file.kind)
        ));
        for (key, value) in &file.entries {
            out.push_str(&format!("    ({key:?}, {value:?}),\n"));
        }
        out.push_str("];\n\n");
    }

    out.push_str("/// The table for a locale and kind, if the overlay provides one.\n");
    out.push_str("#[must_use]\n");
    out.push_str(
        "pub fn table(locale: &str, kind: &str) -> Option<&'static [(&'static str, &'static str)]> {\n",
    );
    out.push_str("    match (locale, kind) {\n");
    for file in files {
        out.push_str(&format!(
            "        ({:?}, {:?}) => Some({}_{}),\n",
            file.locale,
            file.kind,
            locale_static(&file.locale),
            kind_static(&file.kind)
        ));
    }
    out.push_str("        _ => None,\n    }\n}\n");
    out
}

fn locale_static(locale: &str) -> String {
    locale.to_uppercase().replace('-', "_")
}

fn kind_static(kind: &str) -> String {
    kind.to_uppercase().replace('-', "_")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_a_table_and_a_lookup() {
        let files = vec![ContentFile {
            locale: "zh-CN".to_string(),
            kind: "pseudos".to_string(),
            entries: vec![("li".to_string(), "加载".to_string())],
        }];
        let rendered = render(&files);
        assert!(rendered.contains("pub static ZH_CN_PSEUDOS"));
        assert!(rendered.contains("(\"li\", \"加载\")"));
        assert!(rendered.contains("(\"zh-CN\", \"pseudos\") => Some(ZH_CN_PSEUDOS)"));
    }
}
