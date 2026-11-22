# Working on it

Prerequisites: a Rust toolchain (1.85 or newer) with the
`wasm32-unknown-unknown` target, `node` and `pnpm` for the web side, and GNU
binutils built for RISC-V for the differential checks (`riscv64-linux-gnu-as`,
`ld`, `objcopy`, `readelf`).

## Build and test

```bash
cargo test --all
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
```

## Codegen

`isa/*.toml` and `content/overlay/` are compiled into Rust source:

```bash
cargo xtask codegen           # write crates/rvh-isa/src/generated.rs and rvh-content
cargo xtask codegen --check   # fail when the generated file is stale
```

The output is committed, so an ISA change is reviewable as a normal diff; CI fails
on drift.

## Cleaning artifacts

```bash
cargo xtask clean                  # target/, web/dist, the built wasm, out/, *.vsix
cargo xtask clean --all            # also node_modules and scratch/
cargo xtask clean --dry-run        # print what would go, remove nothing
```

It refuses to remove anything that resolves outside the checkout, and reports the
total size. `--all` needs a `pnpm install` afterwards.

### Trace comparison

`xtask/verify-trace.sh` runs the same ELF under `rvh` and under
[spike](https://github.com/riscv-software-src/riscv-isa-sim) and compares the
result instruction by instruction: address, word, and every write. It needs a
spike binary; point `RVH_SPIKE` at one, or have `spike` in `PATH`:

```bash
RVH_SPIKE=~/.local/spike/bin/spike ./xtask/verify-trace.sh
```

Six fixtures cover integer arithmetic, multiply and divide, compressed, atomics,
single and double precision. The fixtures link at `0x80000000` (the `spike` board
preset), end with a `tohost`
write that stops spike, and avoid the console. Two things the comparison taught
us and the fixtures now carry:

- spike traps a floating-point instruction while `mstatus.FS` is Off; `rvh` models
  `FS` too, so the fixtures enable it with `csrw mstatus` and both agree.
- spike logs a memory read as `mem <addr>` with no value, and needs both
  `tohost` and `fromhost` symbols before it will stop.

Spike is not a build dependency and is not installed in CI, so this check is
local and opt-in.

## Snapshots

`tests/corpus/*.snap` pins the CST shape of every corpus file:

```bash
RVH_BLESS=1 cargo test -p rvh-syntax
```

## Differential checks

The assembler is compared with the real toolchain rather than with itself:

```bash
./xtask/verify-asm.sh    # checks encodings; needs binutils for RISC-V
./xtask/verify-trace.sh  # compares step traces with spike; needs spike
./xtask/verify-elf.sh    # ELF fixtures from GNU as/ld, plus a round trip
./xtask/lsp-smoke.sh     # drive `rvh lsp` over stdio
node xtask/wasm-smoke.mjs
```

`verify-asm.sh` assembles every corpus file with both toolchains and compares the
`.text` bytes, so an encoding mistake cannot pass.

## Web

```bash
pnpm install
pnpm wasm                        # build the wasm into web/public/
pnpm dev                         # playground at http://localhost:5173
pnpm --dir web run build         # the smoke test serves web/dist
pnpm --dir web run smoke         # the TS wrapper against the built wasm
pnpm --dir web run test:e2e      # Playwright: click through the page
pnpm --dir web run check         # svelte-check
```

`cargo xtask ...` is an alias for `cargo run -p xtask -- ...`; see
`.cargo/config.toml`.

## Layout

```text
crates/rvh-isa      instruction metadata generated from isa/*.toml
crates/rvh-syntax   lexer, parser and lossless CST
crates/rvh-sem      resolution: operands, pseudos, labels, constants
crates/rvh-asm      layout, encoding, Image
crates/rvh-sim      machine state, step, undo, trace
crates/rvh-session  document facade: parse, assemble, run, JSON snapshots
crates/rvh-wasm     C ABI over the session (wasm and native)
crates/rvh-cli      the `rvh` binary
xtask               codegen, repo chores, differential checks
isa/                hand-written instruction tables
content/overlay/    one-line descriptions per locale
tests/corpus/       assembly files, error cases, and committed snapshots
```

`docs/architecture.md` explains how the layers fit together.

## The online demo

`web/` is deployed to GitHub Pages by `.github/workflows/pages.yml` on every push
to `master` that passed the checks. The project page is served from
`/<repository>/`, so the workflow builds with `RVH_BASE=/<repository>/`; the wasm
URL follows the same base, which is why `Engine.load` takes the target first and
the URL last.

Enable it once under **Settings → Pages → Build and deployment → Source:
GitHub Actions**. After that the demo lives at
`https://<owner>.github.io/<repository>/`.

## Assets

Screenshots and GIFs live in `assets/`. Frames are assembled with
`python3 xtask/assemble-gif.py <frames-dir> <out.gif>`. The VS Code story is
captured by `xtask/record-vscode.mjs` over the DevTools protocol; the playground
story by `RVH_RECORD=1 pnpm --dir web run test:e2e`. The storyboards, the profile
settings and the reasons behind them are in
[docs/notes/demo-recording.md](notes/demo-recording.md).

## Commits

Messages follow Conventional Commits. `CHANGELOG.md` keeps the user-facing
summary; the commit body says what changed and why. A published commit is never
rewritten: a fix goes on top, so the history a reader can fetch stays the history
they had.
