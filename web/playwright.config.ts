import { defineConfig } from '@playwright/test';

// A port of its own: the developer's `pnpm dev` usually sits on 5173.
const port = 5199;
const baseURL = `http://127.0.0.1:${port}`;

export default defineConfig({
  testDir: './e2e',
  timeout: 30_000,
  fullyParallel: false,
  reporter: [['list']],
  use: {
    baseURL,
    headless: true,
    screenshot: 'only-on-failure',
  },
  // Test the built artifact, not the dev server: that is what ships, and it
  // also proves the static-host story (no backend, no special headers).
  webServer: {
    command: `pnpm --dir .. run wasm && pnpm run build && pnpm run preview --port ${port} --strictPort`,
    url: baseURL,
    reuseExistingServer: false,
    timeout: 180_000,
  },
});
