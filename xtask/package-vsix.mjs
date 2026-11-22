#!/usr/bin/env node
// Package the extension for one VS Code target.
//
// An extension that carries a binary has to be built once per platform and
// architecture: the manifest records `TargetPlatform`, and the archive must
// carry only that pair's binary. This script stages the binary, writes an
// ignore file that keeps every other pair out, and calls vsce.
//
// Usage:
//   node xtask/package-vsix.mjs [target] [--binary <path>] [--skip-build]
//
// `target` defaults to the host, as `<platform>-<arch>` (`linux-x64`,
// `darwin-arm64`, `win32-x64`, ...). `--binary` copies a freshly built binary
// into `editors/vscode/bin/<target>/` first, which is what CI does.

import { execFileSync } from 'node:child_process';
import { chmodSync, copyFileSync, existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const EXTENSION = join(ROOT, 'editors', 'vscode');

const TARGETS = new Set([
  'win32-x64',
  'win32-arm64',
  'linux-x64',
  'linux-arm64',
  'linux-armhf',
  'darwin-x64',
  'darwin-arm64',
  'alpine-x64',
  'alpine-arm64',
]);

function fail(message) {
  console.error(`package-vsix: ${message}`);
  process.exit(1);
}

function parseArgs(argv) {
  let binary;
  let skipBuild = false;
  const positional = [];
  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    if (arg === '--binary') {
      binary = argv[(index += 1)];
      if (!binary) {
        fail('--binary needs a path');
      }
    } else if (arg === '--skip-build') {
      skipBuild = true;
    } else {
      positional.push(arg);
    }
  }
  return { target: positional[0], binary, skipBuild };
}

const { target: requested, binary, skipBuild } = parseArgs(process.argv.slice(2));
const target = requested ?? `${process.platform}-${process.arch}`;
if (!TARGETS.has(target)) {
  fail(`${target} is not a VS Code target; pick one of ${[...TARGETS].join(', ')}`);
}

const executable = target.startsWith('win32') ? 'rvh.exe' : 'rvh';
const binDir = join(EXTENSION, 'bin', target);

if (binary) {
  if (!existsSync(binary)) {
    fail(`no binary at ${binary}`);
  }
  mkdirSync(binDir, { recursive: true });
  copyFileSync(binary, join(binDir, executable));
}
if (!existsSync(join(binDir, executable))) {
  fail(
    `missing editors/vscode/bin/${target}/${executable}. Build it with ` +
      '`cargo build -p rvh-cli --release`, then pass --binary <path>.',
  );
}
if (!target.startsWith('win32')) {
  // vsce keeps the file mode, so the bit has to be set before zipping.
  chmodSync(join(binDir, executable), 0o755);
}

if (!skipBuild) {
  // `pnpm` is a `.cmd` shim on Windows, and Node refuses to spawn one without a
  // shell.
  execFileSync(process.platform === 'win32' ? 'pnpm.cmd' : 'pnpm', ['run', 'build'], {
    cwd: EXTENSION,
    stdio: 'inherit',
    shell: process.platform === 'win32',
  });
}
if (!existsSync(join(EXTENSION, 'out', 'extension.js'))) {
  fail('out/extension.js is missing; run `pnpm --dir editors/vscode run build`');
}

// The repository excludes `bin/**` so a plain `vsce package` never ships a
// stray binary. Re-include the one target this build is for.
const outDir = join(ROOT, 'target', 'vsix');
mkdirSync(outDir, { recursive: true });
const ignoreFile = join(outDir, `${target}.ignore`);
const base = readFileSync(join(EXTENSION, '.vscodeignore'), 'utf8').trimEnd();
writeFileSync(ignoreFile, `${base}\n!bin/${target}/**\n`);

const vsce = join(
  EXTENSION,
  'node_modules',
  '.bin',
  process.platform === 'win32' ? 'vsce.cmd' : 'vsce',
);
execFileSync(
  vsce,
  [
    'package',
    '--no-dependencies',
    '--target',
    target,
    '--ignoreFile',
    ignoreFile,
    '--out',
    outDir,
  ],
  { cwd: EXTENSION, stdio: 'inherit', shell: process.platform === 'win32' },
);
