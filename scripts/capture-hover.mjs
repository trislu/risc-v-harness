// Records the hover story as a sequence: the pointer travels to the token, the
// tooltip opens, and it stays open long enough to read.
//
//   node scripts/capture-hover.mjs
//   python3 scripts/assemble-gif.py assets/frames/hover assets/demos/hover.gif --fps 1
//
// The pointer is drawn as an overlay because a screenshot contains the page, not
// the host cursor; the mouse events themselves are real (CDP input), so the
// editor opens the hover on its own.
import { execFileSync } from 'node:child_process';
import { createRequire } from 'node:module';
import { mkdirSync, readFileSync, rmSync } from 'node:fs';

const require = createRequire(new URL('../web/package.json', import.meta.url));
const { chromium } = require('@playwright/test');

const PORT = process.env.RVH_CDP_PORT ?? '9353';
const FRAMES = new URL('../assets/frames/hover/', import.meta.url).pathname;
rmSync(FRAMES, { recursive: true, force: true });
mkdirSync(FRAMES, { recursive: true });

const browser = await chromium.connectOverCDP(`http://127.0.0.1:${PORT}`);
const page = browser
  .contexts()
  .flatMap((context) => context.pages())
  .find((candidate) => candidate.url().includes('workbench'));
if (!page) throw new Error('no workbench page');

await page.waitForSelector('.monaco-workbench', { timeout: 30_000 });
for (const part of ['.part.sidebar', '.part.panel']) {
  const size = await page
    .$eval(part, (node) => node.clientWidth || node.clientHeight)
    .catch(() => 0);
  if (size > 0) await page.keyboard.press(part === '.part.sidebar' ? 'Control+b' : 'Control+j');
}

// Open the file and make the buffer match it, so the tab shows no dirty dot.
await page.keyboard.press('Control+p');
await page.waitForSelector('.quick-input-widget', { timeout: 10_000 });
await page.keyboard.type('001_loop_sum');
await page.waitForTimeout(600);
await page.keyboard.press('Enter');
await page.waitForSelector('.monaco-editor .view-lines', { timeout: 20_000 });
await page.keyboard.press('Control+a');
await page.keyboard.insertText(
  readFileSync(new URL('../tests/corpus/001_loop_sum.s', import.meta.url), 'utf8'),
);
await page.keyboard.press('Control+s');
await page.waitForTimeout(900);

// A pointer the screenshots can see: a small arrow that follows the mouse.
await page.evaluate(() => {
  // A previous run may have left its pointer behind; keep exactly one.
  for (const stale of document.querySelectorAll('#rvh-pointer')) stale.remove();
  const arrow = document.createElement('div');
  arrow.id = 'rvh-pointer';
  arrow.style.cssText = [
    'position:fixed',
    'left:0',
    'top:0',
    'width:22px',
    'height:22px',
    'z-index:99999',
    'pointer-events:none',
    'filter:drop-shadow(0 0 1px rgba(0,0,0,.9))',
  ].join(';');
  // VS Code enforces Trusted Types, so the arrow is built node by node.
  const ns = 'http://www.w3.org/2000/svg';
  const svg = document.createElementNS(ns, 'svg');
  svg.setAttribute('viewBox', '0 0 24 24');
  svg.setAttribute('width', '22');
  svg.setAttribute('height', '22');
  const path = document.createElementNS(ns, 'path');
  path.setAttribute('d', 'M4 2 L4 20 L9 15 L12 22 L15 20.5 L12 13.5 L19 13.5 Z');
  path.setAttribute('fill', '#fff');
  path.setAttribute('stroke', '#111');
  path.setAttribute('stroke-width', '1.2');
  svg.appendChild(path);
  arrow.appendChild(svg);
  document.body.appendChild(arrow);
});

const move = async (x, y) => {
  await page.evaluate(
    ([left, top]) => {
      const arrow = document.getElementById('rvh-pointer');
      if (arrow) arrow.style.transform = `translate(${left}px, ${top}px)`;
    },
    [x, y],
  );
  await page.mouse.move(x, y);
};

// The X cursor is composited into a screenshot; park it off the window so the
// only pointer in the frame is the drawn one.
const park = () => {
  try {
    execFileSync('python3', [new URL('./park-pointer.py', import.meta.url).pathname]);
  } catch {
    // No X display: nothing to park.
  }
};

const shot = async (name) => {
  park();
  await page.waitForTimeout(150); // let the X server drop the cursor before the grab
  // Crop the title bar: the X pointer sits in its corner, and the tab under it
  // already says which file is open.
  await page.screenshot({
    path: `${FRAMES}${name}.png`,
    clip: { x: 0, y: 44, width: 1000, height: 581 },
  });
  console.log('shot', name);
};

// The `add` token on the loop line: hover there, not on the label.
const token = await page.evaluate(() => {
  for (const line of document.querySelectorAll('.view-lines .view-line')) {
    for (const span of line.querySelectorAll('span span')) {
      if ((span.textContent || '').trim() !== 'add') continue;
      // Aim at the lower-right corner of the token: still inside it, but the
      // arrow does not cover the letters.
      const box = span.getBoundingClientRect();
      return { x: box.right - 8, y: box.y + box.height / 2 };
    }
  }
  return null;
});
if (!token) throw new Error('the `add` token is not on screen');

await page.keyboard.press('Escape');
await move(token.x - 260, token.y + 120);
await page.waitForTimeout(600);
await shot('01-before');

await move(token.x - 30, token.y - 6);
await page.waitForTimeout(500);
await shot('02-approach');

await move(token.x, token.y);
await page
  .waitForSelector('.monaco-hover', { timeout: 8_000 })
  .catch(() => console.log('warning: no hover widget appeared'));
await page.waitForTimeout(900);
await shot('03-hover');

await page.waitForTimeout(1200);
await shot('04-hold');

console.log('frames written to assets/frames/hover');
await browser.close();
