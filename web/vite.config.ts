import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';

export default defineConfig({
  plugins: [svelte()],
  // A project page is served from /<repository>/, so the deploy job sets this.
  base: process.env.RVH_BASE ?? '/',
  build: { target: 'es2022' },
});
