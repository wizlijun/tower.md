import { svelte } from '@sveltejs/vite-plugin-svelte';
import { defineConfig } from 'vitest/config';

export default defineConfig({
  plugins: [svelte({ hot: false })],
  resolve: { conditions: ['browser'] },
  test: {
    environment: 'happy-dom',
    include: ['src/**/*.test.ts', 'scripts/stage-cli.test.ts', 'scripts/build-app.test.ts'],
  },
});
