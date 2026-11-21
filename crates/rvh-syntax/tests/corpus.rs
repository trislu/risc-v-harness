//! Corpus tests: real assembly files must parse cleanly, losslessly, and match
//! their committed snapshot.
//!
//! Snapshots are plain text so a reviewer can see exactly what the tree claims.
//! Refresh them with `RVH_BLESS=1 cargo test -p rvh-syntax`.

use std::fs;
use std::path::{Path, PathBuf};

use rvh_syntax::{Element, Node};

fn corpus_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/corpus")
}

fn corpus_files() -> Vec<PathBuf> {
    let dir = corpus_dir();
    let mut files: Vec<PathBuf> = fs::read_dir(&dir)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", dir.display()))
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "s"))
        .collect();
    files.sort();
    assert!(!files.is_empty(), "no corpus files in {}", dir.display());
    files
}

/// Walk every leaf and check that spans tile the source exactly.
fn check_lossless(node: &Node, offset: &mut usize) {
    for child in &node.children {
        match child {
            Element::Node(node) => check_lossless(node, offset),
            Element::Token(token) => {
                assert_eq!(
                    token.span.0, *offset,
                    "gap or overlap before {:?}",
                    token.kind
                );
                assert!(token.span.0 < token.span.1, "empty token {:?}", token.kind);
                *offset = token.span.1;
            }
        }
    }
}

#[test]
fn corpus_parses_losslessly() {
    for file in corpus_files() {
        let source = fs::read_to_string(&file).expect("read corpus file");
        let tree = rvh_syntax::parse(&source);
        let mut offset = 0;
        check_lossless(tree.root(), &mut offset);
        assert_eq!(offset, source.len(), "{} is not covered", file.display());
        assert_eq!(tree.source(), source);
    }
}

#[test]
fn valid_corpus_has_no_diagnostics() {
    for file in corpus_files() {
        let name = file
            .file_name()
            .and_then(|n| n.to_str())
            .expect("file name");
        if name.starts_with("9") {
            continue;
        }
        let source = fs::read_to_string(&file).expect("read corpus file");
        let tree = rvh_syntax::parse(&source);
        assert!(
            tree.diagnostics().is_empty(),
            "{name} produced diagnostics: {:#?}",
            tree.diagnostics()
        );
        assert!(!tree.has_errors(), "{name} produced error tokens");
    }
}

#[test]
fn error_corpus_produces_diagnostics() {
    for file in corpus_files() {
        let name = file
            .file_name()
            .and_then(|n| n.to_str())
            .expect("file name");
        if !name.starts_with("9") {
            continue;
        }
        let source = fs::read_to_string(&file).expect("read corpus file");
        let tree = rvh_syntax::parse(&source);
        assert!(
            !tree.diagnostics().is_empty(),
            "{name} produced no diagnostics"
        );
    }
}

#[test]
fn corpus_matches_snapshots() {
    let dir = corpus_dir();
    let expected_dir = dir.join("expected");
    fs::create_dir_all(&expected_dir).expect("create snapshot directory");
    let bless = std::env::var_os("RVH_BLESS").is_some();

    let mut stale = Vec::new();
    for file in corpus_files() {
        let source = fs::read_to_string(&file).expect("read corpus file");
        let tree = rvh_syntax::parse(&source);
        let name = file
            .file_name()
            .and_then(|n| n.to_str())
            .expect("file name");
        let stem = name.strip_suffix(".s").unwrap_or(name);
        let rendered = tree.render();
        let snapshot = expected_dir.join(format!("{stem}.snap"));

        if bless {
            fs::write(&snapshot, &rendered).expect("write snapshot");
            continue;
        }
        let expected = fs::read_to_string(&snapshot).unwrap_or_default();
        if expected != rendered {
            stale.push(name.to_string());
        }
    }

    assert!(
        stale.is_empty(),
        "snapshots differ: {stale:?}\nrun `RVH_BLESS=1 cargo test -p rvh-syntax` to refresh"
    );
}
