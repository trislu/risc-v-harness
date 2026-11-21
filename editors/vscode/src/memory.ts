import { debug, type WebviewView, type WebviewViewProvider } from 'vscode';

/** What the adapter reports on every stop. */
export interface MachineState {
  pc: string;
  sp: string;
  steps: number;
}

/**
 * The memory view: a hex grid that follows `sp` or `pc`.
 *
 * Bytes that changed since the previous render are highlighted, which is the one
 * thing this view has over a generic hex editor: the simulator knows exactly
 * which bytes a step touched.
 */
export class MemoryView implements WebviewViewProvider {
  public static readonly viewType = 'rvh.memory';
  private view?: WebviewView;

  public resolveWebviewView(view: WebviewView): void {
    this.view = view;
    view.webview.options = { enableScripts: true };
    view.webview.html = html();

    view.webview.onDidReceiveMessage(async (message: { type?: string; address?: string; count?: number }) => {
      if (message?.type !== 'read' || !message.address) {
        return;
      }
      const session = debug.activeDebugSession;
      if (!session || session.type !== 'rvh') {
        this.post({ type: 'error', message: 'no active rvh session' });
        return;
      }
      try {
        const result = (await session.customRequest('readMemory', {
          memoryReference: message.address,
          count: message.count ?? 256,
        })) as { address: string; data: string };
        this.post({
          type: 'bytes',
          address: result.address,
          bytes: base64ToBytes(result.data),
        });
      } catch (error) {
        this.post({ type: 'error', message: String(error) });
      }
    });
  }

  /** Forward a new machine state to the grid. */
  public follow(state: MachineState): void {
    this.post({ type: 'state', ...state });
  }

  /** Tell the grid the session ended. */
  public clear(): void {
    this.post({ type: 'cleared' });
  }

  private post(message: unknown): void {
    void this.view?.webview.postMessage(message);
  }
}

function base64ToBytes(text: string): number[] {
  return Array.from(Buffer.from(text, 'base64'));
}

