<script lang="ts">
  import { onDestroy, onMount } from 'svelte';
  import { Engine, type Diagnostic, type Fix, type MachineState } from './engine';
  import { createEditor, type Editor } from './editor';

  const SAMPLE = `# Write "Hi" and a newline to the console at 0x10000000, then halt.
    li t0, 0x10000000
    li t1, 72
    sb t1, 0(t0)
    li t1, 105
    sb t1, 0(t0)
    li t1, 10
    sb t1, 0(t0)
    ebreak
`;

  const EMPTY_REGS = new Array<number>(32).fill(0);

  const NAMES = [
    'zero', 'ra', 'sp', 'gp', 'tp', 't0', 't1', 't2', 's0', 's1', 'a0', 'a1', 'a2', 'a3', 'a4',
    'a5', 'a6', 'a7', 's2', 's3', 's4', 's5', 's6', 's7', 's8', 's9', 's10', 's11', 't3', 't4',
    't5', 't6',
  ];

  let engine = $state(null as Engine | null);
  let target = $state('rv32i_zicsr');
  let editor: Editor | null = null;
  let editorHost: HTMLDivElement;
  let machine = $state(null as MachineState | null);
  let diagnostics = $state([] as Diagnostic[]);
  let status = $state('loading engine…');
  let addressText = $state('0x100');
  let address = $state(0x100);

  const hex = (value: number) => `0x${(value >>> 0).toString(16).padStart(8, '0')}`;
  const asSigned = (value: number) => value | 0;

  let changedRegisters = $derived(
    new Set((machine?.changed ?? []).filter((c) => c.reg !== undefined).map((c) => c.reg as number)),
  );
  let changedAddresses = $derived(
    new Set((machine?.changed ?? []).filter((c) => c.addr !== undefined).map((c) => c.addr as number)),
  );
  let memoryWords = $derived.by(() => {
    if (!engine || !machine || !machine.ok) {
      return [];
    }
    return engine.readWords(address, 8);
  });
  let halted = $derived(machine?.halt ? `halted: ${machine.halt}` : 'running');

  onMount(async () => {
    try {
      engine = await Engine.load(target);
      editor = createEditor(editorHost, SAMPLE, () => analyze());
      analyze();
    } catch (error) {
      status = `engine failed: ${error}`;
    }
  });

  onDestroy(() => engine?.dispose());

  // `focus` is false while typing: moving the cursor on every keystroke would
  // fight the person editing.
  function apply(state: MachineState, focus = true) {
    machine = state;
    if (!state.ok || state.line < 1) {
      return;
    }
    if (focus) {
      editor?.focusLine(state.line);
    } else {
      editor?.markLine(state.line);
    }
  }

  /// Switch the ISA: a new session, then re-assemble the same text.
  async function pickTarget(next: string) {
    if (!engine || next === target) {
      return;
    }
    const source = editor?.text() ?? SAMPLE;
    const previous = engine;
    try {
      engine = await Engine.load(next);
      target = next;
      previous.dispose();
      editor?.setText(source);
      analyze();
    } catch (error) {
      status = `target rejected: ${error}`;
    }
  }

  function analyze() {
    if (!engine) {
      return;
    }
    const result = engine.setSource(editor?.text() ?? SAMPLE);
    diagnostics = result.diagnostics;
    if (result.ok) {
      apply(engine.state(), false);
    } else {
      machine = null;
    }
    status = result.ok ? 'assembled' : `${result.diagnostics.length} diagnostic(s)`;
  }

  function step(count: number) {
    if (engine && machine) {
      apply(engine.step(count));
    }
  }

  function run() {
    if (engine && machine) {
      apply(engine.run());
    }
  }

  function undo() {
    if (engine && machine) {
      apply(engine.undo(1));
    }
  }

  function reset() {
    if (engine && machine) {
      apply(engine.reset());
    }
  }

  function applyFix(fix: Fix) {
    if (!editor) {
      return;
    }
    editor.view.dispatch({ changes: { from: fix.start, to: fix.end, insert: fix.replacement } });
  }

  function commitAddress() {
    const parsed = Number.parseInt(addressText.replace(/^0x/i, ''), 16);
    if (!Number.isNaN(parsed)) {
      address = parsed;
      addressText = hex(parsed);
    }
  }
</script>

<header>
  <h1>RISC-V harness</h1>
  <div class="toolbar">
    <select class="target" title="Target ISA" onchange={(event) => pickTarget(event.currentTarget.value)}>
      <option value="rv32i" selected={target === 'rv32i'}>rv32i</option>
      <option value="rv32i_zicsr" selected={target === 'rv32i_zicsr'}>rv32i_zicsr</option>
      <option value="rv32im" selected={target === 'rv32im'}>rv32im</option>
      <option value="rv32ia" selected={target === 'rv32ia'}>rv32ia</option>
      <option value="rv32e" selected={target === 'rv32e'}>rv32e</option>
    </select>
    <button onclick={analyze}>Assemble</button>
    <button onclick={() => step(1)} disabled={!machine}>Step</button>
    <button onclick={() => step(10)} disabled={!machine}>Step ×10</button>
    <button onclick={run} disabled={!machine}>Run</button>
    <button onclick={undo} disabled={!machine}>Back</button>
    <button onclick={reset} disabled={!machine}>Reset</button>
    <span class="status">{status}</span>
    <span class="halt">{halted}</span>
    <span class="steps">{machine ? `${machine.steps} step(s) at ${hex(machine.pc)}` : ''}</span>
  </div>
</header>

