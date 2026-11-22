//! Package the extension for one VS Code target.
//!
//! An extension that carries a binary is built once per platform and
//! architecture: the manifest records `TargetPlatform`, and the archive must
//! carry exactly one binary. This stages that binary, writes an ignore file
//! that keeps the other targets out, and calls vsce.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

const TARGETS: [&str; 9] = [
    "win32-x64",
    "win32-arm64",
    "linux-x64",
    "linux-arm64",
    "linux-armhf",
    "darwin-x64",
    "darwin-arm64",
    "alpine-x64",
    "alpine-arm64",
];

/// `cargo xtask package-vsix [target] [--binary <path>] [--skip-build]`
pub fn run(root: &Path, args: &[&str]) -> ExitCode {
    let options = match parse(args) {
        Ok(options) => options,
        Err(error) => {
            eprintln!("package-vsix: {error}");
            return ExitCode::from(2);
        }
    };

    match package(root, &options) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("package-vsix: {error}");
            ExitCode::FAILURE
        }
    }
}

struct Options {
    target: String,
    binary: Option<PathBuf>,
    skip_build: bool,
}

fn parse(args: &[&str]) -> Result<Options, String> {
    let mut target = None;
    let mut binary = None;
    let mut skip_build = false;
    let mut args = args.iter();

    while let Some(arg) = args.next() {
        match *arg {
            "--binary" => {
                let path = args.next().ok_or("--binary needs a path")?;
                binary = Some(PathBuf::from(path));
            }
            "--skip-build" => skip_build = true,
            other if other.starts_with('-') => return Err(format!("unknown flag {other}")),
            other => target = Some(other.to_string()),
        }
    }

    let target = target.unwrap_or_else(host_target);
    if !TARGETS.contains(&target.as_str()) {
        return Err(format!(
            "{target} is not a VS Code target; pick one of {}",
            TARGETS.join(", ")
        ));
    }
    Ok(Options {
        target,
        binary,
        skip_build,
    })
}

/// The pair this machine would be packaged for.
fn host_target() -> String {
    let os = match std::env::consts::OS {
        "macos" => "darwin",
        os => os,
    };
    let arch = match std::env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        "arm" => "armhf",
        arch => arch,
    };
    format!("{os}-{arch}")
}

fn package(root: &Path, options: &Options) -> Result<(), String> {
    let extension = root.join("editors").join("vscode");
    let executable = if options.target.starts_with("win32") {
        "rvh.exe"
    } else {
        "rvh"
    };
    let bin_dir = extension.join("bin").join(&options.target);
    let staged = bin_dir.join(executable);

    if let Some(binary) = &options.binary {
        if !binary.exists() {
            return Err(format!("no binary at {}", binary.display()));
        }
        fs::create_dir_all(&bin_dir)
            .map_err(|error| format!("create {}: {error}", bin_dir.display()))?;
        fs::copy(binary, &staged).map_err(|error| format!("copy {}: {error}", binary.display()))?;
    }
    if !staged.exists() {
        return Err(format!(
            "missing {}; build it with `cargo build -p rvh-cli --release`, then pass --binary <path>",
            staged.display()
        ));
    }
    // vsce keeps the file mode, so the bit has to be set before zipping.
    make_executable(&staged).map_err(|error| format!("chmod {}: {error}", staged.display()))?;

    if !options.skip_build {
        // Windows resolves `pnpm` to `pnpm.cmd` through PATHEXT; `CreateProcess`
        // does not, so the name has to carry the extension there.
        let pnpm = if cfg!(windows) { "pnpm.cmd" } else { "pnpm" };
        run_in(&extension, pnpm, &["run", "build"])?;
    }
    if !extension.join("out").join("extension.js").exists() {
        return Err(
            "out/extension.js is missing; run `pnpm --dir editors/vscode run build`".to_string(),
        );
    }

    // `.vscodeignore` excludes `bin/**` so a plain `vsce package` never ships a
    // stray binary. Re-include the one target this build is for.
    let out_dir = root.join("target").join("vsix");
    fs::create_dir_all(&out_dir)
        .map_err(|error| format!("create {}: {error}", out_dir.display()))?;
    let ignore_file = out_dir.join(format!("{}.ignore", options.target));
    let mut ignore = fs::read_to_string(extension.join(".vscodeignore"))
        .map_err(|error| format!("read .vscodeignore: {error}"))?;
    ignore.push_str(&format!("\n!bin/{}/**\n", options.target));
    fs::write(&ignore_file, ignore)
        .map_err(|error| format!("write {}: {error}", ignore_file.display()))?;

    let vsce = extension
        .join("node_modules")
        .join(".bin")
        .join(if cfg!(windows) { "vsce.cmd" } else { "vsce" });
    run_in(
        &extension,
        &vsce.to_string_lossy(),
        &[
            "package",
            "--no-dependencies",
            "--target",
            &options.target,
            "--ignoreFile",
            &ignore_file.to_string_lossy(),
            "--out",
            &out_dir.to_string_lossy(),
        ],
    )
}

/// Run a program in `dir`, failing on a non-zero status. Windows needs `cmd /C`
/// for the `.cmd` shims that npm and pnpm install.
fn run_in(dir: &Path, program: &str, args: &[&str]) -> Result<(), String> {
    let mut command = if cfg!(windows) && program.ends_with(".cmd") {
        let mut command = Command::new("cmd");
        command.arg("/C").arg(program);
        command
    } else {
        Command::new(program)
    };
    let status = command
        .args(args)
        .current_dir(dir)
        .status()
        .map_err(|error| format!("run {program}: {error}"))?;
    if !status.success() {
        return Err(format!("{program} exited with {status}"));
    }
    Ok(())
}

#[cfg(unix)]
fn make_executable(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o755))
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) -> std::io::Result<()> {
    Ok(())
}
