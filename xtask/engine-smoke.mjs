#!/usr/bin/env node
// Exercise the TypeScript `Engine` wrapper against the real wasm artifact.
//
// The wrapper is the layer the UI talks to, so it deserves its own check: the
// raw ABI is covered by wasm-smoke.mjs, this covers the wrapper's parsing and
// memory handling. Run via `pnpm --dir web run smoke`.

import { createServer } from 'node:http';
import { readFile } from 'node:fs/promises';
import { extname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('..', import.meta.url));
const dist = join(root, 'web/dist');
const compiled = join(root, 'target/engine-js/engine.js');

const types = {
  '.html': 'text/html',
  '.js': 'text/javascript',
  '.css': 'text/css',
  '.wasm': 'application/wasm',
};

const server = createServer(async (request, response) => {
  const path = request.url === '/' ? '/index.html' : request.url;
  try {
    const body = await readFile(join(dist, path));
    response.writeHead(200, { 'content-type': types[extname(path)] ?? 'application/octet-stream' });
    response.end(body);
  } catch {
    response.writeHead(404).end();
  }
});

await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
const { port } = server.address();

const { Engine } = await import(compiled);
const engine = await Engine.load('rv32i_zicsr', `http://127.0.0.1:${port}/rvh_wasm.wasm`);

const clean = engine.setSource('li a0, 55\nebreak\n');
if (!clean.ok) throw new Error(`diagnostics: ${JSON.stringify(clean)}`);

const initial = engine.state();
if (initial.line !== 1) throw new Error(`initial line: ${initial.line}`);

const stepped = engine.step(1);
if (stepped.line !== 2) throw new Error(`line after one step: ${stepped.line}`);
if (stepped.changed[0]?.new !== 55) throw new Error(`change: ${JSON.stringify(stepped.changed)}`);

const state = engine.run();
if (state.halt !== 'ebreak') throw new Error(`halt: ${state.halt}`);
if (engine.regs()[10] !== 55) throw new Error(`a0: ${engine.regs()[10]}`);

const hover = engine.hover(1);
if (hover?.kind !== 'pseudo') throw new Error(`hover: ${JSON.stringify(hover)}`);

// `li a0, 55` becomes addi a0, x0, 55; then ebreak.
const memory = engine.readWords(0, 2);
if (memory[0] !== 0x0370_0513) throw new Error(`memory[0]: 0x${memory[0]?.toString(16)}`);
if (memory[1] !== 0x0010_0073) throw new Error(`memory[1]: 0x${memory[1]?.toString(16)}`);

const broken = engine.setSource('adii a0, a1, 1\n');
if (broken.ok) throw new Error('a broken program must not be ok');
if (broken.diagnostics[0]?.fixes.length !== 1) throw new Error('expected one fix');

engine.dispose();
server.close();
console.log('engine-smoke: OK', {
  halt: state.halt,
  a0: 55,
  hover: hover.kind,
  fix: broken.diagnostics[0].fixes[0].title,
});