<main>
  <section class="editor">
    <div class="editor-host" bind:this={editorHost}></div>
  </section>

  <section class="panels">
    {#if diagnostics.length}
      <div class="diagnostics">
        <h2>Diagnostics</h2>
        <ul>
          {#each diagnostics as diagnostic}
            <li>
              <span class="where">{diagnostic.line}:{diagnostic.column}</span>
              <span class="code">{diagnostic.code}</span>
              <span class="message">{diagnostic.message}</span>
              {#each diagnostic.fixes as fix}
                <button class="fix" onclick={() => applyFix(fix)}>{fix.title}</button>
              {/each}
            </li>
          {/each}
        </ul>
      </div>
    {/if}

    <div class="registers">
      <h2>Registers</h2>
      <div class="register-columns">
        {#each [0, 16] as start}
          <table>
            <tbody>
              {#each (machine?.regs ?? EMPTY_REGS).slice(start, start + 16) as value, offset}
                {@const index = start + offset}
                <tr class:changed={changedRegisters.has(index)}>
                  <td class="number">x{index}</td>
                  <td class="name">{NAMES[index]}</td>
                  <td class="value">{hex(value)}</td>
                  <td class="signed">{asSigned(value)}</td>
                </tr>
              {/each}
            </tbody>
          </table>
        {/each}
      </div>
    </div>

    <div class="memory">
      <h2>Memory</h2>
      <input
        type="text"
        value={addressText}
        oninput={(event) => (addressText = event.currentTarget.value)}
        onchange={commitAddress}
        aria-label="memory address"
      />
      <table>
        <tbody>
          {#each memoryWords as word, index}
            <tr class:changed={changedAddresses.has(address + index * 4)}>
              <td class="number">{hex(address + index * 4)}</td>
              <td class="value">{hex(word)}</td>
              <td class="signed">{asSigned(word)}</td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>

    <div class="console">
      <h2>Output</h2>
      <pre class:empty={!machine?.output}>{machine?.output || '(no output yet)'}</pre>
    </div>
  </section>
</main>

<style>
  :global(body) {
    margin: 0;
    background: #14171c;
    color: #e6e8eb;
    font-family: system-ui, -apple-system, 'Segoe UI', sans-serif;
  }

  header {
    display: flex;
    align-items: center;
    gap: 1.5rem;
    padding: 0.6rem 1rem;
    border-bottom: 1px solid #2a2f36;
  }

  h1 {
    font-size: 1rem;
    margin: 0;
    letter-spacing: 0.04em;
  }

  .toolbar {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    flex-wrap: wrap;
    font-size: 0.85rem;
  }

  button {
    background: #232830;
    color: inherit;
    border: 1px solid #343b45;
    border-radius: 4px;
    padding: 0.25rem 0.6rem;
    cursor: pointer;
    font-size: 0.85rem;
  }

  button:hover:not(:disabled) {
    background: #2c323c;
  }

  button:disabled {
    opacity: 0.45;
    cursor: default;
  }

  .status,
  .halt,
  .steps {
    color: #8b95a4;
  }

  main {
    display: grid;
    grid-template-columns: minmax(320px, 1fr) minmax(340px, 1fr);
    gap: 1rem;
    padding: 1rem;
    height: calc(100vh - 3.4rem);
    box-sizing: border-box;
  }

  .editor {
    min-height: 0;
  }

  .editor-host {
    height: 100%;
    border: 1px solid #2a2f36;
    border-radius: 6px;
    overflow: hidden;
  }

  .panels {
    display: flex;
    flex-direction: column;
    gap: 0.75rem;
    min-height: 0;
  }

  .panels h2 {
    font-size: 0.78rem;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    color: #8b95a4;
    margin: 0 0 0.35rem;
  }

  .diagnostics {
    flex: 0 1 auto;
    max-height: 22%;
    overflow: auto;
  }

  .registers {
    flex: 1 1 auto;
    min-height: 0;
    overflow: auto;
  }

  .register-columns {
    display: flex;
    gap: 1rem;
  }

  .register-columns table {
    flex: 1 1 0;
  }

  .memory,
  .console {
    flex: 0 0 auto;
  }

  .memory .number {
    width: 7rem;
  }

  ul {
    margin: 0;
    padding: 0;
    list-style: none;
    display: grid;
    gap: 0.25rem;
  }

  li {
    display: flex;
    gap: 0.5rem;
    align-items: baseline;
    font-size: 0.85rem;
    background: #1c2128;
    border-left: 3px solid #d08a3e;
    padding: 0.35rem 0.5rem;
  }

  .where {
    color: #8b95a4;
    font-variant-numeric: tabular-nums;
  }

  .code {
    color: #d08a3e;
    font-family: ui-monospace, monospace;
    font-size: 0.8rem;
  }

  .fix {
    margin-left: auto;
  }

  table {
    width: 100%;
    border-collapse: collapse;
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 0.78rem;
  }

  tr.changed {
    background: #1f3a2b;
  }

  td {
    padding: 0.08rem 0.3rem;
    border-bottom: 1px solid #22272e;
  }

  .number {
    color: #8b95a4;
    width: 2.6rem;
  }

  .name {
    color: #7fb3d5;
    width: 3rem;
  }

  .value {
    text-align: right;
  }

  .signed {
    text-align: right;
    color: #8b95a4;
    width: 5rem;
  }

  pre.empty {
    color: #5c6673;
  }

  pre {
    margin: 0;
    background: #1c2128;
    border-radius: 6px;
    padding: 0.5rem;
    min-height: 3rem;
    max-height: 12rem;
    overflow: auto;
    font-family: ui-monospace, monospace;
    white-space: pre-wrap;
  }

  input {
    background: #1c2128;
    border: 1px solid #343b45;
    color: inherit;
    border-radius: 4px;
    padding: 0.2rem 0.4rem;
    font-family: ui-monospace, monospace;
    margin-bottom: 0.4rem;
    width: 8rem;
  }
</style>