function html(): string {
  return `<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<style>
  body { font-family: var(--vscode-editor-font-family); font-size: 12px; padding: 0; margin: 0; color: var(--vscode-foreground); }
  .bar { display: flex; gap: 4px; align-items: center; padding: 4px 6px; border-bottom: 1px solid var(--vscode-panel-border); }
  input { width: 92px; background: var(--vscode-input-background); color: var(--vscode-input-foreground); border: 1px solid var(--vscode-input-border, transparent); padding: 2px 4px; font-family: inherit; }
  button { background: var(--vscode-button-secondaryBackground); color: var(--vscode-button-secondaryForeground); border: none; padding: 2px 6px; cursor: pointer; }
  button.on { background: var(--vscode-button-background); color: var(--vscode-button-foreground); }
  .hint { color: var(--vscode-descriptionForeground); padding: 2px 6px; font-size: 11px; }
  .grid { padding: 4px 6px; white-space: pre; line-height: 1.45; }
  .mark { display: inline-block; width: 2ch; color: var(--vscode-charts-blue); }
  .addr { color: var(--vscode-descriptionForeground); }
  .byte { display: inline-block; width: 2.1ch; text-align: right; }
  .byte.changed { background: var(--vscode-editor-findMatchHighlightBackground); }
  .ascii { color: var(--vscode-descriptionForeground); margin-left: 8px; }
  .status { color: var(--vscode-descriptionForeground); padding: 2px 6px; border-top: 1px solid var(--vscode-panel-border); }
</style>
</head>
<body>
<div class="bar">
  <input id="address" value="0x00000000" spellcheck="false" title="Address to read from">
  <button id="go" title="Read 256 bytes starting at the address in the box">Go</button>
  <button id="sp" title="Keep the window on the stack pointer: saved registers and locals live there">Stack (sp)</button>
  <button id="pc" title="Keep the window on the program counter: the bytes of the instruction about to run">Code (pc)</button>
</div>
<div class="hint" id="hint">manual · 256 bytes · changed bytes are highlighted</div>
<div class="grid" id="grid"></div>
<div class="status" id="status">no session</div>
<script>
  const vscode = acquireVsCodeApi();
  const grid = document.getElementById('grid');
  const status = document.getElementById('status');
  const hint = document.getElementById('hint');
  const input = document.getElementById('address');
  const followSp = document.getElementById('sp');
  const followPc = document.getElementById('pc');

  let follow = 'sp';
  let current = 0;
  let previous = new Map();
  let lastState = { pc: '0x00000000', sp: '0x00000000', steps: 0 };

  function parse(text) {
    const trimmed = String(text).trim();
    return trimmed.startsWith('0x') || trimmed.startsWith('0X')
      ? parseInt(trimmed.slice(2), 16) || 0
      : parseInt(trimmed, 10) || 0;
  }
  function hex(value, width) {
    return '0x' + (value >>> 0).toString(16).padStart(width, '0');
  }
  function request() {
    vscode.postMessage({ type: 'read', address: hex(current, 8), count: 256 });
  }
  function markers(address) {
    const pc = parse(lastState.pc);
    const sp = parse(lastState.sp);
    if (pc >= address && pc < address + 16) return '▶';
    if (sp >= address && sp < address + 16) return '↓';
    return '';
  }
  function render(address, bytes) {
    current = address;
    const next = new Map();
    let out = '';
    for (let row = 0; row < bytes.length; row += 16) {
      const rowAddress = address + row;
      out += '<span class="mark">' + markers(rowAddress) + '</span>';
      out += '<span class="addr">' + hex(rowAddress, 8) + '</span>  ';
      let ascii = '';
      for (let column = 0; column < 16; column += 1) {
        const index = row + column;
        if (index >= bytes.length) { out += '   '; continue; }
        const value = bytes[index];
        next.set(address + index, value);
        const changed = previous.has(address + index) && previous.get(address + index) !== value;
        out += '<span class="byte' + (changed ? ' changed' : '') + '">' +
          value.toString(16).padStart(2, '0') + '</span> ';
        ascii += value >= 32 && value < 127 ? String.fromCharCode(value) : '.';
      }
      out += '<span class="ascii">' + ascii + '</span>\\n';
    }
    grid.innerHTML = out;
    previous = next;
    input.value = hex(address, 8);
    hint.textContent = (follow === 'sp' ? 'following sp' : follow === 'pc' ? 'following pc' : 'manual') +
      ' · 256 bytes · changed bytes are highlighted';
    status.textContent = hex(address, 8) + ' · sp ' + lastState.sp + ' · pc ' + lastState.pc + ' · step ' + lastState.steps;
  }
  function updateButtons() {
    followSp.className = follow === 'sp' ? 'on' : '';
    followPc.className = follow === 'pc' ? 'on' : '';
  }
  document.getElementById('go').addEventListener('click', () => {
    follow = 'manual';
    current = parse(input.value);
    updateButtons();
    request();
  });
  followSp.addEventListener('click', () => { follow = 'sp'; current = parse(lastState.sp); updateButtons(); request(); });
  followPc.addEventListener('click', () => { follow = 'pc'; current = parse(lastState.pc); updateButtons(); request(); });

  window.addEventListener('message', (event) => {
    const message = event.data;
    if (message.type === 'state') {
      lastState = message;
      if (follow === 'sp') { current = parse(message.sp); request(); }
      else if (follow === 'pc') { current = parse(message.pc); request(); }
      else { render(current, []); status.textContent = hex(current, 8) + ' · sp ' + message.sp + ' · pc ' + message.pc; }
    } else if (message.type === 'bytes') {
      render(parse(message.address), message.bytes);
    } else if (message.type === 'error') {
      status.textContent = message.message;
    } else if (message.type === 'cleared') {
      previous = new Map();
      grid.innerHTML = '';
      status.textContent = 'session ended';
    }
  });
  updateButtons();
</script>
</body>
</html>`;
}
