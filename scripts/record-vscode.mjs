// Records the VS Code demo from a running Extension Development Host.
//
//   1. start the host with --remote-debugging-port and the workspace open
//   2. node record-vscode.mjs
//   3. python3 ../scripts/assemble-gif.py ../assets/frames/vscode-fix ../assets/vscode-fix.gif
//
// The story: paste `005_trap_csr.s`, append a `mul` its target lacks, take the
// quick fix, then open the target picker. The buffer is rewritten from the file
// on disk first, so a previous run leaves no state behind.
import { createRequire } from 'node:module';
// Playwright is a dependency of web/, not of the workspace root.
const require = createRequire(new URL('../web/package.json', import.meta.url));
const { chromium } = require('@playwright/test');
import { mkdirSync, readFileSync } from 'node:fs';

const PORT = process.env.RVH_CDP_PORT ?? '9353';
const OUT = new URL('../assets/frames/vscode-fix/', import.meta.url).pathname;
const SOURCE = readFileSync(new URL('../tests/corpus/005_trap_csr.s', import.meta.url), 'utf8');
mkdirSync(OUT, { recursive: true });

const browser = await chromium.connectOverCDP(`http://127.0.0.1:${PORT}`);
const page = browser.contexts().flatMap((c) => c.pages()).find((p) => p.url().includes('workbench'));
if (!page) throw new Error('no workbench page');

const shot = async (name) => {
  await page.mouse.move(4, 4); // outside the editor: no hover in the frame
  await page.waitForTimeout(900);
  await page.screenshot({ path: `${OUT}${name}.png` });
  console.log('shot', name);
};

await page.waitForSelector('.monaco-workbench', { timeout: 30_000 });
await page.keyboard.press('Escape');

// Hide the sidebar only when it is visible: Ctrl+B toggles, so it is not safe
// to press blindly.
const sidebarWidth = await page.$eval('.part.sidebar', (node) => node.clientWidth).catch(() => 0);
if (sidebarWidth > 0) {
  await page.keyboard.press('Control+b');
  await page.waitForTimeout(600);
}

// Open the file, then replace the buffer with the file on disk so a previous
// run cannot leave extra lines behind.
await page.keyboard.press('Control+p');
await page.waitForSelector('.quick-input-widget', { timeout: 10_000 });
await page.keyboard.type('005_trap_csr');
await page.waitForTimeout(600);
await page.keyboard.press('Enter');
await page.waitForSelector('.monaco-editor .view-lines', { timeout: 15_000 });
await page.keyboard.press('Control+a');
await page.keyboard.insertText(SOURCE);
await page.waitForTimeout(1500);
await shot('01-file');

// Append an instruction this file's target does not allow. No click: a click
// would start a hover that then sits in the frame.
await page.keyboard.press('Escape'); // close anything the paste opened
await page.keyboard.press('Control+End');
await page.keyboard.insertText('\n    mul a2, a0, a1');
await page.waitForTimeout(1800);
await shot('02-squiggle');

// The light bulb's menu.
await page.keyboard.press('Control+.');
await page.waitForTimeout(900);
await shot('03-code-action');
await page.keyboard.press('Enter');
await page.waitForTimeout(1500);
await shot('04-fixed');

// The target picker, from the command palette.
await page.keyboard.press('Control+Shift+p');
await page.waitForSelector('.quick-input-widget', { timeout: 10_000 });
await page.keyboard.type('Select target for this file');
await page.waitForTimeout(600);
await shot('05-command');
await page.keyboard.press('Enter');
await page.waitForTimeout(900);
await shot('06-picker');
await page.keyboard.press('Escape');

await browser.close();
