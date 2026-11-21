// Captures the README demo chapter from a running Extension Development Host.
//
//   node xtask/capture-vscode.mjs
//   for story in definition completion target debug; do
//     python3 xtask/assemble-gif.py assets/frames/$story assets/demos/$story.gif --fps 1
//   done
//
// Every story that is a *sequence* becomes a GIF; hover is a state, so it stays a
// still. The script rewrites each buffer from the file on disk, so a previous run
// leaves nothing behind.
import { createRequire } from 'node:module';
import { mkdirSync, readFileSync } from 'node:fs';

// Playwright is a dependency of web/, not of the workspace root.
const require = createRequire(new URL('../web/package.json', import.meta.url));
const { chromium } = require('@playwright/test');

const PORT = process.env.RVH_CDP_PORT ?? '9356';
const FRAMES = new URL('../assets/frames/', import.meta.url).pathname;
const DEMOS = new URL('../assets/demos/', import.meta.url).pathname;
mkdirSync(DEMOS, { recursive: true });

const browser = await chromium.connectOverCDP(`http://127.0.0.1:${PORT}`);
const page = browser.contexts().flatMap((c) => c.pages()).find((p) => p.url().includes('workbench'));
if (!page) throw new Error('no workbench page');

const shot = async (story, name) => {
  await page.mouse.move(4, 4); // outside the editor: no hover in the frame
  await page.waitForTimeout(900);
  mkdirSync(`${FRAMES}${story}`, { recursive: true });
  await page.screenshot({ path: `${FRAMES}${story}/${name}.png` });
  console.log('shot', story, name);
};

const still = async (name) => {
  await page.mouse.move(4, 4);
  await page.waitForTimeout(900);
  await page.screenshot({ path: `${DEMOS}${name}.png` });
  console.log('shot', name);
};

const open = async (file, source) => {
  await page.keyboard.press('Escape');
  await page.keyboard.press('Control+p');
  await page.waitForSelector('.quick-input-widget', { timeout: 10_000 });
  await page.keyboard.type(file);
  await page.waitForTimeout(600);
  await page.keyboard.press('Enter');
  await page.waitForSelector('.monaco-editor .view-lines', { timeout: 15_000 });
  await page.keyboard.press('Control+a');
  await page.keyboard.insertText(readFileSync(new URL(`../tests/corpus/${source}`, import.meta.url), 'utf8'));
  await page.keyboard.press('Control+s'); // buffer matches the file: no dirty dot
  await page.waitForTimeout(1000);
  const tab = await page.$eval('.tab.active', (n) => (n.textContent || '').trim()).catch(() => '?');
  console.log('  opened', file, '->', tab);
};

const goto_line = async (line) => {
  await page.keyboard.press('Control+g');
  await page.waitForSelector('.quick-input-widget', { timeout: 10_000 });
  await page.keyboard.type(String(line));
  await page.waitForTimeout(400);
  await page.keyboard.press('Enter');
};

await page.waitForSelector('.monaco-workbench', { timeout: 30_000 });
const sidebar = await page.$eval('.part.sidebar', (n) => n.clientWidth).catch(() => 0);
if (sidebar > 0) await page.keyboard.press('Control+b');
const panel = await page.$eval('.part.panel', (n) => n.clientHeight).catch(() => 0);
if (panel > 0) await page.keyboard.press('Control+j');

// No breakpoints from a previous run.
await page.keyboard.press('Control+Shift+p');
await page.waitForSelector('.quick-input-widget', { timeout: 10_000 });
await page.keyboard.type('Remove All Breakpoints');
await page.waitForTimeout(500);
await page.keyboard.press('Enter');
await page.waitForTimeout(500);

// 1. Hover: a state, so one still. Line 9 is `add`.
await open('001_loop_sum', '001_loop_sum.s');
await goto_line(9);
await page.keyboard.press('Home');
await page.keyboard.press('Control+k');
await page.keyboard.press('Control+i');
await page.waitForTimeout(1200);
await still('hover');
await page.keyboard.press('Escape');
await page.keyboard.press('Control+w');

// 2. Go to definition: the jump is the story, so two frames.
await open('002_factorial', '002_factorial.s');
await goto_line(6);
await page.keyboard.press('End');
await page.keyboard.press('ArrowLeft');
await page.keyboard.press('ArrowLeft');
await shot('definition', '01-before');
await page.keyboard.press('F12');
await page.waitForTimeout(1200);
await shot('definition', '02-after');
await page.keyboard.press('Control+w');

// 3. Completion: typing, the list, then the accepted line.
await open('001_loop_sum', '001_loop_sum.s');
await page.keyboard.press('Control+End');
await page.keyboard.insertText('\n    ad');
await page.waitForTimeout(600);
await page.keyboard.press('Control+Space');
await page.waitForTimeout(1200);
await shot('completion', '01-list');
await page.keyboard.press('Enter');
await page.waitForTimeout(800);
await shot('completion', '02-accepted');
await page.keyboard.press('Control+z'); // the accepted line
await page.keyboard.press('Control+z'); // the typed prefix
await page.keyboard.press('Control+s');
await page.keyboard.press('Control+w');

// 4. Switch the target: command palette, then the picker, then the file marked.
await page.keyboard.press('Control+Shift+p');
await page.waitForSelector('.quick-input-widget', { timeout: 10_000 });
await page.keyboard.type('Select target for this file');
await page.waitForTimeout(600);
await shot('target', '01-command');
await page.keyboard.press('Enter');
await page.waitForTimeout(900);
await shot('target', '02-picker');
await page.keyboard.press('Escape');

// 5. Debug: the breakpoint, the entry stop, the breakpoint stop, a step. Same
// file as the completion story, so the tab that is open is the tab that is shown.
await open('001_loop_sum', '001_loop_sum.s');
await goto_line(11);
await page.keyboard.press('Home');
await page.keyboard.press('F9');
await page.keyboard.press('Control+b'); // the debug sidebar
await page.waitForTimeout(500);
await page.keyboard.press('F5');
await page.waitForTimeout(4000);
await shot('debug', '01-entry');
await page.keyboard.press('F5');
await page.waitForTimeout(2500);
await shot('debug', '02-breakpoint');
await page.keyboard.press('F10');
await page.keyboard.press('F10');
await page.waitForTimeout(1500);
await shot('debug', '03-step');
await page.keyboard.press('Shift+F5');
await page.waitForTimeout(800);
await page.keyboard.press('Control+b');

await browser.close();
