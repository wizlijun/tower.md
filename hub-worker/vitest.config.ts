import { defineWorkersConfig } from "@cloudflare/vitest-pool-workers/config";

export default defineWorkersConfig({
  test: {
    poolOptions: {
      workers: {
        wrangler: { configPath: "./wrangler.toml" },
        // Same setting as Note.md's mdrelay: shared Durable Object storage
        // across tests avoids sqlite teardown failures; tests use unique
        // project ids instead of isolated storage.
        isolatedStorage: false,
        singleWorker: true,
      },
    },
  },
});
