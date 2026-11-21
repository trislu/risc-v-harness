import { expect, test, type Page } from '@playwright/test';

const STEP = { name: 'Step', exact: true } as const;

function collectErrors(page: Page): string[] {
  const errors: string[] = [];
  page.on('pageerror', (error) => errors.push(`pageerror: ${error.message}`));
  page.on('console', (message) => {
    if (message.type() === 'error') {
      errors.push(`console: ${message.text()}`);
    }
  });
  return errors;
}

async function assemble(page: Page): Promise<void> {
  await page.goto('/');
  await expect(page.locator('.status')).toHaveText('assembled');
}

test('the sample assembles and the highlight follows the program counter', async ({ page }) => {
  const errors = collectErrors(page);
  await assemble(page);

  const executing = page.locator('.cm-line.cm-executing');
  await expect(executing).toHaveCount(1);
  await expect(executing).toContainText('li t0, 0x10000000');
  await expect(page.locator('.halt')).toHaveText('running');

  await page.getByRole('button', STEP).click();
  await expect(executing).toContainText('li t1, 72');

  const t0 = page.locator('tr').filter({ has: page.locator('td.name', { hasText: /^t0$/ }) });
  await expect(t0).toHaveClass(/changed/);
  await expect(t0.locator('td.value')).toHaveText('0x10000000');

  expect(errors).toEqual([]);
});

test('eight steps reach ebreak and print to the console', async ({ page }) => {
  const errors = collectErrors(page);
  await assemble(page);

  for (let index = 0; index < 8; index += 1) {
    await page.getByRole('button', STEP).click();
  }

  await expect(page.locator('.halt')).toHaveText('halted: ebreak');
  await expect(page.locator('.steps')).toContainText('8 step(s)');
  await expect(page.locator('.console pre')).toContainText('Hi');
  await expect(page.locator('.cm-line.cm-executing')).toContainText('ebreak');

  await page.getByRole('button', { name: 'Back' }).click();
  await expect(page.locator('.steps')).toContainText('7 step(s)');
  await expect(page.locator('.halt')).toHaveText('running');

  expect(errors).toEqual([]);
});

test('a diagnostic fix rewrites the source', async ({ page }) => {
  const errors = collectErrors(page);
  await assemble(page);

  const content = page.locator('.cm-content');
  await content.click();
  await page.keyboard.press('Control+A');
  await page.keyboard.press('Delete');
  await page.keyboard.type('adii a0, a1, 1');

  await expect(page.locator('.status')).toContainText('diagnostic');
  const fix = page.locator('.fix');
  await expect(fix).toHaveCount(1);
  await expect(fix).toContainText('replace with `addi`');
  await fix.click();

  await expect(page.locator('.cm-content')).toContainText('addi a0, a1, 1');
  await expect(page.locator('.diagnostics')).toHaveCount(0);
  await expect(page.locator('.status')).toHaveText('assembled');

  expect(errors).toEqual([]);
});

// Recording the README demo: `RVH_RECORD=1 pnpm --dir web run test:e2e -- -g record`.
// It writes frames, not a GIF; `xtask/assemble-gif.py` turns them into one.
test('record the README demo frames', async ({ page }) => {
  test.skip(process.env.RVH_RECORD !== '1', 'set RVH_RECORD=1 to write frames');

  await assemble(page);
  const frame = (name: string) =>
    page.screenshot({ path: `../assets/frames/playground/${name}.png`, fullPage: true });

  const content = page.locator('.cm-content');
  await content.click();
  await page.keyboard.press('Control+A');
  await page.keyboard.press('Delete');
  // `insertText` inserts the string verbatim; `type` would press Enter between
  // lines, and the editor's auto-indent would then stack on the spaces written
  // here, one level per line.
  await page.keyboard.insertText(
    '_start:\n    li a0, 6\n    li a1, 7\n    mul a2, a0, a1\n    ebreak',
  );
  // Park the caret at the end: a leftover insertion selection would draw a box
  // around the first line in the still.
  await page.keyboard.press('Control+End');
  await page.keyboard.press('ArrowUp'); // the caret rests on the line with the error
  await expect(page.locator('.status')).toContainText('diagnostic');
  await frame('01-typing-mul');

  const fix = page.locator('.fix');
  await expect(fix).toHaveCount(1);
  await fix.click();
  await expect(page.locator('.status')).toHaveText('assembled');
  await expect(page.locator('.cm-line').first()).toContainText('.option arch, +m');
  await frame('02-fixed');

  await page.getByRole('button', { name: 'Step', exact: true }).click();
  await page.getByRole('button', { name: 'Step', exact: true }).click();
  await frame('03-running');

  expect(collectErrors(page)).toEqual([]);
});

test('the target selector decides which instructions are legal', async ({ page }) => {
  await assemble(page);

  // rv32i has no multiply/divide, so `mul` is rejected.
  await page.selectOption('.target', 'rv32i');
  await expect(page.locator('.status')).toContainText('assembled');
  const content = page.locator('.cm-content');
  await content.click();
  await page.keyboard.press('Control+a');
  await page.keyboard.press('Delete');
  await page.keyboard.insertText('_start:\n    mul a0, a1, a2\n    ebreak');
  await expect(page.locator('.status')).toContainText('diagnostic');
  await expect(page.locator('.diagnostics')).toContainText('needs extension `m`');

  // The same text assembles once the target includes M.
  await page.selectOption('.target', 'rv32im');
  await expect(page.locator('.status')).toHaveText('assembled');
  await expect(page.locator('.diagnostics')).toHaveCount(0);
});

test('typing does not move the cursor', async ({ page }) => {
  await assemble(page);
  const content = page.locator('.cm-content');
  await content.click();
  await page.keyboard.press('Control+End');
  await page.keyboard.type('# note');

  // The line after the sample keeps the typed text, so the caret stayed put.
  await expect(content).toContainText('# note');
  await expect(page.locator('.cm-line.cm-executing')).toHaveCount(1);
});

test('the page renders as a screenshot', async ({ page }) => {
  await assemble(page);
  await page.getByRole('button', STEP).click();
  await page.waitForTimeout(300);
  // Committed on purpose: it is the picture the README shows.
  await page.screenshot({ path: '../assets/playground.png', fullPage: true });
});
