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
    // The preview host is pinned in vite.config.ts; the flag repeats it so the
    // command line says what the config does.
    command: `pnpm --dir .. run wasm && pnpm run build && pnpm run preview --host 127.0.0.1 --port ${port} --strictPort`,
    url: baseURL,
    reuseExistingServer: false,
    // A hung server should fail in a minute, not three; the piped output then
    // shows which address it bound.
    timeout: 60_000,
    stdout: 'pipe',
    stderr: 'pipe',
  },
});
