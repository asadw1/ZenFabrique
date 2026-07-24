import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';

export default defineConfig({
  plugins: [svelte()],
  // This is a pure client-side SPA (no SSR), so always resolving Svelte's
  // "browser" export condition is correct — without this, Vitest (which
  // runs under Node) picks Svelte's server-rendering build instead of the
  // client one, and `mount()` fails with "not available on the server".
  resolve: {
    conditions: ['browser'],
  },
  test: {
    environment: 'jsdom',
  },
});
