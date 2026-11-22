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

  // The overlays that exist under `content/overlay/`; `rvh-content` normalizes
  // anything else to English.
  const LOCALES = ['en', 'zh-CN'] as const;
  type Locale = (typeof LOCALES)[number];

  const NAMES = [
    'zero', 'ra', 'sp', 'gp', 'tp', 't0', 't1', 't2', 's0', 's1', 'a0', 'a1', 'a2', 'a3', 'a4',
    'a5', 'a6', 'a7', 's2', 's3', 's4', 's5', 's6', 's7', 's8', 's9', 's10', 's11', 't3', 't4',
    't5', 't6',
  ];

  let engine = $state(null as Engine | null);
  let target = $state('rv32i_zicsr');
  let locale = $state(readLocale());
  let editor: Editor | null = null;
  let editorHost: HTMLDivElement;
  let machine = $state(null as MachineState | null);
  let diagnostics = $state([] as Diagnostic[]);
  let status = $state('loading engine…');
  let addressText = $state('0x100');
  let address = $state(0x100);
  let shareStatus = $state('');

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
    document.documentElement.lang = locale;
    const shared = readShare();
    const initial = shared?.source ?? SAMPLE;
    try {
      if (shared) {
        target = shared.target;
      }
      engine = await createEngine(target);
      editor = createEditor(editorHost, initial, () => analyze());
      analyze();
    } catch {
      // A link may name a target this build does not have: fall back.
      target = 'rv32i_zicsr';
      engine = await createEngine(target);
      editor = createEditor(editorHost, initial, () => analyze());
      analyze();
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

  /// The locale comes from `?lang=`, so a link can pin it and the browser suite
  /// can check it. Unknown values fall back to English, as `rvh-content` does.
  function readLocale(): Locale {
    const requested = new URLSearchParams(location.search).get('lang');
    return LOCALES.find((name) => name.toLowerCase() === requested?.toLowerCase()) ?? 'en';
  }

  /// The fragment carries a shared program: `#t=<target>&s=<base64url>`. The
  /// source travels as UTF-8 bytes so non-ASCII comments survive.
  function encodeSource(text: string): string {
    let binary = '';
    for (const byte of new TextEncoder().encode(text)) {
      binary += String.fromCharCode(byte);
    }
    return btoa(binary).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
  }

  function decodeSource(text: string): string | null {
    try {
      const padded = text.replace(/-/g, '+').replace(/_/g, '/');
      const binary = atob(padded.padEnd(Math.ceil(padded.length / 4) * 4, '='));
      return new TextDecoder().decode(Uint8Array.from(binary, (c) => c.charCodeAt(0)));
    } catch {
      return null;
    }
  }

  function readShare(): { source: string; target: string } | null {
    const params = new URLSearchParams(location.hash.slice(1));
    const encoded = params.get('s');
    if (!encoded) {
      return null;
    }
    const source = decodeSource(encoded);
    return source === null ? null : { source, target: params.get('t') ?? 'rv32i_zicsr' };
  }

  /// A new session starts in English, so every rebuild has to re-apply it.
  async function createEngine(next: string): Promise<Engine> {
    const created = await Engine.load(next);
    created.setLocale(locale);
    return created;
  }

  /// Switch the content locale, then reflect it in the URL and in `<html lang>`.
  /// Only hover text is translated today, and this page has no hover view yet,
  /// so the switch becomes visible once that view lands.
  function pickLocale(next: string) {
    locale = LOCALES.find((name) => name === next) ?? 'en';
    engine?.setLocale(locale);
    const url = new URL(location.href);
    url.searchParams.set('lang', locale);
    history.replaceState(null, '', url);
    document.documentElement.lang = locale;
  }

  /// Switch the ISA: a new session, then re-assemble the same text.
  async function pickTarget(next: string) {
    if (!engine || next === target) {
      return;
    }
    const source = editor?.text() ?? SAMPLE;
    const previous = engine;
    try {
      engine = await createEngine(next);
      target = next;
      previous.dispose();
      editor?.setText(source);
      analyze();
    } catch (error) {
      status = `target rejected: ${error}`;
    }
  }

  /// Put the current program in the address bar and copy the link. The fragment
  /// is the only carrier, so a shared link works without a server.
  async function share() {
    const params = new URLSearchParams();
    params.set('t', target);
    params.set('s', encodeSource(editor?.text() ?? SAMPLE));
    const url = `${location.origin}${location.pathname}${location.search}#${params.toString()}`;
    history.replaceState(null, '', url);
    try {
      await navigator.clipboard.writeText(url);
      shareStatus = 'link copied';
    } catch {
      shareStatus = 'link is in the address bar';
    }
    setTimeout(() => {
      shareStatus = '';
    }, 2500);
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
      <option value="rv32ic" selected={target === 'rv32ic'}>rv32ic</option>
      <option value="rv32if" selected={target === 'rv32if'}>rv32if</option>
      <option value="rv32id" selected={target === 'rv32id'}>rv32id</option>
    </select>
    <button onclick={analyze}>Assemble</button>
    <button onclick={() => step(1)} disabled={!machine}>Step</button>
    <button onclick={() => step(10)} disabled={!machine}>Step ×10</button>
    <button onclick={run} disabled={!machine}>Run</button>
    <button onclick={undo} disabled={!machine}>Back</button>
    <button onclick={reset} disabled={!machine}>Reset</button>
    <button onclick={share} title="Copy a link that carries this program">Share</button>
    <span class="share-status">{shareStatus}</span>
    <span class="status">{status}</span>
    <span class="halt">{halted}</span>
    <span class="steps">{machine ? `${machine.steps} step(s) at ${hex(machine.pc)}` : ''}</span>
  </div>
  <label class="locale-picker" title="Content locale: hover text">
    <svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true">
      <circle cx="8" cy="8" r="6.25" />
      <ellipse cx="8" cy="8" rx="2.6" ry="6.25" />
      <path d="M1.75 8h12.5" />
    </svg>
    <select class="locale" aria-label="Content locale" onchange={(event) => pickLocale(event.currentTarget.value)}>
      <option value="en" selected={locale === 'en'}>English</option>
      <option value="zh-CN" selected={locale === 'zh-CN'}>中文</option>
    </select>
  </label>
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

  select {
    background: #232830;
    color: inherit;
    border: 1px solid #343b45;
    border-radius: 4px;
    padding: 0.22rem 0.4rem;
    font-size: 0.85rem;
  }

  /* The globe sits at the right edge; `margin-left: auto` is what pins it there
     while the toolbar keeps growing to its left. */
  .locale-picker {
    display: flex;
    align-items: center;
    gap: 0.35rem;
    margin-left: auto;
    color: #8b95a4;
  }

  .locale-picker svg {
    flex: none;
    fill: none;
    stroke: currentColor;
    stroke-width: 1.2;
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
  .steps,
  .share-status {
    color: #8b95a4;
  }

  .share-status {
    min-width: 8rem;
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
