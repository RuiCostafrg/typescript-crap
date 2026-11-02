// @swc/jest with inline source maps so coverage remaps to TypeScript.
module.exports = {
  // service.test.ts checks this against FIXTURE_TRANSFORMER to prove this config ran.
  globals: { fixtureTransformer: '@swc/jest' },
  testEnvironment: 'node',
  testMatch: ['<rootDir>/packages/service/*.test.ts'],
  transform: { '^.+\\.tsx?$': ['@swc/jest', {
    jsc: { parser: { syntax: 'typescript' }, target: 'es2022' },
    module: { type: 'commonjs' },
    sourceMaps: 'inline',
  }] },
  collectCoverageFrom: ['packages/service/src/**/*.ts', 'packages/core/src/**/*.ts'],
};
