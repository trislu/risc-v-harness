#!/usr/bin/env node
// Load the wasm artifact, drive the C ABI from JavaScript, and check the replies.
//
// This is the check the browser host will do, minus the DOM. Run it after
// `cargo build -p rvh-wasm --target wasm32-unknown-unknown --release`.
//
//   node scripts/wasm-smoke.mjs [path-to-wasm]

import { readFileSync } from 'node:fs';

const path = process.argv[2] ?? 'target/wasm32-unknown-unknown/release/rvh_wasm.wasm';
const module = readFileSync(path);
const { instance } = await WebAssembly.instantiate(module, {});
const e = instance.exports;

const encoder = new TextEncoder();
const decoder = new TextDecoder();

// Memory views must be recreated after every call: growth detaches old buffers.
const bytes = () => new Uint8Array(e.memory.buffer);

function writeString(text) {
  const data = encoder.encode(text);
  const pointer = e.rvh_alloc(data.length);
  if (!pointer) {
    throw new Error(`rvh_alloc(${data.length}) returned null`);
  }
  bytes().set(data, pointer);
  return [pointer, data.length];
}

function readResult(session, length) {
  const pointer = e.rvh_result_ptr(session);
  return decoder.decode(bytes().subarray(pointer, pointer + length));
}

function check(condition, message) {
  if (!condition) {
    console.error(`FAIL ${message}`);
    process.exit(1);
  }
}

const [targetPointer, targetLength] = writeString('rv32i_zicsr');
const session = e.rvh_new(targetPointer, targetLength);
check(session !== 0, 'rvh_new returned null');

const [sourcePointer, sourceLength] = writeString('li a0, 55\nebreak\n');
const diagnostics = readResult(session, e.rvh_set_source(session, sourcePointer, sourceLength));
check(diagnostics.includes('"ok":true'), `diagnostics: ${diagnostics}`);

const [badPointer, badLength] = writeString('adii a0, a1, 1\n');
const bad = readResult(session, e.rvh_set_source(session, badPointer, badLength));
check(bad.includes('did you mean `addi`'), `expected a suggestion: ${bad}`);
check(!readResult(session, e.rvh_state(session)).includes('"ok":true'), 'broken source must not run');

readResult(session, e.rvh_set_source(session, sourcePointer, sourceLength));

const stepped = readResult(session, e.rvh_step(session, 1));
check(stepped.includes('"new":55'), `expected a change record: ${stepped}`);

const state = readResult(session, e.rvh_run(session, 1000));
check(state.includes('"halt":"ebreak"'), `expected ebreak: ${state}`);

const registers = new Uint32Array(e.memory.buffer, e.rvh_regs_ptr(session), 32);
check(registers[10] === 55, `a0 is ${registers[10]}, expected 55`);

const hover = readResult(session, e.rvh_hover(session, 1));
check(hover.includes('"kind":"pseudo"'), `expected pseudo hover: ${hover}`);

const [localePointer, localeLength] = writeString('zh-CN');
readResult(session, e.rvh_set_locale(session, localePointer, localeLength));
const chinese = readResult(session, e.rvh_hover(session, 1));
check(chinese.includes('加载'), `expected a translated hover: ${chinese}`);

const ramLength = e.rvh_mem_len(session);
check(ramLength === 0x40000, `RAM is ${ramLength}, expected 262144`);

e.rvh_free(session);
console.log(
  `wasm-smoke: OK (${(module.length / 1024).toFixed(0)} KiB module, ${ramLength / 1024} KiB RAM)`,
);
