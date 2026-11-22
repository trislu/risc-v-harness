import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';

export default defineConfig({
  plugins: [svelte()],
  // A project page is served from /<repository>/, so the deploy job sets this.
  base: process.env.RVH_BASE ?? '/',
  build: { target: 'es2022' },
  // `localhost` can resolve to ::1 on a CI runner; the e2e readiness poll asks
  // 127.0.0.1, so the preview server binds the IPv4 loopback explicitly.
  preview: { host: '127.0.0.1' },
});
