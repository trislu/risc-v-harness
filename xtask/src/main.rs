//! Repository chores: ISA codegen, packaging and artifact cleanup.

mod clean;
mod content;
mod emit;
mod lsp_smoke;
mod package;
mod schema;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const USAGE: &str = "usage: cargo xtask <command>

commands:
  codegen [--check]   regenerate crates/rvh-isa/src/generated.rs
  clean [--all] [--dry-run]
                      remove build outputs; --all also node_modules and scratch
  lsp-smoke           drive `rvh lsp` over stdio and check the replies
  package-vsix [target] [--binary <path>] [--skip-build]
                      package a per-platform VSIX under target/vsix
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["codegen"] => codegen(false),
        ["codegen", "--check"] => codegen(true),
        ["clean"] => clean_artifacts(false, false),
        ["clean", "--dry-run"] => clean_artifacts(false, true),
        ["clean", "--all"] => clean_artifacts(true, false),
        ["clean", "--all", "--dry-run"] | ["clean", "--dry-run", "--all"] => {
            clean_artifacts(true, true)
        }
        ["lsp-smoke"] => lsp_smoke::run(&repo_root()),
        ["package-vsix", rest @ ..] => package::run(&repo_root(), rest),
        [] | ["help"] | ["--help"] => {
            print!("{USAGE}");
            ExitCode::SUCCESS
        }
        other => {
            eprintln!("unknown arguments: {other:?}\n\n{USAGE}");
            ExitCode::from(2)
        }
    }
}

fn clean_artifacts(all: bool, dry_run: bool) -> ExitCode {
    let root = repo_root();
    let report = match clean::clean(&root, all, dry_run) {
        Ok(report) => report,
        Err(error) => {
            eprintln!("clean: {error}");
            return ExitCode::FAILURE;
        }
    };
    if report.entries.is_empty() {
        println!("clean: nothing to remove");
        return ExitCode::SUCCESS;
    }
    for entry in &report.entries {
        let verb = if entry.removed {
            "removed"
        } else {
            "would remove"
        };
        println!(
            "clean: {verb} {} ({})",
            entry.path.display(),
            clean::human(entry.bytes)
        );
    }
    let verb = if dry_run { "would free" } else { "freed" };
    println!(
        "clean: {verb} {} from {} path(s)",
        clean::human(report.total_bytes()),
        report.entries.len()
    );
    ExitCode::SUCCESS
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask lives under the repository root")
        .to_path_buf()
}

fn codegen(check: bool) -> ExitCode {
    let root = repo_root();
    let insns = match load(&root.join("isa")) {
        Ok(insns) => insns,
        Err(message) => {
            eprintln!("codegen: {message}");
            return ExitCode::FAILURE;
        }
    };
    let files = match content::read_dir(&root.join("content/overlay")) {
        Ok(files) => files,
        Err(error) => {
            eprintln!("codegen: {error}");
            return ExitCode::FAILURE;
        }
    };

    let outputs = [
        (
            root.join("crates/rvh-isa/src/generated.rs"),
            emit::render(&insns),
        ),
        (
            root.join("crates/rvh-content/src/generated.rs"),
            content::render(&files),
        ),
    ];

    if check {
        let mut stale = false;
        for (path, rendered) in &outputs {
            let current = std::fs::read_to_string(path).unwrap_or_default();
            if current != *rendered {
                eprintln!(
                    "codegen: {} is stale; run `cargo xtask codegen`",
                    path.display()
                );
                stale = true;
            }
        }
        if stale {
            return ExitCode::FAILURE;
        }
        println!(
            "codegen: {} instructions and {} content files, both up to date",
            insns.len(),
            files.len()
        );
        return ExitCode::SUCCESS;
    }

    for (path, rendered) in &outputs {
        if let Err(error) = std::fs::write(path, rendered) {
            eprintln!("codegen: cannot write {}: {error}", path.display());
            return ExitCode::FAILURE;
        }
    }
    println!(
        "codegen: {} instructions and {} content files written",
        insns.len(),
        files.len()
    );
    ExitCode::SUCCESS
}

fn load(isa_dir: &Path) -> Result<Vec<schema::Insn>, String> {
    let entries = std::fs::read_dir(isa_dir)
        .map_err(|error| format!("cannot read {}: {error}", isa_dir.display()))?;
    let mut files: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "toml"))
        .collect();
    files.sort();

    if files.is_empty() {
        return Err(format!("no *.toml files in {}", isa_dir.display()));
    }

    let mut all = Vec::new();
    for file in files {
        let mut parsed = schema::parse_file(&file).map_err(|error| error.to_string())?;
        all.append(&mut parsed);
    }
    validate(&all)?;
    Ok(all)
}

fn validate(insns: &[schema::Insn]) -> Result<(), String> {
    let mut by_id: BTreeMap<&str, usize> = BTreeMap::new();
    let mut by_mnemonic: BTreeMap<(&str, &str), usize> = BTreeMap::new();
    for insn in insns {
        if let Some(previous) = by_id.insert(&insn.id, insn.line) {
            return Err(format!(
                "duplicate id `{}` at lines {previous} and {}",
                insn.id, insn.line
            ));
        }
        if let Some(previous) = by_mnemonic.insert((&insn.ext, &insn.mnemonic), insn.line) {
            return Err(format!(
                "duplicate mnemonic `{}` in extension `{}` at lines {previous} and {}",
                insn.mnemonic, insn.ext, insn.line
            ));
        }
    }
    Ok(())
}
