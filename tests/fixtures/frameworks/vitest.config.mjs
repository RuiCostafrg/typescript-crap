import { defineConfig } from 'vitest/config';

export default defineConfig({
  esbuild: { jsx: 'automatic' },
  test: {
    include: ['packages/ui/*.test.tsx'],
    maxWorkers: 1,
    coverage: {
      provider: process.env.FIXTURE_COVERAGE_PROVIDER || 'istanbul',
      include: ['packages/ui/src/**/*.tsx', 'packages/ui/coverage-gaps/*.tsx', 'packages/core/src/**/*.ts'],
    },
  },
});
