// ts-jest transpiles only (diagnostics off); coverageProvider comes from the command line.
module.exports = {
  testEnvironment: 'node',
  testMatch: ['<rootDir>/packages/service/*.test.ts'],
  transform: { '^.+\\.tsx?$': ['ts-jest', {
    diagnostics: false,
    tsconfig: { module: 'commonjs', target: 'es2022', esModuleInterop: true },
  }] },
  collectCoverageFrom: ['packages/service/src/**/*.ts', 'packages/core/src/**/*.ts'],
};
