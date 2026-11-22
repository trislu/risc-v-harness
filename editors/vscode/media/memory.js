// Runs inside the webview. It cannot call into the extension directly, only
// exchange messages over the channel that `acquireVsCodeApi` returns.

const vscode = acquireVsCodeApi();

const grid = document.getElementById('grid');
// Not `status`: a top-level `status` in a classic script collides with the
// deprecated `window.status` global, and the editor then flags this binding.
const statusLine = document.getElementById('status');
const hint = document.getElementById('hint');
const input = document.getElementById('address');
const followSp = document.getElementById('sp');
const followPc = document.getElementById('pc');

/** `sp`, `pc`, or `manual` when the user typed an address. */
let follow = 'sp';
/** Address of the block currently on screen. */
let current = 0;
/** Byte values from the previous render, for the change highlight. */
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
      if (index >= bytes.length) {
        out += '   ';
        continue;
      }
      const value = bytes[index];
      next.set(address + index, value);
      const changed = previous.has(address + index) && previous.get(address + index) !== value;
      out +=
        '<span class="byte' + (changed ? ' changed' : '') + '">' +
        value.toString(16).padStart(2, '0') +
        '</span> ';
      ascii += value >= 32 && value < 127 ? String.fromCharCode(value) : '.';
    }
    out += '<span class="ascii">' + ascii + '</span>\n';
  }
  grid.innerHTML = out;
  previous = next;
  input.value = hex(address, 8);
  hint.textContent =
    (follow === 'sp' ? 'following sp' : follow === 'pc' ? 'following pc' : 'manual') +
    ' · 256 bytes · changed bytes are highlighted';
  statusLine.textContent =
    hex(address, 8) + ' · sp ' + lastState.sp + ' · pc ' + lastState.pc + ' · step ' + lastState.steps;
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

followSp.addEventListener('click', () => {
  follow = 'sp';
  current = parse(lastState.sp);
  updateButtons();
  request();
});

followPc.addEventListener('click', () => {
  follow = 'pc';
  current = parse(lastState.pc);
  updateButtons();
  request();
});

window.addEventListener('message', (event) => {
  const message = event.data;
  if (message.type === 'state') {
    lastState = message;
    if (follow === 'sp') {
      current = parse(message.sp);
      request();
    } else if (follow === 'pc') {
      current = parse(message.pc);
      request();
    } else {
      render(current, []);
      statusLine.textContent = hex(current, 8) + ' · sp ' + message.sp + ' · pc ' + message.pc;
    }
  } else if (message.type === 'bytes') {
    render(parse(message.address), message.bytes);
  } else if (message.type === 'error') {
    statusLine.textContent = message.message;
  } else if (message.type === 'cleared') {
    previous = new Map();
    grid.innerHTML = '';
    statusLine.textContent = 'session ended';
  }
});

updateButtons();
