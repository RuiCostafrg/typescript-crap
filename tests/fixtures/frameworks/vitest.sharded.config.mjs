import { defineConfig } from "vitest/config";

const coverageShard = process.env.VITEST_COVERAGE_SHARD === "1";

export default defineConfig({
  oxc: { jsx: { runtime: "automatic" } },
  test: {
    maxWorkers: 1,
    projects: [
      { extends: true, test: { name: "ui", include: ["packages/ui/*.test.tsx"] } },
      {
        extends: true,
        test: { name: "service", globals: true, include: ["packages/service/*.test.ts"] },
      },
    ],
    coverage: {
      provider: "istanbul",
      include: ["packages/*/src/**/*.{ts,tsx}", "packages/ui/coverage-gaps/*.tsx"],
      reporter: coverageShard ? [] : ["text", "json"],
      // The blob carries shard coverage. Apply the usual percentage gate to the merge.
      thresholds: coverageShard ? undefined : { lines: 100, functions: 100 },
    },
  },
});
