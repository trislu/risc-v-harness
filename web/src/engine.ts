/// <reference types="vite/client" />
// The wasm ABI, wrapped in a small class.
//
// One rule matters here: memory views are invalidated by wasm memory growth, so
// every call that returns a view creates a fresh one.

export interface Fix {
  title: string;
  start: number;
  end: number;
  replacement: string;
}

export interface Diagnostic {
  code: string;
  message: string;
  line: number;
  column: number;
  start: number;
  end: number;
  fixes: Fix[];
}

export interface DiagnosticsResult {
  ok: boolean;
  diagnostics: Diagnostic[];
}

export interface Change {
  reg?: number;
  addr?: number;
  old: number;
  new: number;
}

export interface MachineState {
  ok: boolean;
  reason?: string;
  pc: number;
  steps: number;
  halt: string | null;
  output: string;
  text_bytes: number;
  /** 1-based source line of the program counter, 0 when unknown. */
  line: number;
  column: number;
  regs: number[];
  changed: Change[];
}

export interface Hover {
  kind: string;
  title: string;
  text: string;
  start: number;
  end: number;
}

interface Exports {
  memory: WebAssembly.Memory;
  rvh_alloc(length: number): number;
  rvh_dealloc(pointer: number, length: number): void;
  rvh_new(pointer: number, length: number): number;
  rvh_free(session: number): void;
  rvh_set_source(session: number, pointer: number, length: number): number;
  rvh_step(session: number, count: number): number;
  rvh_undo(session: number, count: number): number;
  rvh_run(session: number, count: number): number;
  rvh_reset(session: number): number;
  rvh_state(session: number): number;
  rvh_hover(session: number, offset: number): number;
  rvh_set_locale(session: number, pointer: number, length: number): number;
  rvh_result_ptr(session: number): number;
  rvh_regs_ptr(session: number): number;
  rvh_mem_ptr(session: number): number;
  rvh_mem_len(session: number): number;
}

const encoder = new TextEncoder();
const decoder = new TextDecoder();

const EMPTY_STATE: MachineState = {
  ok: false,
  pc: 0,
  steps: 0,
  halt: null,
  output: '',
  text_bytes: 0,
  line: 0,
  column: 0,
  regs: new Array<number>(32).fill(0),
  changed: [],
};

export class Engine {
  private readonly exports: Exports;
  private readonly session: number;

  private constructor(exports: Exports, session: number) {
    this.exports = exports;
    this.session = session;
  }

  static async load(
    target = 'rv32i_zicsr',
    url = `${import.meta.env.BASE_URL}rvh_wasm.wasm`,
  ): Promise<Engine> {
    const response = await fetch(url);
    if (!response.ok) {
      throw new Error(`cannot fetch ${url}: ${response.status}`);
    }
    const bytes = await response.arrayBuffer();
    const { instance } = await WebAssembly.instantiate(bytes, {});
    const exports = instance.exports as unknown as Exports;

    const [pointer, length] = writeString(exports, target);
    const session = exports.rvh_new(pointer, length);
    exports.rvh_dealloc(pointer, length);
    if (!session) {
      throw new Error(`rvh_new rejected target ${target}`);
    }
    return new Engine(exports, session);
  }

  setSource(source: string): DiagnosticsResult {
    const [pointer, length] = writeString(this.exports, source);
    const result = this.takeResult(this.exports.rvh_set_source(this.session, pointer, length));
    this.exports.rvh_dealloc(pointer, length);
    return JSON.parse(result) as DiagnosticsResult;
  }

  step(count = 1): MachineState {
    return JSON.parse(this.takeResult(this.exports.rvh_step(this.session, count))) as MachineState;
  }

  undo(count = 1): MachineState {
    return JSON.parse(this.takeResult(this.exports.rvh_undo(this.session, count))) as MachineState;
  }

  run(maxSteps = 100_000): MachineState {
    return JSON.parse(this.takeResult(this.exports.rvh_run(this.session, maxSteps))) as MachineState;
  }

  reset(): MachineState {
    return JSON.parse(this.takeResult(this.exports.rvh_reset(this.session))) as MachineState;
  }

  state(): MachineState {
    const parsed = JSON.parse(this.takeResult(this.exports.rvh_state(this.session))) as MachineState;
    return parsed.ok ? parsed : { ...EMPTY_STATE, reason: parsed.reason };
  }

  /** Content locale for hover text, such as `zh-CN`. */
  setLocale(locale: string): void {
    const [pointer, length] = writeString(this.exports, locale);
    this.takeResult(this.exports.rvh_set_locale(this.session, pointer, length));
    this.exports.rvh_dealloc(pointer, length);
  }

  hover(offset: number): Hover | null {
    const parsed = JSON.parse(this.takeResult(this.exports.rvh_hover(this.session, offset))) as {
      hover: Hover | null;
    };
    return parsed.hover;
  }

  /** Live register file. Copy it with `Array.from` before putting it in `$state`. */
  regs(): Uint32Array {
    const pointer = this.exports.rvh_regs_ptr(this.session);
    if (!pointer) {
      return new Uint32Array(0);
    }
    return new Uint32Array(this.exports.memory.buffer, pointer, 32);
  }

  /** Live RAM view. Do not keep it across calls. */
  mem(): Uint8Array {
    const pointer = this.exports.rvh_mem_ptr(this.session);
    const length = this.exports.rvh_mem_len(this.session);
    if (!pointer || !length) {
      return new Uint8Array(0);
    }
    return new Uint8Array(this.exports.memory.buffer, pointer, length);
  }

  readWords(address: number, count: number): number[] {
    const memory = this.mem();
    const words: number[] = [];
    for (let index = 0; index < count; index += 1) {
      const offset = address + index * 4;
      if (offset + 4 > memory.length) {
        break;
      }
      const word =
        memory[offset] |
        (memory[offset + 1] << 8) |
        (memory[offset + 2] << 16) |
        (memory[offset + 3] << 24);
      words.push(word >>> 0);
    }
    return words;
  }

  dispose(): void {
    this.exports.rvh_free(this.session);
  }

  private takeResult(length: number): string {
    const pointer = this.exports.rvh_result_ptr(this.session);
    const view = new Uint8Array(this.exports.memory.buffer, pointer, length);
    return decoder.decode(view);
  }
}

function writeString(exports: Exports, text: string): [number, number] {
  const data = encoder.encode(text);
  // Allocate one byte even for an empty document: a null pointer is not a
  // string, and the engine would (rightly) reject it.
  const pointer = exports.rvh_alloc(Math.max(data.length, 1));
  if (!pointer) {
    throw new Error(`rvh_alloc(${data.length}) failed`);
  }
  new Uint8Array(exports.memory.buffer).set(data, pointer);
  return [pointer, data.length];
}
