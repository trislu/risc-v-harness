//! Remove build artifacts, refusing to touch anything outside the repository.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// One path the cleaner considered.
pub struct Entry {
    pub path: PathBuf,
    pub bytes: u64,
    pub removed: bool,
}

/// What a run removed, or would remove.
pub struct Report {
    pub entries: Vec<Entry>,
}

impl Report {
    #[must_use]
    pub fn total_bytes(&self) -> u64 {
        self.entries.iter().map(|entry| entry.bytes).sum()
    }
}

/// Build outputs that a plain `cargo clean` does not cover.
const ALWAYS: &[&str] = &[
    "target",
    "web/dist",
    "web/public/rvh_wasm.wasm",
    "editors/vscode/out",
];

/// Extra paths that need a network or a reinstall to restore.
const EVERYTHING: &[&str] = &["web/node_modules", "editors/vscode/node_modules", "scratch"];

/// Remove the artifacts under `root`.
///
/// `all` also removes `node_modules` and `scratch`; `dry_run` only reports.
pub fn clean(root: &Path, all: bool, dry_run: bool) -> io::Result<Report> {
    let root = root.canonicalize()?;
    let mut entries = Vec::new();
    let mut paths: Vec<String> = ALWAYS.iter().map(|path| (*path).to_string()).collect();
    if all {
        paths.extend(EVERYTHING.iter().map(|path| (*path).to_string()));
    }
    // Editor packages leave a versioned `.vsix` next to the manifest.
    let editor = root.join("editors/vscode");
    if editor.is_dir() {
        for entry in fs::read_dir(&editor)? {
            let entry = entry?;
            if entry
                .path()
                .extension()
                .is_some_and(|extension| extension == "vsix")
            {
                paths.push(format!(
                    "editors/vscode/{}",
                    entry.file_name().to_string_lossy()
                ));
            }
        }
    }

    for relative in paths {
        let path = root.join(&relative);
        if !path.exists() {
            continue;
        }
        // The repository check: `canonicalize` resolves `..`, so a symlink that
        // escapes the checkout is refused instead of followed.
        let resolved = path.canonicalize()?;
        if !resolved.starts_with(&root) || resolved == root {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!("refusing to remove {}", resolved.display()),
            ));
        }
        let bytes = size_of(&resolved)?;
        let removed = if dry_run {
            false
        } else if resolved.is_dir() {
            fs::remove_dir_all(&resolved)?;
            true
        } else {
            fs::remove_file(&resolved)?;
            true
        };
        entries.push(Entry {
            path: PathBuf::from(relative),
            bytes,
            removed,
        });
    }
    Ok(Report { entries })
}

fn size_of(path: &Path) -> io::Result<u64> {
    if path.is_file() {
        return Ok(path.metadata()?.len());
    }
    let mut total = 0;
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        total += size_of(&entry.path())?;
    }
    Ok(total)
}

/// Human-readable byte count: `940 B`, `1.2 MiB`.
#[must_use]
pub fn human(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KiB", "MiB", "GiB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;

    fn sandbox(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("rvh-clean-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("sandbox");
        path
    }

    #[test]
    fn removes_build_outputs_and_reports_sizes() {
        let root = sandbox("build");
        fs::create_dir_all(root.join("target/debug")).expect("dir");
        fs::write(root.join("target/debug/a.rlib"), b"12345678").expect("write");
        fs::create_dir_all(root.join("web/dist")).expect("dir");
        fs::write(root.join("web/dist/index.html"), b"<html>").expect("write");

        let report = clean(&root, false, false).expect("clean");
        assert!(!root.join("target").exists());
        assert!(!root.join("web/dist").exists());
        assert_eq!(report.total_bytes(), 14);
        assert!(report.entries.iter().all(|entry| entry.removed));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_dry_run_reports_without_removing() {
        let root = sandbox("dry");
        fs::create_dir_all(root.join("target")).expect("dir");
        let report = clean(&root, false, true).expect("clean");
        assert!(root.join("target").exists());
        assert!(!report.entries[0].removed);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn node_modules_survive_unless_all_is_asked() {
        let root = sandbox("all");
        fs::create_dir_all(root.join("web/node_modules/x")).expect("dir");
        clean(&root, false, false).expect("clean");
        assert!(root.join("web/node_modules").exists());
        clean(&root, true, false).expect("clean");
        assert!(!root.join("web/node_modules").exists());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn human_sizes_read_well() {
        assert_eq!(human(512), "512 B");
        assert_eq!(human(1536), "1.5 KiB");
        assert_eq!(human(3 * 1024 * 1024), "3.0 MiB");
    }

    #[test]
    fn a_vsix_next_to_the_manifest_is_removed() {
        let root = sandbox("vsix");
        fs::create_dir_all(root.join("editors/vscode")).expect("dir");
        File::create(root.join("editors/vscode/thing.vsix")).expect("file");
        let report = clean(&root, false, false).expect("clean");
        assert!(report
            .entries
            .iter()
            .any(|entry| entry.path.ends_with("thing.vsix")));
        assert!(!root.join("editors/vscode/thing.vsix").exists());
        let _ = fs::remove_dir_all(&root);
    }
}
